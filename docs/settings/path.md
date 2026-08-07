<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# AppPaths

OS-correct path resolution for application config and data directories.

`AppPaths` wraps `etcetera`'s native `AppStrategy` so the rest of the
crate has a single point of truth for where settings files live. In
production, `AppPaths::new` queries the OS (XDG on Linux,
`%APPDATA%` on Windows, `~/Library/Application Support` on macOS); in
tests, `AppPaths::for_testing` roots everything inside a `tempdir` so no
test ever touches the user's real config tree.

## Usage

Pass an `AppPaths` instance to `SettingsBundle`,
`SettingsStore`, or `MruList`; the
individual files within it are addressed by name via
`config_file` and
`data_file`.

```ignore
use teksilo_settings::AppPaths;

// Production: returns None when no home directory is detectable.
if let Some(paths) = AppPaths::new("eu", "FernTech", "MyApp") {
    let general_toml = paths.config_file("general");
    let cache_toml   = paths.data_file("cache");
}

// Tests: deterministic, tempdir-rooted, never touches user files.
let tmp = tempfile::tempdir().unwrap();
let paths = AppPaths::for_testing(tmp.path());
assert_eq!(paths.config_file("settings"), tmp.path().join("settings.toml"));
```

## Builder methods at a glance

`for_testing`, `from_dirs`, `config_dir`, `data_dir`, `config_file`, `data_file`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_settings/index.html)

## `pub struct AppPaths`

Resolved OS-correct application directories (config and data).

Construct with `AppPaths::new` for production code, or
`AppPaths::for_testing` in tests and headless CI environments.
Use `AppPaths::from_dirs` when the application manages its own
directory layout (e.g. portable mode).

```rust
pub struct AppPaths { /* fields */ }
```

### Methods

#### `pub fn new(qualifier: &str, organization: &str, application: &str) -> Option<Self>`

Resolve directories from the OS. The `(qualifier, organization,
application)` triple feeds `etcetera::AppStrategyArgs` as
`(top_level_domain, author, app_name)` — same fields, different
names — and selects the platform-native strategy (XDG on Linux,
`%APPDATA%`-based on Windows, `~/Library/Application Support`
on macOS).

Returns `None` when no usable home directory could be detected
(a sandboxed or unconfigured environment). Callers who want to
degrade gracefully should fall back to `AppPaths::for_testing`
with an in-process directory.

#### `pub fn for_testing(root: &Path) -> Self`

Construct an `AppPaths` rooted at an arbitrary directory. Used by
tests so that no test ever touches the user's real config tree.
Both `config_dir` and `data_dir` resolve to `root`.

#### `pub fn from_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self`

Construct from explicit config and data directories. Useful when
an application wants to override one or both (e.g. portable mode).

#### `pub fn config_dir(&self) -> &Path`

The platform-correct config directory (XDG_CONFIG_HOME, %APPDATA%,
`~/Library/Preferences`, etc.).

#### `pub fn data_dir(&self) -> &Path`

The platform-correct data directory. Used for caches and
per-window state — anything larger than a configuration file.

#### `pub fn config_file(&self, name: &str) -> PathBuf`

Resolve a per-concern config file by name (without extension).
`name = "general"` yields `<config_dir>/general.toml`.

#### `pub fn data_file(&self, name: &str) -> PathBuf`

Resolve a per-concern data file by name (without extension).
