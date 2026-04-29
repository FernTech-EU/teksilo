//! `SettingsBundle` — declarative configuration for the fern-app
//! integration.
//!
//! `FernAppBuilder::settings(bundle)` consumes a `SettingsBundle`,
//! opens the requested services against the app's [`AppPaths`], and
//! registers each one in the application's `app_state` registry so it
//! is reachable from any handler via the [`SettingsExt`] trait
//! (`use fern_settings::SettingsExt;`).
//!
//! ## What's in the bundle
//!
//! Only services the framework can construct without app-level type
//! information:
//!
//! * [`SettingsStore`] — the dynamic K/V store for scalar settings.
//! * [`WindowStateService`] — per-window geometry persistence (opt-in
//!   via [`with_window_state`](SettingsBundle::with_window_state)).
//!
//! Anything that needs an app-defined item type (recently-opened
//! projects/files, color palettes, saved searches) is **not** in the
//! bundle. Apps construct an [`MruList<T>`](crate::MruList) for each
//! such collection and register it themselves via
//! [`FernAppBuilder::app_state(handle)`](fern_app::FernAppBuilder::app_state).

use std::time::Duration;

use crate::file::SettingsFileError;
use crate::path::AppPaths;
use crate::store::{DEFAULT_DEBOUNCE, SettingsStore, SettingsStoreError};
use crate::window_state::WindowStateService;

/// Errors surfaced by [`SettingsBundle::open`].
#[derive(Debug)]
pub enum SettingsBundleError {
    Store(SettingsStoreError),
    File(SettingsFileError),
}

impl std::fmt::Display for SettingsBundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsBundleError::Store(e) => write!(f, "settings bundle: {e}"),
            SettingsBundleError::File(e) => write!(f, "settings bundle: {e}"),
        }
    }
}

impl std::error::Error for SettingsBundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SettingsBundleError::Store(e) => Some(e),
            SettingsBundleError::File(e) => Some(e),
        }
    }
}

impl From<SettingsStoreError> for SettingsBundleError {
    fn from(e: SettingsStoreError) -> Self {
        Self::Store(e)
    }
}

impl From<SettingsFileError> for SettingsBundleError {
    fn from(e: SettingsFileError) -> Self {
        Self::File(e)
    }
}

/// Declarative configuration for the persistence services an app
/// wants installed.
///
/// ```no_run
/// use fern_settings::SettingsBundle;
/// use std::time::Duration;
///
/// let bundle = SettingsBundle::new()
///     .with_window_state(true)
///     .with_debounce(Duration::from_millis(250));
/// ```
#[derive(Debug, Clone)]
pub struct SettingsBundle {
    store_name: String,
    window_state_enabled: bool,
    debounce: Duration,
}

impl SettingsBundle {
    /// Default bundle: opens the K/V store under `general.toml`,
    /// no window-state persistence.
    pub fn new() -> Self {
        Self {
            store_name: "general".into(),
            window_state_enabled: false,
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    /// Override the K/V store filename (without `.toml`). Default: `general`.
    pub fn with_store_name(mut self, name: impl Into<String>) -> Self {
        self.store_name = name.into();
        self
    }

    /// Enable the window-state service. The service stores
    /// per-`label` entries, so a multi-window app records each
    /// window's geometry under its own label (e.g. `"main"`,
    /// `"log"`, `"inspector"`).
    pub fn with_window_state(mut self, enabled: bool) -> Self {
        self.window_state_enabled = enabled;
        self
    }

    /// Override the debounce window for all services in this bundle.
    pub fn with_debounce(mut self, delay: Duration) -> Self {
        self.debounce = delay;
        self
    }

    pub fn store_name(&self) -> &str {
        &self.store_name
    }

    pub fn debounce(&self) -> Duration {
        self.debounce
    }

    /// Open every requested service against `paths`.
    pub fn open(self, paths: &AppPaths) -> Result<OpenedSettings, SettingsBundleError> {
        let store = SettingsStore::open_with_delay(
            paths.config_file(&self.store_name),
            self.debounce,
        )?;
        let window_state = if self.window_state_enabled {
            Some(WindowStateService::open_with_delay(paths, self.debounce)?)
        } else {
            None
        };
        Ok(OpenedSettings {
            store,
            window_state,
        })
    }
}

impl Default for SettingsBundle {
    fn default() -> Self {
        Self::new()
    }
}

/// The outcome of [`SettingsBundle::open`]: ready-to-register handles.
///
/// `Clone` is cheap and **shared, not deep**. Each contained service
/// is internally `Rc<>`-shaped (matching `ListModel<T>` / `TreeModel<T>`
/// / `Signal<T>`); cloning produces a second handle to the same
/// in-memory state and the same shared I/O thread queue. Mutations
/// through any clone are visible to every clone, and `flush_all` /
/// `Drop` semantics are unchanged.
#[derive(Debug, Clone)]
pub struct OpenedSettings {
    pub store: SettingsStore,
    pub window_state: Option<WindowStateService>,
}

impl OpenedSettings {
    /// Synchronously flush every active service.
    pub fn flush_all(&self) -> Result<(), SettingsBundleError> {
        self.store.flush_now()?;
        if let Some(w) = &self.window_state {
            w.flush_now()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn empty_bundle_opens_only_store() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let opened = SettingsBundle::new()
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        assert!(opened.window_state.is_none());
    }

    #[test]
    fn full_bundle_opens_window_state() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let opened = SettingsBundle::new()
            .with_window_state(true)
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        assert!(opened.window_state.is_some());
    }

    #[test]
    fn store_name_overrides_path() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let opened = SettingsBundle::new()
            .with_store_name("editor")
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        assert_eq!(opened.store.path(), paths.config_file("editor"));
    }
}
