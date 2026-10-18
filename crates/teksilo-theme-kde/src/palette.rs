// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The KDE palette a Breeze theme is built from, attached to the theme as a
//! typed extension, and Plasma's ten accent swatches.
//!
//! [`ColorTokens`] carries one colour per Teksilo role. A KDE scheme carries
//! more: three colour states of seven colour sets, each with its own
//! foregrounds, backgrounds and decorations, and Breeze paints controls from
//! combinations of them (an outline is the window colour moved toward the
//! window text by the frame contrast). [`KdePalette`] holds the whole
//! resolved scheme, those combinations for each state ([`BreezeColors`]),
//! and the four sets of colour tokens a window paints with, so a Breeze
//! style or KDE-aware app code can read the exact colour it needs:
//!
//! ```
//! use teksilo_theme_kde::{self as kde, KdePalette};
//! use teksilo_theme_kde::scheme::ColorState;
//!
//! let theme = kde::light();
//! let palette = theme.extension::<KdePalette>().expect("a KDE theme carries its palette");
//! let outline = palette.colors(ColorState::Active).frame_outline;
//! assert!(outline.a() > 0.0);
//! ```
//!
//! The resolved scheme ([`KdePalette::resolved`], [`KdePalette::role`])
//! keeps the scheme's own colours. Where the theme's tokens lift one to meet
//! WCAG (see the colour mapping's deviations in the crate documentation),
//! the token carries the lifted colour and the resolved scheme the vendor
//! one. Breeze's combinations are made from the surfaces the window paints:
//! where the mapping moves a background back from the middle of the scale
//! (D11), they are made from the moved one, so a style painting a button
//! face paints the surface the text was lifted on.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use teksilo_core::styles::PaintState;
use teksilo_kde_scheme::math::mix;
use teksilo_kde_scheme::{
    ACCENT_SWATCHES, ColorSet, ColorState, DecorationRole, KdeColor, KdeColorScheme, PaletteRole,
    ResolvedPalette,
};
use teksilo_tokens::{Color, ColorTokens};

use crate::color::{self, Mapping};

/// `[Hp]` How far a focus or hover outline is moved toward the window text.
const OUTLINE_TOWARD_TEXT: f64 = 0.15;

/// `[Hp]` The alpha of a control's 1 px shadow.
const CONTROL_SHADOW_ALPHA: f32 = 0.125;

/// The colours Breeze combines from a scheme's palette for one colour state.
///
/// `fc` is the scheme's frame contrast. Every colour is opaque except
/// [`shadow`](Self::shadow).
///
/// The combinations are made from the surfaces the window paints, which
/// are the scheme's own except where the colour mapping's D11 moves them
/// (an Active or Inactive state whose backgrounds lie in the middle of the
/// scale, under the scheme's inactive effects or the high-contrast floor):
/// the window, the view and the button colour are the colour tokens'
/// `surface_main`, `surface_content` and the button surface their text is
/// lifted on, the frame background is `surface_raised` and the frame
/// outline `border`. The button text is lifted the same way the window's
/// text is (D10). The Disabled state keeps the scheme's colours: its text
/// is exempt from WCAG's contrast floors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeColors {
    /// `[Hp]` The outline of a frame, field or menu: the window colour moved
    /// toward the window text by `fc`.
    pub frame_outline: Color,
    /// `[Hp]` A separator line: the same mix as the frame outline.
    pub separator: Color,
    /// `[Hp]` The fill of a menu, popup or group box: the window colour 30 %
    /// of the way to the view's.
    pub frame_background: Color,
    /// `[Hp]` The view set's focus decoration, the keyboard-focus colour.
    pub focus: Color,
    /// `[Hp]` The view set's hover decoration.
    pub hover: Color,
    /// `[Hp]` The focus colour moved 15 % toward the window text.
    pub focus_outline: Color,
    /// `[Hp]` The hover colour moved 15 % toward the window text.
    pub hover_outline: Color,
    /// `[qt]` The button colour, a push button's face.
    pub button: Color,
    /// `[qt]` The text of a button.
    pub button_text: Color,
    /// `[Hp]` A push button's outline: the button colour moved toward the
    /// button text by `fc`.
    pub button_outline: Color,
    /// `[Hp]` A slider handle's outline: the same mix as the button outline.
    pub slider_outline: Color,
    /// `[Hp]` A control's shadow: black at 12.5 %.
    pub shadow: Color,
}

impl BreezeColors {
    /// The combinations for `state` of `palette` at frame contrast `fc`,
    /// their surfaces carried to the text floor of the high-contrast
    /// projection (7:1) when `high_contrast` is set and to 4.5:1 otherwise.
    pub fn compute(
        palette: &ResolvedPalette,
        state: ColorState,
        fc: f64,
        high_contrast: bool,
    ) -> BreezeColors {
        let role = |r: PaletteRole| palette.role(state, r);
        let window_text = role(PaletteRole::WindowText);
        let vendor_button_text = role(PaletteRole::ButtonText);
        let (window, button, button_text, frame_background) = if state == ColorState::Disabled {
            (
                role(PaletteRole::Window),
                role(PaletteRole::Button),
                vendor_button_text,
                mix(role(PaletteRole::Window), role(PaletteRole::Base), 0.3),
            )
        } else {
            // `[wcag]` D11, the surfaces the colour tokens paint, and D10 for
            // the button text on the button.
            let floor = if high_contrast {
                color::ENHANCED
            } else {
                color::AA
            }
            .text;
            let s = color::Surfaces::of(palette, state, floor);
            (
                s.window,
                s.button,
                color::lift_toward(vendor_button_text, &[s.button], floor, s.text_side()),
                s.frame_background,
            )
        };
        let view = palette.set(state, ColorSet::View);
        let focus = view.decoration(DecorationRole::Focus);
        let hover = view.decoration(DecorationRole::Hover);
        let outline = mix(window, window_text, fc).to_color();
        let button_outline = mix(button, button_text, fc).to_color();
        BreezeColors {
            frame_outline: outline,
            separator: outline,
            frame_background: frame_background.to_color(),
            focus: focus.to_color(),
            hover: hover.to_color(),
            focus_outline: mix(focus, window_text, OUTLINE_TOWARD_TEXT).to_color(),
            hover_outline: mix(hover, window_text, OUTLINE_TOWARD_TEXT).to_color(),
            button: button.to_color(),
            button_text: button_text.to_color(),
            button_outline,
            slider_outline: button_outline,
            shadow: Color::BLACK.with_alpha(CONTROL_SHADOW_ALPHA),
        }
    }
}

/// A resolved KDE scheme and everything a Breeze theme paints from it.
///
/// Installed on a theme by [`crate::from_scheme`] and every constructor
/// built on it, and read back with `theme.extension::<KdePalette>()`. Two
/// palettes are equal when their contents are.
///
/// It also carries the four sets of colour tokens a window paints with: the
/// active and inactive ones, and each under the high-contrast preference.
/// The theme's paint projection swaps them in (see [`KdePalette::tokens`]).
#[derive(Debug, Clone, PartialEq)]
pub struct KdePalette {
    resolved: Arc<ResolvedPalette>,
    frame_contrast: f64,
    dark: bool,
    /// Indexed by [`ColorState::index`].
    colors: [BreezeColors; 3],
    /// The same for the high-contrast scheme, indexed alike.
    high_contrast_colors: [BreezeColors; 3],
    active: ColorTokens,
    inactive: ColorTokens,
    high_contrast: ColorTokens,
    inactive_high_contrast: ColorTokens,
}

impl KdePalette {
    /// The palette of `scheme`.
    ///
    /// The scheme's frame contrast is read clamped to `0..=1`, its default
    /// 0.2 when it is not a number, as KDE reads the key. The high-contrast
    /// tokens come from the same scheme with a frame contrast of 1 and a
    /// contrast of 10.
    pub fn from_scheme(scheme: &KdeColorScheme) -> KdePalette {
        let frame_contrast = if scheme.frame_contrast.is_nan() {
            teksilo_kde_scheme::scheme::DEFAULT_FRAME_CONTRAST
        } else {
            scheme.frame_contrast.clamp(0.0, 1.0)
        };
        let dark = scheme.is_dark();
        let resolved = scheme.resolve();

        let mut enhanced = scheme.clone();
        enhanced.frame_contrast = 1.0;
        enhanced.contrast = 10;
        let enhanced_resolved = enhanced.resolve();

        let project = |palette: &ResolvedPalette, state, fc, high_contrast| {
            color::tokens(&Mapping {
                palette,
                state,
                frame_contrast: fc,
                high_contrast,
                dark,
            })
        };
        let active = project(&resolved, ColorState::Active, frame_contrast, false);
        let mut inactive = project(&resolved, ColorState::Inactive, frame_contrast, false);
        let high_contrast = project(&enhanced_resolved, ColorState::Active, 1.0, true);
        let mut inactive_high_contrast =
            project(&enhanced_resolved, ColorState::Inactive, 1.0, true);
        // The chart series keep the framework's inactive desaturation: KDE's
        // inactive state has nothing to say about them.
        inactive.chart_palette = active.for_inactive_window().chart_palette;
        inactive_high_contrast.chart_palette = high_contrast.for_inactive_window().chart_palette;

        KdePalette {
            colors: ColorState::ALL
                .map(|state| BreezeColors::compute(&resolved, state, frame_contrast, false)),
            high_contrast_colors: ColorState::ALL
                .map(|state| BreezeColors::compute(&enhanced_resolved, state, 1.0, true)),
            resolved: Arc::new(resolved),
            frame_contrast,
            dark,
            active,
            inactive,
            high_contrast,
            inactive_high_contrast,
        }
    }

    /// The resolved scheme: every colour of every set in every state.
    pub fn resolved(&self) -> &ResolvedPalette {
        &self.resolved
    }

    /// The palette colour of `role` in `state`, as a Teksilo colour.
    pub fn role(&self, state: ColorState, role: PaletteRole) -> Color {
        self.resolved.role(state, role).to_color()
    }

    /// The scheme's frame contrast, in `0..=1`.
    pub fn frame_contrast(&self) -> f64 {
        self.frame_contrast
    }

    /// Whether the scheme is dark, as KDE reports it.
    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// Breeze's colour combinations for `state`.
    pub fn colors(&self, state: ColorState) -> &BreezeColors {
        &self.colors[state.index()]
    }

    /// Breeze's colour combinations for `state`, from the high-contrast
    /// scheme when `high_contrast` is set: the scheme at a frame contrast of
    /// 1 and a contrast of 10, the one the high-contrast tokens come from,
    /// where an outline is the text colour itself.
    pub fn breeze_colors(&self, state: ColorState, high_contrast: bool) -> &BreezeColors {
        if high_contrast {
            &self.high_contrast_colors[state.index()]
        } else {
            &self.colors[state.index()]
        }
    }

    /// The colour tokens a window paints with in `state`.
    ///
    /// An active window paints the active tokens. An inactive one paints the
    /// tokens of the scheme's Inactive state: for Breeze, whose inactive
    /// effects are off, they differ only in the selection and the accent,
    /// which KDE mutes to the window colour tinted 40 % toward the selection
    /// colour, while the focus ring keeps the focus decoration. A scheme
    /// that turns its inactive effects on gets them everywhere. Under the
    /// high-contrast preference the same choice is made between the
    /// high-contrast tokens.
    pub fn tokens(&self, state: PaintState) -> &ColorTokens {
        match (state.window_active, state.high_contrast) {
            (true, false) => &self.active,
            (false, false) => &self.inactive,
            (true, true) => &self.high_contrast,
            (false, true) => &self.inactive_high_contrast,
        }
    }
}

/// One of the ten accent swatches Plasma's colour settings page offers, in
/// the order it shows them.
///
/// KDE gives the swatches no names; the identifiers here are Teksilo's,
/// chosen by hue (`[derived]`), and are stable, for persisting a choice
/// ([`as_str`](Self::as_str) and [`FromStr`]).
///
/// ```
/// use teksilo_theme_kde::{self as kde, PlasmaAccent};
/// let theme = kde::dark_with_accent(PlasmaAccent::Purple.color());
/// assert_eq!(theme.id.as_str(), "kde.dark");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlasmaAccent {
    /// 233,58,154.
    Pink,
    /// 233,61,88.
    Red,
    /// 233,100,58.
    Orange,
    /// 232,203,45.
    Yellow,
    /// 61,212,37.
    Green,
    /// 0,211,184.
    Teal,
    /// 61,174,233, Breeze's own accent.
    Blue,
    /// 184,117,220.
    Purple,
    /// 146,110,228.
    Violet,
    /// 104,107,111.
    Gray,
}

impl PlasmaAccent {
    /// Every swatch, in the order Plasma shows them.
    pub const ALL: [PlasmaAccent; 10] = [
        PlasmaAccent::Pink,
        PlasmaAccent::Red,
        PlasmaAccent::Orange,
        PlasmaAccent::Yellow,
        PlasmaAccent::Green,
        PlasmaAccent::Teal,
        PlasmaAccent::Blue,
        PlasmaAccent::Purple,
        PlasmaAccent::Violet,
        PlasmaAccent::Gray,
    ];

    /// The swatch's colour in KDE's 16-bit form.
    pub fn kde_color(self) -> KdeColor {
        ACCENT_SWATCHES[self as usize]
    }

    /// The swatch's colour.
    pub fn color(self) -> Color {
        self.kde_color().to_color()
    }

    /// The stable lowercase identifier of the swatch.
    pub fn as_str(self) -> &'static str {
        match self {
            PlasmaAccent::Pink => "pink",
            PlasmaAccent::Red => "red",
            PlasmaAccent::Orange => "orange",
            PlasmaAccent::Yellow => "yellow",
            PlasmaAccent::Green => "green",
            PlasmaAccent::Teal => "teal",
            PlasmaAccent::Blue => "blue",
            PlasmaAccent::Purple => "purple",
            PlasmaAccent::Violet => "violet",
            PlasmaAccent::Gray => "gray",
        }
    }
}

impl fmt::Display for PlasmaAccent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An identifier that names no [`PlasmaAccent`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a Plasma accent swatch")]
pub struct UnknownPlasmaAccent(pub String);

impl FromStr for PlasmaAccent {
    type Err = UnknownPlasmaAccent;

    /// The swatch whose [`as_str`](PlasmaAccent::as_str) is `s`, exactly.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PlasmaAccent::ALL
            .into_iter()
            .find(|accent| accent.as_str() == s)
            .ok_or_else(|| UnknownPlasmaAccent(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_swatches_are_plasmas_in_order() {
        let rgb: Vec<[u8; 3]> = PlasmaAccent::ALL
            .iter()
            .map(|a| {
                let [r, g, b, _] = a.kde_color().to_rgba8();
                [r, g, b]
            })
            .collect();
        assert_eq!(
            rgb,
            vec![
                [233, 58, 154],
                [233, 61, 88],
                [233, 100, 58],
                [232, 203, 45],
                [61, 212, 37],
                [0, 211, 184],
                [61, 174, 233],
                [184, 117, 220],
                [146, 110, 228],
                [104, 107, 111],
            ]
        );
    }

    #[test]
    fn identifiers_are_unique_and_read_back() {
        let ids: BTreeSet<&str> = PlasmaAccent::ALL.iter().map(|a| a.as_str()).collect();
        assert_eq!(ids.len(), PlasmaAccent::ALL.len());
        for accent in PlasmaAccent::ALL {
            assert_eq!(accent.as_str().parse::<PlasmaAccent>(), Ok(accent));
            assert_eq!(accent.to_string(), accent.as_str());
        }
        assert_eq!(
            "Blue".parse::<PlasmaAccent>(),
            Err(UnknownPlasmaAccent("Blue".to_owned()))
        );
        assert!("".parse::<PlasmaAccent>().is_err());
    }

    #[test]
    fn breeze_blue_is_the_seventh_swatch() {
        assert_eq!(PlasmaAccent::ALL[6], PlasmaAccent::Blue);
        assert_eq!(PlasmaAccent::Blue.kde_color(), KdeColor::rgb8(61, 174, 233));
    }

    #[test]
    fn the_combinations_follow_the_breeze_rules() {
        let scheme = KdeColorScheme::breeze_light();
        let resolved = scheme.resolve();
        let c = BreezeColors::compute(&resolved, ColorState::Active, 0.2, false);
        let r = |role| resolved.role(ColorState::Active, role);
        assert_eq!(
            c.frame_outline,
            mix(r(PaletteRole::Window), r(PaletteRole::WindowText), 0.2).to_color()
        );
        assert_eq!(c.separator, c.frame_outline);
        assert_eq!(
            c.frame_background,
            mix(r(PaletteRole::Window), r(PaletteRole::Base), 0.3).to_color()
        );
        assert_eq!(c.focus, KdeColor::rgb8(61, 174, 233).to_color());
        assert_eq!(c.hover, KdeColor::rgb8(61, 174, 233).to_color());
        assert_eq!(
            c.button_outline,
            mix(r(PaletteRole::Button), r(PaletteRole::ButtonText), 0.2).to_color()
        );
        assert_eq!(c.slider_outline, c.button_outline);
        assert_eq!(c.button, r(PaletteRole::Button).to_color());
        assert_eq!(c.button_text, r(PaletteRole::ButtonText).to_color());
        assert!((c.shadow.a() - 0.125).abs() < 1e-6);
    }

    #[test]
    fn the_high_contrast_combinations_outline_in_the_text_colour() {
        let p = KdePalette::from_scheme(&KdeColorScheme::breeze_light());
        for state in ColorState::ALL {
            assert_eq!(p.breeze_colors(state, false), p.colors(state));
            let hc = p.breeze_colors(state, true);
            // At a frame contrast of 1 the outline is the text itself.
            assert_eq!(hc.button_outline, hc.button_text);
            assert_eq!(hc.frame_outline, p.role(state, PaletteRole::WindowText));
        }
        assert_ne!(
            p.breeze_colors(ColorState::Active, true).button_outline,
            p.colors(ColorState::Active).button_outline
        );
    }

    /// D11 reaches the combinations: in an inactive window whose scheme
    /// lightens its backgrounds toward the middle of the scale, the frame
    /// background and outline are the tokens' and the button colour carries
    /// the primary text, which the scheme's own button colour does not.
    #[test]
    fn the_combinations_are_made_from_the_surfaces_the_window_paints() {
        use teksilo_kde_scheme::{IntensityEffect, StateEffect};
        use teksilo_tokens::contrast::composite_contrast;
        let mut scheme = KdeColorScheme::breeze_dark();
        scheme.inactive_effect = StateEffect {
            enabled: true,
            intensity_effect: IntensityEffect::Lighten,
            intensity_amount: 0.4,
            ..StateEffect::INACTIVE_DEFAULT
        };
        let p = KdePalette::from_scheme(&scheme);
        for (high_contrast, floor) in [(false, 4.5), (true, 7.0)] {
            let t = p.tokens(PaintState {
                window_active: false,
                high_contrast,
            });
            let b = p.breeze_colors(ColorState::Inactive, high_contrast);
            assert_eq!(b.frame_background, t.surface_raised);
            assert_eq!(b.frame_outline, t.border);
            assert!(composite_contrast(t.text_primary, b.button) >= floor);
            assert!(composite_contrast(b.button_text, b.button) >= floor);
            let vendor = p.role(ColorState::Inactive, PaletteRole::Button);
            assert!(
                composite_contrast(t.text_primary, vendor) < floor,
                "premise: the scheme's own button colour falls short"
            );
        }
        // The Disabled state keeps the scheme's colours.
        assert_eq!(
            p.colors(ColorState::Disabled).button,
            p.role(ColorState::Disabled, PaletteRole::Button)
        );
    }

    #[test]
    fn palettes_compare_by_content() {
        let a = KdePalette::from_scheme(&KdeColorScheme::breeze_light());
        let b = KdePalette::from_scheme(&KdeColorScheme::breeze_light());
        assert_eq!(a, b);
        let c = KdePalette::from_scheme(&KdeColorScheme::breeze_dark());
        assert_ne!(a, c);
        assert!(!a.is_dark() && c.is_dark());
    }

    #[test]
    fn a_frame_contrast_out_of_range_is_clamped() {
        let mut scheme = KdeColorScheme::breeze_light();
        scheme.frame_contrast = 3.0;
        assert_eq!(KdePalette::from_scheme(&scheme).frame_contrast(), 1.0);
        scheme.frame_contrast = f64::NAN;
        assert_eq!(KdePalette::from_scheme(&scheme).frame_contrast(), 0.2);
    }

    #[test]
    fn the_four_token_sets_are_chosen_by_the_paint_state() {
        let p = KdePalette::from_scheme(&KdeColorScheme::breeze_light());
        let state = |window_active, high_contrast| PaintState {
            window_active,
            high_contrast,
        };
        assert_eq!(p.tokens(state(true, false)), &p.active);
        assert_eq!(p.tokens(state(false, false)), &p.inactive);
        assert_eq!(p.tokens(state(true, true)), &p.high_contrast);
        assert_eq!(p.tokens(state(false, true)), &p.inactive_high_contrast);
        assert_ne!(p.active, p.inactive);
        assert_ne!(p.active, p.high_contrast);
    }
}
