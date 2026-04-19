# TitleBar Reference

FernUI replaces the native OS title bar with a widget-level one when an application opts into custom chrome. The title bar is a single cross-platform widget ([`TitleBar`](../crates/fern-widgets/src/title_bar.rs)); the window-manipulation plumbing (drag, zoom, close, inset measurements) lives behind a per-OS trait ([`PlatformTitleBarHost`](../crates/fern-core/src/window_chrome.rs)). The widget never touches `NSWindow`, `HWND`, or `xdg_toplevel` directly.

| Layer | Type | Crate | What it does |
|-------|------|-------|--------------|
| **Widget** | [`TitleBar`](../crates/fern-widgets/src/title_bar.rs) | `fern-widgets` | Lays out the bar, dispatches gestures, renders controls |
| **Host trait** | [`PlatformTitleBarHost`](../crates/fern-core/src/window_chrome.rs) | `fern-core` | Seam between the widget and the OS |
| **Backends** | `WaylandHost` / `MacOsHost` / `WindowsHost` / `X11Host` | `fern-platform` | Concrete per-OS implementations |
| **Resize frame** | [`WindowFrame`](../crates/fern-widgets/src/title_bar/window_frame.rs) | `fern-widgets` | Optional invisible edge-resize overlay for borderless windows |

The backend is constructed by `WindowManager` when the app opts into custom chrome, and handed to the widget tree. You retrieve it from the root-builder closure and pass it to `TitleBar::new`.

---

## Quick start

```rust
use fern_ui::prelude::*;
use fern_ui::widgets::{Expand, RectWidget, TextWidget, TitleBar, VStack, ZStack};

fn main() {
    FernAppBuilder::new()
        .theme(Theme::dark_default())
        .window_title("My App")
        .window_size(900, 600)
        .custom_chrome(true)                       // opt in
        .root(|tree| {
            let theme = tree.theme().clone();

            let title_bar: Box<dyn Widget> = match tree.title_bar_host() {
                Some(host) => Box::new(
                    TitleBar::new(host)
                        .height(40.0)
                        .background(theme.colors.surface_pressed)
                        .leading(TextWidget::new_literal("  My App"))
                        .center(TextWidget::new_literal("drag · double-click to maximize")),
                ),
                // X11 (and some stubs) don't support custom chrome.
                None => Box::new(TextWidget::new_literal(
                    "(custom chrome unsupported — native decorations)",
                )),
            };

            let body = Expand::new().fills_stack().child(
                ZStack::new()
                    .child(RectWidget::new().background(theme.colors.surface_main))
                    .child(TextWidget::new_literal("body content")),
            );

            tree.add(VStack::new().spacing(0.0).child(title_bar).child(body))
        })
        .run();
}
```

Two entry points matter:

1. [`FernAppBuilder::custom_chrome(true)`](../crates/fern-app/src/app.rs) — opts the initial window into custom chrome. When using `initial_window(WindowConfig)`, set `WindowConfig::custom_chrome(true)` directly instead.
2. [`WidgetTree::title_bar_host()`](../crates/fern-core/src/widget_tree.rs) — returns `Option<Rc<dyn PlatformTitleBarHost>>`. `None` means *either* the app didn't opt in *or* the platform has no backend (X11). Always handle both arms; a fallback view keeps the app usable on the unsupported path.

Working demo: [`examples/title_bar_demo/src/main.rs`](../examples/title_bar_demo/src/main.rs) — `cargo run -p title-bar-demo`.

---

## Layout model

`TitleBar` is a horizontal band with five slots:

```
┌──────────┬───────────┬──────────────────────┬────────────┬──────────────┐
│ leading  │ leading   │   drag region        │  trailing  │   window     │
│ inset    │ slot      │   (center / flex)    │  slot      │   controls   │
└──────────┴───────────┴──────────────────────┴────────────┴──────────────┘
  host-reserved                                                host-rendered
```

- **Leading inset** — `host.reserved_leading_inset()`. Blank. Reserved so the OS can draw over it; on macOS this is where the traffic lights land. Windows/Wayland return `Size::ZERO`.
- **Leading slot** — `.leading(widget)`. App icon, menu bar, title text.
- **Drag region** — the center slot is wrapped in a flex `DragRegion`: unconsumed presses call `host.begin_drag()`, double-clicks call `host.toggle_maximize()`, right-clicks call `host.show_window_menu()`.
- **Trailing slot** — `.trailing(widget)`. Search field, action buttons.
- **Window controls** — min/max/close cluster. Rendered only when `host.renders_custom_controls()` is `true` (Windows + Wayland; never on macOS — the OS traffic lights already cover this).

```rust
TitleBar::new(host)
    .height(40.0)                                 // default: 40 logical px
    .background(theme.colors.surface_pressed)     // default: transparent
    .border(theme.colors.text_secondary, 2.0)     // 1px+ bottom rule
    .leading(leading_widget)                      // or .leading_id(id)
    .center(center_widget)                        // or .center_id(id)
    .trailing(trailing_widget)                    // or .trailing_id(id)
    .close_action(|ctx| ctx.close_window())       // optional override
```

Full builder surface in [title_bar.rs](../crates/fern-widgets/src/title_bar.rs).

---

## Per-platform behavior

The widget is identical everywhere; the host decides what renders where. `TitleBar::build` reads `host.reserved_leading_inset()` + `host.renders_custom_controls()` on every rebuild.

| Capability | Wayland | macOS | Windows | X11 |
|---|---|---|---|---|
| `custom_chrome` supported | yes | yes | yes (stub → M5) | no — `title_bar_host()` returns `None` |
| `reserved_leading_inset()` | `ZERO` | ~78×22 (traffic-light cluster) | `ZERO` | — |
| `renders_custom_controls()` | `true` | **`false`** | `true` | — |
| `needs_custom_resize_handles()` | `true` | **`false`** | `true` | — |
| `begin_drag()` | winit `drag_window` | winit `drag_window` | winit `drag_window` | — |
| `begin_resize(edge)` | winit `drag_resize_window` | `Unsupported` (NSWindow handles edges) | winit `drag_resize_window` | — |
| `show_window_menu(at)` | xdg-shell `show_window_menu` | no-op (`Ok(())`) | synthetic `WM_SYSMENU` (M5) | — |
| `close()` | routes via event proxy | routes via event proxy | routes via event proxy | — |
| `toggle_maximize()` | winit `set_maximized` | `-[NSWindow performZoom:]` | winit `set_maximized` | — |

On macOS, because `renders_custom_controls()` is `false`, `WindowControls` never enters the tree — the OS's native traffic lights are what you see. Applications still receive the `is_maximized` signal for their own iconography if they need it.

---

## `PlatformTitleBarHost` trait

Full signature in [fern-core/src/window_chrome.rs](../crates/fern-core/src/window_chrome.rs). The trait is intentionally `!Send + !Sync` (it owns platform-handle `Rc`s) and is passed around as `Rc<dyn PlatformTitleBarHost>`.

| Method | Purpose |
|---|---|
| `reserved_leading_inset() -> Size` | Blank leading-edge area the OS draws over |
| `reserved_trailing_inset() -> Size` | Reserved trailing-edge area (always `ZERO` today) |
| `renders_custom_controls() -> bool` | Whether the widget should draw min/max/close |
| `needs_custom_resize_handles() -> bool` | Whether the app should install a `WindowFrame` overlay |
| `begin_drag() -> Result<(), PlatformError>` | Start interactive window move |
| `begin_resize(ResizeEdge) -> Result<(), PlatformError>` | Start interactive resize from an edge/corner |
| `show_window_menu(Point) -> Result<(), PlatformError>` | Wayland system menu; no-op elsewhere |
| `minimize()` / `toggle_maximize()` / `close()` | Button actions |
| `is_maximized() -> bool` | Synchronous OS state |
| `is_maximized_signal() -> Signal<bool>` | Reactive view, driven by the host |
| `notify_window_resized()` | Called by `WindowManager` on `WindowEvent::Resized`; default impl refreshes the signal |
| `update_hit_regions(&HitRegions)` | Windows `WM_NCHITTEST` input (no-op elsewhere) |

`PlatformError::Unsupported` vs `PlatformError::Os(String)` — `Unsupported` means the platform has no way to do it (`begin_resize` on macOS); `Os(String)` means the OS call failed at runtime. The string is for logs, not programmatic matching.

---

## The close action

Closing a window from a widget-tree callback is awkward because winit 0.30 has no synchronous `Window::request_close`. The host routes close through [`TitleBarHostCallbacks::request_close`](../crates/fern-core/src/window_chrome.rs), which boxes a `CloseWindowRequest` onto the event loop; `WindowManager` dequeues it and closes the window on the next tick.

Two paths reach that closure:

1. **Default** — the close button calls `host.close()`, which runs the callback.
2. **Override** — `TitleBar::close_action(|ctx| …)` intercepts the button. Useful when the app wants to confirm unsaved work first, or to send an `Intent` for a root-level `Action`:

```rust
TitleBar::new(host).close_action(|ctx| ctx.close_window())
// or:
TitleBar::new(host).close_action(|ctx| ctx.send_intent(AppIntent::RequestQuit))
```

Even with an override, `host.close()` remains available for programmatic close from anywhere you hold an `Rc<dyn PlatformTitleBarHost>`.

---

## Reactive maximize

`TitleBar::is_maximized_signal()` returns a read-only `Signal<bool>` sourced from the host. The flow:

```
user clicks maximize ─► host.toggle_maximize() ─► OS zooms
                                                    │
                                                    ▼
                      WindowEvent::Resized ─► host.notify_window_resized()
                                                    │
                                                    ▼
                                        signal.set(host.is_maximized())
                                                    │
                                                    ▼
                                    WindowControls Switcher swaps glyph:
                                         □ (U+25A1) ↔ ❐ (U+2750)
```

OS-initiated maximizes — macOS green-light zoom, Wayland `xdg_toplevel.state` changes, Windows `WM_SIZE`/`SC_MAXIMIZE` — all flow through the same `WindowEvent::Resized` path, so the signal is always consistent with the OS. Applications can subscribe to swap their own iconography but **must not** write to the signal; doing so speculatively de-syncs it from the OS.

> **macOS caveat.** `NSWindow.isZoomed` tracks traffic-light zoom only. Native fullscreen (green light + Option, or `-[NSWindow toggleFullScreen:]`) puts the window on its own Space and leaves `isZoomed` false. The title bar isn't visible during fullscreen anyway, so we don't track that state.

---

## `WindowFrame` — edge resize for borderless windows

On Wayland and (eventually) Windows, a borderless window has no OS-drawn frame, so nothing catches clicks at the 1-pixel edge for a resize. [`WindowFrame`](../crates/fern-widgets/src/title_bar/window_frame.rs) solves that with an invisible overlay of resize strips along the four edges and four corners.

```rust
match tree.title_bar_host() {
    Some(host) if host.needs_custom_resize_handles() =>
        tree.add(WindowFrame::new(host).thickness(6.0).content_id(inner)),
    _ => inner,                            // macOS, X11, or no host
}
```

Gate on `needs_custom_resize_handles()`; on macOS `NSWindow` still services edge resize even with `titlebarAppearsTransparent + fullSizeContentView`, and installing the overlay would fight the OS.

Content fills the whole window — the strips sit *on top*. Hit-testing walks children in reverse insertion order so strips win clicks within `thickness` pixels of an edge; interior clicks fall through to the content. Default thickness 6 logical pixels, matching the common Windows 11 / GNOME convention.

Builder: `.new(host)` → `.thickness(f32)` → `.content(widget)` / `.content_boxed(Box<dyn Widget>)` / `.content_id(WidgetId)`.

---

## Windows backend preview — `HitRegions`

The Windows backend (M4/M5) needs a physical-pixel map of hit-test regions ahead of time because `WM_NCHITTEST` runs outside the normal event flow. `TitleBar::paint` publishes this every frame via `host.update_hit_regions(&HitRegions { … })`:

```rust
pub struct HitRegions {
    pub minimize: Option<Rect>,
    pub maximize: Option<Rect>,
    pub close: Option<Rect>,
    pub drag: Vec<Rect>,                    // multiple → non-rectangular drag
    pub resize_borders: ResizeBorders,      // per-edge widths in px
}
```

Wayland and macOS backends ignore the payload (default impl is a no-op). The `Vec<Rect>` for drag lets apps split the drag band around a centered search field or title pill without losing draggability.

---

## File reference

Widget layer:
- [crates/fern-widgets/src/title_bar.rs](../crates/fern-widgets/src/title_bar.rs) — `TitleBar` builder + layout
- [crates/fern-widgets/src/title_bar/controls.rs](../crates/fern-widgets/src/title_bar/controls.rs) — `WindowControls`, `ControlButton`
- [crates/fern-widgets/src/title_bar/drag_region.rs](../crates/fern-widgets/src/title_bar/drag_region.rs) — `DragRegion`
- [crates/fern-widgets/src/title_bar/window_frame.rs](../crates/fern-widgets/src/title_bar/window_frame.rs) — `WindowFrame`, `ResizeStrip`

Core trait:
- [crates/fern-core/src/window_chrome.rs](../crates/fern-core/src/window_chrome.rs)

Backends:
- [crates/fern-platform/src/title_bar_host.rs](../crates/fern-platform/src/title_bar_host.rs) — factory
- [crates/fern-platform/src/title_bar_host/macos.rs](../crates/fern-platform/src/title_bar_host/macos.rs)
- [crates/fern-platform/src/title_bar_host/wayland.rs](../crates/fern-platform/src/title_bar_host/wayland.rs)
- [crates/fern-platform/src/title_bar_host/windows.rs](../crates/fern-platform/src/title_bar_host/windows.rs)
- [crates/fern-platform/src/title_bar_host/x11.rs](../crates/fern-platform/src/title_bar_host/x11.rs)

App integration:
- [crates/fern-app/src/app.rs](../crates/fern-app/src/app.rs) — `FernAppBuilder::custom_chrome`, `CloseWindowRequest`
- [crates/fern-app/src/window_manager.rs](../crates/fern-app/src/window_manager.rs) — host construction + `WindowEvent::Resized` hook
- [crates/fern-core/src/widget_tree.rs](../crates/fern-core/src/widget_tree.rs) — `WidgetTree::title_bar_host`

Demo:
- [examples/title_bar_demo/src/main.rs](../examples/title_bar_demo/src/main.rs)
