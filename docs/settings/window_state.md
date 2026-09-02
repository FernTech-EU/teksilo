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

## `record` is debounced, not synchronous

See `WindowStateService`'s "Why this is debounced, unlike
`SettingsFile`" doc below for the full rationale: `record`/`forget`
update the in-memory state instantly and schedule a coalesced, locked
read-merge-write via a `DebouncedWriter` — a live window drag (which
calls `record` once per reported geometry frame) costs one disk
write per debounce window, not one per frame.

In a typical Teksilo app, `WindowStateService` is managed by the
framework's `SettingsBundle` and wired automatically when the
`WindowConfig` carries a stable `id(...)` — no widget-side plumbing
needed. The service is only used directly when building custom window
management or embedding it outside the standard `TeksiloAppBuilder`
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
use teksilo_settings::{AppPaths, WindowStateService, PerWindowState};
use teksilo_core::WindowPlacement;

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

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-settings/latest/teksilo_settings/index.html)

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

Entries are `PerWindowState`, keyed by a stable string label. `state_for`
reads straight from memory with no I/O.

## Why this is debounced, unlike `SettingsFile`

`SettingsFile`'s `mutate` is a *synchronous* locked read-modify-write, which
is right for a document written rarely (a settings change; one record per
backup). Window geometry is the opposite: `teksilo-app`'s `window_persist`
observes the `size` / `position` / `placement` signals and calls `record`
on **every change** — i.e. once per frame while the user drags a window. A
synchronous `flock` + read + parse + serialize + fsync per frame would make
dragging visibly janky.

So this service owns its own `DebouncedWriter` and schedules a
`WindowOp` patch per `record`, exactly like [`crate::PersistedListModel`]:
in-memory state updates instantly (so `state_for` is always current), and
the burst collapses into **one** locked read-merge-write at the debounce
deadline. Frequent writes ⇒ debounced patch; rare writes ⇒ synchronous
locked RMW. Both are cross-process correct; they differ only in when the
disk write happens.


```rust
pub struct WindowStateService { /* fields */ }
```

### Methods

#### `pub fn open(paths: &AppPaths) -> Result<Self, SettingsFileError>`

Open the window-state file at the standard location inside `paths`.

#### `pub fn open_with_delay(paths: &AppPaths, delay: Duration) -> Result<Self, SettingsFileError>`

Open at the standard location with an explicit debounce window.

#### `pub fn open_at(path: PathBuf, delay: Duration) -> Result<Self, SettingsFileError>`

Open the window-state file at an explicit `path`.

`delay` is the debounce window: geometry changes arriving inside it
coalesce into a single disk write. `Duration::ZERO` writes on the
worker's next tick (used by tests).

#### `pub fn state_for(&self, label: &str) -> Option<PerWindowState>`

Saved state for the window with `label`, or `None` if there's no entry.

#### `pub fn record(&self, state: PerWindowState) -> Result<(), SettingsFileError>`

Record the current geometry for `label`, replacing any prior entry.

Updates memory immediately and schedules a debounced, locked
read-merge-write — so a drag costs one write, not one per frame.

#### `pub fn forget(&self, label: &str) -> Result<(), SettingsFileError>`

Forget the entry for `label`.

#### `pub fn labels(&self) -> Vec<String>`

All recorded labels. Useful for "restore last session" features.

#### `pub fn flush_now(&self) -> Result<(), SettingsFileError>`

Flush any pending geometry to disk immediately, bypassing the debounce.

Flushes the **op queue**, never a re-derived snapshot of the in-memory
document — dumping the snapshot is exactly how a cleanly-exiting process
would erase a peer's window entry.

#### `pub fn path(&self) -> &Path`

Absolute path of the underlying TOML file managed by this service.
