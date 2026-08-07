// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! OS-correct path resolution for application config and data directories.
//!
//! [`AppPaths`] wraps `etcetera`'s native [`AppStrategy`] so the rest of the
//! crate has a single point of truth for where settings files live. In
//! production, [`AppPaths::new`] queries the OS (XDG on Linux,
//! `%APPDATA%` on Windows, `~/Library/Application Support` on macOS); in
//! tests, [`AppPaths::for_testing`] roots everything inside a `tempdir` so no
//! test ever touches the user's real config tree.
//!
//! ## Usage
//!
//! Pass an `AppPaths` instance to [`SettingsBundle`](crate::SettingsBundle),
//! [`SettingsStore`](crate::SettingsStore), or [`MruList`](crate::MruList); the
//! individual files within it are addressed by name via
//! [`config_file`](AppPaths::config_file) and
//! [`data_file`](AppPaths::data_file).
//!
//! ```ignore
//! use teksilo_settings::AppPaths;
//!
//! // Production: returns None when no home directory is detectable.
//! if let Some(paths) = AppPaths::new("eu", "FernTech", "MyApp") {
//!     let general_toml = paths.config_file("general");
//!     let cache_toml   = paths.data_file("cache");
//! }
//!
//! // Tests: deterministic, tempdir-rooted, never touches user files.
//! let tmp = tempfile::tempdir().unwrap();
//! let paths = AppPaths::for_testing(tmp.path());
//! assert_eq!(paths.config_file("settings"), tmp.path().join("settings.toml"));
//! ```

use std::path::{Path, PathBuf};

use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};

/// Resolved OS-correct application directories (config and data).
///
/// Construct with [`AppPaths::new`] for production code, or
/// [`AppPaths::for_testing`] in tests and headless CI environments.
/// Use [`AppPaths::from_dirs`] when the application manages its own
/// directory layout (e.g. portable mode).
#[derive(Debug, Clone)]
pub struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl AppPaths {
    /// Resolve directories from the OS. The `(qualifier, organization,
    /// application)` triple feeds [`etcetera::AppStrategyArgs`] as
    /// `(top_level_domain, author, app_name)` — same fields, different
    /// names — and selects the platform-native strategy (XDG on Linux,
    /// `%APPDATA%`-based on Windows, `~/Library/Application Support`
    /// on macOS).
    ///
    /// Returns `None` when no usable home directory could be detected
    /// (a sandboxed or unconfigured environment). Callers who want to
    /// degrade gracefully should fall back to [`AppPaths::for_testing`]
    /// with an in-process directory.
    pub fn new(qualifier: &str, organization: &str, application: &str) -> Option<Self> {
        let strategy = choose_app_strategy(AppStrategyArgs {
            top_level_domain: qualifier.to_string(),
            author: organization.to_string(),
            app_name: application.to_string(),
        })
        .ok()?;
        Some(Self {
            config_dir: strategy.config_dir(),
            data_dir: strategy.data_dir(),
        })
    }

    /// Construct an `AppPaths` rooted at an arbitrary directory. Used by
    /// tests so that no test ever touches the user's real config tree.
    /// Both `config_dir` and `data_dir` resolve to `root`.
    pub fn for_testing(root: &Path) -> Self {
        Self {
            config_dir: root.to_path_buf(),
            data_dir: root.to_path_buf(),
        }
    }

    /// Construct from explicit config and data directories. Useful when
    /// an application wants to override one or both (e.g. portable mode).
    pub fn from_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
        }
    }

    /// The platform-correct config directory (XDG_CONFIG_HOME, %APPDATA%,
    /// `~/Library/Preferences`, etc.).
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// The platform-correct data directory. Used for caches and
    /// per-window state — anything larger than a configuration file.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Resolve a per-concern config file by name (without extension).
    /// `name = "general"` yields `<config_dir>/general.toml`.
    pub fn config_file(&self, name: &str) -> PathBuf {
        self.config_dir.join(format!("{name}.toml"))
    }

    /// Resolve a per-concern data file by name (without extension).
    pub fn data_file(&self, name: &str) -> PathBuf {
        self.data_dir.join(format!("{name}.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn for_testing_routes_everything_to_root() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        assert_eq!(paths.config_dir(), dir.path());
        assert_eq!(paths.data_dir(), dir.path());
        assert_eq!(
            paths.config_file("recents"),
            dir.path().join("recents.toml")
        );
        assert_eq!(paths.data_file("cache"), dir.path().join("cache.toml"));
    }

    #[test]
    fn from_dirs_keeps_separate_paths() {
        let cfg = tempdir().unwrap();
        let data = tempdir().unwrap();
        let paths = AppPaths::from_dirs(cfg.path().to_path_buf(), data.path().to_path_buf());

        assert_eq!(paths.config_dir(), cfg.path());
        assert_eq!(paths.data_dir(), data.path());
    }
}
