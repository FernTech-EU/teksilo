<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# AppPaths

OS-correct path resolution for application config and data directories.

## Public functions

### `AppPaths`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Option<Self>` | [`new(qualifier: &str, organization: &str, application: &str)`](#apppaths-new) |
| `Self` | [`for_testing(root: &Path)`](#apppaths-for_testing) |
| `Self` | [`from_dirs(config_dir: PathBuf, data_dir: PathBuf)`](#apppaths-from_dirs) |
| | **Methods** |
| `&Path` | [`config_dir()`](#apppaths-config_dir) |
| `&Path` | [`data_dir()`](#apppaths-data_dir) |
| `PathBuf` | [`config_file(name: &str)`](#apppaths-config_file) |
| `PathBuf` | [`data_file(name: &str)`](#apppaths-data_file) |
| `Option<&Path>` | [`runtime_dir()`](#apppaths-runtime_dir) |
| `io::Result<Option<&Path>>` | [`create_runtime_dir()`](#apppaths-create_runtime_dir) |

## Detailed description

`AppPaths` is the rest of the crate's single point of truth for where
settings files live. In production, `AppPaths::new` asks the OS, through
`etcetera`; in tests, `AppPaths::for_testing` roots everything inside a
`tempdir` so no test ever touches the user's real config tree.

| Platform | Config directory | Data directory |
|---|---|---|
| Linux and other Unix | `$XDG_CONFIG_HOME/<app>`, by default `~/.config/<app>` | `$XDG_DATA_HOME/<app>`, by default `~/.local/share/<app>` |
| Windows | `%APPDATA%\<Organization>\<Application>\config` | `%APPDATA%\<Organization>\<Application>\data` |
| macOS | `~/Library/Application Support/<bundle id>/config` | `~/Library/Application Support/<bundle id>/data` |

`<app>` is the application's name in lower case, and the bundle id is
`<qualifier>.<organization in lower case>.<Application>`, spaces made
hyphens in both: `AppPaths::new("eu", "FernTech", "Skribisto")` gives
`~/.config/skribisto` and `~/Library/Application Support/eu.ferntech.Skribisto`.

For sockets and lock files, `AppPaths::runtime_dir` gives a short,
per-user directory emptied at logout or restart, `$XDG_RUNTIME_DIR/<app>`
on Linux; its documentation says where it is on each platform.

#### The move to Application Support on macOS

Up to 0.21.0 a macOS application's config and data went to the Linux
directories, `~/.config/<app>` and `~/.local/share/<app>`. On macOS,
`AppPaths::new` brings each of those it finds to its new place, once:
only while the new directory does not exist yet. It is a rename where both
are on one volume, and otherwise a copy that leaves the old directory
where it was. A move that fails is reported on standard error and leaves
the old directory as it was, to be tried again at the next start.

#### Usage

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

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)

<a id="apppaths"></a>

## `pub struct AppPaths`

**Import:** `use teksilo::prelude::AppPaths;` — in `teksilo::prelude::*`.

Resolved OS-correct application directories (config, data, and the
runtime directory for sockets and locks).

Construct with `AppPaths::new` for production code, or
`AppPaths::for_testing` in tests and headless CI environments.
Use `AppPaths::from_dirs` when the application manages its own
directory layout (e.g. portable mode).

```rust
pub struct AppPaths { /* fields */ }
```

### Methods

<a id="apppaths-new"></a>

#### `pub fn new(qualifier: &str, organization: &str, application: &str) -> Option<Self>`

Resolve directories from the OS; the module documentation says where
they are on each platform. The `(qualifier, organization, application)`
triple feeds `etcetera::AppStrategyArgs` as `(top_level_domain,
author, app_name)`, the same fields under other names.

On macOS it first moves the directories it gave up to 0.21.0, under
`~/.config` and `~/.local/share`, to Application Support, when they
are there and their new places are not.

Returns `None` when no usable home directory could be detected
(a sandboxed or unconfigured environment). Callers who want to
degrade gracefully should fall back to `AppPaths::for_testing`
with an in-process directory.

<a id="apppaths-for_testing"></a>

#### `pub fn for_testing(root: &Path) -> Self`

Construct an `AppPaths` rooted at an arbitrary directory. Used by
tests so that no test ever touches the user's real config tree.
`config_dir`, `data_dir` and `runtime_dir` all resolve to `root`.

<a id="apppaths-from_dirs"></a>

#### `pub fn from_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self`

Construct from explicit config and data directories. Useful when
an application wants to override one or both (e.g. portable mode).
It has no `runtime_dir`.

<a id="apppaths-config_dir"></a>

#### `pub fn config_dir(&self) -> &Path`

The platform-correct config directory; the module documentation says
where it is on each platform.

<a id="apppaths-data_dir"></a>

#### `pub fn data_dir(&self) -> &Path`

The platform-correct data directory. Used for caches and
per-window state — anything larger than a configuration file.

<a id="apppaths-config_file"></a>

#### `pub fn config_file(&self, name: &str) -> PathBuf`

Resolve a per-concern config file by name (without extension).
`name = "general"` yields `<config_dir>/general.toml`.

<a id="apppaths-data_file"></a>

#### `pub fn data_file(&self, name: &str) -> PathBuf`

Resolve a per-concern data file by name (without extension).

<a id="apppaths-runtime_dir"></a>

#### `pub fn runtime_dir(&self) -> Option<&Path>`

A short, per-user directory for the application's sockets and lock
files, what `$XDG_RUNTIME_DIR` is on Linux. `None` where the platform
gives none: on Windows, where a named pipe needs no directory, and on
a Unix without `$XDG_RUNTIME_DIR`. It is not created here:
`create_runtime_dir` does that, private to
the user.

| Platform | Runtime directory |
|---|---|
| Linux and other Unix | `$XDG_RUNTIME_DIR/<app>`, usually `/run/user/<uid>/<app>` |
| macOS | `<per-user temporary directory>/<bundle id>`, the directory under `/var/folders/` |
| Windows | none |

Short, because a Unix socket's path has to be: macOS takes 104 bytes
of it with the closing NUL, Linux 108, and `bind` fails beyond that.
The macOS per-user temporary directory is about 49 bytes, while the
data directory under Application Support is over 80 before the user's
name is counted. On macOS it is read from the system
(`confstr(_CS_DARWIN_USER_TEMP_DIR)`), not from `$TMPDIR`, which a
shell can point at the shared `/tmp`.

What is left there goes when the user logs out or the machine
restarts, and the system may clean up what sits unused for days, so
make a socket or a lock afresh at each start.

<a id="apppaths-create_runtime_dir"></a>

#### `pub fn create_runtime_dir(&self) -> io::Result<Option<&Path>>`

Create `runtime_dir` if it does not exist, private
to the user, and return it; `Ok(None)` where there is none. On Unix it
is made `0700`, and one that exists already is taken only when it is a
directory, not a symbolic link, that the user owns; it is made `0700`
if it is not. The directory it goes in must exist, as
`$XDG_RUNTIME_DIR` and the macOS per-user temporary directory do.
