// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 theme preset for Teksilo.
//!
//! A static Material 3 theme built on the canonical baseline scheme
//! (seed `#6750A4`, the M3 reference purple). The preset replaces the
//! IntUI color, shape, and typography tokens with M3 values, exposes the
//! full M3 role palette via [`Material3Palette`], and installs M3 widget
//! chrome (pill-shaped 40 dp buttons, the M3 switch, 12 dp cards) through
//! the Tier-3 style slots.
//!
//! ```ignore
//! use teksilo_theme_material3 as m3;
//!
//! TeksiloAppBuilder::new()
//!     .theme(m3::light())
//!     .initial_window(WindowConfig::new().title("M3 demo"))
//!     .run();
//! ```
//!
//! ## What's mapped, and what isn't
//!
//! M3's primary/secondary/tertiary + tonal-surface model doesn't line up
//! one-to-one with Teksilo's role taxonomy, so the color mapping projects
//! each `ColorTokens` field onto its nearest M3 equivalent and exposes the
//! rest (containers, secondary, tertiary, the surface-container ladder)
//! through [`Material3Palette`]. Typography keeps Teksilo's bundled
//! **Inter** family (which is metrically Roboto-compatible; the text
//! engine would silently fall back to it for an unregistered "Roboto"
//! anyway) and adopts M3's type-scale sizes / weights / letter-spacing.
//!
//! ### Known limitations
//!
//! - **State layers are pre-composited.** M3 hover/pressed are 8 % / 12 %
//!   on-color overlays; Teksilo recipes take opaque colors, so those
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

use teksilo_core::presets::intui;
use teksilo_core::styles::{Theme, ThemeAppearance};

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

#[cfg(feature = "bundled-fonts")]
mod bundled_fonts {
    use teksilo_text::{FontFaceId, FontRegistrar, TextFontService};

    /// The bundled Roboto Flex variable font (Apache-2.0, ~1.7 MB). Only
    /// compiled under the `bundled-fonts` feature.
    const ROBOTO: &[u8] = include_bytes!("../fonts/Roboto.ttf");

    /// Registers the bundled Roboto under the family name "Roboto" for the
    /// weights M3 uses (400 / 500) and as the default font. The two
    /// `register_font_as` calls force the family name so the M3
    /// typography (which uses "Roboto" under this feature) resolves
    /// regardless of the file's internal name-table family.
    pub struct RobotoRegistrar;

    impl FontRegistrar for RobotoRegistrar {
        fn register_on_service(&self, service: &mut TextFontService) -> Option<FontFaceId> {
            let regular = service.register_font_as(ROBOTO, "Roboto", 400, false);
            service.register_font_as(ROBOTO, "Roboto", 500, false);
            service.set_default_font(regular, 14.0);
            Some(regular)
        }
    }
}

/// A [`FontRegistrar`](teksilo_text::FontRegistrar) that embeds Roboto and
/// registers it under the "Roboto" family. Pass it to the app builder so
/// the M3 typography (which uses "Roboto" under this feature) resolves
/// instead of falling back to the bundled default:
///
/// ```ignore
/// TeksiloAppBuilder::new()
///     .theme(material3::light())
///     .register_fonts(material3::font_registrar())
///     .run();
/// ```
///
/// Available only under the `bundled-fonts` Cargo feature.
#[cfg(feature = "bundled-fonts")]
pub fn font_registrar() -> impl teksilo_text::FontRegistrar {
    bundled_fonts::RobotoRegistrar
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::Color;

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
    fn on_error_and_containers_use_m3_values() {
        // The new core cross-language roles carry M3 values now.
        let l = light().colors;
        assert_eq!(l.text_on_error, Color::from_hex("#FFFFFF"));
        assert_eq!(l.surface_error_container, Color::from_hex("#F9DEDC"));
        assert_eq!(l.surface_container, Color::from_hex("#F3EDF7"));
        let d = dark().colors;
        assert_eq!(d.text_on_error, Color::from_hex("#601410"));
    }

    #[test]
    fn destructive_button_label_is_on_error() {
        use teksilo_core::styles::ButtonVariant;
        use teksilo_tokens::TextRole;
        let style = styles::button::m3_button_style();
        assert_eq!(
            style.label_roles.get(&ButtonVariant::Destructive),
            Some(&TextRole::OnError),
        );
    }

    #[test]
    fn filled_button_hover_is_a_state_layer() {
        use teksilo_core::styles::{ButtonVariant, FillRecipe};
        let style = styles::button::m3_button_style();
        let filled = &style.recipes[&ButtonVariant::Filled];
        assert!(matches!(
            filled.fill.hover,
            Some(FillRecipe::StateLayer { .. })
        ));
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

    #[test]
    fn motion_inherits_desktop_tooltip_delays() {
        // Material 3 overrides colour/shape/type/slots only — motion stays
        // on `MotionTokens::default()` so tooltip feel tracks the shared
        // desktop norms (500 / 700 / 100 ms).
        use std::time::Duration;
        for t in [light(), dark()] {
            assert_eq!(t.motion.tooltip_delay, Duration::from_millis(500));
            assert_eq!(t.motion.tooltip_delay_heavy, Duration::from_millis(700));
            assert_eq!(t.motion.tooltip_reshow_delay, Duration::from_millis(100));
        }
    }
}
