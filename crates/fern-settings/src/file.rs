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

use std::cell::{Ref, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::flush::{DebouncedWriter, FlushError};
use crate::migration::{MigrationError, Migrator, Versioned};

/// Errors surfaced by [`SettingsFile`] operations.
#[derive(Debug)]
pub enum SettingsFileError {
    Io(io::Error),
    Parse(toml::de::Error),
    Migrate(MigrationError),
    Serialize(toml::ser::Error),
    Flush(FlushError),
}

impl std::fmt::Display for SettingsFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsFileError::Io(e) => write!(f, "settings file I/O: {e}"),
            SettingsFileError::Parse(e) => write!(f, "settings file parse: {e}"),
            SettingsFileError::Migrate(e) => write!(f, "settings file migration: {e}"),
            SettingsFileError::Serialize(e) => write!(f, "settings file serialize: {e}"),
            SettingsFileError::Flush(e) => write!(f, "settings file flush: {e}"),
        }
    }
}

impl std::error::Error for SettingsFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SettingsFileError::Io(e) => Some(e),
            SettingsFileError::Parse(e) => Some(e),
            SettingsFileError::Migrate(e) => Some(e),
            SettingsFileError::Serialize(e) => Some(e),
            SettingsFileError::Flush(e) => Some(e),
        }
    }
}

impl From<io::Error> for SettingsFileError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

struct Inner<T> {
    current: RefCell<T>,
    writer: DebouncedWriter,
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
                    "fern-settings: load failed for {}: {}; falling back to defaults",
                    path.display(),
                    other,
                );
                v
            }
        };

        let writer = DebouncedWriter::new(path, delay);
        Ok(Self {
            inner: Rc::new(Inner {
                current: RefCell::new(initial),
                writer,
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

        let writer = DebouncedWriter::new(path, delay);
        Ok(Self {
            inner: Rc::new(Inner {
                current: RefCell::new(initial),
                writer,
            }),
        })
    }

    fn read_from_disk(path: &Path, migrator: &Migrator<T>) -> Result<T, SettingsFileError> {
        let raw_text = fs::read_to_string(path)?;
        let raw_value: toml::Value =
            toml::from_str(&raw_text).map_err(SettingsFileError::Parse)?;
        let mut value = migrator.run(raw_value).map_err(SettingsFileError::Migrate)?;
        value.set_version(T::CURRENT_VERSION);
        Ok(value)
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

    /// Replace the current value and schedule a debounced write.
    /// `T::set_version(T::CURRENT_VERSION)` is called so the version
    /// stamp is always coherent, even if the caller forgot.
    pub fn replace(&self, mut new: T) -> Result<(), SettingsFileError> {
        new.set_version(T::CURRENT_VERSION);
        let serialized = toml::to_string_pretty(&new).map_err(SettingsFileError::Serialize)?;
        *self.inner.current.borrow_mut() = new;
        self.inner.writer.schedule(serialized);
        Ok(())
    }

    /// Mutate the current value in place and schedule a debounced write.
    pub fn mutate<F: FnOnce(&mut T)>(&self, f: F) -> Result<(), SettingsFileError> {
        let serialized = {
            let mut guard = self.inner.current.borrow_mut();
            f(&mut *guard);
            guard.set_version(T::CURRENT_VERSION);
            toml::to_string_pretty(&*guard).map_err(SettingsFileError::Serialize)?
        };
        self.inner.writer.schedule(serialized);
        Ok(())
    }

    /// Synchronously write any pending payload to disk.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.inner.writer.flush_now().map_err(SettingsFileError::Flush)
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
            "fern-settings: could not quarantine {} -> {}: {}",
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
}
