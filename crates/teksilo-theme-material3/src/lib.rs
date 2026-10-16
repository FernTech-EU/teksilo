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
use teksilo_tokens::{InputTokens, TargetDensity};

/// Material 3's input tokens for one density ladder.
///
/// Identical to [`InputTokens::for_density`] except at
/// [`TargetDensity::Touch`], where `target_size` is raised from the generic
/// 44 dp to Material 3's own **48 dp** touch-target minimum (M3 *Accessibility
/// — Touch targets*). 44 dp is Apple HIG / WCAG 2.2 SC 2.5.5 (level AAA); the
/// 24 dp AA floor in `min_target_conformance` is untouched.
///
/// At `Compact` — the default — this is the generic ladder unchanged, so a
/// Material 3 theme is byte-for-byte today's behaviour.
///
/// A density switch keeps this ladder: the preset's private `reproject` is
/// registered as the theme's
/// [`DensityProjection`](teksilo_core::styles::DensityProjection), so
/// [`Theme::with_density`] and `WidgetTree::set_input_density` call back into
/// this function instead of falling through to the generic
/// [`InputTokens::for_density`]. An app therefore does not re-apply these
/// tokens by hand; `a_density_switch_keeps_material3s_own_48_dp_target` pins it.
pub fn input_tokens(density: TargetDensity) -> InputTokens {
    let mut tokens = InputTokens::for_density(density);
    if density == TargetDensity::Touch {
        tokens.target_size = 48.0;
    }
    tokens
}

/// Material 3 light theme (baseline scheme, seed `#6750A4`).
pub fn light() -> Theme {
    let mut theme = intui::light().with_id("material3.light");
    apply_material3_overrides(&mut theme, ThemeAppearance::Light);
    theme
}

/// Re-derive a Material 3 theme for another density.
///
/// Registered as the theme's [`DensityProjection`](teksilo_core::styles::DensityProjection), so
/// `WidgetTree::set_input_density` (and `Theme::with_density`) rebuild the
/// Tier-3 slots this preset installs instead of carrying their Compact
/// dimensions across. Colours, id, the [`Material3Palette`] extension and any
/// slot the app installed itself ride across untouched.
fn reproject(base: &Theme, density: TargetDensity) -> Theme {
    let mut theme = base.clone();
    theme.input = input_tokens(density);
    install_style_slots(&mut theme);
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
    // M3's touch targets are 48 dp, not the generic ladder's 44 — a no-op at
    // the default Compact density. See `input_tokens`.
    theme.input = input_tokens(theme.input.density);
    theme.extensions.insert(if light {
        Material3Palette::light()
    } else {
        Material3Palette::dark()
    });

    install_style_slots(theme);
    theme
        .extensions
        .insert(teksilo_core::styles::DensityProjection(reproject));
}

/// Tier-3 widget chrome. These read M3-mapped roles, so a single install
/// serves both appearances; the dimensions come from `theme.input`, so
/// [`reproject`] can re-run this alone for a new density.
fn install_style_slots(theme: &mut Theme) {
    theme.style_slots.button = Some(Rc::new(styles::button::m3_button_style(&theme.input)));
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
    use std::collections::BTreeSet;
    use teksilo_core::styles::ComponentStyleSlots;
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
        let style = styles::button::m3_button_style(&InputTokens::default());
        assert_eq!(
            style.label_roles.get(&ButtonVariant::Destructive),
            Some(&TextRole::OnError),
        );
    }

    #[test]
    fn filled_button_hover_is_a_state_layer() {
        use teksilo_core::styles::{ButtonVariant, FillRecipe};
        let style = styles::button::m3_button_style(&InputTokens::default());
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

    /// A Material 3 component this preset has not transcribed: the widget's
    /// default recipe paints it, with the M3-mapped colour roles.
    const NOT_TRANSCRIBED: &str =
        "the M3 component is not transcribed; the recipe paints with the M3-mapped colour roles";
    /// A slot Material 3 specifies no component for.
    const NO_M3_COMPONENT: &str =
        "M3 specifies no such component; the recipe paints with the M3-mapped colour roles";

    /// The slots Material 3 deliberately leaves to the widget's default
    /// `Recipe*Style`, each with the reason it is inherited rather than filled.
    const MATERIAL3_UNFILLED: &[(&str, &str)] = &[
        ("split_button", NOT_TRANSCRIBED),
        ("splitter", NO_M3_COMPONENT),
        ("icon_button", NOT_TRANSCRIBED),
        ("checkbox", NOT_TRANSCRIBED),
        ("radio", NOT_TRANSCRIBED),
        ("radio_tile", NO_M3_COMPONENT),
        ("slider", NOT_TRANSCRIBED),
        ("text_input", NOT_TRANSCRIBED),
        ("combo_box", NOT_TRANSCRIBED),
        ("menu_item", NOT_TRANSCRIBED),
        ("panel", NO_M3_COMPONENT),
        ("chart", NO_M3_COMPONENT),
        ("popover", NOT_TRANSCRIBED),
        ("tooltip", NOT_TRANSCRIBED),
        ("scroll_bar", NO_M3_COMPONENT),
        ("standard_item", NOT_TRANSCRIBED),
        ("tab", NOT_TRANSCRIBED),
        ("dialog", NOT_TRANSCRIBED),
        ("snackbar", NOT_TRANSCRIBED),
        ("toast", NO_M3_COMPONENT),
        ("banner", NO_M3_COMPONENT),
        ("badge", NOT_TRANSCRIBED),
        ("progress_bar", NOT_TRANSCRIBED),
        ("link", NO_M3_COMPONENT),
        ("segmented_control", NOT_TRANSCRIBED),
        ("avatar", NO_M3_COMPONENT),
        ("calendar", NOT_TRANSCRIBED),
        ("color_picker", NO_M3_COMPONENT),
        ("spin_box", NO_M3_COMPONENT),
        (
            "date_edit",
            "the recipe is a passthrough around the TextInput, so it follows whatever the text_input slot paints",
        ),
        ("search_field", NOT_TRANSCRIBED),
        ("rich_text_editor", NO_M3_COMPONENT),
        ("table", NO_M3_COMPONENT),
        (
            "list_container",
            "the recipe is only the drag-insertion line, drawn in the M3-mapped accent role",
        ),
        ("drop_zone", NO_M3_COMPONENT),
        ("drop_target", NO_M3_COMPONENT),
        ("grid_view", NO_M3_COMPONENT),
        (
            "web_view",
            "the page paints itself; the recipe only tints the overlay shown over it, from theme roles",
        ),
        (
            "text_selection",
            "its geometry is teksilo_core::text_touch's, shared with hit testing, and its colours follow the M3-mapped accent role",
        ),
    ];

    /// Whether the installed slots and [`MATERIAL3_UNFILLED`] are exactly
    /// [`ComponentStyleSlots::NAMES`], with no slot in both: a slot added to the
    /// framework must be either filled by this preset or listed with a reason.
    #[test]
    fn installed_and_unfilled_slots_partition_the_roster() {
        let unfilled: BTreeSet<&str> = MATERIAL3_UNFILLED.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            unfilled.len(),
            MATERIAL3_UNFILLED.len(),
            "a slot is listed twice"
        );
        for (name, reason) in MATERIAL3_UNFILLED {
            assert!(
                !reason.trim().is_empty(),
                "{name} is listed without a reason"
            );
        }
        let names: BTreeSet<&str> = ComponentStyleSlots::NAMES.iter().copied().collect();
        for t in [light(), dark(), light().with_density(TargetDensity::Touch)] {
            let installed: BTreeSet<&str> = t.style_slots.installed().into_iter().collect();
            let both: Vec<_> = installed.intersection(&unfilled).collect();
            assert!(
                both.is_empty(),
                "installed yet listed as unfilled: {both:?}"
            );
            let covered: BTreeSet<&str> = installed.union(&unfilled).copied().collect();
            let missing: Vec<_> = names.difference(&covered).collect();
            let unknown: Vec<_> = covered.difference(&names).collect();
            assert!(
                missing.is_empty() && unknown.is_empty(),
                "neither installed nor listed: {missing:?}; not a slot: {unknown:?}"
            );
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
    /// Material 3's own touch target is **48 dp**, not the generic ladder's 44,
    /// and a density switch must carry that through to the installed button
    /// slot rather than falling back to the generic numbers.
    #[test]
    fn a_density_switch_keeps_material3s_own_48_dp_target() {
        let base = light();
        let before = base
            .style_slots
            .button
            .clone()
            .expect("button slot installed");

        let touch = base.with_density(TargetDensity::Touch);
        let after = touch
            .style_slots
            .button
            .clone()
            .expect("button slot survives");

        assert_eq!(touch.input.density, TargetDensity::Touch);
        assert_eq!(
            touch.input.target_size, 48.0,
            "the projection must use Material 3's ladder, not `InputTokens::for_density`"
        );
        assert!(
            !Rc::ptr_eq(&before, &after),
            "the projection must rebuild the preset's own slots, not carry them across"
        );

        let compact =
            styles::button::m3_button_style(&InputTokens::for_density(TargetDensity::Compact));
        let touched = styles::button::m3_button_style(&input_tokens(TargetDensity::Touch));
        let variant = teksilo_core::styles::ButtonVariant::Filled;
        assert_eq!(
            compact.recipes[&variant].min_size.height, 40.0,
            "M3's own 40 dp button at Compact"
        );
        assert_eq!(
            touched.recipes[&variant].min_size.height, 48.0,
            "M3's 48 dp touch target, not the generic 44"
        );

        assert_eq!(touch.id, base.id);
        assert!(touch.extension::<Material3Palette>().is_some());
    }
}
