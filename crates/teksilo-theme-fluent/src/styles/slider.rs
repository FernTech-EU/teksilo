// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent `Slider`.
//!
//! Fluent's thumb is two circles, not one: a 22 dp
//! `ControlSolidFillColorDefault` disc with a 1 dp hairline, and inside it
//! a **12 dp accent dot** that swells to 14 dp under the pointer and
//! shrinks to 8.5 dp under the press — the same gesture vocabulary as the
//! radio button's glyph. A single-colour thumb (all IntUI's
//! [`SliderRecipe`](teksilo_widgets::styles::SliderRecipe) can express)
//! loses both the ring and the gesture, so this is a full
//! `impl SliderStyle`.
//!
//! The 22 dp is not a resource value: `SliderHorizontalThumbWidth` is 18,
//! but the template wraps the inner ellipse in a `Border Margin="-2"`, and
//! a *negative* margin grows the element — so the disc that reaches the
//! screen is 18 + 2 × 2. The dot's three sizes are likewise a 12 dp base
//! with `ScaleX/Y` animated to 1.167 and 0.71.
//!
//! The track is 4 dp, filled with the accent up to the value and
//! `ControlStrongFillColorDefault` beyond it.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SliderOrientation, SliderStyle, SliderStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, lerp};

use crate::palette::FluentPalette;
use crate::styles::chrome::paint_focus_ring;

/// `SliderTrackThemeHeight` (dp).
const TRACK: f32 = 4.0;
/// The visible outer disc: `SliderHorizontalThumbWidth` (18) grown by the
/// inner `Border`'s `Margin="-2"` on each side.
const THUMB: f32 = 22.0;
/// `SliderInnerThumbWidth` / `Height` (dp).
const INNER: f32 = 12.0;
/// The inner dot under the pointer — the base scaled by 1.167.
const INNER_HOVER: f32 = 14.0;
/// The inner dot while dragging — the base scaled by 0.71.
const INNER_PRESSED: f32 = 8.5;
/// `SliderHorizontalHeight` — the cross-axis extent the control claims.
const CROSS: f32 = 32.0;
/// Tick mark length (dp) for the discrete variant.
const TICK: f32 = 4.0;

// The inner dot lives inside the outer disc at every size, and the disc
// itself has to fit the cross-axis extent the control claims.
const _: () = assert!(INNER < THUMB);
const _: () = assert!(INNER_HOVER < THUMB);
const _: () = assert!(INNER_PRESSED < THUMB);
const _: () = assert!(TRACK < THUMB);
const _: () = assert!(THUMB <= CROSS);

/// Fluent `SliderStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentSliderStyle;

impl SliderStyle for FluentSliderStyle {
    fn thumb_diameter(&self, _cfg: &SliderStyleConfig) -> f32 {
        THUMB
    }

    fn make_body(&self, cfg: &SliderStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let inner =
            ctx.animated_signal(inner_diameter(cfg.is_hovered.get(), cfg.is_dragging.get()));
        // `ControlNormalAnimationDuration` on `ControlFastOutSlowInKeySpline`.
        let grow = ctx.animate().normal().cubic_bezier(0.0, 0.0, 0.0, 1.0);

        // `Signal::observe` only accepts a *mutable* signal, and a style
        // config is free to hand over derived projections (`RadioStyleConfig`
        // does, which is why the radio's dot snaps). `Slider` passes its own
        // hover / drag signals today, so the tween installs — but rather than
        // depend on another crate's internals, a config that ever stops being
        // observable degrades to snapping instead of panicking.
        let animated = observe_pair(
            ctx,
            &cfg.is_hovered,
            &cfg.is_dragging,
            &inner,
            grow,
            inner_diameter,
        );

        ctx.add(FluentSliderBody {
            value: cfg.value_normalized.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_dragging: cfg.is_dragging.clone(),
            inner,
            animated,
            is_disabled: cfg.is_disabled.clone(),
            focus_origin: cfg.focus_origin.clone(),
            orientation: cfg.orientation,
            tick_count: cfg.tick_count,
        })
    }
}

fn inner_diameter(hovered: bool, dragging: bool) -> f32 {
    if dragging {
        INNER_PRESSED
    } else if hovered {
        INNER_HOVER
    } else {
        INNER
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
    /// Cross-axis centre — the thumb's other coordinate.
    centre_cross: f32,
}

/// Resolve a slider's track geometry and value axis.
///
/// **The vertical axis runs top → bottom**: `value == 0` is at the top and
/// `value == 1` at the bottom, with the fill growing downward. That is not
/// the visual convention one might reach for (a "level" that rises), and
/// getting it backwards is invisible until you grab the thumb — it then
/// travels *away* from the pointer, because a style does not own the
/// mapping. `Slider`'s own hit-testing computes
/// `t = (y - thumb_radius) / usable` from the widget-local pointer position,
/// and the shipped `RecipeSliderStyle` paints to match. A style that
/// disagrees is simply wrong, however it looks standing still.
///
/// The track is inset by the thumb's radius at both ends so the thumb's
/// travel never overhangs the control — the same inset `Slider` subtracts
/// (`bounds.height - thumb_radius * 2.0`) when it maps a position to a
/// value, which is what keeps the two in step.
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

/// Tween `target` toward `f(a, b)` whenever either boolean source changes.
///
/// Returns `false` — and installs nothing — if either source is a derived
/// signal, which cannot be observed. Callers paint from the raw sources in
/// that case.
fn observe_pair(
    ctx: &mut BuildContext,
    a: &Signal<bool>,
    b: &Signal<bool>,
    target: &Signal<f32>,
    anim: teksilo_core::animation_builder::AnimationSpec,
    f: fn(bool, bool) -> f32,
) -> bool {
    let on_a = {
        let (target, anim, b) = (target.clone(), anim.clone(), b.clone());
        a.try_observe(move |v| anim.to_or_snap(&target, f(*v, b.get())))
    };
    let on_b = {
        let (target, anim, a) = (target.clone(), anim, a.clone());
        b.try_observe(move |v| anim.to_or_snap(&target, f(a.get(), *v)))
    };
    match (on_a, on_b) {
        (Ok(ha), Ok(hb)) => {
            ctx.own_handle(ha);
            ctx.own_handle(hb);
            true
        }
        _ => false,
    }
}

struct FluentSliderBody {
    value: Signal<f32>,
    is_hovered: Signal<bool>,
    is_dragging: Signal<bool>,
    inner: Signal<f32>,
    /// Whether `inner` is actually being driven — see [`observe_pair`].
    animated: bool,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    orientation: SliderOrientation,
    tick_count: Option<u32>,
}

impl std::fmt::Debug for FluentSliderBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentSliderBody")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for FluentSliderBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.inner.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_hovered
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_dragging
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_disabled
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.orientation {
            SliderOrientation::Horizontal => {
                Size::new(proposal.width.unwrap_or(160.0), CROSS).into()
            }
            SliderOrientation::Vertical => {
                Size::new(CROSS, proposal.height.unwrap_or(160.0)).into()
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
        let enabled = !self.is_disabled.get();
        let t = self.value.get().clamp(0.0, 1.0);
        let c = &ctx.theme.colors;
        let p = ctx.theme.extension::<FluentPalette>();

        let fill: Color = if enabled { c.accent } else { c.accent_disabled };
        let rail: Color = match (p, enabled) {
            (Some(p), true) => p.control_strong_fill_default,
            (Some(p), false) => p.control_strong_fill_disabled,
            (None, true) => c.surface_sunken,
            (None, false) => c.surface_disabled,
        };
        let thumb_face = match (p, enabled) {
            (Some(p), _) => p.control_solid_fill_default,
            (None, _) => c.surface_content,
        };
        let thumb_ring = match (p, enabled) {
            (Some(p), true) => p.control_stroke_default,
            (Some(p), false) => p.control_strong_stroke_disabled,
            (None, true) => c.border,
            (None, false) => c.border_disabled,
        };

        let SliderAxis {
            track,
            start,
            end,
            centre_cross,
        } = slider_axis(self.orientation, bounds);
        let pill = CornerRadius::uniform(TRACK * 0.5);
        canvas.fill_rounded_rect(track, pill, rail);

        // Filled portion, from the track's origin to the thumb — rightward
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

        // Discrete ticks, drawn on the far side of the track.
        if let Some(n) = self.tick_count.filter(|n| *n >= 2) {
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
                canvas.fill_rect(tick_rect, rail);
            }
        }

        // Thumb: outer disc + hairline + accent inner dot.
        let half = THUMB * 0.5;
        let pos = lerp(start, end, t);
        let centre = match self.orientation {
            SliderOrientation::Horizontal => Point::new(pos, centre_cross),
            SliderOrientation::Vertical => Point::new(centre_cross, pos),
        };
        canvas.fill_circle(centre, half, thumb_face);
        if thumb_ring.a() > 0.0 {
            canvas.stroke_circle(centre, half, thumb_ring, ctx.theme.shape.border_width);
        }
        let d = if self.animated {
            self.inner.get().max(0.0)
        } else {
            inner_diameter(self.is_hovered.get(), self.is_dragging.get())
        };
        if d > 0.25 {
            canvas.fill_circle(centre, d * 0.5, fill);
        }

        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let ring = Rect::new(centre.x - half, centre.y - half, THUMB, THUMB);
            paint_focus_ring(canvas, ring, half, ctx);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this guards: a vertical slider whose paint ran bottom-to-top
    /// while `Slider`'s hit-testing ran top-to-bottom. Everything looked
    /// plausible until you grabbed the thumb, which then slid the opposite
    /// way to the pointer.
    ///
    /// `Slider::set_value_from_position` computes
    /// `t = (pos - thumb_radius) / (extent - thumb_radius * 2)` from the
    /// widget-local coordinate on *both* axes — so on both axes `t` must
    /// increase as the coordinate increases.
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

    /// `Slider` subtracts exactly one thumb diameter from the extent when it
    /// maps a position to a value; the painted travel has to span the same
    /// distance or the thumb drifts from the pointer as you drag.
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
    fn a_slot_smaller_than_the_thumb_clamps_to_zero() {
        let tiny = Rect::new(0.0, 0.0, 4.0, 4.0);
        for orientation in [SliderOrientation::Horizontal, SliderOrientation::Vertical] {
            let axis = slider_axis(orientation, tiny);
            assert!(axis.track.width >= 0.0 && axis.track.height >= 0.0);
        }
    }

    #[test]
    fn inner_dot_follows_the_winui_gesture_sizes() {
        assert_eq!(inner_diameter(false, false), INNER);
        assert_eq!(inner_diameter(true, false), INNER_HOVER);
        assert_eq!(inner_diameter(false, true), INNER_PRESSED);
        // Dragging wins over hover — the pointer is over the thumb in both.
        assert_eq!(inner_diameter(true, true), INNER_PRESSED);
    }

    #[test]
    fn dimensions_are_the_winui_slider() {
        // The containment relationships are compile-time invariants above.
        assert_eq!(TRACK, 4.0);
        assert_eq!(THUMB, 22.0);
        assert_eq!(INNER, 12.0);
        assert_eq!(INNER_HOVER, 14.0);
        assert_eq!(INNER_PRESSED, 8.5);
        assert_eq!(CROSS, 32.0);
    }

    #[test]
    fn the_hit_region_matches_what_is_painted() {
        // `SliderStyle::thumb_diameter` drives the host widget's
        // position→value mapping; if it disagreed with the disc this style
        // paints, the thumb would not sit under the pointer.
        let style = FluentSliderStyle;
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
}
