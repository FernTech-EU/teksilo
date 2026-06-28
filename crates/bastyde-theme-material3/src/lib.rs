// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 theme preset for Bastyde.
//!
//! A static Material 3 theme built on the canonical baseline scheme
//! (seed `#6750A4`, the M3 reference purple). The preset replaces the
//! IntUI color, shape, and typography tokens with M3 values, exposes the
//! full M3 role palette via [`Material3Palette`], and installs M3 widget
//! chrome (pill-shaped 40 dp buttons, the M3 switch, 12 dp cards) through
//! the Tier-3 style slots.
//!
//! ```ignore
//! use bastyde_theme_material3 as m3;
//!
//! BastydeAppBuilder::new()
//!     .theme(m3::light())
//!     .initial_window(WindowConfig::new().title("M3 demo"))
//!     .run();
//! ```
//!
//! ## What's mapped, and what isn't
//!
//! M3's primary/secondary/tertiary + tonal-surface model doesn't line up
//! one-to-one with Bastyde's role taxonomy, so the color mapping projects
//! each `ColorTokens` field onto its nearest M3 equivalent and exposes the
//! rest (containers, secondary, tertiary, the surface-container ladder)
//! through [`Material3Palette`]. Typography keeps Bastyde's bundled
//! **Inter** family (which is metrically Roboto-compatible; the text
//! engine would silently fall back to it for an unregistered "Roboto"
//! anyway) and adopts M3's type-scale sizes / weights / letter-spacing.
//!
//! ### Known limitations
//!
//! - **State layers are pre-composited.** M3 hover/pressed are 8 % / 12 %
//!   on-color overlays; Bastyde recipes take opaque colors, so those
//!   layers are baked into static colors.
//! - **Tonal / Destructive button fills** are frozen per appearance
//!   (`RecipeColor::Static`) because there is no secondaryContainer /
//!   error *surface* role to track reactively.

mod color;
mod palette;
mod shape;
mod styles;
mod typography;

pub use palette::Material3Palette;

use std::rc::Rc;

use bastyde_core::presets::intui;
use bastyde_core::styles::{Theme, ThemeAppearance};

/// Material 3 light theme (baseline scheme, seed `#6750A4`).
pub fn light() -> Theme {
    let mut theme = intui::light().with_id("material3.light");
    apply_material3_overrides(&mut theme, ThemeAppearance::Light);
    theme
}

/// Material 3 dark theme (baseline scheme, seed `#6750A4`).
pub fn dark() -> Theme {
    let mut theme = intui::dark().with_id("material3.dark");
    apply_material3_overrides(&mut theme, ThemeAppearance::Dark);
    theme
}

fn apply_material3_overrides(theme: &mut Theme, appearance: ThemeAppearance) {
    let light = appearance == ThemeAppearance::Light;

    theme.colors = if light {
        color::m3_light_colors()
    } else {
        color::m3_dark_colors()
    };
    theme.shape = if light {
        shape::m3_light_shape()
    } else {
        shape::m3_dark_shape()
    };
    theme.typography = typography::m3_typography();
    theme.extensions.insert(if light {
        Material3Palette::light()
    } else {
        Material3Palette::dark()
    });

    // Tier-3 widget chrome. These read M3-mapped roles, so a single
    // install serves both appearances.
    theme.style_slots.button = Some(Rc::new(styles::button::m3_button_style()));
    theme.style_slots.toggle = Some(Rc::new(styles::toggle::M3ToggleStyle));
    theme.style_slots.card = Some(Rc::new(styles::card::M3CardStyle));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_tokens::Color;

    #[test]
    fn light_reports_light_appearance() {
        assert_eq!(light().appearance, ThemeAppearance::Light);
        assert!(!light().is_dark());
    }

    #[test]
    fn dark_reports_dark_appearance() {
        assert_eq!(dark().appearance, ThemeAppearance::Dark);
        assert!(dark().is_dark());
    }

    #[test]
    fn ids_are_namespaced() {
        assert_eq!(light().id.as_str(), "material3.light");
        assert_eq!(dark().id.as_str(), "material3.dark");
    }

    #[test]
    fn light_and_dark_have_distinct_surfaces() {
        assert_ne!(light().colors.surface_main, dark().colors.surface_main);
    }

    #[test]
    fn accent_is_m3_primary() {
        assert_eq!(light().colors.accent, Color::from_hex("#6750A4"));
        assert_eq!(dark().colors.accent, Color::from_hex("#D0BCFF"));
    }

    #[test]
    fn body_is_m3_body_medium_size() {
        // M3 Body Medium is 14 sp (IntUI's is 13).
        assert!((light().typography.body.size - 14.0).abs() < 0.01);
    }

    #[test]
    fn typography_tracks_letters() {
        // M3 specifies non-zero letter-spacing (IntUI never tracks).
        assert!(light().typography.body.letter_spacing > 0.0);
    }

    #[test]
    fn palette_extension_present() {
        assert!(light().extension::<Material3Palette>().is_some());
        assert!(dark().extension::<Material3Palette>().is_some());
        // The extension carries roles ColorTokens can't hold (e.g. tertiary).
        let p = dark().extension::<Material3Palette>().copied().unwrap();
        assert_eq!(p.tertiary, Color::from_hex("#EFB8C8"));
    }

    #[test]
    fn widget_style_slots_installed() {
        for t in [light(), dark()] {
            assert!(t.style_slots.button.is_some(), "button slot");
            assert!(t.style_slots.toggle.is_some(), "toggle slot");
            assert!(t.style_slots.card.is_some(), "card slot");
        }
    }

    #[test]
    fn dark_elevation_is_stronger_than_light() {
        // M3 softens shadows, but dark still needs more alpha than light.
        assert!(dark().shape.shadow_lg.color.a() > light().shape.shadow_lg.color.a());
    }
}
