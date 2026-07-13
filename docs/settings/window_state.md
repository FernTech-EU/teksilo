<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PerWindowState

Per-window geometry persistence via `WindowStateService`.

Each named window — identified by a stable string label such as
`"main"` or `"inspector"` — can have its position, size, and
placement (`Floating` / `Maximized` / `Fullscreen`) saved across
sessions. In-memory is the source of truth: `state_for` reads
directly from memory without touching disk. On load, the file is
migrated through `Migrator` steps (currently v1 → v2: `maximized:
bool` → `placement: WindowPlacement`) before deserializing, and
corrupt files are quarantined automatically by `SettingsFile`.

## `record` is synchronous, not debounced

This type is built on `SettingsFile<WindowStateFile>`, whose writes
are always a synchronous locked read-modify-write (see `file.rs`'s
module docs) — there is no debounce window to coalesce a burst of
calls into one flush. `record` therefore takes a lock, re-reads and
re-migrates the file, applies the change, and writes it back *on the
calling thread*, every single time it is invoked. That is the right
trade-off for how this type is meant to be called — a handful of
writes around a window's lifetime — but `bastyde-app`'s
`window_persist` module (`crates/bastyde-app/src/window_persist.rs`)
currently wires `record` to fire on *every* `Signal` change of a
window's size/position/placement, which on X11/Windows/macOS means
once per frame during a live drag or resize (Wayland mostly spares
position, since the compositor rarely notifies apps of it — see the
Wayland caveat below — but size still updates during a resize).
Concretely: a live window drag currently does a synchronous
lock-acquire + file read + parse + serialize + atomic write on every
reported geometry change, not a debounced one. This is a known,
intentional-for-now performance characteristic of the current
wiring, not of `WindowStateService` itself — a caller that wants to
coalesce a drag into one write should debounce at the call site
(e.g. only call `record` from a "drag ended" / periodic-timer
observer) rather than from every raw geometry `Signal`.

In a typical Bastyde app, `WindowStateService` is managed by the
framework's `SettingsBundle` and wired automatically when the
`WindowConfig` carries a stable `id(...)` — no widget-side plumbing
needed. The service is only used directly when building custom window
management or embedding it outside the standard `BastydeAppBuilder`
path.

## Wayland caveat

Wayland does not let applications choose their window position;
the compositor places windows. Position fields (`x`, `y`) are still
recorded and persisted (so the config roams across an X11/Wayland
switch), but a Wayland host must ignore them when restoring.
Width, height, and `WindowPlacement` are honored on every platform.

## Example

```ignore
use std::time::Duration;
use bastyde_settings::{AppPaths, WindowStateService, PerWindowState};
use bastyde_core::WindowPlacement;

// In tests use AppPaths::for_testing(tmp_dir); in production use AppPaths::new(...).
let paths = AppPaths::for_testing(std::path::Path::new("/tmp/my-app"));
let svc = WindowStateService::open_with_delay(&paths, Duration::ZERO).unwrap();

// On window move / resize, record the new geometry.
svc.record(PerWindowState {
    label: "main".into(),
    x: 100, y: 80,
    width: 1280, height: 800,
    placement: WindowPlacement::Floating,
}).unwrap();

// On next launch, restore if available.
if let Some(saved) = svc.state_for("main") {
    let ready = saved.sanitize((400, 300), (1920, 1080));
    println!("restore to {}x{} at ({},{})", ready.width, ready.height, ready.x, ready.y);
}
```


## Builder methods at a glance

`sanitize`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_settings/index.html)

## `pub struct PerWindowState`

Persisted geometry for one labeled window.

`placement` captures the full `WindowPlacement` enum (Floating /
Maximized / Fullscreen / Minimized). On restore, `Minimized` is
downgraded to `Floating` so the app doesn't appear to fail to
start; every other variant is honored if the OS supports it
(Wayland will ignore `position` regardless — see `sanitize`'s
docs).

```rust
pub struct PerWindowState { /* fields */ }
```

### Methods

#### `pub fn sanitize(&self, min_size: (u32, u32), work_area: (u32, u32)) -> PerWindowState`

Validate this state against a `(width, height)` work area
(typically the size of the largest available monitor's usable
region) and return a sanitized copy:

* `width` / `height` are clamped to `[min, work_area]`. If the
  minimum is larger than the work area the work area wins —
  this should not happen with sensible mins (e.g. 320x240).
* The position is checked: if the **top-left** point lies
  outside `[0, work_area_w) x [0, work_area_h)` *and* the
  window would not have at least 50 logical-pixel intersection
  with the work area, the position is recentered on the
  monitor so the window comes back on screen instead of
  spawning at coordinates from a missing monitor.
* `maximized` and `label` are preserved.

Use this on app startup with `(work_area_w, work_area_h)`
pulled from the OS (e.g. winit's `MonitorHandle::size()` minus
known taskbars). Without an OS hint, pass conservative
fallbacks like `(1920, 1080)` — the result still improves on
re-using stale coordinates from a monitor that's no longer
connected.

## `pub struct WindowStateService`

Persistent, in-memory-backed store for per-window geometry.

Holds a `SettingsFile`-backed collection of `PerWindowState` entries
keyed by a stable string label. Each `record` call performs a
synchronous locked read-modify-write to disk (see the module docs'
"`record` is synchronous, not debounced" section — this is not a
debounced flush); `state_for` reads directly from memory with no I/O.


```rust
pub struct WindowStateService { /* fields */ }
```

### Methods

#### `pub fn open(paths: &AppPaths) -> Result<Self, SettingsFileError>`

Open the window-state file at the standard location inside `paths`.

#### `pub fn open_with_delay(paths: &AppPaths, delay: Duration) -> Result<Self, SettingsFileError>`

Open the window-state file at the standard location inside `paths`.

`delay` is accepted (and ignored) purely so `crate::SettingsBundle`
can open every service it manages with one uniform debounce
argument: `SettingsFile`'s writes are always synchronous locked
read-modify-writes now (see `file.rs`'s module docs) — `record` /
`forget` calls are one disk write per record, immediately, with no
burst to coalesce.

#### `pub fn open_at(path: PathBuf, delay: Duration) -> Result<Self, SettingsFileError>`

Open the window-state file at an explicit `path`. `delay` is
accepted and ignored — see `open_with_delay`.

#### `pub fn state_for(&self, label: &str) -> Option<PerWindowState>`

Saved state for the window with `label`, or `None` if there's
no entry yet.

#### `pub fn record(&self, state: PerWindowState) -> Result<(), SettingsFileError>`

Record the current geometry for `label`. Replaces any prior
entry. Synchronous: performs a locked read-modify-write to disk
immediately, on the calling thread — see the module docs' "`record`
is synchronous, not debounced" section.

#### `pub fn forget(&self, label: &str) -> Result<(), SettingsFileError>`

Forget the entry for `label`.

#### `pub fn labels(&self) -> Vec<String>`

All recorded labels. Useful for "restore last session" features.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

A harmless no-op: `record` / `forget`
already write synchronously, so nothing is ever pending. Kept so
callers that hold a `WindowStateService` alongside debounced
services (`SettingsStore`, `MruList`) can flush everything
uniformly without special-casing this type.

#### `pub fn path(&self) -> &Path`

Absolute path of the underlying TOML file managed by this service.
