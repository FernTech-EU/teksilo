<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ImageWidget

ImageWidget — displays a raster image (PNG, WebP) with a configurable
sizing policy, content-fit mode, and intra-box alignment.

Unlike `IconWidget` which is designed
for small square tintable icons, `ImageWidget` handles arbitrary aspect
ratios and defaults to full-color rendering.

# Sizing model

Two independent concerns, mirroring Qt's `QLabel`/`QPixmap`, SwiftUI's
`Image`, and CSS's replaced-element model:

1. **Box size** — how big the widget's layout rectangle is.
   - `width` / `height` /
     `size` pin a **fixed** logical extent. A pinned
     axis is *rigid*: it is reported as-is and is never scaled up to a
     parent's proposal (this is the SwiftUI `.frame(width:height:)` /
     Qt fixed-size behaviour). Pinning only one axis derives the other
     from the image's aspect ratio (CSS `width: Npx; height: auto`).
   - With no axis pinned the widget reports its **natural pixel size**.
     By default (`resizable` `= true`) a
     constraining proposal scales that natural size down/up while
     preserving aspect ratio; `resizable(false)` locks it to the raw
     pixel dimensions (SwiftUI's default non-`.resizable()` image).
2. **Content fit** — how the image pixels map into that box, via
   `ImageFit` (`Contain` / `Cover` / `Fill` / `ScaleDown` / `None`,
   the CSS `object-fit` set) plus
   `alignment` (the CSS `object-position`
   equivalent) for where slack/overflow lands. Modes that overflow the
   box (`Cover`, and `None` on an oversized image) are clipped to the
   box so the image never bleeds past its layout rectangle.

For a fixed 32×32 logo: `ImageWidget::new(icon).size(32.0, 32.0)` — the
box is exactly 32×32 and the artwork is letterboxed inside it
(`Contain`, the default).

```rust
# use teksilo_canvas::RasterIcon;
# use teksilo_widgets::primitives::image_widget::{ImageWidget, ImageFit};
# use teksilo_widgets::primitives::image_mask::ImageMaskShape;
// A 64×64 image shown at natural size with no masking.
let icon = RasterIcon::from_raw(vec![255; 64 * 64 * 4], 64, 64);
let _logo = ImageWidget::new(&icon).size(32.0, 32.0);

// Cover a square avatar slot and crop to a circle.
let _avatar = ImageWidget::new(&icon)
    .mask(ImageMaskShape::Circle)
    .fit(ImageFit::Cover)
    .alt("User avatar")
    .size(48.0, 48.0);
```

## Builder methods at a glance

`from_raw`, `mask`, `fit`, `alignment`, `width`, `height`, `size`, `resizable`, `alt`, `a11y_hidden`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/primitives/image_widget/index.html)

## `pub enum ImageFit`

How the image is fitted within its layout bounds.

```rust
pub enum ImageFit { /* variants */ }
```

### Variants

- **`Contain`** — Scale to fit entirely within bounds, preserving aspect ratio. May leave empty space (letterboxing).
- **`Cover`** — Scale to cover the entire bounds, preserving aspect ratio. May crop the image.
- **`Fill`** — Stretch to fill bounds exactly, ignoring aspect ratio.
- **`ScaleDown`** — Like Contain but never upscales — if the image is smaller than bounds, it is centered at its natural size.
- **`None`** — Draw the image at its natural pixel size, neither scaling up nor down. If the image is larger than the box it is cropped to the box (positioned by `alignment`); if smaller it sits inside with empty space. CSS `object-fit: none`.

## `pub struct ImageWidget`

A widget that displays a raster image (PNG, WebP, or raw RGBA pixels) with configurable fit and alignment.

```rust
pub struct ImageWidget { /* fields */ }
```

### Methods

#### `pub fn new(icon: &RasterIcon) -> Self`

Create from a decoded `RasterIcon` (e.g., from `res!()`).

#### `pub fn from_raw(pixels: Vec<u8>, width: u32, height: u32) -> Self`

Create from raw RGBA pixel data.

Each call gets a unique texture-atlas key (via a process-local
atomic counter), so two `from_raw` widgets with the same
dimensions but different bytes don't alias in the renderer's
pending-image cache. Without this, the first writer per frame
would silently win and subsequent ones would render the wrong
pixels — a latent bug fixed alongside the dynamic-image use
cases that need many short-lived `from_raw` widgets.

#### `pub fn mask(mut self, shape: ImageMaskShape) -> Self`

Apply an anti-aliased alpha mask to the image at construction
time. The pixels are first centre-cropped to the shorter side
(so the mask shape is geometrically consistent regardless of
the source aspect ratio), then their alpha channel is
modulated by the mask coverage. RGB is preserved.

`Cover` fit is the natural pairing — the masked square fills
the avatar/thumbnail bounds and the masked-out corners stay
transparent. `Contain` works but may letterbox. The default
fit (`Contain`) is left unchanged so callers explicitly pick
a fit when they apply a mask.

`ImageMaskShape::None` is a no-op. Re-uploading is keyed off a
fresh per-mask name so the un-masked version of the same
source doesn't shadow the masked one in the texture atlas.

#### `pub fn fit(mut self, fit: ImageFit) -> Self`

Set the content-fit mode — how the image pixels map into the box.
See `ImageFit`.

#### `pub fn alignment(mut self, alignment: Alignment) -> Self`

Set where the fitted image sits within the box when the active fit
leaves slack or crops (the CSS `object-position` analogue). Defaults
to `Alignment::CENTER`. Leading/Trailing resolve against the
active layout direction (RTL-aware).

#### `pub fn width(mut self, w: f32) -> Self`

Pin a fixed display width (in logical pixels). The width axis
becomes rigid — reported as-is and never scaled to a parent
proposal. With no height pinned, the height derives from the
image's aspect ratio (CSS `width: Npx; height: auto`).

#### `pub fn height(mut self, h: f32) -> Self`

Pin a fixed display height (in logical pixels). The height axis
becomes rigid. With no width pinned, the width derives from the
image's aspect ratio.

#### `pub fn size(mut self, w: f32, h: f32) -> Self`

Pin both display width and height (in logical pixels). The box is
exactly this size, rigid on both axes; the image content is fitted
inside it via the `fit` mode. This is the
fixed-size-logo case — `.size(32.0, 32.0)`.

#### `pub fn resizable(mut self, resizable: bool) -> Self`

Control whether, with no axis pinned, a constraining parent
proposal scales the natural pixel size (`true`, the default) or the
box stays locked to the raw pixel dimensions (`false`). Equivalent
to opting out of SwiftUI's `.resizable()`. No effect once a
dimension is pinned via `width` /
`height` / `size`.

#### `pub fn alt(mut self, text: impl Into<String>) -> Self`

Set the accessibility alt text.

#### `pub fn a11y_hidden(mut self) -> Self`

Mark this image as decorative — hidden from the accessibility
tree. Use when the image's semantic content is already conveyed
by adjacent text (e.g. a hero image next to its caption). ARIA
equivalent of `alt=""` / `role="presentation"`.
