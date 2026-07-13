<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Settings & Persisted State Reference

Bastyde's persistence layer (`bastyde-settings`) is **reactive end-to-end**:
disk values live as `Signal<T>`s and `ListModel<T>`s, mutating either
the in-memory handle or the underlying file flows to the other side
automatically. There is no separate "config object" you remember to
write back; the in-memory state *is* the source of truth, and disk is
a projection of it — debounced for the types that write often, synchronous
for the types that don't.

**Cross-process safety is not a mode you opt into — it is the only
behaviour every persisted type in this crate has.** Two processes (or two
windows in one process) sharing the same `general.toml` / `recents.toml` /
`window_state.toml` — exactly Skribisto's one-process-per-open-project model
— cannot clobber each other's writes, and neither process has to do
anything to notice the other's change: a peer's write arrives on its own,
live, through the same `Signal`/`ListModel` you're already bound to. Unlike
Qt's `QSettings`, which makes you remember to call `sync()` at the right
moments, there is nothing to remember here at all.

Mental model in one line:

```
SettingsBundle → OpenedSettings → app_state registry → SettingsExt accessors → reactive widgets
                        ↓
                 SettingsRegistry ← SettingsWatcher ← a peer's write landing on disk
```

Three persistence shapes share one storage backbone:

| Shape | Type | Use for |
|---|---|---|
| Dynamic K/V | [`SettingsStore`](../crates/bastyde-settings/src/store.rs) → `Signal<T>` | Scalar prefs (font size, theme name, bools, arrays of scalars) |
| Typed file | [`SettingsFile<T>`](../crates/bastyde-settings/src/file.rs) | App-shaped structs with their own schema + migrations |
| Reactive collection | [`PersistedListModel<T>`](../crates/bastyde-settings/src/collection/list.rs) | Recents, palettes, saved searches — anything that drives a `Repeater` / `ListView` |

There used to be a fourth shape, `PersistedTreeModel<T>`, for nested
hierarchies. It had zero consumers anywhere in this workspace or in
Skribisto and carried the exact whole-snapshot-clobber defect this crate
now hardens everything else against, so it was deleted rather than dragged
through that hardening — see "Cross-process safety, by default" below.
Reintroduce it (ops-based, from scratch) if a consumer actually needs a
persisted tree.

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
    let paths = AppPaths::new("eu", "FernTech", "Bastyde")
        .expect("could not resolve OS config directory");

    // App-typed MRU list — the framework knows nothing about projects.
    let recents: MruList<RecentProject> =
        MruList::open(&paths, "recent_projects", 10).unwrap();

    BastydeAppBuilder::new()
        .theme(intui::light())
        .app_paths(paths)                                // explicit
        // or .application("eu", "FernTech", "Bastyde") // shortcut
        .settings(
            SettingsBundle::new()
                .with_window_state(true),                // opt-in
        )
        // Live cross-process reload is on by default the moment `.settings(...)`
        // is configured — nothing else to write here. Opt out with
        // `.settings_watch(false)` (e.g. a sandboxed test double with no
        // usable filesystem watcher).
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
- The live-reload watcher only starts for `run()` (a real event loop to
  post the reload event through); `build_headless()` never starts one.
  Headless callers that still want to notice a peer's write poll
  `Reloadable::reload_from_disk` themselves.

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
  Its writes are synchronous — see "Cross-process safety, by default"
  below for why that's the right trade-off for this shape.
- **`PersistedListModel<T>`** wraps a reactive `ListModel<T>` and
  persists by **replayable op** (upsert / update / remove / clear by
  key), not by re-serializing the whole collection on every mutation.
  Mutating either the model's ops or the file goes through one path;
  widgets bound to the model see incremental UI updates (`Repeater`
  does the right thing on inserts/removes/reorderings, and never sees
  a `DataChange::Reset` from a peer's write landing — see "Reconciling
  a live collection without losing the user's place" below).

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
let paths = AppPaths::new("eu", "FernTech", "Bastyde")
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
// opened.registry: SettingsRegistry — every service above is
//                  pre-registered into it (see "Live reload" below).
```

`OpenedSettings` is a cheap-to-clone handle bundle. Each contained
service is `Rc<>`-shaped internally; cloning produces a second handle
to the same in-memory state and the same shared I/O thread queue.
`BastydeAppBuilder::run` keeps one `OpenedSettings` on the stack while
clones of each service live in the `app_state` registry — when the
registry is dropped at exit, the `Drop` impls flush every pending
payload synchronously.

`flush_all()` is the explicit form for tests and pre-fork scenarios.

`with_debounce` only actually debounces `SettingsStore`'s writes —
`WindowStateService` accepts the same parameter (so `open` can call
both uniformly) but ignores it, because its writes are always
synchronous now (see `WindowStateService` below).

---

## Cross-process safety, by default

This is the architectural story every other section builds on. It applies
identically to `SettingsStore`, `SettingsFile<T>`, and
`PersistedListModel<T>` — there is no per-type opt-in and no "shared mode"
flag anywhere in this crate's public API.

### Why a lock alone is not enough

The obvious fix for "two processes writing the same file" is "wrap the
write in an advisory `flock`." That is *necessary but not sufficient*. A
lock only serializes the two **writes** against each other; it does
nothing about a **stale in-memory snapshot**. Concretely: process A takes
the lock, writes, releases it; process B then takes the lock and writes
*its own* pre-loaded snapshot — which predates A's write and doesn't
contain A's change — and B's write, though itself perfectly atomic and
lock-protected, still clobbers A's change. The lock made the write safe;
it did nothing to make the write *correct*.

The fix has to be a **locked read-modify-write**: the re-read of the
current on-disk state has to happen *after* the lock is acquired and
*before* the caller's change is applied, so the write that follows is
always based on fresh data, not a snapshot that might already be behind a
peer's write.

```text
  lock  ->  read current  ->  apply queued patches  ->  write atomically  ->  unlock
```

### Patches, not rendered strings

Every write in this crate is expressed as a **`Patch`**: "given the file's
current raw text (or `None` if it doesn't exist yet), produce its new raw
text." The write path used to carry a pre-rendered `String` instead — the
caller serialized its whole in-memory document and the writer blindly
wrote those bytes. That is last-write-wins *by construction*: the writer
has nothing to merge *with*. A `Patch` closure instead runs against
whatever is actually on disk, read fresh under the lock, so a peer's
concurrent change to some *other* part of the document survives:

- `SettingsStore` builds a patch from only the **dotted keys dirtied**
  since the last schedule — never a full render of the document — so a
  peer's change to an unrelated key is untouched.
- `PersistedListModel<T>` builds a patch from a small **`ListOp<T>`**
  (`UpsertFront` / `UpdateInPlace` / `Remove` / `Clear`, keyed by
  `Keyed::key` — see "`MruList<T: MruEntry>`" below) — never a
  re-derived `Vec<T>` — so a peer's concurrent insert or removal survives.
- `SettingsFile<T>::mutate`/`replace` apply the caller's closure directly
  to the freshly re-read, re-migrated value, under the same lock.

### Why `Fn`, not `FnOnce`

A patch may need to run **more than once**: if the write fails (disk full,
a transient network mount), the queued patches are *retained* and replayed
on the next tick against whatever is on disk *then*. That re-application is
exactly the right merge, and it's only possible if the patch can be called
again — `FnOnce` would force a choice between dropping the mutation (silent
data loss) or caching a pre-rendered string (which defeats the merge and
reintroduces last-write-wins for exactly the writes that failed once).
Patches are built entirely *inside* this crate from owned snapshots (a
`Vec<(key, value)>`, a `ListOp<T>`), so this never leaks into the public
API: callers keep writing `signal.set(v)` / `mru.add(e)` /
`file.mutate(|s| ..)` and never see a `Patch`.

### The honest performance story

**One `flock` + read + parse per debounce window, not per `set`.**
`SettingsStore` and `PersistedListModel<T>` batch every mutation that
happens inside the debounce window (default 500 ms) into one patch queue,
and flush that whole queue as a single locked read-modify-write when the
window elapses (or `flush_now()` is called). Setting ten `Signal`s in a
tight loop costs one lock/read/write, not ten.

`SettingsFile<T>` is the deliberate exception: `mutate`/`replace` are
*always* a synchronous locked read-modify-write, on the calling thread,
bypassing the debounce entirely. That's the right trade-off **for how this
type is meant to be used** — a settings change, one record per backup run
— where writes are rare enough that there's no burst to coalesce, and a
synchronous write keeps the "read fresh, apply, write" window as short as
possible. It becomes the *wrong* trade-off if a caller wires it to
something that fires every frame — see `WindowStateService`'s `record`
below for a real instance of exactly that happening today.

### Two mechanisms, one guarantee

| | `SettingsStore` / `PersistedListModel<T>` | `SettingsFile<T>` |
|---|---|---|
| Merge unit | dotted key / `ListOp<T>` | whole struct, via caller's closure |
| Timing | debounced (default 500 ms), shared I/O thread | synchronous, calling thread |
| Retry on failure | queue retained, replayed next tick | error propagates immediately |
| Right for | frequent small writes (a `Signal::set`, a recents `add`) | rare whole-struct writes |

Both go through the same `<path>.lock` sidecar (via `fs2`, cross-platform
`flock`/`LockFileEx`) — see `crates/bastyde-settings/src/lock.rs` — so a
`SettingsStore` write and a `SettingsFile<T>` write to two different paths
never contend, and two handles (in this process or a peer's) to the *same*
path always serialize correctly regardless of which mechanism opened them.

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

### Registration is idempotent, even against a peer's real edit

Registering a key (the first `store.signal("k", default)` call for it)
schedules a **seed write** so a brand-new key hits disk even if nothing
ever calls `.set()` on it. That seed cannot be an unconditional write,
though: two processes independently registering the same key with the
same hardcoded default, at slightly different times, must not have
whichever one's seed-patch happens to run *second* stomp a real value the
*other* process (or a third one) already set there. The dirty-key queue
therefore tags every entry with a `DirtyKind`:

- `Set` — an explicit `Signal::set()` — always wins, unconditionally,
  over whatever is on disk.
- `SeedIfAbsent` — a registration-time default — only written if the key
  is *still absent* from the document at the moment the patch actually
  runs.

This is a real bug this design exists to close, not a hypothetical: an
unconditional seed write would silently discard a peer's already-set real
value under exactly the ordinary "both processes start up and register the
same keys" sequence, with no unusual timing required.

### Reload and the re-entrancy guard

A peer's write doesn't wait for this process to touch the same key —
`Reloadable::reload_from_disk` (see "Live reload" below) pushes it
straight into the already-handed-out `Signal<T>` for that key.
Two independent `SettingsStore` handles over one file, each setting a
*different* key with no coordination — the scenario a two-window /
two-process Skribisto session hits on every settings change:

```rust
let a = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();
let b = SettingsStore::open_with_delay(path.clone(), Duration::ZERO).unwrap();

let dark_a = a.signal::<bool>("ui.dark", false);
let dark_b = b.signal::<bool>("ui.dark", false);
let width_b = b.signal::<f32>("editor.column_width", 80.0);

assert!(!dark_b.get(), "b hasn't seen a's write yet");

dark_a.set(true);
a.flush_now().unwrap();

// b's own write only ever touches its own key — but reload must push
// a's concurrent change into b's *already-live* Signal, no restart needed.
assert!(Reloadable::reload_from_disk(&b).unwrap());
assert!(dark_b.get(), "b's live signal must reflect a's write");

width_b.set(120.0);
b.flush_now().unwrap();

// A third, fresh handle proves both keys are actually on disk together.
let c = SettingsStore::open_with_delay(path, Duration::ZERO).unwrap();
assert!(c.signal::<bool>("ui.dark", false).get());
assert_eq!(c.signal::<f32>("editor.column_width", 80.0).get(), 120.0);
```

Pushing a reloaded value into a `Signal` must not itself schedule a write
— that would bounce the peer's value straight back out as if it were a
local edit, and could race the peer's *next* write. A
`StoreInner::applying_external` flag, set for the duration of the reload,
makes the write-back observer a no-op while a reload is in progress.

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

### `record` is synchronous, not debounced — and that has a real cost today

`WindowStateService` is built directly on `SettingsFile<WindowStateFile>`,
so `record`/`forget` are the same synchronous locked read-modify-write
described in "Cross-process safety, by default" above — there is no
debounce window to coalesce a burst of calls, unlike `SettingsStore` /
`PersistedListModel<T>`. That's the right trade-off for how this type is
*meant* to be called (a handful of writes across a window's lifetime), but
`bastyde-app`'s `window_persist` module currently wires `record` to fire on
*every* `Signal` change of a window's size/position/placement — which on
X11/Windows/macOS means once per reported frame during a live drag or
resize (Wayland mostly spares position, per the caveat below, but size
still updates during a resize). Concretely: a live window drag today does
a synchronous lock-acquire + file read + parse + serialize + atomic write
on every reported geometry change, not a debounced one. This is a known,
current characteristic of that call site's wiring, not a limitation of
`WindowStateService` itself — a caller that wants to coalesce a drag into
one write should debounce at the call site (e.g. call `record` only from a
"drag ended" observer or a periodic timer) rather than from every raw
geometry `Signal`.

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
([`window_state.rs`](../crates/bastyde-settings/src/window_state.rs))
converts each entry's `maximized: true` to `placement = "Maximized"`,
otherwise `"Floating"`. Files are upgraded transparently on first
read; the new shape is written back on the next `record`/`forget`.

---

## `MruList<T: MruEntry>` — generic recents

Apps define their own item type implementing two small traits: `Keyed`
(a stable, owned merge identity — shared with every collection this crate
persists) and `MruEntry` (the pin/touch vocabulary an MRU list specifically
needs on top). The framework provides dedupe-on-add, pin-aware cap
eviction, and cross-process-safe persistence via `PersistedListModel<T>`.

```rust
use std::path::{Path, PathBuf};
use bastyde::settings::{Keyed, MruEntry, MruList};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct RecentProject {
    path: PathBuf,
    display_name: String,
    last_opened: u64,
    pinned: bool,
}

impl Keyed for RecentProject {
    type Key = PathBuf;
    fn key(&self) -> PathBuf { self.path.clone() }
}

impl MruEntry for RecentProject {
    fn is_pinned(&self) -> bool { self.pinned }
    fn set_pinned(&mut self, p: bool) { self.pinned = p; }
    fn touch(&mut self) { self.last_opened += 1; } // a real app stamps a wall-clock time
}
```

`Keyed::Key` is **owned** (`PathBuf`, `String`, a small `Copy` id) — not the
old borrowed `MruEntry::Key: ?Sized` shape — because it must be captured
into a `Patch` closure that crosses to the shared I/O worker thread; a
borrow into `T` cannot outlive the mutation call that produced it. `remove`
/ `touch` / `set_pinned` stay ergonomic despite the owned key by being
generic over `Q where T::Key: Borrow<Q>`, so callers still pass `&Path`
/ `&str` without allocating just to look an entry up.

```rust
let mru: MruList<RecentProject> =
    MruList::open(&paths, "recent_projects", 10).unwrap();

// Bound to UI:
let model = mru.model().clone();   // ListModel<RecentProject>

// Mutations — each of these both updates the live model *and* enqueues
// the matching replayable op (see "Cross-process safety" above):
mru.add(RecentProject {
    path: "/projects/foo".into(),
    display_name: "Foo".into(),
    last_opened: 0,
    pinned: false,
});                                         // dedupes by key, prepends, caps
mru.touch(Path::new("/projects/foo"));     // re-marks as freshly used
mru.set_pinned(Path::new("/projects/foo"), true); // pin (see "Replayable ops" below)
mru.remove(Path::new("/projects/foo"));
mru.clear();
```

The cap policy: only **unpinned** entries count. Pinning a tenth
entry in a 10-cap list doesn't evict anything; pinning eight entries
in a 5-cap list keeps all eight (they're never evicted) plus up to
five unpinned. A re-add of a previously-pinned key *preserves* the
pin even if the new value didn't ask for it.

**Mutate through `MruList`'s methods, never through `.model()`.**
`.model()` is for reading and reactive binding (`ListView` / `Repeater`) —
every UI observer wants live updates regardless of who mutates. There is
no longer an observer that bridges an arbitrary `ListModel` mutation to
disk (that observer *was* the whole-snapshot-clobber bug this crate
exists to fix), so mutating the returned model directly updates what's on
screen but is never persisted.

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

## Live reload: `Reloadable`, the watcher, and self-write suppression

Cross-process safety on the *write* side (above) is only half the story:
a process that loads its state once and never looks again will not notice
a peer's write until it happens to mutate something itself. `Reloadable`
is the *read* side.

```rust
pub trait Reloadable {
    fn path(&self) -> &Path;
    fn reload_from_disk(&self) -> Result<bool, SettingsFileError>;
}
```

Implemented by `SettingsFile<T>` (where `T: PartialEq`), `SettingsStore`,
`PersistedListModel<T>` (where `T: PartialEq`), `WindowStateService`, and
`MruList<T>` (delegating to its `PersistedListModel`). `reload_from_disk`
returns `Ok(true)` if in-memory state actually changed, `Ok(false)` — a
hard guarantee that nothing was touched — otherwise.

### The self-write-suppression contract

A naive implementation would feed back into itself: this process writes
`general.toml`, a watcher notices *that very write* a few milliseconds
later, and calls `reload_from_disk()` — which had better be a cheap no-op,
not a full re-parse-and-notify cycle, and must never re-apply our own
value as if it were a peer's newer one (which could bounce a
just-superseded value back into a live `Signal` between the user's edit
and the debounced write landing). Every implementation layers two checks,
cheapest first:

1. **Stamp check.** Each implementor records the on-disk `(mtime, len)` as
   of the last time it read or wrote the file. If the current stamp
   matches, `reload_from_disk` returns `Ok(false)` immediately — no read,
   no parse, nothing touched. This is the common case for a self-write
   notification.
2. **Content backstop.** If the stamp *did* change (a real write
   happened, by us or a peer — mtime resolution can coincide, or the
   write path didn't get a chance to update the stamp), the file is read
   and parsed, then compared *by value* against what's already live. Only
   a genuine difference is pushed into signals/models; `Ok(false)` is
   returned — again touching nothing — when the content is unchanged.
   This is the actual correctness guarantee; the stamp check is purely an
   optimization to skip the common case cheaply.

### `SettingsWatcher` and `SettingsRegistry`

`SettingsWatcher` owns a `notify::RecommendedWatcher` background thread,
mirrored from `bastyde-i18n`'s `FtlFileWatcher`. It watches **directories**
(`AppPaths::config_dir()` / `data_dir()`), not individual files: every
atomic writer in this crate (and any well-behaved peer) writes a temp file
and renames it over the target, which would invalidate an inode-level
watch on the file itself. `SettingsRegistry` maps a canonical path to a
`Weak<dyn Reloadable>`; a changed-path event is dispatched through it to
the one live handle that owns that path — anything else (a `.lock`
sidecar, a `.tmp` write-in-progress, an unrelated file a peer dropped in
the same directory) is a harmless no-op.

```rust
use bastyde_settings::{SettingsRegistry, SettingsFile, Migrator, Versioned};
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
// Keep `handle` alive for as long as reload should keep working —
// the registry only ever holds a Weak; nothing is called on a
// service that has since been dropped.
let handle = registry.register(Rc::new(file.clone()));
drop(handle); // dropping it deregisters: no leak, no dangling call.
```

`BastydeAppBuilder` wires this up automatically the moment `.settings(...)`
is configured (windowed apps only — `run()`, not `build_headless()`,
since there's a real event loop to post the reload event through): every
service `SettingsBundle::open` opens is pre-registered into
`OpenedSettings::registry`, and that registry itself is installed into
`app_state`, so application code opening its own ad hoc
`SettingsFile<T>` / `PersistedListModel<T>` / `MruList<T>` can register it
too via `ctx.app_state::<SettingsRegistry>()`. Opt out entirely with
`.settings_watch(false)` on the builder (e.g. a sandboxed test double with
no usable filesystem watcher, or an app that wants to poll
`Reloadable::reload_from_disk` on its own schedule instead).

---

## Reconciling a live collection without losing the user's place

A `PersistedListModel<T>`'s reload can't just clear the model and rebuild
it from the freshly-read file — `ListModel::replace_all` emits a blanket
`DataChange::Reset`, which unconditionally clears a positional
`SelectionModel`. If a peer's write lands mid-session while the user has a
row selected (or focused, in a `ListView`), a `Reset`-based reload would
yank that selection out from under them for no reason connected to
anything they did.

Instead, a reload diffs the freshly-read `Vec<T>` against the live model
**by key** (`Keyed::key`) and emits only the minimal granular changes
needed to reconcile the two: coalesced removals for keys that vanished,
single-row moves only for entries that are actually out of place (an
append-only or remove-only reload emits zero moves), value updates
(`T: PartialEq`) for entries whose key survived but whose content changed,
and coalesced insertions for brand-new keys. `ListModel::reconcile_by_key`
(in `bastyde-data`) is the general-purpose primitive this reduces to; a
`ListView`'s row selection and focused-index tracking already consume
exactly this kind of event stream (`ItemsInserted` / `ItemsRemoved` /
`ItemsMoved` / `ItemUpdated`) correctly, because those events are also
what an ordinary user-driven insert/remove/reorder produces — a reload is
just another source of the same event vocabulary, not a special case a
selection has to separately account for.

---

## Replayable ops: why some APIs had to change shape

Every mutation this crate persists is enqueued as a **patch now, applied
later** — at the next debounce tick, possibly after a peer's write has
already landed on disk in the meantime (see "Cross-process safety, by
default"). That constraint rules out any operation whose meaning depends
on **transient state that may no longer be true by the time it replays**:

- **`toggle_pin` → `set_pinned(key, bool)`.** A toggle's effect depends on
  the *current* pinned state at the moment it runs. If two toggles for the
  same key are queued and a peer's concurrent write reorders when either
  actually applies, "toggle" can end up flipped the wrong number of times
  — the operation isn't idempotent, so replaying it against a
  document that has moved on since it was enqueued can silently produce
  the wrong answer. `set_pinned(key, pinned)` states the *desired end
  state* directly: replaying it against any starting document — including
  one a peer has already mutated — always lands on the same pinned value.
- **`NotificationArchiveModel::remove(index)` → `remove_by_id(id)`.** An
  index is a position in *this process's current view* of the list. By
  the time a queued mutation actually runs — after a debounce window, or
  after a peer's concurrent insert has already shifted every row after
  it — that index may no longer name the row it named when the call was
  made, or may not even be in bounds. An id is stable identity: it names
  the same row regardless of how many inserts or removals happened to
  its neighbors in the meantime.

The general principle: **an op must be safe to apply against *any*
document state consistent with "some other writer might have gotten there
first"**, not just the state the caller happened to observe when it made
the call. `ListOp<T>`'s own shape follows the same rule — `Remove(T::Key)`
carries only the key, never the value or a position, because a key is the
only thing a diff of "what's gone" can always produce, even once the
value itself is no longer available to compare against.

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
use bastyde_settings::{Migrator, Versioned};

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

`Migrator<T>` is cheaply `Clone` (each step's closure lives behind an
`Arc`, so cloning is a handful of refcount bumps) and is now taken **by
value** by `SettingsFile::load`/`load_strict`/`PersistedListModel::open`,
retained for the handle's whole lifetime — not just consulted once at
construction. That matters because cross-process safety means every
locked read-modify-write (not just the initial load) has to be able to
bring a peer's still-older on-disk schema forward, since a peer running
an older build might still be writing the pre-migration shape at any
point during this process's lifetime.

Corrupt files (parse failure, missing migration, post-migration
deserialize failure) are renamed to `<path>.broken-<unix_ts>` and the
`SettingsFile` falls back to `T::default()` so the app keeps running.
Apps that want the strict alternative (errors propagate, no fallback)
use `SettingsFile::load_strict`.

---

## Atomic write + debounce

Every flush goes through write-temp + rename via `tempfile`. Debounced
writes (`SettingsStore`, `PersistedListModel<T>`) are coalesced through a
single shared I/O thread (one `OnceLock<Sender<PoolMsg>>` per process).
Each writer holds a `WriterId`, and `Drop` synchronously flushes its
pending payload via the `Unregister` ack so end-of-process state is never
lost. `SettingsFile<T>` writes synchronously instead (see "Cross-process
safety, by default"), so it never has anything queued on this thread;
its own `flush_now()` is consequently a harmless no-op, kept only so
callers can flush every service uniformly without special-casing it.

`DebouncedWriter::schedule` — the method that actually enqueues a
`Patch` — is `pub(crate)`: application code never calls it directly.
`DebouncedWriter` is exported only as the public *type* that
`SettingsFile<T>`, `SettingsStore`, and `PersistedListModel<T>` are built
on and hold internally; the surface an external caller actually gets is
just enough to inspect or force-flush a service that wraps one:

```rust
use bastyde::settings::DebouncedWriter;
use std::time::Duration;

let w = DebouncedWriter::new(path.clone(), Duration::from_millis(500));
assert_eq!(w.path(), path.as_path());
w.flush_now().unwrap();  // synchronous force-flush; a harmless no-op
                          // here since nothing has been scheduled
```

`Duration::ZERO` makes every schedule flush on the worker's next
iteration — useful for tests, where `flush_now()` is the
deterministic anchor.

Application logic stays single-threaded; only the atomic write
happens on the worker. `Signal<T>::observe` callbacks fire on the UI
thread as ever — the path from `signal.set(v)` to the worker being
notified is cheap and synchronous.

---

## Threading and source-of-truth

`Signal<T>` and `*Model<T>` use `Rc<RefCell<>>`; the settings store
inherits that. **In-memory is the source of truth.** Disk is a
projection: seeded once at startup (lock-protected, so a peer mid-write at
startup can't hand this process a torn read), written on every mutation
(debounced or synchronous depending on the type), and re-synced whenever
a peer's write is noticed — either by the live `SettingsWatcher` ("Live
reload" above) or by an explicit `reload_if_stale()` /
`Reloadable::reload_from_disk()` call. Widgets never read from disk
directly.

This is why `OpenedSettings: Clone` is a *shared* clone, not a deep
one. Cloning each contained service is an `Rc` bump; mutations
through any clone are visible to every clone.

This "in-memory is the source of truth" model **used to** make
multi-process sharing last-write-wins by construction: two instances each
held their own private snapshot, and each write re-serialized from that
increasingly-stale copy with no re-read and no lock, so one process's
change was silently discarded by the other's next write. That is no
longer true of anything in this crate — see "Cross-process safety, by
default" above for the locked read-modify-write that replaced it, and
"Live reload" for how a peer's change reaches this process's live state
without this process having to touch anything itself.

---

## Checklist for common tasks

| Task | Recipe |
|---|---|
| Add a new scalar pref | Declare a `const KEY: SettingsKey<T> = SettingsKey::new(...)`, call `ctx.settings().signal_for(&KEY)` from `build()`, bind with `.text(...)` / `.color(...)` etc. |
| Persist a struct | Define `struct Foo { version: u32, ... }`, `impl Versioned for Foo`, open with `SettingsFile::load(path, Migrator::new())`, register via `app_state(handle.clone())`. |
| Persist a list | Define `T: Keyed + MruEntry`, `MruList::open(&paths, "name", N)`, register via `app_state(handle)`. |
| Auto-save / restore window geometry | `.settings(SettingsBundle::new().with_window_state(true))` and `.id("main")` on the `WindowConfig`. Done. |
| Add a v2 schema migration | Bump `CURRENT_VERSION`, register a `Migrator::new().step(1, ...)` transformation, plumb the migrator into `SettingsFile::load`. |
| Force a flush before a child process | `opened.flush_all()` (or per-service `flush_now()`). |
| Test settings code without touching `~/.config` | `AppPaths::for_testing(tempdir.path())` and `Duration::ZERO` for the debounce. |
| React to a peer's write outside a running `BastydeAppBuilder` app (e.g. a headless tool) | Call `Reloadable::reload_from_disk(&handle)` (or the cheaper `reload_if_stale()` on `SettingsFile<T>`) on your own schedule — there's no watcher without a running event loop. |
| Register an ad hoc persisted type for live reload | `ctx.app_state::<SettingsRegistry>().register(Rc::new(my_handle.clone()) as Rc<dyn Reloadable>)`, keep the returned `Rc` alive. |

---

## Reference

- Source: [`crates/bastyde-settings/src/`](../crates/bastyde-settings/src/)
- Window persist integration: [`crates/bastyde-app/src/window_persist.rs`](../crates/bastyde-app/src/window_persist.rs)
- Live-reload wiring: [`crates/bastyde-app/src/app.rs`](../crates/bastyde-app/src/app.rs) (search `settings_watch`)
- End-to-end demo: [`examples/recent_projects/src/main.rs`](../examples/recent_projects/src/main.rs)
- Related architecture topics: [`docs/multi-window.md`](multi-window.md), [`docs/data-models.md`](data-models.md), [`docs/reactive-theme.md`](reactive-theme.md)

### Out of scope — intentional

- **Encryption.** Plaintext TOML. Secrets go through a future
  `bastyde-secrets` crate against the OS keychain.
- **Cloud sync.** No.
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
- **A persisted tree collection.** `PersistedTreeModel<T>` was deleted
  (zero consumers, and it never got the ops-based hardening the rest of
  this crate did). Reintroduce it ops-based, from scratch, if a real
  consumer needs one — do not resurrect the deleted whole-snapshot
  version.
</content>
