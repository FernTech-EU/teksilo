//! IntUI preset — JetBrains-flavored starting-point design language.
//!
//! Surfaces are role-differentiated (not elevation-based). 1 dp universal
//! borders. Focus rings drawn outside the control with a 2 dp gap.
//! Tooltip background is dark in both light and dark variants
//! (intentional Int UI house style).
//!
//! These constructors aggregate raw token data from `bastyde-tokens` with
//! the [`Theme`](crate::styles::Theme) struct. `ComponentStyles` was
//! removed; every widget now reads `pub const`s on its
//! `recipe_<widget>_style` (themable) or owning widget module
//! (group-4 composites). Theme-wide style installs go through
//! `style_slots: ComponentStyleSlots`.

use bastyde_tokens::{ColorTokens, LayoutTokens, MotionTokens, ShapeTokens, TypographyTokens};

use crate::styles::{ComponentStyleSlots, Theme, ThemeAppearance, ThemeExtensions};

/// IntUI light theme.
pub fn light() -> Theme {
    Theme {
        appearance: ThemeAppearance::Light,
        colors: ColorTokens::light_default(),
        layout: LayoutTokens::default(),
        typography: TypographyTokens::default(),
        shape: ShapeTokens::light_default(),
        motion: MotionTokens::default(),
        style_slots: ComponentStyleSlots::default(),
        extensions: ThemeExtensions::new(),
    }
}

/// IntUI dark theme. Shadows use ~4× stronger alphas than the light
/// variant (Int UI v2 §3) so they read against dark surfaces.
pub fn dark() -> Theme {
    Theme {
        appearance: ThemeAppearance::Dark,
        colors: ColorTokens::dark_default(),
        layout: LayoutTokens::default(),
        typography: TypographyTokens::default(),
        shape: ShapeTokens::dark_default(),
        motion: MotionTokens::default(),
        style_slots: ComponentStyleSlots::default(),
        extensions: ThemeExtensions::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_reports_light_appearance() {
        assert_eq!(light().appearance, ThemeAppearance::Light);
        assert!(light().appearance.is_light());
        assert!(!light().is_dark());
    }

    #[test]
    fn dark_reports_dark_appearance() {
        assert_eq!(dark().appearance, ThemeAppearance::Dark);
        assert!(dark().appearance.is_dark());
        assert!(dark().is_dark());
    }

    #[test]
    fn light_and_dark_have_distinct_surfaces() {
        assert_ne!(light().colors.surface_main, dark().colors.surface_main);
    }

    #[test]
    fn dark_shadows_are_stronger_than_light() {
        // Int UI v2 §3 — dark theme shadows use higher alphas.
        let l = light();
        let d = dark();
        assert!(d.shape.shadow_lg.color.a() > l.shape.shadow_lg.color.a());
    }
}
