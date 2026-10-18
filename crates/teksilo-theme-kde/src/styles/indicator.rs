// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The colours and geometry the Breeze check box, radio button and switch
//! share.
//!
//! All three are painted from the **button** colours of the scheme (a check
//! box's box is the button colour, a switch's track is the button colour
//! moved toward the button text), from the current window's highlight (a
//! checked box, unlike a hovered push button, uses the muted highlight of a
//! backdrop window) and from the view's text for the mark. Every colour is
//! resolved at paint time from the theme the walker hands the widget, so the
//! inactive and high-contrast projections reach them.
//!
//! ## Where this departs from Breeze
//!
//! - **D6, the outline of an empty indicator.** Breeze outlines an unchecked
//!   box and an unselected disc in the separator colour, the window colour
//!   moved a fifth of the way to the window text: 1.5:1 on the Breeze Light
//!   window. That outline is the whole control, so it is drawn in
//!   `border_strong`, the same outline lifted to SC 1.4.11's 3:1 (D6 of the
//!   colour mapping), which a scheme whose outline already passes keeps
//!   exactly. A checked box and a selected disc keep Breeze's highlight
//!   outline: their state is carried by the mark, in the view text, which
//!   is well above 3:1 on the highlight-washed fill in stock Breeze. For any
//!   scheme the mark is lifted in luma to the non-text floor (3:1, 4.5:1
//!   under high contrast) on that fill at rest and pressed, which a scheme
//!   whose inactive effects move the button colour or the highlight toward
//!   the text needs.
//! - **D6, the outline of a checked switch.** A switch has no mark: on, its
//!   track's highlight outline (2.2:1 on the Breeze Light window) is as much
//!   the control's only boundary as the off track's. It is drawn in the
//!   highlight lifted in luma to the non-text floor on the control surfaces
//!   and the button colour, [`BreezeIndicatorColors::checked_outline`]; a
//!   scheme whose highlight already passes, Breeze Dark's, keeps it exactly.
//!   See [`crate::styles::toggle`].
//! - **The hover decorations under high contrast.** A hovered box or disc
//!   is ringed in the view's focus decoration, stroked over its outline,
//!   and a hovered switch outlines its track and handle in the button set's
//!   hover decoration; both stand in for the control's lifted outline.
//!   Under high contrast they are lifted to 4.5:1 on the control surfaces
//!   and the button colour (Breeze Light's is 2.2:1 on its window), so a
//!   hovered control keeps its boundary. In the normal projection they are
//!   the scheme's own.
//! - **A translucent button colour.** Breeze fills a box over an opaque
//!   button colour and washes the highlight over that fill
//!   ([`crate::styles::checkbox`]). A scheme may give the button colour an
//!   alpha; the specification is silent on how such a face is painted. It
//!   is made opaque over the window, as the colour mapping makes a lifted
//!   colour opaque, so the box, the disc and the switch's handle are one
//!   opaque fill wherever the control sits, the wash is composited over it
//!   as for stock Breeze, and the mark is lifted on the colour the box
//!   actually shows.
//! - **D5, keyboard focus.** Breeze shows keyboard focus with a 2 px band
//!   of the highlight at 30 % outside the box. The band is drawn in the
//!   opaque `focus_ring` token, the focus colour lifted to 3:1, as on the
//!   buttons.
//!
//! ## Without the palette
//!
//! The button colours come from [`KdePalette`]. A theme without it paints
//! from its own roles: the content surface for the button colour, the
//! primary text for its text, `border` for its outline, `border_focused`
//! for the hover decorations.

use teksilo_canvas::Rect;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::styles::Theme;
use teksilo_kde_scheme::math::{hsv_darker, lift_luma_until};
use teksilo_kde_scheme::{ColorSet, ColorState, DecorationRole, KdeColor, PaletteRole};
use teksilo_tokens::Color;

use crate::color::{AA, ENHANCED, high_contrast_boundary, mark_on_faces, opaque_over};
use crate::palette::KdePalette;
use crate::styles::checkbox::box_paint;
use crate::styles::metrics::{INDICATOR_BOX_INSET, INDICATOR_PRESSED_DARKER, INDICATOR_SIZE};

/// The colours a Breeze check box, radio button or switch is painted from, in
/// one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeIndicatorColors {
    /// `[qt]` The button colour: the face of a box, a disc and a switch's
    /// handle. A translucent button colour is made opaque over the window
    /// (`surface_main`), so a box is one opaque fill whatever lies behind
    /// it and its mark is lifted on what is painted under it.
    pub button: Color,
    /// `[Hp]` The button colour darkened 110 %, a pressed box's face.
    pub button_pressed: Color,
    /// `[qt]` The button text, which a switch's track is tinted toward.
    pub button_text: Color,
    /// `[Hp]` The button colour moved toward the button text by the frame
    /// contrast: a switch handle's outline at rest.
    pub button_outline: Color,
    /// `[Qd]` The button set's hover decoration: a hovered or focused switch
    /// handle's outline, and an off track's. Under high contrast it is
    /// lifted to 4.5:1 on the control surfaces and the button colour, as it
    /// stands in for the track's lifted outline.
    pub button_hover: Color,
    /// `[Hp]` The view's text: the check mark, the partial squares and the
    /// radio dot on a box or disc at rest.
    pub text: Color,
    /// The same mark on a pressed box or disc, whose face is darker.
    pub text_pressed: Color,
    /// `[Hp]` The current window's highlight (the muted one in a backdrop
    /// window, the disabled one for a disabled control): a checked box's
    /// outline and wash, a checked track.
    pub highlight: Color,
    /// `[wcag]` D6: the outline of an unchecked box, an unselected disc and
    /// an unchecked track, `border_strong`; `border_disabled` when disabled.
    pub outline: Color,
    /// `[wcag]` D6: the outline of a checked track, the highlight lifted in
    /// luma to the non-text floor (3:1, 4.5:1 under high contrast) on the
    /// window, the view, the frame background and the button colour; the
    /// disabled highlight, unlifted, when disabled.
    pub checked_outline: Color,
    /// `[Hp]` The view's focus decoration, the ring a hovered box or disc
    /// adds to its outline, stroked over it. Under high contrast it is
    /// lifted to 4.5:1 on the control surfaces and the button colour, as it
    /// covers the lifted outline.
    pub focus: Color,
    /// `[wcag]` D5: the keyboard focus frame, `focus_ring`.
    pub focus_ring: Color,
}

impl BreezeIndicatorColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference, for an enabled
    /// or a disabled control.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeIndicatorColors {
        let mut colors = Self::resolve_faces(theme, window_active, high_contrast, disabled);
        if !disabled {
            let unlifted = colors;
            colors.text = mark_on_marked_face(&unlifted, false, high_contrast);
            colors.text_pressed = mark_on_marked_face(&unlifted, true, high_contrast);
        }
        colors
    }

    /// [`resolve`](Self::resolve) before the mark is lifted on the faces.
    fn resolve_faces(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeIndicatorColors {
        let c = &theme.colors;
        let highlight = if disabled {
            c.accent_disabled
        } else {
            c.accent
        };
        let outline = if disabled {
            c.border_disabled
        } else {
            c.border_strong
        };
        let Some(palette) = theme.extension::<KdePalette>() else {
            let (button, button_text, button_outline, text) = if disabled {
                (
                    c.surface_disabled,
                    c.text_disabled,
                    c.border_disabled,
                    c.text_disabled,
                )
            } else {
                (c.surface_content, c.text_primary, c.border, c.editor_fg)
            };
            let button = opaque_over(button, c.surface_main);
            let hover = high_contrast_boundary(c.border_focused, c, button, high_contrast);
            return BreezeIndicatorColors {
                button,
                button_pressed: darker(button),
                button_text,
                button_outline,
                button_hover: hover,
                text,
                text_pressed: text,
                highlight,
                outline,
                checked_outline: checked_outline(theme, highlight, button, high_contrast, disabled),
                focus: hover,
                focus_ring: c.focus_ring,
            };
        };
        let state = if disabled {
            ColorState::Disabled
        } else if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let b = palette.breeze_colors(state, high_contrast);
        let button = opaque_over(b.button, c.surface_main);
        // The view's text: the projected token, which the high-contrast
        // projection lifts, or the disabled view text.
        let text = if disabled {
            palette.role(ColorState::Disabled, PaletteRole::Text)
        } else {
            c.editor_fg
        };
        BreezeIndicatorColors {
            button,
            button_pressed: darker(button),
            button_text: b.button_text,
            button_outline: b.button_outline,
            button_hover: high_contrast_boundary(
                palette
                    .resolved()
                    .set(state, ColorSet::Button)
                    .decoration(DecorationRole::Hover)
                    .to_color(),
                c,
                button,
                high_contrast,
            ),
            text,
            text_pressed: text,
            highlight,
            outline,
            checked_outline: checked_outline(theme, highlight, button, high_contrast, disabled),
            focus: high_contrast_boundary(b.focus, c, button, high_contrast),
            focus_ring: c.focus_ring,
        }
    }
}

/// `[wcag]` The mark of `colors` (the check mark, the partial squares and
/// the radio dot, in the view's text) on a marked box or disc, pressed or
/// not, lifted to the non-text floor, 3:1 or 4.5:1 under high contrast, on
/// its face: the highlight washed over the button colour as it is painted,
/// one opaque fill, since [`BreezeIndicatorColors::button`] is opaque.
/// The view's text reaches the text floor on the view, not on this face: a
/// scheme whose inactive effects move the button colour or the highlight
/// toward the text would otherwise leave the mark unreadable. Stock
/// Breeze's mark is kept.
fn mark_on_marked_face(
    colors: &BreezeIndicatorColors,
    pressed: bool,
    high_contrast: bool,
) -> Color {
    let paint = box_paint(true, false, pressed, false, colors);
    let face = paint.composited_fill().unwrap_or(paint.face);
    mark_on_faces(colors.text, &[face], high_contrast)
}

/// `[wcag]` D6: `highlight` lifted in luma until it reaches the non-text
/// floor on the control surfaces of `theme` and on `button`, the face of the
/// handle the outline meets; `highlight` itself when `disabled`, which
/// SC 1.4.11 exempts.
fn checked_outline(
    theme: &Theme,
    highlight: Color,
    button: Color,
    high_contrast: bool,
    disabled: bool,
) -> Color {
    if disabled {
        return highlight;
    }
    let c = &theme.colors;
    let floor = if high_contrast { ENHANCED } else { AA }.non_text;
    let on =
        [c.surface_main, c.surface_content, c.surface_raised, button].map(KdeColor::from_color);
    lift_luma_until(KdeColor::from_color(highlight), &on, floor).to_color()
}

/// KDE's darkening of a pressed indicator's button colour.
fn darker(c: Color) -> Color {
    hsv_darker(KdeColor::from_color(c), INDICATOR_PRESSED_DARKER).to_color()
}

/// `[Hp]` The 20 px indicator square inside a body laid out at `bounds`:
/// against the body's leading edge, centred vertically.
///
/// The body can be narrower than the indicator (see
/// [`indicator_body_width`](crate::styles::metrics::indicator_body_width)),
/// so the indicator is anchored to the leading edge in either direction and
/// what does not fit lies past the trailing one.
pub fn indicator_rect(bounds: Rect, direction: LayoutDirection) -> Rect {
    let x = match direction {
        LayoutDirection::LeftToRight => bounds.x,
        LayoutDirection::RightToLeft => bounds.x + bounds.width - INDICATOR_SIZE,
    };
    Rect::new(
        x,
        bounds.y + (bounds.height - INDICATOR_SIZE) * 0.5,
        INDICATOR_SIZE,
        INDICATOR_SIZE,
    )
}

/// `[Hp]` The box (or the disc's square) of an indicator: the indicator
/// inset by 2, 16 x 16.
pub fn box_rect(indicator: Rect) -> Rect {
    inset(indicator, INDICATOR_BOX_INSET)
}

/// `[Hp]` The rectangle the box is filled and stroked on: the box inset by
/// half the `pen`, 15 x 15 at 2.5 for the 1 px pen.
pub fn stroked_box_rect(indicator: Rect, pen: f32) -> Rect {
    inset(box_rect(indicator), pen * 0.5)
}

fn inset(r: Rect, d: f32) -> Rect {
    Rect::new(
        r.x + d,
        r.y + d,
        (r.width - 2.0 * d).max(0.0),
        (r.height - 2.0 * d).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    #[test]
    fn the_indicator_hugs_the_leading_edge_in_either_direction() {
        let body = Rect::new(10.0, 0.0, 18.0, 24.0);
        let ltr = indicator_rect(body, LayoutDirection::LeftToRight);
        assert_eq!(ltr, Rect::new(10.0, 2.0, 20.0, 20.0));
        let rtl = indicator_rect(body, LayoutDirection::RightToLeft);
        assert_eq!(rtl, Rect::new(8.0, 2.0, 20.0, 20.0));
        // The box lies in the body either way.
        for r in [ltr, rtl] {
            let b = box_rect(r);
            assert!(b.x >= body.x && b.x + b.width <= body.x + body.width);
        }
    }

    #[test]
    fn the_box_is_fifteen_pixels_at_two_and_a_half() {
        let i = Rect::new(0.0, 0.0, 20.0, 20.0);
        assert_eq!(box_rect(i), Rect::new(2.0, 2.0, 16.0, 16.0));
        assert_eq!(stroked_box_rect(i, 1.0), Rect::new(2.5, 2.5, 15.0, 15.0));
    }

    /// The Breeze Light colours, against Plasma's painted samples.
    #[test]
    fn breeze_light_paints_the_specifications_colours() {
        let t = crate::light();
        let c = BreezeIndicatorColors::resolve(&t, true, false, false);
        assert_eq!(rgba8(c.button), [252, 252, 252, 255]);
        // 252 darkened 110 % is 229, Plasma's pressed box.
        assert_eq!(rgba8(c.button_pressed), [229, 229, 229, 255]);
        assert_eq!(rgba8(c.text), [35, 38, 41, 255]);
        assert_eq!(rgba8(c.highlight), [61, 174, 233, 255]);
        assert_eq!(rgba8(c.focus), [61, 174, 233, 255]);
        assert_eq!(rgba8(c.button_hover), [61, 174, 233, 255]);
        assert_eq!(c.outline, t.colors.border_strong);
        assert_eq!(c.focus_ring, t.colors.focus_ring);
    }

    #[test]
    fn the_empty_outline_is_lifted_and_its_premise_holds() {
        let t = crate::light();
        let p = t.extension::<KdePalette>().unwrap();
        let separator = p.colors(ColorState::Active).separator;
        let window = t.colors.surface_main;
        assert!(
            separator.contrast_ratio(window) < 3.0,
            "premise: Breeze's own outline falls short"
        );
        let c = BreezeIndicatorColors::resolve(&t, true, false, false);
        assert!(c.outline.contrast_ratio(window) >= 3.0);
        assert!(c.outline.contrast_ratio(c.button) >= 3.0);
    }

    /// D6 for a checked switch: Breeze Light's highlight falls short of 3:1
    /// on its window, so the checked track is outlined in the highlight
    /// lifted to 3:1 on every control surface and on the handle's face.
    #[test]
    fn the_checked_outline_is_lifted_and_its_premise_holds() {
        let t = crate::light();
        let c = BreezeIndicatorColors::resolve(&t, true, false, false);
        let window = t.colors.surface_main;
        assert!(
            c.highlight.contrast_ratio(window) < 3.0,
            "premise: Breeze's own checked outline falls short"
        );
        for on in [
            window,
            t.colors.surface_content,
            t.colors.surface_raised,
            c.button,
        ] {
            assert!(c.checked_outline.contrast_ratio(on) >= 3.0, "{on:?}");
        }
        assert_ne!(c.checked_outline, c.highlight);
    }

    /// The lift is a rule: a highlight that already reaches 3:1 is kept
    /// exactly, a disabled control keeps the disabled highlight, and the
    /// high-contrast projection raises the floor to 4.5:1.
    #[test]
    fn the_checked_outline_follows_the_floor_of_each_projection() {
        let dark = crate::dark();
        let c = BreezeIndicatorColors::resolve(&dark, true, false, false);
        assert!(c.highlight.contrast_ratio(dark.colors.surface_main) >= 3.0);
        assert_eq!(c.checked_outline, c.highlight, "Breeze Dark passes");

        let light = crate::light();
        let disabled = BreezeIndicatorColors::resolve(&light, true, false, true);
        assert_eq!(disabled.checked_outline, light.colors.accent_disabled);

        for t in [crate::light(), crate::dark()] {
            let hc = t.for_paint_state(teksilo_core::styles::PaintState {
                window_active: true,
                high_contrast: true,
            });
            let c = BreezeIndicatorColors::resolve(&hc, true, true, false);
            for on in [hc.colors.surface_main, hc.colors.surface_content, c.button] {
                assert!(c.checked_outline.contrast_ratio(on) >= 4.5, "{on:?}");
            }
        }

        // Without the palette the rule reads the theme's own roles.
        let t = teksilo_core::presets::intui::light();
        let c = BreezeIndicatorColors::resolve(&t, true, false, false);
        assert!(c.checked_outline.contrast_ratio(t.colors.surface_main) >= 3.0);
    }

    #[test]
    fn a_backdrop_window_checks_in_its_muted_highlight() {
        let t = crate::light();
        let inactive = t.for_paint_state(teksilo_core::styles::PaintState {
            window_active: false,
            high_contrast: false,
        });
        let c = BreezeIndicatorColors::resolve(&inactive, false, false, false);
        assert_eq!(c.highlight, inactive.colors.accent);
        assert_ne!(c.highlight, t.colors.accent, "the current window's");
    }

    #[test]
    fn a_disabled_indicator_paints_the_disabled_colours() {
        let t = crate::light();
        let p = t.extension::<KdePalette>().unwrap();
        let c = BreezeIndicatorColors::resolve(&t, true, false, true);
        assert_eq!(c.button, p.role(ColorState::Disabled, PaletteRole::Button));
        assert_eq!(c.text, p.role(ColorState::Disabled, PaletteRole::Text));
        assert_eq!(c.highlight, t.colors.accent_disabled);
        assert_eq!(c.outline, t.colors.border_disabled);
    }

    /// A scheme may give the Button background an alpha, which D11 keeps.
    /// The box is then still one opaque fill, the button colour composited
    /// over the window with the wash over that, and the mark reaches the
    /// floor on it: Breeze Dark with the Button background at alpha 0.8
    /// and Inactive Lighten 0.4 left the mark 4.39:1 on the box as painted
    /// under high contrast, lifted against the bare face instead.
    #[test]
    fn a_translucent_button_colour_paints_one_opaque_box_its_mark_reads_on() {
        use teksilo_kde_scheme::{ColorSet, IntensityEffect, KdeColorScheme, StateEffect};
        let mut scheme = KdeColorScheme::breeze_dark();
        scheme.sets[ColorSet::Button.index()].background_normal = KdeColor::rgba8(41, 44, 48, 204);
        scheme.inactive_effect = StateEffect {
            enabled: true,
            intensity_effect: IntensityEffect::Lighten,
            intensity_amount: 0.4,
            ..StateEffect::INACTIVE_DEFAULT
        };
        let theme = crate::from_scheme(&scheme);
        let hc = theme.for_paint_state(teksilo_core::styles::PaintState {
            window_active: false,
            high_contrast: true,
        });
        let p = hc.extension::<KdePalette>().unwrap();
        assert!(
            p.breeze_colors(ColorState::Inactive, true).button.a() < 1.0,
            "premise: the carried button colour keeps the scheme's alpha"
        );
        let c = BreezeIndicatorColors::resolve(&hc, false, true, false);
        assert_eq!(c.button.a(), 1.0);
        assert_eq!(c.button_pressed.a(), 1.0);
        for pressed in [false, true] {
            let paint = box_paint(true, false, pressed, false, &c);
            let fill = paint.composited_fill().expect("one opaque fill");
            let ratio = paint.mark.contrast_ratio(fill);
            assert!(ratio >= 4.5, "pressed {pressed}: {ratio:.2}:1");
        }
    }

    #[test]
    fn a_theme_without_the_palette_paints_from_its_roles() {
        let t = teksilo_core::presets::intui::light();
        let c = BreezeIndicatorColors::resolve(&t, true, false, false);
        assert_eq!(c.button, t.colors.surface_content);
        assert_eq!(c.button_outline, t.colors.border);
        assert_eq!(c.text, t.colors.editor_fg);
        assert_eq!(c.highlight, t.colors.accent);
        assert_eq!(c.outline, t.colors.border_strong);
        assert_eq!(c.focus, t.colors.border_focused);
    }
}
