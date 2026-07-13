// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SettingsBundle` — declarative configuration for the bastyde-app
//! integration.
//!
//! `BastydeAppBuilder::settings(bundle)` consumes a `SettingsBundle`,
//! opens the requested services against the app's [`AppPaths`], and
//! registers each one in the application's `app_state` registry so it
//! is reachable from any handler via the `SettingsExt` trait
//! (`use bastyde_settings::SettingsExt;`).
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
//! `BastydeAppBuilder::app_state(handle)`.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_settings::{AppPaths, SettingsBundle};
//! use std::time::Duration;
//!
//! let paths = AppPaths::for_testing(std::env::temp_dir());
//! let opened = SettingsBundle::new()
//!     .with_window_state(true)
//!     .with_debounce(Duration::ZERO)
//!     .open(&paths)
//!     .expect("bundle open failed");
//! // opened.store and opened.window_state are now ready to register.
//! ```

use std::rc::Rc;
use std::time::Duration;

use crate::file::SettingsFileError;
use crate::path::AppPaths;
use crate::reload::Reloadable;
use crate::store::{DEFAULT_DEBOUNCE, SettingsStore, SettingsStoreError};
use crate::watch::SettingsRegistry;
use crate::window_state::WindowStateService;

/// Errors surfaced by [`SettingsBundle::open`].
#[derive(Debug, thiserror::Error)]
pub enum SettingsBundleError {
    /// The K/V store could not be opened or flushed.
    #[error("settings bundle: {0}")]
    Store(#[from] SettingsStoreError),
    /// A settings file (e.g. the window-state file) could not be opened or flushed.
    #[error("settings bundle: {0}")]
    File(#[from] SettingsFileError),
}

/// Declarative configuration for the persistence services an app
/// wants installed.
///
/// ```
/// use bastyde_settings::SettingsBundle;
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

    /// Override the debounce window passed to every service this bundle
    /// opens.
    ///
    /// Only [`SettingsStore`] actually debounces on it — its writes are
    /// frequent enough (every `Signal::set`) that coalescing matters.
    /// [`WindowStateService`] accepts the same parameter (so `open` can
    /// call both uniformly) but ignores it: `SettingsFile`'s writes are
    /// always a synchronous locked read-modify-write now, so there is
    /// nothing left to debounce (see `file.rs`'s and `window_state.rs`'s
    /// module docs).
    pub fn with_debounce(mut self, delay: Duration) -> Self {
        self.debounce = delay;
        self
    }

    /// The filename stem (without `.toml`) used for the K/V store.
    pub fn store_name(&self) -> &str {
        &self.store_name
    }

    /// The debounce window passed to every service this bundle opens
    /// (see [`with_debounce`](Self::with_debounce) for which services
    /// actually honor it).
    pub fn debounce(&self) -> Duration {
        self.debounce
    }

    /// Open every requested service against `paths`.
    ///
    /// Every opened service is also registered into a fresh
    /// [`SettingsRegistry`] (exposed as [`OpenedSettings::registry`]) under
    /// its canonical path, so a [`crate::SettingsWatcher`] event naming
    /// that path can be dispatched straight to it. The registration
    /// handles are retained internally by `OpenedSettings` — see its
    /// field docs — so they stay alive (and thus dispatchable) for as
    /// long as the returned `OpenedSettings` (or any clone of it) is.
    pub fn open(self, paths: &AppPaths) -> Result<OpenedSettings, SettingsBundleError> {
        let store =
            SettingsStore::open_with_delay(paths.config_file(&self.store_name), self.debounce)?;
        let window_state = if self.window_state_enabled {
            Some(WindowStateService::open_with_delay(paths, self.debounce)?)
        } else {
            None
        };

        let registry = SettingsRegistry::new();
        let mut reload_handles: Vec<Rc<dyn Reloadable>> = Vec::new();
        reload_handles.push(registry.register(Rc::new(store.clone())));
        if let Some(window_state) = &window_state {
            reload_handles.push(registry.register(Rc::new(window_state.clone())));
        }

        Ok(OpenedSettings {
            store,
            window_state,
            registry,
            reload_handles,
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
#[derive(Clone)]
pub struct OpenedSettings {
    pub store: SettingsStore,
    pub window_state: Option<WindowStateService>,
    /// Registry mapping every managed service's canonical path to its
    /// live [`Reloadable`] handle, so a [`crate::SettingsWatcher`] event
    /// can be dispatched to the right one. Exposed so application code
    /// can register its own ad hoc [`crate::SettingsFile`] /
    /// [`crate::PersistedListModel`] / [`crate::MruList`] handles into
    /// the same registry (reachable elsewhere via
    /// `ctx.app_state::<SettingsRegistry>()` once installed).
    pub registry: SettingsRegistry,
    /// Strong references backing `registry`'s `Weak` entries for
    /// `store` / `window_state`. `SettingsRegistry::register` only ever
    /// keeps a `Weak` — these are what keep that `Weak` upgradeable for
    /// as long as this `OpenedSettings` (or any clone of it) is alive.
    /// Never read after construction; kept only for its `Drop`.
    reload_handles: Vec<Rc<dyn Reloadable>>,
}

impl std::fmt::Debug for OpenedSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedSettings")
            .field("store", &self.store)
            .field("window_state", &self.window_state)
            .field("registry", &self.registry)
            .field("reload_handles", &self.reload_handles.len())
            .finish()
    }
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

    // -----------------------------------------------------------------
    // Live cross-process reload wiring: `OpenedSettings::registry`.
    // -----------------------------------------------------------------

    const NAME: crate::store::SettingsKey<String> =
        crate::store::SettingsKey::new("user.name", String::new);

    #[test]
    fn opened_settings_registry_dispatches_a_peers_store_write() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let mine = SettingsBundle::new()
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        let name_signal = mine.store.signal_for(&NAME);
        assert_eq!(name_signal.get(), "");

        // A second process opening the same store file and writing a
        // value — standing in for a peer, exactly like
        // `two_shared_mode_services_over_one_file_do_not_clobber_each_others_overrides`.
        let peer = SettingsBundle::new()
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        peer.store.signal_for(&NAME).set("peer-name".to_string());
        peer.flush_all().unwrap();

        let changed = mine.store.path().to_path_buf();
        assert!(mine.registry.dispatch(&changed).unwrap());
        assert_eq!(name_signal.get(), "peer-name");
    }

    #[test]
    fn opened_settings_registry_dispatches_a_peers_window_state_write() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let mine = SettingsBundle::new()
            .with_window_state(true)
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        let window_state = mine.window_state.clone().unwrap();
        assert!(window_state.state_for("main").is_none());

        let peer = SettingsBundle::new()
            .with_window_state(true)
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        peer.window_state
            .as_ref()
            .unwrap()
            .record(crate::window_state::PerWindowState {
                label: "main".into(),
                x: 10,
                y: 20,
                width: 800,
                height: 600,
                placement: Default::default(),
            })
            .unwrap();
        // `record` is debounced (it fires once per frame during a window drag),
        // so push the peer's write out before asking the registry to pick it up.
        peer.window_state.as_ref().unwrap().flush_now().unwrap();

        let changed = window_state.path().to_path_buf();
        assert!(mine.registry.dispatch(&changed).unwrap());
        let restored = window_state.state_for("main").unwrap();
        assert_eq!(restored.width, 800);
        assert_eq!(restored.height, 600);
    }

    /// Dropping every clone of an `OpenedSettings` must deregister its
    /// services from the (separately held) registry — proving the
    /// `reload_handles` field actually backs the `Weak` entries, not
    /// just a documentation claim.
    #[test]
    fn dropping_opened_settings_deregisters_its_services() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let opened = SettingsBundle::new()
            .with_window_state(true)
            .with_debounce(Duration::ZERO)
            .open(&paths)
            .unwrap();
        let registry = opened.registry.clone();
        assert_eq!(registry.live_count(), 2, "store + window_state");

        drop(opened);

        assert_eq!(
            registry.live_count(),
            0,
            "dropping every OpenedSettings clone must drop its reload_handles too"
        );
    }
}
