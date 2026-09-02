<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SettingsReloadSink

Live cross-process settings sync: a `notify`-based directory watcher
plus the registry that lets a changed path be dispatched to the
in-memory `Reloadable` handle that owns it.

This is the read-side counterpart to the write-side cross-process
safety documented in `flush.rs` / `reload.rs`: every write in this
crate already merges safely against a peer's concurrent write, but a
process that never looks again will not notice a peer's write until
it happens to touch the same key itself. `SettingsWatcher` is what
makes it look again, automatically, the moment a peer's write lands
on disk.

## Shape, mirrored from `teksilo-i18n`'s `FtlFileWatcher`

`SettingsWatcher` owns a `notify::RecommendedWatcher` background
thread and a type-erased sink `Arc<dyn Fn(PathBuf) + Send + Sync>`.
Exactly like `FtlFileWatcher`, it watches **directories**, not files:
atomic writers (this crate's own `write_atomic` included) write a
temp file and rename it over the target, which invalidates an
inode-level watch on the file itself. Unlike `FtlFileWatcher` — which
watches a fixed, already-existing set of `.ftl` files and derives
their parents — `SettingsWatcher` watches the settings *directories*
(`AppPaths::config_dir()` / `AppPaths::data_dir()`) directly, because
the set of settings files living there is open-ended and some of
them (e.g. `window_state.toml`) may not exist yet at watch-construction
time.

The sink receives the changed path (not yet filtered against anything
this process cares about); `SettingsRegistry::dispatch` is what
decides whether the path names something registered and, if so,
calls its `Reloadable::reload_from_disk`. A path with no registered
owner (a `.lock` sidecar, a `.tmp` write-in-progress, an unrelated
file a peer dropped in the same directory) is a harmless no-op.

## The registry

`SettingsRegistry` maps a canonical path to a `Weak<dyn Reloadable>`.
It never holds a strong reference itself: whoever opens a persisted
service (`SettingsBundle::open`, or application code opening its own
ad hoc `SettingsFile<T>` / `PersistedListModel<T>` / `MruList<T>`)
wraps it in an `Rc<dyn Reloadable>`, registers a weak clone via
`SettingsRegistry::register`, and keeps the returned `Rc` alive for
as long as it wants peer writes to be picked up. When that `Rc` (and
every clone of it) is dropped, the registry's entry can no longer be
upgraded — `SettingsRegistry::dispatch` then quietly prunes it and
reports nothing happened. Nothing leaks and nothing is ever called on
a service that no longer exists.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)

## `pub type SettingsReloadSink`

Sink type invoked on the notify worker thread whenever a watched
settings directory reports a create/modify event. Implementations
must be thread-safe; `teksilo-app`'s implementation posts the path
through the winit `EventLoopProxy` as `AppEvent::SettingsReload`,
which hops back onto the UI thread where the (single-threaded,
`Rc`-based) `SettingsRegistry` actually lives.

```rust
pub type SettingsReloadSink = Arc<dyn Fn(PathBuf) + Send + Sync + 'static>;
```

## `pub struct SettingsWatcher`

Active directory watcher over one or more settings directories. One
per `TeksiloAppBuilder::run` invocation (when a settings bundle with
watching enabled is configured).

Owns the `notify::RecommendedWatcher` background thread for its whole
lifetime; dropping the `SettingsWatcher` stops the watcher and cleans
up. Kept alive by the caller for as long as live reload is wanted —
`teksilo-app` stores it on its window-loop handler, exactly like
`teksilo-i18n`'s `FtlFileWatcher`.

```rust
pub struct SettingsWatcher { /* fields */ }
```

### Methods

#### `pub fn new(dirs: Vec<PathBuf>, sink: SettingsReloadSink) -> Result<Self, notify::Error>`

Build a watcher over `dirs` (deduplicated by canonical path, so
passing the same directory twice — e.g. `AppPaths::for_testing`,
whose `config_dir()` and `data_dir()` are the same tempdir — never
double-watches or double-fires) and a sink callback.

A directory that does not exist (or can't be canonicalized for
any other reason) is logged and skipped — not fatal — since a
freshly-installed app may not have created its data directory yet
when this is called. As long as at least the config directory
exists (which `AppPaths` implies by the time `SettingsBundle` has
successfully opened anything in it), watching still works for the
files that matter.

## `pub struct SettingsRegistry`

Registry mapping a canonical settings path to the live `Reloadable`
handle that owns it, so a file-watcher event naming that path can be
dispatched to the right in-memory state.

`Clone` is cheap (an `Rc` bump) — every clone shares the same
underlying map, matching the rest of this crate's handle types.
Holds only `Weak` references: see the module docs' "the registry"
section for the full ownership contract.

```rust
pub struct SettingsRegistry { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

A fresh, empty registry.

#### `pub fn register(&self, reloadable: Rc<dyn Reloadable>) -> Rc<dyn Reloadable>`

Register `reloadable` under its canonical path and return it back
unchanged, so a caller can register and retain in one expression:

```
use teksilo_settings::{SettingsRegistry, SettingsFile, Migrator, Versioned};
use serde::{Serialize, Deserialize};
use std::rc::Rc;

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
struct Prefs { version: u32 }
impl Versioned for Prefs {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}

let dir = tempfile::tempdir().unwrap();
let file: SettingsFile<Prefs> =
    SettingsFile::load(dir.path().join("prefs.toml"), Migrator::new()).unwrap();

let registry = SettingsRegistry::new();
// Keep `handle` alive for as long as reload should keep working.
let handle = registry.register(Rc::new(file.clone()));
drop(handle); // dropping it deregisters: no leak, no dangling call.
```

The caller is responsible for keeping the returned `Rc` alive —
only a `Weak` is retained internally, by design (see the module
docs). Registering a second `Reloadable` under the same canonical
path replaces the first entry.

#### `pub fn dispatch(&self, changed_path: &Path) -> Result<bool, SettingsFileError>`

Look up `changed_path`'s registered owner and call
`Reloadable::reload_from_disk` on it.

Returns `Ok(true)` if the owner's in-memory state actually
changed, `Ok(false)` if nothing needed to change (including: the
path names nothing registered, or its owner has been dropped —
in the latter case the dead entry is pruned from the map so it
doesn't accumulate forever).

#### `pub fn registered_paths(&self) -> Vec<PathBuf>`

The canonical paths currently registered (including entries whose
owner has since been dropped but not yet pruned by a `dispatch`
call). Exposed for tests and diagnostics.

#### `pub fn live_count(&self) -> usize`

Number of live (upgradeable) entries. Exposed for tests.
