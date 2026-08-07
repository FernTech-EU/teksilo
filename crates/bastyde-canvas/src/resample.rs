// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Correct RGBA8 downscaling, shared by the mip builder and by image decoding.
//!
//! Two corrections separate a correct resample from a subtly wrong one, and
//! both are easy to get backwards. They are stated once here because this
//! module is the single implementation; [`super::raster::RasterIcon`] and
//! `bastyde-render`'s mip chain both call into it.
//!
//! 1. **Filter in linear light, not in sRGB.** Image bytes are sRGB-encoded and
//!    the GPU linearises them at sample time. Averaging the *encoded* bytes
//!    averages the wrong quantity: black and white side by side average to byte
//!    128, which is ~22% linear luminance, not the 50% the eye expects.
//!    Downscaled images come out visibly too dark.
//!
//! 2. **Filter premultiplied, store straight.** The atlas holds straight
//!    (non-premultiplied) alpha. A fully transparent texel still carries some
//!    RGB — usually whatever the encoder left behind — and averaging that in as
//!    if it were visible bleeds it into every neighbour: a dark halo around an
//!    antialiased rim that grows with each step. Weighting each texel's colour
//!    by its alpha is what stops a transparent neighbour voting on the colour.
//!
//! Everything here is pure and CPU-side, so it is unit-tested headlessly.

use std::sync::OnceLock;

/// Decode one sRGB byte to linear (IEC 61966-2-1), memoizing all 256 answers —
/// a full resample decodes every texel, so the `powf` would otherwise run
/// millions of times for a large image.
pub fn srgb_to_linear(byte: u8) -> f32 {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    table[byte as usize]
}

/// Re-encode a linear value back to an sRGB byte — the inverse of
/// [`srgb_to_linear`].
pub fn linear_to_srgb_byte(linear: f32) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let encoded = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// Halve an RGBA8 image with a 2×2 box filter in linear, premultiplied space.
///
/// Odd dimensions halve down (`5 → 2`) and the box clamps to the last
/// row/column rather than reading out of bounds.
pub fn downsample_half(src: &[u8], src_w: u32, src_h: u32) -> (u32, u32, Vec<u8>) {
    let dst_w = (src_w / 2).max(1);
    let dst_h = (src_h / 2).max(1);
    let mut dst = vec![0u8; dst_w as usize * dst_h as usize * 4];

    for y in 0..dst_h {
        for x in 0..dst_w {
            let x0 = (x * 2).min(src_w - 1);
            let x1 = (x * 2 + 1).min(src_w - 1);
            let y0 = (y * 2).min(src_h - 1);
            let y1 = (y * 2 + 1).min(src_h - 1);

            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for (sy, sx) in [(y0, x0), (y0, x1), (y1, x0), (y1, x1)] {
                let i = ((sy as usize * src_w as usize) + sx as usize) * 4;
                let sa = src[i + 3] as f32 / 255.0;
                r += srgb_to_linear(src[i]) * sa;
                g += srgb_to_linear(src[i + 1]) * sa;
                b += srgb_to_linear(src[i + 2]) * sa;
                a += sa;
            }

            let o = ((y as usize * dst_w as usize) + x as usize) * 4;
            // Un-premultiply by the *summed* alpha (not by 4): the colour
            // average is over the texels that actually carried colour.
            if a > 0.0 {
                dst[o] = linear_to_srgb_byte(r / a);
                dst[o + 1] = linear_to_srgb_byte(g / a);
                dst[o + 2] = linear_to_srgb_byte(b / a);
            }
            dst[o + 3] = ((a / 4.0) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }

    (dst_w, dst_h, dst)
}

/// Resample an RGBA8 image to exactly `dst_w` × `dst_h` by area averaging.
///
/// Each destination pixel covers a rectangle of source pixels — generally a
/// fractional one — and every source pixel it touches contributes in proportion
/// to the overlapped area. That is the correct filter for downscaling by an
/// arbitrary ratio, and unlike repeated halving it does not quantise the
/// achievable sizes to powers of two.
///
/// Returns an empty buffer for degenerate inputs rather than panicking.
pub fn resample_area(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let (sw, sh) = (src_w as usize, src_h as usize);
    let (dw, dh) = (dst_w as usize, dst_h as usize);
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || src.len() < sw * sh * 4 {
        return Vec::new();
    }

    let x_ratio = sw as f32 / dw as f32;
    let y_ratio = sh as f32 / dh as f32;
    let mut dst = vec![0u8; dw * dh * 4];

    for dy in 0..dh {
        let sy0 = dy as f32 * y_ratio;
        let sy1 = sy0 + y_ratio;
        for dx in 0..dw {
            let sx0 = dx as f32 * x_ratio;
            let sx1 = sx0 + x_ratio;

            let (mut r, mut g, mut b, mut a, mut total) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);

            for sy in (sy0.floor() as usize)..(sy1.ceil() as usize).min(sh) {
                // How much of this source row the destination row covers.
                let wy = (sy1.min(sy as f32 + 1.0) - sy0.max(sy as f32)).max(0.0);
                if wy <= 0.0 {
                    continue;
                }
                for sx in (sx0.floor() as usize)..(sx1.ceil() as usize).min(sw) {
                    let wx = (sx1.min(sx as f32 + 1.0) - sx0.max(sx as f32)).max(0.0);
                    if wx <= 0.0 {
                        continue;
                    }
                    let w = wx * wy;
                    let i = (sy * sw + sx) * 4;
                    let sa = src[i + 3] as f32 / 255.0;
                    r += srgb_to_linear(src[i]) * sa * w;
                    g += srgb_to_linear(src[i + 1]) * sa * w;
                    b += srgb_to_linear(src[i + 2]) * sa * w;
                    a += sa * w;
                    total += w;
                }
            }

            let o = (dy * dw + dx) * 4;
            if a > 0.0 {
                dst[o] = linear_to_srgb_byte(r / a);
                dst[o + 1] = linear_to_srgb_byte(g / a);
                dst[o + 2] = linear_to_srgb_byte(b / a);
            }
            if total > 0.0 {
                dst[o + 3] = ((a / total) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }

    dst
}

/// The size `(w, h)` scaled to fit within `max_edge` on its longer side,
/// preserving aspect ratio. Returns `None` if it already fits.
///
/// Never returns a zero dimension: an extreme aspect ratio (a 4000×1 banner)
/// would otherwise round its short side to nothing.
pub fn fit_within(width: u32, height: u32, max_edge: u32) -> Option<(u32, u32)> {
    if max_edge == 0 || width == 0 || height == 0 || width.max(height) <= max_edge {
        return None;
    }
    let scale = max_edge as f64 / width.max(height) as f64;
    let w = ((width as f64 * scale).round() as u32).max(1);
    let h = ((height as f64 * scale).round() as u32).max(1);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trips_through_linear() {
        for b in [0u8, 1, 55, 128, 200, 254, 255] {
            assert_eq!(linear_to_srgb_byte(srgb_to_linear(b)), b, "byte {b}");
        }
    }

    #[test]
    fn black_and_white_average_to_mid_luminance_not_mid_byte() {
        // The whole point of filtering in linear light: the naive answer is 128.
        let src = vec![0, 0, 0, 255, 255, 255, 255, 255];
        let out = resample_area(&src, 2, 1, 1, 1);
        let mid = linear_to_srgb_byte(0.5);
        assert_eq!(out[0], mid);
        assert!(
            out[0] > 180,
            "linear-light average should be ~188, got {}",
            out[0]
        );
    }

    #[test]
    fn transparent_neighbour_does_not_vote_on_colour() {
        // Opaque red beside transparent black: the result must stay red, not
        // drift toward black, or antialiased edges grow dark halos.
        let src = vec![255, 0, 0, 255, 0, 0, 0, 0];
        let out = resample_area(&src, 2, 1, 1, 1);
        assert_eq!(&out[0..3], &[255, 0, 0]);
        assert_eq!(out[3], 128, "alpha is the plain area average");
    }

    #[test]
    fn resample_to_same_size_is_lossless() {
        let src: Vec<u8> = (0..(4 * 3 * 4)).map(|i| (i * 7 % 256) as u8).collect();
        let out = resample_area(&src, 4, 3, 4, 3);
        assert_eq!(out, src);
    }

    #[test]
    fn resample_produces_the_requested_size() {
        let src = vec![200u8; 10 * 10 * 4];
        for (w, h) in [(3u32, 3u32), (7, 2), (1, 1), (10, 4)] {
            let out = resample_area(&src, 10, 10, w, h);
            assert_eq!(out.len(), (w * h * 4) as usize, "{w}x{h}");
        }
    }

    #[test]
    fn a_flat_colour_survives_any_ratio() {
        // Area averaging a uniform image must not shift its colour.
        let src = [70u8, 130, 180, 255].repeat(9 * 9);
        let out = resample_area(&src, 9, 9, 4, 4);
        for px in out.chunks(4) {
            assert_eq!(px, &[70, 130, 180, 255]);
        }
    }

    #[test]
    fn degenerate_inputs_return_empty_rather_than_panicking() {
        assert!(resample_area(&[], 0, 0, 4, 4).is_empty());
        assert!(resample_area(&[1, 2, 3, 4], 1, 1, 0, 4).is_empty());
        assert!(
            resample_area(&[1, 2], 4, 4, 2, 2).is_empty(),
            "short buffer"
        );
    }

    #[test]
    fn halving_matches_area_resample_for_even_sizes() {
        let src: Vec<u8> = (0..(8 * 8 * 4)).map(|i| (i % 251) as u8).collect();
        let (w, h, half) = downsample_half(&src, 8, 8);
        let area = resample_area(&src, 8, 8, 4, 4);
        assert_eq!((w, h), (4, 4));
        // Both compute the same 2×2 box average; allow one ulp of rounding.
        for (a, b) in half.iter().zip(area.iter()) {
            assert!(a.abs_diff(*b) <= 1, "half={a} area={b}");
        }
    }

    #[test]
    fn fit_within_preserves_aspect_and_never_returns_zero() {
        assert_eq!(fit_within(4000, 3000, 2000), Some((2000, 1500)));
        assert_eq!(fit_within(3000, 4000, 2000), Some((1500, 2000)));
        assert_eq!(fit_within(100, 100, 2000), None, "already fits");
        assert_eq!(
            fit_within(4000, 1, 100),
            Some((100, 1)),
            "short side clamps to 1"
        );
        assert_eq!(fit_within(0, 10, 100), None);
    }
}
