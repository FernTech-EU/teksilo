# Custom `TitleBar` Widget — M3+ Plan

**Companion to:** fern-ui-architecture.md, fern-ui-milestones.md
**Status:** Living document — M1 + M2 + M3 shipped; M4 / M5 (Windows)
and M6 (polish) remain. Reference docs at
[docs/title-bar.md](../title-bar.md).

This document picks up where the original title-bar plan (in the private
`~/.claude/plans/` scratch area) left off. It records:

1. What M1, M2, and M3 actually landed.
2. Framework changes that were needed along the way but weren't in the
   original plan.
3. Known follow-ups — resolved and still-open.
4. Deferred work and caveats from M3.
5. The scope of M4 and M5 (Windows) and M6 (polish).
6. Risks and the reading list.

---

## 1. What's done (M1 + M2 + M3)

### M1 — Platform trait + Wayland + X11 stubs

All landed under `fern-core` and `fern-platform`:

- [`fern_core::window_chrome`](../crates/fern-core/src/window_chrome.rs) —
  new module defining `PlatformTitleBarHost`, `HitRegions`,
  `ResizeBorders`, `ResizeEdge`, `PlatformError`. Re-exported from
  `fern_core` top level so `fern-widgets` can hold
  `Rc<dyn PlatformTitleBarHost>` without a `fern-platform` dependency.
- [`fern_platform::title_bar_host`](../crates/fern-platform/src/title_bar_host.rs) —
  factory `create_title_bar_host(Arc<Window>) -> Result<Rc<dyn …>, …>`
  dispatched via `cfg(target_os)` plus `active_window_system()` for
  Linux. Returns `Rc`, not `Box`, so the same handle can sit on the
  `WidgetTree`, the `ManagedWindow`, and inside widgets simultaneously.
- [`WaylandHost`](../crates/fern-platform/src/title_bar_host/wayland.rs) —
  full delegation to winit 0.30: `drag_window` / `drag_resize_window(dir)`
  / `show_window_menu(pos)` / `set_minimized` / `set_maximized` /
  `is_maximized`. Full mapping from `ResizeEdge` to winit's
  `ResizeDirection`. `close()` is currently a no-op; see §3.
- [`MacOsHost`](../crates/fern-platform/src/title_bar_host/macos.rs) and
  [`WindowsHost`](../crates/fern-platform/src/title_bar_host/windows.rs)
  are stubs returning `PlatformError::Unsupported`.
- [`X11Host`](../crates/fern-platform/src/title_bar_host/x11.rs) is an
  unconstructable placeholder; the factory logs a warning and returns
  `Unsupported` when `active_window_system() == WindowSystem::X11` or
  `Unknown`, and the application silently falls back to server-side
  decorations.

Wiring into `fern-app`:

- [`WindowConfig::custom_chrome(bool)`](../crates/fern-app/src/window_config.rs) —
  opt-in flag.
- [`WindowManager::create_window`](../crates/fern-app/src/window_manager.rs) —
  applies `with_decorations(false)` on Wayland when `custom_chrome` is
  true, calls `create_title_bar_host(pw.window_arc())`, stores the host
  on `ManagedWindow.title_bar_host`, and installs it on the `WidgetTree`
  via `tree.set_title_bar_host(host.clone())` **before** invoking the
  `root_builder` closure so the closure can fetch it.
- `FernAppBuilder::custom_chrome(bool)` propagates into the implicit
  `WindowConfig` built by `.run()`.
- `wm.title_bar_host(fern_id)` accessor returns the `Rc` for external
  consumers (future: for dispatching Windows synthetic button events).
- New `PlatformWindow::window_arc()` accessor returns
  `Arc<winit::Window>` so the factory can share ownership.

### M2 — Widget, resize frame, framework fixes

#### Widgets that landed

Five widgets, all in [`fern-widgets/src/title_bar/`](../crates/fern-widgets/src/title_bar/):

| Widget | Purpose |
|---|---|
| [`TitleBar`](../crates/fern-widgets/src/title_bar.rs) | Composing root: HStack with `[leading-inset Spacer?, leading, DragRegion, trailing, trailing-inset Spacer?, WindowControls?]`. Paints its own background + bottom border. Builder API: `.height()`, `.background()`, `.border()`, `.leading()`, `.center()`, `.trailing()`, `.close_action()`, `.is_maximized_signal()`. |
| [`DragRegion`](../crates/fern-widgets/src/title_bar/drag_region.rs) | `is_spacer() == true`. Wires `on_drag` → `host.begin_drag()` on `GestureEvent::DragStarted { button: Primary }`, `on_double_tap` → `host.toggle_maximize()`, `on_pointer_event` filtering secondary button → `host.show_window_menu(pos)`. Publishes its physical bounds into the shared `HitRegions` from `paint()` so the future Windows backend can return `HTCAPTION`. |
| [`WindowControls`](../crates/fern-widgets/src/title_bar/controls.rs) | HStack of three `ControlButton` cells (min/max/close). No `Switcher` — the max button shows a static `□` until M3+ wires `is_maximized` from `WindowEvent::Resized`. Close routes through the user's `TitleBar::close_action` override if set, otherwise `host.close()`. |
| [`ControlButton`](../crates/fern-widgets/src/title_bar/controls.rs) | Compact flush-fitting button: `FixedSize → ZStack(RectWidget bg, Center(TextWidget glyph))`. Reactive hover via a `Signal<Color>` bound to the background. Glyphs: `—` (U+2014), `□` (U+25A1), `×` (U+00D7). U+00D7 specifically because U+2715 ✕ is missing from many default Linux sans-serif fonts. |
| [`ResizeStrip`](../crates/fern-widgets/src/title_bar/resize_strip.rs) | Invisible edge / corner cell that calls `host.begin_resize(edge)` on primary-button down. Three constructors: `::horizontal(host, edge, t)`, `::vertical(host, edge, t)`, `::corner(host, edge, size)`. Sets the correct cursor per edge (including the two diagonals). |
| [`WindowFrame`](../crates/fern-widgets/src/title_bar/window_frame.rs) | Invisible resize-handle overlay. Content child fills the full window; 4 edge strips and 4 corner squares overlay the outer `t` pixels. Hit-test priority is handled by children-in-reverse-order: corners win over edges, edges win over content. |

Demo lives at [`examples/title_bar_demo/`](../examples/title_bar_demo/)
and exercises the full stack on Wayland.

#### Framework changes along the way

These weren't in the original plan but were required to make the V2
attached-handler API actually route gestures correctly. Keep these in
mind when designing new gesture-driven widgets — they change the
semantics of `HandlerSet` non-trivially.

1. **`HandlerSet::on_double_tap` and `HandlerSet::on_long_press`** —
   the inner `EventHandlers` struct had the fields but `HandlerSet`
   (the public builder used by `apply_self_handlers`) had no setter.
   Added in
   [`widget_builder.rs`](../crates/fern-core/src/widget_builder.rs).
2. **Gesture arena auto-wiring in
   [`event_dispatch_impl.rs`](../crates/fern-core/src/widget_tree/event_dispatch_impl.rs)** —
   `try_handler_bubble` previously only built a `GestureArena` when
   `on_tap.is_some()`, and populated it with just `TapRecognizer`. Any
   widget that wired `on_drag` or `on_double_tap` (but not `on_tap`)
   got no arena at all and the handlers never fired. Replaced with
   `ensure_gesture_arena` which installs `TapRecognizer` /
   `DoubleTapRecognizer` / `DragRecognizer` / `LongPressRecognizer`
   based on which handlers are set, plus `dispatch_recognized_gesture`
   to route every `GestureEvent` variant.
3. **Raw event precedence** — the same dispatch used to return
   `Handled` from the gesture arena without giving `on_pointer_event`
   a look at the event. Reordered so `on_pointer_event` runs first and
   can short-circuit the arena by returning `Handled` (useful for
   right-click handlers co-existing with drag/double-tap on the same
   widget).
4. **`GestureArena::process` reset-on-Down** —
   [`gesture.rs`](../crates/fern-core/src/gesture.rs) used to call
   `recognizer.reset()` on every `RawPointerEvent::Down`, which wiped
   `DoubleTapRecognizer::first_tap_time` before the second tap could
   be recognised. Removed the `reset()` call, kept only the
   `failed = false` clearing. The recognisers all overwrite their
   per-sequence state on `Down` themselves, so the external reset was
   both unnecessary and harmful.
5. **`CursorIcon::NeswResize` and `CursorIcon::NwseResize`** added to
   [`widget.rs`](../crates/fern-core/src/widget.rs) and mapped to
   winit's `NeswResize` / `NwseResize` in
   [`app.rs`](../crates/fern-app/src/app.rs).

Test count before M2: 562. After M2: 838. +276 net, of which the
new title-bar / window-frame tests account for 12. The rest come from
other work in parallel.

### M3 — macOS backend + §2.1–§2.3 follow-ups

All landed on `feat/macos-titlebar`:

- [`MacOsHost`](../../crates/fern-platform/src/title_bar_host/macos.rs)
  replaces the M1 stub. Retains the underlying
  `Retained<NSWindow>` (obtained from winit's `RawWindowHandle::AppKit`
  → `ns_view.window()`), measures the traffic-light cluster once at
  construction via `standardWindowButton(…).frame()`, and drives zoom
  via `-[NSWindow performZoom:]` so the OS animates the transition.
  `begin_drag` still delegates to winit's `drag_window`; edge resize is
  unsupported because the native `NSWindow` frame services it. No
  widget-layer drag or resize handles run on macOS.
- **Trait additions** (see [`fern_core::window_chrome`](../../crates/fern-core/src/window_chrome.rs)):
  `needs_custom_resize_handles() -> bool`, `is_maximized_signal() -> Signal<bool>`,
  and `notify_window_resized(&self)` with a default impl that refreshes
  the signal from `is_maximized()`. `TitleBar` now uses the host's
  signal rather than an internally-created one; OS-initiated maximize
  (green-light, drag-to-top) flows through the same path.
- **`TitleBarHostCallbacks`** — a new `Clone` struct carrying an
  `Rc<dyn Fn()>` close callback, plus a `::noop()` helper. Breaks the
  fern-platform ↔ fern-app cycle: the app constructs a closure that
  boxes a `CloseWindowRequest { fern_id }` onto `AppEvent::External`;
  the host calls it opaquely. Replaces §2.3's no-op `close()` on every
  backend simultaneously.
- **`needs_custom_resize_handles` gating in the demo** — the
  `WindowFrame` overlay is now conditional on `host.needs_custom_resize_handles()`,
  so macOS gets the native `NSWindow` edge-resize and Wayland/Windows
  keep the invisible strips.
- **Switcher glyph swap** restored in `WindowControls`. Each Switcher
  child (`□` U+25A1 / `❐` U+2750) has its own static a11y name
  ("Maximize" / "Restore"); the hidden child's a11y node doesn't reach
  AT, so no reactive name is needed.
- **macOS window attributes** — `WindowManager::create_window` applies
  `with_titlebar_transparent(true) + with_fullsize_content_view(true) +
  with_title_hidden(true)` under `#[cfg(target_os = "macos")]` when
  `custom_chrome` is set.
- **`WindowEvent::Resized` hook** — the app's resize handler now calls
  `host.notify_window_resized()` after `platform_window.resize`, which
  is what keeps the Switcher glyph in sync with OS-initiated zoom.
- Reference doc landed at [docs/title-bar.md](../title-bar.md).

Visual QA walked the §3.5 checklist on Apple Silicon — see the
"Acceptance" block below for the surviving caveats.

Commit: `feat(title-bar): macOS backend + OS-sync maximize signal`.

---

## 2. Known follow-ups carried out of M2

### Resolved in M3

- **2.1 `is_maximized` signal OS-sync** — DONE. Host owns the
  `Signal<bool>`, `notify_window_resized()` refreshes it from
  `WindowEvent::Resized`.
- **2.2 Maximize / restore glyph swap** — DONE. `Switcher` restored in
  `WindowControls` (U+25A1 / U+2750). Both glyphs confirmed present in
  the fonts shipped with Wayland/macOS/Windows defaults.
- **2.3 `host.close()` no-op on Wayland** — DONE. `TitleBarHostCallbacks`
  + `AppEvent::External(CloseWindowRequest)` routing; every backend now
  closes correctly. The demo's `close_action(|ctx| ctx.close_window())`
  override is kept as a redundancy illustrating the app-level hook, but
  is no longer required.

### Still open

### 2.4 Title bar hit-region publishing isn't wired yet

`DragRegion` and `ControlButton` don't yet call
`host.update_hit_regions()` in their `paint()`. The Wayland and macOS
backends would treat it as a no-op, but M4's Windows backend depends on
it. Needs to be wired in M4 alongside the WM_NCHITTEST work, not
earlier (the data has nowhere to go yet).

### 2.5 `TitleBar` doesn't re-publish its bounds across frames

Related to 2.4: when the window resizes, the drag region and button
rects change. The current stub publishes a single rect from
`paint()`, which works as long as the backend reads the hit regions
once per `WM_NCHITTEST`. Needs testing in M4; if it flickers we may
need a dedicated `after_layout` hook in `fern-core`.

---

## 3. M3 — macOS backend (shipped)

Retrospective of what actually landed, what was deferred, and the
caveats discovered during implementation. The as-built reference is
[docs/title-bar.md](../title-bar.md); detailed API docs are inline on
[`MacOsHost`](../../crates/fern-platform/src/title_bar_host/macos.rs)
and [`PlatformTitleBarHost`](../../crates/fern-core/src/window_chrome.rs).

### 3.1 Shipped

- NSWindow attributes (transparent titlebar + full-size content view +
  hidden title) applied in `WindowManager::create_window` under
  `#[cfg(target_os = "macos")]`.
- `MacOsHost` with:
  - `Retained<NSWindow>` extracted from
    `RawWindowHandle::AppKit` → `NSView::window()`, held for the
    lifetime of the host so `isZoomed` / `performZoom` avoid
    re-traversing the view chain.
  - `measure_traffic_light_inset` — reads `standardWindowButton(Close)`
    and `standardWindowButton(Zoom)` frames once at construction,
    returns `leading_edge + cluster_width + 12pt trailing_padding` as
    a plain `Size` (no interior mutability — the cluster doesn't move
    at runtime while the title-bar height is the OS default).
  - `toggle_maximize` → `-[NSWindow performZoom:]` (matches green-light
    click, honours `NSWindowDelegate.windowWillUseStandardFrame:`,
    animates).
  - `begin_drag` → winit `drag_window`; `begin_resize` returns
    `Unsupported`; `show_window_menu` returns `Ok(())` (no AppKit
    equivalent of xdg-shell's client-requested menu).
  - `renders_custom_controls = false` and
    `needs_custom_resize_handles = false` — the widget yields the
    leading band to the OS and skips its own control/resize overlays.
- Close routing via `TitleBarHostCallbacks::request_close` →
  `AppEvent::External(CloseWindowRequest)` → `WindowManager::queue_close`.
  Wayland shares the mechanism; Windows M5 will reuse it for synthetic
  button events.
- `is_maximized_signal` + `notify_window_resized` added to the trait
  (default impl syncs from `is_maximized()`). `WindowEvent::Resized`
  calls it in `FernAppHandler`; the widget's Switcher swaps □/❐ in
  sync with OS-initiated zoom.
- Demo gates `WindowFrame` on `host.needs_custom_resize_handles()`.
- Reference doc at [docs/title-bar.md](../title-bar.md).

### 3.2 Deferred — custom title-bar height ≠ OS default

The plan called for a `position_traffic_lights(x, y)` helper (old §3.3)
for apps that set a title bar taller or shorter than the OS's 22-pt
default. **Not implemented.** The current design assumes callers use
the default-compatible height (`TitleBar::height(40.0)` is fine
because the traffic-light cluster sits in the top ~22 pt of the band
and the extra height below it is app-drawable). When an app chooses
a non-standard height in the future:

1. Add `MacOsHost::position_traffic_lights(x: f32, y: f32)` that calls
   `setFrameOrigin` on each `standardWindowButton`.
2. Call it from `notify_window_resized` — a `TODO` marker at
   [macos.rs:185](../../crates/fern-platform/src/title_bar_host/macos.rs#L185)
   points back here.
3. Mind the timing risk (see §7, risk #2): defer by one frame or run
   inside NSWindow's own layout callback, otherwise rapid resize drags
   flicker.

### 3.3 Caveats

- **`NSWindow.isZoomed` does not track native macOS fullscreen.**
  Green-light zoom and `-[NSWindow performZoom:]` flip `isZoomed`;
  green-light + Option (or `-[NSWindow toggleFullScreen:]`) puts the
  window on its own Space and `isZoomed` stays `false`. The title bar
  is hidden during fullscreen anyway, so this is benign; documented
  inline on `MacOsHost::is_maximized`.
- **`reserved_leading_inset` is measured once at construction.** Cocoa
  guarantees the cluster stays in place across window resizes and DPI
  changes, so no refresh is wired in. If that assumption ever breaks
  (e.g. a future macOS changes cluster metrics mid-session), move the
  remeasure into `notify_window_resized`.
- **`is_maximized_signal` is read-only for callers.** The host drives
  it from `notify_window_resized`; speculative widget-side writes on
  button click de-sync it from the OS (especially when the user
  cancels a zoom by clicking off the button during animation).
  Documented on `TitleBar::is_maximized_signal`.
- **`PlatformError::Os(String)` on AppKit handle mismatch** (rather
  than `Unsupported`): `Unsupported` is reserved for "this platform
  cannot do X" (e.g. X11 has no custom-chrome backend at all);
  `Os(String)` covers runtime call failures including an unexpected
  `RawWindowHandle` variant from winit.

### 3.4 Acceptance — as-built

Walked on Apple Silicon on `feat/macos-titlebar`:

- [x] Traffic lights visible at launch, stable under rapid resize drag.
- [x] Leading app label doesn't overlap the traffic lights (spacer
      reserves `reserved_leading_inset.width`).
- [x] No custom min/max/close cluster at the trailing edge.
- [x] Middle-band drag moves the window (winit `drag_window`).
- [x] Double-click the band → `performZoom:` animates the zoom toggle.
- [x] Right-click the band → no-op, no crash.
- [x] Edge resize works via the native NSWindow frame; no
      `WindowFrame` overlay installed.
- [x] Close button closes the window via `host.close()` alone (the
      demo's `close_action` override is redundant now).
- [x] Title-bar height is exactly the value passed to `.height()`.
- [x] OS-initiated maximize (drag to screen top) flips the Switcher
      glyph to ❐ within one frame of `WindowEvent::Resized`.

No feature-gated live-NSWindow test (`macos-live`) was added — the
backend is pure visual QA territory, and the rest of the workspace's
838 tests exercise the widget layer in isolation.

---

## 4. M4 — Windows backend, phase 1

**Target**: custom frame rendering replaces the native title bar while
preserving Aero Snap, native drop shadow, Win11 rounded corners, and
the 8-edge resize borders. No custom button interception yet — that's
M5.

### 4.1 Window attributes at creation time

Unlike Wayland, on Windows **we keep `with_decorations(true)`**. The
DWM custom-frame recipe relies on the native frame still being present
so the OS draws the shadow and rounded corners. See the Microsoft DWM
custom-frame article in the reading list.

No changes to `WindowManager::create_window`'s `window_attrs` on
Windows — the M1 wiring already handles this correctly by skipping the
`with_decorations(false)` call.

### 4.2 `windows` crate features

Extend the existing `windows = 0.61` features in
[`fern-platform/Cargo.toml`](../crates/fern-platform/Cargo.toml):

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.61", features = [
    "Win32_UI_WindowsAndMessaging",      # existing
    "Win32_UI_Accessibility",            # existing
    "UI_ViewManagement",                 # existing
    "Win32_Graphics_Dwm",                # NEW — DwmExtendFrameIntoClientArea
    "Win32_Graphics_Gdi",                # NEW — MonitorFromWindow, monitor metrics
    "Win32_UI_HiDpi",                    # NEW — GetSystemMetricsForDpi, GetDpiForWindow
    "Win32_UI_Controls",                 # NEW — SetWindowSubclass / DefSubclassProc
] }
```

We stick with the existing `windows` crate, not `windows-sys`, to
avoid compiling two parallel bindings (decision from the original plan
§3 questions).

### 4.3 `WindowsHost` construction

Replace the M1 stub in
[`fern-platform/src/title_bar_host/windows.rs`](../crates/fern-platform/src/title_bar_host/windows.rs):

```rust
pub struct WindowsHost {
    window: Arc<winit::window::Window>,
    hwnd: HWND,
    subclass_id: usize,
    // Shared hit-region table: the widget writes into it from `paint()`,
    // the subclass proc reads it from WM_NCHITTEST. Arc<Mutex> because
    // the subclass proc can be re-entered, and we don't want `RefCell`
    // panics under re-entrancy.
    hit_regions: Arc<Mutex<HitRegions>>,
    // For posting synthetic events back into the fern-app event loop.
    event_proxy: AppEventProxy,
}
```

At construction:

1. Extract the `HWND` from the winit window via
   `raw_window_handle::HasWindowHandle`:
   ```rust
   let handle = window.window_handle()?.as_raw();
   let RawWindowHandle::Win32(h) = handle else {
       return Err(PlatformError::Unsupported);
   };
   let hwnd = HWND(h.hwnd.get() as _);
   ```
2. Call `DwmExtendFrameIntoClientArea` with margins
   `MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 1, cyBottomHeight: 0 }`.
   The 1-pixel top inset is the magic that preserves Win11 rounded
   corners — setting 0 there gives square corners. Document this
   inline (the Handmade Network article explains it).
3. `SetWindowPos(hwnd, .., SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE
   | SWP_NOZORDER | SWP_NOACTIVATE)` to force the frame recomputation.
4. Install a window subclass via
   `SetWindowSubclass(hwnd, proc, SUBCLASS_ID, dw_ref_data)`. The
   `dw_ref_data` is a raw pointer to a heap-allocated
   `Box<WindowsHostSubclassData>` containing an `Arc<Mutex<HitRegions>>`
   clone, the event proxy, and anything else the proc needs.
5. On `Drop`, call `RemoveWindowSubclass` and `Box::from_raw` to
   release the subclass data.

**Subclassing vs `SetWindowLongPtrW`** — this was flagged as risk #1 in
the original plan. winit 0.30 uses `SetWindowLongPtrW` internally to
install its own proc (check
`winit/src/platform_impl/windows/window.rs` to confirm before
writing). Using `SetWindowSubclass` with a unique subclass id chains
correctly regardless of winit's installation method, because the
comctl32 subclass chain is separate from the raw wndproc slot. That's
the safer bet.

### 4.4 Subclass proc: `WM_NCCALCSIZE`

```c
case WM_NCCALCSIZE:
    if (wparam == TRUE) {
        NCCALCSIZE_PARAMS* p = (NCCALCSIZE_PARAMS*)lparam;
        // Zero the non-client insets so the client area covers the full window.
        // Preserve a 1-pixel top resize border when NOT maximized.
        if (is_maximized(hwnd)) {
            int padded = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            int frame  = GetSystemMetricsForDpi(SM_CYFRAME, dpi);
            p->rgrc[0].top    += padded + frame;
            p->rgrc[0].left   += padded + frame;
            p->rgrc[0].right  -= padded + frame;
            p->rgrc[0].bottom -= padded + frame;
            // Clamp to monitor work area so the maximized window
            // doesn't overflow the taskbar.
            HMONITOR mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            MONITORINFO info; info.cbSize = sizeof(info);
            GetMonitorInfoW(mon, &info);
            IntersectRect(&p->rgrc[0], &p->rgrc[0], &info.rcWork);
        } else {
            // Leave a 1-pixel top reserved for the resize border.
            // The other three sides are zeroed so the client area
            // touches the window edge.
            // top already correctly set; left/right/bottom zeroed.
        }
        return 0;
    }
    break;
```

Translate carefully to Rust + `windows = 0.61`. The
`GetSystemMetricsForDpi` / `MonitorFromWindow` imports come from the
features added in §4.2.

### 4.5 Subclass proc: `WM_NCHITTEST` (M4 version — no custom buttons)

```rust
WM_NCHITTEST => {
    let screen_x = GET_X_LPARAM(lparam);
    let screen_y = GET_Y_LPARAM(lparam);
    let mut pt = POINT { x: screen_x, y: screen_y };
    ScreenToClient(hwnd, &mut pt);

    let dpi = GetDpiForWindow(hwnd);
    let border_thickness = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        + GetSystemMetricsForDpi(SM_CXFRAME, dpi);
    let caption_thickness = border_thickness; // or a configurable value

    // Resize borders win first — they're the 8 outer edges.
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let on_top    = pt.y < border_thickness;
    let on_bottom = pt.y >= rect.bottom - border_thickness;
    let on_left   = pt.x < border_thickness;
    let on_right  = pt.x >= rect.right - border_thickness;

    return match (on_top, on_bottom, on_left, on_right) {
        (true,  false, true,  false) => HTTOPLEFT,
        (true,  false, false, true ) => HTTOPRIGHT,
        (false, true,  true,  false) => HTBOTTOMLEFT,
        (false, true,  false, true ) => HTBOTTOMRIGHT,
        (true,  false, _,     _    ) => HTTOP,
        (false, true,  _,     _    ) => HTBOTTOM,
        (_,     _,     true,  false) => HTLEFT,
        (_,     _,     false, true ) => HTRIGHT,
        _ => {
            // Read shared hit_regions, check drag region.
            let regions = hit_regions.try_lock();
            if let Some(regions) = regions {
                for drag in &regions.drag {
                    if drag.contains_point(pt.x, pt.y) { return HTCAPTION; }
                }
            }
            HTCLIENT
        }
    };
}
```

Mutex reentrance: use `try_lock` — if the lock is held (shouldn't
happen in practice but handles re-entrant wndproc invocations), fall
through to `HTCLIENT` so we never deadlock the message pump. Risk #4
in the original plan.

### 4.6 Method impls on `WindowsHost`

- `begin_drag()`:
  ```rust
  ReleaseCapture();
  SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, make_lparam(pt));
  ```
  where `pt` is a screen coordinate. The easiest source of the
  coordinate is to read `GetCursorPos` at call time.
- `begin_resize(edge)`:
  ```rust
  let hit = match edge {
      ResizeEdge::Top => HTTOP,
      ResizeEdge::TopRight => HTTOPRIGHT,
      // ...
  };
  ReleaseCapture();
  SendMessageW(hwnd, WM_NCLBUTTONDOWN, hit, make_lparam(pt));
  ```
- `minimize()`: `ShowWindow(hwnd, SW_MINIMIZE)`.
- `toggle_maximize()`: check `IsZoomed`, call `ShowWindow(hwnd, SW_RESTORE)` or `SW_MAXIMIZE`.
- `close()`: `PostMessageW(hwnd, WM_CLOSE, 0, 0)`.
- `is_maximized()`: `IsZoomed(hwnd)` returns BOOL.
- `update_hit_regions(regions)`: convert the widget-layer logical
  rects to physical pixels using the cached DPI scale, store into
  `hit_regions`.

### 4.7 Scale factor tracking

Add a `WM_DPICHANGED` case to the subclass proc that:

1. Updates the cached DPI used by `update_hit_regions`.
2. Calls `DwmExtendFrameIntoClientArea` again with the same margins —
   the DWM extended frame has to be re-applied after a DPI change.

Risk #5 in the original plan: HiDPI hit-region conversion. Centralise
the logical→physical conversion in one place (`update_hit_regions`)
and test it by moving the demo window between monitors with different
scaling.

### 4.8 M4 acceptance checklist

- [ ] No visible native title bar.
- [ ] Custom title bar height is exactly the value passed to `.height()`.
- [ ] Drop shadow visible (DWM extended frame).
- [ ] Rounded corners visible on Win11 (the 1-pixel top inset trick).
- [ ] Drag the title bar → window moves via the OS, so snap-to-edge
      and drag-to-top-to-maximize work.
- [ ] Double-click the title bar → toggles maximize (handled by
      `DefSubclassProc`'s default `HTCAPTION` handling).
- [ ] Resize from all 8 edges — the `WM_NCHITTEST` return values
      hand the resize over to the OS so the native cursor and drag
      loop kick in.
- [ ] Maximized window does NOT overflow the taskbar.
- [ ] DPI change (move the window between monitors at different
      scales) does not break the layout or the hit regions.
- [ ] Right-click the title bar → system menu (default
      `DefSubclassProc` handling of `HTCAPTION` right-click).
- [ ] Buttons are rendered by the widget but are NOT interactive yet
      — clicking them does nothing. That's M5.

---

## 5. M5 — Windows backend, phase 2

**Target**: custom button hit-testing + hover state + the Win11
snap-layout flyout.

### 5.1 Extended `WM_NCHITTEST`

Extend §4.5 to also return `HTMINBUTTON`, `HTMAXBUTTON`, `HTCLOSE`
when the cursor is over a published button rect:

```rust
// After the resize-border checks, before the drag-region fallback:
if let Some(rect) = &regions.minimize {
    if rect.contains_point(pt.x, pt.y) { return HTMINBUTTON; }
}
if let Some(rect) = &regions.maximize {
    if rect.contains_point(pt.x, pt.y) { return HTMAXBUTTON; }
}
if let Some(rect) = &regions.close {
    if rect.contains_point(pt.x, pt.y) { return HTCLOSE; }
}
```

**Returning `HTMAXBUTTON` is what triggers the Win11 snap-layout
flyout** — per Microsoft's "Apply snap layout menu" doc, hovering the
button that reports `HTMAXBUTTON` for at least 50 ms makes the
compositor show the snap layouts. This is winit issue #3884.

### 5.2 Click forwarding via `WM_NCLBUTTONUP`

Because the custom button rects now return non-`HTCLIENT` from
`WM_NCHITTEST`, the OS treats them as non-client area. That means
`WM_LBUTTONDOWN` / `WM_LBUTTONUP` never fire on them — the widget
never sees the click. Forward via the synthetic event pathway:

```rust
WM_NCLBUTTONUP => {
    let action = match wparam.0 as u32 {
        HTMINBUTTON  => Some(TitleBarAction::Minimize),
        HTMAXBUTTON  => Some(TitleBarAction::MaximizeToggle),
        HTCLOSE      => Some(TitleBarAction::Close),
        _ => None,
    };
    if let Some(action) = action {
        data.event_proxy.send_external(Box::new(
            TitleBarSyntheticEvent { window_id: data.fern_id, action }
        ));
        return 0; // consumed
    }
}
```

Add a matching handler in
[`WindowManager`](../crates/fern-app/src/window_manager.rs)'s
`AppEvent::External` processing: downcast to
`TitleBarSyntheticEvent`, look up the window's host, and call the
matching method directly (`host.minimize()` / `host.toggle_maximize()` /
host.close()`). This short-circuits the widget's on_tap chain because
the OS owns the pixels — the widget never gets the click even
logically.

The `AppEvent::External` plumbing already exists
([app.rs:769-801](../crates/fern-app/src/app.rs#L769-L801)); no new
winit user-event type is needed. This is also how §2.3 (the
Wayland `close()` fix) should be implemented — same mechanism.

### 5.3 Hover state forwarding

The OS doesn't send `WM_MOUSEMOVE` while the cursor is over non-
client area. It sends `WM_NCMOUSEMOVE`. Subclass handles:

```rust
WM_NCMOUSEMOVE => {
    let hit = /* re-run hit test */;
    if matches!(hit, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) {
        // Post a "hover this button" synthetic event to the widget.
        data.event_proxy.send_external(Box::new(TitleBarHover { button, entered: true }));
        // Track mouse leave so we can post `entered: false` later.
        let mut tme = TRACKMOUSEEVENT { cbSize: sizeof, dwFlags: TME_NONCLIENT | TME_LEAVE, ... };
        TrackMouseEvent(&mut tme);
    }
}
WM_NCMOUSELEAVE => {
    data.event_proxy.send_external(Box::new(TitleBarHover { button: last_hover, entered: false }));
}
```

The widget side: `ControlButton` needs to expose a way to receive
these events. Add a
`ControlButton::hover_signal() -> Signal<bool>` accessor and let the
`WindowControls` build() wire the synthetic-hover dispatch to flip
the signal. The `WindowManager`'s `AppEvent::External` handler
navigates to the right widget by `fern_id` + `button` discriminant.

### 5.4 Suppress legacy caption-button artwork

Once `WM_NCHITTEST` starts returning non-`HTCLIENT` codes for the
button rects, the OS will try to paint its own "legacy" caption-
button artwork over them unless we also:

- Return 0 from `WM_NCPAINT` — let DWM handle it entirely.
- Possibly consume `WM_NCACTIVATE` and return `TRUE` so the frame
  doesn't flicker on focus changes.

Verify visually on Win10 and Win11 — the behaviour differs between
the two.

### 5.5 M5 acceptance checklist

- [ ] Hover the maximize button → the Win11 snap-layout flyout
      appears within ~300 ms.
- [ ] Click each button → the corresponding action fires. No native
      caption-button art appears over our custom pixels.
- [ ] Hover state updates visually on each button (the reactive
      `ControlButton` hover signal flips).
- [ ] Clicking the flyout options (snap-left, snap-right, etc.)
      correctly snaps the window.
- [ ] Alt-Space still opens the system menu (the OS handles this —
      just verify it isn't broken).

---

## 6. M6 — Polish

Small quality-of-life items, doable in any order. Items 1–3 shipped in
M3; 4–8 remain.

1. ~~**OS-initiated maximize tracking**~~ — DONE in M3 via
   `notify_window_resized`.
2. ~~**Maximize / restore glyph swap**~~ — DONE in M3 (Switcher with
   static a11y names per child).
3. ~~**Close action default**~~ — DONE in M3 via
   `TitleBarHostCallbacks` + `AppEvent::External(CloseWindowRequest)`.
4. **Inactive window dim**: subscribe to `WindowEvent::Focused` and
   bind a `Signal<bool>` that `TitleBar` reads to dim its content
   when the window is unfocused. Not started.
5. **Theme change regression test**: the title bar already repaints
   correctly when `tree.set_theme(…)` flips light/dark (the V2
   reactive-theme flow dirty-marks every node), but there's no
   dedicated regression test pinning the behaviour. Add one.
6. **Architecture doc update**: add a "Custom window chrome" section
   to [`docs/fern-ui-architecture.md`](../fern-ui-architecture.md)
   pointing at [docs/title-bar.md](../title-bar.md), this plan, and
   the `PlatformTitleBarHost` trait.
7. **Milestones doc update**: add an entry to
   [`docs/fern-ui-milestones.md`](../fern-ui-milestones.md) marking
   the title bar work as M-TB (Title Bar) complete through M3.
8. **Drop the dead-code warning** on `list_view.rs:69` (unrelated but
   noisy during every demo build) — remove the unused `is_empty`
   method from `ListSource<T>`.
9. **Custom title-bar height reposition**: the deferred §3.2 work —
   `position_traffic_lights(x, y)` + a public hook so apps can pick a
   non-default title-bar height without the traffic lights drifting
   out of the band.

---

## 7. Risks and open questions

These are the sharp edges that have a non-trivial chance of biting
the implementer. Re-read before starting each milestone.

1. **`SetWindowSubclass` vs winit's own wndproc** (M4). Verify
   empirically by reading winit 0.30's
   `platform_impl/windows/window.rs` how winit installs its proc. If
   it uses the raw `SetWindowLongPtrW` path, we may need to also use
   `SetWindowLongPtrW` and chain through the stored previous proc.
2. **macOS traffic-light reposition timing** (M3.2, §6.9). Dormant —
   M3 shipped without the reposition helper because the default OS
   cluster geometry is correct for our default title-bar height. The
   moment a custom height is introduced this risk goes live: the
   reposition call must run *after* the content-view layout has
   settled, otherwise there's a one-frame visual snap during rapid
   resize. See `notify_window_resized` in
   [macos.rs](../../crates/fern-platform/src/title_bar_host/macos.rs)
   for the anchor point.
3. **HiDPI hit-region conversion** (M4.7). `WM_NCHITTEST` is in
   physical pixels; the widget works in logical. Centralise the
   conversion in exactly one place (`WindowsHost::update_hit_regions`).
4. **Wndproc re-entry** (M4.4). The subclass proc can be re-entered
   on the same thread during message processing. Never hold the
   `HitRegions` `Mutex` across a `CallMsgFilter` or a `SendMessageW`.
   Use `try_lock` and fall back to `HTCLIENT` if the lock is held.
5. **Win11 rounded corners + extended frame** (M4.3). The 1-pixel top
   inset in `DwmExtendFrameIntoClientArea` is what preserves the
   rounded corners. Setting 0 gives square corners. Document this
   inline in the code with a comment pointing at this plan entry.
6. **Wayland double-click maximize** — already solved in M2 (the
   double-tap recognizer state is preserved across the second
   `Down` event). But verify on non-Mutter compositors (KWin, Sway)
   since some of the fixes touched fern-core's gesture arena.
7. **AppEvent::External payload lifetime** (M5.2, §2.3). The
   `Box<dyn Any + Send>` payload is constructed on the wndproc thread
   and dispatched on the winit thread. Both threads are the same on
   Windows (winit marshals messages onto its own thread), but the
   type bounds still require `Send`. `TitleBarSyntheticEvent` is
   `POD + Copy`, so no lifetime issues.
8. **Existing framework regressions from M2 gesture fixes**. The
   removal of `recognizer.reset()` on `Down` in `GestureArena::process`
   changes the semantics of every gesture-driven widget. Run the
   full workspace test suite before and after each milestone — we
   got to 838 after M2; any milestone that touches `gesture.rs` or
   `event_dispatch_impl.rs` should not drop that number.

---

## 8. Reading list (prioritised)

1. [Microsoft: Custom Window Frame Using DWM](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe) — the authoritative M4 recipe.
2. [Microsoft: Apply snap layout menu](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-snap-layout-menu) — the M5 `HTMAXBUTTON` trick.
3. `winit/src/platform_impl/windows/window.rs` at the pinned version
   — how winit installs its own wndproc, to inform the subclassing
   decision in §4.3.
4. [Handmade Network: Custom window title bar](https://handmade.network/forums/articles/t/9073-custom_window_title_bar_and_almost_correctly_drawing_windows_10_borders)
   — the 1-pixel top inset trick (§4.3, risk #5) with pictures.
5. [`tauri-plugin-decorum`](https://crates.io/crates/tauri-plugin-decorum)
   — the closest Rust reference implementation. Read `windows.rs`,
   `macos.rs`, `linux.rs`.
6. `wry/examples/custom_titlebar.rs` — concise hit-test function for
   the Windows resize borders.
7. winit issues
   [#221](https://github.com/rust-windowing/winit/issues/221) (no
   `WM_NCHITTEST` hook) and
   [#3884](https://github.com/rust-windowing/winit/issues/3884)
   (returning `HTMAXBUTTON`) for context on what winit deliberately
   doesn't do for us.
8. The module doc on
   [`fern_core::window_chrome`](../crates/fern-core/src/window_chrome.rs)
   for the trait contract.
9. The M2 regression tests in
   [`title_bar.rs`](../crates/fern-widgets/src/title_bar.rs) and
   [`window_frame.rs`](../crates/fern-widgets/src/title_bar/window_frame.rs)
   — any M3+ framework change must not break these.
