// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A resolved KDE palette projected onto Teksilo's [`ColorTokens`].
//!
//! The source is the scheme's [`ResolvedPalette`]: Qt's palette roles
//! (Window, Base, Text, Highlight, ...) and the seven colour sets with their
//! decorations and computed backgrounds. This module chooses one source per
//! token, applies the Breeze painting rules that combine two of them (a frame
//! outline is the window colour moved toward the window text by the frame
//! contrast), and lifts the colours that would otherwise fail WCAG.
//!
//! `X over Y at a` below means `X` at alpha `a` composited on the opaque
//! `Y`; with both opaque that is KDE's mix from `Y` toward `X` by `a`, which
//! is how it is computed. `fc` is the scheme's frame contrast (0.2 for
//! Breeze). Every token is computed from one colour state: the Active one
//! for an active window, the Inactive one for an inactive window
//! ([`crate::palette::KdePalette`] keeps both), except the disabled tokens,
//! which always read the Disabled state, and the tooltip, which always reads
//! the Active one, as Qt's palette does.
//!
//! ## Where this preset departs from the scheme
//!
//! KDE never adjusts a colour for readability, and several stock Breeze
//! colours fall short of WCAG 2.x. Each departure below is a rule, applied
//! to whatever scheme the theme is built from, so a scheme whose colours
//! already pass keeps them exactly. Each one is marked at its assignment
//! and covered by a test that the lifted colour passes and a test that the
//! vendor colour fails. The palette extension keeps the vendor colours.
//!
//! - **D1, text on the accent.** Breeze Light's selection text is white on
//!   its 61,174,233 highlight, 2.5:1 against SC 1.4.3's 4.5:1. The scheme's
//!   selection text is kept when it reaches the floor on the fill it sits
//!   on; otherwise the text is black or white by Plasma's own rule for text
//!   on an accent (the one it writes when it applies an accent colour),
//!   and the other of the two when that one still falls short. Where
//!   Teksilo paints text it does not choose on a selection (a table cell,
//!   a selected menu row, selected text in a field, all in the primary text
//!   colour), the selection fill keeps its hue and moves in HCY luma until
//!   that text reaches the floor; for Breeze Light nothing moves, and in
//!   Breeze Dark the fill darkens, as Plasma's own accent application
//!   darkens it. The accent's hover and press shades move the same way if
//!   the accent text falls short on them, and so do the hover and press
//!   washes made from the accent if the primary text falls short on them.
//!   A selection or a wash moves only toward the window's end of the scale
//!   (lighter in a light scheme), never across the text to the other end:
//!   under a mid-grey text a light fill would otherwise turn black.
//! - **D2, inactive text.** The inactive foreground (secondary text,
//!   placeholders, line numbers, a tooltip's shortcut) is 3.7:1 on the
//!   Breeze Light window. It moves in luma to 4.5:1 on every surface text
//!   sits on, the hover and press washes included.
//! - **D3, links.** Breeze Light's link colour is 3.8:1 on the window; the
//!   unvisited, visited and hover links move to 4.5:1 the same way.
//! - **D4, status colours.** The negative, neutral and positive foregrounds
//!   (4.3:1, 2.8:1 and 2.9:1 on Breeze Light's view) move to 4.5:1 on every
//!   surface and on their own status background; as borders they move to
//!   SC 1.4.11's 3:1.
//! - **D5, the focus decoration.** The focus colour, 61,174,233, is 2.5:1
//!   on the Breeze Light view; the focus ring moves to 3:1 on every control
//!   surface. It stays opaque.
//! - **D6, frame outlines.** At the frame contrast 0.2 the frame outline is
//!   1.5:1 on the window. `border` keeps that outline, which Breeze draws
//!   around passive frames; `border_strong` and `divider_strong`, which
//!   Teksilo draws where a boundary must be seen, move to 3:1.
//! - **D9, the scroll-bar thumb.** Breeze paints the idle thumb in the
//!   window text at an effective 17.5 % over the window, 1.4:1; the hovered
//!   one in the hover colour at 50 %. Both move to 3:1.
//! - **D10, the scheme's own text.** The window, view and tooltip text
//!   move to the text floor where they fall short, toward their own end of
//!   the scale. Stock Breeze's never do; a scheme whose text is a mid grey
//!   does, and so does an inactive window whose scheme's inactive effects
//!   fade or shade its text toward its background.
//! - **D11, surfaces in the middle of the scale.** Where black (under a
//!   dark text) or white (under a light one) cannot reach the text floor on
//!   a surface text is set on, the surface moves in luma away from the text
//!   until it can, so that every lift above can succeed. It happens in an
//!   inactive window whose scheme's inactive effects shade, lighten or
//!   darken its backgrounds toward the middle of the scale, and under high
//!   contrast, whose 7:1 needs a surface further from the middle; stock
//!   Breeze, whose inactive effects are off, keeps every surface. All the
//!   window's surfaces move the same way, the one the window lies in from
//!   its text, and the tooltip by its own text.
//!
//! D7 and D8 are row heights, not colours, and live with the styles. The
//! specification has no rule for D10 and D11 either: like D1 to D9 they are
//! Teksilo's.
//!
//! A scheme colour may be translucent (the `r,g,b,a` and `#AARRGGBB`
//! forms). The specification is silent on how such a text or decoration
//! should be painted; every colour a rule above lifts is first made opaque,
//! composited over the surface it is lifted on first (the window, or the
//! view for the editor's text), so every lifted token is opaque and its
//! contrast is the contrast it is painted at.
//!
//! ## The high-contrast projection
//!
//! Under the OS "increase contrast" preference the tokens are rebuilt from
//! the scheme with a frame contrast of 1 and a contrast of 10, every text
//! floor raised to 7:1 (SC 1.4.6) and every non-text floor to 4.5:1, and the
//! secondary text grades made the primary ones. The accent moves in luma
//! until its text reaches 7:1 on it and it reaches 4.5:1 on the control
//! surfaces. In a light scheme no single colour lets black text reach 7:1
//! (the fill must be light) while the fill reaches 4.5:1 on a near-white
//! window (the fill must be dark), so there the accent darkens and its text
//! is white: the one place this preset overrides Plasma's black-text rule
//! of D1.
//!
//! A hovered or focused field, check box, radio button, switch or slider
//! handle is outlined in the scheme's hover or focus decoration in place of
//! its lifted outline. Under high contrast the style lifts that decoration
//! the same way, to 4.5:1 on the control surfaces and on the control's own
//! face ([`high_contrast_boundary`]), so the control keeps a 4.5:1 boundary
//! while the pointer is over it. In the normal projection it keeps the
//! scheme's colour, as Breeze paints it.
//!
//! The selection fill does not move with the accent: it carries the
//! window's primary text, which must reach 7:1 on it, so where that text
//! falls short it moves toward the window until the text reaches 7:1
//! (Breeze Light's highlight lightens,
//! Breeze Dark's darkens). That leaves it closer to the window than in the
//! normal projection, whose text floor is 4.5:1: 1.9:1 on Breeze Light's
//! window and 2.2:1 on Breeze Dark's, against 2.2:1 and 3.4:1. No colour
//! can do better, in any scheme: the text is at 7:1 on the window too, so
//! a fill at 7:1 under the text and 4.5:1 on the window would put the
//! darkest and the lightest of the three colours at least 31.5:1 apart,
//! whatever their order, beyond the 21:1 of black on white. The fill is a
//! backdrop for text, not a boundary; a style that must show a selection's
//! extent at 4.5:1 outlines it in a colour that reaches it, such as
//! `focus_ring`.
//!
//! ## Kept from Int UI
//!
//! KDE has no colours for four tokens, which keep Teksilo's Int UI values
//! for the scheme's appearance: `search_match_bg` and
//! `search_match_current_bg` (KDE has no find-match role), `scrim` (KDE
//! dims nothing behind a dialog) and `chart_palette` (KDE has no
//! data-visualisation sequence; Int UI's is the colour-blind-safe Okabe-Ito
//! one).
//!
//! ## Known limitations
//!
//! - `surface_selected` serves two kinds of text: cells and rows Teksilo
//!   paints in the primary text colour, and rows whose style paints the
//!   on-accent text. In Breeze Dark those are white and black, and no fill
//!   carries both, so the token is the D1 fill for the primary text, and a
//!   style that paints on-accent text on a selection draws on `accent`.
//! - Under high contrast the selection fill stays below 4.5:1 on the
//!   window, which no fill carrying the window's text at 7:1 can reach (see
//!   the high-contrast projection above).
//! - Teksilo sets the window text on the view too, so a scheme whose view
//!   is in the other tone from its window (a dark view in a light window)
//!   cannot keep both: D11 moves the view toward the window's tone until
//!   the window text can be read on it.
//! - D11 moves a surface only until black or white reaches the floor on
//!   it, so on a surface it moved, a lifted colour can end at black or
//!   white and lose its hue.

use teksilo_kde_scheme::math::{
    LumaDirection, black_or_white_on, hsv_darker, hsv_lighter, lift_luma_toward, lift_luma_until,
    luma, mix, read_channel,
};
use teksilo_kde_scheme::{
    BackgroundRole, ColorSet, ColorState, DecorationRole, ForegroundRole, KdeColor, PaletteRole,
    ResolvedPalette,
};
use teksilo_tokens::contrast::composite_contrast;
use teksilo_tokens::{Color, ColorTokens};

/// The contrast floors a projection lifts colours to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Floors {
    /// Text on its backdrop.
    pub text: f32,
    /// A control boundary, focus ring or other graphical object on its
    /// backdrop.
    pub non_text: f32,
}

/// `[wcag]` SC 1.4.3 (text, 4.5:1) and SC 1.4.11 (non-text, 3:1).
pub(crate) const AA: Floors = Floors {
    text: 4.5,
    non_text: 3.0,
};

/// `[wcag]` SC 1.4.6 (text, 7:1), with non-text raised to 4.5:1, the
/// targets of Teksilo's high-contrast projection.
pub(crate) const ENHANCED: Floors = Floors {
    text: 7.0,
    non_text: 4.5,
};

/// `[Hp]` The alpha of Breeze's hover wash on an item and of its press
/// blend on a button: the style's blend value.
const BLEND: f64 = 0.3;

/// `[Hp]` The idle scroll-bar thumb: the window text with alpha times 0.5,
/// times 0.7 while the pointer is away from the bar, painted at alpha times
/// 0.5 over the window.
const THUMB_ALPHA: f64 = 0.5 * 0.7 * 0.5;

/// `[Hp]` The hovered scroll-bar thumb: the hover colour painted at 0.5
/// over the window.
const THUMB_HOVER_ALPHA: f64 = 0.5;

/// `[qt]` `[Hp]` Qt's `lighter` and `darker` factor Breeze uses for a
/// hovered or pressed highlight.
const FOCUS_LIGHTEN_FACTOR: i32 = 110;

/// `[Hp]` The `darker` factor of an unselected tab's fill, the recessed
/// surface Breeze paints.
const RECESSED_FACTOR: i32 = 120;

/// Everything a projection is computed from.
pub(crate) struct Mapping<'a> {
    /// The resolved scheme.
    pub palette: &'a ResolvedPalette,
    /// The colour state the window paints in: Active or Inactive.
    pub state: ColorState,
    /// The frame contrast `fc`, in `0..=1`.
    pub frame_contrast: f64,
    /// Whether this is the high-contrast projection.
    pub high_contrast: bool,
    /// Whether the scheme is dark, which picks the Int UI values kept.
    pub dark: bool,
}

/// `top` at alpha `alpha` over the opaque `bottom`.
fn over(top: KdeColor, alpha: f64, bottom: KdeColor) -> KdeColor {
    mix(bottom, top, alpha)
}

/// The lowest WCAG contrast `fg` reaches, painted over each of `on`.
pub(crate) fn worst_contrast(fg: KdeColor, on: &[KdeColor]) -> f32 {
    let fg = fg.to_color();
    on.iter()
        .map(|bg| composite_contrast(fg, bg.to_color()))
        .fold(f32::INFINITY, f32::min)
}

fn passes(fg: KdeColor, on: &[KdeColor], floor: f32) -> bool {
    worst_contrast(fg, on) >= floor
}

/// `[wcag]` `color` as painted on `backdrop`, made opaque: a translucent
/// colour composited over the opaque `backdrop`, an opaque one unchanged.
fn opaque_on(color: KdeColor, backdrop: KdeColor) -> KdeColor {
    if color.a == u16::MAX {
        return color;
    }
    let solid = |c: KdeColor| KdeColor { a: u16::MAX, ..c };
    over(solid(color), read_channel(color.a), solid(backdrop))
}

/// `[wcag]` `color` with its HCY luma moved toward more contrast until it
/// reaches `floor` on every one of `on`; unchanged when it already does.
///
/// A translucent `color` is first made opaque over the first of `on`, the
/// surface it is chiefly painted on, so every lifted token is opaque.
fn lift(color: KdeColor, on: &[KdeColor], floor: f32) -> KdeColor {
    let color = on.first().map_or(color, |&bg| opaque_on(color, bg));
    lift_luma_until(color, on, floor)
}

/// `[wcag]` A hover or focus decoration a style paints in place of a
/// control's only boundary, as it paints it: under the high-contrast
/// preference, [`lift`]ed to the enhanced non-text floor on the window, the
/// view and the frame background of `colors` (`surface_main`,
/// `surface_content`, `surface_raised`) and on `face`, the control's own
/// fill the outline meets; otherwise `decoration` unchanged, the scheme's
/// own colour.
///
/// The hovered or focused outline of a field, a check box, a radio button,
/// a switch and a slider handle replaces the outline lifted at rest, so
/// without this a hovered control would lose the 4.5:1 boundary the
/// high-contrast projection gives it at rest (2.2:1 for Breeze Light's
/// hover colour on its window).
pub(crate) fn high_contrast_boundary(
    decoration: Color,
    colors: &ColorTokens,
    face: Color,
    high_contrast: bool,
) -> Color {
    if !high_contrast {
        return decoration;
    }
    let on = [
        colors.surface_main,
        colors.surface_content,
        colors.surface_raised,
        face,
    ]
    .map(KdeColor::from_color);
    lift(KdeColor::from_color(decoration), &on, ENHANCED.non_text).to_color()
}

/// `[wcag]` [`lift`] in the given direction only: a text lifted toward its
/// own end of the scale, so it never crosses to the other side of the
/// surfaces it is set on, where it might reach the floor on them but not on
/// a fill moved away from it.
fn lift_toward(color: KdeColor, on: &[KdeColor], floor: f32, toward: LumaDirection) -> KdeColor {
    let color = on.first().map_or(color, |&bg| opaque_on(color, bg));
    lift_luma_toward(color, on, floor, toward)
}

/// The other direction.
fn opposite(direction: LumaDirection) -> LumaDirection {
    match direction {
        LumaDirection::Lighter => LumaDirection::Darker,
        LumaDirection::Darker => LumaDirection::Lighter,
    }
}

/// The direction away from `text` on the luma scale, as seen from
/// `surface`: lighter for a surface at least as light as the text.
fn away_from(text: KdeColor, surface: KdeColor) -> LumaDirection {
    if luma(surface) >= luma(text) {
        LumaDirection::Lighter
    } else {
        LumaDirection::Darker
    }
}

/// `[wcag]` D11: `surface` moved in luma toward `direction`, away from
/// the text set on it, until the end of the scale on the text's side (black
/// under a dark text, white under a light one) reaches `floor` on it;
/// unchanged when it already does, as every surface of a scheme with
/// readable text does.
///
/// This keeps every surface where a text lift can reach the floor. No text
/// reaches 7:1 on a surface in the middle of the scale, and no text reaches
/// 4.5:1 on a mid-scale surface and on the surfaces either side of it at
/// once, as happens in an inactive window when the scheme's inactive
/// effects shade its backgrounds toward the middle. The surface is measured
/// as opaque and keeps its alpha.
fn carry(surface: KdeColor, direction: LumaDirection, floor: f32) -> KdeColor {
    let extreme = match direction {
        LumaDirection::Lighter => KdeColor::BLACK,
        LumaDirection::Darker => KdeColor::WHITE,
    };
    let solid = KdeColor {
        a: u16::MAX,
        ..surface
    };
    let carried = lift_luma_toward(solid, &[extreme], floor, direction);
    KdeColor {
        a: surface.a,
        ..carried
    }
}

/// `[wcag]` D1: the text to paint on `fills`.
///
/// `vendor` when it reaches `floor` on every fill; otherwise black or white
/// by Plasma's rule for text on an accent, judged on the first fill, or the
/// other of the two when the rule's choice still falls short (black and
/// white cannot both fall below 4.58:1 on one opaque fill).
///
/// A translucent `vendor` is judged, and kept, made opaque over the first
/// fill.
pub(crate) fn text_on(vendor: KdeColor, fills: &[KdeColor], floor: f32) -> KdeColor {
    let vendor = opaque_on(vendor, fills[0]);
    if passes(vendor, fills, floor) {
        return vendor;
    }
    let plasma = black_or_white_on(fills[0], true);
    let other = if plasma == KdeColor::BLACK {
        KdeColor::WHITE
    } else {
        KdeColor::BLACK
    };
    if passes(plasma, fills, floor) || worst_contrast(plasma, fills) >= worst_contrast(other, fills)
    {
        plasma
    } else {
        other
    }
}

/// `[wcag]` D1: `fill` with its luma moved away from `text` until `text`
/// reaches `floor` on it; unchanged when it already does.
fn fill_under(fill: KdeColor, text: &[KdeColor], floor: f32) -> KdeColor {
    lift_luma_until(fill, text, floor)
}

/// `[wcag]` D1: `fill` with its luma moved toward `toward`, the window's end
/// of the scale, until `text` reaches `floor` on it; unchanged when it
/// already does.
///
/// A wash or a selection sits among the window's surfaces, so it moves only
/// toward their end, never across the text to the other end, where the
/// text might read but the fill would be black in a light window. Where
/// even that end falls short the fill stops there, as near the floor as the
/// text can come; the text's own lift (D10) makes sure that cannot happen
/// to the primary text.
fn fill_toward(fill: KdeColor, text: &[KdeColor], floor: f32, toward: LumaDirection) -> KdeColor {
    lift_luma_toward(fill, text, floor, toward)
}

/// The high-contrast accent and its text.
///
/// For each text candidate (D1's choice first, then the other of black and
/// white), the highlight moves until the text reaches `floors.text` on it
/// and then until it reaches `floors.non_text` on the control surfaces. The
/// first candidate that satisfies both wins. A light scheme ends on the
/// second candidate, white text on a darkened accent; see the module doc.
fn high_contrast_accent(
    highlight: KdeColor,
    vendor_text: KdeColor,
    surfaces: &[KdeColor],
    floors: Floors,
) -> (KdeColor, KdeColor) {
    let first = text_on(vendor_text, &[highlight], floors.text);
    // The other end of the scale from the first candidate: white for a dark
    // text, black for a light one.
    let second =
        if worst_contrast(KdeColor::WHITE, &[first]) >= worst_contrast(KdeColor::BLACK, &[first]) {
            KdeColor::WHITE
        } else {
            KdeColor::BLACK
        };
    // How far a candidate falls short of either floor, as the smaller of
    // its two margins: at least zero when it reaches both.
    let attempt = |text: KdeColor| {
        let fill = fill_under(highlight, &[text], floors.text);
        let fill = lift_luma_until(fill, surfaces, floors.non_text);
        let margin = (worst_contrast(text, &[fill]) - floors.text)
            .min(worst_contrast(fill, surfaces) - floors.non_text);
        (fill, text, margin)
    };
    let (fill, text, margin) = attempt(first);
    if margin >= 0.0 {
        return (fill, text);
    }
    let (fill2, text2, margin2) = attempt(second);
    if margin2 > margin {
        (fill2, text2)
    } else {
        (fill, text)
    }
}

/// The colour tokens of `m`.
pub(crate) fn tokens(m: &Mapping<'_>) -> ColorTokens {
    let p = m.palette;
    let s = m.state;
    let d = ColorState::Disabled;
    let fc = m.frame_contrast;
    let floors = if m.high_contrast { ENHANCED } else { AA };
    // Int UI's palette for the appearance, for the four tokens KDE has no
    // colours for (see the module doc).
    let kept = if m.dark {
        ColorTokens::dark_default()
    } else {
        ColorTokens::light_default()
    };

    let role = |state: ColorState, r: PaletteRole| p.role(state, r);
    let window_text = role(s, PaletteRole::WindowText);
    // `[wcag]` D11: each surface text is set on moves away from the window
    // text, in the direction the window lies from it, until black or white
    // can reach the text floor on it. One direction for every surface, as
    // the primary text is set on all of them.
    let polarity = away_from(window_text, role(s, PaletteRole::Window));
    let surface = |c: KdeColor| carry(c, polarity, floors.text);
    let window = surface(role(s, PaletteRole::Window));
    let base = surface(role(s, PaletteRole::Base));
    let alternate = surface(role(s, PaletteRole::AlternateBase));
    let button = surface(role(s, PaletteRole::Button));
    let highlight = role(s, PaletteRole::Highlight);
    let highlighted_text = role(s, PaletteRole::HighlightedText);
    let view = p.set(s, ColorSet::View);
    let window_set = p.set(s, ColorSet::Window);
    // `[qt]` Qt's palette takes the tooltip from the Active state whatever
    // state is painted.
    let tooltip_set = p.set(ColorState::Active, ColorSet::Tooltip);
    let focus = view.decoration(DecorationRole::Focus);
    let hover = view.decoration(DecorationRole::Hover);

    // `[Hp]` Breeze's frame background (menus, popups, group boxes): the
    // window colour 30 % of the way to the view's.
    let frame_background = surface(mix(window, base, BLEND));
    // `[Hp]` The frame outline and the separator: the window colour moved
    // toward the window text by the frame contrast.
    let frame_outline = mix(window, window_text, fc);
    // `[derived]` A recessed surface: the window's alternate background when
    // it is darker than the window, otherwise `[Hp]` the window darkened as
    // Breeze darkens an unselected tab.
    let window_alternate = window_set.background(BackgroundRole::Alternate);
    let sunken = surface(if luma(window_alternate) < luma(window) {
        window_alternate
    } else {
        hsv_darker(window, RECESSED_FACTOR)
    });
    // Every opaque plain surface text is set on.
    let plain_surfaces = [window, base, frame_background, alternate, button, sunken];
    // `[Hp]` What a control stands on.
    let control_surfaces = [window, base, frame_background, button];

    // ── Accent ──────────────────────────────────────────────────────────
    // `[qt]` Highlight is the accent. D1 and, for high contrast, the HCY
    // move of the module doc.
    let accent_of = |highlight: KdeColor, highlighted_text: KdeColor| {
        if m.high_contrast {
            high_contrast_accent(highlight, highlighted_text, &control_surfaces, floors)
        } else {
            (
                highlight,
                // `[wcag]` D1: the scheme's selection text, or Plasma's black
                // or white where it falls short.
                text_on(highlighted_text, &[highlight], floors.text),
            )
        }
    };
    let (accent, text_on_accent) = accent_of(highlight, highlighted_text);
    // `[Hp]` Breeze lightens a hovered highlight and darkens a pressed one
    // by Qt's 110 % factor; `[wcag]` D1 moves either if the accent text
    // falls short on it.
    let accent_hover = fill_under(
        hsv_lighter(accent, FOCUS_LIGHTEN_FACTOR),
        &[text_on_accent],
        floors.text,
    );
    let accent_pressed = fill_under(
        hsv_darker(accent, FOCUS_LIGHTEN_FACTOR),
        &[text_on_accent],
        floors.text,
    );

    // ── Text ────────────────────────────────────────────────────────────
    // `[qt]` WindowText is the text of the window. `[wcag]` D10: lifted to
    // the text floor on the plain surfaces where it falls short (it never
    // does in a stock scheme's normal projection; the high-contrast one
    // raises the floor to 7:1).
    let text_side = opposite(polarity);
    let text_primary = lift_toward(window_text, &plain_surfaces, floors.text, text_side);
    // The window's end of the luma scale, away from its text.
    let toward_window = away_from(text_primary, window);

    // `[wcag]` D1: an accent wash carries the primary text, so it moves away
    // from that text, toward the window's end of the scale, if the text
    // falls short on it.
    let wash = |fill: KdeColor| fill_toward(fill, &[text_primary], floors.text, toward_window);
    // `[Hp]` The item-view hover: Highlight over the view at the blend
    // value. Also the subtle accent wash.
    let surface_hover = wash(over(accent, BLEND, base));
    // `[Hp]` A pressed push button's fill: the button colour 30 % of the way
    // to the highlight.
    let surface_pressed = wash(mix(button, accent, BLEND));
    // The same two washes in the Active state's accent. An inactive
    // window's text is lifted against both, so a text colour does not
    // change when the window loses focus while the scheme's surfaces do not.
    let (active_accent, _) = accent_of(
        role(ColorState::Active, PaletteRole::Highlight),
        role(ColorState::Active, PaletteRole::HighlightedText),
    );
    let active_hover = wash(over(active_accent, BLEND, base));
    let active_pressed = wash(mix(button, active_accent, BLEND));

    // Every opaque surface text is set on, the washes included.
    let text_surfaces = [
        window,
        base,
        frame_background,
        alternate,
        button,
        sunken,
        surface_hover,
        surface_pressed,
        active_hover,
        active_pressed,
    ];
    // `[wcag]` D2: the window set's inactive foreground, lifted to the text
    // floor on every surface. The high-contrast projection uses the primary
    // text instead.
    let text_secondary = if m.high_contrast {
        text_primary
    } else {
        lift(
            window_set.foreground(ForegroundRole::Inactive),
            &text_surfaces,
            floors.text,
        )
    };

    // `[qt]` Text is the view's text. The editor surfaces it is set on.
    let editor_current_line = alternate;
    // `[derived]` A code block sits half-way between the view and the
    // window: a step below the view in a light scheme, above it in a dark
    // one.
    // `[wcag]` D11 again: a mix of two carried surfaces of different hues
    // can be darker than both.
    let editor_code_block = surface(mix(base, window, 0.5));
    let editor_surfaces = [base, editor_current_line, editor_code_block];
    // `[wcag]` D10: lifted where it falls short on them.
    let editor_fg = lift_toward(
        view.foreground(ForegroundRole::Normal),
        &editor_surfaces,
        floors.text,
        text_side,
    );

    // `[wcag]` D3: links on every text surface.
    let link = |r: ForegroundRole| lift(view.foreground(r), &text_surfaces, floors.text);
    // `[qt]` The view's link colours. KDE has no hovered-link colour: the
    // active foreground, KDE's "hot" text, stands in for it.
    let text_link = link(ForegroundRole::Link);
    let text_link_visited = link(ForegroundRole::Visited);
    let text_link_hover = link(ForegroundRole::Active);

    // `[wcag]` D4: each status colour on every text surface and on its own
    // computed background (`[Hp]` the view's background tinted 30 % toward
    // the foreground, KDE's negative, neutral, positive and active
    // backgrounds).
    let status = |fg: ForegroundRole, bg: BackgroundRole| {
        let background = surface(view.background(bg));
        let mut on = text_surfaces.to_vec();
        on.push(background);
        (lift(view.foreground(fg), &on, floors.text), background)
    };
    let (error, error_bg) = status(ForegroundRole::Negative, BackgroundRole::Negative);
    let (warning, warning_bg) = status(ForegroundRole::Neutral, BackgroundRole::Neutral);
    let (success, success_bg) = status(ForegroundRole::Positive, BackgroundRole::Positive);
    let (info, info_bg) = status(ForegroundRole::Active, BackgroundRole::Active);
    // `[wcag]` D4 as a boundary: the status colours lifted to the non-text
    // floor on the control surfaces.
    let status_border =
        |fg: ForegroundRole| lift(view.foreground(fg), &control_surfaces, floors.non_text);
    let border_error = status_border(ForegroundRole::Negative);
    let border_warning = status_border(ForegroundRole::Neutral);

    // ── Selection ───────────────────────────────────────────────────────
    // `[qt]` Highlight is the selection. `[wcag]` D1: the fill moves until
    // the primary text Teksilo paints on it reaches the floor.
    let selection_fill =
        |fill: KdeColor| fill_toward(fill, &[text_primary, editor_fg], floors.text, toward_window);
    let surface_selected = selection_fill(highlight);
    // `[qt]` The Inactive state's highlight: KDE tints the window colour 40 %
    // toward the selection colour, the muted selection of a backdrop window.
    let inactive_highlight = role(ColorState::Inactive, PaletteRole::Highlight);
    let surface_selected_inactive = selection_fill(inactive_highlight);
    let selection_text_active = text_on(highlighted_text, &[surface_selected], floors.text);
    let selection_text_inactive = text_on(
        role(ColorState::Inactive, PaletteRole::HighlightedText),
        &[surface_selected_inactive],
        floors.text,
    );

    // ── Borders and focus ───────────────────────────────────────────────
    // `[wcag]` D6: the frame outline lifted to the non-text floor where a
    // boundary must be seen.
    let border_strong = lift(frame_outline, &control_surfaces, floors.non_text);
    // `[Hp]` The separator is the frame outline's colour.
    let divider = frame_outline;
    let divider_strong = border_strong;
    // `[wcag]` D5: the view's focus decoration lifted to the non-text floor.
    let focus_ring = lift(focus, &control_surfaces, floors.non_text);

    // ── Scroll bar ──────────────────────────────────────────────────────
    // `[wcag]` D9: Breeze's thumbs lifted to the non-text floor on the window
    // and the view.
    let thumb_backdrops = [window, base];
    let scrollbar_thumb = lift(
        over(window_text, THUMB_ALPHA, window),
        &thumb_backdrops,
        floors.non_text,
    );
    let scrollbar_thumb_hover = lift(
        over(hover, THUMB_HOVER_ALPHA, window),
        &thumb_backdrops,
        floors.non_text,
    );

    // ── Tooltip ─────────────────────────────────────────────────────────
    let tooltip_fg = tooltip_set.foreground(ForegroundRole::Normal);
    // `[wcag]` D11 for the tooltip's own surface.
    let tooltip_normal = tooltip_set.background(BackgroundRole::Normal);
    let tooltip_bg = carry(
        tooltip_normal,
        away_from(tooltip_fg, tooltip_normal),
        floors.text,
    );
    // `[wcag]` D10.
    let tooltip_text = lift(tooltip_fg, &[tooltip_bg], floors.text);
    // `[wcag]` D2: the tooltip's inactive text, the shortcut's.
    let tooltip_shortcut = if m.high_contrast {
        tooltip_text
    } else {
        lift(
            tooltip_set.foreground(ForegroundRole::Inactive),
            &[tooltip_bg],
            floors.text,
        )
    };

    // ── Editor gutter ───────────────────────────────────────────────────
    // `[wcag]` D2: the view's inactive text, for line numbers.
    let editor_gutter_fg = if m.high_contrast {
        editor_fg
    } else {
        lift(
            view.foreground(ForegroundRole::Inactive),
            &editor_surfaces,
            floors.text,
        )
    };

    let c = |k: KdeColor| k.to_color();
    ColorTokens {
        // ── Surfaces ──
        // `[qt]` Window: the window's background.
        surface_main: c(window),
        // `[qt]` Base: the background of views and fields.
        surface_content: c(base),
        // `[Hp]` The frame background.
        surface_raised: c(frame_background),
        surface_sunken: c(sunken),
        surface_hover: c(surface_hover),
        surface_pressed: c(surface_pressed),
        surface_selected: c(surface_selected),
        surface_selected_inactive: c(surface_selected_inactive),
        // `[qt]` AlternateBase: alternate rows.
        surface_alt_row: c(alternate),
        // `[qt]` A disabled control: the Disabled state's Button.
        surface_disabled: c(role(d, PaletteRole::Button)),

        // ── Text ──
        text_primary: c(text_primary),
        text_secondary: c(text_secondary),
        // `[qt]` The Disabled state's WindowText. Disabled text is exempt
        // from SC 1.4.3 and is not lifted.
        text_disabled: c(role(d, PaletteRole::WindowText)),
        text_on_accent: c(text_on_accent),
        text_link: c(text_link),
        text_link_hover: c(text_link_hover),
        text_link_visited: c(text_link_visited),
        text_error: c(error),
        text_warning: c(warning),
        text_success: c(success),

        // ── Accent ──
        accent: c(accent),
        accent_hover: c(accent_hover),
        accent_pressed: c(accent_pressed),
        // `[qt]` The Disabled state's Highlight.
        accent_disabled: c(role(d, PaletteRole::Highlight)),
        // `[Hp]` The item hover wash.
        accent_subtle_bg: c(surface_hover),

        // ── Borders ──
        // `[Hp]` The frame outline, as Breeze draws it.
        border: c(frame_outline),
        border_strong: c(border_strong),
        // `[Hp]` Focus is shown in the focus decoration, lifted by D5.
        border_focused: c(focus_ring),
        border_error: c(border_error),
        border_warning: c(border_warning),
        // `[Hp]` The Disabled state's frame outline.
        border_disabled: c(mix(
            role(d, PaletteRole::Window),
            role(d, PaletteRole::WindowText),
            fc,
        )),
        divider: c(divider),
        divider_strong: c(divider_strong),

        // ── Status ──
        status_info_fg: c(info),
        status_info_bg: c(info_bg),
        status_success_fg: c(success),
        status_success_bg: c(success_bg),
        status_warning_fg: c(warning),
        status_warning_bg: c(warning_bg),
        status_error_fg: c(error),
        status_error_bg: c(error_bg),

        // ── Selection ──
        selection_bg_active: c(surface_selected),
        selection_text_active: c(selection_text_active),
        selection_bg_inactive: c(surface_selected_inactive),
        selection_text_inactive: c(selection_text_inactive),
        // Kept from Int UI: KDE has no find-match colour.
        search_match_bg: kept.search_match_bg,
        search_match_current_bg: kept.search_match_current_bg,

        // ── Scroll bar ──
        // `[Hp]` Breeze paints no scroll-bar track, idle or hovered.
        scrollbar_track: Color::TRANSPARENT,
        scrollbar_track_hover: Color::TRANSPARENT,
        scrollbar_thumb: c(scrollbar_thumb),
        scrollbar_thumb_hover: c(scrollbar_thumb_hover),
        // `[Hp]` Breeze has no pressed handle colour: a grabbed handle is
        // under the pointer and stays the hovered one.
        scrollbar_thumb_pressed: c(scrollbar_thumb_hover),

        // ── Tooltip ──
        // `[qt]` ToolTipBase and ToolTipText.
        tooltip_bg: c(tooltip_bg),
        tooltip_text: c(tooltip_text),
        // `[Hp]` The tooltip outline: its background moved toward its text
        // by the frame contrast.
        tooltip_border: c(mix(tooltip_bg, tooltip_fg, fc)),
        tooltip_shortcut: c(tooltip_shortcut),

        // ── Editor ──
        // `[qt]` Base and Text.
        editor_bg: c(base),
        editor_fg: c(editor_fg),
        // `[qt]` Qt draws the text cursor in the text colour.
        editor_caret: c(editor_fg),
        // `[derived]` KDE has no current-line colour: the view's alternate
        // background, its subtlest step.
        editor_current_line_bg: c(editor_current_line),
        editor_gutter_fg: c(editor_gutter_fg),
        // `[qt]` The selection, D1's fill.
        editor_selection_bg: c(surface_selected),
        editor_code_block_bg: c(editor_code_block),
        // `[derived]` KDE has no code register: the view's text.
        editor_code_block_fg: c(editor_fg),

        // ── Misc ──
        focus_ring: c(focus_ring),
        // `[wcag]` D4: the negative colour as a boundary.
        focus_ring_error: c(border_error),
        // Kept from Int UI: KDE dims nothing behind a dialog.
        scrim: kept.scrim,

        // ── Cross-design-language roles ──
        // `[derived]` White on the error fill, unless it falls short; then
        // D1's black-or-white rule.
        text_on_error: c(text_on(KdeColor::WHITE, &[error], floors.text)),
        text_on_error_container: c(error),
        // `[Hp]` The view's negative background.
        surface_error_container: c(error_bg),
        // `[Hp]` A group box is drawn on the frame background.
        surface_container: c(frame_background),
        // `[derived]` One step above the frame background: the view's.
        surface_container_raised: c(base),
        surface_container_sunken: c(sunken),

        // Kept from Int UI: KDE has no data-visualisation sequence.
        chart_palette: kept.chart_palette,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColorScheme;

    fn active(scheme: &KdeColorScheme) -> ColorTokens {
        let palette = scheme.resolve();
        tokens(&Mapping {
            palette: &palette,
            state: ColorState::Active,
            frame_contrast: scheme.frame_contrast,
            high_contrast: false,
            dark: scheme.is_dark(),
        })
    }

    #[test]
    fn d1_keeps_a_passing_vendor_text_and_otherwise_follows_plasmas_rule() {
        let blue = KdeColor::rgb8(61, 174, 233);
        // White on the Breeze highlight fails, black is Plasma's choice.
        assert_eq!(text_on(KdeColor::WHITE, &[blue], 4.5), KdeColor::BLACK);
        // A vendor text that already passes is kept.
        let navy = KdeColor::rgb8(0, 0, 80);
        assert_eq!(text_on(navy, &[blue], 4.5), navy);
        // On a dark fill Plasma's rule is white.
        let dark = KdeColor::rgb8(30, 60, 90);
        assert_eq!(text_on(KdeColor::BLACK, &[dark], 4.5), KdeColor::WHITE);
    }

    #[test]
    fn the_high_contrast_accent_is_white_on_a_darkened_accent_in_a_light_scheme() {
        let window = KdeColor::rgb8(239, 240, 241);
        let (fill, text) = high_contrast_accent(
            KdeColor::rgb8(61, 174, 233),
            KdeColor::WHITE,
            &[window, KdeColor::WHITE],
            ENHANCED,
        );
        assert_eq!(text, KdeColor::WHITE);
        assert!(worst_contrast(text, &[fill]) >= 7.0);
        assert!(worst_contrast(fill, &[window, KdeColor::WHITE]) >= 4.5);
    }

    #[test]
    fn the_high_contrast_accent_keeps_black_text_in_a_dark_scheme() {
        let window = KdeColor::rgb8(32, 35, 38);
        let (fill, text) = high_contrast_accent(
            KdeColor::rgb8(61, 174, 233),
            KdeColor::rgb8(252, 252, 252),
            &[window, KdeColor::rgb8(20, 22, 24)],
            ENHANCED,
        );
        assert_eq!(text, KdeColor::BLACK);
        assert!(worst_contrast(text, &[fill]) >= 7.0);
    }

    #[test]
    fn the_light_selection_carries_dark_text_unmoved_and_the_dark_one_darkens() {
        let light = active(&KdeColorScheme::breeze_light());
        assert_eq!(light.surface_selected, light.accent);
        let dark = active(&KdeColorScheme::breeze_dark());
        assert_ne!(dark.surface_selected, dark.accent);
        assert!(dark.surface_selected.relative_luminance() < dark.accent.relative_luminance());
        assert!(composite_contrast(dark.text_primary, dark.surface_selected) >= 4.5);
    }

    #[test]
    fn the_recessed_surface_is_the_window_alternate_or_the_darkened_window() {
        // Breeze Light's window alternate (227,229,231) is darker than its
        // window; Breeze Dark's (41,44,48) is lighter, so it darkens.
        let light = active(&KdeColorScheme::breeze_light());
        assert_eq!(
            light.surface_sunken,
            KdeColor::rgb8(227, 229, 231).to_color()
        );
        let dark = active(&KdeColorScheme::breeze_dark());
        assert!(dark.surface_sunken.relative_luminance() < dark.surface_main.relative_luminance());
    }
}
