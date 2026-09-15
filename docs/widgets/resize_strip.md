<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ResizeStrip

A thin invisible widget that forwards a window resize gesture to the
platform host when the user presses the primary button inside it. Used
to build a 6-px resize frame around a borderless window on Wayland.

This is the frame complement to [`crate::title_bar::DragRegion`]: drag
moves the window, resize strips drag the window edges. On platforms
that don't expose `Window::drag_resize_window` (notably winit's macOS
backend), `PlatformTitleBarHost::begin_resize` returns
`PlatformError::Unsupported` and the strip becomes a silent no-op —
macOS handles edge resize via its own native chrome.

## Reaching a 6 dp edge with a finger

The strip's thickness is fixed at every density — it is an overlay drawn
*over* the window's own edge, so widening it would eat into content rather
than into empty space, and the frame would start swallowing presses meant
for the app.

The grab is `Widget::hit_outset` instead: for a direct pointer the strip
is offered the press against bounds inflated inward to the density's target
size (24 dp Compact, 44 dp Touch), and the arena's outset pre-pass runs
before the ordinary reverse-sibling walk, so the widened band beats the
content underneath. An edge strip inflates only across its thickness; a
corner cell inflates on all four sides, since both of its axes are the
diagonal grab. For a precise pointer the outset is zero and a mouse press
resolves exactly where it always did.

Two strips whose bands overlap are settled by distance to their own
uninflated rectangles, not by sibling order, so the midpoint between two
adjacent edges belongs to the nearer one.

## One resize per gesture

`PlatformTitleBarHost::begin_resize` hands the window to the compositor
for the rest of the gesture, so it must be asked once. The press that asks
is gated on the pointer being the **primary** one: a second finger landing
on the frame while a resize is already running would ask for a second
interactive resize of the same window, which on Wayland means a second
`xdg_toplevel::resize` against a live one. A mouse is always primary, so the
gate never fires for one.

## Builder methods at a glance

`horizontal`, `vertical`, `corner`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/title_bar/resize_strip/index.html)

## `pub struct ResizeStrip`

A single edge of a resize frame. Construct one per side and lay them
out around your content (HStack of left + content + right inside a
VStack of top + middle + bottom is the conventional shape — see the
title bar demo for an example).

```rust
pub struct ResizeStrip { /* fields */ }
```

### Methods

#### `pub fn horizontal( host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, thickness: f32, ) -> Self`

Build a horizontal (top / bottom) strip of the given height. The
width is unconstrained — the strip claims whatever its parent
container offers, so it can stretch across the full window width.

#### `pub fn vertical(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, thickness: f32) -> Self`

Build a vertical (left / right) strip of the given width. The
height is unconstrained.

#### `pub fn corner(host: Rc<dyn PlatformTitleBarHost>, edge: ResizeEdge, size: f32) -> Self`

Build a square corner cell of the given size. The corner handles a
diagonal resize gesture (e.g. `TopLeft` does NW/SE resize). Should
be placed *on top of* the edge strips at the four corners so the
framework's hit-test routes the click to the corner rather than
the adjacent edge.
