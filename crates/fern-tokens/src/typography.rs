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
    fn letter_spacing_is_zero_everywhere() {
        let t = TypographyTokens::default();
        for s in [&t.body, &t.body_bold, &t.small, &t.small_bold, &t.tiny, &t.mono] {
            assert_eq!(s.letter_spacing, 0.0);
        }
    }
}
