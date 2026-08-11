// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent (Windows 11 / WinUI 3) theme preset for Teksilo.
//!
//! A full Fluent theme: the WinUI theme-resource colour dictionaries, the
//! two-radius geometry, the WinUI type ramp, Fluent's control durations and
//! easing, and Tier-3 chrome for the widgets whose WinUI counterpart is
//! structurally its own thing.
//!
//! ```ignore
//! use teksilo_theme_fluent as fluent;
//!
//! TeksiloAppBuilder::new()
//!     .theme(fluent::light())
//!     .initial_window(WindowConfig::new().title("Fluent demo"))
//!     .run();
//! ```
//!
//! ## What makes it read as Fluent
//!
//! - **The elevation edge.** A raised control carries a heavier stroke on
//!   one edge — the bottom in light (a cast shadow), the top in dark (a
//!   catch-light) — and loses it when pressed. WinUI gets this from a
//!   gradient border brush anchored to a fixed 3 dp band; see
//!   [`palette::FluentEdgeSide`].
//! - **A high-contrast focus ring, never the accent.** Two tones: a 2 dp
//!   near-black (light) or white (dark) outer ring with a 1 dp opposite
//!   inner ring between it and the control. It reads on any background,
//!   which is why Fluent does not tint it.
//! - **The accent focus underline.** A focused field's bottom edge grows to
//!   2 dp and turns accent while its fill brightens.
//! - **Neutral selection plus an accent pill.** A selected list row gets a
//!   grey wash and a 3 × 16 dp accent bar on its leading edge.
//! - **An asymmetric accent ramp.** The light theme fills with a *darkened*
//!   accent and white labels, the dark theme with a *lightened* one and
//!   black labels — which is how both clear contrast from one seed colour.
//!
//! ## Two radii, 32 dp, no tracking
//!
//! Fluent's geometry is `ControlCornerRadius` 4 dp for anything in the page
//! and `OverlayCornerRadius` 8 dp for anything that floats (tooltips
//! excepted — they float but round at 4). Interactive controls share a
//! 32 dp height. The type ramp specifies **zero** letter-spacing at every
//! rung: Fluent's size-appropriate texture comes from Segoe UI Variable's
//! optical-size axis, not from tracking — the opposite of Material 3.
//!
//! ## Accent colour
//!
//! `AccentFillColor*` is not a literal in WinUI's dictionary; it binds to
//! the ramp Windows generates from the user's chosen accent. [`light`] and
//! [`dark`] resolve it against the Windows out-of-box `#0078D4`;
//! [`light_with_accent`] / [`dark_with_accent`] rebuild the whole theme
//! around a different seed, so an app that reads the live Windows accent —
//! or wants a brand colour — stays Fluent everywhere else.
//!
//! ## Known limitations
//!
//! - **Mica and Acrylic are their solid fallbacks.** Both are compositor
//!   materials: Mica samples the desktop wallpaper, Acrylic blurs what is
//!   behind the window. Neither is expressible in a flat-fill renderer, and
//!   Teksilo's wgpu surface does not expose compositor-side blur. Every
//!   surface therefore uses the opaque fallback WinUI itself falls back to
//!   when the material is unavailable — which is the documented behaviour
//!   on a machine with transparency effects off, not an approximation.
//! - **Segoe UI Variable cannot be bundled.** It is proprietary, so the
//!   default build keeps the metric-neutral bundled Inter and the
//!   `system-fonts` feature names the Windows faces for the text engine to
//!   resolve. See [`typography`].
//! - **The accent ramp is reconstructed, not published.** Microsoft
//!   documents that Windows generates `SystemAccentColorLight1..3` /
//!   `Dark1..3` from the base accent but not *how*.
//!   [`palette::FluentAccentRamp::windows_default`] carries the measured
//!   values for `#0078D4`; `from_base` approximates the curve for any other
//!   seed.
//! - **Widget chrome is chosen at build time.** Switching between Fluent
//!   and another preset at runtime re-tints instantly but keeps the shapes
//!   the tree was built with — a property of the styling system, not of
//!   this preset.

mod color;
pub mod motion;
pub mod palette;
pub mod shape;
pub mod styles;
pub mod typography;

pub use palette::{FluentAccentRamp, FluentEdgeSide, FluentPalette};

use std::rc::Rc;

use teksilo_core::presets::intui;
use teksilo_core::styles::{Theme, ThemeAppearance};
use teksilo_tokens::Color;

/// Fluent light theme, on the Windows default accent `#0078D4`.
pub fn light() -> Theme {
    build(ThemeAppearance::Light, FluentPalette::light())
}

/// Fluent dark theme, on the Windows default accent `#0078D4`.
pub fn dark() -> Theme {
    build(ThemeAppearance::Dark, FluentPalette::dark())
}

/// Fluent light theme rebuilt around `accent` — the substitution Windows
/// performs when the user picks an accent colour.
///
/// ```ignore
/// let theme = teksilo_theme_fluent::light_with_accent(Color::from_hex("#B146C2"));
/// ```
pub fn light_with_accent(accent: Color) -> Theme {
    build(
        ThemeAppearance::Light,
        FluentPalette::light_with_accent(ramp_for(accent)),
    )
}

/// Fluent dark theme rebuilt around `accent`.
pub fn dark_with_accent(accent: Color) -> Theme {
    build(
        ThemeAppearance::Dark,
        FluentPalette::dark_with_accent(ramp_for(accent)),
    )
}

/// The measured Windows ramp when the seed *is* the default accent,
/// the approximation otherwise.
fn ramp_for(accent: Color) -> FluentAccentRamp {
    let default = FluentAccentRamp::windows_default();
    if accent == default.base {
        default
    } else {
        FluentAccentRamp::from_base(accent)
    }
}

fn build(appearance: ThemeAppearance, palette: FluentPalette) -> Theme {
    let light = appearance == ThemeAppearance::Light;
    // Starting from the IntUI baseline keeps every token Fluent has no
    // opinion about (the chart palette, the find-match highlight) at a
    // sensible value instead of at zero.
    let mut theme = if light {
        intui::light().with_id("fluent.light")
    } else {
        intui::dark().with_id("fluent.dark")
    };

    theme.colors = if light {
        color::fluent_light_colors(&palette)
    } else {
        color::fluent_dark_colors(&palette)
    };
    theme.shape = if light {
        shape::fluent_light_shape()
    } else {
        shape::fluent_dark_shape()
    };
    theme.typography = typography::fluent_typography();
    theme.motion = motion::fluent_motion();
    theme.extensions.insert(palette);

    install_styles(&mut theme);
    theme
}

/// Install the Fluent Tier-3 chrome. Every style resolves its colours from
/// the live theme at paint time, so one install serves both appearances —
/// and a custom-accent theme too.
fn install_styles(theme: &mut Theme) {
    let slots = &mut theme.style_slots;

    // Structurally Fluent — see `styles`.
    slots.button = Some(Rc::new(styles::button::FluentButtonStyle));
    slots.toggle = Some(Rc::new(styles::toggle::FluentToggleStyle));
    slots.checkbox = Some(Rc::new(styles::checkbox::FluentCheckboxStyle));
    slots.radio = Some(Rc::new(styles::radio::FluentRadioStyle));
    slots.text_input = Some(Rc::new(styles::text_input::FluentTextInputStyle));
    slots.slider = Some(Rc::new(styles::slider::FluentSliderStyle));
    slots.menu_item = Some(Rc::new(styles::menu_item::FluentMenuItemStyle));
    slots.standard_item = Some(Rc::new(styles::standard_item::FluentStandardItemStyle));

    // Fluent metrics over the shipped composition.
    slots.card = Some(Rc::new(styles::metrics::FluentCardStyle));
    slots.panel = Some(Rc::new(styles::metrics::fluent_panel_style()));
    slots.popover = Some(Rc::new(styles::metrics::fluent_popover_style()));
    slots.tooltip = Some(Rc::new(styles::metrics::fluent_tooltip_style()));
    slots.dialog = Some(Rc::new(styles::metrics::fluent_dialog_style()));
    slots.snackbar = Some(Rc::new(styles::metrics::fluent_snackbar_style()));
    slots.toast = Some(Rc::new(styles::metrics::fluent_toast_style()));
    slots.banner = Some(Rc::new(styles::metrics::fluent_banner_style()));
    slots.combo_box = Some(Rc::new(styles::metrics::fluent_combo_box_style()));
    slots.icon_button = Some(Rc::new(styles::metrics::fluent_icon_button_style()));
    slots.link = Some(Rc::new(styles::metrics::fluent_link_style()));
    slots.segmented_control = Some(Rc::new(styles::metrics::fluent_segmented_control_style()));
    slots.badge = Some(Rc::new(styles::metrics::fluent_badge_style()));
    slots.progress_bar = Some(Rc::new(styles::metrics::fluent_progress_bar_style()));
    slots.scroll_bar = Some(Rc::new(styles::metrics::fluent_scroll_bar_style()));
    slots.tab = Some(Rc::new(styles::metrics::fluent_tab_style()));
    slots.table = Some(Rc::new(styles::metrics::fluent_table_style()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::argb;

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
        assert_eq!(light().id.as_str(), "fluent.light");
        assert_eq!(dark().id.as_str(), "fluent.dark");
        // The widget-catalog persists and restores themes by this id, so
        // a rename here silently breaks restore-on-launch.
        assert_eq!(light_with_accent(Color::WHITE).id.as_str(), "fluent.light");
        assert_eq!(dark_with_accent(Color::WHITE).id.as_str(), "fluent.dark");
    }

    #[test]
    fn light_and_dark_have_distinct_surfaces() {
        assert_ne!(light().colors.surface_main, dark().colors.surface_main);
    }

    #[test]
    fn surfaces_are_the_winui_solid_backgrounds() {
        assert_eq!(light().colors.surface_main, argb("#F3F3F3"));
        assert_eq!(dark().colors.surface_main, argb("#FF202020"));
    }

    #[test]
    fn accent_is_the_asymmetric_ramp() {
        // Light fills with a darkened accent, dark with a lightened one —
        // so white-on-light and black-on-dark both clear contrast.
        assert_eq!(light().colors.accent, argb("#0067C0"));
        assert_eq!(dark().colors.accent, argb("#4CC2FF"));
        assert_eq!(light().colors.text_on_accent, Color::WHITE);
        assert_eq!(dark().colors.text_on_accent, Color::BLACK);
    }

    #[test]
    fn focus_indicator_is_high_contrast_not_accent() {
        for t in [light(), dark()] {
            assert_ne!(t.colors.focus_ring, t.colors.accent);
        }
        assert_eq!(light().colors.focus_ring, argb("#E4000000"));
        assert_eq!(dark().colors.focus_ring, Color::WHITE);
    }

    #[test]
    fn geometry_is_the_two_fluent_radii() {
        for t in [light(), dark()] {
            assert_eq!(t.shape.radius_control, 4.0);
            assert_eq!(t.shape.radius_popup, 8.0);
        }
    }

    #[test]
    fn body_is_the_winui_body_ramp_with_no_tracking() {
        let t = light();
        assert!((t.typography.body.size - 14.0).abs() < 0.01);
        assert_eq!(t.typography.body.letter_spacing, 0.0);
        assert_eq!(t.typography.small.letter_spacing, 0.0);
    }

    #[test]
    fn motion_uses_the_fluent_control_durations() {
        use std::time::Duration;
        for t in [light(), dark()] {
            assert_eq!(t.motion.duration_fast, Duration::from_millis(167));
            assert_eq!(t.motion.duration_normal, Duration::from_millis(250));
        }
    }

    #[test]
    fn palette_extension_present_and_appearance_matched() {
        let l = light().extension::<FluentPalette>().copied().unwrap();
        let d = dark().extension::<FluentPalette>().copied().unwrap();
        assert_eq!(l.elevation_edge, FluentEdgeSide::Bottom);
        assert_eq!(d.elevation_edge, FluentEdgeSide::Top);
        // The extension carries tokens `ColorTokens` has no slot for.
        assert_eq!(l.control_stroke_secondary, argb("#29000000"));
        assert_eq!(d.control_fill_input_active, argb("#B31E1E1E"));
    }

    #[test]
    fn every_installed_style_slot_is_populated() {
        for t in [light(), dark()] {
            let s = &t.style_slots;
            for (name, present) in [
                ("button", s.button.is_some()),
                ("toggle", s.toggle.is_some()),
                ("checkbox", s.checkbox.is_some()),
                ("radio", s.radio.is_some()),
                ("text_input", s.text_input.is_some()),
                ("slider", s.slider.is_some()),
                ("menu_item", s.menu_item.is_some()),
                ("standard_item", s.standard_item.is_some()),
                ("card", s.card.is_some()),
                ("panel", s.panel.is_some()),
                ("popover", s.popover.is_some()),
                ("tooltip", s.tooltip.is_some()),
                ("dialog", s.dialog.is_some()),
                ("snackbar", s.snackbar.is_some()),
                ("toast", s.toast.is_some()),
                ("banner", s.banner.is_some()),
                ("combo_box", s.combo_box.is_some()),
                ("icon_button", s.icon_button.is_some()),
                ("link", s.link.is_some()),
                ("segmented_control", s.segmented_control.is_some()),
                ("badge", s.badge.is_some()),
                ("progress_bar", s.progress_bar.is_some()),
                ("scroll_bar", s.scroll_bar.is_some()),
                ("tab", s.tab.is_some()),
                ("table", s.table.is_some()),
            ] {
                assert!(present, "{name} slot not installed");
            }
        }
    }

    #[test]
    fn custom_accent_rebuilds_the_whole_accent_family() {
        let seed = Color::from_hex("#B146C2");
        let t = light_with_accent(seed);
        let base = light();
        assert_ne!(t.colors.accent, base.colors.accent);
        assert_ne!(t.colors.text_link, base.colors.text_link);
        assert_ne!(
            t.colors.selection_bg_active,
            base.colors.selection_bg_active
        );
        // Neutral tokens are untouched — only the accent family moves.
        assert_eq!(t.colors.surface_main, base.colors.surface_main);
        assert_eq!(t.colors.text_primary, base.colors.text_primary);
        assert_eq!(t.colors.focus_ring, base.colors.focus_ring);
        // …and the extension follows, so widget chrome sees it too.
        let p = t.extension::<FluentPalette>().copied().unwrap();
        assert_eq!(p.accent_ramp.base, seed);
        assert_eq!(p.system_fill_attention, seed);
    }

    #[test]
    fn passing_the_default_accent_reproduces_the_measured_ramp() {
        let default = FluentAccentRamp::windows_default();
        assert_eq!(
            light_with_accent(default.base).colors.accent,
            light().colors.accent
        );
        assert_eq!(
            dark_with_accent(default.base).colors.accent,
            dark().colors.accent
        );
    }

    #[test]
    fn the_theme_survives_the_window_inactive_projection() {
        // The paint walker swaps this palette in on focus loss; a preset
        // that left `accent` unmapped would produce a no-op here.
        for t in [light(), dark()] {
            let inactive = t.for_inactive_window();
            assert_ne!(inactive.colors.accent, t.colors.accent);
            assert_eq!(inactive.colors.surface_main, t.colors.surface_main);
        }
    }
}
