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

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_settings/index.html)
