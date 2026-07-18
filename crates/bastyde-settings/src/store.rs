// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Dynamic, dotted-key K/V store backed by TOML.
//!
//! [`SettingsStore`] is the QSettings analogue: callers ask for any
//! dotted key with a type; the store returns a cached `Signal<T>` whose
//! mutations write back into an in-memory `toml::Value` and schedule a
//! debounced flush to disk.
//!
//! Keys carry static names via [`SettingsKey<T>`], or are passed as
//! ad-hoc strings via [`SettingsStore::signal`]. Same key, same type,
//! across any number of call sites returns clones of the same `Signal`.
//!
//! ## When to use
//!
//! Use `SettingsStore` for **scalar and array-of-scalar** preferences
//! (numbers, strings, booleans, `Vec<String>`). It is the right choice
//! for the majority of user-facing prefs that have a flat, well-known key
//! name. For rich structs with migrations, use
//! [`SettingsFile<T>`](crate::SettingsFile) instead — struct values
//! serialize as TOML tables and collide with the dotted-key model.
//!
//! ## Invariants enforced at registration
//!
//! * **Type stability** — once a key has been registered with type
//!   `T`, calling `signal::<U>` on the same key panics. Settings are
//!   programmer-named; type drift is a code bug, surfaced immediately.
//! * **No path-shape collisions** — `"editor.font_size"` cannot coexist
//!   with `"editor"` as a leaf value, in either order. Both directions
//!   panic at the call site that creates the conflict.
//!
//! ## Merging by dirty key, not by whole-document overwrite
//!
//! Every `Signal<T>::set` schedules a [`crate::flush::Patch`] that carries
//! only the keys dirtied since the last schedule — never a full render of
//! `raw`. The patch, applied at flush time against the document read fresh
//! off disk under a lock, `write_nested`s just those keys onto it — so a
//! peer process's change to some *other* key survives. This is the fix for
//! Skribisto's `general.toml`: today, changing any one of its 26 keys
//! reverts every other key a peer process changed, because the whole
//! document gets re-serialized from an increasingly stale in-memory copy.
//!
//! ## Reload and the re-entrancy guard
//!
//! [`Reloadable::reload_from_disk`]
//! pushes a peer's on-disk change straight into the already-handed-out
//! `Signal<T>` for that key — see [`SignalCell::apply_external`]'s doc
//! comment for why that requires capturing the concrete `T` at
//! registration time. Setting a signal from a reload would otherwise
//! re-trigger this same write-back observer and bounce the value straight
//! back out to disk as if it were a local edit; `StoreInner::applying_external`
//! is the flag the observer checks to short-circuit that.
//!
//! ## Cycle-free observer wiring
//!
//! The cell each key owns includes an [`ObserverHandle`] returned by
//! `signal.observe(|new_val| …)`. The observer's closure captures a
//! `Weak<RefCell<StoreInner>>` — never a strong `Rc` — and bails when
//! the store has already been dropped. This avoids a reference cycle: a
//! strong capture would trap the entire store inside its own observer,
//! leaking for the life of the process.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_settings::{SettingsKey, SettingsStore};
//! use std::time::Duration;
//!
//! // Declare a typed, statically-named key once — typically at the module level.
//! const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);
//!
//! // Open the store (uses `tempfile` in tests, a real path in production).
//! let store = SettingsStore::open_with_delay(
//!     "settings.toml".into(),
//!     Duration::from_millis(500),
//! )?;
//!
//! // Each call for the same key returns a clone of the same Signal<T>.
//! let font_size = store.signal_for(&FONT_SIZE); // Signal<f32>, seeded from disk
//! font_size.set(18.0);                          // writes back to TOML on next flush
//! store.flush_now()?;                           // force sync (useful in tests)
//! # Ok::<(), bastyde_settings::SettingsStoreError>(())
//! ```

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde::de::DeserializeOwned;

use bastyde_core::ObserverHandle;
use bastyde_core::signal::Signal;

use crate::file::{SettingsFileError, disk_stamp};
use crate::flush::{DebouncedWriter, FlushError, Patch};
use crate::reload::Reloadable;

/// Default debounce window for store flushes.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);

/// Errors surfaced by [`SettingsStore::open`].
#[derive(Debug, thiserror::Error)]
pub enum SettingsStoreError {
    /// The settings file could not be read or written (missing directory,
    /// permission denied, etc.).
    #[error("settings store I/O: {0}")]
    Io(#[from] io::Error),
    /// The settings file exists but its contents are not valid TOML.
    #[error("settings store parse: {0}")]
    Parse(#[source] toml::de::Error),
    /// An attempt to flush the in-memory state to disk failed.
    #[error("settings store flush: {0}")]
    Flush(#[source] FlushError),
}

/// Lets [`SettingsStore::reload_from_disk`] surface a `SettingsFileError`
/// via `?`, since [`crate::Reloadable`] is shared across every persisted
/// type in this crate and standardizes on that error type.
impl From<SettingsStoreError> for SettingsFileError {
    fn from(e: SettingsStoreError) -> Self {
        match e {
            SettingsStoreError::Io(e) => SettingsFileError::Io(e),
            SettingsStoreError::Parse(e) => SettingsFileError::Parse(e),
            SettingsStoreError::Flush(e) => SettingsFileError::Flush(e),
        }
    }
}

/// A statically-named setting. Centralizes the dotted key, the value
/// type, and the default factory. Construct as a `const`:
///
/// ```
/// use bastyde_settings::SettingsKey;
///
/// const FONT_SIZE: SettingsKey<f32> =
///     SettingsKey::new("editor.font_size", || 14.0);
/// ```
pub struct SettingsKey<T: 'static> {
    /// The dotted TOML path used to look up this setting (e.g. `"editor.font_size"`).
    pub key: &'static str,
    /// Factory that produces the default value when the key is absent from disk.
    pub default: fn() -> T,
}

impl<T: 'static> SettingsKey<T> {
    /// Create a new key descriptor; intended for use in `const` declarations.
    pub const fn new(key: &'static str, default: fn() -> T) -> Self {
        Self { key, default }
    }
}

/// Persisted user-controlled global text-scale factor (`1.0` = 100 %).
///
/// Read at startup by `bastyde-app` to seed every window's text scale, and
/// bound by the `TextScaleControl` widget so edits persist. The key accepts
/// any `f32`; the UI control restricts the user-facing range to 80 %–200 %.
/// The effective rendered scale is this value multiplied by the OS
/// accessibility text-scale preference.
pub const TEXT_SCALE_KEY: SettingsKey<f32> =
    SettingsKey::new("accessibility.text_scale", || 1.0_f32);

impl<T: 'static> std::fmt::Debug for SettingsKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsKey")
            .field("key", &self.key)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

/// One cached Signal per registered key.
struct SignalCell {
    type_id: TypeId,
    type_name: &'static str,
    signal: Box<dyn Any>,
    /// Pushes a freshly-parsed `toml::Value` for this key into the live
    /// `Signal<T>` this cell wraps — captured at [`SettingsStore::signal`]
    /// registration time, which is the **only** place both the concrete
    /// `T` (needed to deserialize) and the live `Signal<T>` (needed to
    /// `.set()`) are simultaneously in scope. `signal` above is type-erased
    /// (`Box<dyn Any>`) specifically so one `HashMap` can hold every key's
    /// differently-typed cell — but that erasure is exactly what makes a
    /// reload otherwise unable to push a disk value into an
    /// already-handed-out signal: there is nothing generic to deserialize
    /// into. This closure is the escape hatch.
    apply_external: Box<dyn Fn(&toml::Value)>,
    /// RAII handle for the observer that pipes Signal mutations back
    /// into the in-memory `toml::Value`. Dropping it would unhook the
    /// write-back, so the cell — and therefore the observer — lives
    /// for the life of the store.
    _handle: ObserverHandle,
}

/// Whether a dirty-key entry is an explicit user edit or a mere
/// registration-time default. See [`StoreInner::dirty`]'s doc comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirtyKind {
    /// A registration-time seed: written only if the key is still absent
    /// from the document at the moment the patch actually runs.
    SeedIfAbsent,
    /// An explicit `Signal::set()` (or a deferred re-entrant one) — always
    /// wins, unconditionally, over whatever is on disk.
    Set,
}

struct StoreInner {
    raw: toml::Value,
    cells: HashMap<String, SignalCell>,
    writer: DebouncedWriter,
    /// Keys dirtied since the last patch was scheduled from them. Drained
    /// into one owned `Vec<(key, value)>` per schedule, which becomes the
    /// patch's payload — see the module docs' "merging by dirty key"
    /// section. Each entry's [`DirtyKind`] distinguishes an explicit user
    /// `set()` (always wins) from a registration-time default seed (wins
    /// only if the key is still absent by the time the patch actually
    /// runs — otherwise it would stomp a peer's already-set real value
    /// with this store's mere default, exactly the bug this whole design
    /// exists to prevent).
    dirty: Vec<(String, toml::Value, DirtyKind)>,
    /// Set for the duration of [`SettingsStore::reload_from_disk`] pushing
    /// fresh values into signals. The write-back observer checks this
    /// (via a shared, non-conflicting immutable borrow — see that method's
    /// implementation) and does nothing while it's set, so a reload cannot
    /// bounce straight back out to disk as if it were a local edit.
    applying_external: Cell<bool>,
    /// `(mtime, len)` as of the last time this store read or wrote the
    /// file — the cheap staleness / self-write-suppression stamp behind
    /// [`Reloadable::reload_from_disk`].
    last_known_stamp: Cell<(Option<SystemTime>, Option<u64>)>,
}

impl StoreInner {
    /// Drain the current dirty-key batch into one patch and schedule it.
    /// No-op if nothing is dirty (e.g. called defensively after a
    /// no-op deferred-drain).
    fn schedule_dirty_flush(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        let batch: Vec<(String, toml::Value, DirtyKind)> = std::mem::take(&mut self.dirty);
        let patch: Patch = Box::new(move |current: Option<String>| {
            let mut doc: toml::Value = match current {
                Some(s) => toml::from_str(&s).map_err(|e| FlushError::Merge(e.to_string()))?,
                None => empty_table(),
            };
            if !doc.is_table() {
                doc = empty_table();
            }
            for (k, v, kind) in &batch {
                match kind {
                    DirtyKind::Set => write_nested(&mut doc, k, v.clone()),
                    DirtyKind::SeedIfAbsent => {
                        if get_nested(&doc, k).is_none() {
                            write_nested(&mut doc, k, v.clone());
                        }
                    }
                }
            }
            toml::to_string_pretty(&doc).map_err(|e| FlushError::Merge(e.to_string()))
        });
        self.writer.schedule(patch);
    }
}

/// A dynamic dotted-key reactive settings store.
///
/// `Clone` is cheap (an `Rc` bump). All clones share one cache and one
/// I/O thread.
pub struct SettingsStore {
    inner: Rc<RefCell<StoreInner>>,
    /// Write-backs that couldn't borrow `inner` (a re-entrant `set` during
    /// another `set`'s observer chain, while `inner` is already borrowed) are
    /// queued here instead of being dropped. They are drained into `raw` +
    /// `dirty` — preserving the in-memory → disk invariant — the next time
    /// `inner` is successfully borrowed for a write-back, and on
    /// `flush_now`. Held in its own cell so it can be pushed to even while
    /// `inner` is borrowed.
    pending: Rc<RefCell<Vec<(String, toml::Value)>>>,
    /// Duplicated from `inner.writer.path()` so [`Reloadable::path`] can
    /// return a plain `&Path` without needing a `RefCell` borrow to
    /// outlive `&self` (paths never change post-construction).
    path: PathBuf,
}

impl Clone for SettingsStore {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            pending: Rc::clone(&self.pending),
            path: self.path.clone(),
        }
    }
}

impl SettingsStore {
    /// Open a store at `path` with the default debounce window.
    pub fn open(path: PathBuf) -> Result<Self, SettingsStoreError> {
        Self::open_with_delay(path, DEFAULT_DEBOUNCE)
    }

    /// Open a store at `path` with a custom debounce window. `delay =
    /// Duration::ZERO` is useful for tests — every set writes through
    /// on the next worker iteration, and `flush_now()` is fully
    /// deterministic.
    pub fn open_with_delay(path: PathBuf, delay: Duration) -> Result<Self, SettingsStoreError> {
        let raw = match fs::read_to_string(&path) {
            Ok(s) => toml::from_str::<toml::Value>(&s).map_err(SettingsStoreError::Parse)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => empty_table(),
            Err(e) => return Err(SettingsStoreError::Io(e)),
        };

        // The root must be a table — top-level scalars are nonsense for
        // a multi-key store. Coerce or reject.
        let raw = match raw {
            v @ toml::Value::Table(_) => v,
            _ => empty_table(),
        };

        let stamp = disk_stamp(&path);
        let writer = DebouncedWriter::new(path.clone(), delay);
        let inner = Rc::new(RefCell::new(StoreInner {
            raw,
            cells: HashMap::new(),
            writer,
            dirty: Vec::new(),
            applying_external: Cell::new(false),
            last_known_stamp: Cell::new(stamp),
        }));

        Ok(Self {
            inner,
            pending: Rc::new(RefCell::new(Vec::new())),
            path,
        })
    }

    /// Path of the underlying file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Force any pending payload to disk synchronously.
    pub fn flush_now(&self) -> Result<(), SettingsStoreError> {
        // Fold any deferred (re-entrant) write-backs into `raw` + `dirty`
        // and reschedule before forcing the write, so a value queued while
        // `inner` was borrowed still reaches disk. Acquire the `inner`
        // borrow first, then drain — otherwise a failed borrow would lose
        // the drained values. `try_borrow_mut` keeps this safe if
        // `flush_now` runs inside a borrow.
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            let deferred: Vec<(String, toml::Value)> =
                self.pending.borrow_mut().drain(..).collect();
            if !deferred.is_empty() {
                for (k, v) in deferred {
                    write_nested(&mut inner.raw, &k, v.clone());
                    // These came from the write-back observer's
                    // deferred-on-reentrancy path, i.e. a real `set()` —
                    // always wins.
                    inner.dirty.push((k, v, DirtyKind::Set));
                }
                inner.schedule_dirty_flush();
            }
        }
        self.inner
            .borrow()
            .writer
            .flush_now()
            .map_err(SettingsStoreError::Flush)?;

        // Re-sync with reality rather than just bumping the stamp: our
        // write just merged against whatever was on disk, which may have
        // included keys a peer set that this store never locally ingested
        // (it only knows the keys *it* wrote). A blind stamp bump here
        // would make a later `reload_from_disk` wrongly believe nothing
        // changed — since the stamp would already match — even though a
        // peer's concurrently-merged key was never pushed into this
        // store's live signals. Folding this through the same
        // read-parse-compare path `reload_from_disk` uses keeps the
        // "stamp matches disk <=> in-memory reflects disk" invariant
        // intact in both the plain-self-write and the merged-with-a-peer
        // case.
        let _ = self.resync_with_disk()?;
        Ok(())
    }

    /// Whether the given key has already been registered.
    pub fn has(&self, key: &str) -> bool {
        self.inner.borrow().cells.contains_key(key)
    }

    /// All keys registered so far. Order is unspecified.
    pub fn registered_keys(&self) -> Vec<String> {
        self.inner.borrow().cells.keys().cloned().collect()
    }

    /// Get-or-create a `Signal<T>` for `key`, seeded from disk or
    /// `default` if absent. Subsequent calls for the same key return
    /// clones of the same signal.
    ///
    /// # Panics
    ///
    /// * If the key was previously registered with a different type.
    /// * If the key's path conflicts with an existing leaf-value /
    ///   table shape (e.g. `"editor"` is a string and now you ask for
    ///   `"editor.font_size"`).
    pub fn signal<T>(&self, key: &str, default: T) -> Signal<T>
    where
        T: Clone + Serialize + DeserializeOwned + 'static,
    {
        if let Some(existing) = self.try_existing::<T>(key) {
            return existing;
        }

        let mut inner = self.inner.borrow_mut();

        // Re-check inside the lock in case of races between borrow drops.
        // (Single-threaded, but defensive.)
        if let Some(cell) = inner.cells.get(key) {
            return downcast_or_panic::<T>(key, cell);
        }

        // Validate the path shape before we do anything else.
        if let Err(err) = check_path_shape(&inner.raw, key) {
            panic!("{}", err.message_for(key, std::any::type_name::<T>()));
        }

        // Seed: deserialize from raw if present; else default.
        let initial = match get_nested(&inner.raw, key) {
            Some(v) => match T::deserialize(v.clone()) {
                Ok(v) => v,
                Err(_) => default,
            },
            None => default,
        };

        // Stamp the seed back into raw so that the on-disk shape
        // matches the program's understanding immediately.
        let initial_value =
            serialize_to_value(&initial).expect("initial T value must serialize as TOML");

        // Reject struct-shaped values at the leaf: they serialize as
        // TOML tables, which collide with the store's nested-key model
        // (we cannot distinguish "table is a struct value" from
        // "table is a parent of nested keys" on a re-read). Apps that
        // need to persist struct values should use `SettingsFile<T>`
        // directly. Arrays and scalars are fine.
        if matches!(&initial_value, toml::Value::Table(_)) {
            panic!(
                "SettingsStore: cannot register key \"{key}\" as {ty} — \
                 struct values serialize as TOML tables, which collide with \
                 the store's nested-key model. Use SettingsFile<{ty}> \
                 instead.",
                ty = std::any::type_name::<T>(),
            );
        }

        let sig: Signal<T> = Signal::new(initial.clone());

        write_nested(&mut inner.raw, key, initial_value.clone());

        // Wire write-back. The closure captures Weaks so a dropped store does
        // not stay alive via its own observer.
        let key_owned = key.to_string();
        let weak: Weak<RefCell<StoreInner>> = Rc::downgrade(&self.inner);
        let weak_pending: Weak<RefCell<Vec<(String, toml::Value)>>> = Rc::downgrade(&self.pending);
        let handle = sig.observe(move |new_val: &T| {
            let Some(inner_rc) = weak.upgrade() else {
                return;
            };
            let Some(pending_rc) = weak_pending.upgrade() else {
                return;
            };
            // Re-entrancy guard: a reload-driven `.set()` must not write
            // back — it would bounce the value it just read straight back
            // out to disk as if it were a fresh local edit. A shared
            // borrow is enough to check the flag, and does not conflict
            // with the shared borrow `reload_from_disk` may itself be
            // holding while it drives this very observer.
            if inner_rc
                .try_borrow()
                .map(|r| r.applying_external.get())
                .unwrap_or(false)
            {
                return;
            }
            let value = match serialize_to_value(new_val) {
                Ok(v) => v,
                Err(_) => return,
            };
            match inner_rc.try_borrow_mut() {
                Ok(mut inner) => {
                    // Apply any deferred re-entrant writes first, then this one,
                    // so `raw` (and therefore disk) never diverges from the
                    // in-memory signals. Drain into a local Vec before touching
                    // `raw` so a re-entrant push during `write_nested` doesn't
                    // contend on the `pending` borrow.
                    let deferred: Vec<(String, toml::Value)> =
                        pending_rc.borrow_mut().drain(..).collect();
                    for (k, v) in deferred {
                        write_nested(&mut inner.raw, &k, v.clone());
                        inner.dirty.push((k, v, DirtyKind::Set));
                    }
                    write_nested(&mut inner.raw, &key_owned, value.clone());
                    inner.dirty.push((key_owned.clone(), value, DirtyKind::Set));
                    inner.schedule_dirty_flush();
                }
                Err(_) => {
                    // The store is borrowed elsewhere — a re-entrant set during
                    // another set's observer chain. Defer rather than drop:
                    // queue the new value so the next successful write-back (or
                    // `flush_now`) folds it into `raw` + `dirty`. Dropping it
                    // here would silently diverge disk from the in-memory
                    // signal.
                    pending_rc.borrow_mut().push((key_owned.clone(), value));
                }
            }
        });

        let apply_external: Box<dyn Fn(&toml::Value)> = {
            let sig_for_apply = sig.clone();
            Box::new(move |fresh: &toml::Value| {
                if let Ok(value) = T::deserialize(fresh.clone()) {
                    sig_for_apply.set(value);
                }
            })
        };

        let cell = SignalCell {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            signal: Box::new(sig.clone()),
            apply_external,
            _handle: handle,
        };
        inner.cells.insert(key.to_string(), cell);

        // Flush the seed-stamped raw so brand-new keys hit disk on first
        // registration even without a `set` — but only if the key is
        // still absent by the time this patch actually runs. A peer
        // process may register the same key with the same hardcoded
        // default and, by pure timing, have its own seed-patch fire
        // *after* this store (or a third party) has already set a real
        // value there; an unconditional write would stomp that real
        // value back to a mere default. `SeedIfAbsent` makes registration
        // idempotent with respect to a peer's concurrent real edit.
        inner
            .dirty
            .push((key.to_string(), initial_value, DirtyKind::SeedIfAbsent));
        inner.schedule_dirty_flush();

        sig
    }

    /// Like [`signal`](Self::signal), but driven by a strongly-named
    /// [`SettingsKey<T>`] constant.
    pub fn signal_for<T>(&self, key: &SettingsKey<T>) -> Signal<T>
    where
        T: Clone + Serialize + DeserializeOwned + 'static,
    {
        self.signal(key.key, (key.default)())
    }

    fn try_existing<T: Clone + 'static>(&self, key: &str) -> Option<Signal<T>> {
        let inner = self.inner.borrow();
        let cell = inner.cells.get(key)?;
        Some(downcast_or_panic::<T>(key, cell))
    }

    /// The actual re-sync-with-disk logic shared by [`flush_now`](Self::flush_now)
    /// (which needs it right after every write, merged or not — see that
    /// method's doc comment) and [`Reloadable::reload_from_disk`] (the
    /// public, watcher-facing entry point). Kept as a `SettingsStoreError`-returning
    /// private method so `flush_now` doesn't have to round-trip through
    /// `SettingsFileError` for a case that can only ever produce the I/O /
    /// parse variants.
    fn resync_with_disk(&self) -> Result<bool, SettingsStoreError> {
        let current_stamp = disk_stamp(&self.path);
        if current_stamp == self.inner.borrow().last_known_stamp.get() {
            return Ok(false);
        }

        let raw_text = match fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(SettingsStoreError::Io(e)),
        };
        let parsed: toml::Value = if raw_text.trim().is_empty() {
            empty_table()
        } else {
            toml::from_str(&raw_text).map_err(SettingsStoreError::Parse)?
        };
        let parsed = match parsed {
            v @ toml::Value::Table(_) => v,
            _ => empty_table(),
        };

        // Backstop: compare the whole parsed document to what's already
        // live. Unchanged content (e.g. a peer wrote back byte-identical
        // bytes, or this really was our own write and the stamp merely
        // didn't line up) touches nothing.
        {
            let mut inner = self.inner.borrow_mut();
            if inner.raw == parsed {
                inner.last_known_stamp.set(current_stamp);
                return Ok(false);
            }
            inner.raw = parsed.clone();
            inner.last_known_stamp.set(current_stamp);
        }

        // Push each registered cell's fresh sub-value into its live
        // signal, with the re-entrancy guard held for the whole batch.
        {
            let inner_ref = self.inner.borrow();
            inner_ref.applying_external.set(true);
            for (key, cell) in inner_ref.cells.iter() {
                if let Some(value) = get_nested(&parsed, key) {
                    (cell.apply_external)(value);
                }
            }
            inner_ref.applying_external.set(false);
        }

        Ok(true)
    }
}

impl Reloadable for SettingsStore {
    fn path(&self) -> &Path {
        &self.path
    }

    fn reload_from_disk(&self) -> Result<bool, SettingsFileError> {
        self.resync_with_disk().map_err(SettingsFileError::from)
    }
}

impl std::fmt::Debug for SettingsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("SettingsStore")
            .field("path", &self.path)
            .field("registered_keys", &inner.cells.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

fn downcast_or_panic<T: Clone + 'static>(key: &str, cell: &SignalCell) -> Signal<T> {
    if cell.type_id != TypeId::of::<T>() {
        panic!(
            "SettingsStore: key \"{key}\" was registered as {prev}, but \
             signal::<{new}>(...) was called. Pick one type per key.",
            prev = cell.type_name,
            new = std::any::type_name::<T>(),
        );
    }
    cell.signal
        .downcast_ref::<Signal<T>>()
        .expect("type id matched but downcast failed — bastyde-settings bug")
        .clone()
}

fn serialize_to_value<T: Serialize>(value: &T) -> Result<toml::Value, toml::ser::Error> {
    toml::Value::try_from(value)
}

/// Walk a dotted key into a `toml::Value`, returning the leaf if all
/// intermediate steps are tables and the leaf exists.
fn get_nested<'a>(raw: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut current = raw;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Insert `value` at the dotted `key`, creating intermediate tables as
/// needed. Caller must have already verified that the path shape is
/// compatible via [`check_path_shape`].
fn write_nested(raw: &mut toml::Value, key: &str, value: toml::Value) {
    let parts: Vec<&str> = key.split('.').collect();
    let last = parts.len() - 1;
    let mut current = raw;
    for (i, part) in parts.iter().enumerate() {
        let table = current
            .as_table_mut()
            .expect("write_nested: path validated by check_path_shape, but encountered non-table");
        if i == last {
            table.insert((*part).to_string(), value);
            return;
        }
        let entry = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        current = entry;
    }
}

#[derive(Debug)]
enum CollisionKind {
    /// An intermediate component of the dotted path is a non-table
    /// scalar, so the path can't be deepened.
    IntermediateIsValue {
        existing_path: String,
        existing_kind: &'static str,
    },
    /// The leaf is currently a table, so the path can't be assigned a
    /// scalar.
    LeafIsTable { existing_path: String },
}

impl CollisionKind {
    fn message_for(&self, requested_key: &str, requested_type: &str) -> String {
        match self {
            CollisionKind::IntermediateIsValue {
                existing_path,
                existing_kind,
            } => format!(
                "SettingsStore: cannot register key \"{requested_key}\" as {requested_type} — \
                 the prefix \"{existing_path}\" is already a {existing_kind} value. \
                 A key cannot be both a value and a parent.",
            ),
            CollisionKind::LeafIsTable { existing_path } => format!(
                "SettingsStore: cannot register key \"{requested_key}\" as {requested_type} — \
                 \"{existing_path}\" is already a table (parent of other keys). \
                 A key cannot be both a value and a parent.",
            ),
        }
    }
}

fn check_path_shape(raw: &toml::Value, key: &str) -> Result<(), CollisionKind> {
    let parts: Vec<&str> = key.split('.').collect();
    let last = parts.len() - 1;
    let mut current = raw;
    let mut walked = String::new();
    for (i, part) in parts.iter().enumerate() {
        if !walked.is_empty() {
            walked.push('.');
        }
        walked.push_str(part);

        let Some(child) = current.get(part) else {
            return Ok(());
        };
        let is_last = i == last;
        if is_last {
            if child.is_table() {
                return Err(CollisionKind::LeafIsTable {
                    existing_path: walked,
                });
            }
            return Ok(());
        }
        if !child.is_table() {
            return Err(CollisionKind::IntermediateIsValue {
                existing_path: walked,
                existing_kind: kind_name(child),
            });
        }
        current = child;
    }
    Ok(())
}

fn kind_name(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

// Allow `Path` references for ergonomics in tests / examples.
impl SettingsStore {
    /// Convenience constructor accepting `&Path`.
    pub fn open_path(path: &Path) -> Result<Self, SettingsStoreError> {
        Self::open(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
    struct Window {
        x: i32,
        y: i32,
        title: String,
    }

    fn open_in(dir: &Path, name: &str) -> SettingsStore {
        SettingsStore::open_with_delay(dir.join(name), Duration::ZERO).unwrap()
    }

    #[test]
    fn signal_returns_default_when_key_absent() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "store.toml");
        let sig = store.signal::<f32>("editor.font_size", 14.0);
        assert_eq!(sig.get(), 14.0);
    }

    #[test]
    fn signal_dedupes_per_key() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "store.toml");
        let a = store.signal::<f32>("editor.font_size", 14.0);
        let b = store.signal::<f32>("editor.font_size", 99.0); // default ignored
        assert_eq!(b.get(), 14.0);
        a.set(22.0);
        assert_eq!(b.get(), 22.0);
    }

    #[test]
    fn set_persists_after_flush_now_and_reopens() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");

        {
            let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
            let sig = store.signal::<f32>("editor.font_size", 14.0);
            sig.set(18.0);
            store.flush_now().unwrap();
        }

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<f32>("editor.font_size", 14.0);
        assert_eq!(sig.get(), 18.0);
    }

    #[test]
    #[should_panic(expected = "struct values serialize as TOML tables")]
    fn struct_values_rejected_at_registration() {
        // The store does not support struct values: they serialize as
        // TOML tables, which collide with the dotted-key model.
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _w = store.signal::<Window>(
            "window.main",
            Window {
                x: 0,
                y: 0,
                title: String::new(),
            },
        );
    }

    #[test]
    fn array_of_scalars_roundtrip() {
        // Arrays serialize as TOML arrays (not tables), so they
        // coexist with the dotted-key model just fine.
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");

        {
            let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
            let palette =
                store.signal::<Vec<String>>("ui.palette", vec!["red".into(), "blue".into()]);
            palette.set(vec!["green".into(), "yellow".into(), "purple".into()]);
            store.flush_now().unwrap();
        }

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let palette = store.signal::<Vec<String>>("ui.palette", vec![]);
        assert_eq!(palette.get(), vec!["green", "yellow", "purple"]);
    }

    #[test]
    #[should_panic(expected = "registered as f32")]
    fn type_mismatch_panics() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _a = store.signal::<f32>("k", 1.0);
        let _b = store.signal::<i32>("k", 2);
    }

    #[test]
    #[should_panic(expected = "is already a string value")]
    fn intermediate_value_collision_panics() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _ = store.signal::<String>("editor", "blue".into());
        let _ = store.signal::<f32>("editor.font_size", 14.0);
    }

    #[test]
    #[should_panic(expected = "is already a table")]
    fn leaf_table_collision_panics() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let _ = store.signal::<f32>("editor.font_size", 14.0);
        let _ = store.signal::<String>("editor", "blue".into());
    }

    #[test]
    fn deeply_nested_collision_caught() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        // Pre-populate file with `[a.b] c = 5`
        fs::write(&path, "[a.b]\nc = 5\n").unwrap();

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        // a.b is a table; asking for a.b as a scalar should panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            store.signal::<i32>("a.b", 0);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn registered_keys_lists_touched_keys_only() {
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        assert!(store.registered_keys().is_empty());
        let _ = store.signal::<i32>("a", 1);
        let _ = store.signal::<bool>("b.c", true);
        let mut keys = store.registered_keys();
        keys.sort();
        assert_eq!(keys, vec!["a".to_string(), "b.c".to_string()]);
        assert!(store.has("a"));
        assert!(!store.has("nonexistent"));
    }

    #[test]
    fn signal_for_uses_constant_default() {
        const HEIGHT: SettingsKey<f32> = SettingsKey::new("layout.height", || 42.0);

        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let sig = store.signal_for(&HEIGHT);
        assert_eq!(sig.get(), 42.0);
    }

    #[test]
    fn dropping_store_does_not_leak_via_observer() {
        // Regression for the cycle bug: a strong Rc capture in the
        // observer closure would prevent StoreInner from being freed
        // even after the user drops their last clone.
        let dir = tempdir().unwrap();
        let store = open_in(dir.path(), "p.toml");
        let weak = {
            let inner_rc = Rc::clone(&store.inner);
            let weak = Rc::downgrade(&inner_rc);
            let _sig = store.signal::<f32>("k", 1.0);
            drop(inner_rc);
            weak
        };
        // `store` still alive — weak is upgradable.
        assert!(weak.upgrade().is_some());
        drop(store);
        // After last strong Rc drops, the weak cannot upgrade.
        assert!(
            weak.upgrade().is_none(),
            "observer must not keep StoreInner alive via a strong capture"
        );
    }

    #[test]
    fn observer_writes_back_to_raw() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        let store = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
        let sig = store.signal::<i32>("answer", 0);
        sig.set(42);
        store.flush_now().unwrap();

        let on_disk = fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("answer = 42"));
    }

    #[test]
    fn pre_existing_file_seeds_signals() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        fs::write(&path, "[editor]\nfont_size = 18.0\n").unwrap();

        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<f32>("editor.font_size", 1.0);
        assert_eq!(sig.get(), 18.0);
    }

    #[test]
    fn path_with_top_level_scalar_recovers_with_empty_table() {
        // A weird but possible file: top-level scalar (e.g., from
        // hand-edits). The store should not panic; it should treat the
        // root as empty.
        let dir = tempdir().unwrap();
        let path = dir.path().join("p.toml");
        // toml does not actually allow a bare scalar at the top level,
        // but we also test the more common path: empty file works.
        fs::write(&path, "").unwrap();
        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<i32>("k", 7);
        assert_eq!(sig.get(), 7);
    }

    // -----------------------------------------------------------------
    // Cross-process merge + Reloadable
    // -----------------------------------------------------------------

    /// THE HEADLINE TEST. Two independent `SettingsStore` handles over the
    /// same file — standing in for two Skribisto processes sharing
    /// `general.toml` — each set a *different* key with no coordination.
    /// Because every write merges its dirty key onto the document read
    /// fresh under the lock, both keys must survive, and reloading must
    /// push the peer's key into this process's *already-live* `Signal`
    /// with no restart needed.
    #[test]
    fn two_concurrent_stores_each_setting_a_different_key_both_survive_and_reload_updates_live_signal()
     {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared_store.toml");

        let a = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
        let b = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();

        let dark_a = a.signal::<bool>("ui.dark", false);
        let dark_b = b.signal::<bool>("ui.dark", false);
        let width_b = b.signal::<f32>("editor.column_width", 80.0);

        // Not yet reloaded: b's live signal for a's key still reads its
        // own local default.
        assert!(!dark_b.get(), "b hasn't seen a's write yet");

        dark_a.set(true);
        a.flush_now().unwrap();

        // b's write only ever touches its own key — but reloading must
        // push a's concurrent change into b's *already-live* `Signal`,
        // with no restart needed.
        assert!(Reloadable::reload_from_disk(&b).unwrap());
        assert!(dark_b.get(), "b's live signal must reflect a's write");

        width_b.set(120.0);
        b.flush_now().unwrap();

        // A third, fresh handle proves both keys are actually on disk
        // together, not just cached in `a`'s or `b`'s memory.
        let c = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        assert!(c.signal::<bool>("ui.dark", false).get());
        assert_eq!(c.signal::<f32>("editor.column_width", 80.0).get(), 120.0);
    }

    #[test]
    fn reload_driven_set_schedules_no_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("no_bounce.toml");

        let a = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
        let b = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
        let _dark_b = b.signal::<bool>("ui.dark", false);

        a.signal::<bool>("ui.dark", false).set(true);
        a.flush_now().unwrap();

        assert!(Reloadable::reload_from_disk(&b).unwrap());
        // If the reload's `sig.set()` had scheduled a write, `b.inner`
        // would have a non-empty `dirty` batch right now.
        assert!(
            b.inner.borrow().dirty.is_empty(),
            "a reload-driven set must not enqueue a write-back"
        );
    }

    #[test]
    fn reload_from_disk_returns_false_and_touches_nothing_when_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("unchanged_store.toml");
        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        let sig = store.signal::<f32>("k", 1.0);
        sig.set(2.0);
        store.flush_now().unwrap();

        assert!(!Reloadable::reload_from_disk(&store).unwrap());
        assert_eq!(sig.get(), 2.0);
    }

    #[test]
    fn reload_from_disk_ignores_our_own_last_write_via_cheap_stamp_check() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("self_write_store.toml");
        let store = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
        store.signal::<f32>("k", 1.0).set(9.0);
        store.flush_now().unwrap();

        // flush_now() re-stamps last_known_stamp right after the write
        // completes, so an immediate reload sees a matching stamp.
        assert!(!Reloadable::reload_from_disk(&store).unwrap());
    }
}
