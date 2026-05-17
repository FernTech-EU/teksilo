//! Extension traits exposing settings services on `BuildContext` and
//! `EventContext`.
//!
//! `bastyde-settings` cannot live below `bastyde-core` (it depends on
//! `bastyde-core` for `Signal`, `ObserverHandle`, etc.), so the
//! convenience accessors `ctx.settings()` / `ctx.window_state()` /
//! `ctx.mru::<T>()` ship as an extension trait that apps `use`
//! explicitly:
//!
//! ```ignore
//! use bastyde_settings::SettingsExt;
//!
//! // inside any handler / build method:
//! let store = ctx.settings();
//! let recents = ctx.mru::<RecentProject>();
//! ```
//!
//! Each accessor wraps the existing `app_state::<T>()` lookup. The
//! mandatory accessors panic with a clear message if the service has
//! not been registered; the `try_*` variants return `Option`.
//!
//! Window-geometry persistence is **not** an extension method: when a
//! `WindowStateService` is registered via `BastydeAppBuilder::settings`,
//! every `WindowConfig` carrying an `id(...)` is automatically
//! restored on creation and recorded on every change by `bastyde-app`'s
//! window manager. No widget-side wiring needed.

use bastyde_core::BuildContext;
use bastyde_core::widget::EventContext;

use crate::mru::{MruEntry, MruList};
use crate::store::SettingsStore;
use crate::window_state::WindowStateService;

/// Convenience accessors for settings services attached to the app's
/// `app_state` registry.
pub trait SettingsExt {
    /// The K/V settings store. Panics if `BastydeAppBuilder::settings(...)`
    /// was not called.
    fn settings(&self) -> &SettingsStore {
        self.try_settings().unwrap_or_else(|| {
            panic!(
                "SettingsExt::settings(): no SettingsStore registered. \
                 Call BastydeAppBuilder::settings(SettingsBundle::new()) at startup."
            )
        })
    }

    /// The window-state service. Panics if not registered.
    fn window_state(&self) -> &WindowStateService {
        self.try_window_state().unwrap_or_else(|| {
            panic!(
                "SettingsExt::window_state(): no WindowStateService registered. \
                 Call .with_window_state(true) on the SettingsBundle."
            )
        })
    }

    /// An app-defined MRU list. Panics if no `MruList<T>` was
    /// registered for that exact `T` via
    /// `BastydeAppBuilder::app_state(mru_handle.clone())`.
    fn mru<T: MruEntry>(&self) -> &MruList<T> {
        self.try_mru::<T>().unwrap_or_else(|| {
            panic!(
                "SettingsExt::mru::<{}>(): no MruList registered. \
                 Construct one with MruList::open(...) and register via \
                 BastydeAppBuilder::app_state(mru_handle.clone()).",
                std::any::type_name::<T>(),
            )
        })
    }

    fn try_settings(&self) -> Option<&SettingsStore>;
    fn try_window_state(&self) -> Option<&WindowStateService>;
    fn try_mru<T: MruEntry>(&self) -> Option<&MruList<T>>;
}

impl<'a> SettingsExt for BuildContext<'a> {
    fn try_settings(&self) -> Option<&SettingsStore> {
        self.app_state::<SettingsStore>()
    }
    fn try_window_state(&self) -> Option<&WindowStateService> {
        self.app_state::<WindowStateService>()
    }
    fn try_mru<T: MruEntry>(&self) -> Option<&MruList<T>> {
        self.app_state::<MruList<T>>()
    }
}

impl<'a> SettingsExt for EventContext<'a> {
    fn try_settings(&self) -> Option<&SettingsStore> {
        self.app_state::<SettingsStore>()
    }
    fn try_window_state(&self) -> Option<&WindowStateService> {
        self.app_state::<WindowStateService>()
    }
    fn try_mru<T: MruEntry>(&self) -> Option<&MruList<T>> {
        self.app_state::<MruList<T>>()
    }
}
