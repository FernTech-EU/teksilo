// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Anti-aliased alpha masking for raster images — circle / rounded-square
//! / square coverage applied in-place to RGBA8 pixel buffers.
//!
//! The retained renderer's `Canvas::set_clip` is rectangular-only, so to
//! crop a photo into a circle (avatar, contact icon, channel thumbnail,
//! etc.) we modulate the source image's alpha channel with a per-pixel
//! coverage value computed analytically. 4×4 super-sampling (16
//! sub-samples per pixel) gives a smooth edge at the small sizes these
//! masks are typically used at (≤96 logical pixels).
//!
//! Used directly by [`ImageWidget::mask`](super::ImageWidget::mask) and
//! by `Avatar`. Other widgets that want a non-rectangular image silhouette
//! can call [`apply_alpha_mask`] and [`center_crop_square`] directly.
//!
//! ```rust
//! # use bastyde_widgets::primitives::image_mask::{ImageMaskShape, apply_alpha_mask};
//! let mut pixels = vec![255u8; 32 * 32 * 4]; // opaque white 32×32
//! apply_alpha_mask(&mut pixels, 32, 32, ImageMaskShape::Circle);
//! // Corner pixels are now transparent; the center is still opaque.
//! assert_eq!(pixels[3], 0);         // top-left alpha
//! assert_eq!(pixels[(16 * 32 + 16) * 4 + 3], 255); // center alpha
//! ```

/// Shape of the alpha mask applied to an image.
///
/// `RoundedSquare` carries the corner radius **as a fraction of the
/// shorter side** (0.0 ⇒ square, 0.5 ⇒ circle), matching the convention
/// `Avatar` and `ImageWidget::mask` accept on their public APIs. The
/// `apply_alpha_mask` helper expects a radius in *pixels* — convert
/// before calling.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ImageMaskShape {
    /// No mask. The pixels pass through unchanged.
    #[default]
    None,
    /// Inscribed circle in the image's bounding square (after a centred
    /// crop to the shorter side).
    Circle,
    /// Rounded rectangle. The carried `f32` is the corner radius as a
    /// fraction of `min(width, height)`, clamped to `0.0..=0.5`.
    RoundedSquare(f32),
}

/// Internal mask shape used by `apply_alpha_mask` after the radius
/// has been resolved to pixel space. Kept private so callers don't
/// accidentally mix the ratio API and the absolute API.
#[derive(Debug, Clone, Copy)]
enum MaskShape {
    Circle,
    RoundedSquare(f32),
    Square,
}

const SAMPLES_PER_AXIS: u32 = 4;

/// Crop the source RGBA buffer to a centered square of edge `min(w, h)`.
/// The returned buffer is `side * side * 4` bytes. If the input is
/// already square, a copy of the original is returned.
pub fn center_crop_square(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32) {
    let side = width.min(height);
    if width == side && height == side {
        return (pixels.to_vec(), side);
    }
    debug_assert_eq!(
        pixels.len(),
        (width * height * 4) as usize,
        "pixel buffer length must be width * height * 4"
    );
    let x_off = ((width - side) / 2) as usize;
    let y_off = ((height - side) / 2) as usize;
    let stride = (width * 4) as usize;
    let row_bytes = (side * 4) as usize;
    let mut out = Vec::with_capacity((side as usize) * row_bytes);
    for j in 0..side as usize {
        let row_start = (y_off + j) * stride + x_off * 4;
        out.extend_from_slice(&pixels[row_start..row_start + row_bytes]);
    }
    (out, side)
}

/// Apply an alpha mask in-place to an RGBA8 buffer. RGB channels are
/// preserved; only alpha is modulated by the coverage value, so a
/// pre-multiplied source remains pre-multiplied (the alpha-channel-only
/// transformation matches `RasterIcon::to_alpha_mask`).
///
/// The `shape` accepts the public [`ImageMaskShape`] surface; the
/// `RoundedSquare` radius is interpreted as a **fraction** of
/// `min(width, height)`, clamped to `0.0..=0.5`. `None` is a no-op.
pub fn apply_alpha_mask(pixels: &mut [u8], width: u32, height: u32, shape: ImageMaskShape) {
    debug_assert_eq!(pixels.len(), (width * height * 4) as usize);
    let internal = match shape {
        ImageMaskShape::None => return,
        ImageMaskShape::Circle => MaskShape::Circle,
        ImageMaskShape::RoundedSquare(ratio) => {
            let r = ratio.clamp(0.0, 0.5) * (width.min(height) as f32);
            if r <= 0.0 {
                MaskShape::Square
            } else {
                MaskShape::RoundedSquare(r)
            }
        }
    };
    let radius = match internal {
        MaskShape::Square => return,
        MaskShape::Circle => (width.min(height) as f32) / 2.0,
        MaskShape::RoundedSquare(r) => r,
    };
    apply_rounded(pixels, width, height, radius);
}

fn apply_rounded(pixels: &mut [u8], width: u32, height: u32, radius: f32) {
    if width == 0 || height == 0 {
        return;
    }
    let w = width as f32;
    let h = height as f32;
    let r = radius.clamp(0.0, (w.min(h)) / 2.0);
    if r <= 0.0 {
        // Square corner — every pixel fully covered, nothing to do.
        return;
    }

    for j in 0..height {
        for i in 0..width {
            let coverage = pixel_coverage(i as f32, j as f32, w, h, r);
            let idx = ((j * width + i) * 4 + 3) as usize;
            let original = pixels[idx] as f32;
            // Round-to-nearest, not truncate, so a fully-covered pixel
            // stays at 255 instead of rounding down.
            let masked = (original * coverage + 0.5).clamp(0.0, 255.0) as u8;
            pixels[idx] = masked;
        }
    }
}

/// Coverage of one pixel by a rounded-rectangle of size `w` × `h` with
/// corner radius `r`, super-sampled `SAMPLES_PER_AXIS²` times. The
/// pixel's top-left integer coordinate is `(px, py)`.
///
/// Each sub-sample is at the center of its sub-pixel cell; coverage is
/// `1.0` if the sub-sample is inside the rounded rectangle, `0.0`
/// otherwise. The mean over all samples is the pixel's anti-aliased
/// alpha multiplier.
fn pixel_coverage(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let mut hits: u32 = 0;
    let total = SAMPLES_PER_AXIS * SAMPLES_PER_AXIS;
    for sy in 0..SAMPLES_PER_AXIS {
        for sx in 0..SAMPLES_PER_AXIS {
            let sub_x = px + (sx as f32 + 0.5) / SAMPLES_PER_AXIS as f32;
            let sub_y = py + (sy as f32 + 0.5) / SAMPLES_PER_AXIS as f32;
            if inside_rounded_rect(sub_x, sub_y, w, h, r) {
                hits += 1;
            }
        }
    }
    hits as f32 / total as f32
}

#[inline]
fn inside_rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> bool {
    if x < 0.0 || y < 0.0 || x > w || y > h {
        return false;
    }
    // Closest point of the inner "rounded core" rectangle [r..w-r] × [r..h-r].
    let cx = x.clamp(r, w - r);
    let cy = y.clamp(r, h - r);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32) -> Vec<u8> {
        // RGBA = (10, 20, 30, 200) so we can detect RGB preservation.
        let mut v = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            v.extend_from_slice(&[10, 20, 30, 200]);
        }
        v
    }

    fn alpha_at(pixels: &[u8], width: u32, x: u32, y: u32) -> u8 {
        pixels[((y * width + x) * 4 + 3) as usize]
    }

    #[test]
    fn mask_circle_zeros_corners() {
        let mut pixels = solid(32, 32);
        apply_alpha_mask(&mut pixels, 32, 32, ImageMaskShape::Circle);
        // All four corners are fully outside an inscribed circle.
        assert_eq!(alpha_at(&pixels, 32, 0, 0), 0);
        assert_eq!(alpha_at(&pixels, 32, 31, 0), 0);
        assert_eq!(alpha_at(&pixels, 32, 0, 31), 0);
        assert_eq!(alpha_at(&pixels, 32, 31, 31), 0);
    }

    #[test]
    fn mask_circle_full_center() {
        let mut pixels = solid(32, 32);
        apply_alpha_mask(&mut pixels, 32, 32, ImageMaskShape::Circle);
        // The center pixel sits well inside the circle and must
        // preserve the source alpha exactly.
        assert_eq!(alpha_at(&pixels, 32, 16, 16), 200);
    }

    #[test]
    fn mask_circle_aa_at_boundary() {
        // The 32×32 inscribed circle has radius 16 centred at (16, 16).
        // At y = 4 the boundary x is 16 ± √(256 − 144) ≈ 16 ± 10.58, so
        // pixel (5, 4) (centre 5.5, 4.5) straddles the curve — some
        // sub-samples are inside the circle, some outside, so the
        // resulting alpha must be strictly between 0 and 200.
        let mut pixels = solid(32, 32);
        apply_alpha_mask(&mut pixels, 32, 32, ImageMaskShape::Circle);
        let edge = alpha_at(&pixels, 32, 5, 4);
        assert!(
            edge > 0 && edge < 200,
            "expected partial coverage at the curve boundary, got {edge}"
        );
    }

    #[test]
    fn mask_rounded_square_radius_zero_is_passthrough() {
        let mut pixels = solid(16, 16);
        apply_alpha_mask(&mut pixels, 16, 16, ImageMaskShape::RoundedSquare(0.0));
        // Every pixel keeps its original alpha.
        for j in 0..16 {
            for i in 0..16 {
                assert_eq!(alpha_at(&pixels, 16, i, j), 200);
            }
        }
    }

    #[test]
    fn mask_rounded_square_full_radius_equals_circle() {
        let mut a = solid(24, 24);
        let mut b = solid(24, 24);
        apply_alpha_mask(&mut a, 24, 24, ImageMaskShape::Circle);
        // ratio = 0.5 ⇒ radius = 0.5 × 24 = 12 ⇒ matches a circle.
        apply_alpha_mask(&mut b, 24, 24, ImageMaskShape::RoundedSquare(0.5));
        // Both formulas reduce to a circle when radius == size/2 of a
        // square buffer. Allow a 1-LSB rounding tolerance.
        for (av, bv) in a.iter().zip(b.iter()) {
            assert!(
                av.abs_diff(*bv) <= 1,
                "circle and full-radius rounded-square should match within 1 alpha LSB"
            );
        }
    }

    #[test]
    fn mask_preserves_rgb() {
        let mut pixels = solid(16, 16);
        apply_alpha_mask(&mut pixels, 16, 16, ImageMaskShape::Circle);
        for i in (0..pixels.len()).step_by(4) {
            assert_eq!(pixels[i], 10);
            assert_eq!(pixels[i + 1], 20);
            assert_eq!(pixels[i + 2], 30);
        }
    }

    #[test]
    fn mask_none_is_noop() {
        let mut pixels = solid(8, 8);
        apply_alpha_mask(&mut pixels, 8, 8, ImageMaskShape::None);
        for j in 0..8 {
            for i in 0..8 {
                assert_eq!(alpha_at(&pixels, 8, i, j), 200);
            }
        }
    }

    #[test]
    fn mask_handles_size_one_image() {
        // A 1×1 image masked to a circle inscribed in 1×1: only the
        // four sub-samples within √(0.5)−0.5 of the centre are inside
        // the unit-diameter circle, so the result is partial coverage.
        // The contract is no panic + alpha non-zero + alpha ≤ source.
        let mut pixels = vec![10, 20, 30, 200];
        apply_alpha_mask(&mut pixels, 1, 1, ImageMaskShape::Circle);
        assert!(pixels[3] > 0, "1×1 alpha must remain non-zero");
        assert!(pixels[3] <= 200, "1×1 alpha cannot exceed source");
        // RGB still preserved.
        assert_eq!(&pixels[..3], &[10, 20, 30]);
    }

    #[test]
    fn mask_circle_alpha_multiplied_with_source() {
        // A pixel-dim source (alpha = 100) in the circle's interior
        // must keep its 100/255 alpha — not be promoted to 255.
        let mut pixels = Vec::with_capacity(32 * 32 * 4);
        for _ in 0..(32 * 32) {
            pixels.extend_from_slice(&[10, 20, 30, 100]);
        }
        apply_alpha_mask(&mut pixels, 32, 32, ImageMaskShape::Circle);
        assert_eq!(alpha_at(&pixels, 32, 16, 16), 100);
        // Corner is still zero — coverage zero × source alpha = 0.
        assert_eq!(alpha_at(&pixels, 32, 0, 0), 0);
    }

    #[test]
    fn center_crop_square_is_identity_when_already_square() {
        let p = solid(16, 16);
        let (out, side) = center_crop_square(&p, 16, 16);
        assert_eq!(side, 16);
        assert_eq!(out, p);
    }

    #[test]
    fn center_crop_square_landscape() {
        // 8 wide × 4 tall: should crop the centered 4×4 square (cols 2..6).
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..8 {
                pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        let (out, side) = center_crop_square(&pixels, 8, 4);
        assert_eq!(side, 4);
        assert_eq!(out.len(), 4 * 4 * 4);
        // Top-left of the crop is the column at x = 2 of the original.
        assert_eq!(out[0], 2);
        // Bottom-right of the crop is the column at x = 5 of the original.
        let last = out.len() - 4;
        assert_eq!(out[last], 5);
    }

    #[test]
    fn center_crop_square_portrait() {
        // 4 wide × 8 tall: should crop rows 2..6.
        let mut pixels = Vec::new();
        for y in 0..8 {
            for x in 0..4 {
                pixels.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        let (out, side) = center_crop_square(&pixels, 4, 8);
        assert_eq!(side, 4);
        assert_eq!(out[1], 2); // first pixel's y == 2
    }
}
