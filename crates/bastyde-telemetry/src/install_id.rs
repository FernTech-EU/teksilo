// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pseudonymous-mode install identifier.
//!
//! [`InstallId`] is a stable random UUID v4 generated at first run,
//! persisted via [`SettingsFile<InstallIdFile>`] and rotated every 13
//! months (CNIL Sheet n°14 cookie/tracker lifespan ceiling).
//!
//! Anonymous mode does not construct an `InstallId` at all —
//! [`UsageReporter::install_id`](bastyde_core::telemetry::UsageReporter::install_id)
//! returns `None` unconditionally there.
//!
//! Rotation happens at `open_or_create` time. The caller is expected
//! to invoke `erase_remote_data()` *before* rotation so the user
//! doesn't lose the only handle to their server data — orchestrated
//! by `DynamicReporter` / `TelemetryBundle::open`.

use std::time::{Duration, SystemTime};

use bastyde_settings::{AppPaths, Migrator, SettingsFile, SettingsFileError, Versioned};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 13 months in seconds — the CNIL cookie/tracker lifespan ceiling.
pub const ROTATION_INTERVAL: Duration = Duration::from_secs(60 * 60 * 24 * 395);

/// Persisted install identifier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallIdFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub uuid: String,
    #[serde(default = "InstallIdFile::epoch_now")]
    pub generated_at: SystemTime,
}

impl InstallIdFile {
    fn epoch_now() -> SystemTime {
        SystemTime::UNIX_EPOCH
    }
}

impl Default for InstallIdFile {
    fn default() -> Self {
        Self {
            version: 0,
            uuid: String::new(),
            generated_at: SystemTime::UNIX_EPOCH,
        }
    }
}

impl Versioned for InstallIdFile {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

#[derive(Clone)]
pub struct InstallId {
    file: SettingsFile<InstallIdFile>,
}

impl InstallId {
    /// Open or create the install id file at
    /// `paths.config_file("telemetry-install-id")`.
    ///
    /// Generates a fresh UUID on first run or when rotation is overdue
    /// (≥13 months). The caller MUST call `erase_remote_data()` before
    /// rotation if a rotation is expected — once the local UUID is
    /// gone, the user loses the only handle to their server data.
    pub fn open_or_create(paths: &AppPaths, delay: Duration) -> Result<Self, SettingsFileError> {
        Self::open_with_clock(paths, delay, SystemTime::now())
    }

    /// Constructor that accepts an explicit `now` for tests with a
    /// mocked clock. Production code should use `open_or_create`.
    pub fn open_with_clock(
        paths: &AppPaths,
        delay: Duration,
        now: SystemTime,
    ) -> Result<Self, SettingsFileError> {
        // `delay` is vestigial — see `ConsentStore::open_with_clock`. The install
        // id is written once and then rotated ~yearly; there is no burst to
        // debounce, and `SettingsFile` writes synchronously under a lock now.
        let _ = delay;
        let migrator = Migrator::<InstallIdFile>::new();
        let file = SettingsFile::load(paths.config_file("telemetry-install-id"), migrator)?;

        let snap = file.snapshot();
        let needs_rotation = snap.uuid.is_empty()
            || now
                .duration_since(snap.generated_at)
                .map(|d| d > ROTATION_INTERVAL)
                .unwrap_or(true);
        if needs_rotation {
            file.mutate(|f| {
                f.uuid = Uuid::new_v4().to_string();
                f.generated_at = now;
            })?;
        }

        Ok(Self { file })
    }

    /// The current UUID. Always non-empty on a successfully opened
    /// `InstallId`.
    pub fn get(&self) -> String {
        self.file.snapshot().uuid
    }

    /// `Some` view of the UUID without cloning. Holds a `Ref` guard
    /// — see [`SettingsFile::borrow`].
    pub fn with<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        let snap = self.file.borrow();
        f(&snap.uuid)
    }

    /// Wipe the local UUID. Called by `discard_pending` on revoke
    /// and by `erase_remote_data` after a successful server delete.
    /// A subsequent `open_or_create` will regenerate.
    pub fn clear(&self) -> Result<(), SettingsFileError> {
        self.file.replace(InstallIdFile::default())
    }

    /// Force a fresh UUID right now (e.g. user wants to rotate
    /// preemptively for privacy reasons). Should be preceded by
    /// `erase_remote_data()` so the old server records are deleted.
    pub fn rotate(&self) -> Result<String, SettingsFileError> {
        let new = Uuid::new_v4().to_string();
        let new_clone = new.clone();
        self.file.mutate(|f| {
            f.uuid = new_clone;
            f.generated_at = SystemTime::now();
        })?;
        Ok(new)
    }

    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.file.flush_now()
    }

    pub fn path(&self) -> &std::path::Path {
        self.file.path()
    }
}

impl std::fmt::Debug for InstallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallId")
            .field("uuid", &self.get())
            .field("path", &self.file.path())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open(dir: &std::path::Path) -> InstallId {
        let paths = AppPaths::for_testing(dir);
        InstallId::open_or_create(&paths, Duration::ZERO).unwrap()
    }

    #[test]
    fn first_open_generates_uuid() {
        let dir = tempdir().unwrap();
        let id = open(dir.path());
        let uuid = id.get();
        assert!(!uuid.is_empty());
        assert!(Uuid::parse_str(&uuid).is_ok());
    }

    #[test]
    fn second_open_returns_same_uuid() {
        let dir = tempdir().unwrap();
        let first = open(dir.path()).get();
        let second = open(dir.path()).get();
        assert_eq!(first, second, "UUID should persist across reopens");
    }

    #[test]
    fn clear_then_open_generates_new_uuid() {
        let dir = tempdir().unwrap();
        let first;
        {
            let id = open(dir.path());
            first = id.get();
            id.clear().unwrap();
            id.flush_now().unwrap();
        }
        let id = open(dir.path());
        let second = id.get();
        assert_ne!(first, second);
    }

    #[test]
    fn rotation_overdue_generates_new_uuid() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(0);
        let id = InstallId::open_with_clock(&paths, Duration::ZERO, old_time).unwrap();
        let first = id.get();
        id.flush_now().unwrap();

        // Move the clock forward by 14 months — past the 13-month ceiling.
        let later = old_time + Duration::from_secs(60 * 60 * 24 * 30 * 14);
        let id = InstallId::open_with_clock(&paths, Duration::ZERO, later).unwrap();
        let second = id.get();

        assert_ne!(first, second, "UUID should rotate after 13 months");
    }

    #[test]
    fn manual_rotate_changes_uuid() {
        let dir = tempdir().unwrap();
        let id = open(dir.path());
        let first = id.get();
        let new = id.rotate().unwrap();
        assert_ne!(first, new);
        assert_eq!(id.get(), new);
    }
}
