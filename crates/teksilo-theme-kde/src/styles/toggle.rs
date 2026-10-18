// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze switch.
//!
//! Plasma's widget style has no switch: the one KDE applications show is the
//! desktop QtQuick style's, which Kirigami applications use (`[Qd]`), and
//! this transcribes it. It is drawn in the **button** colours.
//!
//! - **The indicator** is 36 x 18, Kirigami's grid unit tall and twice as
//!   wide.
//! - **The track** is the indicator inset by 3 above and below, 36 x 12 at
//!   a 6 px radius, filled with the button colour moved 10 % toward the
//!   button text and outlined in 1 px: the highlight when on, otherwise the
//!   handle's outline. On, the track is filled again with the highlight at
//!   50 % from its leading end to the handle's trailing edge.
//! - **The handle** is a circle of the indicator's full height, 18 px,
//!   filled with the button colour and outlined in 1 px: the button set's
//!   hover decoration while hovered or focused from the keyboard, otherwise
//!   the button colour moved toward the button text by the frame contrast.
//!   Its leading edge is at `clamp(p x 36 - 9, 0, 18)` for the position `p`,
//!   0 off and 1 on, mirrored right to left.
//!
//! ## Motion
//!
//! The handle moves on Qt's smoothed animation: Kirigami's short duration,
//! 100 ms, or the time it takes at Qt's default 200 px a second if that is
//! shorter, so the 18 px travel takes 90 ms, on the quadratic ease-in-out
//! approximated by [`BREEZE_SWITCH_HANDLE_EASING`]. The handle's outline
//! changes colour over the short duration on the cubic ease-in,
//! [`BREEZE_SWITCH_COLOR_EASING`]. Both snap under the reduced-motion
//! preference.
//!
//! ## Where this departs from the desktop style
//!
//! - **D6, the track's outline.** Off and at rest, the track is outlined in
//!   the handle's outline, which is 1.3:1 on the Breeze Light window; it is
//!   the only boundary of the off switch, so it is drawn in `border_strong`,
//!   lifted to 3:1 (D6 of the colour mapping). On, it is outlined in the
//!   highlight, 2.2:1 on that window, and is just as much the only boundary,
//!   so it is drawn in the highlight lifted the same way,
//!   [`checked_outline`](BreezeIndicatorColors::checked_outline), which
//!   Breeze Dark's highlight already passes and keeps. The lift includes the
//!   button colour, the handle's face, so in either state the handle stands
//!   out at 3:1 where the track's outline meets it. Hovered, the off track
//!   and the handle are outlined in the button set's hover decoration,
//!   which under high contrast is lifted to 4.5:1 on the control surfaces
//!   and the handle's face, so the off switch keeps its boundary
//!   ([`BreezeIndicatorColors::button_hover`]).
//! - **The handle keeps its colours.** Its face, the button colour, is
//!   1.7:1 on the checked fill and its outline 1.3:1 on the Breeze Light
//!   window, as the desktop style paints them. The handle's position is
//!   shown by where it interrupts the track's lifted outline, which is 3:1
//!   on its face.
//! - **D5, keyboard focus.** The desktop style shows keyboard focus only by
//!   the handle's hover outline. The switch adds the 2 px focus frame the
//!   Breeze widget style draws around a check box, in the opaque
//!   `focus_ring` token, from the indicator's edge outward (`[derived]`).
//! - **The target.** The 36 x 18 indicator is painted unchanged, centred in
//!   a node raised to the density's target size on both axes (36 x 24 at
//!   Compact), which WCAG 2.2 SC 2.5.8 asks of a control.
//! - **The label** is the `Toggle`'s, 6 px after the node; the desktop style
//!   puts it 4 px after the indicator.

use std::time::Duration;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::Signal;
use teksilo_core::styles::density::density_min_size;
use teksilo_core::styles::{FocusRingStroke, ToggleStyle, ToggleStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, TargetAxes};

use crate::motion::{BREEZE_SWITCH_COLOR_EASING, BREEZE_SWITCH_HANDLE_EASING};
use crate::styles::chrome::kde_mix;
use crate::styles::indicator::BreezeIndicatorColors;
use crate::styles::metrics::{
    SWITCH_CHECKED_ALPHA, SWITCH_HANDLE_RADIUS, SWITCH_HANDLE_VELOCITY, SWITCH_HEIGHT,
    SWITCH_TRACK_INSET, SWITCH_TRACK_RADIUS, SWITCH_TRACK_TEXT, SWITCH_WIDTH,
};

/// The handle's travel, from off to on.
const TRAVEL: f32 = SWITCH_WIDTH - SWITCH_HEIGHT;

/// `[Qd]` How far the handle's leading edge lies from the indicator's leading
/// edge at the visual position `position` (0 off, 1 on).
pub fn handle_offset(position: f32) -> f32 {
    (position * SWITCH_WIDTH - SWITCH_HANDLE_RADIUS).clamp(0.0, TRAVEL)
}

/// `[qt]` How long the handle takes to travel: the short duration, or the
/// time the full travel takes at Qt's default velocity if that is shorter.
pub fn handle_duration(short: Duration) -> Duration {
    short.min(Duration::from_secs_f32(TRAVEL / SWITCH_HANDLE_VELOCITY))
}

/// Breeze `ToggleStyle`: the desktop QtQuick style's switch.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeToggleStyle;

impl ToggleStyle for BreezeToggleStyle {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let short = ctx.theme().motion.duration_fast;
        let target = |on: bool| handle_offset(if on { 1.0 } else { 0.0 });

        let offset = ctx.animated_signal(target(cfg.is_on.get()));
        let slide = ctx
            .animate()
            .duration(handle_duration(short))
            .easing(BREEZE_SWITCH_HANDLE_EASING);
        {
            let offset = offset.clone();
            ctx.effect(&cfg.is_on, move |on| slide.to_or_snap(&offset, target(*on)));
        }

        // The handle's outline: hovered, or focused from the keyboard.
        let emphasised = cfg
            .is_hovered
            .zip3(&cfg.is_focused, &cfg.is_focus_visible)
            .map(|(hovered, focused, visible)| *hovered || (*focused && *visible));
        let emphasis = ctx.animated_signal(if emphasised.get() { 1.0 } else { 0.0 });
        let tint = ctx.animate().fast().easing(BREEZE_SWITCH_COLOR_EASING);
        {
            let emphasis = emphasis.clone();
            ctx.effect(&emphasised, move |on| {
                tint.to_or_snap(&emphasis, if *on { 1.0 } else { 0.0 })
            });
        }

        let show_focus_frame = cfg
            .is_focused
            .zip3(&cfg.is_focus_visible, &cfg.is_disabled)
            .map(|(focused, visible, disabled)| *focused && *visible && !*disabled);

        ctx.add(BreezeSwitchBody {
            offset,
            emphasis,
            is_disabled: cfg.is_disabled.clone(),
            show_focus_frame,
        })
    }
}

struct BreezeSwitchBody {
    /// The handle's leading edge from the indicator's, 0 to 18.
    offset: Signal<f32>,
    /// How far the handle's outline has turned to the hover colour, 0 to 1.
    emphasis: Signal<f32>,
    is_disabled: Signal<bool>,
    show_focus_frame: Signal<bool>,
}

impl std::fmt::Debug for BreezeSwitchBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeSwitchBody").finish()
    }
}

/// What a switch paints at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwitchPaint {
    /// The track: the button colour moved 10 % toward the button text.
    pub track: Color,
    /// The track's outline.
    pub track_outline: Color,
    /// The highlight at 50 % from the track's leading end to the handle's
    /// trailing edge.
    pub checked_fill: Color,
    /// The handle's face.
    pub handle: Color,
    /// The handle's outline.
    pub handle_outline: Color,
}

/// `[Qd]` The colours of a switch whose handle has travelled `on` of the way
/// (0 off, 1 on) and whose outline has turned `emphasis` of the way to the
/// hover colour, from `c`.
pub fn switch_paint(on: f32, emphasis: f32, c: &BreezeIndicatorColors) -> SwitchPaint {
    let on = on.clamp(0.0, 1.0);
    let emphasis = emphasis.clamp(0.0, 1.0);
    let handle_outline = c.button_outline.mix(c.button_hover, emphasis);
    // D6: the track's outline is the lifted one, off at rest and on.
    let off_outline = c.outline.mix(c.button_hover, emphasis);
    SwitchPaint {
        track: kde_mix(c.button, c.button_text, SWITCH_TRACK_TEXT),
        track_outline: off_outline.mix(c.checked_outline, on),
        checked_fill: c.highlight.with_alpha(SWITCH_CHECKED_ALPHA),
        handle: c.button,
        handle_outline,
    }
}

/// `[Qd]` The 36 x 18 indicator, centred in the node `bounds`.
pub fn switch_indicator(bounds: Rect) -> Rect {
    Rect::new(
        bounds.x + (bounds.width - SWITCH_WIDTH) * 0.5,
        bounds.y + (bounds.height - SWITCH_HEIGHT) * 0.5,
        SWITCH_WIDTH,
        SWITCH_HEIGHT,
    )
}

/// `[Qd]` The track: the indicator inset by 3 above and below.
pub fn switch_track(indicator: Rect) -> Rect {
    Rect::new(
        indicator.x,
        indicator.y + SWITCH_TRACK_INSET,
        indicator.width,
        (indicator.height - 2.0 * SWITCH_TRACK_INSET).max(0.0),
    )
}

/// The handle's square, `offset` from the indicator's leading edge in
/// `direction`.
pub fn switch_handle(indicator: Rect, offset: f32, direction: LayoutDirection) -> Rect {
    let x = match direction {
        LayoutDirection::LeftToRight => indicator.x + offset,
        LayoutDirection::RightToLeft => indicator.x + indicator.width - SWITCH_HEIGHT - offset,
    };
    Rect::new(x, indicator.y, SWITCH_HEIGHT, SWITCH_HEIGHT)
}

/// The checked part of the track: from its leading end to the handle's
/// trailing edge.
pub fn switch_checked_part(track: Rect, handle: Rect, direction: LayoutDirection) -> Rect {
    match direction {
        LayoutDirection::LeftToRight => Rect::new(
            track.x,
            track.y,
            (handle.x + handle.width - track.x).max(0.0),
            track.height,
        ),
        LayoutDirection::RightToLeft => Rect::new(
            handle.x,
            track.y,
            (track.x + track.width - handle.x).max(0.0),
            track.height,
        ),
    }
}

impl Widget for BreezeSwitchBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.offset.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.emphasis
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_focus_frame
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    /// The indicator, raised to the density's target size on both axes; the
    /// paint stays 36 x 18, centred.
    fn layout_response(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        density_min_size(
            Size::new(SWITCH_WIDTH, SWITCH_HEIGHT),
            TargetAxes::BOTH,
            &ctx.theme.input,
        )
        .into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = BreezeIndicatorColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            self.is_disabled.get(),
        );
        let offset = self.offset.get().clamp(0.0, TRAVEL);
        let paint = switch_paint(offset / TRAVEL, self.emphasis.get(), &colors);
        let pen = ctx.theme.shape.border_width;
        let direction = ctx.layout_direction;

        let indicator = switch_indicator(bounds);
        let track = switch_track(indicator);
        let handle = switch_handle(indicator, offset, direction);
        let track_corner = CornerRadius::uniform(SWITCH_TRACK_RADIUS);

        canvas.fill_rounded_rect(track, track_corner, paint.track);
        if offset > 0.0 && paint.checked_fill.a() > 0.0 {
            canvas.fill_rounded_rect(
                switch_checked_part(track, handle, direction),
                track_corner,
                paint.checked_fill,
            );
        }
        if pen > 0.0 && paint.track_outline.a() > 0.0 {
            // A Qt Quick border lies inside its rectangle.
            canvas.stroke_rounded_rect(
                inset(track, pen * 0.5),
                CornerRadius::uniform((SWITCH_TRACK_RADIUS - pen * 0.5).max(0.0)),
                paint.track_outline,
                pen,
            );
        }

        let centre = Point::new(
            handle.x + SWITCH_HANDLE_RADIUS,
            handle.y + SWITCH_HANDLE_RADIUS,
        );
        canvas.fill_circle(centre, SWITCH_HANDLE_RADIUS, paint.handle);
        if pen > 0.0 && paint.handle_outline.a() > 0.0 {
            canvas.stroke_circle(
                centre,
                SWITCH_HANDLE_RADIUS - pen * 0.5,
                paint.handle_outline,
                pen,
            );
        }

        if self.show_focus_frame.get() {
            let shape = &ctx.theme.shape;
            if colors.focus_ring.a() > 0.0 && shape.focus_ring_width > 0.0 {
                let ring = FocusRingStroke::at_distance(
                    indicator,
                    SWITCH_HEIGHT * 0.5,
                    shape.focus_ring_width * 0.5,
                );
                canvas.stroke_rounded_rect(
                    ring.rect,
                    CornerRadius::uniform(ring.corner_radius),
                    colors.focus_ring,
                    shape.focus_ring_width,
                );
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the `Toggle` emits the switch node.
        builder.set_hidden();
    }
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
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    #[test]
    fn the_handle_follows_the_desktop_styles_position_rule() {
        assert_eq!(handle_offset(0.0), 0.0);
        assert_eq!(handle_offset(1.0), 18.0);
        // It moves only over the middle half of the position.
        assert_eq!(handle_offset(0.25), 0.0);
        assert_eq!(handle_offset(0.5), 9.0);
        assert_eq!(handle_offset(0.75), 18.0);
    }

    #[test]
    fn the_travel_takes_ninety_milliseconds_at_the_default_speed() {
        let d = handle_duration(Duration::from_millis(100));
        assert!((d.as_secs_f32() - 0.09).abs() < 1e-6, "{d:?}");
        // A shorter short duration wins.
        assert_eq!(
            handle_duration(Duration::from_millis(50)),
            Duration::from_millis(50)
        );
        assert_eq!(handle_duration(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn the_geometry_is_the_desktop_styles() {
        let node = Rect::new(0.0, 0.0, 36.0, 24.0);
        let i = switch_indicator(node);
        assert_eq!(i, Rect::new(0.0, 3.0, 36.0, 18.0));
        assert_eq!(switch_track(i), Rect::new(0.0, 6.0, 36.0, 12.0));
        let off = switch_handle(i, 0.0, LayoutDirection::LeftToRight);
        let on = switch_handle(i, 18.0, LayoutDirection::LeftToRight);
        assert_eq!(off, Rect::new(0.0, 3.0, 18.0, 18.0));
        assert_eq!(on, Rect::new(18.0, 3.0, 18.0, 18.0));
        // Mirrored right to left.
        let rtl_off = switch_handle(i, 0.0, LayoutDirection::RightToLeft);
        assert_eq!(rtl_off, on);
        let rtl_on = switch_handle(i, 18.0, LayoutDirection::RightToLeft);
        assert_eq!(rtl_on, off);
        // The checked part runs from the leading end to the handle's trailing
        // edge: the whole track when on, either way.
        let t = switch_track(i);
        assert_eq!(switch_checked_part(t, on, LayoutDirection::LeftToRight), t);
        assert_eq!(
            switch_checked_part(t, rtl_on, LayoutDirection::RightToLeft),
            t
        );
    }

    /// The figures the module documentation gives for the handle, which
    /// keeps the desktop style's colours, and the 3:1 its position is read
    /// at in either state.
    #[test]
    fn the_handle_keeps_its_colours_and_meets_a_lifted_outline() {
        let t = crate::light();
        let c = BreezeIndicatorColors::resolve(&t, true, false, false);
        let off = switch_paint(0.0, 0.0, &c);
        let on = switch_paint(1.0, 0.0, &c);
        let checked = over(on.checked_fill, on.track);
        let face = on.handle.contrast_ratio(checked);
        assert!((1.65..1.75).contains(&face), "{face}");
        let outline = on.handle_outline.contrast_ratio(t.colors.surface_main);
        assert!((1.3..1.4).contains(&outline), "{outline}");
        for p in [off, on] {
            assert!(p.track_outline.contrast_ratio(p.handle) >= 3.0, "{p:?}");
        }
    }

    /// `top` composited over the opaque `bottom`.
    fn over(top: Color, bottom: Color) -> Color {
        bottom.with_alpha(1.0).mix(top.with_alpha(1.0), top.a())
    }

    #[test]
    fn breeze_lights_switch_colours() {
        let c = BreezeIndicatorColors::resolve(&crate::light(), true, false, false);
        let p = switch_paint(0.0, 0.0, &c);
        // 252 moved 10 % toward 35,38,41.
        assert_eq!(rgba8(p.track), [230, 231, 231, 255]);
        assert_eq!(p.handle, c.button);
        assert_eq!(p.handle_outline, c.button_outline);
        assert_eq!(p.track_outline, c.outline, "D6 at rest");
        let on = switch_paint(1.0, 0.0, &c);
        assert_eq!(on.track_outline, c.checked_outline, "D6 when on");
        assert_ne!(on.track_outline, c.highlight, "Breeze Light's is lifted");
        let on_hovered = switch_paint(1.0, 1.0, &c);
        assert_eq!(on_hovered.track_outline, c.checked_outline);
        assert_eq!(on.checked_fill, c.highlight.with_alpha(0.5));
        let hovered = switch_paint(0.0, 1.0, &c);
        assert_eq!(hovered.handle_outline, c.button_hover);
        assert_eq!(hovered.track_outline, c.button_hover);
    }
}
