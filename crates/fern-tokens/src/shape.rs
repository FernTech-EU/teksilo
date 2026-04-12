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
    /// (e.g. `radius_pill = 9999`) on small rectangles.
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
            color: Color::new(0.0, 0.0, 0.0, 0.16),
        }
    }
}

/// Shape tokens — Int UI corner radii, border widths, focus ring, and shadows.
///
/// Int UI uses 1 dp borders universally; emphasis is color-only, never
/// thickness. Focus rings are drawn **outside** the control with a 2 dp gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeTokens {
    /// 4 dp — buttons, fields, combo boxes, checkbox visual, menu items.
    pub radius_control: f32,
    /// 8 dp — tooltips, notification balloons, large popups, dialogs, panels.
    pub radius_popup: f32,
    /// 9999 — fully rounded (tags, chips, badges).
    pub radius_pill: f32,
    /// 1 dp — universal border width. Int UI has no thicker variant.
    pub border_width: f32,
    /// 2 dp — focus ring outline width, drawn outside the control.
    pub focus_ring_width: f32,
    /// 2 dp — gap between control edge and focus ring.
    pub focus_ring_offset: f32,
    /// Tooltips.
    pub shadow_xs: Shadow,
    /// Menus, dropdowns.
    pub shadow_sm: Shadow,
    /// Notification balloons.
    pub shadow_md: Shadow,
    /// Modal dialogs.
    pub shadow_lg: Shadow,
}

impl ShapeTokens {
    /// Light-theme shadows. Alphas: xs 10%, sm 12%, md 16%, lg 20%.
    pub fn light_default() -> Self {
        Self::with_shadow_alphas(0.10, 0.12, 0.16, 0.20)
    }

    /// Dark-theme shadows — alphas roughly 4× stronger than light to remain
    /// visible against dark surfaces, per the Int UI v2 reference (Section 3).
    /// Alphas: xs 40%, sm 50%, md 60%, lg 70%.
    pub fn dark_default() -> Self {
        Self::with_shadow_alphas(0.40, 0.50, 0.60, 0.70)
    }

    fn with_shadow_alphas(a_xs: f32, a_sm: f32, a_md: f32, a_lg: f32) -> Self {
        Self {
            radius_control: 4.0,
            radius_popup: 8.0,
            radius_pill: 9999.0,
            border_width: 1.0,
            focus_ring_width: 2.0,
            focus_ring_offset: 2.0,
            shadow_xs: Shadow {
                offset_y: 1.0,
                blur: 2.0,
                color: Color::new(0.0, 0.0, 0.0, a_xs),
                ..Shadow::default()
            },
            shadow_sm: Shadow {
                offset_y: 2.0,
                blur: 6.0,
                color: Color::new(0.0, 0.0, 0.0, a_sm),
                ..Shadow::default()
            },
            shadow_md: Shadow {
                offset_y: 4.0,
                blur: 12.0,
                color: Color::new(0.0, 0.0, 0.0, a_md),
                ..Shadow::default()
            },
            shadow_lg: Shadow {
                offset_y: 8.0,
                blur: 24.0,
                color: Color::new(0.0, 0.0, 0.0, a_lg),
                ..Shadow::default()
            },
        }
    }
}

impl Default for ShapeTokens {
    fn default() -> Self {
        Self::light_default()
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
        assert!(s.radius_control < s.radius_popup);
        assert!(s.radius_popup < s.radius_pill);
    }

    #[test]
    fn focus_ring_offset_present() {
        let s = ShapeTokens::default();
        assert!(s.focus_ring_offset > 0.0);
        assert!(s.focus_ring_width > 0.0);
    }
}
