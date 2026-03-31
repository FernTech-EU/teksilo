use serde::{Deserialize, Serialize};

/// Horizontal alignment — respects LayoutDirection for Leading/Trailing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HAlignment {
    /// Left in LTR, right in RTL.
    Leading,
    Center,
    /// Right in LTR, left in RTL.
    Trailing,
}

impl Default for HAlignment {
    fn default() -> Self {
        Self::Center
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VAlignment {
    Top,
    Center,
    Bottom,
}

impl Default for VAlignment {
    fn default() -> Self {
        Self::Center
    }
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
}
