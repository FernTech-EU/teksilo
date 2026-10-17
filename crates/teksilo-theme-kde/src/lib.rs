// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! KDE Plasma theme preset for Teksilo: the Breeze design language.
//!
//! The colours come from a KDE colour scheme, the stock Breeze Light or
//! Breeze Dark or any other, read and resolved by [`scheme`]
//! (`teksilo-kde-scheme`) exactly as KDE resolves it, including Plasma's
//! accent colours. The geometry, typography and motion are Breeze's, as
//! Plasma 6.8 ships it ([`BREEZE_VERSION`]).
//!
//! ```
//! use teksilo_theme_kde::{self as kde, PlasmaAccent};
//!
//! let light = kde::light();
//! let dark = kde::dark();
//! let purple = kde::light_with_accent(PlasmaAccent::Purple.color());
//! assert_eq!(light.id.as_str(), "kde.light");
//! assert_eq!(dark.id.as_str(), "kde.dark");
//! assert_ne!(purple.colors.accent, light.colors.accent);
//! ```
//!
//! A scheme of the user's own, read from its `.colors` file, gives its own
//! theme, light or dark as KDE judges it:
//!
//! ```
//! use teksilo_theme_kde::{self as kde, scheme::KdeColorScheme};
//!
//! let scheme = KdeColorScheme::breeze_dark();
//! let theme = kde::from_scheme(&scheme);
//! assert!(theme.is_dark());
//! ```
//!
//! ## What makes it read as Breeze
//!
//! - **One radius.** Controls and floating surfaces share Breeze's 5 px
//!   frame radius (4.5 where a 1 px outline is stroked), between Fluent's 4
//!   and macOS's 6 and 10.
//! - **Breeze's buttons.** A push button is a flat fill outlined in the
//!   button colour moved a fifth of the way to its text (the frame
//!   contrast), with an outline that turns to the highlight under the
//!   pointer or keyboard focus and a 1 px shadow below it in an active
//!   window; the default button's fill moves toward the highlight, and a
//!   tool button shows nothing until it is hovered ([`styles`]). Other
//!   controls are outlined in the window colour moved the same way toward
//!   the text, the `border` token; the highlight-based outlines are in
//!   [`BreezeColors`].
//! - **Breeze's focus ring geometry.** Keyboard focus is a 2 px band, half
//!   inside the control and half outside, in the scheme's focus colour.
//!   Breeze paints it at 30 % ([`shape::BREEZE_FOCUS_RING_ALPHA`]); the
//!   `focus_ring` token is opaque, lifted to 3:1, and the styles draw it as
//!   it is.
//! - **Linear, quick motion.** Style transitions run 100 ms, linearly.
//! - **Text-driven heights.** A push button is the text line plus 16 px, a
//!   field the line plus 14: 34 and 32 px at Plasma's default font, and
//!   they grow with it ([`geometry`]).
//!
//! ## Accent colour
//!
//! [`light_with_accent`] and [`dark_with_accent`] apply an accent the way
//! Plasma's colour settings apply it to the stock scheme: links, focus and
//! hover take the accent (a link darkened or lightened once when it would be
//! hard to read), and the selection becomes the accent at 70 % over the
//! view with black or white text. So even Breeze's own blue,
//! [`PlasmaAccent::Blue`], gives a lighter selection than [`light`], as it
//! does on a Plasma desktop. [`PlasmaAccent`] lists the ten swatches Plasma
//! offers.
//!
//! ## Colours and WCAG
//!
//! KDE never adjusts a colour for readability, and some stock Breeze colours
//! fall short of WCAG 2.x (the light theme's secondary text, links, status
//! colours, focus colour, frame outlines, scroll-bar thumb and selection
//! text). The theme's colour tokens lift each of them by rule, for any
//! scheme, keeping its hue; the [`KdePalette`] extension keeps the scheme's
//! own colours. The rules, the inactive-window and high-contrast colours and
//! the tokens kept from Int UI are listed in the colour mapping's
//! documentation (the private `color` module); every lift is tested to pass
//! and its premise, that the scheme's colour fails, is tested too.
//!
//! An inactive window paints the scheme's Inactive colours: for Breeze, a
//! selection and accent muted to the window colour tinted toward the
//! selection colour, as Plasma paints a backdrop window, with the focus ring
//! unchanged. Under the OS high-contrast preference the theme paints its own
//! high-contrast colours, with 7:1 text and 4.5:1 boundaries.
//!
//! ## Trademarks
//!
//! KDE® and the K Desktop Environment® logo are registered trademarks of
//! KDE e.V. This preset is not affiliated with, endorsed by or sponsored by
//! KDE e.V.; the names KDE, Plasma and Breeze are used only to say which
//! design language it reproduces. It contains no KDE code, artwork, icon or
//! font. See `NOTICE` and `TRADEMARKS.md` in the Teksilo repository.
//!
//! ## Known limitations
//!
//! - **Most widget chrome is the framework's.** This version installs
//!   Breeze styles for `Button`, `IconButton` and `SplitButton` only: every
//!   other widget keeps Teksilo's recipe style, painted in the Breeze
//!   colours, radii, focus ring, typography and motion.
//! - **A filled `SplitButton` is filled with the accent.** The widget labels
//!   its `Filled` and `Destructive` variants in the on-accent text and gives
//!   its style no say, so they are framed in the accent rather than as
//!   Breeze's default button, whose light fill would not carry that text in
//!   a dark scheme ([`styles::split_button`]).
//! - **The fonts are not Plasma's.** The text sizes and line boxes are Noto
//!   Sans and Hack's at Plasma's default sizes, drawn in Teksilo's bundled
//!   Inter and JetBrains Mono.
//! - **The high-contrast selection is light on a light window.** Under the
//!   OS high-contrast preference the selection fill carries the window's
//!   text at 7:1, which keeps it below 4.5:1 on the window (about 1.9:1 for
//!   Breeze Light, 2.2:1 for Breeze Dark): no colour reaches both.
//! - **Widget shapes are chosen at build time.** Switching between this and
//!   another preset at runtime re-tints instantly but keeps the shapes the
//!   tree was built with, a property of the styling system.

mod color;
pub mod geometry;
pub mod motion;
pub mod palette;
pub mod shape;
pub mod styles;
pub mod typography;

pub use palette::{BreezeColors, KdePalette, PlasmaAccent, UnknownPlasmaAccent};
pub use teksilo_kde_scheme as scheme;

use std::rc::Rc;

use teksilo_core::presets::intui;
use teksilo_core::styles::{DensityProjection, PaintProjection, PaintState, Theme};
use teksilo_kde_scheme::KdeColorScheme;
use teksilo_tokens::{Color, InputTokens, LayoutTokens, TargetDensity};

/// The Plasma release whose Breeze style this preset transcribes.
pub const BREEZE_VERSION: &str = "6.8.0";

/// `[M]` The spacing between sibling controls: Breeze's layout spacing.
pub const BREEZE_CONTROL_GAP: f32 = 6.0;
/// `[K]` The spacing between sections: Kirigami's large spacing.
pub const BREEZE_SECTION_GAP: f32 = 8.0;

/// **Breeze Light**, Plasma's default scheme.
pub fn light() -> Theme {
    from_scheme(&KdeColorScheme::breeze_light())
}

/// **Breeze Dark**.
pub fn dark() -> Theme {
    from_scheme(&KdeColorScheme::breeze_dark())
}

/// Breeze Light with `accent` applied as Plasma's colour settings apply it.
///
/// ```
/// use teksilo_theme_kde::{self as kde, PlasmaAccent};
/// let theme = kde::light_with_accent(PlasmaAccent::Green.color());
/// assert!(!theme.is_dark());
/// ```
pub fn light_with_accent(accent: Color) -> Theme {
    from_scheme(&KdeColorScheme::breeze_light().with_accent(accent))
}

/// Breeze Dark with `accent` applied as Plasma's colour settings apply it.
pub fn dark_with_accent(accent: Color) -> Theme {
    from_scheme(&KdeColorScheme::breeze_dark().with_accent(accent))
}

/// The Breeze theme of any KDE colour scheme.
///
/// Light or dark, and the id `kde.light` or `kde.dark`, as KDE judges the
/// scheme ([`KdeColorScheme::is_dark`]). The scheme's colours, frame
/// contrast and inactive and disabled effects all come through; geometry,
/// typography and motion are Breeze's.
pub fn from_scheme(scheme: &KdeColorScheme) -> Theme {
    build(KdePalette::from_scheme(scheme))
}

fn build(palette: KdePalette) -> Theme {
    // Starting from Int UI keeps the input tokens, the density ladder, at
    // the framework's Compact baseline; every other token group is replaced.
    let mut theme = if palette.is_dark() {
        intui::dark().with_id("kde.dark")
    } else {
        intui::light().with_id("kde.light")
    };
    theme.colors = palette.tokens(ACTIVE).clone();
    theme.layout = LayoutTokens {
        control_gap: BREEZE_CONTROL_GAP,
        section_gap: BREEZE_SECTION_GAP,
    };
    theme.typography = typography::breeze_typography();
    theme.shape = shape::breeze_shape();
    theme.motion = motion::breeze_motion();
    theme.extensions.insert(palette);
    install_styles(&mut theme);
    theme.extensions.insert(DensityProjection(reproject));
    theme.extensions.insert(PaintProjection(project));
    theme
}

/// The paint state of an active window without the high-contrast
/// preference.
const ACTIVE: PaintState = PaintState {
    window_active: true,
    high_contrast: false,
};

/// Re-derive a Breeze theme for another density.
///
/// Registered as the theme's [`DensityProjection`]. `[unpublished]` KDE
/// publishes no touch ladder, so the input tokens are the framework's
/// generic [`InputTokens::for_density`] ladder; colours, id, the
/// [`KdePalette`] and both projections ride across untouched, and the
/// slots this preset installs are rebuilt for the new tokens.
fn reproject(base: &Theme, density: TargetDensity) -> Theme {
    let mut theme = base.clone();
    theme.input = InputTokens::for_density(density);
    install_styles(&mut theme);
    theme
}

/// Re-colour a Breeze theme for a window state and the high-contrast
/// preference.
///
/// Registered as the theme's [`PaintProjection`]. While the theme's colours
/// are still the active tokens of its [`KdePalette`], the state's own tokens
/// are painted ([`KdePalette::tokens`]). A theme whose colours an app has
/// changed since, or one without the palette, gets the framework's generic
/// inactive and high-contrast projection of its own colours instead, so a
/// change the app made is never painted over.
fn project(theme: &Theme, state: PaintState) -> Theme {
    if state == ACTIVE {
        return theme.clone();
    }
    match theme.extension::<KdePalette>() {
        Some(palette) if theme.colors == *palette.tokens(ACTIVE) => Theme {
            colors: palette.tokens(state).clone(),
            ..theme.clone()
        },
        _ => {
            let mut colors = theme.colors.clone();
            if !state.window_active {
                colors = colors.for_inactive_window();
            }
            if state.high_contrast {
                colors = colors.for_high_contrast();
            }
            Theme {
                colors,
                ..theme.clone()
            }
        }
    }
}

/// Install the Breeze Tier-3 chrome.
///
/// The buttons are Breeze's own ([`styles`]); every other slot is left to
/// the widget's recipe style, which paints from the Breeze tokens, and the
/// crate's tests list the reason for each. Every style installed here reads
/// its metrics from the theme it builds under, `theme.input` included, so
/// [`reproject`] can rerun this alone.
fn install_styles(theme: &mut Theme) {
    let slots = &mut theme.style_slots;
    slots.button = Some(Rc::new(styles::button::BreezeButtonStyle));
    slots.icon_button = Some(Rc::new(styles::icon_button::BreezeIconButtonStyle));
    slots.split_button = Some(Rc::new(styles::split_button::BreezeSplitButtonStyle));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::Duration;
    use teksilo_core::styles::{ComponentStyleSlots, ThemeAppearance};
    use teksilo_kde_scheme::math::{hsv_darker, hsv_lighter, mix};
    use teksilo_kde_scheme::{
        BackgroundRole, ColorSet, ColorState, DecorationRole, ForegroundRole, KdeColor, PaletteRole,
    };
    use teksilo_tokens::contrast::composite_contrast;
    use teksilo_tokens::{ColorTokens, Easing};

    /// Whether two themes are the same by content.
    ///
    /// `Theme`'s own equality compares style slots by pointer and extensions
    /// by type only, so it cannot tell two independently built themes
    /// apart. The destructuring has no `..`: a field added to `Theme` must
    /// be compared here before this compiles.
    fn assert_same_theme(a: &Theme, b: &Theme) {
        let Theme {
            id,
            appearance,
            colors,
            layout,
            typography,
            shape,
            motion,
            input,
            style_slots,
            extensions,
        } = a;
        assert_eq!(*id, b.id, "id");
        assert_eq!(*appearance, b.appearance, "appearance");
        assert_eq!(*colors, b.colors, "colors");
        assert_eq!(*layout, b.layout, "layout");
        assert_eq!(*typography, b.typography, "typography");
        assert_eq!(*shape, b.shape, "shape");
        assert_eq!(*motion, b.motion, "motion");
        assert_eq!(*input, b.input, "input");
        assert_eq!(
            style_slots.installed(),
            b.style_slots.installed(),
            "installed slots"
        );
        assert_eq!(
            extensions.get::<KdePalette>(),
            b.extension::<KdePalette>(),
            "KDE palette"
        );
    }

    fn paint(theme: &Theme, window_active: bool, high_contrast: bool) -> Theme {
        theme.for_paint_state(PaintState {
            window_active,
            high_contrast,
        })
    }

    fn hex8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

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
    fn ids_are_namespaced_and_follow_the_appearance() {
        assert_eq!(light().id.as_str(), "kde.light");
        assert_eq!(dark().id.as_str(), "kde.dark");
        // The widget catalog persists themes by this id.
        assert_eq!(
            light_with_accent(PlasmaAccent::Red.color()).id.as_str(),
            "kde.light"
        );
        assert_eq!(
            dark_with_accent(PlasmaAccent::Red.color()).id.as_str(),
            "kde.dark"
        );
        assert_eq!(
            from_scheme(&KdeColorScheme::breeze_classic()).id.as_str(),
            "kde.light"
        );
    }

    #[test]
    fn from_scheme_of_breeze_light_is_light() {
        assert_same_theme(&from_scheme(&KdeColorScheme::breeze_light()), &light());
        assert_same_theme(&from_scheme(&KdeColorScheme::breeze_dark()), &dark());
    }

    #[test]
    fn breezes_own_blue_as_an_accent_still_changes_the_colours() {
        // Plasma's accent application blends the selection from the accent
        // over the view, so applying Breeze's own blue lightens it.
        let accented = light_with_accent(Color::from_hex("#3daee9"));
        let base = light();
        assert_ne!(accented.colors, base.colors);
        assert_eq!(hex8(accented.colors.accent), [119, 198, 239, 255]);
        assert_eq!(hex8(base.colors.accent), [61, 174, 233, 255]);
        // Everything that is not a colour is the same.
        let Theme {
            id,
            appearance,
            layout,
            typography,
            shape,
            motion,
            input,
            ..
        } = &accented;
        assert_eq!(*id, base.id);
        assert_eq!(*appearance, base.appearance);
        assert_eq!(*layout, base.layout);
        assert_eq!(*typography, base.typography);
        assert_eq!(*shape, base.shape);
        assert_eq!(*motion, base.motion);
        assert_eq!(*input, base.input);
    }

    #[test]
    fn surfaces_are_the_breeze_window_backgrounds() {
        assert_eq!(hex8(light().colors.surface_main), [239, 240, 241, 255]);
        assert_eq!(hex8(dark().colors.surface_main), [32, 35, 38, 255]);
        assert_eq!(hex8(light().colors.surface_content), [255, 255, 255, 255]);
        assert_eq!(hex8(dark().colors.surface_content), [20, 22, 24, 255]);
    }

    #[test]
    fn the_accent_is_the_highlight_with_plasmas_black_text() {
        for t in [light(), dark()] {
            assert_eq!(hex8(t.colors.accent), [61, 174, 233, 255]);
            assert_eq!(t.colors.text_on_accent, Color::BLACK);
        }
    }

    #[test]
    fn geometry_is_the_breeze_frame_radius_and_ring() {
        for t in [light(), dark()] {
            assert_eq!(t.shape.radius_control, 5.0);
            assert_eq!(t.shape.radius_popup, 5.0);
            assert_eq!(t.shape.focus_ring_width, 2.0);
            assert_eq!(t.shape.focus_ring_offset, -1.0);
            assert_eq!(t.layout.control_gap, 6.0);
            assert_eq!(t.layout.section_gap, 8.0);
        }
    }

    #[test]
    fn body_is_qts_thirteen_pixel_face_on_an_eighteen_pixel_line() {
        let t = light();
        assert_eq!(t.typography.body.size, 13.0);
        assert!((t.typography.body.size * t.typography.body.line_height - 18.0).abs() < 1e-4);
        assert_eq!(t.typography.body.letter_spacing, 0.0);
    }

    #[test]
    fn motion_is_breezes_linear_hundred_milliseconds() {
        for t in [light(), dark()] {
            assert_eq!(t.motion.duration_fast, Duration::from_millis(100));
            assert_eq!(t.motion.duration_normal, Duration::from_millis(200));
            assert_eq!(t.motion.easing_standard, Easing::Linear);
        }
    }

    #[test]
    fn the_palette_extension_is_present_and_matches_the_appearance() {
        let l = light();
        let d = dark();
        let lp = l.extension::<KdePalette>().expect("light palette");
        let dp = d.extension::<KdePalette>().expect("dark palette");
        assert!(!lp.is_dark());
        assert!(dp.is_dark());
        assert_eq!(
            lp.role(ColorState::Active, PaletteRole::Window),
            l.colors.surface_main
        );
        assert!((lp.frame_contrast() - 0.2).abs() < 1e-12);
    }

    /// Every slot this commit leaves to the widget's own recipe style, with
    /// the reason it is inherited rather than filled.
    const KDE_UNFILLED: &[(&str, &str)] = &[
        (
            "splitter",
            "the recipe's thin divider in the border role reads as Breeze's 1 px splitter handle",
        ),
        (
            "toggle",
            "no Breeze switch is installed; the recipe switch reads the Breeze accent and border tokens",
        ),
        (
            "checkbox",
            "no Breeze check box is installed; the recipe box reads the Breeze accent and strong border tokens",
        ),
        (
            "radio",
            "no Breeze radio button is installed; the recipe ring reads the Breeze accent and strong border tokens",
        ),
        (
            "radio_tile",
            "Breeze has no selectable card; the recipe surface reads the preset's surface and accent roles",
        ),
        (
            "slider",
            "no Breeze slider is installed; the recipe track and knob read the Breeze accent and border tokens",
        ),
        (
            "text_input",
            "no Breeze line-edit frame is installed; the recipe field reads the Breeze content, border and focus tokens",
        ),
        (
            "combo_box",
            "no Breeze combo box is installed; the recipe paints from the Breeze field and border tokens",
        ),
        (
            "menu_item",
            "no Breeze menu row is installed; the recipe row paints its highlight from the Breeze selection tokens",
        ),
        (
            "panel",
            "the recipe panel paints the Breeze surface and border tokens with the Breeze radius",
        ),
        (
            "card",
            "Breeze has no card; the recipe card paints the Breeze raised surface and border tokens",
        ),
        (
            "chart",
            "Breeze has no chart chrome; the teksilo-charts default reads the series colours and the Breeze border role",
        ),
        (
            "popover",
            "no Breeze popup frame is installed; the recipe popover paints the Breeze raised surface, outline and shadow tokens",
        ),
        (
            "tooltip",
            "no Breeze tooltip frame is installed; the recipe tooltip paints the Breeze tooltip tokens",
        ),
        (
            "scroll_bar",
            "no Breeze scroll bar is installed; the recipe paints the Breeze thumb tokens over no track",
        ),
        (
            "standard_item",
            "no Breeze row highlight is installed; the recipe row paints the Breeze hover and selection tokens",
        ),
        (
            "tab",
            "no Breeze tab is installed; the recipe tab paints the Breeze surface and accent tokens",
        ),
        (
            "dialog",
            "the recipe dialog paints the Breeze surfaces, radius and shadow tokens",
        ),
        (
            "snackbar",
            "Breeze has no snackbar; the recipe paints the Breeze raised surface and text tokens",
        ),
        (
            "toast",
            "Breeze has no toast; the recipe paints the Breeze raised surface and status tokens",
        ),
        (
            "banner",
            "the recipe banner paints the Breeze status foregrounds and their computed backgrounds",
        ),
        (
            "badge",
            "Breeze has no badge; the recipe badge paints the Breeze accent and status tokens",
        ),
        (
            "progress_bar",
            "no Breeze progress groove is installed; the recipe bar paints the Breeze accent token",
        ),
        (
            "link",
            "the recipe link paints the Breeze link tokens, lifted to the text floor",
        ),
        (
            "segmented_control",
            "Breeze has no segmented control; the recipe paints the Breeze surface, accent and border tokens",
        ),
        (
            "avatar",
            "Breeze has no avatar; the recipe paints the Breeze accent and surface tokens",
        ),
        (
            "calendar",
            "the recipe calendar paints the Breeze selection, hover and text tokens",
        ),
        (
            "color_picker",
            "Breeze has no in-window colour picker chrome; the recipe reads the theme roles",
        ),
        (
            "spin_box",
            "no Breeze spin box is installed; the recipe frames the field and step buttons in the Breeze roles",
        ),
        (
            "date_edit",
            "the recipe is a passthrough around the TextInput, whose slot this preset leaves to its recipe",
        ),
        (
            "rich_text_editor",
            "the recipe frames the viewport in the Breeze border and surface roles; the text is document content",
        ),
        (
            "table",
            "no Breeze table rows are installed; the recipe paints the Breeze selection, alternate-row and divider tokens",
        ),
        (
            "list_container",
            "the recipe is only the drag-insertion line, drawn in the Breeze accent",
        ),
        (
            "drop_zone",
            "Breeze has no drop-zone control; the recipe tints its region from the theme roles",
        ),
        (
            "drop_target",
            "Breeze has no drop-target control; the recipe border reads the theme roles",
        ),
        (
            "grid_view",
            "the recipe's focus ring, marquee and insertion bar use the focus and accent roles this preset maps",
        ),
        (
            "web_view",
            "the page paints itself; the recipe only tints the overlay shown over it, from theme roles",
        ),
        (
            "text_selection",
            "touch chrome Breeze does not define; its geometry is teksilo_core::text_touch's, shared with hit testing",
        ),
        (
            "search_field",
            "the recipe field reads the Breeze content, border and focus tokens",
        ),
    ];

    /// Whether the installed slots and [`KDE_UNFILLED`] are exactly
    /// [`ComponentStyleSlots::NAMES`], with no slot in both.
    #[test]
    fn installed_and_unfilled_slots_partition_the_roster() {
        let unfilled: BTreeSet<&str> = KDE_UNFILLED.iter().map(|(name, _)| *name).collect();
        assert_eq!(unfilled.len(), KDE_UNFILLED.len(), "a slot is listed twice");
        for (name, reason) in KDE_UNFILLED {
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
    fn an_accent_moves_the_accent_family_and_the_links_but_no_neutral() {
        let t = light_with_accent(PlasmaAccent::Purple.color());
        let base = light();
        assert_ne!(t.colors.accent, base.colors.accent);
        assert_ne!(t.colors.focus_ring, base.colors.focus_ring);
        assert_ne!(t.colors.surface_selected, base.colors.surface_selected);
        // Plasma writes the accent into the link colour too, unlike macOS.
        assert_ne!(t.colors.text_link, base.colors.text_link);
        assert_eq!(t.colors.surface_main, base.colors.surface_main);
        assert_eq!(t.colors.surface_content, base.colors.surface_content);
        assert_eq!(t.colors.text_primary, base.colors.text_primary);
        let palette = t.extension::<KdePalette>().unwrap();
        let [r, g, b, _] = palette
            .resolved()
            .set(ColorState::Active, ColorSet::View)
            .decoration(DecorationRole::Focus)
            .to_rgba8();
        assert_eq!([r, g, b], [184, 117, 220]);
    }

    #[test]
    fn every_swatch_builds_a_usable_theme() {
        for accent in PlasmaAccent::ALL {
            for t in [
                light_with_accent(accent.color()),
                dark_with_accent(accent.color()),
            ] {
                let c = &t.colors;
                for fill in [c.accent, c.accent_hover, c.accent_pressed] {
                    assert!(
                        composite_contrast(c.text_on_accent, fill) >= 4.5,
                        "{accent:?}: the accent label is {:.2}:1",
                        composite_contrast(c.text_on_accent, fill)
                    );
                }
                assert!(t.extension::<KdePalette>().is_some());
            }
        }
    }

    #[test]
    fn the_inactive_projection_paints_the_schemes_muted_selection() {
        for t in [light(), dark()] {
            let palette = t.extension::<KdePalette>().unwrap();
            let inactive = paint(&t, false, false);
            let pi = palette.role(ColorState::Inactive, PaletteRole::Highlight);
            // KDE's Inactive state: the window colour tinted toward the
            // selection colour.
            assert_eq!(inactive.colors.accent, pi);
            assert_ne!(inactive.colors.accent, t.colors.accent);
            let chroma = |c: Color| {
                let (_, s, _) = c.to_hsv();
                s
            };
            assert!(chroma(inactive.colors.accent) < chroma(t.colors.accent));
            // Breeze's inactive effects are off: the neutrals pass through
            // and the focus ring keeps the focus decoration.
            assert_eq!(inactive.colors.surface_main, t.colors.surface_main);
            assert_eq!(inactive.colors.text_primary, t.colors.text_primary);
            assert_eq!(inactive.colors.focus_ring, t.colors.focus_ring);
            assert_eq!(
                inactive.colors,
                *palette.tokens(PaintState {
                    window_active: false,
                    high_contrast: false,
                })
            );
        }
        // The Breeze Light example of the specification: 194,224,245.
        assert_eq!(
            hex8(paint(&light(), false, false).colors.accent),
            [194, 224, 245, 255]
        );
    }

    #[test]
    fn a_scheme_with_inactive_effects_is_honoured() {
        let mut scheme = KdeColorScheme::breeze_light();
        scheme.inactive_effect.enabled = true;
        scheme.inactive_effect.intensity_effect = teksilo_kde_scheme::IntensityEffect::Darken;
        scheme.inactive_effect.intensity_amount = 0.2;
        let t = from_scheme(&scheme);
        let inactive = paint(&t, false, false);
        assert_ne!(inactive.colors.surface_main, t.colors.surface_main);
        let palette = t.extension::<KdePalette>().unwrap();
        assert_eq!(
            inactive.colors.surface_main,
            palette.role(ColorState::Inactive, PaletteRole::Window)
        );
    }

    #[test]
    fn a_colour_the_app_changed_is_never_painted_over() {
        let mut t = light();
        t.colors.accent = Color::from_hex("#FF0000");
        let inactive = paint(&t, false, false);
        assert_eq!(inactive.colors, t.colors.for_inactive_window());
        assert_eq!(paint(&t, true, false).colors, t.colors);
        let mut bare = light();
        bare.extensions.remove::<KdePalette>();
        assert_eq!(
            paint(&bare, true, true).colors,
            bare.colors.for_high_contrast()
        );
    }

    #[test]
    fn the_active_paint_state_is_the_theme_itself() {
        let t = light();
        assert_same_theme(&paint(&t, true, false), &t);
    }

    #[test]
    fn a_density_switch_keeps_the_colours_and_both_projections() {
        let base = light();
        let touch = base.with_density(TargetDensity::Touch);
        assert_eq!(touch.input, InputTokens::for_density(TargetDensity::Touch));
        assert_eq!(touch.input.density, TargetDensity::Touch);
        assert_eq!(touch.id, base.id);
        assert_eq!(touch.colors, base.colors);
        assert_eq!(
            touch.extension::<KdePalette>(),
            base.extension::<KdePalette>()
        );
        assert!(touch.extension::<DensityProjection>().is_some());
        assert!(touch.extension::<PaintProjection>().is_some());
        // The projection still works on the re-derived theme.
        assert_eq!(
            paint(&touch, false, false).colors,
            paint(&base, false, false).colors
        );
    }

    /// Every colour token, pinned to its source in Breeze Light's resolved
    /// palette. The destructuring has no `..`: a field added to
    /// `ColorTokens` must be pinned here before this compiles.
    ///
    /// A lifted token is pinned by its rule: it keeps its source's hue,
    /// reaches its floor, and equals the source where the source already
    /// does (checked in Breeze Dark, where most of them pass).
    #[test]
    fn every_colour_token_comes_from_its_documented_source() {
        let t = light();
        let p = t.extension::<KdePalette>().unwrap().clone();
        let r = |role| p.resolved().role(ColorState::Active, role);
        let d = |role| p.resolved().role(ColorState::Disabled, role);
        let i = |role| p.resolved().role(ColorState::Inactive, role);
        let view = *p.resolved().set(ColorState::Active, ColorSet::View);
        let win = *p.resolved().set(ColorState::Active, ColorSet::Window);
        let tip = *p.resolved().set(ColorState::Active, ColorSet::Tooltip);
        let c = |k: KdeColor| k.to_color();
        let window = r(PaletteRole::Window);
        let base = r(PaletteRole::Base);
        let frame_background = mix(window, base, 0.3);
        let outline = mix(window, r(PaletteRole::WindowText), 0.2);
        let hover_wash = mix(base, r(PaletteRole::Highlight), 0.3);
        let readable = |token: Color, source: KdeColor, floor: f32, on: &[Color]| {
            let (h1, s1, _) = token.to_hsv();
            let (h2, s2, _) = c(source).to_hsv();
            if s1 > 0.05 && s2 > 0.05 {
                let dh = (h1 - h2).abs().min(360.0 - (h1 - h2).abs());
                assert!(dh < 6.0, "{token:?} lost the hue of {source:?}");
            }
            for bg in on {
                assert!(
                    composite_contrast(token, *bg) >= floor,
                    "{token:?} from {source:?} is {:.2}:1 on {bg:?}",
                    composite_contrast(token, *bg)
                );
            }
        };
        let surfaces = [t.colors.surface_main, t.colors.surface_content];
        let kept = ColorTokens::light_default();

        let ColorTokens {
            surface_main,
            surface_content,
            surface_raised,
            surface_sunken,
            surface_hover,
            surface_pressed,
            surface_selected,
            surface_selected_inactive,
            surface_alt_row,
            surface_disabled,
            text_primary,
            text_secondary,
            text_disabled,
            text_on_accent,
            text_link,
            text_link_hover,
            text_link_visited,
            text_error,
            text_warning,
            text_success,
            accent,
            accent_hover,
            accent_pressed,
            accent_disabled,
            accent_subtle_bg,
            border,
            border_strong,
            border_focused,
            border_error,
            border_warning,
            border_disabled,
            divider,
            divider_strong,
            status_info_fg,
            status_info_bg,
            status_success_fg,
            status_success_bg,
            status_warning_fg,
            status_warning_bg,
            status_error_fg,
            status_error_bg,
            selection_bg_active,
            selection_text_active,
            selection_bg_inactive,
            selection_text_inactive,
            search_match_bg,
            search_match_current_bg,
            scrollbar_track,
            scrollbar_track_hover,
            scrollbar_thumb,
            scrollbar_thumb_hover,
            scrollbar_thumb_pressed,
            tooltip_bg,
            tooltip_text,
            tooltip_border,
            tooltip_shortcut,
            editor_bg,
            editor_fg,
            editor_caret,
            editor_current_line_bg,
            editor_gutter_fg,
            editor_selection_bg,
            editor_code_block_bg,
            editor_code_block_fg,
            focus_ring,
            focus_ring_error,
            scrim,
            text_on_error,
            text_on_error_container,
            surface_error_container,
            surface_container,
            surface_container_raised,
            surface_container_sunken,
            chart_palette,
        } = t.colors.clone();

        // Surfaces.
        assert_eq!(surface_main, c(window));
        assert_eq!(surface_content, c(base));
        assert_eq!(surface_raised, c(frame_background));
        assert_eq!(surface_sunken, c(win.background(BackgroundRole::Alternate)));
        assert_eq!(surface_hover, c(hover_wash));
        assert_eq!(
            surface_pressed,
            c(mix(r(PaletteRole::Button), r(PaletteRole::Highlight), 0.3))
        );
        assert_eq!(surface_selected, c(r(PaletteRole::Highlight)));
        assert_eq!(surface_selected_inactive, c(i(PaletteRole::Highlight)));
        assert_eq!(surface_alt_row, c(r(PaletteRole::AlternateBase)));
        assert_eq!(surface_disabled, c(d(PaletteRole::Button)));
        assert_eq!(surface_container, c(frame_background));
        assert_eq!(surface_container_raised, c(base));
        assert_eq!(surface_container_sunken, surface_sunken);
        assert_eq!(
            surface_error_container,
            c(view.background(BackgroundRole::Negative))
        );

        // Text.
        assert_eq!(text_primary, c(r(PaletteRole::WindowText)));
        readable(
            text_secondary,
            win.foreground(ForegroundRole::Inactive),
            4.5,
            &surfaces,
        );
        assert_eq!(text_disabled, c(d(PaletteRole::WindowText)));
        assert_eq!(text_on_accent, Color::BLACK);
        readable(
            text_link,
            view.foreground(ForegroundRole::Link),
            4.5,
            &surfaces,
        );
        readable(
            text_link_hover,
            view.foreground(ForegroundRole::Active),
            4.5,
            &surfaces,
        );
        readable(
            text_link_visited,
            view.foreground(ForegroundRole::Visited),
            4.5,
            &surfaces,
        );
        readable(
            text_error,
            view.foreground(ForegroundRole::Negative),
            4.5,
            &surfaces,
        );
        readable(
            text_warning,
            view.foreground(ForegroundRole::Neutral),
            4.5,
            &surfaces,
        );
        readable(
            text_success,
            view.foreground(ForegroundRole::Positive),
            4.5,
            &surfaces,
        );
        assert_eq!(text_on_error, Color::WHITE);
        assert_eq!(text_on_error_container, text_error);

        // Accent.
        assert_eq!(accent, c(r(PaletteRole::Highlight)));
        assert_eq!(accent_hover, c(hsv_lighter(r(PaletteRole::Highlight), 110)));
        assert_eq!(
            accent_pressed,
            c(hsv_darker(r(PaletteRole::Highlight), 110))
        );
        assert_eq!(accent_disabled, c(d(PaletteRole::Highlight)));
        assert_eq!(accent_subtle_bg, c(hover_wash));

        // Borders.
        assert_eq!(border, c(outline));
        readable(border_strong, outline, 3.0, &surfaces);
        assert_eq!(divider, c(outline));
        assert_eq!(divider_strong, border_strong);
        readable(
            focus_ring,
            view.decoration(DecorationRole::Focus),
            3.0,
            &surfaces,
        );
        assert_eq!(border_focused, focus_ring);
        readable(
            border_error,
            view.foreground(ForegroundRole::Negative),
            3.0,
            &surfaces,
        );
        readable(
            border_warning,
            view.foreground(ForegroundRole::Neutral),
            3.0,
            &surfaces,
        );
        assert_eq!(focus_ring_error, border_error);
        assert_eq!(
            border_disabled,
            c(mix(d(PaletteRole::Window), d(PaletteRole::WindowText), 0.2))
        );

        // Status.
        assert_eq!(status_error_fg, text_error);
        assert_eq!(status_warning_fg, text_warning);
        assert_eq!(status_success_fg, text_success);
        readable(
            status_info_fg,
            view.foreground(ForegroundRole::Active),
            4.5,
            &surfaces,
        );
        assert_eq!(status_info_bg, c(view.background(BackgroundRole::Active)));
        assert_eq!(
            status_success_bg,
            c(view.background(BackgroundRole::Positive))
        );
        assert_eq!(
            status_warning_bg,
            c(view.background(BackgroundRole::Neutral))
        );
        assert_eq!(
            status_error_bg,
            c(view.background(BackgroundRole::Negative))
        );

        // Selection.
        assert_eq!(selection_bg_active, surface_selected);
        assert_eq!(selection_text_active, Color::BLACK);
        assert_eq!(selection_bg_inactive, surface_selected_inactive);
        assert_eq!(selection_text_inactive, c(i(PaletteRole::HighlightedText)));
        assert_eq!(search_match_bg, kept.search_match_bg);
        assert_eq!(search_match_current_bg, kept.search_match_current_bg);

        // Scroll bar.
        assert_eq!(scrollbar_track, Color::TRANSPARENT);
        assert_eq!(scrollbar_track_hover, Color::TRANSPARENT);
        readable(
            scrollbar_thumb,
            mix(window, r(PaletteRole::WindowText), 0.175),
            3.0,
            &surfaces,
        );
        readable(
            scrollbar_thumb_hover,
            mix(window, view.decoration(DecorationRole::Hover), 0.5),
            3.0,
            &surfaces,
        );
        assert_eq!(scrollbar_thumb_pressed, scrollbar_thumb_hover);

        // Tooltip.
        let tip_bg = tip.background(BackgroundRole::Normal);
        let tip_fg = tip.foreground(ForegroundRole::Normal);
        assert_eq!(tooltip_bg, c(tip_bg));
        assert_eq!(tooltip_text, c(tip_fg));
        assert_eq!(tooltip_border, c(mix(tip_bg, tip_fg, 0.2)));
        readable(
            tooltip_shortcut,
            tip.foreground(ForegroundRole::Inactive),
            4.5,
            &[tooltip_bg],
        );

        // Editor.
        assert_eq!(editor_bg, c(base));
        assert_eq!(editor_fg, c(r(PaletteRole::Text)));
        assert_eq!(editor_caret, editor_fg);
        assert_eq!(editor_current_line_bg, c(r(PaletteRole::AlternateBase)));
        readable(
            editor_gutter_fg,
            view.foreground(ForegroundRole::Inactive),
            4.5,
            &[editor_bg, editor_current_line_bg],
        );
        assert_eq!(editor_selection_bg, surface_selected);
        assert_eq!(editor_code_block_bg, c(mix(base, window, 0.5)));
        assert_eq!(editor_code_block_fg, editor_fg);

        // Kept from Int UI.
        assert_eq!(scrim, kept.scrim);
        assert_eq!(chart_palette, kept.chart_palette);

        // Where the source already reaches its floor the lift leaves it
        // alone: Breeze Dark's tooltip shortcut text is readable as it is.
        let dark = super::dark();
        let dp = dark.extension::<KdePalette>().unwrap();
        let dark_shortcut = dp
            .resolved()
            .set(ColorState::Active, ColorSet::Tooltip)
            .foreground(ForegroundRole::Inactive);
        assert!(composite_contrast(dark_shortcut.to_color(), dark.colors.tooltip_bg) >= 4.5);
        assert_eq!(dark.colors.tooltip_shortcut, dark_shortcut.to_color());
    }

    /// No assignment may stay tagged as unverified: the tag means a value
    /// the specification has not settled.
    #[test]
    fn no_unverified_tags() {
        let needle = concat!("[", "unverified", "]");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        let mut scanned = 0;
        let mut stack = vec![root.join("src"), root.join("tests")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    scanned += 1;
                    let text = std::fs::read_to_string(&path).expect("readable source");
                    for (n, line) in text.lines().enumerate() {
                        if line.contains(needle) {
                            found.push(format!("{}:{}", path.display(), n + 1));
                        }
                    }
                }
            }
        }
        assert!(scanned > 5, "only {scanned} source files were scanned");
        assert!(
            found.is_empty(),
            "unverified values remain:\n{}",
            found.join("\n")
        );
    }
}
