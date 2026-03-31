use serde::{Deserialize, Serialize};

/// Spacing tokens providing a scale of spacing values plus semantic values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpacingTokens {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
    pub xxl: f32,
    pub widget_padding: f32,
    pub content_padding: f32,
    pub item_spacing: f32,
}

impl Default for SpacingTokens {
    fn default() -> Self {
        Self {
            xs: 2.0,
            sm: 4.0,
            md: 8.0,
            lg: 16.0,
            xl: 24.0,
            xxl: 32.0,
            widget_padding: 8.0,
            content_padding: 16.0,
            item_spacing: 8.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_default_scale_is_increasing() {
        let s = SpacingTokens::default();
        assert!(s.xs < s.sm);
        assert!(s.sm < s.md);
        assert!(s.md < s.lg);
        assert!(s.lg < s.xl);
        assert!(s.xl < s.xxl);
    }
}
