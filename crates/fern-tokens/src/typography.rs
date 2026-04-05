use serde::{Deserialize, Serialize};

use crate::text_style::{FontWeight, TextStyle};

/// Typography tokens: named text styles for consistent text rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypographyTokens {
    pub body: TextStyle,
    pub body_small: TextStyle,
    pub heading_1: TextStyle,
    pub heading_2: TextStyle,
    pub heading_3: TextStyle,
    pub label: TextStyle,
    pub caption: TextStyle,
    pub monospace: TextStyle,
}

impl Default for TypographyTokens {
    fn default() -> Self {
        let family = "sans-serif".to_string();
        let mono_family = "monospace".to_string();

        Self {
            body: TextStyle {
                family: family.clone(),
                size: 14.0,
                weight: FontWeight::REGULAR,
                line_height: 1.4,
                letter_spacing: 0.0,
            },
            body_small: TextStyle {
                family: family.clone(),
                size: 12.0,
                weight: FontWeight::REGULAR,
                line_height: 1.4,
                letter_spacing: 0.0,
            },
            heading_1: TextStyle {
                family: family.clone(),
                size: 28.0,
                weight: FontWeight::BOLD,
                line_height: 1.2,
                letter_spacing: -0.5,
            },
            heading_2: TextStyle {
                family: family.clone(),
                size: 22.0,
                weight: FontWeight::SEMI_BOLD,
                line_height: 1.3,
                letter_spacing: -0.25,
            },
            heading_3: TextStyle {
                family: family.clone(),
                size: 18.0,
                weight: FontWeight::SEMI_BOLD,
                line_height: 1.3,
                letter_spacing: 0.0,
            },
            label: TextStyle {
                family: family.clone(),
                size: 12.0,
                weight: FontWeight::MEDIUM,
                line_height: 1.2,
                letter_spacing: 0.5,
            },
            caption: TextStyle {
                family,
                size: 11.0,
                weight: FontWeight::REGULAR,
                line_height: 1.3,
                letter_spacing: 0.25,
            },
            monospace: TextStyle {
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
    fn heading_sizes_decrease() {
        let t = TypographyTokens::default();
        assert!(t.heading_1.size > t.heading_2.size);
        assert!(t.heading_2.size > t.heading_3.size);
    }

    #[test]
    fn body_larger_than_caption() {
        let t = TypographyTokens::default();
        assert!(t.body.size > t.caption.size);
    }
}
