// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live cross-process settings sync: a `notify`-based directory watcher
//! plus the registry that lets a changed path be dispatched to the
//! in-memory [`Reloadable`] handle that owns it.
//!
//! This is the read-side counterpart to the write-side cross-process
//! safety documented in `flush.rs` / `reload.rs`: every write in this
//! crate already merges safely against a peer's concurrent write, but a
//! process that never looks again will not notice a peer's write until
//! it happens to touch the same key itself. [`SettingsWatcher`] is what
//! makes it look again, automatically, the moment a peer's write lands
//! on disk.
//!
//! ## Shape, mirrored from `teksilo-i18n`'s `FtlFileWatcher`
//!
//! [`SettingsWatcher`] owns a `notify::RecommendedWatcher` background
//! thread and a type-erased sink `Arc<dyn Fn(PathBuf) + Send + Sync>`.
//! Exactly like `FtlFileWatcher`, it watches **directories**, not files:
//! atomic writers (this crate's own `write_atomic` included) write a
//! temp file and rename it over the target, which invalidates an
//! inode-level watch on the file itself. Unlike `FtlFileWatcher` — which
//! watches a fixed, already-existing set of `.ftl` files and derives
//! their parents — `SettingsWatcher` watches the settings *directories*
//! (`AppPaths::config_dir()` / `AppPaths::data_dir()`) directly, because
//! the set of settings files living there is open-ended and some of
//! them (e.g. `window_state.toml`) may not exist yet at watch-construction
//! time.
//!
//! The sink receives the changed path (not yet filtered against anything
//! this process cares about); [`SettingsRegistry::dispatch`] is what
//! decides whether the path names something registered and, if so,
//! calls its [`Reloadable::reload_from_disk`]. A path with no registered
//! owner (a `.lock` sidecar, a `.tmp` write-in-progress, an unrelated
//! file a peer dropped in the same directory) is a harmless no-op.
//!
//! ## The registry
//!
//! [`SettingsRegistry`] maps a canonical path to a `Weak<dyn Reloadable>`.
//! It never holds a strong reference itself: whoever opens a persisted
//! service (`SettingsBundle::open`, or application code opening its own
//! ad hoc `SettingsFile<T>` / `PersistedListModel<T>` / `MruList<T>`)
//! wraps it in an `Rc<dyn Reloadable>`, registers a weak clone via
//! [`SettingsRegistry::register`], and keeps the returned `Rc` alive for
//! as long as it wants peer writes to be picked up. When that `Rc` (and
//! every clone of it) is dropped, the registry's entry can no longer be
//! upgraded — [`SettingsRegistry::dispatch`] then quietly prunes it and
//! reports nothing happened. Nothing leaks and nothing is ever called on
//! a service that no longer exists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::file::SettingsFileError;
use crate::reload::Reloadable;

/// Sink type invoked on the notify worker thread whenever a watched
/// settings directory reports a create/modify event. Implementations
/// must be thread-safe; `teksilo-app`'s implementation posts the path
/// through the winit `EventLoopProxy` as `AppEvent::SettingsReload`,
/// which hops back onto the UI thread where the (single-threaded,
/// `Rc`-based) [`SettingsRegistry`] actually lives.
pub type SettingsReloadSink = Arc<dyn Fn(PathBuf) + Send + Sync + 'static>;

/// Active directory watcher over one or more settings directories. One
/// per `TeksiloAppBuilder::run` invocation (when a settings bundle with
/// watching enabled is configured).
///
/// Owns the `notify::RecommendedWatcher` background thread for its whole
/// lifetime; dropping the `SettingsWatcher` stops the watcher and cleans
/// up. Kept alive by the caller for as long as live reload is wanted —
/// `teksilo-app` stores it on its window-loop handler, exactly like
/// `teksilo-i18n`'s `FtlFileWatcher`.
pub struct SettingsWatcher {
    _inner: RecommendedWatcher,
}

impl SettingsWatcher {
    /// Build a watcher over `dirs` (deduplicated by canonical path, so
    /// passing the same directory twice — e.g. `AppPaths::for_testing`,
    /// whose `config_dir()` and `data_dir()` are the same tempdir — never
    /// double-watches or double-fires) and a sink callback.
    ///
    /// A directory that does not exist (or can't be canonicalized for
    /// any other reason) is logged and skipped — not fatal — since a
    /// freshly-installed app may not have created its data directory yet
    /// when this is called. As long as at least the config directory
    /// exists (which `AppPaths` implies by the time `SettingsBundle` has
    /// successfully opened anything in it), watching still works for the
    /// files that matter.
    pub fn new(dirs: Vec<PathBuf>, sink: SettingsReloadSink) -> Result<Self, notify::Error> {
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut targets: Vec<PathBuf> = Vec::new();
        for dir in dirs {
            let canonical = match dir.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "teksilo-settings: cannot watch `{}` ({e}); live settings reload \
                         disabled for that directory",
                        dir.display()
                    );
                    continue;
                }
            };
            if seen.insert(canonical.clone()) {
                targets.push(canonical);
            }
        }

        let sink_handle = sink.clone();
        let mut watcher = notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) if should_reload(&event.kind) => {
                    for path in &event.paths {
                        (sink_handle)(path.clone());
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("teksilo-settings: watcher error: {e}");
                }
            },
        )?;

        for target in &targets {
            watcher.watch(target, RecursiveMode::NonRecursive)?;
        }

        Ok(Self { _inner: watcher })
    }
}

/// Return `true` for event kinds that mean a file's content may have
/// changed. `notify` fires many events for other operations (access,
/// metadata-only, permissions) that never need a reload. Mirrors
/// `teksilo-i18n::file_watcher::should_reload` exactly.
fn should_reload(kind: &notify::EventKind) -> bool {
    use notify::EventKind::*;
    matches!(kind, Modify(_) | Create(_))
}

/// Best-effort canonical form of `path`, robust to the file itself not
/// existing yet on disk (unlike [`Path::canonicalize`], which requires
/// every component — including the leaf — to exist).
///
/// Canonicalizes the *parent* directory (which every registered settings
/// path has, and which is guaranteed to exist before anything is ever
/// written into it) and rejoins the file name. This is the same key a
/// [`SettingsWatcher`] event path resolves to, since the watcher is
/// always constructed over the canonicalized parent directory too — so
/// a registration made before a file's first write and an event fired
/// after it exists still land on the same map key.
fn canonical_settings_path(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(canonical_parent) => canonical_parent.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

/// Registry mapping a canonical settings path to the live [`Reloadable`]
/// handle that owns it, so a file-watcher event naming that path can be
/// dispatched to the right in-memory state.
///
/// `Clone` is cheap (an `Rc` bump) — every clone shares the same
/// underlying map, matching the rest of this crate's handle types.
/// Holds only [`Weak`] references: see the module docs' "the registry"
/// section for the full ownership contract.
#[derive(Clone, Default)]
pub struct SettingsRegistry {
    entries: Rc<std::cell::RefCell<HashMap<PathBuf, Weak<dyn Reloadable>>>>,
}

impl SettingsRegistry {
    /// A fresh, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `reloadable` under its canonical path and return it back
    /// unchanged, so a caller can register and retain in one expression:
    ///
    /// ```
    /// use teksilo_settings::{SettingsRegistry, SettingsFile, Migrator, Versioned};
    /// use serde::{Serialize, Deserialize};
    /// use std::rc::Rc;
    ///
    /// #[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
    /// struct Prefs { version: u32 }
    /// impl Versioned for Prefs {
    ///     const CURRENT_VERSION: u32 = 1;
    ///     fn version(&self) -> u32 { self.version }
    ///     fn set_version(&mut self, v: u32) { self.version = v; }
    /// }
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let file: SettingsFile<Prefs> =
    ///     SettingsFile::load(dir.path().join("prefs.toml"), Migrator::new()).unwrap();
    ///
    /// let registry = SettingsRegistry::new();
    /// // Keep `handle` alive for as long as reload should keep working.
    /// let handle = registry.register(Rc::new(file.clone()));
    /// drop(handle); // dropping it deregisters: no leak, no dangling call.
    /// ```
    ///
    /// The caller is responsible for keeping the returned `Rc` alive —
    /// only a `Weak` is retained internally, by design (see the module
    /// docs). Registering a second `Reloadable` under the same canonical
    /// path replaces the first entry.
    pub fn register(&self, reloadable: Rc<dyn Reloadable>) -> Rc<dyn Reloadable> {
        let key = canonical_settings_path(reloadable.path());
        self.entries
            .borrow_mut()
            .insert(key, Rc::downgrade(&reloadable));
        reloadable
    }

    /// Look up `changed_path`'s registered owner and call
    /// [`Reloadable::reload_from_disk`] on it.
    ///
    /// Returns `Ok(true)` if the owner's in-memory state actually
    /// changed, `Ok(false)` if nothing needed to change (including: the
    /// path names nothing registered, or its owner has been dropped —
    /// in the latter case the dead entry is pruned from the map so it
    /// doesn't accumulate forever).
    pub fn dispatch(&self, changed_path: &Path) -> Result<bool, SettingsFileError> {
        let key = canonical_settings_path(changed_path);
        let weak = { self.entries.borrow().get(&key).cloned() };
        let Some(weak) = weak else {
            return Ok(false);
        };
        match weak.upgrade() {
            Some(reloadable) => reloadable.reload_from_disk(),
            None => {
                self.entries.borrow_mut().remove(&key);
                Ok(false)
            }
        }
    }

    /// The canonical paths currently registered (including entries whose
    /// owner has since been dropped but not yet pruned by a `dispatch`
    /// call). Exposed for tests and diagnostics.
    pub fn registered_paths(&self) -> Vec<PathBuf> {
        self.entries.borrow().keys().cloned().collect()
    }

    /// Number of live (upgradeable) entries. Exposed for tests.
    pub fn live_count(&self) -> usize {
        self.entries
            .borrow()
            .values()
            .filter(|w| w.upgrade().is_some())
            .count()
    }
}

impl std::fmt::Debug for SettingsRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let entries = self.entries.borrow();
        f.debug_struct("SettingsRegistry")
            .field("registered_paths", &entries.len())
            .field(
                "live",
                &entries.values().filter(|w| w.upgrade().is_some()).count(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::SettingsFile;
    use crate::migration::{Migrator, Versioned};
    use serde::{Deserialize, Serialize};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
    struct Prefs {
        version: u32,
        value: String,
    }
    impl Versioned for Prefs {
        const CURRENT_VERSION: u32 = 1;
        fn version(&self) -> u32 {
            self.version
        }
        fn set_version(&mut self, v: u32) {
            self.version = v;
        }
    }

    /// Poll `condition` for up to `timeout`, sleeping briefly between
    /// checks. Never a blind fixed sleep: returns as soon as the
    /// condition is true, and fails loudly (via the caller's assertion)
    /// if it never becomes true within the (generous) budget.
    fn poll_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if condition() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    const GENEROUS_TIMEOUT: Duration = Duration::from_secs(5);

    // -----------------------------------------------------------------
    // canonical_settings_path
    // -----------------------------------------------------------------

    #[test]
    fn canonical_settings_path_resolves_even_when_file_is_missing() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("not-yet-written.toml");
        let resolved = canonical_settings_path(&missing);
        // The parent must have been canonicalized even though the leaf
        // file does not exist.
        assert_eq!(
            resolved.parent().unwrap(),
            dir.path().canonicalize().unwrap()
        );
        assert_eq!(resolved.file_name().unwrap(), "not-yet-written.toml");
    }

    #[test]
    fn canonical_settings_path_is_stable_across_existence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("general.toml");

        let before = canonical_settings_path(&path);
        std::fs::write(&path, "version = 1\n").unwrap();
        let after = canonical_settings_path(&path);

        assert_eq!(
            before, after,
            "registering before vs. after creation must key identically"
        );
    }

    // -----------------------------------------------------------------
    // SettingsRegistry
    // -----------------------------------------------------------------

    #[test]
    fn dispatch_on_unregistered_path_is_a_harmless_no_op() {
        let registry = SettingsRegistry::new();
        let dir = tempdir().unwrap();
        assert!(!registry.dispatch(&dir.path().join("unknown.toml")).unwrap());
    }

    #[test]
    fn register_then_dispatch_reloads_a_peers_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.toml");

        let a: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        let registry = SettingsRegistry::new();
        let _handle = registry.register(Rc::new(b.clone()) as Rc<dyn Reloadable>);

        a.mutate(|p| p.value = "peer-write".into()).unwrap();

        assert!(registry.dispatch(&path).unwrap());
        assert_eq!(b.snapshot().value, "peer-write");
    }

    #[test]
    fn dropped_service_is_pruned_and_never_dispatched_to() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.toml");

        let a: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        let registry = SettingsRegistry::new();
        let handle = registry.register(Rc::new(b.clone()) as Rc<dyn Reloadable>);
        assert_eq!(registry.live_count(), 1);

        drop(handle);
        // `b` itself is still alive (it's a separate `SettingsFile` clone
        // handle sharing the same on-disk file), but the *registration*
        // Rc was the only strong reference the registry's Weak could
        // upgrade through — dropping it must deregister.
        assert_eq!(registry.live_count(), 0);

        a.mutate(|p| p.value = "peer-write-after-drop".into())
            .unwrap();

        // No owner to dispatch to any more: reports nothing happened,
        // and prunes the dead entry.
        assert!(!registry.dispatch(&path).unwrap());
        assert!(registry.registered_paths().is_empty());
        // `b`'s own in-memory value is untouched — nothing was called on it.
        assert_eq!(b.snapshot().value, "");
    }

    #[test]
    fn registering_under_the_same_path_replaces_the_previous_owner() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.toml");

        let first: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let second: SettingsFile<Prefs> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        let registry = SettingsRegistry::new();
        let _first_handle = registry.register(Rc::new(first.clone()) as Rc<dyn Reloadable>);
        let _second_handle = registry.register(Rc::new(second.clone()) as Rc<dyn Reloadable>);

        assert_eq!(registry.registered_paths().len(), 1);

        let writer: SettingsFile<Prefs> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        writer.mutate(|p| p.value = "via-second".into()).unwrap();

        assert!(registry.dispatch(&path).unwrap());
        assert_eq!(second.snapshot().value, "via-second");
        // `first` was replaced in the registry and never touched.
        assert_eq!(first.snapshot().value, "");
    }

    // -----------------------------------------------------------------
    // SettingsWatcher — end-to-end, real notify + real filesystem.
    // -----------------------------------------------------------------

    #[test]
    fn construction_over_a_missing_directory_does_not_error() {
        let sink: SettingsReloadSink = Arc::new(|_path| {});
        let watcher = SettingsWatcher::new(
            vec![PathBuf::from("/definitely/does/not/exist/anywhere")],
            sink,
        );
        assert!(watcher.is_ok());
    }

    #[test]
    fn construction_dedupes_identical_directories() {
        // `AppPaths::for_testing` routes config_dir and data_dir to the
        // same tempdir; watching it twice must not error (and must not
        // register the OS-level watch twice).
        let dir = tempdir().unwrap();
        let sink: SettingsReloadSink = Arc::new(|_path| {});
        let watcher = SettingsWatcher::new(
            vec![dir.path().to_path_buf(), dir.path().to_path_buf()],
            sink,
        );
        assert!(watcher.is_ok());
    }

    /// `SettingsRegistry` is `Rc`-based (single-threaded) by design — it
    /// is only ever touched from the UI thread in production, reached
    /// via an `AppEvent` posted through the winit proxy (see
    /// `teksilo-app`'s wiring). A `SettingsWatcher`'s sink, in contrast,
    /// runs on the notify background thread and must be `Send + Sync`.
    /// These tests reproduce that exact split: the sink only pushes the
    /// changed path onto a thread-safe queue; the *test* thread (which
    /// is also where every `SettingsFile` / `SettingsRegistry` handle
    /// below was constructed) drains it and calls `dispatch` itself —
    /// exactly mirroring the real cross-thread handoff.
    type PathQueue = Arc<std::sync::Mutex<std::collections::VecDeque<PathBuf>>>;

    fn queueing_sink(queue: PathQueue) -> SettingsReloadSink {
        Arc::new(move |path| {
            queue.lock().unwrap().push_back(path);
        })
    }

    /// Drain every path currently queued and dispatch each through
    /// `registry` (on the calling thread), returning how many produced
    /// an *effective* reload (`Ok(true)`).
    fn drain_and_dispatch(registry: &SettingsRegistry, queue: &PathQueue) -> usize {
        let paths: Vec<PathBuf> = queue.lock().unwrap().drain(..).collect();
        paths
            .into_iter()
            .filter(|p| matches!(registry.dispatch(p), Ok(true)))
            .count()
    }

    /// An external write (a second handle, standing in for a peer
    /// process) to a watched, registered file drives exactly one
    /// *effective* reload — i.e. the registered `Reloadable`'s
    /// in-memory state flips exactly once, no matter how many raw
    /// filesystem events the OS coalesces the write into. This is the
    /// self-write-suppression / content-backstop contract in
    /// `reload_from_disk` doing its job on top of a possibly-noisy
    /// stream of notify events — exactly the property a watcher must
    /// preserve.
    #[test]
    fn external_write_triggers_exactly_one_effective_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.toml");

        // Create the file up front so `dir` already contains something
        // watchable and the peer's first write is a real Modify.
        let peer: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let mine: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        let registry = SettingsRegistry::new();
        let _handle = registry.register(Rc::new(mine.clone()) as Rc<dyn Reloadable>);

        let queue: PathQueue = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let _watcher =
            SettingsWatcher::new(vec![dir.path().to_path_buf()], queueing_sink(queue.clone()))
                .unwrap();

        peer.mutate(|p| p.value = "external".into()).unwrap();

        let mut effective_reloads = 0usize;
        assert!(
            poll_until(GENEROUS_TIMEOUT, || {
                effective_reloads += drain_and_dispatch(&registry, &queue);
                effective_reloads >= 1
            }),
            "expected the external write to be picked up within the timeout"
        );

        // Give any duplicate/coalesced OS events a further bounded window
        // to arrive and prove they don't cause a second *effective*
        // reload (the stamp/content backstop must absorb them).
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            effective_reloads += drain_and_dispatch(&registry, &queue);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            effective_reloads, 1,
            "exactly one effective reload, however many raw fs events fired"
        );
        assert_eq!(mine.snapshot().value, "external");
    }

    /// Our own write must never be treated as a peer's: dispatching the
    /// path right after our own `mutate` must report `Ok(false)` and
    /// touch nothing, because `reload_from_disk`'s stamp check
    /// recognizes it as already-current.
    #[test]
    fn our_own_write_triggers_no_effective_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.toml");

        let mine: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        let registry = SettingsRegistry::new();
        let _handle = registry.register(Rc::new(mine.clone()) as Rc<dyn Reloadable>);

        mine.mutate(|p| p.value = "mine".into()).unwrap();

        // Simulate the watcher noticing our own write (it would: the OS
        // can't tell who wrote it) and dispatching it straight through.
        assert!(!registry.dispatch(&path).unwrap());
        assert_eq!(
            mine.snapshot().value,
            "mine",
            "our own value must be untouched"
        );
    }

    /// A dropped service's registration must never fire, even though its
    /// underlying file keeps being written to by a peer. Proven via a
    /// live sentinel registered on a *different* file in the same
    /// watched directory: once the sentinel's write is observed, the
    /// dropped service's counter is asserted to still be zero — bounding
    /// the wait deterministically instead of a blind sleep.
    #[test]
    fn dropped_service_is_never_called_by_a_live_watcher() {
        let dir = tempdir().unwrap();
        let dropped_path = dir.path().join("dropped.toml");
        let sentinel_path = dir.path().join("sentinel.toml");

        let dropped_peer: SettingsFile<Prefs> =
            SettingsFile::load(dropped_path.clone(), Migrator::new()).unwrap();
        let dropped_mine: SettingsFile<Prefs> =
            SettingsFile::load(dropped_path.clone(), Migrator::new()).unwrap();
        let sentinel_peer: SettingsFile<Prefs> =
            SettingsFile::load(sentinel_path.clone(), Migrator::new()).unwrap();
        let sentinel_mine: SettingsFile<Prefs> =
            SettingsFile::load(sentinel_path.clone(), Migrator::new()).unwrap();

        let registry = SettingsRegistry::new();
        let dropped_handle = registry.register(Rc::new(dropped_mine.clone()) as Rc<dyn Reloadable>);
        let _sentinel_handle =
            registry.register(Rc::new(sentinel_mine.clone()) as Rc<dyn Reloadable>);

        // Drop the "dropped" service's registration handle before any
        // write happens.
        drop(dropped_handle);
        assert_eq!(registry.live_count(), 1);

        let queue: PathQueue = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let _watcher =
            SettingsWatcher::new(vec![dir.path().to_path_buf()], queueing_sink(queue.clone()))
                .unwrap();

        dropped_peer
            .mutate(|p| p.value = "should-never-land".into())
            .unwrap();
        sentinel_peer
            .mutate(|p| p.value = "sentinel-fired".into())
            .unwrap();

        let dropped_key = canonical_settings_path(&dropped_path);
        let sentinel_key = canonical_settings_path(&sentinel_path);
        let mut dropped_reloads = 0usize;
        let mut sentinel_reloads = 0usize;

        // Manual poll loop (not `poll_until`, which only tracks a single
        // `bool` condition) so both counters can be accumulated on every
        // iteration without a nested-closure double-borrow.
        let deadline = Instant::now() + GENEROUS_TIMEOUT;
        loop {
            let paths: Vec<PathBuf> = queue.lock().unwrap().drain(..).collect();
            for p in paths {
                let key = canonical_settings_path(&p);
                if let Ok(true) = registry.dispatch(&p) {
                    if key == dropped_key {
                        dropped_reloads += 1;
                    } else if key == sentinel_key {
                        sentinel_reloads += 1;
                    }
                }
            }
            if sentinel_reloads >= 1 || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(
            sentinel_reloads >= 1,
            "sentinel write should have been observed within the timeout"
        );
        assert_eq!(
            dropped_reloads, 0,
            "a dropped service's registration must never be dispatched to"
        );
        assert_eq!(
            dropped_mine.snapshot().value,
            "",
            "the dropped service's in-memory value must be untouched"
        );
    }

    /// The rename dance: deleting the underlying file and recreating it
    /// must not break the watch (which targets the parent directory, not
    /// the file's inode) — the recreation is picked up like any other
    /// write.
    #[test]
    fn survives_file_deletion_and_recreation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.toml");

        // File must exist up front for the initial `load` to establish a
        // stamp baseline consistent with the rest of the suite.
        std::fs::write(&path, "version = 1\nvalue = \"\"\n").unwrap();
        let mine: SettingsFile<Prefs> = SettingsFile::load(path.clone(), Migrator::new()).unwrap();

        let registry = SettingsRegistry::new();
        let _handle = registry.register(Rc::new(mine.clone()) as Rc<dyn Reloadable>);

        let queue: PathQueue = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
        let _watcher =
            SettingsWatcher::new(vec![dir.path().to_path_buf()], queueing_sink(queue.clone()))
                .unwrap();

        // Delete the file entirely (no peer handle involved — a plain
        // `remove_file`, closer to what an external "reset settings"
        // tool would do), then recreate it via a fresh locked write,
        // exactly like `write_atomic`'s temp-file-then-rename.
        std::fs::remove_file(&path).unwrap();

        let recreated: SettingsFile<Prefs> =
            SettingsFile::load(path.clone(), Migrator::new()).unwrap();
        recreated
            .mutate(|p| p.value = "recreated-after-delete".into())
            .unwrap();

        let mut effective_reloads = 0usize;
        assert!(
            poll_until(GENEROUS_TIMEOUT, || {
                effective_reloads += drain_and_dispatch(&registry, &queue);
                effective_reloads >= 1
            }),
            "the recreated file's write should still be observed after the watched \
             file was deleted and recreated"
        );
        assert_eq!(mine.snapshot().value, "recreated-after-delete");
    }
}
