use serde::{Deserialize, Serialize};

use crate::color::Color;

/// Corner radius for rounded rectangles. Each corner can have a different radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CornerRadius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_left: f32,
    pub bottom_right: f32,
}

impl CornerRadius {
    pub const ZERO: CornerRadius = CornerRadius {
        top_left: 0.0,
        top_right: 0.0,
        bottom_left: 0.0,
        bottom_right: 0.0,
    };

    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    /// Clamp all corner radii so they don't exceed half the rect dimension.
    /// Without this, the SDF shader produces incorrect results for large radii
    /// (e.g. `radius_full = 9999`) on small rectangles.
    pub fn clamped(self, width: f32, height: f32) -> Self {
        let max_r = (width.min(height) * 0.5).max(0.0);
        Self {
            top_left: self.top_left.min(max_r),
            top_right: self.top_right.min(max_r),
            bottom_left: self.bottom_left.min(max_r),
            bottom_right: self.bottom_right.min(max_r),
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }
}

impl Default for CornerRadius {
    fn default() -> Self {
        Self::ZERO
    }
}

/// A shadow definition.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 4.0,
            spread: 0.0,
            color: Color::new(0.0, 0.0, 0.0, 0.15),
        }
    }
}

/// Shape tokens: corner radii, border widths, and shadows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeTokens {
    pub radius_sm: f32,
    pub radius_md: f32,
    pub radius_lg: f32,
    pub radius_full: f32,
    pub border_width: f32,
    pub border_width_strong: f32,
    pub shadow_sm: Shadow,
    pub shadow_md: Shadow,
    pub shadow_lg: Shadow,
}

impl Default for ShapeTokens {
    fn default() -> Self {
        Self {
            radius_sm: 4.0,
            radius_md: 8.0,
            radius_lg: 16.0,
            radius_full: 9999.0,
            border_width: 1.0,
            border_width_strong: 2.0,
            shadow_sm: Shadow {
                offset_y: 1.0,
                blur: 2.0,
                ..Shadow::default()
            },
            shadow_md: Shadow::default(),
            shadow_lg: Shadow {
                offset_y: 4.0,
                blur: 8.0,
                ..Shadow::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_radius_uniform() {
        let r = CornerRadius::uniform(6.0);
        assert_eq!(r.top_left, 6.0);
        assert_eq!(r.top_right, 6.0);
        assert_eq!(r.bottom_left, 6.0);
        assert_eq!(r.bottom_right, 6.0);
    }

    #[test]
    fn corner_radius_to_array() {
        let r = CornerRadius::uniform(6.0);
        assert_eq!(r.to_array(), [6.0, 6.0, 6.0, 6.0]);
    }

    #[test]
    fn corner_radius_zero() {
        assert_eq!(CornerRadius::ZERO.top_left, 0.0);
    }

    #[test]
    fn shape_tokens_radius_scale() {
        let s = ShapeTokens::default();
        assert!(s.radius_sm < s.radius_md);
        assert!(s.radius_md < s.radius_lg);
    }
}
