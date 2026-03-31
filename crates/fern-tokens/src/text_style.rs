use serde::{Deserialize, Serialize};

/// Font weight as a numeric value (100-900) following CSS conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const THIN: FontWeight = FontWeight(100);
    pub const EXTRA_LIGHT: FontWeight = FontWeight(200);
    pub const LIGHT: FontWeight = FontWeight(300);
    pub const REGULAR: FontWeight = FontWeight(400);
    pub const MEDIUM: FontWeight = FontWeight(500);
    pub const SEMI_BOLD: FontWeight = FontWeight(600);
    pub const BOLD: FontWeight = FontWeight(700);
    pub const EXTRA_BOLD: FontWeight = FontWeight(800);
    pub const BLACK: FontWeight = FontWeight(900);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::REGULAR
    }
}

/// A text style defining font properties for a category of text (body, heading, label, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub weight: FontWeight,
    pub line_height: f32,
    pub letter_spacing: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "sans-serif".to_string(),
            size: 14.0,
            weight: FontWeight::REGULAR,
            line_height: 1.4,
            letter_spacing: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_weight_ordering() {
        assert!(FontWeight::LIGHT.0 < FontWeight::BOLD.0);
    }

    #[test]
    fn text_style_default() {
        let style = TextStyle::default();
        assert_eq!(style.weight, FontWeight::REGULAR);
        assert!(style.size > 0.0);
    }

    #[test]
    fn text_style_serde_roundtrip() {
        let style = TextStyle {
            family: "Inter".to_string(),
            size: 16.0,
            weight: FontWeight::BOLD,
            line_height: 1.5,
            letter_spacing: 0.5,
        };
        let json = serde_json::to_string(&style).unwrap();
        let deserialized: TextStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(style, deserialized);
    }
}
