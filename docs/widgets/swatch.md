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
# use bastyde_widgets::color_picker::ColorSwatch;
# use bastyde_tokens::Color;
let _swatch = ColorSwatch::new(Color::new(0.21, 0.52, 0.89, 1.0))
    .size(24.0)
    .corner_radius(4.0);
```

## Builder methods at a glance

`selected`, `label`, `size`, `corner_radius`, `enabled`, `on_activate_fn`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/color_picker/index.html)

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

#### `pub fn new(color: impl Into<bastyde_core::signal::Prop<Color>>) -> Self`

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

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Set the initial enabled state. Forwarded to the arena at build time.

#### `pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Register an activation callback invoked on tap, Enter, Space, or
the `Action::Click` accessibility action.
