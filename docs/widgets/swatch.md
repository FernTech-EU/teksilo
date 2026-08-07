<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ColorSwatch

`ColorSwatch` — single clickable color cell with `Role::ColorWell`.

Public widget so apps can compose their own swatch rows or palettes
outside of the bundled `SwatchGrid`. Renders an optional checkerboard
underlay when `color.a() < 1.0` so transparent swatches read correctly.
The displayed color is a `Prop<Color>` — pass a static `Color` for a
fixed palette entry or a `Signal<Color>` for a live preview that
re-paints whenever the bound value changes (used by `ColorPicker`'s
current-color preview and `ColorEdit`'s trigger swatch).

## Accessibility

Declares `Role::ColorWell`; `set_color_value` carries the RGBA value
and `set_value` carries the formatted hex string so braille and
voice output both have a human-readable form. Selected swatches
append a localized "selected" suffix to their announced name.

```rust
# use teksilo_widgets::color_picker::ColorSwatch;
# use teksilo_tokens::Color;
let _swatch = ColorSwatch::new(Color::new(0.21, 0.52, 0.89, 1.0))
    .size(24.0)
    .corner_radius(4.0);
```

## Builder methods at a glance

`selected`, `label`, `size`, `corner_radius`, `enabled`, `on_activate_fn`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/color_picker/index.html)

## `pub struct ColorSwatch`

Single-cell color swatch.

The displayed color is a `Prop<Color>` — pass a `Color` for a
static palette entry (the common case in `SwatchGrid`) or a
`Signal<Color>` for a live preview that re-paints when the bound
value changes (used by `ColorPicker`'s current-color preview and
`ColorEdit`'s trigger).

```rust
pub struct ColorSwatch { /* fields */ }
```

### Methods

#### `pub fn new(color: impl Into<teksilo_core::signal::Prop<Color>>) -> Self`

Create a swatch displaying `color`. Accepts a static `Color` or a
`Signal<Color>` (via `impl Into<Prop<Color>>`); a reactive value
re-paints the cell whenever the signal changes.

#### `pub fn selected(mut self, selected: bool) -> Self`

Mark the swatch as currently selected, which paints an accent
border and appends a localized "selected" suffix to the AT name.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Override the accessible label. Default is a localized "Color: #RRGGBB"
string derived from the displayed color's hex value.

#### `pub fn size(mut self, size: f32) -> Self`

Set the swatch cell size in logical pixels (square). Defaults to
the theme's `recipe_color_picker_style::SWATCH_SIZE`.

#### `pub fn corner_radius(mut self, r: f32) -> Self`

Set the corner radius of the swatch cell in logical pixels.
Defaults to `recipe_color_picker_style::SWATCH_CORNER_RADIUS`.

#### `pub fn enabled(mut self, enabled: impl Into<teksilo_core::signal::Prop<bool>>) -> Self`

Set the enabled state, statically or reactively. Forwarded to the
arena at build time.

#### `pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Register an activation callback invoked on tap, Enter, Space, or
the `Action::Click` accessibility action.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after a hover delay.

Mutually exclusive with `Self::rich_tooltip`, `Self::rich_tooltip_content`,
and `Self::composite_tooltip` — this call clears the other slots.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip looked up from the tooltip registry by key.

Mutually exclusive with `Self::tooltip`, `Self::rich_tooltip_content`,
and `Self::composite_tooltip` — this call clears the other slots.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip with inline content (no registry lookup required).

Mutually exclusive with `Self::tooltip`, `Self::rich_tooltip`,
and `Self::composite_tooltip` — this call clears the other slots.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.

Mutually exclusive with `Self::tooltip`, `Self::rich_tooltip`,
and `Self::rich_tooltip_content` — this call clears the other slots.
