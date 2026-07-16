<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ScrollArea

ScrollArea — a clipping viewport that scrolls its content on wheel, touch,
and assistive-technology actions.

Wrap any widget in `ScrollArea` to make it scrollable. The scroll position
is stored in reactive `Signal<f32>` signals (one per axis), shared with the
built-in `ScrollBar` children. Two display
modes cover most use cases: `Overlay` (the default, macOS-style thin-at-rest
indicator that expands on hover) and `Permanent` (a layout-consuming gutter
always on screen). Use `ScrollBarPolicy` to control when each axis shows.

## Accessibility

Reports `Role::ScrollView` with per-axis `scroll_y` / `scroll_x` position
and limit fields. Advertises `ScrollUp` / `ScrollDown` / `ScrollLeft` /
`ScrollRight` actions only for the axes that actually overflow, so AT clients
(NVDA, JAWS, VoiceOver) know which directions are reachable.

```rust
# use bastyde_widgets::scroll_area::{ScrollArea, ScrollBarMode};
# use bastyde_widgets::primitives::MinSize;
let _w = ScrollArea::new()
    .child(MinSize::new(0.0, 2000.0))
    .scroll_bar_style(ScrollBarMode::Permanent)
    .smooth_scrolling(true);
```

## Builder methods at a glance

`child`, `from_id`, `scroll_bar_style`, `scroll_bar_thumb_color`, `vertical_scroll_bar_policy`, `horizontal_scroll_bar_policy`, `line_height`, `scroll_bar_thickness`, `widget_resizable`, `smooth_scrolling`, `smooth_scroll_duration`, `preferred_size`, `preferred_height`, `overscroll_behavior`, `scroll_y_signal`, `scroll_x_signal`, `max_scroll_y_signal`, `max_scroll_x_signal`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/scroll_area/index.html)

## `pub enum ScrollBarMode`

How the scroll bar is presented relative to the viewport content.

```rust
pub enum ScrollBarMode { /* variants */ }
```

### Variants

- **`Overlay`** — Scroll bar overlays the content (macOS-style): a thin passive indicator is painted while scrolling; the full interactive track expands on pointer proximity. Does not reduce the viewport width.
- **`Permanent`** — Scroll bar is a permanent layout sibling of the viewport, reserving its full thickness and always remaining interactive — the classic Windows/Linux gutter style.
- **`Thin`** — Floats over the content like `Overlay` but only ever shows the thin resting indicator, never the full track. A passive scroll-position display for minimal UIs; drag, track-click, and keyboard still work against the full slot bounds.

## `pub enum ScrollBarPolicy`

Controls when the scroll bar appears for a given axis.

```rust
pub enum ScrollBarPolicy { /* variants */ }
```

### Variants

- **`AsNeeded`** — Show the scroll bar only when content exceeds the viewport size (default).
- **`AlwaysOn`** — Always show the scroll bar, even when content fits without scrolling.
- **`AlwaysOff`** — Never show the scroll bar; content is still scrollable via wheel and touch.

## `pub struct ScrollArea`

A clipping viewport that makes any child widget scrollable.

The scroll offset per axis is stored in a reactive `Signal<f32>`, shared
with the built-in `ScrollBar` children. See `ScrollBarMode` for display
options and `ScrollBarPolicy` for per-axis visibility control.

```rust
pub struct ScrollArea { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a new `ScrollArea` with overlay scroll bars, smooth scrolling, and no content yet.

#### `pub fn child(mut self, child: impl Widget + 'static) -> Self`

Set the scrollable content widget.

#### `pub fn from_id(child: WidgetId) -> Self`

Construct from an already-registered child WidgetId.

#### `pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self`

Set the scroll bar display mode (`Overlay`, `Permanent`, or `Thin`).

#### `pub fn scroll_bar_thumb_color(mut self, color: impl Into<ColorProp>) -> Self`

Tint the built-in scroll bars' thumb with an explicit colour instead of
the theme's `scrollbar_thumb*` tokens. Accepts anything
`impl Into<ColorProp>` — a `Color`, a theme role, or a `Signal` —
resolved against the live theme at paint, so roles/signals stay
reactive. Forwarded to both scroll bars via
`ScrollBar::thumb_color`.
Use when the area sits on a surface the surface-relative tokens don't
suit — e.g. a tooltip's inverse chip (`TextRole::TooltipText`).

#### `pub fn vertical_scroll_bar_policy(mut self, policy: ScrollBarPolicy) -> Self`

Set the vertical scroll bar visibility policy.

#### `pub fn horizontal_scroll_bar_policy(mut self, policy: ScrollBarPolicy) -> Self`

Set the horizontal scroll bar visibility policy.

#### `pub fn line_height(mut self, lh: f32) -> Self`

Set the pixels-per-line used when translating line-based wheel events.

#### `pub fn scroll_bar_thickness(mut self, thickness: f32) -> Self`

Set the scroll bar thickness in logical pixels (applies to both axes).

#### `pub fn widget_resizable(mut self, resizable: bool) -> Self`

When true, content smaller than the viewport is stretched to fill it.
Similar to Qt's `QScrollArea::setWidgetResizable(true)`.

#### `pub fn smooth_scrolling(mut self, enabled: bool) -> Self`

Enable or disable smooth animated scrolling for wheel events.
Enabled by default. Applies to both line-based (`ScrollDelta::Lines`)
and pixel-based (`ScrollDelta::Pixels`) wheel events — on Wayland and
other platforms with high-resolution scroll axes, mouse wheel notches
are delivered as pixel deltas, so animating both paths is required for
a fast flick to feel smooth instead of jumping.

#### `pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self`

Set the duration of the smooth scroll animation (default: 150ms).

#### `pub fn preferred_size(mut self, width: f32, height: f32) -> Self`

Set a preferred size returned when the parent proposes unconstrained
dimensions. If not set, falls back to cached content size or 300×200.

This overrides **both** axes. If you only want to cap the height and let
the width follow the content — the usual case for a menu or popover, which
must be as wide as its widest row — use `preferred_height` instead.
Passing a width of `0.0` here does *not* mean "no preference": it means
zero, and the scroll area will collapse.


#### `pub fn preferred_height(mut self, height: f32) -> Self`

Cap the height when the parent proposes an unconstrained one, while
letting the **width** continue to follow the content.

This is what a scrolling menu/popover wants: it must not grow taller than
its viewport, but it must still be as wide as its widest item. Using
`preferred_size` with a `0.0` width for this
collapses the panel to its minimum width and clips every row — the parent
proposes an unconstrained width (it is hugging its content), so the `0.0`
is taken literally.

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Set the scroll-chaining behavior at the boundary. Default
`OverscrollBehavior::Chain` (a boundary scroll bubbles to an ancestor
scrollable); `OverscrollBehavior::Contain` absorbs it instead.

#### `pub fn scroll_y_signal(&self) -> &Signal<f32>`

Get the vertical scroll position signal (for external observation).

#### `pub fn scroll_x_signal(&self) -> &Signal<f32>`

Get the horizontal scroll position signal (for external observation).

#### `pub fn max_scroll_y_signal(&self) -> &Signal<f32>`

Maximum vertical scroll offset for the current content
(`content_height − viewport_height`, or 0 when content fits).
External callers bind to this for "is there more to scroll?"
chrome (e.g. trailing scroll-arrow visibility).

#### `pub fn max_scroll_x_signal(&self) -> &Signal<f32>`

Maximum horizontal scroll offset for the current content.
External callers bind to this for "is there more to scroll?"
chrome (e.g. trailing scroll-arrow visibility on a tab bar).
