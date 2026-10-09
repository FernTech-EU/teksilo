<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# IconWidget

![IconWidget preview](img/icon_widget.png)

IconWidget — a vector or raster icon rendered at a configurable size.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`IconMode`](#iconmode) — Whether an icon is rendered as a theme-tinted mask or in its original colors |
| `struct` | [`IconWidget`](#iconwidget) — A leaf widget that renders an icon from a path, SVG string, PNG, or WebP source |

## Public functions

### `IconWidget`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`from_path(path: Path, size: f32)`](#iconwidget-from_path) |
| `Self` | [`checkmark(size: f32)`](#iconwidget-checkmark) |
| `Self` | [`dash(size: f32)`](#iconwidget-dash) |
| `Self` | [`radio_dot(size: f32)`](#iconwidget-radio_dot) |
| `Self` | [`chevron_down(size: f32)`](#iconwidget-chevron_down) |
| `Self` | [`chevron_right(size: f32)`](#iconwidget-chevron_right) |
| `Self` | [`chevron_left(size: f32)`](#iconwidget-chevron_left) |
| `Self` | [`chevron_up(size: f32)`](#iconwidget-chevron_up) |
| `Self` | [`from_svg(svg_str: &str)`](#iconwidget-from_svg) |
| `Self` | [`from_svg_icon(icon: &SvgIcon)`](#iconwidget-from_svg_icon) |
| `Self` | [`from_png(data: &'static [u8], size: f32)`](#iconwidget-from_png) |
| `Self` | [`from_webp(data: &'static [u8], size: f32)`](#iconwidget-from_webp) |
| `Self` | [`from_raster(icon: &RasterIcon, size: f32)`](#iconwidget-from_raster) |
| `Self` | [`from_animated(icon: &AnimatedIcon, size: f32)`](#iconwidget-from_animated) |
| | **Builder methods** |
| `Self` | [`mode(mode: IconMode)`](#iconwidget-mode) |
| `Self` | [`color(color: impl Into<ColorProp>)`](#iconwidget-color) |
| `Self` | [`icon_size(size: f32)`](#iconwidget-icon_size) |
| `Self` | [`follow_text_scale(follow: bool)`](#iconwidget-follow_text_scale) |

## Detailed description

Supports multiple source formats: programmatic `Path` (checkmarks,
chevrons, dots), SVG strings, PNG, static WebP, and animated WebP. Icons
default to **tintable** mode — the pixels are treated as an alpha mask
and multiplied by the widget's color property (defaults to
`TextRole::Primary`) so they follow theme switches automatically.
`IconMode::FullColor` preserves original pixel colors and is appropriate
for emoji-style graphics or brand logos.

For arbitrary-aspect-ratio photos or artwork see
`ImageWidget`.

#### Accessibility

Icons are decorative by default — they set no accessibility role and
announce nothing. The parent widget (e.g. `Button`, `IconButton`) is
responsible for the accessible label.

```rust
# use teksilo_widgets::primitives::icon_widget::{IconWidget, IconMode};
# use teksilo_tokens::TextRole;
let _check = IconWidget::checkmark(20.0);

let _chevron = IconWidget::chevron_down(16.0)
    .color(TextRole::Primary)
    .follow_text_scale(false);
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![IconWidget at Touch density](img/icon_widget-touch.png)

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/icon_widget/index.html)

<a id="iconmode"></a>

## `pub enum IconMode`

Whether an icon is rendered as a theme-tinted mask or in its original colors.

Applies to every source an `IconWidget` can hold — raster *and* SVG. For an
SVG the two modes select between the two representations the parser builds
(see `teksilo_canvas::svg`): `Tintable` draws the merged
silhouette in the widget's color, `FullColor` walks the
document-ordered ops and honours each shape's own fill / stroke / gradient.

The default is `Tintable`, which is what a UI glyph wants —
it follows the theme into dark mode. Reach for
`FullColor` for artwork whose colors *are* the content: a
brand mark, a flag, a colored file-type badge. A `currentColor` shape inside
full-color artwork still takes the widget's color, so the two are mixable.

```rust
pub enum IconMode { /* variants */ }
```

### Variants

- **`Tintable`** — Treat as an alpha mask: tint the whole icon with the widget's color.
- **`FullColor`** — Render the icon's own colors; the widget color supplies `currentColor` and its alpha attenuates the result.

<a id="iconwidget"></a>

## `pub struct IconWidget`

A leaf widget that renders an icon from a path, SVG string, PNG, or WebP source.

```rust
pub struct IconWidget { /* fields */ }
```

### Methods

<a id="iconwidget-from_path"></a>

#### `pub fn from_path(path: Path, size: f32) -> Self`

Create an icon from a custom path. The path should be defined
in coordinates matching the given size (e.g., 0..24 for size=24).

<a id="iconwidget-checkmark"></a>

#### `pub fn checkmark(size: f32) -> Self`

A checkmark icon (✓) at the given size.

<a id="iconwidget-dash"></a>

#### `pub fn dash(size: f32) -> Self`

A short horizontal dash at the given size — used as the
indeterminate-state glyph for tristate menu items (mirrors
the Windows "mixed-state" convention).

<a id="iconwidget-radio_dot"></a>

#### `pub fn radio_dot(size: f32) -> Self`

A small filled disc centered in the given size — used as the
selected-state glyph for radio menu items.

<a id="iconwidget-chevron_down"></a>

#### `pub fn chevron_down(size: f32) -> Self`

A downward-pointing chevron (▼) at the given size.

<a id="iconwidget-chevron_right"></a>

#### `pub fn chevron_right(size: f32) -> Self`

A right-pointing chevron (▶) at the given size.

<a id="iconwidget-chevron_left"></a>

#### `pub fn chevron_left(size: f32) -> Self`

A left-pointing chevron (◀) at the given size.

<a id="iconwidget-chevron_up"></a>

#### `pub fn chevron_up(size: f32) -> Self`

An upward-pointing chevron (▲) at the given size.

<a id="iconwidget-from_svg"></a>

#### `pub fn from_svg(svg_str: &str) -> Self`

Create an icon from an SVG string. Parses the SVG and extracts
geometry, ignoring any colors in the SVG. Display size defaults
to the SVG's viewBox dimensions; use `icon_size`
to override.

If parsing fails, logs the error in debug mode and produces an empty icon.

<a id="iconwidget-from_svg_icon"></a>

#### `pub fn from_svg_icon(icon: &SvgIcon) -> Self`

Create an icon from a pre-parsed `SvgIcon`. Display size
defaults to the SVG's viewBox; use `icon_size`
to override. Scaling is deferred to paint time.

<a id="iconwidget-from_png"></a>

#### `pub fn from_png(data: &'static [u8], size: f32) -> Self`

Create an icon from PNG data.

If decoding fails, logs the error in debug mode and produces an empty icon.

<a id="iconwidget-from_webp"></a>

#### `pub fn from_webp(data: &'static [u8], size: f32) -> Self`

Create an icon from WebP data. Auto-detects static vs animated.

If decoding fails, logs the error in debug mode and produces an empty icon.

<a id="iconwidget-from_raster"></a>

#### `pub fn from_raster(icon: &RasterIcon, size: f32) -> Self`

Create an icon from a pre-decoded `RasterIcon`.
Accepts a reference — full-color mode shares the icon's pixels, and
tintable mode keeps an alpha mask computed from them.

The texture is named after the icon's identity, so every widget
showing this icon (or a clone of it) in one mode shares one texture.
Like every static image texture it lives as long as the window: an
icon decoded afresh for each widget gets a texture of its own each
time.

<a id="iconwidget-from_animated"></a>

#### `pub fn from_animated(icon: &AnimatedIcon, size: f32) -> Self`

Create an icon from a pre-decoded `AnimatedIcon`.
Accepts a reference — frame data is copied internally.

Named after its first frame's identity, with the same sharing and
lifetime as `from_raster`.

<a id="iconwidget-mode"></a>

#### `pub fn mode(mut self, mode: IconMode) -> Self`

Set the icon rendering mode (tintable or full-color).
Re-computes cached pixel data for raster/animated icons.

<a id="iconwidget-color"></a>

#### `pub fn color(mut self, color: impl Into<ColorProp>) -> Self`

Set the tint. Accepts any `impl Into<ColorProp>`:

- A raw `Color` — a frozen literal.
- A `TextRole` / `SurfaceRole` / `BorderRole` — resolved against
  the theme at paint time (reactive across theme switches).
- A `Signal<Color>` — reactive state (usually interaction-driven).

<a id="iconwidget-icon_size"></a>

#### `pub fn icon_size(mut self, size: f32) -> Self`

Set the display size of the icon. The path/image is scaled to fit
this size during rendering. This does not affect the design-time
coordinate space — SVG paths scale correctly.

<a id="iconwidget-follow_text_scale"></a>

#### `pub fn follow_text_scale(mut self, follow: bool) -> Self`

Make this icon grow with the global accessibility text scale
(`ctx.text_scale`). Off by default. Enable for icons that sit inline
with text and should scale together — e.g. status glyphs in a
`SeverityBadge`. The reported (and rendered) size becomes
`display_size × text_scale`.
