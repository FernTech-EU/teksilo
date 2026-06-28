<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# RectWidget

RectWidget — a leaf widget that paints a filled and/or stroked rounded rectangle.

`RectWidget` has no intrinsic content: it fills whatever space its parent
proposes (or reports `0×0` when unconstrained) and draws a solid or reactive
background color, an optional border, and an optional corner radius. It is
the low-level building block for card backgrounds, focus rings, dividers, and
highlight overlays.

All visual properties accept `impl Into<ColorProp>` (a raw `Color`, a theme
role such as `SurfaceRole::Hover`, or a `Signal<Color>`) so reactive
interaction-driven colors require no extra wiring.

```rust
# use bastyde_tokens::{Color, CornerRadius};
# use bastyde_widgets::primitives::RectWidget;
// A pill-shaped accent badge background:
let _w = RectWidget::new()
    .background(Color::from_rgba(0.2, 0.5, 1.0, 1.0))
    .corner_radius(CornerRadius::uniform(12.0));
```

## Builder methods at a glance

`background`, `border_color`, `border_width`, `corner_radius`, `bind_background`, `bind_border_color`, `bind_border_width`, `bind_corner_radius`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/rect_widget/index.html)

## `pub struct RectWidget`

A leaf widget that paints a filled and/or stroked rounded rectangle.

See the `module documentation` for the full feature description.
All visual properties accept `impl Into<ColorProp>` (colors/roles/signals) or
`impl Into<Prop<f32>>` / `impl Into<Prop<CornerRadius>>` (static or reactive)
— so the common "fill with theme surface, border with theme border" setup is
just `.background(SurfaceRole::Main).border_color(BorderRole::Default)`.

```rust
pub struct RectWidget { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a fully transparent, zero-border rectangle with no corner radius.

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Fill color. Accepts `Color`, a theme role (`SurfaceRole`, etc.),
or a `Signal<Color>`.

#### `pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self`

Border color. Accepts `Color`, a theme role (`BorderRole`, etc.),
or a `Signal<Color>`.

#### `pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self`

Stroke width, in logical pixels. Accepts a static value or a reactive `Signal<f32>`.

#### `pub fn corner_radius(mut self, radius: impl Into<Prop<CornerRadius>>) -> Self`

Corner radius for the fill and stroke. Accepts a `CornerRadius` (per-corner
control) or a reactive `Signal<CornerRadius>`.

#### `pub fn bind_background(self, state: impl Into<ColorProp>) -> Self`

Compatibility shim — `.bind_background(signal)` → `.background(signal)`.

#### `pub fn bind_border_color(self, state: impl Into<ColorProp>) -> Self`

Compatibility shim — `.bind_border_color(signal)` → `.border_color(signal)`.

#### `pub fn bind_border_width(self, state: impl Into<Prop<f32>>) -> Self`

Compatibility shim — `.bind_border_width(signal)` → `.border_width(signal)`.

#### `pub fn bind_corner_radius(self, state: impl Into<Prop<CornerRadius>>) -> Self`

Compatibility shim — `.bind_corner_radius(signal)` → `.corner_radius(signal)`.
