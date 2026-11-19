// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! How a glyph's antialiased edge is blended onto the page.
//!
//! The surface is an sRGB format, so the GPU blends in linear light. That is
//! right for images and gradients and wrong for text: a glyph's coverage
//! blended in linear light paints a half-covered edge pixel of black text on
//! white at 188/255 where FreeType, Skia, DirectWrite and Core Text paint it
//! near 128. Every stem has two such edges, so text reads thinner and greyer
//! than in any other application on the same screen.
//!
//! [`TextContrast`] remaps a glyph's coverage before the blend so that the
//! linear blend lands where a blend in a perceptual space would have, which
//! is the correction Skia applies to its glyph masks (its "preblend" tables)
//! and Firefox's WebRender inherited. The page under the glyph is unknown to
//! the shader, so, as in Skia, it is taken to be the opposite of the text's
//! luminance: dark text is corrected for a light page and light text for a
//! dark one. Text whose luminance is near the middle gets the contrast boost
//! only.
//!
//! [`TextContrast::corrected_coverage`] is the CPU statement of the remap and
//! `shaders/quad.wgsl`'s `text_coverage` is its GPU twin; the tests here pin
//! the first and `renderer.rs`'s GPU test checks the second against it.

/// The coverage remap applied to monochrome glyphs (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextContrast {
    /// The exponent, relative to sRGB-encoded values, of the space the glyph
    /// edge is blended in: `1.0` blends in sRGB-encoded values as FreeType
    /// and most toolkits do, larger values move toward linear light (about
    /// `2.2`), smaller values darken edges further. `0.0` turns the
    /// correction off and the coverage is blended in linear light as is.
    /// Clamped to `0.0..=4.0`.
    pub gamma: f32,
    /// Extra weight given to partly covered pixels, `coverage + (1 −
    /// coverage) · contrast · coverage`, scaled down as the text gets
    /// lighter (Skia's contrast term). `0.0` adds none. Clamped to
    /// `0.0..=1.0`.
    pub contrast: f32,
}

impl TextContrast {
    /// No correction: coverage is blended in linear light, as before this
    /// type existed.
    pub const OFF: Self = Self {
        gamma: 0.0,
        contrast: 0.0,
    };

    /// The correction a renderer starts with.
    pub const DEFAULT: Self = Self {
        gamma: 1.2,
        contrast: 0.2,
    };

    /// Heavier edges on dark text than [`DEFAULT`](Self::DEFAULT): the
    /// blend of sRGB-encoded values, plus half the contrast term.
    pub const STRONGER: Self = Self {
        gamma: 1.0,
        contrast: 0.5,
    };

    /// The heaviest edges this type offers: the blend of sRGB-encoded
    /// values, plus the full contrast term.
    pub const STRONGEST: Self = Self {
        gamma: 1.0,
        contrast: 1.0,
    };

    /// Whether this value changes anything.
    pub fn is_off(self) -> bool {
        self.gamma <= 0.0
    }

    /// The two parameters as the shader reads them: unorm16 `gamma / 4` in
    /// the low half, unorm16 `contrast` in the high half, `0` when off.
    pub(crate) fn packed(self) -> u32 {
        if self.is_off() {
            return 0;
        }
        let g = ((self.gamma.clamp(0.0, 4.0) / 4.0) * 65535.0).round() as u32;
        // A gamma that rounds to zero would read as "off" in the shader.
        let g = g.max(1);
        let c = (self.contrast.clamp(0.0, 1.0) * 65535.0).round() as u32;
        g | (c << 16)
    }

    /// The parameters [`packed`](Self::packed) stores, as the shader reads
    /// them back.
    fn unpacked(packed: u32) -> Option<(f32, f32)> {
        if packed == 0 {
            return None;
        }
        let g = (packed & 0xffff) as f32 / 65535.0 * 4.0;
        let c = (packed >> 16) as f32 / 65535.0;
        Some((g, c))
    }

    /// The coverage to blend in linear light for a glyph pixel of
    /// `coverage` in text of linear colour `text_rgb`, so that the result
    /// looks as if it had been blended in the space [`gamma`](Self::gamma)
    /// names. The CPU twin of the shader's `text_coverage`; it reads the
    /// parameters through the same packing so both see the same values.
    pub fn corrected_coverage(self, coverage: f32, text_rgb: [f32; 3]) -> f32 {
        let Some((gamma, contrast)) = Self::unpacked(self.packed()) else {
            return coverage;
        };
        let coverage = coverage.clamp(0.0, 1.0);
        let src_lin = luminance(text_rgb);
        let src = srgb_encode(src_lin);
        // The page is unknown; take it to be the text's opposite.
        let dst = 1.0 - src;
        let dst_lin = srgb_decode(dst);
        let dst_g = dst.powf(gamma);
        // Skia's contrast term, fading out as the text gets lighter.
        let c = contrast * dst_g;
        let a = coverage + (1.0 - coverage) * c * coverage;
        if (src - dst).abs() < 1.0 / 256.0 {
            return a.clamp(0.0, 1.0);
        }
        let src_g = src.powf(gamma);
        let out = (dst_g + (src_g - dst_g) * a).max(0.0).powf(1.0 / gamma);
        ((srgb_decode(out) - dst_lin) / (src_lin - dst_lin)).clamp(0.0, 1.0)
    }
}

impl Default for TextContrast {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Rec. 709 luminance of a linear colour.
fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn srgb_encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_decode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: [f32; 3] = [0.0, 0.0, 0.0];
    const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

    /// The sRGB byte a linear blend of `text` over `page` at `alpha` writes.
    fn blended_byte(text: f32, page: f32, alpha: f32) -> u8 {
        let lin = srgb_decode(page) + (srgb_decode(text) - srgb_decode(page)) * alpha;
        (srgb_encode(lin) * 255.0).round() as u8
    }

    #[test]
    fn off_leaves_coverage_alone() {
        for i in 0..=10 {
            let a = i as f32 / 10.0;
            assert_eq!(TextContrast::OFF.corrected_coverage(a, BLACK), a);
        }
    }

    #[test]
    fn empty_and_full_coverage_are_kept() {
        for text in [BLACK, WHITE, [0.2, 0.05, 0.6], [0.5, 0.5, 0.5]] {
            let t = TextContrast::DEFAULT;
            assert!(t.corrected_coverage(0.0, text).abs() < 1e-4, "{text:?}");
            assert!(
                (t.corrected_coverage(1.0, text) - 1.0).abs() < 1e-4,
                "{text:?}"
            );
        }
    }

    #[test]
    fn gamma_one_blends_black_on_white_as_srgb_values() {
        let t = TextContrast {
            gamma: 1.0,
            contrast: 0.0,
        };
        // Half-covered black on white: 128 in sRGB space, 188 in linear.
        assert_eq!(blended_byte(0.0, 1.0, 0.5), 188);
        let a = t.corrected_coverage(0.5, BLACK);
        let byte = blended_byte(0.0, 1.0, a);
        assert!((127..=129).contains(&byte), "got {byte}");
    }

    #[test]
    fn gamma_one_blends_white_on_black_as_srgb_values() {
        let t = TextContrast {
            gamma: 1.0,
            contrast: 0.0,
        };
        // Half-covered white on black: 128 in sRGB space, 188 in linear,
        // so the correction thins light text instead.
        let a = t.corrected_coverage(0.5, WHITE);
        let byte = blended_byte(1.0, 0.0, a);
        assert!((127..=129).contains(&byte), "got {byte}");
    }

    #[test]
    fn dark_text_edges_darken_and_light_text_edges_lighten() {
        let t = TextContrast::DEFAULT;
        for i in 1..10 {
            let a = i as f32 / 10.0;
            assert!(t.corrected_coverage(a, BLACK) > a, "dark text at {a}");
            assert!(t.corrected_coverage(a, WHITE) < a, "light text at {a}");
        }
    }

    #[test]
    fn coverage_stays_monotonic() {
        for text in [BLACK, WHITE, [0.3, 0.3, 0.3], [0.05, 0.1, 0.5]] {
            let t = TextContrast::DEFAULT;
            let mut last = -1.0;
            for i in 0..=255 {
                let a = t.corrected_coverage(i as f32 / 255.0, text);
                assert!(a >= last - 1e-6, "{text:?} at {i}: {a} < {last}");
                last = a;
            }
        }
    }

    #[test]
    fn mid_grey_text_gets_the_contrast_term_only() {
        // sRGB-encoded luminance 0.5: the guessed page is the same, so no
        // gamma correction can be computed.
        let grey = srgb_decode(0.5);
        let t = TextContrast {
            gamma: 1.0,
            contrast: 0.0,
        };
        let a = t.corrected_coverage(0.3, [grey; 3]);
        assert!((a - 0.3).abs() < 1e-3, "got {a}");
    }

    #[test]
    fn packing_round_trips_within_a_step() {
        let t = TextContrast {
            gamma: 1.37,
            contrast: 0.42,
        };
        let (g, c) = TextContrast::unpacked(t.packed()).unwrap();
        assert!((g - 1.37).abs() < 1e-4 && (c - 0.42).abs() < 1e-4);
        assert_eq!(TextContrast::OFF.packed(), 0);
        assert_ne!(
            TextContrast {
                gamma: 1e-9,
                contrast: 0.0
            }
            .packed(),
            0
        );
    }
}
