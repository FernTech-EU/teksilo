# Settings & Persisted State Reference

Bastyde's persistence layer (`bastyde-settings`) is **reactive end-to-end**:
disk values live as `Signal<T>`s and `ListModel<T>`s, mutating either
the in-memory handle or the underlying file flows to the other side
automatically. There is no separate "config object" you remember to
write back; the in-memory state *is* the source of truth, and disk is
a debounced atomic projection of it.

Mental model in one line:

```
SettingsBundle → OpenedSettings → app_state registry → SettingsExt accessors → reactive widgets
```

Three persistence shapes share one storage backbone:

| Shape | Type | Use for |
|---|---|---|
| Dynamic K/V | [`SettingsStore`](../crates/bastyde-settings/src/store.rs) → `Signal<T>` | Scalar prefs (font size, theme name, bools, arrays of scalars) |
| Typed file | [`SettingsFile<T>`](../crates/bastyde-settings/src/file.rs) | App-shaped structs with their own schema + migrations |
| Reactive collection | [`PersistedListModel<T>`](../crates/bastyde-settings/src/collection/list.rs) / [`PersistedTreeModel<T>`](../crates/bastyde-settings/src/collection/tree.rs) | Recents, palettes, custom hierarchies — anything that drives a `Repeater` / `ListView` / `TreeView` |

Two built-in services are layered on those primitives:

| Service | Backed by | Scope |
|---|---|---|
| `MruList<T: MruEntry>` | `PersistedListModel<T>` | Generic dedupe + pin + cap recents over an *app-defined* item type |
| `WindowStateService` | `SettingsFile<WindowStateFile>` | Per-window labelled geometry; auto-restored and auto-saved by the framework when a `WindowConfig` carries an `id(...)` |

End-to-end example:
[`examples/recent_projects`](../examples/recent_projects/src/main.rs).

---

## Canonical app shape

```rust
use bastyde::prelude::*;
use bastyde::app::BastydeAppBuilder;
use bastyde::settings::{AppPaths, MruList, SettingsBundle};

fn main() {
    let paths = AppPaths::new("com", "FernTech", "Bastyde")
        .expect("could not resolve OS config directory");

    // App-typed MRU list — the framework knows nothing about projects.
    let recents: MruList<RecentProject> =
        MruList::open(&paths, "recent_projects", 10).unwrap();

    BastydeAppBuilder::new()
        .theme(intui::light())
        .app_paths(paths)                                // explicit
        // or .application("com", "FernTech", "Bastyde") // shortcut
        .settings(
            SettingsBundle::new()
                .with_window_state(true),                // opt-in
        )
        .app_state(recents)                              // register MRU
        .initial_window(
            WindowConfig::new()
                .id("main")                              // <- enables auto save/restore
                .title("Bastyde")
                .size(1200, 800)
                .min_size(640, 400)
                .root(|tree, _state| tree.add(AppRoot::new())),
        )
        .run();
}
```

Notes:

- `app_paths` *or* `application` must be set **and** `settings` must
  be configured before `run()` / `build_headless()` is called — the
  bundle has nowhere to write without a directory, and the runtime
  panics if `settings(...)` was used but no paths were configured.
  (The order of the builder calls themselves doesn't matter; the
  builder just stores fields.)
- The bundle only opens the K/V store and (optionally) the
  `WindowStateService`. Apps register their own `MruList<T>`,
  `SettingsFile<T>`, etc. via `.app_state(handle)`.
- Auto-save / auto-restore of window geometry is enabled by
  `.id("main")` on `WindowConfig` plus `.with_window_state(true)` on
  the bundle. No widget-side wiring needed.

---

## Why three shapes, not one

The clean shapes lose information when you collapse them.

- **`SettingsStore`** is for *scalars*. Behind the scenes the file is
  a TOML map with dotted keys (`editor.font_size`, `ui.theme`). Struct
  values aren't supported because TOML serializes them as tables —
  indistinguishable on a re-read from "a parent of nested keys."
  Apps that need struct persistence go through `SettingsFile<T>`.
- **`SettingsFile<T>`** owns one struct per file with its own schema
  and `Versioned` impl. The migration story (raw `toml::Value`
  transformations registered as `from → from + 1` steps) lives here.
- **`PersistedListModel<T>`** and **`PersistedTreeModel<T>`** wrap a
  reactive `*Model<T>` and re-serialize on every mutation, debounced.
  Mutating either the model or the file goes through one path; widgets
  bound to the model see incremental UI updates (`Repeater` does the
  right thing on inserts/removes/reorderings).

Don't shoehorn a list into `Signal<Vec<T>>`. A 100-row recents menu
backed by a `Vec` would full-rebuild every `Repeater` on every add;
backed by `ListModel`, only the changed range patches.

---

## `AppPaths`

Single point of truth for *where* settings live. Wraps
`directories::ProjectDirs` so the rest of the crate (and the rest of
this doc) ignores XDG / `%APPDATA%` / `~/Library/Preferences`
differences.

```rust
pub struct AppPaths { /* private */ }

impl AppPaths {
    pub fn new(qualifier: &str, organization: &str, application: &str)
        -> Option<Self>;
    pub fn for_testing(root: &Path) -> Self;
    pub fn from_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self;

    pub fn config_dir(&self) -> &Path;
    pub fn data_dir(&self) -> &Path;
    pub fn config_file(&self, name: &str) -> PathBuf;  // <config>/<name>.toml
    pub fn data_file(&self, name: &str) -> PathBuf;    // <data>/<name>.toml
}
```

`new(...)` returns `Option` because OS path resolution can fail
(sandboxed CI, missing `HOME`). `BastydeAppBuilder::application(...)`
panics with a clear message in that case; production apps that want
to fall back to a portable directory use the `Option` directly:

```rust
let paths = AppPaths::new("com", "FernTech", "Bastyde")
    .or_else(|| {
        let cwd = std::env::current_dir().ok()?;
        Some(AppPaths::for_testing(&cwd.join(".bastyde-state")))
    })
    .expect("no usable directory");
```

`for_testing` is the canonical test path — every test in this crate
calls it against a `tempdir()`, never against the user's real config
tree. Tests that consult production `ProjectDirs` would pollute
`~/.config` and would be non-hermetic on CI.

---

## `SettingsBundle` and `OpenedSettings`

Declarative configuration for the framework integration.

```rust
let bundle = SettingsBundle::new()           // store only
    .with_store_name("general")              // → general.toml
    .with_window_state(true)                 // → window_state.toml
    .with_debounce(Duration::from_millis(500));

let opened: OpenedSettings = bundle.open(&paths)?;
// opened.store: SettingsStore
// opened.window_state: Option<WindowStateService>
```

`OpenedSettings` is a cheap-to-clone handle bundle. Each contained
service is `Rc<>`-shaped internally; cloning produces a second handle
to the same in-memory state and the same shared I/O thread queue.
`BastydeAppBuilder::run` keeps one `OpenedSettings` on the stack while
clones of each service live in the `app_state` registry — when the
registry is dropped at exit, the `Drop` impls flush every pending
payload synchronously.

`flush_all()` is the explicit form for tests and pre-fork scenarios.

---

## `SettingsStore` — dynamic K/V scalars

The QSettings analogue. Dotted keys, types chosen at the call site,
backing `Signal<T>` cached by key.

```rust
pub const FONT_SIZE: SettingsKey<f32> =
    SettingsKey::new("editor.font_size", || 14.0);

let store = SettingsStore::open(paths.config_file("general"))?;
let size = store.signal_for(&FONT_SIZE);     // first call seeds + caches
size.set(18.0);                              // schedules debounced flush
let same = store.signal_for(&FONT_SIZE);     // second call returns the same Signal
assert_eq!(same.get(), 18.0);
```

Invariants enforced at registration:

- *Type stability.* Once a key is registered as `f32`, calling
  `signal::<i32>` on the same key panics. Settings are programmer-
  named; type drift is a code bug surfaced immediately.
- *Path-shape collisions.* `"editor.font_size"` cannot coexist with
  `"editor"` as a leaf value, in either order. Both directions panic
  at the call site that creates the conflict.
- *Struct rejection.* `signal::<MyStruct>` panics — see "Why three
  shapes" above. Use `SettingsFile<MyStruct>`.

**Cycle-free observer.** The closure each key installs to write
mutations back into the in-memory `toml::Value` captures
`Weak<RefCell<StoreInner>>`, never a strong `Rc`: a strong capture
would trap the entire store inside its own observer and leak it for
the life of the process. The
`weak.upgrade().is_none()` early-return also gives correct teardown
semantics — in-flight signal sets after a store drop bail silently.

---

## Built-in: `WindowStateService`

Per-window labelled geometry, fully framework-driven.

A window participates in auto save / restore when:

1. Its `WindowConfig` carries an `id(...)` (a stable string label), **and**
2. A `WindowStateService` is registered (i.e., `SettingsBundle::with_window_state(true)`).

That naturally excludes modal dialogs, popovers, and any transient
surface that doesn't ask for an id. Multi-window apps just give each
window a different id (`"main"`, `"log"`, `"inspector"`); the service
stores them under their own keys and the manager round-trips each
independently.

### What round-trips

```rust
pub struct PerWindowState {
    pub label: String,
    pub x: i32, pub y: i32,
    pub width: u32, pub height: u32,
    pub placement: WindowPlacement,   // Floating | Maximized | Fullscreen | Minimized
}
```

- **Size**: honored on every platform.
- **Position**: honored on X11, macOS, Windows. Wayland ignores
  position by design — see "Wayland caveat" below.
- **Placement**: `Floating`, `Maximized`, `Fullscreen` round-trip
  exactly. `Minimized` is downgraded to `Floating` on restore — a
  window that comes back invisible looks like the app failed to start.

### Restoration: the sanitize step

What happens if the saved coordinate is for a monitor that's no longer
connected? The framework runs every saved entry through
`PerWindowState::sanitize(min_size, work_area)` before applying it:

- **Width / height** are clamped to `[min, work_area]`. A 4K saved
  size on a 1080p screen comes back as 1920×1080.
- **Position is checked per-axis** against a 50-pixel intersection
  test with the work area. A window saved at `x=2200, y=100` on a
  now-disconnected secondary monitor recenters its `x` to the
  primary's middle while *keeping* `y=100` (it was always on-screen
  vertically). A window with `x=-2000, y=-2000` recenters both axes.
- The original on-disk state is untouched, so re-plugging the second
  monitor restores the original geometry on the next launch.

The work-area hint is pulled from `winit`'s
`ActiveEventLoop::primary_monitor().size().to_logical(scale_factor)`,
falling back to `(1920, 1080)` on hosts where no monitor handle is
reachable (headless, wired-only).

### Wayland caveat

Wayland's xdg-shell protocol does **not** let an application choose
its own window position. The compositor (Mutter, KWin, sway,
Hyprland) is the sole authority — by design, for security and tiling
reasons. Concretely:

- `winit::Window::set_outer_position(...)` silently no-ops on
  Wayland; `outer_position()` returns `Err(NotSupportedError)`.
- The position observer in [`window_persist.rs`](../crates/bastyde-app/src/window_persist.rs)
  almost never fires on Wayland because the compositor doesn't
  notify apps of their position.
- `WindowState.position` keeps whatever value we initialized it with.

The framework persists `(x, y)` regardless because the saved value is
*portable storage* — useful when the same config roams to an X11
session. On Wayland itself, compositors with per-app placement
memory (KWin's window rules, sway's `for_window`, GNOME's heuristic
stickiness) match windows by their Wayland `app_id` (typically
derived from the binary name by winit), not by anything bastyde-app
wires from `WindowConfig::id(...)` — that string is purely an
internal lookup key for `find_window` and the persistence service.
The result for users is fine on Wayland: the compositor remembers
placement at *its* layer, the framework remembers placement at
*ours*, and on a switch back to X11 / Windows / macOS the saved
coordinates apply.

### v1 → v2 migration

`PerWindowState` originally stored a single `maximized: bool`. v2
replaces it with the full `WindowPlacement` enum so Fullscreen
round-trips properly. The migrator
([`window_state.rs:164-188`](../crates/bastyde-settings/src/window_state.rs))
converts each entry's `maximized: true` to `placement = "Maximized"`,
otherwise `"Floating"`. Files are upgraded transparently on first
read; the new shape is written back on the next mutation.

---

## `MruList<T: MruEntry>` — generic recents

Apps define their own item type implementing `MruEntry`; the framework
provides dedupe, pin-aware cap, and persistence.

```rust
use std::path::{Path, PathBuf};
use bastyde::settings::{MruEntry, MruList};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct RecentProject {
    path: PathBuf,
    display_name: String,
    last_opened: u64,
    pinned: bool,
}

impl MruEntry for RecentProject {
    type Key = Path;
    fn key(&self) -> &Path { &self.path }
    fn is_pinned(&self) -> bool { self.pinned }
    fn set_pinned(&mut self, p: bool) { self.pinned = p; }
    fn touch(&mut self) { self.last_opened = now_unix(); }
}
```

`Key` is `?Sized`-friendly: unsized keys like `Path` and `str` work
because `key()` returns a reference and equality is performed on the
dereferenced value.

```rust
let mru: MruList<RecentProject> =
    MruList::open(&paths, "recent_projects", 10)?;

// Bound to UI:
let model = mru.model().clone();   // ListModel<RecentProject>

// Mutations:
mru.add(RecentProject::new(path, name));   // dedupes by key, prepends, caps
mru.touch(&path);                          // re-marks as freshly used
mru.toggle_pin(&path);                     // pinned entries don't count toward cap
mru.remove(&path);
mru.clear();
```

The cap policy: only **unpinned** entries count. Pinning a tenth
entry in a 10-cap list doesn't evict anything; pinning eight entries
in a 5-cap list keeps all eight (they're never evicted) plus up to
five unpinned. A re-add of a previously-pinned key *preserves* the
pin even if the new value didn't ask for it.

Bind to a list via `Repeater` (this is what the demo does):

```rust
ctx.add(Repeater::new(
    mru.model().clone(),
    |_idx, project: &RecentProject| {
        let path = project.path.clone();
        Box::new(
            Button::new(lit!(project.display_name.clone()))
                .on_activate_fn(move |ctx| {
                    ctx.send_intent(AppIntent::OpenRecent(path.clone()));
                }),
        )
    },
));
```

Driving a `MenuList` from an `MruList` is also a reasonable pattern,
but `MenuList`'s builder takes `MenuItem`s by value through `.item(...)`
rather than wrapping a single child widget — adapting a `Repeater` to
that shape needs an extra "rebuild on `ListModel` change" indirection
that's outside this doc's scope.

---

## `SettingsExt` accessors

A single extension trait on `BuildContext` and `EventContext`.

```rust
use bastyde::settings::SettingsExt;

// Inside any handler / build method:
let store = ctx.settings();                     // panics if not registered
let store_opt = ctx.try_settings();             // Option<&SettingsStore>

let recents = ctx.mru::<RecentProject>();       // panics if not registered
let recents_opt = ctx.try_mru::<RecentProject>();

let svc = ctx.window_state();                   // panics if not registered
let svc_opt = ctx.try_window_state();
```

Each accessor wraps the existing `app_state::<T>()` lookup. Mandatory
forms panic with a clear message that names the missing service and
the call to register it; `try_*` variants return `Option`.

**Window-geometry persistence is not an extension method.** When a
`WindowStateService` is registered, every `WindowConfig` carrying an
`id(...)` is automatically restored on creation and recorded on every
change by `bastyde-app`'s window manager. No `ctx.persist_window_state(...)`
call needed.

---

## Migrations

Every persisted struct carries a `version: u32` (via the `Versioned`
trait). Migrations operate on raw `toml::Value` *before* deserialize,
so a v1 file that no longer matches the v2 type can still be upgraded:

```rust
use bastyde::settings::{Migrator, Versioned};

#[derive(Serialize, Deserialize, Default)]
struct Recents {
    version: u32,
    items: Vec<Entry>,
}

impl Versioned for Recents {
    const CURRENT_VERSION: u32 = 2;
    fn version(&self) -> u32 { self.version }
    fn set_version(&mut self, v: u32) { self.version = v; }
}

let migrator: Migrator<Recents> = Migrator::new()
    .step(1, |mut v| {
        // v1 had no `pinned` field; default to false.
        if let Some(items) = v.get_mut("items").and_then(|i| i.as_array_mut()) {
            for item in items {
                if let Some(t) = item.as_table_mut() {
                    t.insert("pinned".into(), toml::Value::Boolean(false));
                }
            }
        }
        Ok(v)
    });
```

`Migrator::run` reads the version directly from the raw value's
`version` field (defaulting to v1 if missing) before any deserialize
attempt, walks registered steps in order, and stamps each intermediate
result with the new version so subsequent steps see a coherent
struct. A file *newer* than `CURRENT_VERSION` returns
`MigrationError::NewerThanCurrent` rather than risk silent
corruption — this lets a downgraded build refuse to read forward
state.

Corrupt files (parse failure, missing migration, post-migration
deserialize failure) are renamed to `<path>.broken-<unix_ts>` and the
`SettingsFile` falls back to `T::default()` so the app keeps running.
Apps that want the strict alternative (errors propagate, no fallback)
use `SettingsFile::load_strict`.

---

## Atomic write + debounce

Every flush goes through write-temp + rename via `tempfile`. Writes
are debounced through a single shared I/O thread (one
`OnceLock<Sender<PoolMsg>>` per process). Each writer holds a
`WriterId`, and `Drop` synchronously flushes its pending payload via
the `Unregister` ack so end-of-process state is never lost.

```rust
use bastyde::settings::DebouncedWriter;
use std::time::Duration;

let w = DebouncedWriter::new(path, Duration::from_millis(500));
w.schedule("v = 1\n".into());     // resets deadline to now + 500ms
w.schedule("v = 2\n".into());     // replaces, resets again
// ... 500 ms later: v = 2 hits disk atomically.
w.flush_now()?;                   // synchronous force-flush
```

`Duration::ZERO` makes every schedule flush on the worker's next
iteration — useful for tests, where `flush_now()` is the
deterministic anchor.

Application logic stays single-threaded; only the atomic write
happens on the worker. `Signal<T>::observe` callbacks fire on the UI
thread as ever — the path from `signal.set(v)` to `tx.send(...)` is
cheap and synchronous.

---

## Threading and source-of-truth

`Signal<T>` and `*Model<T>` use `Rc<RefCell<>>`; the settings store
inherits that. **In-memory is the source of truth.** Disk is a
flushed projection: load once at startup, write on change. Widgets
never read from disk.

This is why `OpenedSettings: Clone` is a *shared* clone, not a deep
one. Cloning each contained service is an `Rc` bump; mutations
through any clone are visible to every clone.

Multi-process is **out of scope**. Two app instances writing to the
same file is last-write-wins. Single-instance apps are the target;
if a CLI tool ever wants to share config with the GUI, an advisory
`fcntl` lock around the atomic write is the path forward — file format
unchanged.

---

## Checklist for common tasks

| Task | Recipe |
|---|---|
| Add a new scalar pref | Declare a `const KEY: SettingsKey<T> = SettingsKey::new(...)`, call `ctx.settings().signal_for(&KEY)` from `build()`, bind with `.bind_text(...)` / `.bind_color(...)` etc. |
| Persist a struct | Define `struct Foo { version: u32, ... }`, `impl Versioned for Foo`, open with `SettingsFile::load(path, delay, &Migrator::new())`, register via `app_state(handle.clone())`. |
| Persist a list | Define `T: MruEntry`, `MruList::open(&paths, "name", N)`, register via `app_state(handle)`. |
| Auto-save / restore window geometry | `.settings(SettingsBundle::new().with_window_state(true))` and `.id("main")` on the `WindowConfig`. Done. |
| Add a v2 schema migration | Bump `CURRENT_VERSION`, register a `Migrator::new().step(1, ...)` transformation, plumb the migrator into `SettingsFile::load`. |
| Force a flush before a child process | `opened.flush_all()` (or per-service `flush_now()`). |
| Test settings code without touching `~/.config` | `AppPaths::for_testing(tempdir.path())` and `Duration::ZERO` for the debounce. |

---

## Reference

- Source: [`crates/bastyde-settings/src/`](../crates/bastyde-settings/src/)
- Window persist integration: [`crates/bastyde-app/src/window_persist.rs`](../crates/bastyde-app/src/window_persist.rs)
- End-to-end demo: [`examples/recent_projects/src/main.rs`](../examples/recent_projects/src/main.rs)
- Related architecture topics: [`docs/multi-window.md`](multi-window.md), [`docs/data-models.md`](data-models.md), [`docs/reactive-theme.md`](reactive-theme.md)

### Out of scope — intentional

- **Encryption.** Plaintext TOML. Secrets go through a future
  `bastyde-secrets` crate against the OS keychain.
- **Cloud sync.** No.
- **Multi-instance write coordination.** Last-write-wins, documented.
- **Large persisted collections (> ~1k items).** Use SQLite via
  `rusqlite`; the persistence bridges intentionally re-serialize
  whole on every change.
- **Per-document state.** Document state belongs in the document file
  or its sidecar, not in app settings. Same primitive
  (`SettingsFile<T>`) is reusable by app code, but no built-in
  service.
- **`QSettings::sync()`-style read-back guarantees.** `flush_all` is
  the only sync barrier; there is no per-key sync, no
  read-after-write barrier within a tick, and no cross-handle
  visibility within the same process for two stores opened on the
  same file. Apps that need any of these are using the wrong tool.
