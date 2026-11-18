<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Icons and Resources

## Overview

Teksilo supports three icon formats, SVG, PNG, and WebP, embedded at compile time via the `res!()` macro. Icons are tintable by default: their color follows the theme and interaction state (hover, pressed, disabled) automatically.

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
// SVG, returns &'static SvgIcon, compile-time validated
let save = teksilo::res!("resources/icons/save.svg");

// PNG, returns &'static RasterIcon, compile-time validated
let star = teksilo::res!("resources/icons/star.png");

// WebP, returns &'static RasterIcon (static) or &'static AnimatedIcon (animated)
let clock = teksilo::res!("resources/icons/clock.webp");

// Unknown extensions, returns &'static [u8], existence checked only
let font = teksilo::res!("resources/fonts/custom.ttf");
```

The macro validates known formats at compile time: SVG is parsed with the real `SvgIcon` parser and rejected if it contains no drawable geometry (an `<image>`/embedded-bitmap-only SVG fails, since the icon renderer skips `<image>` and `<text>`); PNG and WebP are checked by magic bytes (PNG's 8-byte signature + IHDR chunk; WebP's RIFF/WEBP signature). Unknown extensions are embedded as raw bytes without validation, only file existence is checked.

### Using icons in buttons

```rust
let save = teksilo::res!("resources/icons/save.svg");

// Leading icon, most common
Button::new(lit!("Save"))
    .icon(IconWidget::from_svg_icon(save), IconLocation::Leading)
    .variant(ButtonVariant::Plain)

// Icon only, toolbars
Button::new(lit!("Save"))
    .icon(IconWidget::from_svg_icon(save), IconLocation::IconOnly)
    .variant(ButtonVariant::Ghost)

// Raster icon
let star = teksilo::res!("resources/icons/star.png");
Button::new(lit!("Favorite"))
    .icon(IconWidget::from_raster(star, 24.0), IconLocation::Leading)
```

The button controls the icon's display size via the `BUTTON_ICON_SIZE` constant (default 16dp) in `teksilo-widgets`. The icon's color is bound to the button's text color signal, it follows hover, pressed, disabled, and theme changes automatically.

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
// SVG, size defaults to viewBox, override with icon_size()
IconWidget::from_svg_icon(icon).icon_size(32.0).color(Color::RED)

// Programmatic, built-in shapes
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

## Icons from the desktop's theme

On Linux and BSD desktops an application can show the desktop's own icons (Breeze, Adwaita, Papirus, whatever the user picked) by their freedesktop name, as GTK and Qt applications do. Names follow the freedesktop Icon Naming Specification: `document-send`, `go-down`, `view-refresh`, `face-smile`.

```rust
let send = teksilo::res!("resources/icons/send.svg");

Button::new(lit!("Send"))
    .icon(
        IconWidget::themed("document-send-symbolic")
            .fallback(IconWidget::from_svg_icon(send)),
        IconLocation::Leading,
    )
```

- **Symbolic or full color.** A name ending in `-symbolic` asks for the theme's single-color glyph, which is tinted with the widget's color like an embedded icon. Without the suffix, or when the theme has no symbolic variant, the full-color icon is drawn in its own colors. The file found decides, not `.mode(...)`.
- **The fallback** is drawn when the theme has no such icon, and on macOS and Windows, which have no icon themes. Without a fallback nothing is drawn, and the icon keeps its size in the layout.
- **Size.** A themed icon is 16 dp until `icon_size(...)` says otherwise (a `Button` sets its own icon size). The size and the window's scale pick the best file the theme has for it.
- **Lookup.** The theme is the desktop's (`kdeglobals [Icons] Theme` on KDE Plasma, the portal's `org.gnome.desktop.interface icon-theme` elsewhere), searched as the XDG Icon Theme Specification describes: the theme, the themes it inherits from, `hicolor`, then `/usr/share/pixmaps`. A name the themes lack is tried again without its last dash-separated part (`document-send-mail` finds `document-send`).
- **Cost.** Nothing is read until the first themed icon is built. Each icon is then read and decoded once, on the UI thread (a few kilobytes), and kept by the application's `IconThemeService`.
- **Theme changes.** While the application's theme follows the OS (`ThemeMode::Native` or `FollowSystem`), each appearance change the desktop announces reloads the icon theme and rebuilds every themed icon. On KDE Plasma that includes a new icon theme (it is written to `kdeglobals`); on GNOME a new icon theme alone is not heard, and is seen at the next start or the next change of colors.

`TeksiloAppBuilder::run` registers an `IconThemeService` reading the desktop. Register your own with `.app_state(IconThemeService::with_source(...))` to use a fixed theme or a test fixture; a headless app has none unless you register one, so it draws the fallbacks. The lookup itself is `teksilo_platform::icon_theme` (`IconTheme::load`, `IconTheme::lookup`, `current_icon_theme_name`) for code that wants the file paths.

## Raster images and GPU memory

A window uploads each raster it draws to a GPU texture, with a mip chain. How long the texture stays depends on how the widget got its pixels.

**A shared texture, kept until the window closes.** `ImageWidget::new(&icon)` without a mask, and `IconWidget::from_raster`, name the texture after the icon's identity:

- Clones of one `RasterIcon` share its pixels and its texture, and a `res!()` raster is a single static value, so showing one image in many widgets costs one texture (one per mode for `IconWidget`).
- Every `RasterIcon::decode` and `RasterIcon::from_raw` call is a new identity, even for the same bytes.
- The texture is never freed before the window closes, even once no widget shows the icon.

**A texture of its own, freed with the widget.** `ImageWidget::from_raw` and any `ImageWidget` with a `mask` hold their pixels in a source of their own, and so does an `Avatar`'s image. The window frees that texture at the first frame that does not draw the widget, because it was destroyed, scrolled out or parked, and uploads it again if the widget comes back. GPU memory follows what is on screen.

Pick by how long the image lives:

- A bounded set of long-lived images (toolbar art, a logo, a handful of placeholders): `res!()` or a cached `RasterIcon`, shown with `ImageWidget::new(&icon)`.
- Images that come and go, such as album covers or thumbnails in a virtualized grid: `ImageWidget::from_raw(pixels, width, height)`. Keep the decoded pixels in an application cache if decoding is expensive. A tile then costs a texture only while it is drawn.

Building tiles with `ImageWidget::new(&icon)` from icons decoded at runtime is the case to avoid: each new identity keeps its texture until the window closes. Debug builds print a warning when one window holds more than 256 shared image textures.

## Creating Icon Assets

### SVG icons

Use any SVG editor. Icons should be single-color paths on a transparent background. Fill and stroke colors in the SVG are ignored, the rendering color comes from the theme.

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
