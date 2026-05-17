use serde::{Deserialize, Serialize};

/// Horizontal alignment — respects LayoutDirection for Leading/Trailing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HAlignment {
    /// Left in LTR, right in RTL.
    Leading,
    #[default]
    Center,
    /// Right in LTR, left in RTL.
    Trailing,
}

impl HAlignment {
    /// Resolve to an x-offset given child width, container width, and whether
    /// the layout direction is right-to-left.
    pub fn resolve(self, child_width: f32, container_width: f32, rtl: bool) -> f32 {
        match self {
            Self::Leading => {
                if rtl {
                    container_width - child_width
                } else {
                    0.0
                }
            }
            Self::Center => (container_width - child_width) / 2.0,
            Self::Trailing => {
                if rtl {
                    0.0
                } else {
                    container_width - child_width
                }
            }
        }
    }
}

/// Vertical alignment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

impl VAlignment {
    /// Resolve to a y-offset given child height and container height.
    pub fn resolve(self, child_height: f32, container_height: f32) -> f32 {
        match self {
            Self::Top => 0.0,
            Self::Center => (container_height - child_height) / 2.0,
            Self::Bottom => container_height - child_height,
        }
    }
}

/// A viewport corner — used by overlay placement (toast notifications,
/// floating action buttons) and any UI that anchors to a corner of the
/// containing surface. The leading/trailing axis respects
/// `LayoutDirection`: `TopTrailing` is top-right in LTR and top-left
/// in RTL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Corner {
    TopLeading,
    TopTrailing,
    BottomLeading,
    BottomTrailing,
}

impl Corner {
    /// Decompose into independent `HAlignment` + `VAlignment` so the
    /// caller can reuse their existing axis-resolution paths.
    pub fn axes(self) -> (HAlignment, VAlignment) {
        match self {
            Self::TopLeading => (HAlignment::Leading, VAlignment::Top),
            Self::TopTrailing => (HAlignment::Trailing, VAlignment::Top),
            Self::BottomLeading => (HAlignment::Leading, VAlignment::Bottom),
            Self::BottomTrailing => (HAlignment::Trailing, VAlignment::Bottom),
        }
    }

    /// Resolve to the top-left corner of a `content_size`-sized rect
    /// anchored to `self` inside a `viewport`-sized area, with the
    /// content inset by `margin` from both axes of the chosen corner.
    ///
    /// The leading/trailing axis flips for RTL: in LTR, `TopTrailing`
    /// places the content at the top-right with the margin measured
    /// from the right edge; in RTL, it places it at the top-left with
    /// the margin from the left edge. The vertical axis is independent
    /// of direction.
    pub fn resolve(
        self,
        content_size: (f32, f32),
        viewport: (f32, f32),
        margin: (f32, f32),
        rtl: bool,
    ) -> (f32, f32) {
        let (cw, ch) = content_size;
        let (vw, vh) = viewport;
        let (mx, my) = margin;
        let (h, v) = self.axes();
        // Insetting is symmetric: shrink the available area by 2 * margin
        // on each axis, resolve alignment inside the shrunk box, then
        // translate by margin to bring it back. Equivalent to "stick to
        // the chosen edge then back off by margin" in one expression.
        let inner_w = (vw - 2.0 * mx).max(0.0);
        let inner_h = (vh - 2.0 * my).max(0.0);
        let x = mx + h.resolve(cw, inner_w, rtl);
        let y = my + v.resolve(ch, inner_h);
        (x, y)
    }
}

/// Combined two-axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Alignment {
    pub horizontal: HAlignment,
    pub vertical: VAlignment,
}

impl Default for Alignment {
    fn default() -> Self {
        Self::CENTER
    }
}

impl Alignment {
    pub const CENTER: Self = Self {
        horizontal: HAlignment::Center,
        vertical: VAlignment::Center,
    };
    pub const TOP_LEADING: Self = Self {
        horizontal: HAlignment::Leading,
        vertical: VAlignment::Top,
    };
    pub const TOP_CENTER: Self = Self {
        horizontal: HAlignment::Center,
        vertical: VAlignment::Top,
    };
    pub const TOP_TRAILING: Self = Self {
        horizontal: HAlignment::Trailing,
        vertical: VAlignment::Top,
    };
    pub const CENTER_LEADING: Self = Self {
        horizontal: HAlignment::Leading,
        vertical: VAlignment::Center,
    };
    pub const CENTER_TRAILING: Self = Self {
        horizontal: HAlignment::Trailing,
        vertical: VAlignment::Center,
    };
    pub const BOTTOM_LEADING: Self = Self {
        horizontal: HAlignment::Leading,
        vertical: VAlignment::Bottom,
    };
    pub const BOTTOM_CENTER: Self = Self {
        horizontal: HAlignment::Center,
        vertical: VAlignment::Bottom,
    };
    pub const BOTTOM_TRAILING: Self = Self {
        horizontal: HAlignment::Trailing,
        vertical: VAlignment::Bottom,
    };

    /// Resolve to (x_offset, y_offset) given child and container dimensions.
    pub fn resolve(
        self,
        child_size: (f32, f32),
        container_size: (f32, f32),
        rtl: bool,
    ) -> (f32, f32) {
        let x = self.horizontal.resolve(child_size.0, container_size.0, rtl);
        let y = self.vertical.resolve(child_size.1, container_size.1);
        (x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- HAlignment ---

    #[test]
    fn halign_leading_ltr() {
        assert_eq!(HAlignment::Leading.resolve(40.0, 100.0, false), 0.0);
    }

    #[test]
    fn halign_leading_rtl() {
        assert_eq!(HAlignment::Leading.resolve(40.0, 100.0, true), 60.0);
    }

    #[test]
    fn halign_center() {
        assert_eq!(HAlignment::Center.resolve(40.0, 100.0, false), 30.0);
        assert_eq!(HAlignment::Center.resolve(40.0, 100.0, true), 30.0);
    }

    #[test]
    fn halign_trailing_ltr() {
        assert_eq!(HAlignment::Trailing.resolve(40.0, 100.0, false), 60.0);
    }

    #[test]
    fn halign_trailing_rtl() {
        assert_eq!(HAlignment::Trailing.resolve(40.0, 100.0, true), 0.0);
    }

    // --- VAlignment ---

    #[test]
    fn valign_top() {
        assert_eq!(VAlignment::Top.resolve(30.0, 100.0), 0.0);
    }

    #[test]
    fn valign_center() {
        assert_eq!(VAlignment::Center.resolve(30.0, 100.0), 35.0);
    }

    #[test]
    fn valign_bottom() {
        assert_eq!(VAlignment::Bottom.resolve(30.0, 100.0), 70.0);
    }

    // --- Alignment ---

    #[test]
    fn alignment_center() {
        let (x, y) = Alignment::CENTER.resolve((40.0, 30.0), (100.0, 100.0), false);
        assert_eq!(x, 30.0);
        assert_eq!(y, 35.0);
    }

    #[test]
    fn alignment_top_leading_ltr() {
        let (x, y) = Alignment::TOP_LEADING.resolve((40.0, 30.0), (100.0, 100.0), false);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn alignment_bottom_trailing_rtl() {
        let (x, y) = Alignment::BOTTOM_TRAILING.resolve((40.0, 30.0), (100.0, 100.0), true);
        assert_eq!(x, 0.0); // Trailing in RTL = left = 0
        assert_eq!(y, 70.0);
    }

    #[test]
    fn all_nine_constants_exist() {
        let constants = [
            Alignment::CENTER,
            Alignment::TOP_LEADING,
            Alignment::TOP_CENTER,
            Alignment::TOP_TRAILING,
            Alignment::CENTER_LEADING,
            Alignment::CENTER_TRAILING,
            Alignment::BOTTOM_LEADING,
            Alignment::BOTTOM_CENTER,
            Alignment::BOTTOM_TRAILING,
        ];
        // Each is unique
        for (i, a) in constants.iter().enumerate() {
            for (j, b) in constants.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "constants {i} and {j} are identical");
                }
            }
        }
    }

    #[test]
    fn default_is_center() {
        assert_eq!(Alignment::default(), Alignment::CENTER);
        assert_eq!(HAlignment::default(), HAlignment::Center);
        assert_eq!(VAlignment::default(), VAlignment::Center);
    }

    #[test]
    fn child_larger_than_container() {
        // When child is larger, offsets go negative — that's fine
        let x = HAlignment::Center.resolve(200.0, 100.0, false);
        assert_eq!(x, -50.0);
    }

    // --- Corner ---

    #[test]
    fn corner_axes_decomposition() {
        assert_eq!(
            Corner::TopLeading.axes(),
            (HAlignment::Leading, VAlignment::Top)
        );
        assert_eq!(
            Corner::TopTrailing.axes(),
            (HAlignment::Trailing, VAlignment::Top)
        );
        assert_eq!(
            Corner::BottomLeading.axes(),
            (HAlignment::Leading, VAlignment::Bottom)
        );
        assert_eq!(
            Corner::BottomTrailing.axes(),
            (HAlignment::Trailing, VAlignment::Bottom)
        );
    }

    #[test]
    fn corner_resolve_top_leading_ltr() {
        // 380x100 content in 800x600 viewport with margin (24, 24).
        // TopLeading in LTR: x = 24, y = 24.
        let (x, y) =
            Corner::TopLeading.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), false);
        assert_eq!(x, 24.0);
        assert_eq!(y, 24.0);
    }

    #[test]
    fn corner_resolve_top_trailing_ltr() {
        // TopTrailing in LTR: right-anchored, x = 800 - 380 - 24 = 396, y = 24.
        let (x, y) =
            Corner::TopTrailing.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), false);
        assert_eq!(x, 396.0);
        assert_eq!(y, 24.0);
    }

    #[test]
    fn corner_resolve_bottom_leading_ltr() {
        // BottomLeading: x = 24, y = 600 - 100 - 24 = 476.
        let (x, y) =
            Corner::BottomLeading.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), false);
        assert_eq!(x, 24.0);
        assert_eq!(y, 476.0);
    }

    #[test]
    fn corner_resolve_bottom_trailing_ltr() {
        // BottomTrailing: x = 396, y = 476.
        let (x, y) =
            Corner::BottomTrailing.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), false);
        assert_eq!(x, 396.0);
        assert_eq!(y, 476.0);
    }

    #[test]
    fn corner_resolve_top_trailing_rtl_flips_to_left() {
        // RTL: TopTrailing becomes top-left visually. x = 24, y = 24.
        let (x, y) =
            Corner::TopTrailing.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), true);
        assert_eq!(x, 24.0);
        assert_eq!(y, 24.0);
    }

    #[test]
    fn corner_resolve_top_leading_rtl_flips_to_right() {
        // RTL: TopLeading becomes top-right visually. x = 396, y = 24.
        let (x, y) = Corner::TopLeading.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), true);
        assert_eq!(x, 396.0);
        assert_eq!(y, 24.0);
    }

    #[test]
    fn corner_resolve_bottom_corners_rtl() {
        let (x, _) =
            Corner::BottomTrailing.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), true);
        assert_eq!(x, 24.0, "BottomTrailing flips to bottom-left under RTL");
        let (x, _) =
            Corner::BottomLeading.resolve((380.0, 100.0), (800.0, 600.0), (24.0, 24.0), true);
        assert_eq!(x, 396.0, "BottomLeading flips to bottom-right under RTL");
    }

    #[test]
    fn corner_resolve_zero_margin() {
        let (x, y) =
            Corner::BottomTrailing.resolve((100.0, 100.0), (800.0, 600.0), (0.0, 0.0), false);
        assert_eq!((x, y), (700.0, 500.0));
    }

    #[test]
    fn corner_resolve_oversized_content_clamps_to_zero_inner_area() {
        // If 2 * margin exceeds viewport, the inner area goes to zero
        // and the content snaps to the margin offset (not negative).
        let (x, y) = Corner::TopLeading.resolve((10.0, 10.0), (40.0, 40.0), (30.0, 30.0), false);
        // inner_w = max(40 - 60, 0) = 0, so HAlignment::Leading.resolve(10, 0, false) = 0
        // x = 30 + 0 = 30; same for y.
        assert_eq!((x, y), (30.0, 30.0));
    }
}
