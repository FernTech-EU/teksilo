// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS typography — the AppKit text-style ramp mapped onto Teksilo's six
//! [`TextStyle`](teksilo_tokens::TextStyle) slots.
//!
//! This is the one part of the macOS design language Apple publishes
//! completely: the text-style table (size, line height, weight, and the
//! emphasised weight) and a per-point tracking table are both in the
//! Human Interface Guidelines, exact.
//!
//! | Teksilo slot | macOS text style | Size / line height / weight | Tracking |
//! | --- | --- | --- | --- |
//! | `body` | Body | 13 / 16 / Regular | −0.08 |
//! | `body_bold` | Body, Emphasized | 13 / 16 / Semibold | −0.08 |
//! | `small` | Callout | 12 / 15 / Regular | 0.00 |
//! | `small_bold` | Callout, Emphasized | 12 / 15 / Semibold | 0.00 |
//! | `tiny` | Subheadline | 11 / 14 / Regular | +0.06 |
//! | `mono` | SF Mono | — | — |
//!
//! Every row is transcribed, not interpolated — including the fact that
//! macOS's default UI size is **13 pt**, a full point smaller than
//! Fluent's 14 and a point below Material 3's 14 sp. Combined with the
//! 22 dp control height in [`crate::shape`], that density is a large part
//! of why a macOS window fits more in the same space.
//!
//! ## Tracking is the interesting part
//!
//! macOS is the only one of the three design languages Teksilo ships that
//! tracks text **non-uniformly and with a sign change**. Apple publishes a
//! tracking value for every integer point size from 6 to 96; the region
//! this ramp lives in reads:
//!
//! | pt | 10 | 11 | 12 | 13 | 15 | 17 |
//! | --- | --- | --- | --- | --- | --- | --- |
//! | tracking (pt) | +0.12 | +0.06 | 0.00 | −0.08 | −0.23 | −0.43 |
//!
//! Small text is *loosened* so it stays legible; larger text is
//! *tightened* so it does not read as gappy; 12 pt is the crossover. That
//! is the opposite of Material 3 (which tracks positively at every rung)
//! and different again from Fluent and IntUI (which track nothing at all,
//! and get their optical compensation from a variable font's `opsz` axis
//! instead). It is a small effect per glyph and a very visible one across
//! a paragraph.
//!
//! ## Font family
//!
//! macOS's UI face is **San Francisco** (SF Pro Text below ~20 pt, SF Pro
//! Display above; since Big Sur one variable font with continuous optical
//! sizing), and its monospace is **SF Mono**. Neither can be bundled:
//! Apple licenses both for use *on* Apple platforms only, with no
//! redistribution rights inside a cross-platform binary. So the default
//! build keeps Teksilo's bundled Inter / JetBrains Mono, and the
//! `system-fonts` feature switches the family *names* to the macOS stack
//! for the text engine to resolve from installed system fonts. On a
//! machine without them the engine falls back to the bundled default, so
//! the feature is never fatal — it simply does nothing off macOS.
//!
//! Inter is a defensible stand-in: it was drawn to the same brief as SF
//! (a neutral grotesque for screen UI at small sizes) and its metrics are
//! close enough that the tracking values above still land correctly.

use teksilo_tokens::{FontWeight, TypographyTokens};

/// The macOS **Body** size — and the system default UI size.
pub const MACOS_BODY_SIZE: f32 = 13.0;
/// The macOS **Callout** size.
pub const MACOS_CALLOUT_SIZE: f32 = 12.0;
/// The macOS **Subheadline** size.
pub const MACOS_SUBHEADLINE_SIZE: f32 = 11.0;

/// Published tracking at 13 pt.
pub const MACOS_TRACKING_13: f32 = -0.08;
/// Published tracking at 12 pt — the crossover point, exactly zero.
pub const MACOS_TRACKING_12: f32 = 0.0;
/// Published tracking at 11 pt.
pub const MACOS_TRACKING_11: f32 = 0.06;

/// The macOS UI face, used under the `system-fonts` feature.
#[cfg(feature = "system-fonts")]
pub const MACOS_UI_FONT_FAMILY: &str = "SF Pro Text";
/// The macOS monospace face, used under the `system-fonts` feature.
#[cfg(feature = "system-fonts")]
pub const MACOS_MONO_FONT_FAMILY: &str = "SF Mono";

/// macOS typography tokens (AppKit text-style metrics).
pub fn macos_typography() -> TypographyTokens {
    let mut t = TypographyTokens::default();

    // Under `system-fonts`, name the macOS faces so the text engine
    // resolves them from the installed system fonts; otherwise keep the
    // bundled Inter / JetBrains Mono.
    #[cfg(feature = "system-fonts")]
    {
        for style in [
            &mut t.body,
            &mut t.body_bold,
            &mut t.small,
            &mut t.small_bold,
            &mut t.tiny,
        ] {
            style.family = MACOS_UI_FONT_FAMILY.to_string();
        }
        t.mono.family = MACOS_MONO_FONT_FAMILY.to_string();
    }

    // Body — 13 / 16 / Regular. `line_height` is a multiplier here, so
    // AppKit's 16 pt line on a 13 pt body is 16 / 13.
    t.body.size = MACOS_BODY_SIZE;
    t.body.weight = FontWeight::REGULAR;
    t.body.line_height = 16.0 / MACOS_BODY_SIZE;
    t.body.letter_spacing = MACOS_TRACKING_13;

    // Body, Emphasized — same 13 / 16 metrics, Semibold. (Apple's
    // *Headline* style is 13 pt Bold; Semibold is the emphasised weight
    // of Body itself, which is what a section label in a Teksilo app is.)
    t.body_bold.size = MACOS_BODY_SIZE;
    t.body_bold.weight = FontWeight::SEMI_BOLD;
    t.body_bold.line_height = 16.0 / MACOS_BODY_SIZE;
    t.body_bold.letter_spacing = MACOS_TRACKING_13;

    // Callout — 12 / 15 / Regular, and the point where tracking crosses
    // zero.
    t.small.size = MACOS_CALLOUT_SIZE;
    t.small.weight = FontWeight::REGULAR;
    t.small.line_height = 15.0 / MACOS_CALLOUT_SIZE;
    t.small.letter_spacing = MACOS_TRACKING_12;

    // Callout, Emphasized — Semibold at the same size.
    t.small_bold.size = MACOS_CALLOUT_SIZE;
    t.small_bold.weight = FontWeight::SEMI_BOLD;
    t.small_bold.line_height = 15.0 / MACOS_CALLOUT_SIZE;
    t.small_bold.letter_spacing = MACOS_TRACKING_12;

    // Subheadline — 11 / 14 / Regular, for status-bar and timestamp
    // chrome. Apple's floor for macOS is 10 pt (Footnote / Caption); this
    // stays one rung above it, so the smallest text in the UI is still
    // comfortably above the minimum rather than at it.
    t.tiny.size = MACOS_SUBHEADLINE_SIZE;
    t.tiny.weight = FontWeight::REGULAR;
    t.tiny.line_height = 14.0 / MACOS_SUBHEADLINE_SIZE;
    t.tiny.letter_spacing = MACOS_TRACKING_11;

    // `mono` keeps the bundled default metrics — Apple names SF Mono as
    // the fixed-width face and publishes six weights for it, but no size
    // ramp for code.

    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_the_published_macos_body_style() {
        let t = macos_typography();
        assert!((t.body.size - 13.0).abs() < 0.01);
        assert_eq!(t.body.weight, FontWeight::REGULAR);
        // 13 / 16 → the computed line box must land on 16 pt.
        assert!((t.body.size * t.body.line_height - 16.0).abs() < 0.01);
    }

    #[test]
    fn emphasised_styles_share_their_metrics_and_only_gain_weight() {
        let t = macos_typography();
        for (regular, emphasised) in [(&t.body, &t.body_bold), (&t.small, &t.small_bold)] {
            assert_eq!(regular.size, emphasised.size);
            assert!((regular.line_height - emphasised.line_height).abs() < 1e-6);
            assert_eq!(regular.letter_spacing, emphasised.letter_spacing);
            assert!(emphasised.weight.0 > regular.weight.0);
        }
    }

    #[test]
    fn callout_is_twelve_over_fifteen() {
        let t = macos_typography();
        assert!((t.small.size - 12.0).abs() < 0.01);
        assert!((t.small.size * t.small.line_height - 15.0).abs() < 0.01);
    }

    #[test]
    fn subheadline_is_eleven_over_fourteen() {
        let t = macos_typography();
        assert!((t.tiny.size - 11.0).abs() < 0.01);
        assert!((t.tiny.size * t.tiny.line_height - 14.0).abs() < 0.01);
    }

    /// The signature typographic fact of this preset, and the one thing
    /// that most distinguishes it from every other Teksilo theme.
    #[test]
    fn tracking_is_signed_and_crosses_zero_at_twelve_point() {
        let t = macos_typography();
        assert!(t.body.letter_spacing < 0.0, "13 pt must tighten");
        assert_eq!(t.small.letter_spacing, 0.0, "12 pt is the crossover");
        assert!(t.tiny.letter_spacing > 0.0, "11 pt must loosen");
        // …and it is monotonic: as the size falls, tracking rises.
        assert!(t.body.letter_spacing < t.small.letter_spacing);
        assert!(t.small.letter_spacing < t.tiny.letter_spacing);
    }

    #[test]
    fn tracking_matches_the_published_table() {
        let t = macos_typography();
        assert!((t.body.letter_spacing - (-0.08)).abs() < 1e-6);
        assert!((t.small.letter_spacing - 0.0).abs() < 1e-6);
        assert!((t.tiny.letter_spacing - 0.06).abs() < 1e-6);
    }

    #[test]
    fn the_ramp_is_monotonic_and_denser_than_the_other_presets() {
        let t = macos_typography();
        assert!(t.body.size > t.small.size);
        assert!(t.small.size > t.tiny.size);
        // macOS's system size is 13 pt where Fluent's is 14 and M3's 14 sp.
        assert!(t.body.size < 14.0);
    }

    #[test]
    fn nothing_falls_below_apples_ten_point_floor() {
        let t = macos_typography();
        for s in [&t.body, &t.body_bold, &t.small, &t.small_bold, &t.tiny] {
            assert!(s.size >= 10.0, "{} pt is below the macOS floor", s.size);
        }
    }

    #[cfg(not(feature = "system-fonts"))]
    #[test]
    fn default_build_keeps_the_bundled_families() {
        let t = macos_typography();
        let base = TypographyTokens::default();
        assert_eq!(t.body.family, base.body.family);
        assert_eq!(t.mono.family, base.mono.family);
    }

    #[cfg(feature = "system-fonts")]
    #[test]
    fn system_fonts_feature_names_the_macos_stack() {
        let t = macos_typography();
        assert_eq!(t.body.family, MACOS_UI_FONT_FAMILY);
        assert_eq!(t.small.family, MACOS_UI_FONT_FAMILY);
        assert_eq!(t.tiny.family, MACOS_UI_FONT_FAMILY);
        assert_eq!(t.mono.family, MACOS_MONO_FONT_FAMILY);
    }
}
