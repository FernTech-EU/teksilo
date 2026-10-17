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
//!
//! Right to left, a horizontal slider runs from the right: its minimum is at
//! the leading edge, so the knob, the fill and the ticks are mirrored, as
//! the `Slider` mirrors its pointer and arrow keys. A vertical slider is laid
//! out the same way in either direction, with its minimum at the bottom, as
//! the `Slider` reads its pointer and `ArrowUp` raises the value: the fill
//! rises from the bottom to the knob.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
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
/// **The vertical axis runs bottom to top**: `value == 0` is at the bottom
/// and `value == 1` at the top, with the fill rising from the bottom.
/// Getting it backwards is invisible until you grab the knob: it then sits
/// opposite to where the press put the value, and `ArrowUp` moves it down,
/// because a style does not own the mapping. `Slider`'s own hit-testing
/// computes `t = 1 - (y - thumb_radius) / usable` from the widget-local
/// pointer position, and its `thumb_rect` and the shipped
/// `RecipeSliderStyle` put the thumb there; a style that disagrees is
/// simply wrong, however it looks standing still.
///
/// The track is inset by the knob's radius at both ends so its travel
/// never overhangs the control: the same inset `Slider` subtracts when it
/// maps a position to a value, which is what keeps the two in step.
///
/// **A horizontal axis follows `direction`**: its minimum is at the leading
/// edge, the right one under right to left, so there `start` is the right
/// end and `end` the left one, as `Slider` reads its pointer and arrow keys.
/// The vertical axis does not depend on the direction.
fn slider_axis(
    orientation: SliderOrientation,
    direction: LayoutDirection,
    bounds: Rect,
) -> SliderAxis {
    let half = THUMB * 0.5;
    match orientation {
        SliderOrientation::Horizontal => {
            let cy = bounds.y + bounds.height * 0.5;
            let (left, right) = (bounds.x + half, bounds.x + bounds.width - half);
            let (start, end) = match direction {
                LayoutDirection::LeftToRight => (left, right),
                LayoutDirection::RightToLeft => (right, left),
            };
            SliderAxis {
                track: Rect::new(
                    bounds.x + half,
                    cy - TRACK * 0.5,
                    (bounds.width - THUMB).max(0.0),
                    TRACK,
                ),
                start,
                end,
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
                start: bounds.y + bounds.height - half,
                end: bounds.y + half,
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
        } = slider_axis(self.orientation, ctx.layout_direction, bounds);
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

        // Filled portion, from the minimum's end of the track to the knob:
        // from the leading edge when horizontal, from the bottom when
        // vertical (see `slider_axis`).
        let knob = lerp(start, end, t);
        let extent = (knob - start).abs();
        if extent > 0.0 {
            let filled = match self.orientation {
                SliderOrientation::Horizontal => {
                    Rect::new(start.min(knob), track.y, extent, track.height)
                }
                SliderOrientation::Vertical => {
                    Rect::new(track.x, start.min(knob), track.width, extent)
                }
            };
            canvas.fill_rounded_rect(filled, pill, fill);
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
        let centre = match self.orientation {
            SliderOrientation::Horizontal => Point::new(knob, centre_cross),
            SliderOrientation::Vertical => Point::new(centre_cross, knob),
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

    const LTR: LayoutDirection = LayoutDirection::LeftToRight;
    const RTL: LayoutDirection = LayoutDirection::RightToLeft;

    /// The bug this guards: a vertical slider painted with its minimum at
    /// the top while `Slider` puts it at the bottom, so the knob sat
    /// opposite to where a press put the value and `ArrowUp` moved it down.
    ///
    /// This mounts the real `Slider` under this style at several values, in
    /// both orientations and both directions, and asks it where its thumb is
    /// (`target_regions`, from the same `thumb_rect` its value readout uses
    /// and that agrees with its pointer mapping). The axis has to put the
    /// knob's centre there.
    #[test]
    fn the_value_axis_runs_the_same_way_the_widget_maps_the_pointer() {
        use teksilo_tokens::Orientation;
        use teksilo_widgets::Slider;
        use teksilo_widgets::slider::SLIDER_PART_THUMB;

        for (orientation, widget_orientation) in [
            (SliderOrientation::Horizontal, Orientation::Horizontal),
            (SliderOrientation::Vertical, Orientation::Vertical),
        ] {
            for direction in [LTR, RTL] {
                for value in [0.0_f32, 0.2, 0.5, 0.9, 1.0] {
                    let mut tree = teksilo_core::widget_tree::WidgetTree::new()
                        .with_theme(crate::light())
                        .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                            teksilo_canvas::MockTextBackend::new(),
                        )));
                    tree.set_layout_direction(direction);
                    let slider = tree.add(
                        Slider::new(Signal::new(value), 0.0, 1.0)
                            .orientation(widget_orientation)
                            .style(MacOsSliderStyle),
                    );
                    tree.layout(SizeProposal::exact(200.0, 200.0));

                    let bounds = tree.bounds(slider);
                    let thumb = tree
                        .widget_target_regions(slider)
                        .into_iter()
                        .find(|r| r.part == SLIDER_PART_THUMB)
                        .expect("the slider reports its thumb")
                        .rect;
                    let axis = slider_axis(orientation, direction, bounds);
                    let painted = lerp(axis.start, axis.end, value);
                    let reported = match orientation {
                        SliderOrientation::Horizontal => thumb.x + thumb.width * 0.5,
                        SliderOrientation::Vertical => thumb.y + thumb.height * 0.5,
                    };
                    assert!(
                        (painted - reported).abs() < 1e-3,
                        "{orientation:?} {direction:?} at {value}: the knob is \
                         painted at {painted}, the widget's thumb is at {reported}"
                    );
                }
            }
        }
    }

    /// Right to left, a horizontal axis runs from the right end to the left
    /// one over the same track, as `Slider` reads its pointer from the other
    /// end; a vertical axis is the same in both directions.
    #[test]
    fn right_to_left_mirrors_the_horizontal_axis_only() {
        let bounds = Rect::new(10.0, 20.0, 200.0, 300.0);
        let l = slider_axis(SliderOrientation::Horizontal, LTR, bounds);
        let r = slider_axis(SliderOrientation::Horizontal, RTL, bounds);
        assert_eq!((r.start, r.end), (l.end, l.start));
        assert_eq!(r.track, l.track);
        assert_eq!(r.centre_cross, l.centre_cross);
        let mid = bounds.x + bounds.width * 0.5;
        for t in [0.0, 0.25, 0.5, 1.0] {
            let (a, b) = (lerp(l.start, l.end, t), lerp(r.start, r.end, t));
            assert!(
                (a - mid + (b - mid)).abs() < 1e-4,
                "the knob at {t} is mirrored about the centre: {a} and {b}"
            );
        }

        let lv = slider_axis(SliderOrientation::Vertical, LTR, bounds);
        let rv = slider_axis(SliderOrientation::Vertical, RTL, bounds);
        assert_eq!((rv.start, rv.end, rv.track), (lv.start, lv.end, lv.track));
    }

    /// `Slider` subtracts exactly one knob diameter from the extent when it
    /// maps a position to a value; the painted travel has to span the same
    /// distance or the knob drifts from the pointer as you drag.
    #[test]
    fn painted_travel_matches_the_widgets_usable_extent() {
        let bounds = Rect::new(10.0, 20.0, 200.0, 300.0);

        let h = slider_axis(SliderOrientation::Horizontal, LTR, bounds);
        assert!((h.end - h.start - (bounds.width - THUMB)).abs() < 1e-4);
        assert!((h.track.width - (bounds.width - THUMB)).abs() < 1e-4);

        let v = slider_axis(SliderOrientation::Vertical, LTR, bounds);
        assert!((v.start - v.end - (bounds.height - THUMB)).abs() < 1e-4);
        assert!((v.track.height - (bounds.height - THUMB)).abs() < 1e-4);
    }

    #[test]
    fn the_track_is_centred_on_the_cross_axis() {
        let bounds = Rect::new(10.0, 20.0, 200.0, 300.0);

        let h = slider_axis(SliderOrientation::Horizontal, LTR, bounds);
        assert_eq!(h.centre_cross, bounds.y + bounds.height * 0.5);
        assert!((h.track.y + h.track.height * 0.5 - h.centre_cross).abs() < 1e-4);

        let v = slider_axis(SliderOrientation::Vertical, LTR, bounds);
        assert_eq!(v.centre_cross, bounds.x + bounds.width * 0.5);
        assert!((v.track.x + v.track.width * 0.5 - v.centre_cross).abs() < 1e-4);
    }

    /// A degenerate slot must not produce a negative-extent rect.
    #[test]
    fn a_slot_smaller_than_the_knob_clamps_to_zero() {
        let tiny = Rect::new(0.0, 0.0, 4.0, 4.0);
        for orientation in [SliderOrientation::Horizontal, SliderOrientation::Vertical] {
            let axis = slider_axis(orientation, LTR, tiny);
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
