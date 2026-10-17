// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze shape tokens: corner radii, stroke widths, the focus ring and the
//! shadows.
//!
//! ## Radii
//!
//! Breeze has one frame radius, 5 px, for controls and for floating
//! surfaces alike, which Kirigami repeats as its corner radius. A stroked
//! frame is drawn on a rectangle inset by half the 1 px pen, so the outline
//! a user sees has a radius of 4.5; the token carries the 5 and the styles
//! inset their own strokes. Smaller radii belong to individual parts and are
//! the constants below: the check box (4), the progress and slider grooves
//! (3) and the scroll-bar handle (4).
//!
//! ## The focus ring
//!
//! Breeze marks keyboard focus with a ring 2 px wide of the Highlight colour
//! at 30 % alpha, running from 1 px inside the control's edge to 1 px
//! outside it. That is a width of 2 and an offset of -1, which is what the
//! tokens say; the colour is the theme's `focus_ring`.
//!
//! ## Shadows
//!
//! A Breeze window shadow (menus, tooltips, combo-box popups) has two
//! layers, and its size is the `breezerc` `ShadowSize` setting, Large by
//! default. The tokens carry the Small, Medium and Large shadows, first
//! layer in `shadow_sm` / `shadow_md` / `shadow_lg` and second layer in the
//! matching `shadow_inner_*`; the styles paint both. `shadow_xs` is the push
//! button's own 1 px shadow, which has no second layer. Every offset in the
//! table is the size's offset plus the layer's, and the blur is Breeze's
//! radius, a CSS blur radius. The colour is black, at each layer's opacity.
//! Breeze's shadows do not change with the colour scheme, so light and dark
//! share one set of shape tokens. A desktop that changes the shadow's size,
//! colour or strength in `breezerc` gets its own window shadow through
//! [`breeze_shape_for`], which [`crate::from_desktop`] uses.

use teksilo_kde_scheme::ShadowSize;
use teksilo_tokens::{Color, Shadow, ShapeTokens};

/// `[M]` The frame radius of a control: push button, field, combo box, menu
/// item, tab, row highlight. Also `[K]` Kirigami's corner radius.
pub const BREEZE_CONTROL_RADIUS: f32 = 5.0;
/// `[M]` The radius of a floating surface: menu, tooltip, popup, dialog
/// frame. Breeze uses the frame radius here too.
pub const BREEZE_POPUP_RADIUS: f32 = 5.0;
/// `[Hp]` The radius a 1 px stroked frame is drawn with: the frame radius
/// less half the pen, because the stroke lies on the rectangle inset by
/// half the pen.
pub const BREEZE_STROKED_FRAME_RADIUS: f32 = 4.5;
/// `[M]` The check box's radius: the frame radius less one.
pub const BREEZE_CHECKBOX_RADIUS: f32 = 4.0;
/// `[Hp]` The radius of the progress-bar and slider grooves.
pub const BREEZE_GROOVE_RADIUS: f32 = 3.0;
/// `[Hp]` The radius of the scroll-bar handle: half its 8 px width.
pub const BREEZE_SCROLL_HANDLE_RADIUS: f32 = 4.0;
/// `[M]` The pen every frame, separator and symbol is drawn with. Breeze
/// writes 1.001, an antialiasing workaround; Teksilo draws 1.
pub const BREEZE_PEN_WIDTH: f32 = 1.0;
/// `[Hp]` The focus ring's width.
pub const BREEZE_FOCUS_RING_WIDTH: f32 = 2.0;
/// `[Hp]` The focus ring's offset from the control edge: it starts 1 px
/// inside the edge.
pub const BREEZE_FOCUS_RING_OFFSET: f32 = -1.0;
/// `[Hp]` The alpha the focus ring's Highlight is painted at.
pub const BREEZE_FOCUS_RING_ALPHA: f32 = 0.3;
/// `[Hp]` The focus ring's outer radius: the inner edge has the frame
/// radius and the ring grows it by its width.
pub const BREEZE_FOCUS_RING_OUTER_RADIUS: f32 = 7.0;

/// `[Hp]` The push button's shadow: a 1 px line of black at 12.5 % below the
/// button, which is an offset of (0, 1) with no blur.
pub const BREEZE_BUTTON_SHADOW_ALPHA: f32 = 0.125;

/// `[Hp]` The overlap of a window and its shadow.
pub const BREEZE_SHADOW_OVERLAP: f32 = 2.0;

/// One layer of a window shadow: vertical offset, blur and opacity, the
/// size's offset already added in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLayer {
    /// Downward offset in logical pixels.
    pub offset_y: f32,
    /// Blur radius in logical pixels.
    pub blur: f32,
    /// Opacity of the shadow colour, before `ShadowStrength`.
    pub opacity: f32,
}

/// `[Hp]` The two layers of a Breeze window shadow of `size`, `None` for
/// [`ShadowSize::None`].
///
/// | Size | Layer 1 (offset, blur, opacity) | Layer 2 |
/// | --- | --- | --- |
/// | Small | 3, 12, 0.26 | 1, 6, 0.16 |
/// | Medium | 4, 16, 0.24 | 2, 8, 0.14 |
/// | Large | 5, 20, 0.22 | 2, 10, 0.12 |
/// | Very large | 6, 24, 0.20 | 3, 12, 0.10 |
pub const fn window_shadow_layers(size: ShadowSize) -> Option<[ShadowLayer; 2]> {
    const fn layer(offset_y: f32, blur: f32, opacity: f32) -> ShadowLayer {
        ShadowLayer {
            offset_y,
            blur,
            opacity,
        }
    }
    match size {
        ShadowSize::None => None,
        ShadowSize::Small => Some([layer(3.0, 12.0, 0.26), layer(1.0, 6.0, 0.16)]),
        ShadowSize::Medium => Some([layer(4.0, 16.0, 0.24), layer(2.0, 8.0, 0.14)]),
        ShadowSize::Large => Some([layer(5.0, 20.0, 0.22), layer(2.0, 10.0, 0.12)]),
        ShadowSize::VeryLarge => Some([layer(6.0, 24.0, 0.20), layer(3.0, 12.0, 0.10)]),
    }
}

/// A shadow token for `layer` in `color`, at the layer's opacity times
/// `strength` (`breezerc` `ShadowStrength / 255`, 1 by default).
pub fn shadow_token(layer: ShadowLayer, color: Color, strength: f32) -> Shadow {
    Shadow {
        offset_x: 0.0,
        offset_y: layer.offset_y,
        blur: layer.blur,
        spread: 0.0,
        color: color.with_alpha(layer.opacity * strength.clamp(0.0, 1.0)),
    }
}

/// Breeze shape tokens, for both colour schemes, at Breeze's default
/// window shadow: Large, black, at full strength.
pub fn breeze_shape() -> ShapeTokens {
    breeze_shape_for(ShadowSize::Large, Color::BLACK, 1.0)
}

/// Breeze shape tokens with the window shadow a desktop configures:
/// `breezerc`'s `ShadowSize` as `window`, `ShadowColor` as `color` and
/// `ShadowStrength / 255` as `strength`.
///
/// `shadow_lg` and `shadow_inner_lg` are the window shadow the popups
/// paint, so they carry `window`'s two layers, and nothing at all for
/// [`ShadowSize::None`]; `shadow_sm` and `shadow_md` stay the Small and
/// Medium layers. All of them take `color` at each layer's opacity times
/// `strength`. The push button's own shadow, `shadow_xs`, is not a window
/// shadow and does not change.
pub fn breeze_shape_for(window: ShadowSize, color: Color, strength: f32) -> ShapeTokens {
    let none = Shadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 0.0,
        spread: 0.0,
        color: Color::TRANSPARENT,
    };
    let pair = |size: ShadowSize| -> (Shadow, Shadow) {
        match window_shadow_layers(size) {
            Some([outer, inner]) => (
                shadow_token(outer, color, strength),
                shadow_token(inner, color, strength),
            ),
            None => (none, none),
        }
    };
    let (sm, inner_sm) = pair(ShadowSize::Small);
    let (md, inner_md) = pair(ShadowSize::Medium);
    let (lg, inner_lg) = pair(window);
    ShapeTokens {
        radius_control: BREEZE_CONTROL_RADIUS,
        radius_popup: BREEZE_POPUP_RADIUS,
        // `[unpublished]` Breeze has no pill shape; the framework's fully
        // rounded value, for the widgets that ask for one.
        radius_pill: ShapeTokens::light_default().radius_pill,
        border_width: BREEZE_PEN_WIDTH,
        focus_ring_width: BREEZE_FOCUS_RING_WIDTH,
        focus_ring_offset: BREEZE_FOCUS_RING_OFFSET,
        shadow_xs: Shadow {
            offset_x: 0.0,
            offset_y: 1.0,
            blur: 0.0,
            spread: 0.0,
            color: Color::BLACK.with_alpha(BREEZE_BUTTON_SHADOW_ALPHA),
        },
        // `[Hp]` The button shadow has one layer: the second is empty.
        shadow_inner_xs: Shadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow_sm: sm,
        shadow_inner_sm: inner_sm,
        shadow_md: md,
        shadow_inner_md: inner_md,
        shadow_lg: lg,
        shadow_inner_lg: inner_lg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_popups_share_the_frame_radius() {
        let s = breeze_shape();
        assert_eq!(s.radius_control, 5.0);
        assert_eq!(s.radius_popup, 5.0);
        assert_eq!(s.border_width, 1.0);
        assert_eq!(
            BREEZE_STROKED_FRAME_RADIUS,
            BREEZE_CONTROL_RADIUS - BREEZE_PEN_WIDTH / 2.0
        );
        assert_eq!(BREEZE_CHECKBOX_RADIUS, BREEZE_CONTROL_RADIUS - 1.0);
    }

    #[test]
    fn the_focus_ring_runs_from_one_pixel_inside_to_one_pixel_outside() {
        let s = breeze_shape();
        assert_eq!(s.focus_ring_width, 2.0);
        assert_eq!(s.focus_ring_offset, -1.0);
        // One pixel of the ring lies outside the control and is all it
        // reserves; the stroke's centre line is on the edge itself.
        assert_eq!(s.focus_ring_envelope(), 1.0);
        assert_eq!(s.focus_ring_stroke_distance(), 0.0);
        assert_eq!(
            BREEZE_FOCUS_RING_OUTER_RADIUS,
            BREEZE_CONTROL_RADIUS + BREEZE_FOCUS_RING_WIDTH
        );
    }

    #[test]
    fn the_window_shadows_are_the_breeze_table() {
        let s = breeze_shape();
        let check = |shadow: Shadow, offset_y: f32, blur: f32, alpha: f32| {
            assert_eq!(shadow.offset_x, 0.0);
            assert_eq!(shadow.offset_y, offset_y);
            assert_eq!(shadow.blur, blur);
            assert_eq!(shadow.spread, 0.0);
            assert!((shadow.color.a() - alpha).abs() < 1e-6);
            assert_eq!(shadow.color.with_alpha(1.0), Color::BLACK);
        };
        check(s.shadow_sm, 3.0, 12.0, 0.26);
        check(s.shadow_inner_sm, 1.0, 6.0, 0.16);
        check(s.shadow_md, 4.0, 16.0, 0.24);
        check(s.shadow_inner_md, 2.0, 8.0, 0.14);
        check(s.shadow_lg, 5.0, 20.0, 0.22);
        check(s.shadow_inner_lg, 2.0, 10.0, 0.12);
        check(s.shadow_xs, 1.0, 0.0, 0.125);
        assert_eq!(s.shadow_inner_xs.color.a(), 0.0);
    }

    #[test]
    fn no_shadow_has_no_layers_and_strength_scales_the_opacity() {
        assert_eq!(window_shadow_layers(ShadowSize::None), None);
        let [outer, _] = window_shadow_layers(ShadowSize::VeryLarge).unwrap();
        assert_eq!((outer.offset_y, outer.blur), (6.0, 24.0));
        let half = shadow_token(outer, Color::BLACK, 0.5);
        assert!((half.color.a() - 0.10).abs() < 1e-6);
        // A strength outside 0..=1 is clamped rather than amplifying.
        assert!((shadow_token(outer, Color::BLACK, 3.0).color.a() - 0.20).abs() < 1e-6);
    }

    #[test]
    fn the_desktops_window_shadow_is_the_popups() {
        let red = Color::from_hex("#FF0000");
        let s = breeze_shape_for(ShadowSize::Small, red, 0.5);
        // The window shadow is the Small pair, in the colour, at half the
        // opacity.
        assert_eq!((s.shadow_lg.offset_y, s.shadow_lg.blur), (3.0, 12.0));
        assert!((s.shadow_lg.color.a() - 0.13).abs() < 1e-6);
        assert_eq!(s.shadow_lg.color.with_alpha(1.0), red);
        assert_eq!(
            (s.shadow_inner_lg.offset_y, s.shadow_inner_lg.blur),
            (1.0, 6.0)
        );
        assert!((s.shadow_inner_lg.color.a() - 0.08).abs() < 1e-6);
        // The Small and Medium tokens keep their sizes in the same colour.
        assert_eq!((s.shadow_md.offset_y, s.shadow_md.blur), (4.0, 16.0));
        assert_eq!(s.shadow_md.color.with_alpha(1.0), red);
        // The button's shadow is not a window shadow.
        assert_eq!(s.shadow_xs, breeze_shape().shadow_xs);
        // No window shadow at all.
        let bare = breeze_shape_for(ShadowSize::None, Color::BLACK, 1.0);
        assert_eq!(bare.shadow_lg.color.a(), 0.0);
        assert_eq!(bare.shadow_inner_lg.color.a(), 0.0);
        assert_eq!(bare.shadow_sm, breeze_shape().shadow_sm);
        // The defaults are Large, black, at full strength.
        assert_eq!(
            breeze_shape_for(ShadowSize::Large, Color::BLACK, 1.0),
            breeze_shape()
        );
    }

    #[test]
    fn larger_shadows_reach_further_and_fade_more() {
        let sizes = [
            ShadowSize::Small,
            ShadowSize::Medium,
            ShadowSize::Large,
            ShadowSize::VeryLarge,
        ];
        for pair in sizes.windows(2) {
            let [a, _] = window_shadow_layers(pair[0]).unwrap();
            let [b, _] = window_shadow_layers(pair[1]).unwrap();
            assert!(b.blur > a.blur && b.offset_y > a.offset_y);
            assert!(b.opacity < a.opacity);
        }
    }
}
