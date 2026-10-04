<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SettingsExt

Extension traits exposing settings services on `BuildContext` and
`EventContext`.

`teksilo-settings` cannot live below `teksilo-core` (it depends on
`teksilo-core` for `Signal`, `ObserverHandle`, etc.), so the
convenience accessors `ctx.settings()` / `ctx.window_state()` /
`ctx.mru::<T>()` ship as an extension trait that apps `use`
explicitly:

```ignore
use teksilo_settings::SettingsExt;

// inside any handler / build method:
let store = ctx.settings();
let recents = ctx.mru::<RecentProject>();
```

Each accessor wraps the existing `app_state::<T>()` lookup. The
mandatory accessors panic with a clear message if the service has
not been registered; the `try_*` variants return `Option`.

Window-geometry persistence is **not** an extension method: when a
`WindowStateService` is registered via `TeksiloAppBuilder::settings`,
every `WindowConfig` carrying an `id(...)` is automatically
restored on creation and recorded on every change by `teksilo-app`'s
window manager. No widget-side wiring needed.

## Builder methods at a glance

`settings`, `window_state`, `mru`, `try_settings`, `try_window_state`, `try_mru`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)

## `pub trait SettingsExt`

Convenience accessors for settings services attached to the app's
`app_state` registry.

```rust
pub trait SettingsExt { /* associated items below */ }
```

### Associated items

#### `fn settings(&self) -> &SettingsStore { /* default implementation */ }`

The K/V settings store. Panics if `TeksiloAppBuilder::settings(...)`
was not called.

#### `fn window_state(&self) -> &WindowStateService { /* default implementation */ }`

The window-state service. Panics if not registered.

#### `fn mru<T: MruEntry>(&self) -> &MruList<T> { /* default implementation */ }`

An app-defined MRU list. Panics if no `MruList<T>` was
registered for that exact `T` via
`TeksiloAppBuilder::app_state(mru_handle.clone())`.

#### `fn try_settings(&self) -> Option<&SettingsStore>;`

Returns the K/V settings store, or `None` if not registered.

#### `fn try_window_state(&self) -> Option<&WindowStateService>;`

Returns the window-state service, or `None` if not registered.

#### `fn try_mru<T: MruEntry>(&self) -> Option<&MruList<T>>;`

Returns the MRU list for `T`, or `None` if no `MruList<T>` was registered.
