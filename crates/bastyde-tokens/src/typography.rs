// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use serde::{Deserialize, Serialize};

use crate::text_style::{FontWeight, TextStyle};

/// Typography tokens — Int UI text styles.
///
/// Int UI uses Inter for UI text and JetBrains Mono for code. Letter spacing
/// is 0 everywhere — Int UI never tracks text. There are no heading styles:
/// section headers are `body_bold` with extra spacing above/below.
///
/// IntelliJ lets users override the UI font size globally; the 13 sp default
/// here should ideally be a setting, not a constant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypographyTokens {
    /// Default UI text — button labels, field text, body copy.
    pub body: TextStyle,
    /// Section headers — the closest thing Int UI has to a heading.
    pub body_bold: TextStyle,
    /// Secondary info, captions, hints.
    pub small: TextStyle,
    /// Small emphasized labels.
    pub small_bold: TextStyle,
    /// Status bar, tag labels, timestamps.
    pub tiny: TextStyle,
    /// Code, paths, identifiers.
    pub mono: TextStyle,
}

impl TypographyTokens {
    /// Return a copy with every [`TextStyle::size`] multiplied by `factor`.
    ///
    /// Used by the global user text-scale accessibility feature: the
    /// `WidgetTree` derives a scaled typography bag from the active theme so
    /// every text widget grows uniformly. `factor` is clamped to `[0.25, 8.0]`
    /// to guard against absurd inputs, and each resulting size is floored at
    /// `1.0` pt so rounding toward zero never produces invisible text. Font
    /// weight, family, line height, and letter spacing are preserved.
    pub fn scaled(&self, factor: f32) -> Self {
        let f = factor.clamp(0.25, 8.0);
        let scale = |s: &TextStyle| TextStyle {
            size: (s.size * f).max(1.0),
            ..s.clone()
        };
        Self {
            body: scale(&self.body),
            body_bold: scale(&self.body_bold),
            small: scale(&self.small),
            small_bold: scale(&self.small_bold),
            tiny: scale(&self.tiny),
            mono: scale(&self.mono),
        }
    }
}

impl Default for TypographyTokens {
    fn default() -> Self {
        let family = "Inter".to_string();
        let mono_family = "JetBrains Mono".to_string();

        Self {
            body: TextStyle {
                family: family.clone(),
                size: 13.0,
                weight: FontWeight::REGULAR,
                line_height: 1.4,
                letter_spacing: 0.0,
            },
            body_bold: TextStyle {
                family: family.clone(),
                size: 13.0,
                weight: FontWeight::SEMI_BOLD,
                line_height: 1.4,
                letter_spacing: 0.0,
            },
            small: TextStyle {
                family: family.clone(),
                size: 12.0,
                weight: FontWeight::REGULAR,
                line_height: 1.35,
                letter_spacing: 0.0,
            },
            small_bold: TextStyle {
                family: family.clone(),
                size: 12.0,
                weight: FontWeight::SEMI_BOLD,
                line_height: 1.35,
                letter_spacing: 0.0,
            },
            tiny: TextStyle {
                family,
                size: 11.0,
                weight: FontWeight::REGULAR,
                line_height: 1.3,
                letter_spacing: 0.0,
            },
            mono: TextStyle {
                family: mono_family,
                size: 13.0,
                weight: FontWeight::REGULAR,
                line_height: 1.5,
                letter_spacing: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_body_bold_share_size_differ_in_weight() {
        let t = TypographyTokens::default();
        assert_eq!(t.body.size, t.body_bold.size);
        assert!(t.body_bold.weight.0 > t.body.weight.0);
    }

    #[test]
    fn body_larger_than_tiny() {
        let t = TypographyTokens::default();
        assert!(t.body.size > t.tiny.size);
    }

    #[test]
    fn scaled_identity_preserves_sizes_and_weights() {
        let t = TypographyTokens::default();
        let s = t.scaled(1.0);
        assert_eq!(s.body.size, t.body.size);
        assert_eq!(s.small.size, t.small.size);
        assert_eq!(s.tiny.size, t.tiny.size);
        assert_eq!(s.mono.size, t.mono.size);
        assert_eq!(s.body_bold.weight, t.body_bold.weight);
        assert_eq!(s.mono.family, t.mono.family);
    }

    #[test]
    fn scaled_doubles_every_size() {
        let t = TypographyTokens::default();
        let s = t.scaled(2.0);
        for (scaled, base) in [
            (&s.body, &t.body),
            (&s.body_bold, &t.body_bold),
            (&s.small, &t.small),
            (&s.small_bold, &t.small_bold),
            (&s.tiny, &t.tiny),
            (&s.mono, &t.mono),
        ] {
            assert!((scaled.size - base.size * 2.0).abs() < 0.001);
            assert_eq!(scaled.weight, base.weight);
        }
    }

    #[test]
    fn scaled_clamps_extremes_and_floors_at_one() {
        let t = TypographyTokens::default();
        // Below the clamp floor: factor clamps to 0.25 but size never < 1.0.
        let small = t.scaled(0.0);
        assert!(small.body.size >= 1.0);
        assert!((small.body.size - (t.body.size * 0.25).max(1.0)).abs() < 0.001);
        // Above the clamp ceiling: factor clamps to 8.0.
        let large = t.scaled(100.0);
        assert!((large.body.size - t.body.size * 8.0).abs() < 0.001);
    }

    #[test]
    fn letter_spacing_is_zero_everywhere() {
        let t = TypographyTokens::default();
        for s in [
            &t.body,
            &t.body_bold,
            &t.small,
            &t.small_bold,
            &t.tiny,
            &t.mono,
        ] {
            assert_eq!(s.letter_spacing, 0.0);
        }
    }
}
