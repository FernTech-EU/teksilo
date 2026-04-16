# Icons and Resources

## Overview

FernUI supports three icon formats — SVG, PNG, and WebP — embedded at compile time via the `res!()` macro. Icons are tintable by default: their color follows the theme and interaction state (hover, pressed, disabled) automatically.

## Supported Formats

| Format | Use case | Tintable | Notes |
|--------|----------|----------|-------|
| SVG | Vector icons (preferred) | Yes | Scales to any size without loss |
| PNG | Raster icons | Yes | Fixed resolution, best at native size |
| WebP | Raster icons (smaller files) | Yes | Use **lossless** encoding for icons |
| Other | Arbitrary files | N/A | Embedded as raw `&'static [u8]` |

## Usage

### Embedding resources with `res!()`

Place resource files under `resources/` in your crate root:

```
my-app/
  Cargo.toml
  src/
    main.rs
  resources/
    icons/
      save.svg
      star.png
      clock.webp
```

Embed and use them:

```rust
// SVG — returns &'static SvgIcon, compile-time validated
let save = fern_ui::res!("resources/icons/save.svg");

// PNG — returns &'static RasterIcon, compile-time validated
let star = fern_ui::res!("resources/icons/star.png");

// WebP — returns &'static RasterIcon (static) or &'static AnimatedIcon (animated)
let clock = fern_ui::res!("resources/icons/clock.webp");

// Unknown extensions — returns &'static [u8], existence checked only
let font = fern_ui::res!("resources/fonts/custom.ttf");
```

The macro validates known formats at compile time (XML structure for SVG, magic bytes for PNG/WebP). Unknown extensions are embedded as raw bytes without validation — only file existence is checked.

### Using icons in buttons

```rust
let save = fern_ui::res!("resources/icons/save.svg");

// Leading icon — most common
Button::new_literal("Save")
    .icon(IconWidget::from_svg_icon(save), IconLocation::Leading)
    .style(ButtonVariant::Default)

// Icon only — toolbars
Button::new_literal("Save")
    .icon(IconWidget::from_svg_icon(save), IconLocation::IconOnly)
    .style(ButtonVariant::Flat)

// Raster icon
let star = fern_ui::res!("resources/icons/star.png");
Button::new_literal("Favorite")
    .icon(IconWidget::from_raster(star, 24.0), IconLocation::Leading)
```

The button controls the icon's display size via `theme.components.button.icon_size` (default 16dp). The icon's color is bound to the button's text color signal — it follows hover, pressed, disabled, and theme changes automatically.

### Icon locations

| `IconLocation` | Layout |
|----------------|--------|
| `None` | No icon (default) |
| `Leading` | Icon left of label |
| `Trailing` | Icon right of label |
| `IconOnly` | Icon only, no label |
| `Top` | Icon above label |
| `Bottom` | Icon below label |

### Standalone icons (outside buttons)

```rust
// SVG — size defaults to viewBox, override with icon_size()
IconWidget::from_svg_icon(icon).icon_size(32.0).color(Color::RED)

// Programmatic — built-in shapes
IconWidget::checkmark(24.0)
IconWidget::chevron_down(16.0)
IconWidget::chevron_right(16.0)

// From raw SVG string (no res! macro, parses at runtime)
IconWidget::from_svg(include_str!("../resources/icons/save.svg"))
```

### Tintable vs full-color mode

Icons default to **tintable** mode: the image is treated as an alpha mask and tinted with the widget's color property. This enables theme-aware coloring.

For icons that should keep their original colors (e.g., app logos, colored emoji):

```rust
IconWidget::from_raster(logo, 32.0).mode(IconMode::FullColor)
```

In full-color mode, the icon's RGB is rendered directly; the widget color only controls opacity.

## Creating Icon Assets

### SVG icons

Use any SVG editor. Icons should be single-color paths on a transparent background. Fill and stroke colors in the SVG are ignored — the rendering color comes from the theme.

Standard viewBox: `0 0 24 24` (Material Design convention).

### PNG icons

Export as white shape on transparent background (RGBA). The luminance of the image becomes the alpha mask for tinting.

- Use 24x24 or 48x48 pixels for standard icons
- Export as RGBA PNG (not indexed/palette)

### WebP icons

**Use lossless encoding.** Lossy WebP with separate alpha planes (VP8X + ALPH chunks) may not decode correctly. Lossless WebP (VP8L) stores RGBA natively and works reliably.

With ImageMagick:

```bash
convert -size 24x24 xc:none -fill none -stroke white -strokewidth 2 \
  -draw "circle 12,12 12,3" \
  -define webp:lossless=true \
  icon.webp
```

With cwebp:

```bash
cwebp -lossless input.png -o icon.webp
```

WebP is ~40-60% smaller than PNG for the same quality, making it a good choice for apps with many icons.

### Animated WebP

Animated WebP icons (loading spinners, status indicators) are supported. The `res!()` macro auto-detects animation and returns `&'static AnimatedIcon`. Use `IconWidget::from_animated()` to render.

Frame cycling is automatic and loops continuously. Each frame should use lossless encoding.
