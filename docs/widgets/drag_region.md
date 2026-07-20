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

```ignore
// Used internally by TitleBar; the snippet shows the construction pattern.
let region = DragRegion::with_child(host.clone(), TextWidget::new(lit!("My App")));
```

## Builder methods at a glance

`with_child`, `with_child_id`, `close_action`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/title_bar/index.html)

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

#### `pub fn close_action( mut self, action: Option<Rc<dyn Fn(&mut bastyde_core::widget::EventContext)>>, ) -> Self`

Forward the title bar's close-action override, so the fallback window
menu's Close entry matches the close button. No effect on platforms
that provide their own window menu.
