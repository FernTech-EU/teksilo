// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Mip-chain construction for uploaded images.
//!
//! A texture sampled with `min_filter: Linear` and a single mip level reads
//! only a 2×2 texel neighbourhood no matter how far it is minified. Draw a
//! 512×512 app icon into a 25 dp title-bar slot and the GPU picks 4 texels out
//! of every ~400: the result aliases — speckle in the interior, ragged edges,
//! and a shimmer that crawls whenever the quad moves by a subpixel. Prebuilding
//! a mip chain and sampling it trilinearly (`mipmap_filter: Linear`) is the fix,
//! and it costs 1/3 more texture memory.
//!
//! Two details make the difference between a correct chain and a subtly wrong
//! one, and both are easy to get backwards:
//!
//! 1. **Filter in linear light, not in sRGB.** Image textures are
//!    `Rgba8UnormSrgb` — the bytes are sRGB-encoded and the GPU linearizes them
//!    at sample time. Averaging the *encoded* bytes averages the wrong quantity:
//!    black and white side by side average to byte 128, which is ~22% linear
//!    luminance, not the 50% the eye expects. Downscaled images come out visibly
//!    too dark. So every box average here decodes to linear, averages, and
//!    re-encodes.
//!
//! 2. **Filter premultiplied, store straight.** The atlas holds *straight*
//!    (non-premultiplied) alpha — `quad.wgsl` samples `tex_color.rgb` directly
//!    and composites against an alpha-blending target. A fully transparent texel
//!    still carries some RGB (usually black, whatever the encoder left behind),
//!    and averaging that RGB in as if it were visible bleeds it into every
//!    neighbouring pixel: a dark halo around a logo's antialiased rim, growing
//!    with each mip level. Weighting each texel's color by its alpha (premultiply
//!    → average → unpremultiply) is what keeps a transparent neighbour from
//!    voting on the color.
//!
//! [`build_mip_chain`] is pure and CPU-side, so it is unit-tested headlessly;
//! [`super::image_manager::ImageManager::register_image`] uploads what it
//! returns.

// The two corrections above are subtle enough that a second copy of them would
// eventually drift from this one, so the kernel lives in `bastyde-canvas` and
// is shared with image decoding's downscale path.
use bastyde_canvas::resample::downsample_half;

/// Build every mip level below level 0 for an RGBA8 image, halving until 1×1.
///
/// `pixels` is level 0 — straight-alpha, sRGB-encoded, `width * height * 4`
/// bytes. The returned levels are ordered 1, 2, 3, … and each is
/// `(width, height, pixels)` in the same format, so a caller can upload them
/// with `mip_level` = index + 1. A 1×1 (or degenerate) input has no levels
/// below it and yields an empty vector.
///
/// Each level is a 2×2 box average of the one above it, taken in linear,
/// premultiplied space (see the module docs for why both matter). Odd
/// dimensions halve down (`5 → 2`) and the box clamps to the last row/column
/// rather than reading out of bounds.
pub(crate) fn build_mip_chain(pixels: &[u8], width: u32, height: u32) -> Vec<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || pixels.len() < (width as usize * height as usize * 4) {
        return Vec::new();
    }

    let mut levels = Vec::new();
    let mut src_w = width;
    let mut src_h = height;
    let mut src: Vec<u8> = pixels[..(width as usize * height as usize * 4)].to_vec();

    while src_w > 1 || src_h > 1 {
        let (dst_w, dst_h, dst) = downsample_half(&src, src_w, src_h);
        levels.push((dst_w, dst_h, dst.clone()));
        src = dst;
        src_w = dst_w;
        src_h = dst_h;
    }

    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 512×512 icon halves all the way down to 1×1: 9 levels below level 0.
    #[test]
    fn chain_halves_to_one_by_one() {
        let px = vec![255u8; 512 * 512 * 4];
        let chain = build_mip_chain(&px, 512, 512);
        let dims: Vec<(u32, u32)> = chain.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(
            dims,
            vec![
                (256, 256),
                (128, 128),
                (64, 64),
                (32, 32),
                (16, 16),
                (8, 8),
                (4, 4),
                (2, 2),
                (1, 1)
            ]
        );
        for (w, h, data) in &chain {
            assert_eq!(data.len(), (*w as usize) * (*h as usize) * 4);
        }
    }

    /// Nothing to build below a 1×1 (or a degenerate) image.
    #[test]
    fn no_levels_below_a_single_texel() {
        assert!(build_mip_chain(&[255, 0, 0, 255], 1, 1).is_empty());
        assert!(build_mip_chain(&[], 0, 0).is_empty());
        // Short buffer: refuse rather than read out of bounds.
        assert!(build_mip_chain(&[255, 0, 0, 255], 4, 4).is_empty());
    }

    /// **Averaging happens in linear light.** Black and white in equal measure
    /// is 50% *luminance*, which re-encodes to sRGB byte ~188 — not the 128 a
    /// naive average of the encoded bytes would produce. Getting this wrong is
    /// invisible in a unit test that only checks dimensions, and very visible
    /// on screen: every minified image comes out too dark.
    #[test]
    fn averages_in_linear_space_not_srgb() {
        // 2×2: two black texels, two white ones, all opaque.
        let mut px = Vec::new();
        for byte in [0u8, 255, 255, 0] {
            px.extend_from_slice(&[byte, byte, byte, 255]);
        }
        let chain = build_mip_chain(&px, 2, 2);
        let (_, _, level1) = &chain[0];
        let v = level1[0];
        assert!(
            (186..=190).contains(&v),
            "50% linear luminance should re-encode to sRGB ~188, got {v} \
             (128 means the bytes were averaged in sRGB space)"
        );
        assert_eq!(level1[3], 255, "alpha must stay opaque");
    }

    /// **Transparent texels must not vote on color.** One opaque red texel
    /// beside three transparent black ones averages to *red* at quarter alpha —
    /// not to a dark maroon. Averaging straight (non-premultiplied) RGB is what
    /// produces the classic dark halo around a logo's antialiased rim, and it
    /// compounds at every level of the chain.
    #[test]
    fn transparent_texels_do_not_darken_their_neighbours() {
        let mut px = Vec::new();
        px.extend_from_slice(&[255, 0, 0, 255]); // opaque red
        px.extend_from_slice(&[0, 0, 0, 0]); // transparent black
        px.extend_from_slice(&[0, 0, 0, 0]);
        px.extend_from_slice(&[0, 0, 0, 0]);

        let chain = build_mip_chain(&px, 2, 2);
        let (_, _, level1) = &chain[0];
        assert_eq!(
            &level1[0..3],
            &[255, 0, 0],
            "the surviving color must stay pure red, not be dragged toward black"
        );
        assert_eq!(
            level1[3], 64,
            "alpha is the plain average of the four texels (255/4)"
        );
    }

    /// Odd dimensions halve down and clamp at the edge instead of reading past it.
    #[test]
    fn odd_dimensions_halve_and_clamp() {
        let px = vec![128u8; 3 * 5 * 4];
        let chain = build_mip_chain(&px, 3, 5);
        let dims: Vec<(u32, u32)> = chain.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(dims, vec![(1, 2), (1, 1)]);
    }

    /// A uniformly colored image survives the round trip through linear space
    /// unchanged — the decode/encode pair must not drift the color.
    #[test]
    fn a_flat_color_survives_the_round_trip() {
        let px: Vec<u8> = std::iter::repeat_n([37u8, 150, 190, 255], 4 * 4)
            .flatten()
            .collect();
        let chain = build_mip_chain(&px, 4, 4);
        for (_, _, data) in &chain {
            for texel in data.chunks_exact(4) {
                assert!(
                    (texel[0] as i32 - 37).abs() <= 1
                        && (texel[1] as i32 - 150).abs() <= 1
                        && (texel[2] as i32 - 190).abs() <= 1
                        && texel[3] == 255,
                    "flat color drifted to {texel:?}"
                );
            }
        }
    }
}
