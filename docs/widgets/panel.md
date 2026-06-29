<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Panel

Panel — a themed single-child container that provides a background, border,
corner radius, and padding.

The equivalent of Qt's `QFrame`: a visual wrapper whose chrome comes from
the active `PanelStyle` trait
implementation. The IntUI default (`RecipePanelStyle`) honours four
`PanelVariant` presets (Plain /
Sunken / Raised / Highlighted) while still accepting per-call overrides
for background, border colour/width, corner radius, and padding. Apps
requiring a custom surface (frosted glass, brutalist frame) supply their
own `impl PanelStyle` per-call (`.style(...)`) or theme-wide via
`theme.style_slots.panel`.

## Accessibility

Emits `Role::Group` by default. Call `.a11y_presentational()` to suppress
the group node when the panel is purely decorative (e.g. a toolbar
background that should not introduce a spurious container in the AT tree).

```rust
# use bastyde_widgets::Panel;
# use bastyde_widgets::primitives::TextWidget;
# use bastyde_i18n::lit;
let _w = Panel::new()
    .padding(12.0)
    .child(TextWidget::new(lit!("Content")));
```

## Builder methods at a glance

`variant`, `style`, `a11y_presentational`, `child_id`, `child`, `background`, `border_color`, `border_width`, `corner_radius`, `padding`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/panel/index.html)

## `pub struct Panel`

A themed container with background, border, corner radius, and padding.

```rust
pub struct Panel { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Construct a panel with default theme values (Plain variant, no manual overrides).

#### `pub fn variant(mut self, variant: PanelVariant) -> Self`

Pick the design-language variant. Default `Plain`. The active
`PanelStyle` decides what each variant means visually (the
IntUI default maps Plain → `surface_main`, Sunken →
`surface_sunken`, Raised → `surface_raised`, Highlighted →
`accent_subtle_bg`, with matching border defaults).

#### `pub fn style(mut self, style: impl bastyde_core::styles::PanelStyle) -> Self`

Per-call style override. Replaces the theme-wide default
`PanelStyle` for just this Panel instance — same role as
`Button::style(...)`. Manual overrides (`background`,
`border_color`, etc.) are still passed to the style via
`PanelStyleConfig`; custom styles are free to honour or ignore
them.

#### `pub fn a11y_presentational(mut self) -> Self`

Mark the panel as presentational for assistive tech: the panel's
own a11y node is hidden so its wrapping chrome (background,
border, padding) doesn't introduce a spurious `Group` node
between an outer widget (Toolbar, StatusBar, etc.) and the
real content. Children remain visible in the a11y tree.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set child by pre-registered ID.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set an inline child widget (deferred insertion).

#### `pub fn background(mut self, color: impl Into<ColorProp>) -> Self`

Override the background. Accepts `Color`, a `SurfaceRole`,
or a `Signal<Color>`. Default (unset) is `SurfaceRole::Main`.

#### `pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self`

Override the border color. Accepts `Color`, a `BorderRole`,
or a `Signal<Color>`. Default (unset) is `BorderRole::Default`.

#### `pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self`

Override the border width (default: 0 — no border).
Accepts a static `f32` or a reactive `Signal<f32>`.

#### `pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self`

Override the corner radius (default: theme `radius_popup`).
Accepts a static `f32` or a reactive `Signal<f32>`.

#### `pub fn padding(mut self, padding: impl Into<Prop<f32>>) -> Self`

Override the padding (default: theme `components.panel.padding`).
Accepts a static `f32` or a reactive `Signal<f32>`.
