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

## Builder methods at a glance

`horizontal`, `vertical`, `corner`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/title_bar/index.html)

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
