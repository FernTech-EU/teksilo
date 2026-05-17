# TitleBar Reference

Bastyde replaces the native OS title bar with a widget-level one when an application opts into custom chrome. The title bar is a single cross-platform widget ([`TitleBar`](../crates/bastyde-widgets/src/title_bar.rs)); the window-manipulation plumbing (drag, zoom, close, inset measurements) lives behind a per-OS trait ([`PlatformTitleBarHost`](../crates/bastyde-core/src/window_chrome.rs)). The widget never touches `NSWindow`, `HWND`, or `xdg_toplevel` directly.

| Layer | Type | Crate | What it does |
|-------|------|-------|--------------|
| **Widget** | [`TitleBar`](../crates/bastyde-widgets/src/title_bar.rs) | `bastyde-widgets` | Lays out the bar, dispatches gestures, renders controls |
| **Host trait** | [`PlatformTitleBarHost`](../crates/bastyde-core/src/window_chrome.rs) | `bastyde-core` | Seam between the widget and the OS |
| **Backends** | `WaylandHost` / `MacOsHost` / `WindowsHost` / `X11Host` | `bastyde-platform` | Concrete per-OS implementations |
| **Resize frame** | [`WindowFrame`](../crates/bastyde-widgets/src/title_bar/window_frame.rs) | `bastyde-widgets` | Optional invisible edge-resize overlay for borderless windows |

The backend is constructed by `WindowManager` when the app opts into custom chrome, and handed to the widget tree. You retrieve it from the root-builder closure and pass it to `TitleBar::new`.

---

## Quick start

```rust
use bastyde::prelude::*;
use bastyde::widgets::{Expand, RectWidget, TextWidget, TitleBar, VStack, ZStack};

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::dark())
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

1. [`BastydeAppBuilder::custom_chrome(true)`](../crates/bastyde-app/src/app.rs) — opts the initial window into custom chrome. When using `initial_window(WindowConfig)`, set `WindowConfig::custom_chrome(true)` directly instead.
2. [`WidgetTree::title_bar_host()`](../crates/bastyde-core/src/widget_tree.rs) — returns `Option<Rc<dyn PlatformTitleBarHost>>`. `None` means *either* the app didn't opt in *or* the platform has no backend (X11). Always handle both arms; a fallback view keeps the app usable on the unsupported path.

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

Full builder surface in [title_bar.rs](../crates/bastyde-widgets/src/title_bar.rs).

---

## Per-platform behavior

The widget is identical everywhere; the host decides what renders where. `TitleBar::build` reads `host.reserved_leading_inset()` + `host.renders_custom_controls()` on every rebuild.

| Capability | Wayland | macOS | Windows | X11 |
|---|---|---|---|---|
| `custom_chrome` supported | yes | yes | yes | no — `title_bar_host()` returns `None` |
| `reserved_leading_inset()` | `ZERO` | ~78×22 (traffic-light cluster) | `ZERO` | — |
| `renders_custom_controls()` | `true` | **`false`** | `true` | — |
| `needs_custom_resize_handles()` | `true` | **`false`** | **`false`** (OS handles via `WM_NCHITTEST`) | — |
| `begin_drag()` | winit `drag_window` | winit `drag_window` | winit `drag_window` | — |
| `begin_resize(edge)` | winit `drag_resize_window` | `Unsupported` (NSWindow handles edges) | winit `drag_resize_window` | — |
| `show_window_menu(at)` | xdg-shell `show_window_menu` | no-op (`Ok(())`) | `SendMessage(WM_SYSCOMMAND, SC_KEYMENU)` | — |
| `update_hit_regions(&HitRegions)` | no-op | no-op | snapshot stored for `WM_NCHITTEST` (logical→physical converted via `GetDpiForWindow`) | — |
| Snap-layout flyout (Win11) | n/a | n/a | yes — proc returns `HTMAXBUTTON` for the maximize-button rect | — |

On macOS, because `renders_custom_controls()` is `false`, `WindowControls` never enters the tree — the OS's native traffic lights are what you see.

Window minimize / maximize / close are **not** trait methods. They flow through [`WindowState::placement`](../crates/bastyde-core/src/window/state.rs) (a `Signal<WindowPlacement>`) and `WindowState::close` — `WindowControls` mutates these and the app-level [`apply_window_command`](../crates/bastyde-app/src/window_manager.rs) translates each `WindowCommand` into the matching winit call. OS-initiated changes flow back via `set_placement_from_os` (re-entrancy guarded).

---

## `PlatformTitleBarHost` trait

Full signature in [bastyde-core/src/window_chrome.rs](../crates/bastyde-core/src/window_chrome.rs). The trait is intentionally `!Send + !Sync` (it owns platform-handle `Rc`s) and is passed around as `Rc<dyn PlatformTitleBarHost>`.

| Method | Purpose |
|---|---|
| `reserved_leading_inset() -> Size` | Blank leading-edge area the OS draws over (macOS traffic lights); `ZERO` elsewhere |
| `reserved_trailing_inset() -> Size` | Reserved trailing-edge area (always `ZERO` today) |
| `renders_custom_controls() -> bool` | Whether the widget should draw min/max/close |
| `needs_custom_resize_handles() -> bool` | Whether the app should install a `WindowFrame` overlay |
| `begin_drag() -> Result<(), PlatformError>` | Start interactive window move |
| `begin_resize(ResizeEdge) -> Result<(), PlatformError>` | Start interactive resize from an edge/corner |
| `show_window_menu(Point) -> Result<(), PlatformError>` | Show the system window menu |
| `update_hit_regions(&HitRegions)` | Per-frame snapshot of drag + button rects; only the Windows backend uses it (`WM_NCHITTEST`) |
| `title_bar_widget_id(ControlTarget) -> Option<WidgetId>` | Resolve a button target to its widget id; Windows uses this to route synthetic taps from `WM_NCLBUTTONUP` |
| `set_button_hover(ControlTarget, bool)` | Inject non-client hover from `WM_NCMOUSEMOVE` (Windows) |
| `register_hover_signal(ControlTarget, Signal<bool>)` | `WindowControls` registers the per-button hover signal at build time so the host can drive it |

Window-state mutations (minimize, maximize, close) are **not** trait methods. The widget tree mutates `WindowState::placement` / `WindowState::close` directly; the app-level `apply_window_command` translates them into the matching winit calls. This means a custom `close_action` override on `TitleBar` is honoured on every backend including Windows — the `ControlButton`'s `on_tap` runs the override regardless of how the click arrived (widget tree or synthetic tap from the wndproc).

`PlatformError::Unsupported` vs `PlatformError::Os(String)` — `Unsupported` means the platform has no way to do it (`begin_resize` on macOS); `Os(String)` means the OS call failed at runtime. The string is for logs, not programmatic matching.

### `Widget::after_paint` aggregation

`TitleBar` overrides [`Widget::after_paint`](../crates/bastyde-core/src/widget.rs) (gated on `wants_after_paint() == true`) to publish a single complete `HitRegions` snapshot per frame. The hook receives a read-only [`WidgetTreeView`](../crates/bastyde-core/src/widget.rs) so the parent can read the resolved bounds of memoised descendants — the drag region and the three `ControlButton`s registered by `WindowControls` via a shared layout sink. Wayland and macOS hosts ignore the published payload (their `update_hit_regions` is a no-op); the Windows host converts logical→physical via `GetDpiForWindow(hwnd)` and stores the snapshot under a `Mutex` for `WM_NCHITTEST` to consume.

This is also why per-button publishing from `ControlButton::paint` would be wrong: `update_hit_regions` is replace-semantics, so concurrent publishes by sibling controls would each clobber the previous payload. Aggregation in the parent is the only correct approach.

---

## The close action

The close button on `WindowControls` has two paths:

1. **Default** — the button's `on_tap` calls [`EventContext::close_window`](../crates/bastyde-core/src/widget.rs), which queues a `WindowCommand::Close` on the window's `WindowState`. The app drains the queue on the next event-loop tick (winit 0.30 has no synchronous `Window::request_close`, so we hop through the command queue).
2. **Override** — `TitleBar::close_action(|ctx| …)` replaces the default `on_tap` entirely. Useful when the app wants to confirm unsaved work first, or to send an `Intent` for a root-level `Action`:

```rust
TitleBar::new(host).close_action(|ctx| ctx.close_window())
// or:
TitleBar::new(host).close_action(|ctx| ctx.send_intent(AppIntent::RequestQuit))
```

The override fires on **every** backend including Windows: when the OS reports `WM_NCLBUTTONUP` over the close-button rect, bastyde-platform posts a `TitleBarSyntheticEvent` through `AppEvent::External`, the dispatcher resolves the button's `WidgetId` via `host.title_bar_widget_id(Close)`, and `WidgetTree::synthesise_tap` runs the same `on_tap` handler the override installed.

---

## Reactive maximize

`TitleBar` derives the maximize signal from the hosting window's [`WindowState::placement`](../crates/bastyde-core/src/window/state.rs):

```rust
let is_maximized_signal = ctx
    .window()
    .map(|w| w.placement().map(|p| p.is_maximized()))
    .unwrap_or_else(|| Signal::new(false));
```

End-to-end flow:

```
user clicks maximize ─► ControlButton on_tap fires:
                       w.placement().set(Maximized | Floating)
                                                    │
                                                    ▼
                  observer enqueues WindowCommand::SetPlacement(...)
                                                    │
                                                    ▼
              WindowManager::drain_window_commands → apply_window_command
                                                    │
                                                    ▼
                                    winit `set_maximized(true|false)`
                                                    │
                                                    ▼
                       OS zooms, fires WindowEvent::Resized
                                                    │
                                                    ▼
              BastydeAppHandler::window_event → set_placement_from_os(...)
                                  (re-entrancy guarded — observers don't echo)
                                                    │
                                                    ▼
                       placement signal flips → Switcher swaps glyph
                                  (currently both children render □ — see below)
```

OS-initiated maximizes (macOS green-light zoom, Windows drag-to-top snap, Wayland `xdg_toplevel.state` changes) all flow through the same `WindowEvent::Resized` arm, so the placement signal is always consistent with the OS. Applications can subscribe to drive their own iconography from the same signal.

> **macOS caveat.** `NSWindow.isZoomed` tracks traffic-light zoom only. Native fullscreen (green light + Option, or `-[NSWindow toggleFullScreen:]`) puts the window on its own Space and leaves `isZoomed` false. The title bar isn't visible during fullscreen anyway, so we don't track that state.

> **Glyph fallback.** Both Switcher children currently use `□` (U+25A1, Geometric Shapes). The semantically nicer "two stacked squares" glyphs (`❐` U+2750 Dingbats, `⧉` U+29C9 Math Symbols, `🗗` U+1F5D7 Symbols and Pictographs) and even neighbouring Geometric Shapes glyphs like `▭` U+25AD all render as missing on Windows because text-typeset's font fallback chain only reliably hits `□` from Segoe UI's basic geometric coverage (same root cause as the close button using U+00D7 instead of U+2715). State is still distinguished by the OS window itself, the action toggling correctly via `WindowState::placement`, and the reactive a11y name (`Maximize` / `Restore` via `tr_widget!`). A future pass can swap to custom rect-primitive icons to restore the visual delta.

---

## `WindowFrame` — edge resize for borderless windows

On Wayland and (eventually) Windows, a borderless window has no OS-drawn frame, so nothing catches clicks at the 1-pixel edge for a resize. [`WindowFrame`](../crates/bastyde-widgets/src/title_bar/window_frame.rs) solves that with an invisible overlay of resize strips along the four edges and four corners.

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

## Windows backend

The Windows host extends the DWM-drawn frame into the client area with a 1-pixel top inset (the magic value that preserves Win11's rounded corners — `0` gives square corners), then installs a `SetWindowSubclass` proc on the HWND to intercept the non-client messages that would otherwise hand control back to the OS frame. winit's own wndproc was registered at class-registration time via raw `SetWindowLongPtrW` and runs first; the comctl32 subclass chain fires after and falls through to `DefSubclassProc` for messages we don't intercept. AccessKit's `WM_GETOBJECT` subclass is a separate slot and they coexist.

**Messages the proc handles:**

- `WM_NCCALCSIZE` — zero non-client insets so the client area covers the full window. When `IsZoomed` is true, restore the system `SM_CXFRAME + SM_CXPADDEDBORDER` insets and clamp to the monitor work area so the maximized window doesn't cover the taskbar.
- `WM_NCHITTEST` — return `HTLEFT` / `HTTOP` / corner codes for the outer N pixels (so the OS handles the resize loop natively, with the right cursor and snap behavior), `HTCAPTION` for the widget's drag region, and `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE` for the control-button rects. Returning `HTMAXBUTTON` is what makes Win11 show the snap-layout flyout on hover.
- `WM_NCLBUTTONDOWN` over a button hit code — return 0 to prevent `DefSubclassProc` from entering its built-in press-tracking modal loop, which would otherwise consume the matching `WM_NCLBUTTONUP` itself (user-visible symptom: the button appears to need a double-click).
- `WM_NCLBUTTONUP` over a button hit code — post a [`TitleBarSyntheticEvent`](../crates/bastyde-core/src/window_chrome.rs) through `AppEventProxy::send_external_boxed`. The bastyde-app dispatcher resolves the matching `WidgetId` via `host.title_bar_widget_id(target)` and calls `WidgetTree::synthesise_tap` to run the button's `on_tap` handler. `close_action` overrides fire here.
- `WM_NCMOUSEMOVE` / `WM_NCMOUSELEAVE` — post `TitleBarHoverEvent` for the same reason. The host writes the matching `Signal<bool>` (registered by `WindowControls` via `host.register_hover_signal(...)` at build time); an effect inside `ControlButton` maps the bool to its visual `bg_signal`, so OS-driven hover renders identically to widget-tree hover.
- `WM_DPICHANGED` — re-call `DwmExtendFrameIntoClientArea` so rounded corners survive a DPI change (winit handles the resize but doesn't re-extend). Falls through to `DefSubclassProc` for the rest.
- `WM_NCPAINT` / `WM_NCACTIVATE` — return early (`0` and `TRUE` respectively) so DWM doesn't paint legacy caption-button artwork over our pixels and the frame doesn't flicker on focus changes.

**Hit-region snapshot.** `TitleBar::after_paint` publishes a single complete `HitRegions` per frame. Wayland and macOS backends ignore it; the Windows host converts the logical-pixel rects to physical pixels via `GetDpiForWindow(hwnd)` and stores under a `Mutex<HitRegions>` shared with the proc. The proc reads via `try_lock` — if it's contended (re-entry via `SendMessage`), it falls through to `HTCLIENT` rather than blocking the message pump.

```rust
pub struct HitRegions {
    pub minimize: Option<Rect>,
    pub maximize: Option<Rect>,
    pub close: Option<Rect>,
    pub minimize_id: Option<WidgetId>,
    pub maximize_id: Option<WidgetId>,
    pub close_id: Option<WidgetId>,
    pub drag: Vec<Rect>,                    // multiple → non-rectangular drag
    pub resize_borders: ResizeBorders,      // per-edge widths
}
```

The `Vec<Rect>` for drag lets apps split the drag band around a centered search field or title pill without losing draggability. The `*_id` companions are the routing target for synthetic-tap forwarding. `maximize_id` specifically points to the **Switcher** wrapping the two glyph buttons (not to either glyph child): the inactive Switcher child is dormant and reports `Rect::ZERO`, but the Switcher container itself is always laid out by the parent HStack, so its bounds are stable across the floating ↔ maximized swap. `WidgetTree::synthesise_tap` dispatches the click at the Switcher's bounds-center, and the normal hit-test routing then delivers it to whichever child is currently visible.

---

## File reference

Widget layer:
- [crates/bastyde-widgets/src/title_bar.rs](../crates/bastyde-widgets/src/title_bar.rs) — `TitleBar` builder + layout
- [crates/bastyde-widgets/src/title_bar/controls.rs](../crates/bastyde-widgets/src/title_bar/controls.rs) — `WindowControls`, `ControlButton`
- [crates/bastyde-widgets/src/title_bar/drag_region.rs](../crates/bastyde-widgets/src/title_bar/drag_region.rs) — `DragRegion`
- [crates/bastyde-widgets/src/title_bar/window_frame.rs](../crates/bastyde-widgets/src/title_bar/window_frame.rs) — `WindowFrame`, `ResizeStrip`

Core trait:
- [crates/bastyde-core/src/window_chrome.rs](../crates/bastyde-core/src/window_chrome.rs)

Backends:
- [crates/bastyde-platform/src/title_bar_host.rs](../crates/bastyde-platform/src/title_bar_host.rs) — factory
- [crates/bastyde-platform/src/title_bar_host/macos.rs](../crates/bastyde-platform/src/title_bar_host/macos.rs)
- [crates/bastyde-platform/src/title_bar_host/wayland.rs](../crates/bastyde-platform/src/title_bar_host/wayland.rs)
- [crates/bastyde-platform/src/title_bar_host/windows.rs](../crates/bastyde-platform/src/title_bar_host/windows.rs)
- [crates/bastyde-platform/src/title_bar_host/x11.rs](../crates/bastyde-platform/src/title_bar_host/x11.rs)

App integration:
- [crates/bastyde-app/src/app.rs](../crates/bastyde-app/src/app.rs) — `BastydeAppBuilder::custom_chrome`, `CloseWindowRequest`
- [crates/bastyde-app/src/window_manager.rs](../crates/bastyde-app/src/window_manager.rs) — host construction + `WindowEvent::Resized` hook
- [crates/bastyde-core/src/widget_tree.rs](../crates/bastyde-core/src/widget_tree.rs) — `WidgetTree::title_bar_host`

Demo:
- [examples/title_bar_demo/src/main.rs](../examples/title_bar_demo/src/main.rs)
