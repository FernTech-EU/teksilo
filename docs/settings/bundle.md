<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SettingsBundleError

`SettingsBundle` — declarative configuration for the bastyde-app
integration.

`BastydeAppBuilder::settings(bundle)` consumes a `SettingsBundle`,
opens the requested services against the app's `AppPaths`, and
registers each one in the application's `app_state` registry so it
is reachable from any handler via the `SettingsExt` trait
(`use bastyde_settings::SettingsExt;`).

## What's in the bundle

Only services the framework can construct without app-level type
information:

* `SettingsStore` — the dynamic K/V store for scalar settings.
* `WindowStateService` — per-window geometry persistence (opt-in
  via `with_window_state`).

Anything that needs an app-defined item type (recently-opened
projects/files, color palettes, saved searches) is **not** in the
bundle. Apps construct an `MruList<T>` for each
such collection and register it themselves via
`BastydeAppBuilder::app_state(handle)`.

## Example

```ignore
use bastyde_settings::{AppPaths, SettingsBundle};
use std::time::Duration;

let paths = AppPaths::for_testing(std::env::temp_dir());
let opened = SettingsBundle::new()
    .with_window_state(true)
    .with_debounce(Duration::ZERO)
    .open(&paths)
    .expect("bundle open failed");
// opened.store and opened.window_state are now ready to register.
```

## Builder methods at a glance

`with_store_name`, `with_window_state`, `with_debounce`, `store_name`, `debounce`, `open`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub enum SettingsBundleError`

Errors surfaced by `SettingsBundle::open`.

```rust
pub enum SettingsBundleError { /* variants */ }
```

### Variants

- **`Store`** — The K/V store could not be opened or flushed.
- **`File`** — A settings file (e.g. the window-state file) could not be opened or flushed.

## `pub struct SettingsBundle`

Declarative configuration for the persistence services an app
wants installed.

```
use bastyde_settings::SettingsBundle;
use std::time::Duration;

let bundle = SettingsBundle::new()
    .with_window_state(true)
    .with_debounce(Duration::from_millis(250));
```

```rust
pub struct SettingsBundle { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Default bundle: opens the K/V store under `general.toml`,
no window-state persistence.

#### `pub fn with_store_name(mut self, name: impl Into<String>) -> Self`

Override the K/V store filename (without `.toml`). Default: `general`.

#### `pub fn with_window_state(mut self, enabled: bool) -> Self`

Enable the window-state service. The service stores
per-`label` entries, so a multi-window app records each
window's geometry under its own label (e.g. `"main"`,
`"log"`, `"inspector"`).

#### `pub fn with_debounce(mut self, delay: Duration) -> Self`

Override the debounce window passed to every service this bundle
opens.

Only `SettingsStore` actually debounces on it — its writes are
frequent enough (every `Signal::set`) that coalescing matters.
`WindowStateService` accepts the same parameter (so `open` can
call both uniformly) but ignores it: `SettingsFile`'s writes are
always a synchronous locked read-modify-write now, so there is
nothing left to debounce (see `file.rs`'s and `window_state.rs`'s
module docs).

#### `pub fn store_name(&self) -> &str`

The filename stem (without `.toml`) used for the K/V store.

#### `pub fn debounce(&self) -> Duration`

The debounce window passed to every service this bundle opens
(see `with_debounce` for which services
actually honor it).

#### `pub fn open(self, paths: &AppPaths) -> Result<OpenedSettings, SettingsBundleError>`

Open every requested service against `paths`.

Every opened service is also registered into a fresh
`SettingsRegistry` (exposed as `OpenedSettings::registry`) under
its canonical path, so a `crate::SettingsWatcher` event naming
that path can be dispatched straight to it. The registration
handles are retained internally by `OpenedSettings` — see its
field docs — so they stay alive (and thus dispatchable) for as
long as the returned `OpenedSettings` (or any clone of it) is.

## `pub struct OpenedSettings`

The outcome of [`SettingsBundle::open`]: ready-to-register handles.

`Clone` is cheap and **shared, not deep**. Each contained service
is internally `Rc<>`-shaped (matching `ListModel<T>` / `TreeModel<T>`
/ `Signal<T>`); cloning produces a second handle to the same
in-memory state and the same shared I/O thread queue. Mutations
through any clone are visible to every clone, and `flush_all` /
`Drop` semantics are unchanged.

```rust
pub struct OpenedSettings { /* fields */ }
```

### Methods

#### `pub fn flush_all(&self) -> Result<(), SettingsBundleError>`

Synchronously flush every active service.
