# Settings & Persisted State Plan

A QSettings-equivalent for FernUI: type-safe, reactive, dynamically-keyed
user preferences, persisted to OS-correct locations with atomic writes
and debounced flushing. Plus a parallel persistence path for
collection-shaped state (recent projects, window layouts) backed by
`ListModel<T>`/`TreeModel<T>`. One crate, two APIs, one storage backbone.

## Context

FernUI today has **no built-in settings layer**. Every example hardcodes
its theme, locale, font sizes, and shortcuts; nothing survives a restart.
`ShortcutSettings`
([`crates/fern-widgets/src/shortcut_settings.rs`](../../crates/fern-widgets/src/shortcut_settings.rs))
is a *UI* over `ShortcutRegistry` — it can rebind in memory but cannot
write the rebinds to disk.

The framework already has the right primitives to build on:

- `Signal<T>` — reactive scalar with `observe(|&T|) -> ObserverHandle`
  and `set(T)`
  ([`crates/fern-core/src/signal.rs:198-302`](../../crates/fern-core/src/signal.rs#L198-L302)).
- `ListModel<T>` / `TreeModel<T>` — reactive collections with
  `observe_changes(|&DataChange|)` emitting incremental events
  ([`crates/fern-data/src/list_model.rs`](../../crates/fern-data/src/list_model.rs),
  [`tree_model.rs`](../../crates/fern-data/src/tree_model.rs)).
- `ObserverHandle` — RAII unsubscription
  ([`crates/fern-core/src/signal.rs:26-58`](../../crates/fern-core/src/signal.rs#L26-L58)).
- `BuildContext::effect` — scoped, auto-cleaned reactive side effects
  ([`crates/fern-core/src/build_context.rs`](../../crates/fern-core/src/build_context.rs)).

These mean the *reactive* half of "settings" is already solved. What's
missing is a *persistence* half: a place to write values, a way to seed
signals from disk on startup, and a debounced flush so that
`Signal::set()` from a slider doesn't hit the filesystem on every drag
sample.

### Reference reading

- Qt `QSettings` —
  [doc.qt.io/qt-6/qsettings.html](https://doc.qt.io/qt-6/qsettings.html).
  The mental model we're emulating: dotted keys, type-erased values,
  platform-correct storage, dynamic registration.
- `directories` crate —
  [docs.rs/directories](https://docs.rs/directories). XDG /
  Known-Folders / `Application Support` resolution.
- `tempfile` crate — [docs.rs/tempfile](https://docs.rs/tempfile).
  `NamedTempFile::persist` is the atomic-rename primitive we use for
  every flush.
- Mozilla Firefox `prefs.js` model — single flat K/V store, dynamic
  registration, change notifications. Good prior art for the dynamic
  half.

## Design targets

1. **Two persistence shapes, one backbone.** Scalars/structs go through
   `SettingsStore` and surface as `Signal<T>`. Collections
   (`ListModel<T>`, `TreeModel<T>`) get their own persistence bridge —
   same disk format conventions, different in-memory primitive. Don't
   shoehorn lists into `Signal<Vec<T>>`.
2. **In-memory is the source of truth.** Disk is a flushed projection.
   Load once at startup, write on change. Widgets never read from disk.
3. **Dynamic keys, typed access.** `store.signal::<f32>("editor.font_size",
   14.0)` — caller picks the type at the call site; the store caches
   one `Signal<T>` per key. Same key from two callsites returns clones
   of the *same* signal.
4. **Atomic writes, debounced.** Every flush goes through write-temp +
   rename. A single per-store debounce timer batches all dirty keys
   into one write per ~500 ms.
5. **One file per concern.** `general.toml`, `recents.toml`,
   `shortcuts.toml`, `window_state.toml` — independent files so a parse
   error in one section doesn't poison the others, and so each domain
   can evolve its schema version independently.
6. **Migrations first-class.** Every persisted file carries
   `version: u32`. Loading runs registered migrations from
   `file_version → CURRENT_VERSION` before deserialization succeeds.
7. **Single-threaded, like the rest of the framework.** `Signal<T>`
   and `ListModel<T>` use `Rc<RefCell<>>`; the settings store inherits
   that. Disk I/O runs on the UI thread, debounced. Files are kilobytes,
   not megabytes, and the debounce window absorbs slider-drag bursts
   before any write happens. If a future profile shows a measurable
   stutter, a worker-thread flush hides behind the same `DebouncedWriter`
   interface — but we're not adding that thread on speculation.
8. **Off by default.** No store created unless the app builder calls
   `.settings(...)`. Apps that don't want persistence pay nothing.

## 1. Crate layout — `fern-settings`

New crate `crates/fern-settings/`. Lives in the dependency graph between
`fern-data` and `fern-widgets` (it depends on `fern-core` for `Signal`,
`fern-data` for `ListModel`/`TreeModel`, and `serde` + `toml` +
`directories` + `tempfile` from the workspace). No `confy` — the
atomic-write primitive is ~10 lines of `tempfile::NamedTempFile::persist`,
and owning that code lets us interleave migration peek and atomic
write in one pass instead of round-tripping through confy's API.

```text
crates/fern-settings/
    src/
        lib.rs                  # public surface, re-exports
        store.rs                # SettingsStore: dynamic K/V Signal cache
        file.rs                 # SettingsFile<T>: typed single-struct file
        path.rs                 # OS-correct path resolution (wraps directories)
        flush.rs                # debounced flusher, atomic writer
        migration.rs            # Migrator<T>: version → version
        collection/
            list.rs             # PersistedListModel<T>
            tree.rs             # PersistedTreeModel<T>
        recents.rs              # built-in: RecentsService<T>
        window_state.rs         # built-in: WindowStateService
    tests/
        roundtrip.rs            # save → load identity tests
        migration.rs            # version 1 → 2 fixtures
        debounce.rs             # multiple sets → one write
```

`fern-ui` re-exports `fern_settings` as `pub use fern_settings as settings;`
in [`crates/fern-ui/src/lib.rs`](../../crates/fern-ui/src/lib.rs).

## 2. Storage primitives

### 2.1 Path resolution — `path.rs`

```rust
pub struct AppPaths {
    qualifier: String,      // "com"
    organization: String,   // "FernTech"
    application: String,    // "Skribisto"
    project_dirs: ProjectDirs,
}

impl AppPaths {
    pub fn new(qualifier: &str, organization: &str, application: &str) -> Self;
    pub fn config_dir(&self) -> &Path;   // %APPDATA%, ~/.config, ~/Library/Preferences
    pub fn data_dir(&self) -> &Path;     // for window state, caches
    pub fn config_file(&self, name: &str) -> PathBuf {
        self.config_dir().join(format!("{name}.toml"))
    }
}
```

Thin wrapper around `directories::ProjectDirs` to keep that dependency
contained in one place and to provide deterministic paths for testing
(`AppPaths::for_testing(tmp: &Path)`).

### 2.2 Atomic + debounced writer — `flush.rs`

```rust
pub(crate) struct DebouncedWriter {
    path: PathBuf,
    pending: Rc<Cell<Option<String>>>,    // serialized bytes awaiting flush
    timer: Rc<RefCell<Option<TimerHandle>>>,
    delay: Duration,
}

impl DebouncedWriter {
    pub fn new(path: PathBuf, delay: Duration) -> Self;

    /// Replace the pending payload. If a flush is already scheduled,
    /// it picks up the new payload when it fires; no extra timer.
    pub fn schedule(&self, serialized: String);

    /// Force an immediate write. Called on graceful shutdown.
    pub fn flush_now(&self);
}

fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().expect("config path has parent");
    let tmp = NamedTempFile::new_in(dir)?;
    tmp.as_file().write_all(contents.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
```

Timer scheduling reuses fern-core's frame scheduler if available; falls
back to a single OS timer thread otherwise. `flush_now` is wired to the
`FernAppBuilder` shutdown hook.

### 2.3 Typed single-struct file — `file.rs`

For state that *is* a known typed struct (the recents file, the window
state file), not a dynamic K/V map:

```rust
pub struct SettingsFile<T: Serialize + DeserializeOwned + Default + Versioned> {
    inner: Rc<SettingsFileInner<T>>,
}

struct SettingsFileInner<T> {
    path: PathBuf,
    current: RefCell<T>,
    writer: DebouncedWriter,
}

pub trait Versioned {
    const CURRENT_VERSION: u32;
    fn version(&self) -> u32;
    fn set_version(&mut self, v: u32);
}

impl<T> Clone for SettingsFile<T> { /* clones the Rc — same handle */ }

impl<T> SettingsFile<T> where T: ... {
    pub fn load(path: PathBuf, migrator: &Migrator<T>) -> Self;
    pub fn snapshot(&self) -> T;             // borrow + clone out of `current`
    pub fn replace(&self, new: T);           // updates `current`, schedules flush
    pub fn flush_now(&self);
}
```

`SettingsFile<T>` is a cheap `Rc`-handle, like `ListModel<T>` and
`TreeModel<T>`. Cloning produces a second handle to the same on-disk
projection, which is what `PersistedListModel` needs when it captures
the file inside the `observe_changes` callback. Mirrors the
`Rc<RefCell<Inner>>` pattern used throughout fern-data.

This is the "you own the struct, I own the disk" half. `RecentsService`
and `WindowStateService` are built on top of it.

## 3. `SettingsStore` — dynamic K/V Signal cache

The QSettings-equivalent: dotted keys, type chosen at the call site,
backing `Signal<T>` cached by key.

### 3.1 Public API

```rust
pub struct SettingsStore {
    inner: Rc<RefCell<StoreInner>>,
    writer: DebouncedWriter,
}

impl SettingsStore {
    /// Load (or create empty) from a TOML file. Used internally by
    /// FernAppBuilder; apps usually receive a `&SettingsStore` from
    /// EventContext / BuildContext rather than constructing one.
    pub fn open(path: PathBuf) -> Self;

    /// Get-or-create a typed Signal for a key. The default is used
    /// only the first time the key is touched (and absent from disk);
    /// subsequent calls ignore it and return the cached Signal.
    ///
    /// Panics if the key was previously created with a different type.
    pub fn signal<T>(&self, key: &str, default: T) -> Signal<T>
    where T: Clone + Serialize + DeserializeOwned + 'static;

    /// Like `signal`, but typed via a strongly-named `SettingsKey<T>`
    /// constant. Preferred when the setting has a single canonical
    /// owner (avoids string typos and centralizes the default).
    pub fn signal_for<T>(&self, key: &SettingsKey<T>) -> Signal<T>
    where T: Clone + Serialize + DeserializeOwned + 'static;

    /// True if the key already has a Signal — useful for "registration
    /// pass" code that wants to seed defaults exactly once.
    pub fn has(&self, key: &str) -> bool;

    /// Iterate keys that have been registered (i.e., touched at least
    /// once). For "Settings UI" introspection.
    pub fn registered_keys(&self) -> Vec<String>;

    /// Force flush to disk (e.g. before opening a child process).
    pub fn flush_now(&self);
}

/// Statically-named setting. Centralizes key + default + type.
pub struct SettingsKey<T> {
    pub key: &'static str,
    pub default: fn() -> T,
}

impl<T> SettingsKey<T> {
    pub const fn new(key: &'static str, default: fn() -> T) -> Self {
        Self { key, default }
    }
}
```

Apps typically declare keys as constants:

```rust
pub const EDITOR_FONT_SIZE: SettingsKey<f32> =
    SettingsKey::new("editor.font_size", || 14.0);
pub const EDITOR_MINIMAP: SettingsKey<bool> =
    SettingsKey::new("editor.minimap", || true);
```

…and bind anywhere:

```rust
let font_size = ctx.settings().signal_for(&EDITOR_FONT_SIZE);
TextWidget::new("Hello").bind_font_size(font_size.clone())
```

### 3.2 Internals

```rust
struct StoreInner {
    raw: toml::Value,
    cells: HashMap<String, SignalCell>,
}

struct SignalCell {
    type_id: TypeId,
    type_name: &'static str,    // for panic messages
    signal: Box<dyn Any>,       // Signal<T>
    handle: ObserverHandle,     // observer that writes back to raw
}
```

`signal::<T>` does:

1. Lock `inner` (RefCell borrow_mut).
2. If the cell exists: assert `TypeId::of::<T>() == cell.type_id`,
   downcast `&Signal<T>`, clone, return.
3. Else: read `get_nested(&raw, key)`. Try `T::deserialize` on it.
   On failure or absence, use `default`.
4. Construct `Signal::new(initial)`. Register an `observe` callback
   that captures a **`Weak<RefCell<StoreInner>>`** — never a strong
   `Rc` — and on each `set`:
   - `let Some(inner) = weak.upgrade() else { return };`
   - serializes the new `T` to `toml::Value`,
   - writes it back into `raw` at `key` (creating tables as needed),
   - schedules a debounced flush via the (also-weakly-captured) writer.
5. Insert the new cell, return the signal.

The weak-capture is load-bearing. A strong `Rc` would create a cycle
— the closure lives inside `ObserverHandle`, which lives inside
`SignalCell`, which lives inside `StoreInner.cells`, which is the very
thing the closure would be capturing — leaking the entire store for
the life of the process. fern-core's signal docstring at
[signal.rs:230-244](../../crates/fern-core/src/signal.rs#L230-L244)
documents the exact pattern. The `weak.upgrade()?` early-return also
gives correct teardown semantics: if the store is dropped, in-flight
signal sets stop trying to write back.

The `ObserverHandle` itself lives in the cell, which lives in the
store; observers persist for the life of the store. The cell holds a
`Signal<T>` clone (one Rc bump), not a duplicate of the store's inner
state.

### 3.3 Dotted-key semantics

`"editor.font_size"` walks into TOML as `[editor] font_size = 14.0`.
`set_nested` creates intermediate tables on demand. Constraint:
**a key cannot be both a value and a parent**. `"editor"` and
`"editor.font_size"` cannot coexist.

Both collision directions panic at registration time, never silently
overwrite (silent overwrite would lose user data):

- *Value-then-table.* File on disk has `editor = "blue"` (a string),
  code calls `signal::<f32>("editor.font_size", 14.0)`. `set_nested`
  detects that `editor` is a non-table value and panics with
  `SettingsStore: cannot create child key "editor.font_size" — "editor"
  is already a value of type string. Pick a different key.`
- *Table-then-value.* File has `[editor] font_size = 14.0`, code calls
  `signal::<String>("editor", "blue".into())`. Same check, opposite
  order: `editor` resolves to a table, `signal` panics with the
  symmetric message.

Both panics fire on the very first `signal` call that creates the
collision — no chance for the offending key to ship to disk. Test
fixtures in `tests/store_dotted_keys.rs` cover both directions plus
deeply nested cases (`a.b.c.d` collision with `a.b`).

### 3.4 Type-mismatch panic

A code-bug, not a runtime condition. Panic message:

```text
SettingsStore: key "editor.font_size" registered as f32, but
signal::<i32>(...) was called. This is a programming error — pick
one type per key.
```

Alternative considered (returning `Result`): rejected because every
call-site would need to unwrap, and the failure mode is always
programmer error. A panic surfaces it on first run; a Result hides it
behind a `?` that no one writes.

## 4. Persisted collections

`SettingsStore` is wrong for collections — `Signal<Vec<T>>` would
full-rebuild every `ListView` on every add. Collections get a parallel
API.

### 4.1 `PersistedListModel<T>` — `collection/list.rs`

```rust
pub struct PersistedListModel<T> where T: Clone + Serialize + DeserializeOwned + 'static {
    pub model: ListModel<T>,
    file: SettingsFile<ListFile<T>>,
    _handle: ObserverHandle,
}

#[derive(Serialize, Deserialize)]
struct ListFile<T> {
    version: u32,
    items: Vec<T>,
}

impl<T> PersistedListModel<T> where T: ... {
    pub fn open(path: PathBuf, migrator: Migrator<ListFile<T>>) -> Self {
        let file = SettingsFile::load(path, &migrator);
        let snapshot = file.snapshot();
        let model = ListModel::from_vec(snapshot.items);

        // Bridge: any DataChange → re-serialize whole list, schedule flush
        let model_clone = model.clone();
        let file_clone = file.clone();    // SettingsFile is Clone (Rc'd inside)
        let handle = model.observe_changes(move |_change| {
            let items: Vec<T> = (0..model_clone.len())
                .filter_map(|i| model_clone.with_item(i, |t| t.clone()))
                .collect();
            file_clone.replace(ListFile { version: ListFile::<T>::CURRENT_VERSION, items });
        });

        Self { model, file, _handle: handle }
    }
}
```

Why re-serialize the whole list on every change? The lists this is for
(recents, pinned files, custom palettes) are small — under 100 items —
and TOML serialization of 100 small structs is microseconds, much less
than the 500 ms debounce window. If an app needs a 10k-row persisted
list, that is SQLite territory and out of scope.

### 4.2 `PersistedTreeModel<T>` — `collection/tree.rs`

Symmetric, but `TreeModel<T>` serializes to a recursive structure:

```rust
#[derive(Serialize, Deserialize)]
struct TreeNodeData<T> {
    value: T,
    children: Vec<TreeNodeData<T>>,
}
```

Used for things like the user's saved query tree, custom menu
hierarchies. Same observe-changes-flush bridge.

### 4.3 What about per-item updates?

`ListModel::with_item` returns through a callback (no `&mut`); to
modify an item, the app reads it (via `with_item`), constructs the
new value, and calls `model.set(idx, new_value)` — which emits
`DataChange::ItemUpdated{index}`. The persistence bridge re-serializes
the whole list either way, so the model API limitation has no impact
here. If `ListModel` ever grows an in-place `update(idx, |&mut T|)`,
the bridge gets it for free.

## 5. Worked example — Recent projects

Built into `fern-settings` as a concrete service so apps don't
re-implement it.

```rust
// crates/fern-settings/src/recents.rs

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: PathBuf,
    pub display_name: String,
    pub last_opened: SystemTime,
    pub pinned: bool,
}

pub struct RecentsService {
    persisted: PersistedListModel<RecentProject>,
    max_items: usize,
}

impl RecentsService {
    pub fn open(paths: &AppPaths, max_items: usize) -> Self {
        let migrator = Migrator::<ListFile<RecentProject>>::new();
        let persisted = PersistedListModel::open(
            paths.config_file("recents"),
            migrator,
        );
        Self { persisted, max_items }
    }

    /// The reactive list. Bind to ListView / Repeater directly.
    pub fn model(&self) -> &ListModel<RecentProject> {
        &self.persisted.model
    }

    pub fn add(&self, project: RecentProject) {
        // Dedupe by path: remove any existing entry with same path.
        if let Some(existing) = self.find_index(&project.path) {
            self.persisted.model.remove(existing);
        }
        self.persisted.model.insert(0, project);
        self.cap_to_max();
    }

    pub fn remove(&self, path: &Path) {
        if let Some(idx) = self.find_index(path) {
            self.persisted.model.remove(idx);
        }
    }

    pub fn touch(&self, path: &Path) {
        if let Some(idx) = self.find_index(path) {
            let mut updated = self.persisted.model
                .with_item(idx, |p| p.clone())
                .expect("index is in bounds");
            updated.last_opened = SystemTime::now();
            self.persisted.model.set(idx, updated);
        }
    }

    pub fn toggle_pin(&self, path: &Path) {
        if let Some(idx) = self.find_index(path) {
            let mut updated = self.persisted.model
                .with_item(idx, |p| p.clone())
                .expect("index is in bounds");
            updated.pinned = !updated.pinned;
            self.persisted.model.set(idx, updated);
        }
    }

    fn find_index(&self, path: &Path) -> Option<usize> {
        let model = &self.persisted.model;
        (0..model.len()).find(|&i| {
            model.with_item(i, |p| p.path == path).unwrap_or(false)
        })
    }

    fn cap_to_max(&self) {
        let model = &self.persisted.model;
        while model.len() > self.max_items {
            // Walk from the end, drop the first non-pinned entry.
            let drop_idx = (0..model.len()).rev().find(|&i| {
                model.with_item(i, |p| !p.pinned).unwrap_or(false)
            });
            match drop_idx {
                Some(i) => { model.remove(i); }
                None    => break,    // everything is pinned
            }
        }
    }
}
```

### 5.1 UI binding

`Repeater` already exists at
[`crates/fern-widgets/src/repeater.rs`](../../crates/fern-widgets/src/repeater.rs)
and accepts a `ListModel<T>` plus a per-item factory closure — it
subscribes via `observe_changes` and patches its child list
incrementally. That's exactly the shape the recents menu needs:

```rust
// In a "File" menu builder:
let recents = ctx.recents();    // shorthand on BuildContext (see §7)
let model = recents.model().clone();

ctx.add(
    MenuList::new()
        .child(Repeater::new(model, |project: &RecentProject| {
            MenuItem::new_literal(project.display_name.clone())
                .on_activate_fn({
                    let path = project.path.clone();
                    move |ctx| ctx.send_intent(AppIntent::OpenRecent(path.clone()))
                })
        }))
        .child(Divider::new())
        .child(MenuItem::new_literal("Clear recents")
            .on_activate_fn(|ctx| ctx.send_intent(AppIntent::ClearRecents)))
)
```

The `fern!` DSL does not yet have a `for x in list_model { ... }` form
that desugars into `Repeater`. Adding one is a separate, optional
follow-up to the `fern-ui-macros` crate — out of scope for this plan.
Until then, recents menus use `Repeater` directly.

### 5.2 Wiring through Action/Intent

```rust
ctx.register_action(Action::new("app.open_recent").on_invoke(|i, ctx| {
    if let Some(AppIntent::OpenRecent(path)) = AppIntent::from_intent(i) {
        ctx.recents().touch(&path);   // bumps last_opened, debounced flush
        open_project(path, ctx);
    }
}));

ctx.register_action(Action::new("app.clear_recents").on_invoke(|_, ctx| {
    ctx.recents().model().clear();
}));
```

## 6. Built-in: window state — `window_state.rs`

Position, size, maximized state, last open document — restored on
launch. Single typed file, not in `SettingsStore`:

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct WindowState {
    pub version: u32,
    pub windows: Vec<PerWindowState>,
}

#[derive(Serialize, Deserialize)]
pub struct PerWindowState {
    pub label: String,           // "main", "log", per `WindowConfig::label`
    pub x: i32, pub y: i32,
    pub width: u32, pub height: u32,
    pub maximized: bool,
}
```

Restoration policy:

- If the saved monitor no longer exists, snap back to the primary
  monitor's center.
- If saved size > monitor work area, clamp to work area.
- If position has any overlap > 50 px with *some* monitor, accept it
  as-is.

**Wayland caveat.** Set-position is, by design, not a thing on Wayland
— the compositor places windows. `winit` returns `Ok` from
`set_outer_position` on Wayland but the value is ignored.
`PerWindowState.x/y` are therefore only honored on X11, macOS, and
Windows; on Wayland the manager skips the position request and lets
the compositor decide. Width/height/maximized are honored on all
platforms. The serialized fields stay the same so a config file roams
correctly between sessions on different display servers.

Wired at `WindowManager` create-window time
([`crates/fern-app/src/`](../../crates/fern-app/src/)) — the manager
asks the `WindowStateService` for the saved geometry of `label` before
instantiating `winit::Window`, and registers an `on_close` hook that
captures final geometry.

## 7. App builder integration — `fern-app`

```rust
FernAppBuilder::new()
    .application("com", "FernTech", "Skribisto")    // mandatory if persistence used
    .settings(Settings::default()
        .with_recent_projects(10)
        .with_window_state(true))
    .root(|tree| { ... })
    .run();
```

As part of this step, three accessors are **added** to
[`crates/fern-core/src/event_context.rs`](../../crates/fern-core/src/event_context.rs)
and
[`crates/fern-core/src/build_context.rs`](../../crates/fern-core/src/build_context.rs)
— they do not exist today:

```rust
fn settings(&self) -> &SettingsStore;
fn recents(&self) -> &RecentsService;     // panics if not enabled
fn window_state(&self) -> &WindowStateService;
```

The contexts hold an `Rc<AppServices>` (or similar shared handle)
populated by `FernAppBuilder::run` before the first frame. Apps that
don't call `.settings(...)` get a `None` for each service and the
accessors panic on use — same pattern as `ctx.i18n()` today, see
[`crates/fern-core/src/build_context.rs`](../../crates/fern-core/src/build_context.rs)
for the precedent.

The store's `flush_now()` is wired to the app builder's shutdown path
so that exit-time writes always make it to disk (the debounce timer
might still be pending).

## 8. Schema migration — `migration.rs`

```rust
pub struct Migrator<T: Versioned + DeserializeOwned> {
    steps: Vec<Box<dyn Fn(toml::Value) -> Result<toml::Value, MigrationError>>>,
    target: u32,
}

impl<T> Migrator<T> {
    pub fn new() -> Self;

    /// Add a step `from_version → from_version + 1`.
    pub fn step<F>(mut self, from: u32, f: F) -> Self
    where F: Fn(toml::Value) -> Result<toml::Value, MigrationError> + 'static;

    pub(crate) fn run(&self, raw: toml::Value) -> Result<T, MigrationError>;
}
```

`run` operates on `toml::Value` — pre-deserialization — so it can read
the version before the type system gets involved:

```rust
pub(crate) fn run(&self, mut raw: toml::Value) -> Result<T, MigrationError> {
    // Peek version field directly; missing = treat as v1 (legacy).
    let mut current = raw.get("version")
        .and_then(|v| v.as_integer())
        .map(|n| n as u32)
        .unwrap_or(1);

    while current < self.target {
        let step = self.steps.iter()
            .find(|s| s.from == current)
            .ok_or(MigrationError::NoStepFor(current))?;
        raw = (step.f)(raw)?;
        current += 1;
        // Update the version field in-place so each step sees the new number.
        if let Some(t) = raw.as_table_mut() {
            t.insert("version".into(), toml::Value::Integer(current as i64));
        }
    }
    T::deserialize(raw).map_err(MigrationError::Deserialize)
}
```

The version peek must happen on `toml::Value`, never on `T` — a v1
file generally fails to deserialize as the v2 type, so the migrator
can't run *after* `T::deserialize`. This ordering is the whole point
of holding raw TOML at the boundary.

Apps register migrators per-file at startup:

```rust
let recents_migrator = Migrator::<ListFile<RecentProject>>::new()
    .step(1, |mut v| {
        // v1 → v2: add `pinned` field, default false
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

If migration fails: log, back up corrupt file as `recents.toml.broken-<ts>`,
start with defaults. Never silently lose data.

## 9. Multi-process & external edits

### 9.1 Multi-process

Two app instances both writing to `~/.config/skribisto/general.toml` =
last-write-wins. QSettings has the same problem on Linux INI; macOS
plist is fundamentally the same; only Windows registry is partially
arbitrated by the kernel.

Decision: **don't try to solve it in v1**. Document it. Skribisto is
single-instance via DBus single-instance lock anyway. If a multi-app
scenario emerges (e.g., a CLI tool sharing config with the app), add a
`fcntl`/`LockFile` advisory lock around the atomic write — file format
unchanged. Out of scope for the initial cut.

### 9.2 External edits / file watcher

Optional. When enabled, `SettingsStore` registers a `notify` watcher on
the config directory. On debounced "external change" event:

1. Suspend the writer (mark "reload in progress").
2. Re-parse the file.
3. For each cell in `cells`: if the new TOML value deserializes and
   differs from the current `Signal<T>::get()`, call `signal.set(new)`.
   The internal observer fires *back* into `raw`, which is now equal,
   so no re-flush.
4. Resume the writer.

Useful for power users editing TOML by hand while the app runs. Default:
**off**, behind `Settings::with_file_watch(true)`. Adds the `notify`
crate dep when enabled.

## 10. Testing

```rust
// tests/roundtrip.rs
#[test]
fn signal_persists_across_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("general.toml");

    {
        let store = SettingsStore::open(path.clone());
        let s = store.signal::<f32>("editor.font_size", 14.0);
        s.set(18.0);
        store.flush_now();
    }

    let store = SettingsStore::open(path);
    let s = store.signal::<f32>("editor.font_size", 14.0);    // default ignored
    assert_eq!(s.get(), 18.0);
}

#[test]
fn type_mismatch_panics() { ... }

#[test]
fn dotted_key_collision_panics() { ... }

#[test]
fn recents_dedupes_by_path() { ... }

#[test]
fn recents_caps_to_max_keeping_pinned() { ... }

#[test]
fn list_model_changes_flush_after_debounce() {
    // Use a `flush.rs` test mode that exposes the timer for control.
}

#[test]
fn migration_v1_to_v2_adds_pinned_field() { ... }

#[test]
fn corrupt_file_backs_up_and_uses_defaults() { ... }
```

All tests are headless; no `directories::ProjectDirs` involved (paths
come from `tempdir`).

## 11. Implementation order

1. **`fern-settings` skeleton** — crate, deps, empty modules. Wire
   workspace `[workspace.dependencies]` for `directories`, `tempfile`,
   `toml`, `serde`. Re-export from `fern-ui`.
2. **`AppPaths` + atomic write + `DebouncedWriter`** — pure mechanical;
   tests with `tempdir`.
3. **`SettingsFile<T>` + `Versioned` + `Migrator<T>`** — the typed-file
   half. Cover: load, save, migrate, corrupt-file backup.
4. **`SettingsStore`** — the dynamic half. Cover: signal cache, dotted
   keys, type mismatch, write-back observer, flush.
5. **`PersistedListModel<T>`** — bridge ListModel ↔ SettingsFile.
   Tests: every `DataChange` variant produces correct on-disk state
   after debounce.
6. **`RecentsService`** — public service on top of (5). Includes the
   dedupe / cap-with-pin policy.
7. **`PersistedTreeModel<T>`** — symmetric to (5). No built-in service
   yet; purely available for app use.
8. **`WindowStateService`** — wire into `WindowManager`. Geometry
   capture on close, restoration on open. Validate-against-monitor
   policy.
9. **`FernAppBuilder` integration** — `.settings(...)`, `EventContext`
   accessors, shutdown flush hook.
10. **Demo example** — `examples/recent_projects/` showing menu +
    settings dialog reading/writing through the store. Uses existing
    [`Repeater`](../../crates/fern-widgets/src/repeater.rs) for the
    menu binding — no widget work needed.
11. **Migrate `ShortcutSettings`** to persist via `SettingsFile<ShortcutBindings>`
    instead of in-memory only. Closes the loop on the first real
    consumer. Requires extracting a serde-friendly `ShortcutBindings`
    struct from `ShortcutRegistry`'s current internal map; check
    [`crates/fern-core/src/shortcut.rs`](../../crates/fern-core/src/shortcut.rs)
    for the existing serialization shape before designing.
12. **Optional: file watcher** behind a feature flag.

Steps 1-4 are the critical path; everything else can land independently
once the store exists. Steps 8-11 should each ship behind their own
PR with a working example.

## 12. Out of scope — intentional

- **Encryption.** Settings are plaintext TOML. Secrets (API tokens,
  passwords) go through OS keychain via a separate `fern-secrets` crate
  (not planned here).
- **Cloud sync.** No.
- **Schema-driven UI generation.** No automatic "Settings dialog from
  the schema." Apps build settings UI by hand using regular widgets
  bound to `Signal<T>`s from the store. (A future plan could add this;
  not now.)
- **Multi-instance write coordination.** Documented as last-write-wins.
- **Large persisted collections (>1000 items).** Use SQLite via
  `rusqlite`; the persistence bridge is intentionally re-serialize-whole.
- **Hot config reload from network/HTTP.** No.
- **Per-document state.** Document state (open file's cursor position,
  fold state) belongs in the document file or its sidecar, not in app
  settings. Same architectural primitive (`SettingsFile<T>`) reusable
  by app code, but no built-in service.
- **`QSettings::sync()`-style read-back guarantees.** We expose a
  whole-store `flush_now()` for shutdown and pre-fork scenarios, but
  there is no per-key sync, no read-after-write barrier within a
  single tick, and no guarantee that two stores opened on the same
  file in the same process see each other's writes. Apps that need
  any of these are using the wrong tool.

## 13. Open questions

1. **Default debounce window.** 500 ms feels right for slider drags
   and keystroke-driven settings. Faster means more disk I/O; slower
   means visible "did my change save?" anxiety. Could expose as
   `Settings::with_debounce(Duration)`.
2. **Single-file vs split files.** Plan splits by concern. Counter-
   argument: one file is easier to back up / sync via Dropbox / commit
   to dotfiles repo. Could expose both modes — `SettingsStore::open_single`
   vs `SettingsStore::open_split` — but adds API surface for marginal
   value. Defer.
3. **`SettingsKey<T>` ergonomics.** Constants with `fn() -> T`
   defaults work for non-`Copy` types but read awkwardly. Alternative:
   a `settings_key!` macro that desugars `settings_key!("editor.font_size", f32 = 14.0)`
   into a `const`. Cosmetic; defer.
4. **Cross-platform path consistency for tests.** `directories` returns
   different paths per OS. CI must run tests with `AppPaths::for_testing(tmp)`
   only — never against real `ProjectDirs`. Document loudly.
