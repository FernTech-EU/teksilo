<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DragRegion

`DragRegion` — flexible drag region inside a `TitleBar`.

Captures pointer events that are not consumed by inner content and
forwards them to the platform host: drag gestures begin a window move,
double taps toggle maximize, and right-clicks open the system window
menu (Wayland only). On Windows the drag rect is published into
`HitRegions::drag` so the wndproc subclass returns `HTCAPTION` for
the same area — but the actual publish happens from
`crate::title_bar::TitleBar::after_paint`, which aggregates this
drag region and the three control buttons into one snapshot per
frame. This widget no longer publishes from `paint()`.

The region grows via `flex = 1.0` to claim all remaining horizontal
space in the parent `HStack`, so it naturally sits between any leading
widgets (app icon, document title) and the trailing `WindowControls`
cluster. An optional child widget — typically a centered title — is
placed at the full region bounds and passes pointer events upward to
the drag handler when it does not consume them.

# A finger on the title bar

The drag region is deliberately **not** a press-time actor: the window move
starts from `DragPhase::Started`, after the recognizer has decided the
press is a drag, which is why a quick press still reaches the double-tap
recognizer and why a contact that turns out to be a scroll is never stolen.
Nothing here changes that.

Three routes serve a contact:

* **Double tap → maximise / restore.** The same handler a double click
  drives; the multi-tap recognizer already tunes its slop per pointer kind,
  so a finger's looser aim is accounted for without a second code path.
* **Long press → the window menu.** A mouse reaches it with the secondary
  button, which a finger does not have. Where the platform owns the menu
  (Wayland's `xdg_toplevel.show_window_menu`) the long press asks the host
  for it, and that menu's **Move** entry is a finger's only route to a
  window move — see below. Where it does not (X11), this widget's
  `.context_menu(..)` factory is the menu, and it is reached through the
  framework's own long-press context-menu route rather than through a second
  copy of the opening machinery here.
* **Drag → nothing, on purpose.** `BackendCaps::touch_window_drag` is
  `false` on every platform Teksilo supports, and it is not a to-do:
  `xdg_toplevel::move` needs a serial from an input event on a toplevel the
  compositor agrees the client owns, and winit 0.30's `drag_window` harvests
  a *pointer* serial internally, so a finger cannot reach it however the app
  asks. Calling it anyway would be a silent no-op — the compositor drops a
  request whose serial does not match — so the drag handler does not call
  it for a direct pointer, and the long-press menu is the documented
  alternative. This is also the WCAG 2.5.7 single-pointer alternative to the
  dragging operation. A headless test cannot detect the real failure (a fake
  `PlatformTitleBarHost` will happily record a `begin_drag` that a
  compositor would have ignored), so this is a hardware-checklist line.

```ignore
// Used internally by TitleBar; the snippet shows the construction pattern.
let region = DragRegion::with_child(host.clone(), TextWidget::new(lit!("My App")));
```

## Builder methods at a glance

`with_child`, `with_child_id`, `close_action`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/title_bar/index.html)

## `pub struct DragRegion`

Flexible, hit-transparent region inside a title bar that routes pointer events to the
platform host for window dragging, maximize-toggle, and the system window menu.

```rust
pub struct DragRegion { /* fields */ }
```

### Methods

#### `pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self`

Create a drag region with no inner content — the entire region is a pure drag handle.

#### `pub fn with_child(host: Rc<dyn PlatformTitleBarHost>, child: Box<dyn Widget>) -> Self`

Create a drag region wrapping an arbitrary boxed child widget (typically a centered
title). Pointer events not consumed by the child bubble up to the drag handler.

#### `pub fn with_child_id(host: Rc<dyn PlatformTitleBarHost>, id: WidgetId) -> Self`

Create a drag region with an already-registered child identified by `id`.
Use this when the child widget was added to the tree before constructing the
region (e.g. when you need the child's `WidgetId` for another reference).

#### `pub fn close_action( mut self, action: Option<Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>, ) -> Self`

Forward the title bar's close-action override, so the fallback window
menu's Close entry matches the close button. No effect on platforms
that provide their own window menu.
