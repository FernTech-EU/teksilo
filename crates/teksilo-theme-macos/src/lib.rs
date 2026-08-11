// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS (Aqua / Dark Aqua) theme preset for Teksilo.
//!
//! A full macOS theme: the AppKit semantic-colour palette, the two
//! measured radii, the published San Francisco type ramp with its signed
//! tracking, Core Animation's default duration and curve, and Tier-3
//! chrome for the widgets whose AppKit counterpart is structurally its own
//! thing.
//!
//! ```ignore
//! use teksilo_theme_macos as macos;
//!
//! TeksiloAppBuilder::new()
//!     .theme(macos::light())
//!     .initial_window(WindowConfig::new().title("macOS demo"))
//!     .run();
//! ```
//!
//! ## What makes it read as macOS
//!
//! - **The focus ring is the accent.** Not a neutral high-contrast outline
//!   (Fluent) and not a thickened border (Material 3) — the user's own
//!   accent colour, as a soft halo hugging the control's outline. See
//!   [`shape`] for how the halo is built so it still clears WCAG 1.4.11.
//! - **Controls are physical objects.** A push button, a popup button, a
//!   switch knob and a slider knob all share one bezel: a faint top-to-
//!   bottom gradient, a hairline, and a shadow that separates it from the
//!   surface underneath. Big Sur flattened Aqua's gloss but kept all
//!   three, and it is what stops a macOS control reading as a coloured
//!   rectangle.
//! - **Selection is a capsule, and its label turns white.** A selected
//!   list row is an accent-filled rounded capsule inset from the row's
//!   edges, with `alternateSelectedControlTextColor` on top. Menus do the
//!   same. This is the most recognisable single element of a macOS list.
//! - **Signed tracking.** Small text is loosened, larger text tightened,
//!   crossing zero at 12 pt — the only Teksilo preset that tracks
//!   non-uniformly, and a published Apple table rather than a guess.
//! - **Denser than Fluent.** A 22 dp control height against Windows 11's
//!   32, and a 13 pt body against 14. macOS fits more in the same window.
//! - **Symmetric motion.** `ease-in-ease-out` rather than Fluent's
//!   decelerate-only curve: things start gently and stop gently.
//!
//! ## Accent colour
//!
//! `controlAccentColor` is whatever the user picked in System Settings.
//! [`light`] and [`dark`] resolve it against the out-of-box `systemBlue`;
//! [`light_with_accent`] / [`dark_with_accent`] rebuild the whole theme
//! around a different seed, and [`SystemAccent`] carries the eight
//! swatches macOS itself offers:
//!
//! ```ignore
//! use teksilo_theme_macos::{self as macos, SystemAccent};
//! let theme = macos::light_with_accent(SystemAccent::Purple.light());
//! ```
//!
//! Note that `linkColor` deliberately does **not** follow: AppKit keeps
//! links a fixed blue whatever accent is chosen, and so does this preset.
//!
//! ## Known limitations
//!
//! - **The OS accent is not read.** Teksilo's platform layer returns only
//!   the light/dark preference on macOS — `query_os_theme_colors` fills in
//!   the colour scheme and nothing else — so there is no live
//!   `NSColor.controlAccentColor` to bind to. The preset ships the stock
//!   blue and an app that wants the user's real accent passes it to
//!   [`light_with_accent`] itself. Wiring the AppKit read is a
//!   platform-layer change, not a theme one.
//! - **Vibrancy is its opaque fallback.** Sidebars, menus, popovers and
//!   title bars are `NSVisualEffectView` materials that sample and blur
//!   what is *behind the window*; Teksilo's wgpu surface exposes no
//!   compositor-side blur, so every such surface uses the opaque colour
//!   the material falls back to — which is what macOS itself shows with
//!   "Reduce transparency" on, not an invention. (Teksilo's `Blur`
//!   primitive can blur content *within* a window, so a popover over the
//!   app's own content is expressible; sampling the desktop wallpaper is
//!   not.)
//! - **The table selection band is a wash, not a capsule.** `ListView` and
//!   `TreeView` rows get the authentic solid-accent capsule with a white
//!   label, because [`styles::standard_item`] owns both halves.
//!   `TableView` / `TreeTableView` / `GridView` paint the shared
//!   `surface_selected` token behind *app-supplied* cell widgets whose
//!   text this preset cannot retint, so that token is the accent as a wash
//!   the primary label still clears 4.5:1 on. Fluent makes the same split
//!   for the same reason.
//! - **Several literals are snapshots.** Apple's own standing disclaimer
//!   is that published colour values "will fluctuate from release to
//!   release"; control heights, corner radii, focus-ring geometry and
//!   every animation duration but one are not published at all. Each value
//!   in this crate is tagged `[HIG]`, `[measured]` or `[derived]` at its
//!   definition, and the handful that deviate from Apple's own numbers to
//!   meet WCAG are called out with the measurement that forced the change.
//! - **Window activation is two-state, AppKit's is three.** Teksilo
//!   models a window as active or not; AppKit distinguishes Main, Key and
//!   Inactive (a document window stays Main while its sheet is Key). The
//!   accent desaturation this preset inherits fires on the Teksilo
//!   boundary, which matches AppKit for every case except that one.
//! - **Widget chrome is chosen at build time.** Switching between macOS
//!   and another preset at runtime re-tints instantly but keeps the shapes
//!   the tree was built with — a property of the styling system, not of
//!   this preset.

mod color;
pub mod motion;
pub mod palette;
pub mod shape;
pub mod styles;
pub mod typography;

pub use palette::{MacOsAccentRamp, MacOsBezel, MacOsPalette, SystemAccent};

use std::rc::Rc;

use teksilo_core::presets::intui;
use teksilo_core::styles::{Theme, ThemeAppearance};
use teksilo_tokens::Color;

/// macOS **Aqua** (light), on the stock `systemBlue` accent.
pub fn light() -> Theme {
    build(ThemeAppearance::Light, MacOsPalette::light())
}

/// macOS **Dark Aqua**, on the stock `systemBlue` accent.
pub fn dark() -> Theme {
    build(ThemeAppearance::Dark, MacOsPalette::dark())
}

/// Aqua rebuilt around `accent` — the substitution macOS performs when the
/// user picks an accent colour in System Settings.
///
/// ```ignore
/// use teksilo_theme_macos::{self as macos, SystemAccent};
/// let theme = macos::light_with_accent(SystemAccent::Green.light());
/// ```
pub fn light_with_accent(accent: Color) -> Theme {
    build(
        ThemeAppearance::Light,
        MacOsPalette::light_with_accent(light_ramp_for(accent)),
    )
}

/// Dark Aqua rebuilt around `accent`.
pub fn dark_with_accent(accent: Color) -> Theme {
    build(
        ThemeAppearance::Dark,
        MacOsPalette::dark_with_accent(dark_ramp_for(accent)),
    )
}

/// The measured ramp when the seed *is* the stock accent, the derivation
/// otherwise — so `light_with_accent(SystemAccent::Blue.light())` and
/// [`light`] agree exactly.
fn light_ramp_for(accent: Color) -> MacOsAccentRamp {
    let default = MacOsAccentRamp::system_blue_light();
    if accent == default.base {
        default
    } else {
        MacOsAccentRamp::light_from_base(accent)
    }
}

fn dark_ramp_for(accent: Color) -> MacOsAccentRamp {
    let default = MacOsAccentRamp::system_blue_dark();
    if accent == default.base {
        default
    } else {
        MacOsAccentRamp::dark_from_base(accent)
    }
}

fn build(appearance: ThemeAppearance, palette: MacOsPalette) -> Theme {
    let light = appearance == ThemeAppearance::Light;
    // Starting from the IntUI baseline keeps every token macOS has no
    // opinion about (the chart palette, the find-match highlight) at a
    // sensible value instead of at zero.
    let mut theme = if light {
        intui::light().with_id("macos.light")
    } else {
        intui::dark().with_id("macos.dark")
    };

    theme.colors = if light {
        color::macos_light_colors(&palette)
    } else {
        color::macos_dark_colors(&palette)
    };
    theme.shape = if light {
        shape::macos_light_shape()
    } else {
        shape::macos_dark_shape()
    };
    theme.typography = typography::macos_typography();
    theme.motion = motion::macos_motion();
    theme.extensions.insert(palette);

    install_styles(&mut theme);
    theme
}

/// Install the macOS Tier-3 chrome. Every style resolves its colours from
/// the live theme at paint time, so one install serves both appearances —
/// and a custom-accent theme too.
fn install_styles(theme: &mut Theme) {
    let slots = &mut theme.style_slots;

    // Structurally macOS — see `styles`.
    slots.button = Some(Rc::new(styles::button::MacOsButtonStyle));
    slots.toggle = Some(Rc::new(styles::toggle::MacOsToggleStyle));
    slots.checkbox = Some(Rc::new(styles::checkbox::MacOsCheckboxStyle));
    slots.radio = Some(Rc::new(styles::radio::MacOsRadioStyle));
    slots.text_input = Some(Rc::new(styles::text_input::MacOsTextInputStyle));
    slots.slider = Some(Rc::new(styles::slider::MacOsSliderStyle));
    slots.menu_item = Some(Rc::new(styles::menu_item::MacOsMenuItemStyle));
    slots.standard_item = Some(Rc::new(styles::standard_item::MacOsStandardItemStyle));

    // macOS metrics over the shipped composition.
    slots.card = Some(Rc::new(styles::metrics::MacOsCardStyle));
    slots.panel = Some(Rc::new(styles::metrics::macos_panel_style()));
    slots.popover = Some(Rc::new(styles::metrics::macos_popover_style()));
    slots.tooltip = Some(Rc::new(styles::metrics::macos_tooltip_style()));
    slots.dialog = Some(Rc::new(styles::metrics::macos_dialog_style()));
    slots.snackbar = Some(Rc::new(styles::metrics::macos_snackbar_style()));
    slots.toast = Some(Rc::new(styles::metrics::macos_toast_style()));
    slots.banner = Some(Rc::new(styles::metrics::macos_banner_style()));
    slots.combo_box = Some(Rc::new(styles::metrics::macos_combo_box_style()));
    slots.icon_button = Some(Rc::new(styles::metrics::macos_icon_button_style()));
    slots.link = Some(Rc::new(styles::metrics::macos_link_style()));
    slots.segmented_control = Some(Rc::new(styles::metrics::macos_segmented_control_style()));
    slots.badge = Some(Rc::new(styles::metrics::macos_badge_style()));
    slots.progress_bar = Some(Rc::new(styles::metrics::macos_progress_bar_style()));
    slots.scroll_bar = Some(Rc::new(styles::metrics::macos_scroll_bar_style()));
    slots.tab = Some(Rc::new(styles::metrics::macos_tab_style()));
    slots.table = Some(Rc::new(styles::metrics::macos_table_style()));
    slots.calendar = Some(Rc::new(styles::metrics::macos_calendar_style()));
    slots.search_field = Some(Rc::new(styles::metrics::macos_search_field_style()));
    slots.avatar = Some(Rc::new(styles::metrics::macos_avatar_style()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::hex;

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
        assert_eq!(light().id.as_str(), "macos.light");
        assert_eq!(dark().id.as_str(), "macos.dark");
        // The widget-catalog persists and restores themes by this id, so
        // a rename here silently breaks restore-on-launch.
        assert_eq!(light_with_accent(Color::WHITE).id.as_str(), "macos.light");
        assert_eq!(dark_with_accent(Color::WHITE).id.as_str(), "macos.dark");
    }

    #[test]
    fn surfaces_are_the_appkit_window_backgrounds() {
        assert_eq!(light().colors.surface_main, hex("#ECECEC"));
        assert_eq!(dark().colors.surface_main, hex("#323232"));
        assert_ne!(light().colors.surface_main, dark().colors.surface_main);
    }

    #[test]
    fn the_accent_is_the_selection_shade_not_the_raw_control_accent() {
        // The distinction the whole `MacOsAccentRamp` exists for.
        assert_eq!(light().colors.accent, hex("#0063E1"));
        assert_eq!(dark().colors.accent, hex("#0058D0"));
        assert_eq!(light().colors.focus_ring, SystemAccent::Blue.light());
        assert_eq!(dark().colors.focus_ring, SystemAccent::Blue.dark());
        assert_eq!(light().colors.text_on_accent, Color::WHITE);
        assert_eq!(dark().colors.text_on_accent, Color::WHITE);
    }

    /// The single clearest way this preset differs from Fluent.
    #[test]
    fn the_focus_indicator_is_the_accent_unlike_fluent() {
        for t in [light(), dark()] {
            assert_eq!(t.colors.focus_ring, t.colors.border_focused);
            // Fluent asserts the opposite: that its ring is *not* the
            // accent. macOS asserts that it comes from the same family.
            assert_eq!(
                t.colors.focus_ring,
                t.extension::<MacOsPalette>().unwrap().accent_ramp.base
            );
        }
    }

    #[test]
    fn geometry_is_the_two_measured_radii() {
        for t in [light(), dark()] {
            assert_eq!(t.shape.radius_control, 6.0);
            assert_eq!(t.shape.radius_popup, 10.0);
            assert_eq!(t.shape.focus_ring_offset, 0.0);
        }
    }

    #[test]
    fn body_is_the_published_macos_ramp_with_signed_tracking() {
        let t = light();
        assert!((t.typography.body.size - 13.0).abs() < 0.01);
        assert!(t.typography.body.letter_spacing < 0.0);
        assert_eq!(t.typography.small.letter_spacing, 0.0);
        assert!(t.typography.tiny.letter_spacing > 0.0);
    }

    #[test]
    fn motion_uses_the_core_animation_default() {
        use std::time::Duration;
        for t in [light(), dark()] {
            assert_eq!(t.motion.duration_normal, Duration::from_millis(250));
        }
    }

    #[test]
    fn palette_extension_present_and_appearance_matched() {
        let l = light().extension::<MacOsPalette>().copied().unwrap();
        let d = dark().extension::<MacOsPalette>().copied().unwrap();
        // The extension carries tokens `ColorTokens` has no slot for.
        assert_eq!(l.window_background, hex("#ECECEC"));
        assert_eq!(d.window_background, hex("#323232"));
        assert!(l.bezel.inner_light.a() == 0.0);
        assert!(d.bezel.inner_light.a() > 0.0);
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
                ("calendar", s.calendar.is_some()),
                ("search_field", s.search_field.is_some()),
                ("avatar", s.avatar.is_some()),
            ] {
                assert!(present, "{name} slot not installed");
            }
        }
    }

    #[test]
    fn custom_accent_rebuilds_the_whole_accent_family() {
        let seed = SystemAccent::Purple.light();
        let t = light_with_accent(seed);
        let base = light();
        assert_ne!(t.colors.accent, base.colors.accent);
        assert_ne!(t.colors.focus_ring, base.colors.focus_ring);
        assert_ne!(
            t.colors.selection_bg_active,
            base.colors.selection_bg_active
        );
        // Neutral tokens are untouched — only the accent family moves…
        assert_eq!(t.colors.surface_main, base.colors.surface_main);
        assert_eq!(t.colors.text_primary, base.colors.text_primary);
        // …and `linkColor` is not part of it, on macOS.
        assert_eq!(t.colors.text_link, base.colors.text_link);
        // …and the extension follows, so widget chrome sees it too.
        let p = t.extension::<MacOsPalette>().copied().unwrap();
        assert_eq!(p.accent_ramp.base, seed);
    }

    #[test]
    fn passing_the_default_accent_reproduces_the_measured_ramp() {
        assert_eq!(
            light_with_accent(SystemAccent::Blue.light()).colors.accent,
            light().colors.accent
        );
        assert_eq!(
            dark_with_accent(SystemAccent::Blue.dark()).colors.accent,
            dark().colors.accent
        );
    }

    #[test]
    fn every_system_accent_builds_a_usable_theme() {
        for accent in SystemAccent::ALL {
            for t in [
                light_with_accent(accent.light()),
                dark_with_accent(accent.dark()),
            ] {
                let c = &t.colors;
                assert!(
                    crate::palette::over(c.text_on_accent, c.accent).contrast_ratio(c.accent)
                        >= 4.5,
                    "{accent:?}: an accent-filled label is unreadable"
                );
                // …and the chrome is installed whichever accent was used.
                assert!(t.style_slots.button.is_some());
            }
        }
    }

    #[test]
    fn the_theme_survives_the_window_inactive_projection() {
        // The paint walker swaps this palette in on focus loss — the
        // convention macOS itself originated. A preset that left `accent`
        // unmapped would produce a no-op here.
        for t in [light(), dark()] {
            let inactive = t.for_inactive_window();
            assert_ne!(inactive.colors.accent, t.colors.accent);
            assert_ne!(inactive.colors.focus_ring, t.colors.focus_ring);
            assert_eq!(inactive.colors.surface_main, t.colors.surface_main);
        }
    }
}
