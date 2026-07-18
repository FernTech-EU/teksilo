// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SettingsFile<T>` — typed single-struct persistence.
//!
//! Used when the persisted shape is a known struct (recents, window
//! state) rather than a dynamic K/V map. The current value lives in a
//! `RefCell<T>` inside an `Rc<>`-shared inner so that multiple handles
//! can observe and mutate the same projection.
//!
//! ## Cross-process safety is the only mode
//!
//! Every read and every write goes through the exclusive advisory lock on
//! `<path>.lock` (see [`crate::lock`]):
//!
//! * [`load`](SettingsFile::load) acquires the lock, reads + migrates the file
//!   fresh, and retains the [`Migrator`] for the handle's whole lifetime —
//!   not just for this one read — because [`mutate`](SettingsFile::mutate),
//!   [`replace`](SettingsFile::replace),
//!   [`reload_if_stale`](SettingsFile::reload_if_stale)
//!   and [`reload_from_disk`](crate::Reloadable::reload_from_disk) all need
//!   to re-migrate a peer's still-older on-disk schema on demand, not just
//!   once at construction.
//! * [`mutate`](SettingsFile::mutate) and [`replace`](SettingsFile::replace) perform a
//!   **locked read-modify-write**: acquire the lock, re-read + re-migrate
//!   the file from disk *under the lock*, apply the caller's change to that
//!   fresh value, write it back atomically, refresh the in-memory snapshot,
//!   then release the lock. A lock alone would only stop the two writes
//!   from interleaving on disk — it does nothing to stop a stale in-memory
//!   snapshot from clobbering a peer's newer data, so the re-read has to
//!   happen *under* the same lock that guards the write. These writes are
//!   synchronous, on the calling thread, bypassing the shared debounced I/O
//!   worker entirely — deliberately: `SettingsFile<T>` is for **rare**
//!   writes (a settings change, one record per backup), so there is no
//!   burst to coalesce. Contrast [`crate::SettingsStore`] and
//!   [`crate::PersistedListModel`], which write far more often and keep
//!   the debounce.
//! * [`reload_if_stale`](SettingsFile::reload_if_stale) and
//!   [`reload_from_disk`](crate::Reloadable::reload_from_disk) are how
//!   *reads* pick up a peer's change — a cheap mtime/len check, escalating
//!   to a full re-read only when something actually moved.
//!
//! ```ignore
//! use bastyde_settings::{SettingsFile, Migrator, Versioned};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize, Debug, Default, Clone)]
//! struct AppPrefs { version: u32, font_size: f32 }
//! impl Versioned for AppPrefs {
//!     const CURRENT_VERSION: u32 = 1;
//!     fn version(&self) -> u32 { self.version }
//!     fn set_version(&mut self, v: u32) { self.version = v; }
//! }
//!
//! let path = dirs::config_dir().unwrap().join("myapp/prefs.toml");
//! let file: SettingsFile<AppPrefs> =
//!     SettingsFile::load(path, Migrator::new()).unwrap();
//!
//! file.mutate(|p| p.font_size = 16.0).unwrap();
//! ```

use std::cell::{Cell, Ref, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::flush::{FlushError, write_atomic};
use crate::lock::FileLock;
use crate::migration::{MigrationError, Migrator, Versioned};
use crate::reload::Reloadable;

/// Number of times a locked read (initial load, a locked read-modify-write, or
/// [`SettingsFile::reload_if_stale`] / [`Reloadable::reload_from_disk`]) will
/// retry a TOML parse failure before surfacing it. Atomic rename means a
/// well-behaved peer should never hand us a torn write, but we retry briefly
/// rather than treat a transient failure as fatal — and, critically, rather
/// than quarantine the file, which could destroy a peer's legitimate data.
pub(crate) const MAX_READ_ATTEMPTS: u32 = 5;
/// Delay between retries in [`MAX_READ_ATTEMPTS`].
pub(crate) const READ_RETRY_DELAY: Duration = Duration::from_millis(5);

/// Errors surfaced by [`SettingsFile`] operations (and, by extension, every
/// other persisted type in this crate — they all share this error type).
#[derive(Debug, thiserror::Error)]
pub enum SettingsFileError {
    /// An OS-level file I/O error (read, write, or rename).
    #[error("settings file I/O: {0}")]
    Io(#[from] io::Error),
    /// The file's TOML could not be parsed.
    #[error("settings file parse: {0}")]
    Parse(#[source] toml::de::Error),
    /// A migration step failed; the file version could not be brought
    /// up to `T::CURRENT_VERSION`.
    #[error("settings file migration: {0}")]
    Migrate(#[source] MigrationError),
    /// The in-memory value could not be serialized to TOML before writing.
    #[error("settings file serialize: {0}")]
    Serialize(#[source] toml::ser::Error),
    /// The debounced background write failed.
    #[error("settings file flush: {0}")]
    Flush(#[source] FlushError),
}

struct Inner<T: Versioned + DeserializeOwned> {
    current: RefCell<T>,
    /// The file this handle reads from and writes to. Every write here
    /// (`mutate`/`replace`) is a synchronous locked read-modify-write on
    /// the calling thread — this type never registers with the shared
    /// debounced-write worker pool at all, so there is nothing to keep
    /// uniform with `SettingsStore`/`PersistedListModel` beyond the path
    /// itself.
    path: PathBuf,
    /// The on-disk `(mtime, len)` as of the last time we read or wrote the
    /// file (via construction, a locked read-modify-write, or
    /// [`SettingsFile::reload_if_stale`] / [`Reloadable::reload_from_disk`]).
    /// `(None, None)` means "file did not exist as of our last look." Used
    /// both as the cheap staleness probe and, symmetrically, as the
    /// self-write-suppression stamp a file watcher's `reload_from_disk`
    /// call relies on (see `reload.rs`'s module docs).
    last_known_stamp: Cell<(Option<SystemTime>, Option<u64>)>,
    /// Retained for the handle's whole lifetime: every locked read (not
    /// just the first one at construction) re-migrates through this, since
    /// a peer process may still be writing an older on-disk schema.
    migrator: Migrator<T>,
}

/// A reactive handle to a single typed file on disk.
///
/// `Clone` is cheap (an `Rc` bump). All clones share one in-memory
/// projection and one I/O thread.
pub struct SettingsFile<T: Versioned + DeserializeOwned> {
    inner: Rc<Inner<T>>,
}

impl<T: Versioned + DeserializeOwned> Clone for SettingsFile<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> SettingsFile<T>
where
    T: Versioned + Serialize + DeserializeOwned + Default + Clone + 'static,
{
    /// Load the file from disk (running migrations) or initialize with
    /// `T::default()` if the file does not exist.
    ///
    /// The initial read is lock-protected, exactly like every subsequent
    /// `mutate` / `replace`: a peer that is mid-write when this process
    /// starts up cannot hand us a torn read.
    ///
    /// `migrator` is taken **by value** and retained for the lifetime of
    /// the handle: every later locked read re-runs it, since a peer might
    /// still be on an older on-disk schema at any point, not just at
    /// startup.
    ///
    /// On a genuine parse failure (the bytes are not valid TOML at all,
    /// surviving `MAX_READ_ATTEMPTS` retries) the offending file is
    /// renamed to `<path>.broken-<ts>` and the returned `SettingsFile`
    /// starts from `T::default()` — the file really is corrupt, and the
    /// quarantine lets the next launch start clean instead of repeatedly
    /// failing to load it.
    ///
    /// A [`SettingsFileError::Migrate`] or [`SettingsFileError::Io`]
    /// failure, by contrast, is **not** quarantined:
    ///
    /// * `Migrate` means the TOML parsed fine, but this build's own
    ///   [`Migrator`] chain doesn't know how to bring it up to
    ///   `T::CURRENT_VERSION` — the classic symptom of an *older* build
    ///   opening a file a *newer* peer process already wrote in a newer
    ///   schema. The file is not corrupt; renaming it would destroy that
    ///   peer's live, legitimate, still-in-use data.
    /// * `Io` means we couldn't even read the file (permissions, a
    ///   transient failure) — we never saw its content, so there is no
    ///   basis at all for deciding it's corrupt, and renaming (itself
    ///   another I/O operation, on a path we just failed to read) would
    ///   be reckless.
    ///
    /// In both of those cases the handle falls back to `T::default()` for
    /// this session only, but the file on disk is left completely
    /// untouched. Use [`load_strict`](Self::load_strict) in tests that
    /// want to assert on the specific failure instead.
    pub fn load(path: PathBuf, migrator: Migrator<T>) -> Result<Self, SettingsFileError> {
        let lock = FileLock::acquire_exclusive(&path).map_err(SettingsFileError::Io)?;
        let initial = match Self::read_or_default(&path, &migrator) {
            Ok(value) => value,
            Err(SettingsFileError::Migrate(e)) => {
                // Not corruption: a peer on a newer schema. Leave the file
                // alone so that peer's data survives; fall back to
                // in-memory defaults for this session only.
                eprintln!(
                    "bastyde-settings: {} is on a schema this build cannot migrate ({}); using in-memory defaults for this session, file left untouched",
                    path.display(),
                    e,
                );
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                v
            }
            Err(SettingsFileError::Io(e)) => {
                // We never even read the content, so we have no basis to
                // judge it corrupt. Fall back to in-memory defaults for
                // this session only.
                eprintln!(
                    "bastyde-settings: could not read {} ({}); using in-memory defaults for this session, file left untouched",
                    path.display(),
                    e,
                );
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                v
            }
            Err(other) => {
                // A genuinely unparsable-after-retries document: real
                // corruption. Quarantine it so the next launch starts
                // clean instead of repeatedly failing.
                quarantine(&path);
                eprintln!(
                    "bastyde-settings: load failed for {}: {}; quarantined, falling back to defaults",
                    path.display(),
                    other,
                );
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                v
            }
        };
        let stamp = disk_stamp(&path);
        drop(lock);
        Ok(Self::new_inner(path, initial, stamp, migrator))
    }

    /// Like [`load`](Self::load), but returns parse / migration errors
    /// instead of quarantining the file. Intended for tests that want
    /// to assert on a specific failure mode.
    pub fn load_strict(path: PathBuf, migrator: Migrator<T>) -> Result<Self, SettingsFileError> {
        let lock = FileLock::acquire_exclusive(&path).map_err(SettingsFileError::Io)?;
        let initial = Self::read_or_default(&path, &migrator)?;
        let stamp = disk_stamp(&path);
        drop(lock);
        Ok(Self::new_inner(path, initial, stamp, migrator))
    }

    fn new_inner(
        path: PathBuf,
        initial: T,
        stamp: (Option<SystemTime>, Option<u64>),
        migrator: Migrator<T>,
    ) -> Self {
        // Writes never go through the shared debounced-write worker pool
        // (see the module docs): every `mutate` / `replace` is a
        // synchronous locked read-modify-write on the calling thread. This
        // type never registers with that pool at all — it just remembers
        // its own `path` directly.
        Self {
            inner: Rc::new(Inner {
                current: RefCell::new(initial),
                path,
                last_known_stamp: Cell::new(stamp),
                migrator,
            }),
        }
    }

    /// [`read_toml_with_retry`], then run `migrator`, stamping the current
    /// version. A missing file falls back to `T::default()`.
    fn read_or_default(path: &Path, migrator: &Migrator<T>) -> Result<T, SettingsFileError> {
        match read_toml_with_retry(path)? {
            Some(raw) => {
                let mut value = migrator.run(raw).map_err(SettingsFileError::Migrate)?;
                value.set_version(T::CURRENT_VERSION);
                Ok(value)
            }
            None => {
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                Ok(v)
            }
        }
    }

    /// Borrow the current value. The returned `Ref` holds a `RefCell`
    /// guard; do not call any mutating method on this `SettingsFile`
    /// while a `Ref` is alive.
    pub fn borrow(&self) -> Ref<'_, T> {
        self.inner.current.borrow()
    }

    /// Clone the current value out. Convenient when you don't want to
    /// juggle a borrow.
    pub fn snapshot(&self) -> T {
        self.inner.current.borrow().clone()
    }

    /// Replace the current value and persist it via a locked
    /// read-modify-write. The disk read is discarded — `replace` always
    /// wins over whatever was on disk — but the lock still serializes it
    /// against a concurrent peer write, and the fresh disk stamp is
    /// recorded so a subsequent reload doesn't re-read our own write back
    /// in as if it were new. `T::set_version(T::CURRENT_VERSION)` is called
    /// so the version stamp is always coherent, even if the caller forgot.
    pub fn replace(&self, new: T) -> Result<(), SettingsFileError> {
        self.locked_read_modify_write(move |v| *v = new)
    }

    /// Mutate the current value in place and persist it via a locked
    /// read-modify-write: the file is re-read and re-migrated from disk
    /// *under an exclusive lock* before `f` is applied, so `f` always sees
    /// a fresh value — not this handle's possibly-stale in-memory snapshot
    /// — and the result is written back atomically before the lock is
    /// released.
    ///
    /// Takes `f` as `FnOnce` (not `Fn`) and imposes no `Send` bound on `T`:
    /// this write is synchronous on the calling thread, never replayed on a
    /// background worker, so there is no reason to tax every call site with
    /// a `Send`/`Fn` requirement it doesn't need.
    pub fn mutate<F: FnOnce(&mut T)>(&self, f: F) -> Result<(), SettingsFileError> {
        self.locked_read_modify_write(f)
    }

    /// The locked read-modify-write behind `mutate`/`replace`.
    ///
    /// Acquires the exclusive advisory lock, re-reads + re-migrates the
    /// document fresh from disk (falling back to `T::default()` if it
    /// is absent, exactly like `load`), applies `f` to that fresh value,
    /// writes the result back atomically, then refreshes this handle's
    /// in-memory `current` and stamp baseline — all before releasing
    /// the lock.
    fn locked_read_modify_write<F: FnOnce(&mut T)>(&self, f: F) -> Result<(), SettingsFileError> {
        let path = self.inner.path.clone();
        let lock = FileLock::acquire_exclusive(&path).map_err(SettingsFileError::Io)?;

        let mut fresh = Self::read_or_default(&path, &self.inner.migrator)?;
        f(&mut fresh);
        fresh.set_version(T::CURRENT_VERSION);
        let serialized = toml::to_string_pretty(&fresh).map_err(SettingsFileError::Serialize)?;
        write_atomic(&path, &serialized).map_err(SettingsFileError::Io)?;

        let new_stamp = disk_stamp(&path);
        *self.inner.current.borrow_mut() = fresh;
        self.inner.last_known_stamp.set(new_stamp);

        drop(lock);
        Ok(())
    }

    /// Pick up a peer's change: if the on-disk `(mtime, len)` differs from
    /// the last one this handle observed, re-read and re-migrate the file
    /// and refresh `current`. Returns whether a reload happened.
    ///
    /// This is the cheap public probe — a `stat`, safe to call
    /// speculatively (e.g. on every focus-in, or on a timer). It does not
    /// perform the content-equality backstop that
    /// [`Reloadable::reload_from_disk`] adds on top (which additionally
    /// requires `T: PartialEq`); use that when a value-level "did anything
    /// actually change" guarantee is needed (e.g. driven by a file
    /// watcher, where a coincident stamp match must never be relied on
    /// alone).
    pub fn reload_if_stale(&self) -> Result<bool, SettingsFileError> {
        let path = self.inner.path.as_path();
        let current_stamp = disk_stamp(path);
        if current_stamp == self.inner.last_known_stamp.get() {
            return Ok(false);
        }

        let value = Self::read_or_default(path, &self.inner.migrator)?;
        *self.inner.current.borrow_mut() = value;
        self.inner.last_known_stamp.set(current_stamp);
        Ok(true)
    }

    /// Synchronously write any pending payload to disk. A genuine no-op:
    /// `mutate` / `replace` already write synchronously on the calling
    /// thread, so nothing is ever pending — this type never registers
    /// with the shared debounced-write worker pool at all, so there is
    /// nothing to flush and nothing that can fail. Kept so callers that
    /// hold a `SettingsFile` alongside debounced types (`SettingsStore`,
    /// `PersistedListModel`) can flush everything uniformly without
    /// special-casing this type.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        Ok(())
    }

    /// The path being written to.
    pub fn path(&self) -> &Path {
        self.inner.path.as_path()
    }
}

/// The content-equality backstop on top of [`SettingsFile::reload_if_stale`]
/// — see `reload.rs`'s module docs for the two-layer contract. Requires
/// `T: PartialEq` (only for this impl block; every other `SettingsFile`
/// method is unaffected), since this is the only place that needs to ask
/// "is the freshly-read value actually different from what's live."
impl<T> Reloadable for SettingsFile<T>
where
    T: Versioned + Serialize + DeserializeOwned + Default + Clone + PartialEq + 'static,
{
    fn path(&self) -> &Path {
        SettingsFile::path(self)
    }

    fn reload_from_disk(&self) -> Result<bool, SettingsFileError> {
        let path = self.inner.path.as_path();
        let current_stamp = disk_stamp(path);
        if current_stamp == self.inner.last_known_stamp.get() {
            // Cheap check: this is almost always our own last write
            // being noticed by a watcher. Nothing touched.
            return Ok(false);
        }

        let value = Self::read_or_default(path, &self.inner.migrator)?;
        self.inner.last_known_stamp.set(current_stamp);

        if *self.inner.current.borrow() == value {
            // Backstop: the stamp moved (e.g. a peer wrote back
            // byte-identical content, or the filesystem's mtime
            // resolution coincided with an unrelated change) but the
            // value itself is unchanged. Touch nothing.
            return Ok(false);
        }

        *self.inner.current.borrow_mut() = value;
        Ok(true)
    }
}

impl<T: Versioned + DeserializeOwned + std::fmt::Debug> std::fmt::Debug for SettingsFile<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsFile")
            .field("path", &self.inner.path)
            .field("current", &*self.inner.current.borrow())
            .finish()
    }
}

/// The file's `(modified time, byte length)` as of right now, or
/// `(None, None)` if it doesn't exist (or the platform can't report a
/// modification time). The pairing — not mtime alone — is deliberately
/// used everywhere in this crate as the staleness / self-write-suppression
/// stamp: some filesystems have coarse mtime resolution, and a length
/// mismatch at an unchanged mtime (or vice versa) is still a reliable
/// enough signal that *something* is different and a real comparison is
/// warranted.
pub(crate) fn disk_stamp(path: &Path) -> (Option<SystemTime>, Option<u64>) {
    match fs::metadata(path) {
        Ok(m) => (m.modified().ok(), Some(m.len())),
        Err(_) => (None, None),
    }
}

/// Read `path` as TOML, retrying a few times on parse failure (see
/// [`MAX_READ_ATTEMPTS`]'s doc comment for why: atomic rename means a
/// well-behaved peer should never hand us a torn write, but a brief retry
/// is cheap insurance). Returns `Ok(None)` if the file does not exist.
///
/// Shared by every persisted type in this crate that reads raw TOML off
/// disk before running its own migration ([`SettingsFile`],
/// [`crate::collection::list::PersistedListModel`]).
pub(crate) fn read_toml_with_retry(path: &Path) -> Result<Option<toml::Value>, SettingsFileError> {
    let mut last_parse_err = None;
    for attempt in 0..MAX_READ_ATTEMPTS {
        match fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<toml::Value>(&text) {
                Ok(v) => return Ok(Some(v)),
                Err(e) => {
                    last_parse_err = Some(e);
                    if attempt + 1 < MAX_READ_ATTEMPTS {
                        thread::sleep(READ_RETRY_DELAY);
                    }
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(SettingsFileError::Io(e)),
        }
    }
    Err(SettingsFileError::Parse(last_parse_err.unwrap()))
}

/// Rename a broken/unrecoverable settings file out of the way so the next
/// launch starts clean instead of repeatedly failing to load it. Shared by
/// every persisted type in this crate that can hit an unrecoverable parse
/// or migration error.
pub(crate) fn quarantine(path: &Path) {
    if !path.exists() {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut quarantine_path = path.to_path_buf();
    let new_name = match path.file_name() {
        Some(name) => format!("{}.broken-{ts}", name.to_string_lossy()),
        None => format!("settings.broken-{ts}"),
    };
    quarantine_path.set_file_name(new_name);
    if let Err(e) = fs::rename(path, &quarantine_path) {
        eprintln!(
            "bastyde-settings: could not quarantine {} -> {}: {}",
            path.display(),
            quarantine_path.display(),
            e,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
    struct Settings {
        version: u32,
        font_size: f32,
        theme: String,
    }
    impl Versioned for Settings {
        const CURRENT_VERSION: u32 = 1;
        fn version(&self) -> u32 {
            self.version
        }
        fn set_version(&mut self, v: u32) {
            self.version = v;
        }
    }

    #[test]
    fn load_creates_default_when_file_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        let file: SettingsFile<Settings> = SettingsFile::load(path, Migrator::new()).unwrap();
        assert_eq!(
            file.snapshot(),
            Settings {
                version: 1,
                font_size: 0.0,
                theme: String::new(),
            }
        );
    }

    #[test]
    fn replace_persists_immediately() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let file: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        file.replace(Settings {
            version: 1,
            font_size: 16.0,
            theme: "dark".into(),
        })
        .unwrap();

        // No flush_now needed: replace() is synchronous.
        let raw = fs::read_to_string(&path).unwrap();
        let again: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(again.font_size, 16.0);
        assert_eq!(again.theme, "dark");
    }

    #[test]
    fn mutate_modifies_in_place() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let file: SettingsFile<Settings> = SettingsFile::load(path, Migrator::new()).unwrap();

        file.mutate(|s| {
            s.font_size = 22.0;
            s.theme = "light".into();
        })
        .unwrap();

        assert_eq!(file.snapshot().font_size, 22.0);
    }

    #[test]
    fn corrupt_file_falls_back_to_default_and_quarantines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.toml");
        fs::write(&path, "this is = not valid TOML = at all").unwrap();

        let file: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        assert_eq!(file.snapshot().version, 1);

        // A genuine, unparsable-after-retries parse failure IS real
        // corruption: the original path must be gone and a .broken-<ts>
        // sibling must exist in its place. This proves the F2 fix did not
        // regress the legitimate-corruption case while fixing the two
        // illegitimate ones below.
        assert!(
            !path.exists(),
            "the corrupt original should have been renamed away"
        );
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let has_quarantine = entries
            .iter()
            .any(|e| e.file_name().to_string_lossy().contains(".broken-"));
        assert!(has_quarantine, "expected a .broken-<ts> file");
    }

    /// F2 regression: a file that parses fine but is on a schema version
    /// this build's `Migrator` cannot handle (e.g. written by a *newer*
    /// peer process) must NOT be quarantined — that would destroy the
    /// peer's still-live, legitimate data. Before the fix, `load` treated
    /// every `Err` from `read_or_default` (including `Migrate`) alike and
    /// renamed the file away; this test fails on the old code (the
    /// original path would no longer exist) and passes on the fix (the
    /// file survives byte-for-byte, and the handle falls back to
    /// `T::default()` in memory only, for this session).
    #[test]
    fn migration_failure_does_not_quarantine_a_peers_newer_schema() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("newer_schema.toml");
        // `Settings::CURRENT_VERSION` is 1; a `version = 99` document
        // looks like it came from a much newer build. An empty
        // `Migrator` has no idea how to bring version 99 down to 1 (it
        // only walks forward), so `Migrator::run` reports
        // `MigrationError::NewerThanCurrent`.
        let original_contents = "version = 99\nfont_size = 12.0\ntheme = \"from-the-future\"\n";
        fs::write(&path, original_contents).unwrap();

        let file: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        // The handle falls back to in-memory defaults for this session
        // (stamped to `CURRENT_VERSION`, exactly like every other
        // fallback-to-default path in `load`)...
        assert_eq!(
            file.snapshot(),
            Settings {
                version: Settings::CURRENT_VERSION,
                ..Settings::default()
            }
        );

        // ...but the file on disk must be completely untouched: same
        // path, same bytes, no .broken-<ts> sibling anywhere.
        assert!(path.exists(), "the peer's file must not be renamed away");
        let on_disk = fs::read_to_string(&path).unwrap();
        assert_eq!(
            on_disk, original_contents,
            "the peer's file must be byte-identical before and after load()"
        );
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !entries
                .iter()
                .any(|e| e.file_name().to_string_lossy().contains(".broken-")),
            "a migration failure must never produce a quarantine sibling"
        );
    }

    /// F2 regression: an I/O error means we never even read the file's
    /// content, so there is no basis at all for judging it corrupt.
    /// Before the fix, `load` quarantined (attempted to rename) the file
    /// on any `Err`, including a plain I/O failure. Skipped when running
    /// as root, since root ignores the read-permission bit and the setup
    /// wouldn't actually reproduce an I/O error.
    #[test]
    #[cfg(unix)]
    fn io_error_does_not_quarantine() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("unreadable.toml");
        fs::write(&path, "version = 1\nfont_size = 1.0\ntheme = \"x\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

        // If we can still read it (e.g. running as root in CI), this
        // setup doesn't reproduce the bug's precondition; skip rather
        // than assert something meaningless.
        if fs::read_to_string(&path).is_ok() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let file: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        assert_eq!(
            file.snapshot(),
            Settings {
                version: Settings::CURRENT_VERSION,
                ..Settings::default()
            }
        );

        // Restore permissions so tempdir cleanup can remove the file.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(path.exists(), "an unreadable file must not be renamed away");
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !entries
                .iter()
                .any(|e| e.file_name().to_string_lossy().contains(".broken-")),
            "an I/O error must never produce a quarantine sibling"
        );
    }

    #[test]
    fn load_strict_propagates_parse_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.toml");
        fs::write(&path, "= = =").unwrap();

        let result: Result<SettingsFile<Settings>, _> =
            SettingsFile::load_strict(path, Migrator::new());
        assert!(matches!(result, Err(SettingsFileError::Parse(_))));
    }

    #[test]
    fn clones_share_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let a: SettingsFile<Settings> = SettingsFile::load(path, Migrator::new()).unwrap();
        let b = a.clone();

        a.mutate(|s| s.font_size = 99.0).unwrap();
        assert_eq!(b.snapshot().font_size, 99.0);
    }

    /// THE HEADLINE TEST. Two independent `SettingsFile::load` handles over
    /// the *same* path — standing in for two Skribisto processes sharing
    /// `backup.toml` — each mutate a *different* field. Because the locked
    /// read-modify-write re-reads the file fresh from disk under the lock
    /// before applying each change, both changes must survive: neither
    /// handle's stale in-memory snapshot gets a chance to clobber the
    /// other's write.
    #[test]
    fn two_concurrent_handles_both_writes_survive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 42.0).unwrap();
        b.mutate(|s| s.theme = "solarized".into()).unwrap();

        // A third, fresh handle proves both writes are actually on disk
        // together, not just cached in `a`'s or `b`'s memory.
        let c: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let snapshot = c.snapshot();
        assert_eq!(snapshot.font_size, 42.0, "a's write must survive");
        assert_eq!(snapshot.theme, "solarized", "b's write must survive");

        // And what actually landed on disk (not just what a fresh load
        // produces) carries both fields too.
        let raw = fs::read_to_string(&path).unwrap();
        let on_disk: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(on_disk.font_size, 42.0);
        assert_eq!(on_disk.theme, "solarized");
    }

    /// Bug-repro, updated for the single-mode design: this exact scenario
    /// — two handles opened over the same path, each mutating a different
    /// field with no explicit flush/reload between them — used to lose
    /// data in the old dual-mode design's *default* mode (debounced
    /// schedule of a whole re-serialized snapshot, with no re-read under a
    /// lock). Now that the locked read-modify-write is the *only* mode,
    /// the same sequence must no longer clobber anything.
    #[test]
    fn two_non_synchronized_handles_no_longer_clobber_each_other() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonshared.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 42.0).unwrap();

        // Before the fix, b's in-memory snapshot still had font_size ==
        // 0.0 from its own `load` — it never re-read a's write, and its
        // debounced write re-serialized *that* stale snapshot (with b's
        // own change layered on top), overwriting a's change on disk.
        // Now, b's mutate re-reads fresh from disk under the lock first.
        b.mutate(|s| s.theme = "solarized".into()).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let on_disk: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(on_disk.theme, "solarized", "b's own write is present");
        assert_eq!(
            on_disk.font_size, 42.0,
            "a's write must survive b's later, unrelated mutate"
        );
    }

    #[test]
    fn reload_if_stale_picks_up_a_peers_write_and_reports_no_change_when_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reload.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        // Nothing has changed since both handles were constructed.
        assert!(!b.reload_if_stale().unwrap());

        a.mutate(|s| s.font_size = 7.0).unwrap();

        assert!(b.reload_if_stale().unwrap(), "b should notice a's write");
        assert_eq!(b.snapshot().font_size, 7.0);

        // Calling again with no further changes reports nothing new.
        assert!(!b.reload_if_stale().unwrap());
    }

    #[test]
    fn round_trips_versioned_migration() {
        #[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
        struct Prefs {
            version: u32,
            name: String,
            pinned: bool,
        }
        impl Versioned for Prefs {
            const CURRENT_VERSION: u32 = 2;
            fn version(&self) -> u32 {
                self.version
            }
            fn set_version(&mut self, v: u32) {
                self.version = v;
            }
        }

        let dir = tempdir().unwrap();
        let path = dir.path().join("migrated.toml");
        // A legacy v1 file, missing the v2 `pinned` field.
        fs::write(&path, "version = 1\nname = \"legacy\"\n").unwrap();

        let migrator: Migrator<Prefs> = Migrator::new().step(1, |mut v| {
            if let Some(t) = v.as_table_mut() {
                t.insert("pinned".into(), toml::Value::Boolean(true));
            }
            Ok(v)
        });

        let file: SettingsFile<Prefs> = SettingsFile::load(path.clone(), migrator).unwrap();
        let snapshot = file.snapshot();
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.name, "legacy");
        assert!(snapshot.pinned);

        // A subsequent locked mutate must also preserve the migrated
        // shape (not just the initial load).
        file.mutate(|p| p.name = "renamed".into()).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let on_disk: Prefs = toml::from_str(&raw).unwrap();
        assert_eq!(on_disk.version, 2);
        assert_eq!(on_disk.name, "renamed");
        assert!(on_disk.pinned);
    }

    #[test]
    fn replace_also_uses_the_locked_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("replace.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 5.0).unwrap();
        b.replace(Settings {
            version: 1,
            font_size: 9.0,
            theme: "replaced".into(),
        })
        .unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let on_disk: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(on_disk.font_size, 9.0);
        assert_eq!(on_disk.theme, "replaced");
    }

    #[test]
    fn flush_now_is_a_harmless_no_op() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("flush.toml");
        let file: SettingsFile<Settings> = SettingsFile::load(path, Migrator::new()).unwrap();

        file.mutate(|s| s.font_size = 1.0).unwrap();
        // Already durably written by `mutate`'s synchronous locked
        // write; `flush_now` must not error even though nothing is ever
        // pending — this type never registers with the shared
        // debounced-write worker pool at all any more.
        file.flush_now().unwrap();
    }

    /// F13 regression: `flush_now` must be a genuine, unconditional no-op
    /// — never touching the shared debounced-write worker pool at all —
    /// even while a *different* `SettingsFile` handle pointed at the same
    /// path is concurrently `mutate`-ing (i.e. holding the file lock).
    /// Before the fix, `flush_now` forwarded to a real
    /// `DebouncedWriter::flush_now`, which round-trips through the shared
    /// worker thread; that write path is entirely gone now, so this must
    /// return `Ok(())` immediately regardless of what any other handle
    /// (or the file lock) is doing.
    #[test]
    fn flush_now_never_touches_the_shared_worker_even_under_concurrent_mutate() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("no_worker.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> = SettingsFile::load(path, Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 5.0).unwrap();

        // `b` never registered a writer with the shared pool (there is
        // none any more), so `flush_now` on `b` has nothing to wait on
        // and nothing to fail, no matter what `a` just did to the same
        // file.
        assert!(b.flush_now().is_ok());
        assert!(a.flush_now().is_ok());
    }

    /// F13 regression: constructing and dropping a `SettingsFile` must be
    /// cheap. Before the fix, `new_inner` registered a `DebouncedWriter`
    /// with the shared worker pool, whose `Drop` blocks on a synchronous
    /// ack round-trip through that thread — for a queue that was always
    /// empty here. A tight loop of construct/drop would pay that
    /// round-trip cost 1000 times. This is a coarse regression guard: a
    /// generous wall-clock bound that the old behavior could plausibly
    /// blow (thread-pool ack round-trips are not free) and the fix
    /// trivially satisfies (no worker registration happens at all).
    #[test]
    fn construct_and_drop_is_cheap_in_a_tight_loop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("drop_timing.toml");

        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let file: SettingsFile<Settings> =
                SettingsFile::load(path.clone(), Migrator::new()).unwrap();
            drop(file);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "1000 construct/drop cycles took {elapsed:?}; \
             this type must never register with the shared worker pool"
        );
    }

    // -----------------------------------------------------------------
    // Reloadable
    // -----------------------------------------------------------------

    #[test]
    fn reload_from_disk_picks_up_a_peers_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reloadable.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        a.mutate(|s| s.theme = "peer-write".into()).unwrap();

        assert!(Reloadable::reload_from_disk(&b).unwrap());
        assert_eq!(b.snapshot().theme, "peer-write");
    }

    #[test]
    fn reload_from_disk_returns_false_and_touches_nothing_when_content_is_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("unchanged.toml");

        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        // Nothing changed at all: cheap stamp check short-circuits.
        assert!(!Reloadable::reload_from_disk(&a).unwrap());

        // Write byte-identical content via a second handle (same value,
        // forces a real disk write and a new stamp) — the content
        // backstop must still report no change and must not touch `a`'s
        // in-memory value between the read and the comparison.
        let b: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        b.replace(a.snapshot()).unwrap();

        assert!(!Reloadable::reload_from_disk(&a).unwrap());
    }

    #[test]
    fn reload_from_disk_self_write_suppression_no_reparse_needed_after_own_mutate() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("self_write.toml");
        let a: SettingsFile<Settings> = SettingsFile::load(path, Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 3.0).unwrap();
        // The stamp was refreshed synchronously inside `mutate`'s locked
        // write, so a watcher calling reload_from_disk right after our
        // own write sees a matching stamp and bails via the cheap check.
        assert!(!Reloadable::reload_from_disk(&a).unwrap());
        assert_eq!(a.snapshot().font_size, 3.0);
    }

    #[test]
    fn reload_from_disk_also_works_through_the_path_method() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("path.toml");
        let a: SettingsFile<Settings> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        assert_eq!(Reloadable::path(&a), path.as_path());
    }
}
