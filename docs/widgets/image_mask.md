<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ImageMaskShape

Anti-aliased alpha masking for raster images — circle / rounded-square
/ square coverage applied in-place to RGBA8 pixel buffers.

The retained renderer's `Canvas::set_clip` is rectangular-only, so to
crop a photo into a circle (avatar, contact icon, channel thumbnail,
etc.) we modulate the source image's alpha channel with a per-pixel
coverage value computed analytically. 4×4 super-sampling (16
sub-samples per pixel) gives a smooth edge at the small sizes these
masks are typically used at (≤96 logical pixels).

Used directly by `ImageWidget::mask` and
by `Avatar`. Other widgets that want a non-rectangular image silhouette
can call `apply_alpha_mask` and `center_crop_square` directly.

```rust
# use bastyde_widgets::primitives::image_mask::{ImageMaskShape, apply_alpha_mask};
let mut pixels = vec![255u8; 32 * 32 * 4]; // opaque white 32×32
apply_alpha_mask(&mut pixels, 32, 32, ImageMaskShape::Circle);
// Corner pixels are now transparent; the center is still opaque.
assert_eq!(pixels[3], 0);         // top-left alpha
assert_eq!(pixels[(16 * 32 + 16) * 4 + 3], 255); // center alpha
```

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/primitives/image_mask/index.html)

## `pub enum ImageMaskShape`

Shape of the alpha mask applied to an image.

`RoundedSquare` carries the corner radius **as a fraction of the
shorter side** (0.0 ⇒ square, 0.5 ⇒ circle), matching the convention
`Avatar` and `ImageWidget::mask` accept on their public APIs. The
`apply_alpha_mask` helper expects a radius in *pixels* — convert
before calling.

```rust
pub enum ImageMaskShape { /* variants */ }
```

### Variants

- **`None`** — No mask. The pixels pass through unchanged.
- **`Circle`** — Inscribed circle in the image's bounding square (after a centred crop to the shorter side).
- **`RoundedSquare`** — Rounded rectangle. The carried `f32` is the corner radius as a fraction of `min(width, height)`, clamped to `0.0..=0.5`.

## `pub fn center_crop_square(...)`

Crop the source RGBA buffer to a centered square of edge `min(w, h)`.
The returned buffer is `side * side * 4` bytes. If the input is
already square, a copy of the original is returned.

```rust
pub fn center_crop_square(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32);
```

## `pub fn apply_alpha_mask(...)`

Apply an alpha mask in-place to an RGBA8 buffer. RGB channels are
preserved; only alpha is modulated by the coverage value, so a
pre-multiplied source remains pre-multiplied (the alpha-channel-only
transformation matches `RasterIcon::to_alpha_mask`).

The `shape` accepts the public `ImageMaskShape` surface; the
`RoundedSquare` radius is interpreted as a **fraction** of
`min(width, height)`, clamped to `0.0..=0.5`. `None` is a no-op.

```rust
pub fn apply_alpha_mask(pixels: &mut [u8], width: u32, height: u32, shape: ImageMaskShape);
```
