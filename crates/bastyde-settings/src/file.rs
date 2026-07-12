// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SettingsFile<T>` — typed single-struct persistence.
//!
//! Used when the persisted shape is a known struct (recents, window
//! state) rather than a dynamic K/V map. The current value lives in a
//! `RefCell<T>` inside an `Rc<>`-shared inner so that multiple handles
//! can observe and mutate the same projection.
//!
//! The file is loaded once at construction (running registered
//! migrations); after that, all reads come from `current` in memory and
//! all writes flush via [`DebouncedWriter`]. Corrupt or unmigratable
//! files are renamed to `<path>.broken-<unix_ts>` and the in-memory
//! value falls back to `T::default()` so the app keeps running.
//!
//! ## When to use
//!
//! Prefer [`SettingsFile<T>`] over [`crate::SettingsStore`] when the
//! settings form a known typed struct (e.g. a window-layout blob or a
//! recents list). Use `SettingsStore` for open-ended scalar K/V pairs
//! that arrive at different call sites.
//!
//! ```ignore
//! use bastyde_settings::{SettingsFile, Migrator, Versioned};
//! use serde::{Serialize, Deserialize};
//! use std::time::Duration;
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
//!     SettingsFile::load(path, Duration::from_millis(500), &Migrator::new()).unwrap();
//!
//! file.mutate(|p| p.font_size = 16.0).unwrap();
//! file.flush_now().unwrap();
//! ```
//!
//! ## Cross-process (shared) mode — opt-in
//!
//! [`SettingsFile::load`] (the default, and the only mode described
//! above) is last-write-wins across processes: it reads the file
//! exactly once and every later `mutate`/`replace` re-serializes from
//! that increasingly-stale in-memory snapshot. Two processes sharing a
//! file this way will have one silently clobber the other's write.
//! That default is unchanged and remains zero-cost — most consumers
//! (`WindowStateService`, `MruList`, `SettingsStore`-backed state) are
//! single-instance-per-file and should keep using it.
//!
//! Apps that know a file is genuinely shared by more than one process
//! (Skribisto's `backup.toml`, one process per open project) opt into
//! [`SettingsFile::load_shared`] instead. In shared mode:
//!
//! * [`mutate`](SettingsFile::mutate) and
//!   [`replace`](SettingsFile::replace) perform a **locked
//!   read-modify-write**: acquire an exclusive advisory lock (see
//!   [`crate::lock`]) on a `<path>.lock` sidecar, re-read + re-migrate
//!   the file from disk *under the lock*, apply the caller's change to
//!   that fresh value, write it back atomically, refresh the in-memory
//!   snapshot, then release the lock. A lock alone would only stop the
//!   two writes from interleaving on disk — it does nothing to stop a
//!   stale in-memory snapshot from clobbering a peer's newer data, so
//!   the re-read has to happen *under* the same lock that guards the
//!   write.
//! * These writes bypass the debounce entirely and happen
//!   synchronously on the caller's thread — shared-mode writes are
//!   expected to be rare (a settings change, a backup record), so
//!   there is no burst to coalesce.
//! * [`reload_if_stale`](SettingsFile::reload_if_stale) is how *reads*
//!   pick up a peer's change; it is a cheap mtime check, available in
//!   both modes.
//!
//! The default mode remains last-write-wins; see `docs/settings.md`
//! for the full contract and rationale.

use std::cell::{Cell, Ref, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::flush::{DebouncedWriter, FlushError, write_atomic};
use crate::lock::FileLock;
use crate::migration::{MigrationError, Migrator, Versioned};

/// Number of times [`SettingsFile`]'s locked read-modify-write (and
/// [`SettingsFile::reload_if_stale`]) will retry a TOML parse failure
/// before surfacing it. Atomic rename means a well-behaved peer should
/// never hand us a torn write, but we retry briefly rather than treat a
/// transient failure as fatal — and, critically, rather than
/// quarantine the file, which could destroy a peer's legitimate data.
const MAX_READ_ATTEMPTS: u32 = 5;
/// Delay between retries in [`MAX_READ_ATTEMPTS`].
const READ_RETRY_DELAY: Duration = Duration::from_millis(5);

/// Errors surfaced by [`SettingsFile`] operations.
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

struct Inner<T> {
    current: RefCell<T>,
    writer: DebouncedWriter,
    /// The on-disk mtime as of the last time we read or wrote the file
    /// (via construction, a locked read-modify-write, or
    /// [`SettingsFile::reload_if_stale`]). `None` means "file did not
    /// exist as of our last look."
    last_known_mtime: Cell<Option<SystemTime>>,
    /// `Some` puts this handle in shared (cross-process) mode: `mutate`
    /// / `replace` perform a locked read-modify-write instead of the
    /// default debounced schedule, and `reload_if_stale` re-migrates
    /// using this closure rather than a disposable empty `Migrator`.
    shared: Option<SharedMode<T>>,
}

/// The type-erased re-migration capability a shared-mode
/// [`SettingsFile`] retains for the lifetime of the handle. Boxing the
/// closure (rather than storing `Migrator<T>` directly) keeps `Inner<T>`
/// itself unbounded — `Migrator<T>` requires `T: Versioned +
/// DeserializeOwned`, which would otherwise have to be threaded onto
/// every impl block touching `Inner<T>` / `SettingsFile<T>`.
struct SharedMode<T> {
    migrate: Box<dyn Fn(toml::Value) -> Result<T, MigrationError>>,
}

/// A reactive handle to a single typed file on disk.
///
/// `Clone` is cheap (an `Rc` bump). All clones share one in-memory
/// projection and one I/O thread.
pub struct SettingsFile<T> {
    inner: Rc<Inner<T>>,
}

impl<T> Clone for SettingsFile<T> {
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
    /// `delay` is the debounce window for subsequent writes.
    ///
    /// On parse / migration failure the offending file is renamed to
    /// `<path>.broken-<ts>` and the returned `SettingsFile` starts from
    /// `T::default()`. The caller can detect this by passing a
    /// pre-known default; the broken-file path is returned in the
    /// error variants only when `load_strict` is used.
    pub fn load(
        path: PathBuf,
        delay: Duration,
        migrator: &Migrator<T>,
    ) -> Result<Self, SettingsFileError> {
        let initial = match Self::read_from_disk(&path, migrator) {
            Ok(value) => value,
            Err(SettingsFileError::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                v
            }
            Err(other) => {
                // Quarantine the broken file so the next launch starts clean.
                quarantine(&path);
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                eprintln!(
                    "bastyde-settings: load failed for {}: {}; falling back to defaults",
                    path.display(),
                    other,
                );
                v
            }
        };

        let last_known_mtime = disk_mtime(&path);
        let writer = DebouncedWriter::new(path, delay);
        Ok(Self {
            inner: Rc::new(Inner {
                current: RefCell::new(initial),
                writer,
                last_known_mtime: Cell::new(last_known_mtime),
                shared: None,
            }),
        })
    }

    /// Like [`load`](Self::load), but returns parse / migration errors
    /// instead of quarantining the file. Intended for tests that want
    /// to assert on a specific failure mode.
    pub fn load_strict(
        path: PathBuf,
        delay: Duration,
        migrator: &Migrator<T>,
    ) -> Result<Self, SettingsFileError> {
        let initial = match Self::read_from_disk(&path, migrator) {
            Ok(value) => value,
            Err(SettingsFileError::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                v
            }
            Err(other) => return Err(other),
        };

        let last_known_mtime = disk_mtime(&path);
        let writer = DebouncedWriter::new(path, delay);
        Ok(Self {
            inner: Rc::new(Inner {
                current: RefCell::new(initial),
                writer,
                last_known_mtime: Cell::new(last_known_mtime),
                shared: None,
            }),
        })
    }

    /// Open the file in **shared (cross-process) mode**. See the "Cross-process
    /// (shared) mode" section of the module docs for the full contract.
    ///
    /// Unlike [`load`](Self::load) / [`load_strict`](Self::load_strict),
    /// `migrator` is taken **by value** and retained for the lifetime of
    /// the handle: shared mode needs to re-run migrations on every
    /// locked read (a peer might still be on an older on-disk schema),
    /// not just once at construction.
    ///
    /// The initial read itself is lock-protected too, so a peer that is
    /// mid-write when this process starts up can't hand us a torn read
    /// (see [`crate::lock`]).
    ///
    /// On parse / migration failure the offending file is renamed to
    /// `<path>.broken-<ts>` and the returned `SettingsFile` starts from
    /// `T::default()`, exactly like [`load`](Self::load).
    ///
    /// Writes made through the returned handle (`mutate` / `replace`)
    /// bypass the debounce entirely — they are synchronous, locked
    /// read-modify-writes.
    pub fn load_shared(path: PathBuf, migrator: Migrator<T>) -> Result<Self, SettingsFileError> {
        // Move the migrator into a type-erased closure once; `migrate`
        // stays callable (by shared reference) for as long as this
        // `SettingsFile` lives.
        let migrate: Box<dyn Fn(toml::Value) -> Result<T, MigrationError>> =
            Box::new(move |raw| migrator.run(raw));

        let lock = FileLock::acquire_exclusive(&path).map_err(SettingsFileError::Io)?;
        let initial = match Self::read_or_default(&path, migrate.as_ref()) {
            Ok(value) => value,
            Err(other) => {
                quarantine(&path);
                let mut v = T::default();
                v.set_version(T::CURRENT_VERSION);
                eprintln!(
                    "bastyde-settings: shared load failed for {}: {}; falling back to defaults",
                    path.display(),
                    other,
                );
                v
            }
        };
        let last_known_mtime = disk_mtime(&path);
        drop(lock);

        // Shared mode never calls `schedule` on this writer — writes go
        // through the locked path in `mutate`/`replace` and hit disk
        // synchronously via `write_atomic`. We still keep a
        // `DebouncedWriter` around purely so `path()` / `flush_now()`
        // (a no-op here, since nothing is ever pending) keep working
        // uniformly across both modes.
        let writer = DebouncedWriter::new(path, Duration::ZERO);

        Ok(Self {
            inner: Rc::new(Inner {
                current: RefCell::new(initial),
                writer,
                last_known_mtime: Cell::new(last_known_mtime),
                shared: Some(SharedMode { migrate }),
            }),
        })
    }

    fn read_from_disk(path: &Path, migrator: &Migrator<T>) -> Result<T, SettingsFileError> {
        Self::read_from_disk_via(path, &|raw| migrator.run(raw))
    }

    /// Shared implementation behind [`read_from_disk`](Self::read_from_disk):
    /// parse the file's TOML, run `migrate`, and stamp the current
    /// version. Generic over the migration closure so both the
    /// borrowed-`Migrator` path (default mode) and the owned, boxed
    /// closure (shared mode) can share this body.
    fn read_from_disk_via(
        path: &Path,
        migrate: &dyn Fn(toml::Value) -> Result<T, MigrationError>,
    ) -> Result<T, SettingsFileError> {
        let raw_text = fs::read_to_string(path)?;
        let raw_value: toml::Value = toml::from_str(&raw_text).map_err(SettingsFileError::Parse)?;
        let mut value = migrate(raw_value).map_err(SettingsFileError::Migrate)?;
        value.set_version(T::CURRENT_VERSION);
        Ok(value)
    }

    /// [`read_from_disk_via`](Self::read_from_disk_via), but a missing
    /// file falls back to `T::default()` (exactly like `load`'s
    /// `NotFound` arm) and a TOML parse failure is retried a few times
    /// before being surfaced — see [`MAX_READ_ATTEMPTS`]'s doc comment
    /// for why we retry rather than quarantine here.
    fn read_or_default(
        path: &Path,
        migrate: &dyn Fn(toml::Value) -> Result<T, MigrationError>,
    ) -> Result<T, SettingsFileError> {
        let mut last_parse_err = None;
        for attempt in 0..MAX_READ_ATTEMPTS {
            match Self::read_from_disk_via(path, migrate) {
                Ok(value) => return Ok(value),
                Err(SettingsFileError::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {
                    let mut v = T::default();
                    v.set_version(T::CURRENT_VERSION);
                    return Ok(v);
                }
                Err(SettingsFileError::Parse(e)) => {
                    last_parse_err = Some(e);
                    if attempt + 1 < MAX_READ_ATTEMPTS {
                        thread::sleep(READ_RETRY_DELAY);
                    }
                }
                Err(other) => return Err(other),
            }
        }
        Err(SettingsFileError::Parse(last_parse_err.unwrap()))
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

    /// Replace the current value and persist it.
    /// `T::set_version(T::CURRENT_VERSION)` is called so the version
    /// stamp is always coherent, even if the caller forgot.
    ///
    /// In shared mode this performs a locked read-modify-write (the
    /// "read" is discarded — `replace` always wins over whatever was on
    /// disk — but the lock still serializes it against a concurrent
    /// peer write, and the fresh disk mtime is recorded so
    /// `reload_if_stale` doesn't immediately re-read our own write back
    /// in). In default mode this schedules a debounced write, exactly
    /// as before.
    pub fn replace(&self, mut new: T) -> Result<(), SettingsFileError> {
        if let Some(shared) = &self.inner.shared {
            return self.locked_read_modify_write(shared, move |v| *v = new);
        }
        new.set_version(T::CURRENT_VERSION);
        let serialized = toml::to_string_pretty(&new).map_err(SettingsFileError::Serialize)?;
        *self.inner.current.borrow_mut() = new;
        self.inner.writer.schedule(serialized);
        Ok(())
    }

    /// Mutate the current value in place and persist it.
    ///
    /// In shared mode this performs a locked read-modify-write: the
    /// file is re-read and re-migrated from disk *under an exclusive
    /// lock* before `f` is applied, so `f` always sees a fresh value —
    /// not this handle's possibly-stale in-memory snapshot — and the
    /// result is written back atomically before the lock is released.
    /// In default mode this schedules a debounced write, exactly as
    /// before.
    pub fn mutate<F: FnOnce(&mut T)>(&self, f: F) -> Result<(), SettingsFileError> {
        if let Some(shared) = &self.inner.shared {
            return self.locked_read_modify_write(shared, f);
        }
        let serialized = {
            let mut guard = self.inner.current.borrow_mut();
            f(&mut *guard);
            guard.set_version(T::CURRENT_VERSION);
            toml::to_string_pretty(&*guard).map_err(SettingsFileError::Serialize)?
        };
        self.inner.writer.schedule(serialized);
        Ok(())
    }

    /// The locked read-modify-write behind shared-mode `mutate`/`replace`.
    ///
    /// Acquires the exclusive advisory lock, re-reads + re-migrates the
    /// document fresh from disk (falling back to `T::default()` if it
    /// is absent, exactly like `load`), applies `f` to that fresh value,
    /// writes the result back atomically, then refreshes this handle's
    /// in-memory `current` and mtime baseline — all before releasing
    /// the lock. Bypasses the debounce: this write happens synchronously
    /// on the caller's thread.
    fn locked_read_modify_write<F: FnOnce(&mut T)>(
        &self,
        shared: &SharedMode<T>,
        f: F,
    ) -> Result<(), SettingsFileError> {
        let path = self.inner.writer.path().to_path_buf();
        let lock = FileLock::acquire_exclusive(&path).map_err(SettingsFileError::Io)?;

        let mut fresh = Self::read_or_default(&path, shared.migrate.as_ref())?;
        f(&mut fresh);
        fresh.set_version(T::CURRENT_VERSION);
        let serialized = toml::to_string_pretty(&fresh).map_err(SettingsFileError::Serialize)?;
        write_atomic(&path, &serialized).map_err(SettingsFileError::Io)?;

        let new_mtime = disk_mtime(&path);
        *self.inner.current.borrow_mut() = fresh;
        self.inner.last_known_mtime.set(new_mtime);

        drop(lock);
        Ok(())
    }

    /// Pick up a peer's change: if the on-disk mtime differs from the
    /// last one this handle observed (via construction, a locked
    /// read-modify-write, or a previous `reload_if_stale` call), re-read
    /// and re-migrate the file and refresh `current`. Returns whether a
    /// reload happened.
    ///
    /// Available in both modes — it is how a *reader* (as opposed to a
    /// writer, which goes through the locked path above in shared mode)
    /// notices a peer's write. It is a cheap `stat`, safe to call
    /// speculatively (e.g. on every focus-in, or on a timer).
    ///
    /// In shared mode, re-migration uses the `Migrator` this handle was
    /// opened with. In default mode there is no retained `Migrator` (by
    /// design — see [`load`](Self::load)'s signature), so a disposable
    /// empty one is used instead: this correctly picks up a peer's
    /// write when the on-disk file is already at `T::CURRENT_VERSION`
    /// (the expected case — a peer running the same build writes
    /// current-version files), and surfaces a clear
    /// [`SettingsFileError::Migrate`] rather than silently losing data
    /// if the on-disk file genuinely still needs a migration step this
    /// handle can't run.
    pub fn reload_if_stale(&self) -> Result<bool, SettingsFileError> {
        let path = self.inner.writer.path();
        let current_mtime = disk_mtime(path);
        if current_mtime == self.inner.last_known_mtime.get() {
            return Ok(false);
        }

        let value = match &self.inner.shared {
            Some(shared) => Self::read_or_default(path, shared.migrate.as_ref())?,
            None => {
                let empty = Migrator::<T>::new();
                Self::read_or_default(path, &|raw| empty.run(raw))?
            }
        };

        *self.inner.current.borrow_mut() = value;
        self.inner.last_known_mtime.set(current_mtime);
        Ok(true)
    }

    /// Synchronously write any pending payload to disk. In shared mode
    /// this is a no-op (`Ok(())`): shared-mode writes already happen
    /// synchronously in `mutate`/`replace`, so nothing is ever pending.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.inner
            .writer
            .flush_now()
            .map_err(SettingsFileError::Flush)
    }

    /// The path being written to.
    pub fn path(&self) -> &Path {
        self.inner.writer.path()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for SettingsFile<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsFile")
            .field("path", &self.inner.writer.path())
            .field("current", &*self.inner.current.borrow())
            .finish()
    }
}

/// The file's modification time, or `None` if it doesn't exist (or the
/// platform can't report one). Used as the cheap staleness signal for
/// [`SettingsFile::reload_if_stale`] and to re-baseline after a locked
/// read-modify-write.
fn disk_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn quarantine(path: &Path) {
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
        let file: SettingsFile<Settings> =
            SettingsFile::load(path, Duration::ZERO, &Migrator::new()).unwrap();
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
    fn replace_persists_after_flush_now() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let file: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Duration::from_millis(10), &Migrator::new()).unwrap();

        file.replace(Settings {
            version: 1,
            font_size: 16.0,
            theme: "dark".into(),
        })
        .unwrap();
        file.flush_now().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let again: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(again.font_size, 16.0);
        assert_eq!(again.theme, "dark");
    }

    #[test]
    fn mutate_modifies_in_place() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let file: SettingsFile<Settings> =
            SettingsFile::load(path, Duration::ZERO, &Migrator::new()).unwrap();

        file.mutate(|s| {
            s.font_size = 22.0;
            s.theme = "light".into();
        })
        .unwrap();
        file.flush_now().unwrap();

        assert_eq!(file.snapshot().font_size, 22.0);
    }

    #[test]
    fn corrupt_file_falls_back_to_default_and_quarantines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.toml");
        fs::write(&path, "this is = not valid TOML = at all").unwrap();

        let file: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Duration::ZERO, &Migrator::new()).unwrap();
        assert_eq!(file.snapshot().version, 1);

        // Original path should now be vacant or not the broken contents.
        // The .broken-<ts> sibling should exist.
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let has_quarantine = entries
            .iter()
            .any(|e| e.file_name().to_string_lossy().contains(".broken-"));
        assert!(has_quarantine, "expected a .broken-<ts> file");
    }

    #[test]
    fn load_strict_propagates_parse_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.toml");
        fs::write(&path, "= = =").unwrap();

        let result: Result<SettingsFile<Settings>, _> =
            SettingsFile::load_strict(path, Duration::ZERO, &Migrator::new());
        assert!(matches!(result, Err(SettingsFileError::Parse(_))));
    }

    #[test]
    fn clones_share_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.toml");
        let a: SettingsFile<Settings> =
            SettingsFile::load(path, Duration::ZERO, &Migrator::new()).unwrap();
        let b = a.clone();

        a.mutate(|s| s.font_size = 99.0).unwrap();
        assert_eq!(b.snapshot().font_size, 99.0);
    }

    // -----------------------------------------------------------------
    // Shared (cross-process) mode
    // -----------------------------------------------------------------

    /// THE HEADLINE TEST. Two independent `SettingsFile::load_shared`
    /// handles over the *same* path — standing in for two Skribisto
    /// processes sharing `backup.toml` — each mutate a *different*
    /// field. Because the locked read-modify-write re-reads the file
    /// fresh from disk under the lock before applying each change, both
    /// changes must survive: neither handle's stale in-memory snapshot
    /// gets a chance to clobber the other's write.
    #[test]
    fn shared_mode_two_concurrent_handles_both_writes_survive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared.toml");

        let a: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 42.0).unwrap();
        b.mutate(|s| s.theme = "solarized".into()).unwrap();

        // A third, fresh handle proves both writes are actually on disk
        // together, not just cached in `a`'s or `b`'s memory.
        let c: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();
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

    /// The bug this feature fixes: two *non-shared* handles (the
    /// pre-existing `load`) over the same path each load a private,
    /// never-refreshed snapshot. The second mutate's debounced write
    /// re-serializes from *its* stale snapshot — which still has the
    /// first handle's change as an in-memory default — clobbering it.
    #[test]
    fn non_shared_mode_loses_a_concurrent_peers_write_bug_repro() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonshared.toml");

        let a: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Duration::ZERO, &Migrator::new()).unwrap();
        let b: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Duration::ZERO, &Migrator::new()).unwrap();

        a.mutate(|s| s.font_size = 42.0).unwrap();
        a.flush_now().unwrap();

        // b's in-memory snapshot still has font_size == 0.0 from its own
        // `load` — it never re-read a's write. Its mutate serializes
        // *that* stale snapshot (with b's own change layered on top),
        // overwriting a's change on disk.
        b.mutate(|s| s.theme = "solarized".into()).unwrap();
        b.flush_now().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let on_disk: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(on_disk.theme, "solarized", "b's own write is present");
        assert_eq!(
            on_disk.font_size, 0.0,
            "documents the bug: b's stale snapshot silently discarded a's write"
        );
    }

    #[test]
    fn reload_if_stale_picks_up_a_peers_write_and_reports_no_change_when_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reload.toml");

        let a: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Duration::ZERO, &Migrator::new()).unwrap();
        let b: SettingsFile<Settings> =
            SettingsFile::load(path.clone(), Duration::ZERO, &Migrator::new()).unwrap();

        // Nothing has changed since both handles were constructed.
        assert!(!b.reload_if_stale().unwrap());

        a.mutate(|s| s.font_size = 7.0).unwrap();
        a.flush_now().unwrap();

        assert!(b.reload_if_stale().unwrap(), "b should notice a's write");
        assert_eq!(b.snapshot().font_size, 7.0);

        // Calling again with no further changes reports nothing new.
        assert!(!b.reload_if_stale().unwrap());
    }

    #[test]
    fn reload_if_stale_also_works_in_shared_mode() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reload_shared.toml");

        let a: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();

        assert!(!b.reload_if_stale().unwrap());

        a.mutate(|s| s.theme = "peer-write".into()).unwrap();

        assert!(b.reload_if_stale().unwrap());
        assert_eq!(b.snapshot().theme, "peer-write");
    }

    #[test]
    fn shared_mode_round_trips_versioned_migration() {
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
        let path = dir.path().join("migrated_shared.toml");
        // A legacy v1 file, missing the v2 `pinned` field.
        fs::write(&path, "version = 1\nname = \"legacy\"\n").unwrap();

        let migrator: Migrator<Prefs> = Migrator::new().step(1, |mut v| {
            if let Some(t) = v.as_table_mut() {
                t.insert("pinned".into(), toml::Value::Boolean(true));
            }
            Ok(v)
        });

        let file: SettingsFile<Prefs> = SettingsFile::load_shared(path.clone(), migrator).unwrap();
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
    fn shared_mode_replace_also_uses_the_locked_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared_replace.toml");

        let a: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();
        let b: SettingsFile<Settings> =
            SettingsFile::load_shared(path.clone(), Migrator::new()).unwrap();

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
    fn shared_mode_flush_now_is_a_harmless_no_op() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shared_flush.toml");
        let file: SettingsFile<Settings> =
            SettingsFile::load_shared(path, Migrator::new()).unwrap();

        file.mutate(|s| s.font_size = 1.0).unwrap();
        // Already durably written by `mutate`'s synchronous locked
        // write; `flush_now` must not error even though there is
        // nothing pending on the (unused, in shared mode) debounce path.
        file.flush_now().unwrap();
    }
}
