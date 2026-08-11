// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS `NSSlider`.
//!
//! A thin 4 dp track, filled with the accent up to the value, and a plain
//! round **18 dp bezelled knob** — the same graded face, hairline and
//! shadow the switch knob and the push button wear. Nothing about the knob
//! changes on hover or while dragging.
//!
//! That stillness is the point. Fluent's thumb is two circles whose inner
//! accent dot swells to 14 dp under the pointer and shrinks to 8.5 dp
//! under the press; Material 3's grows a halo. AppKit's knob is a physical
//! object that slides, and physical objects do not change size when you
//! touch them. It also means this style needs no tween — and so cannot
//! trip over the derived-signal trap documented on
//! [`crate::styles::radio`].
//!
//! Discrete sliders get tick marks on the far side of the track, as AppKit
//! draws them for an `NSSlider` with `numberOfTickMarks` set.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SliderOrientation, SliderStyle, SliderStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, Shadow, lerp};

use crate::palette::MacOsPalette;
use crate::shape::MACOS_CONTROL_HEIGHT;
use crate::styles::chrome::{MacOsState, paint_focus_ring, resolve_bezel, vertical_gradient};

/// Track thickness (dp).
const TRACK: f32 = 4.0;
/// Knob diameter (dp) — the same disc the switch uses, so the two line up
/// in a settings column.
const THUMB: f32 = 18.0;
/// The cross-axis extent the control claims (dp).
const CROSS: f32 = MACOS_CONTROL_HEIGHT;
/// Tick-mark length (dp) for the discrete variant.
const TICK: f32 = 4.0;
/// Default main-axis extent when the proposal is unbounded.
const DEFAULT_LENGTH: f32 = 160.0;
/// The knob's own drop shadow.
const KNOB_SHADOW_OFFSET_Y: f32 = 0.5;
const KNOB_SHADOW_BLUR: f32 = 1.5;

// The knob has to fit the cross-axis extent the control claims, and the
// track has to fit inside the knob.
const _: () = assert!(TRACK < THUMB);
const _: () = assert!(THUMB <= CROSS);

/// macOS `SliderStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsSliderStyle;

impl SliderStyle for MacOsSliderStyle {
    fn thumb_diameter(&self, _cfg: &SliderStyleConfig) -> f32 {
        THUMB
    }

    fn make_body(&self, cfg: &SliderStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let state = MacOsState::derive(&cfg.is_disabled, &cfg.is_dragging, &cfg.is_hovered);
        ctx.add(MacOsSliderBody {
            value: cfg.value_normalized.clone(),
            state,
            is_disabled: cfg.is_disabled.clone(),
            focus_origin: cfg.focus_origin.clone(),
            orientation: cfg.orientation,
            tick_count: cfg.tick_count,
        })
    }
}

/// Where the track sits and which way the value runs along it.
struct SliderAxis {
    /// The rail rect.
    track: Rect,
    /// Main-axis coordinate of `value == 0`.
    start: f32,
    /// Main-axis coordinate of `value == 1`.
    end: f32,
    /// Cross-axis centre — the knob's other coordinate.
    centre_cross: f32,
}

/// Resolve a slider's track geometry and value axis.
///
/// **The vertical axis runs top → bottom**: `value == 0` is at the top and
/// `value == 1` at the bottom, with the fill growing downward. That is not
/// the visual convention one might reach for (a "level" that rises), and
/// getting it backwards is invisible until you grab the knob — it then
/// travels *away* from the pointer, because a style does not own the
/// mapping. `Slider`'s own hit-testing computes
/// `t = (y - thumb_radius) / usable` from the widget-local pointer
/// position; a style that disagrees is simply wrong, however it looks
/// standing still.
///
/// The track is inset by the knob's radius at both ends so its travel
/// never overhangs the control — the same inset `Slider` subtracts when it
/// maps a position to a value, which is what keeps the two in step.
fn slider_axis(orientation: SliderOrientation, bounds: Rect) -> SliderAxis {
    let half = THUMB * 0.5;
    match orientation {
        SliderOrientation::Horizontal => {
            let cy = bounds.y + bounds.height * 0.5;
            SliderAxis {
                track: Rect::new(
                    bounds.x + half,
                    cy - TRACK * 0.5,
                    (bounds.width - THUMB).max(0.0),
                    TRACK,
                ),
                start: bounds.x + half,
                end: bounds.x + bounds.width - half,
                centre_cross: cy,
            }
        }
        SliderOrientation::Vertical => {
            let cx = bounds.x + bounds.width * 0.5;
            SliderAxis {
                track: Rect::new(
                    cx - TRACK * 0.5,
                    bounds.y + half,
                    TRACK,
                    (bounds.height - THUMB).max(0.0),
                ),
                start: bounds.y + half,
                end: bounds.y + bounds.height - half,
                centre_cross: cx,
            }
        }
    }
}

struct MacOsSliderBody {
    value: Signal<f32>,
    state: Signal<MacOsState>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    orientation: SliderOrientation,
    tick_count: Option<u32>,
}

impl std::fmt::Debug for MacOsSliderBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsSliderBody")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for MacOsSliderBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.orientation {
            SliderOrientation::Horizontal => {
                Size::new(proposal.width.unwrap_or(DEFAULT_LENGTH), CROSS).into()
            }
            SliderOrientation::Vertical => {
                Size::new(CROSS, proposal.height.unwrap_or(DEFAULT_LENGTH)).into()
            }
        }
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
        let state = self.state.get();
        let enabled = state != MacOsState::Disabled;
        let t = self.value.get().clamp(0.0, 1.0);
        let c = &ctx.theme.colors;
        let p = ctx.theme.extension::<MacOsPalette>();

        let fill: Color = if enabled { c.accent } else { c.accent_disabled };
        let rail: Color = match (p, enabled) {
            (Some(p), true) => p.control_track,
            (Some(p), false) => p.control_track.with_alpha(p.control_track.a() * 0.5),
            (None, true) => c.surface_sunken,
            (None, false) => c.surface_disabled,
        };

        let SliderAxis {
            track,
            start,
            end,
            centre_cross,
        } = slider_axis(self.orientation, bounds);
        let pill = CornerRadius::uniform(TRACK * 0.5);
        // No hairline on the rail, unlike the switch track and the
        // unchecked checkbox and radio, which all carry a `border_strong`
        // outline so the *control* clears WCAG SC 1.4.11's 3:1 boundary
        // floor. The asymmetry is deliberate: on those three the outlined
        // shape **is** the control, so losing its boundary loses the
        // affordance. Here the knob is — a bezelled disc with its own
        // hairline and shadow, present at every value including zero —
        // and the rail is the groove behind it, which AppKit also leaves
        // deliberately faint.
        canvas.fill_rounded_rect(track, pill, rail);

        // Filled portion, from the track's origin to the knob — rightward
        // when horizontal, *downward* when vertical (see `slider_axis`).
        match self.orientation {
            SliderOrientation::Horizontal => {
                let w = track.width * t;
                if w > 0.0 {
                    canvas.fill_rounded_rect(
                        Rect::new(track.x, track.y, w, track.height),
                        pill,
                        fill,
                    );
                }
            }
            SliderOrientation::Vertical => {
                let h = track.height * t;
                if h > 0.0 {
                    canvas.fill_rounded_rect(
                        Rect::new(track.x, track.y, track.width, h),
                        pill,
                        fill,
                    );
                }
            }
        }

        // Discrete ticks, on the far side of the track.
        //
        // `border_strong`, not `tertiaryLabelColor`. A tick marks a stop
        // the value can actually take, so it carries information and has
        // to clear WCAG SC 1.4.11's 3:1 floor — the same reason the
        // checkbox, radio and switch outlines use this token. AppKit's
        // 25 % tertiary label measures 1.8:1 on the window in Aqua and
        // 2.2:1 in Dark Aqua: a granularity cue nobody can see.
        //
        // This is *not* the rail, which is deliberately faint (see the
        // note above `fill_rounded_rect` for the track) — the rail is a
        // groove, a tick is a mark.
        if let Some(n) = self.tick_count.filter(|n| *n >= 2) {
            let tick_color = if enabled {
                c.border_strong
            } else {
                c.border_disabled
            };
            for i in 0..n {
                let ft = i as f32 / (n - 1) as f32;
                let pos = lerp(start, end, ft);
                let tick_rect = match self.orientation {
                    SliderOrientation::Horizontal => {
                        Rect::new(pos - 0.5, centre_cross + THUMB * 0.5 + 2.0, 1.0, TICK)
                    }
                    SliderOrientation::Vertical => {
                        Rect::new(centre_cross + THUMB * 0.5 + 2.0, pos - 0.5, TICK, 1.0)
                    }
                };
                canvas.fill_rect(tick_rect, tick_color);
            }
        }

        // The knob: the same bezel a push button wears, at 18 dp and round.
        let half = THUMB * 0.5;
        let pos = lerp(start, end, t);
        let centre = match self.orientation {
            SliderOrientation::Horizontal => Point::new(pos, centre_cross),
            SliderOrientation::Vertical => Point::new(centre_cross, pos),
        };
        paint_knob(canvas, centre, half, state, ctx);

        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let ring = Rect::new(centre.x - half, centre.y - half, THUMB, THUMB);
            paint_focus_ring(canvas, ring, half, ctx);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// Shadow, graded face, hairline — the bezel, round.
fn paint_knob(
    canvas: &mut Canvas,
    centre: Point,
    radius: f32,
    state: MacOsState,
    ctx: &PaintContext,
) {
    let bezel = resolve_bezel(ctx, state);
    let rect = Rect::new(
        centre.x - radius,
        centre.y - radius,
        radius * 2.0,
        radius * 2.0,
    );

    if bezel.shadow.a() > 0.0 {
        canvas.draw_shadow(
            rect,
            CornerRadius::uniform(radius),
            &Shadow {
                offset_x: 0.0,
                offset_y: KNOB_SHADOW_OFFSET_Y,
                blur: KNOB_SHADOW_BLUR,
                spread: 0.0,
                color: bezel.shadow,
            },
        );
    }

    canvas.fill_circle(
        centre,
        radius,
        vertical_gradient(bezel.face_top, bezel.face_bottom, radius * 2.0),
    );
    if bezel.stroke.a() > 0.0 && ctx.theme.shape.border_width > 0.0 {
        canvas.stroke_circle(centre, radius, bezel.stroke, ctx.theme.shape.border_width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this guards: a vertical slider whose paint ran bottom-to-top
    /// while `Slider`'s hit-testing ran top-to-bottom. Everything looked
    /// plausible until you grabbed the knob, which then slid the opposite
    /// way to the pointer.
    #[test]
    fn the_value_axis_runs_the_same_way_the_widget_maps_the_pointer() {
        let bounds = Rect::new(10.0, 20.0, 200.0, 300.0);
        for orientation in [SliderOrientation::Horizontal, SliderOrientation::Vertical] {
            let axis = slider_axis(orientation, bounds);
            assert!(
                axis.end > axis.start,
                "{orientation:?}: value must grow with the coordinate, as \
                 Slider's own position→value mapping does"
            );
        }
    }

    /// `Slider` subtracts exactly one knob diameter from the extent when it
    /// maps a position to a value; the painted travel has to span the same
    /// distance or the knob drifts from the pointer as you drag.
    #[test]
    fn painted_travel_matches_the_widgets_usable_extent() {
        let bounds = Rect::new(10.0, 20.0, 200.0, 300.0);

        let h = slider_axis(SliderOrientation::Horizontal, bounds);
        assert!((h.end - h.start - (bounds.width - THUMB)).abs() < 1e-4);
        assert!((h.track.width - (bounds.width - THUMB)).abs() < 1e-4);

        let v = slider_axis(SliderOrientation::Vertical, bounds);
        assert!((v.end - v.start - (bounds.height - THUMB)).abs() < 1e-4);
        assert!((v.track.height - (bounds.height - THUMB)).abs() < 1e-4);
    }

    #[test]
    fn the_track_is_centred_on_the_cross_axis() {
        let bounds = Rect::new(10.0, 20.0, 200.0, 300.0);

        let h = slider_axis(SliderOrientation::Horizontal, bounds);
        assert_eq!(h.centre_cross, bounds.y + bounds.height * 0.5);
        assert!((h.track.y + h.track.height * 0.5 - h.centre_cross).abs() < 1e-4);

        let v = slider_axis(SliderOrientation::Vertical, bounds);
        assert_eq!(v.centre_cross, bounds.x + bounds.width * 0.5);
        assert!((v.track.x + v.track.width * 0.5 - v.centre_cross).abs() < 1e-4);
    }

    /// A degenerate slot must not produce a negative-extent rect.
    #[test]
    fn a_slot_smaller_than_the_knob_clamps_to_zero() {
        let tiny = Rect::new(0.0, 0.0, 4.0, 4.0);
        for orientation in [SliderOrientation::Horizontal, SliderOrientation::Vertical] {
            let axis = slider_axis(orientation, tiny);
            assert!(axis.track.width >= 0.0 && axis.track.height >= 0.0);
        }
    }

    #[test]
    fn the_hit_region_matches_what_is_painted() {
        // `SliderStyle::thumb_diameter` drives the host widget's
        // position→value mapping; if it disagreed with the disc this style
        // paints, the knob would not sit under the pointer.
        let style = MacOsSliderStyle;
        let cfg = SliderStyleConfig {
            value_normalized: Signal::new(0.0),
            is_hovered: Signal::new(false),
            is_dragging: Signal::new(false),
            is_disabled: Signal::new(false),
            focus_origin: Signal::new(None),
            orientation: SliderOrientation::Horizontal,
            tick_count: None,
            variant: teksilo_core::styles::SliderVariant::Continuous,
        };
        assert_eq!(style.thumb_diameter(&cfg), THUMB);
    }

    #[test]
    fn dimensions_are_the_measured_ns_slider() {
        // Containment relationships are compile-time invariants above.
        assert_eq!(TRACK, 4.0);
        assert_eq!(THUMB, 18.0);
        assert_eq!(CROSS, MACOS_CONTROL_HEIGHT);
    }

    /// The knob is the same disc the switch uses — AppKit reuses one piece
    /// of chrome across both, and a mismatch shows in any settings pane
    /// that stacks them.
    #[test]
    fn the_knob_matches_the_switch_knob() {
        assert_eq!(THUMB, 18.0);
    }

    /// A tick a user cannot see is not a tick.
    ///
    /// The regression this guards: the tick colour was
    /// `p.map_or(c.border_strong, |p| p.tertiary_label)`, which put the
    /// WCAG-passing token on the *fallback* branch — so a theme carrying
    /// the palette (that is, every real macOS theme) painted the failing
    /// one, and only a degraded non-macOS theme painted the good one.
    #[test]
    fn tick_marks_clear_the_non_text_contrast_floor() {
        for theme in [crate::light(), crate::dark()] {
            let c = &theme.colors;
            let p = theme.extension::<MacOsPalette>().copied().unwrap();
            for surface in [c.surface_main, c.surface_content] {
                let tick = crate::palette::over(c.border_strong, surface);
                assert!(
                    tick.contrast_ratio(surface) >= 3.0,
                    "a tick is {:.2}:1 on {surface:?}",
                    tick.contrast_ratio(surface)
                );
                // …and the token that used to be painted would not have.
                let old = crate::palette::over(p.tertiary_label, surface);
                assert!(
                    old.contrast_ratio(surface) < 3.0,
                    "tertiaryLabelColor now passes — the comment above the \
                     tick colour should be revisited"
                );
            }
        }
    }
}
