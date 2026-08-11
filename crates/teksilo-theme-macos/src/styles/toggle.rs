// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS `NSSwitch`.
//!
//! A 38 × 22 dp capsule with an **18 dp knob** — a knob that very nearly
//! fills the track, sitting 2 dp in from each edge. That proportion is the
//! whole tell: Fluent's switch is a 40 × 20 track with a 12 dp knob
//! (a small dot travelling down a wide corridor), Material 3's knob grows
//! and shrinks, and macOS's is a physical disc that only just fits.
//!
//! The knob is a [bezel](crate::styles::chrome), not a coloured dot: a
//! graded white face, a hairline, and its own small shadow. It looks the
//! same whether the switch is on or off — only the track changes, from a
//! neutral wash to the accent fill. AppKit does not recolour the knob and
//! does not resize it on hover or press, so neither does this.
//!
//! **One deviation.** AppKit's off track has no visible outline; a
//! `#E4E4E4` capsule on an `#ECECEC` window is perceivable on a real
//! display and not at all in a contrast measurement (1.1:1, against WCAG
//! SC 1.4.11's 3:1 floor for a control boundary). A 1 dp `border_strong`
//! hairline is added while off — the same lift the checkbox and radio
//! outlines already carry, documented at
//! [`ColorTokens::border_strong`](teksilo_tokens::ColorTokens::border_strong)
//! in the crate's colour projection. It disappears when the track fills with the
//! accent, which needs no help being seen.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{ToggleStyle, ToggleStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, Shadow, lerp};

use crate::palette::MacOsPalette;
use crate::shape::MACOS_CONTROL_HEIGHT;
use crate::styles::chrome::{MacOsState, paint_focus_ring, resolve_bezel, vertical_gradient};

/// Track width (dp).
const TRACK_W: f32 = 38.0;
/// Track height (dp) — the same 22 dp every other regular-size control
/// stands at, so a switch lines up with the field beside it.
const TRACK_H: f32 = MACOS_CONTROL_HEIGHT;
/// Knob diameter (dp).
const KNOB: f32 = 18.0;
/// Clearance between the knob and the track edge (dp).
const KNOB_INSET: f32 = 2.0;
/// Track hairline while off (dp).
const TRACK_STROKE: f32 = 1.0;
/// The knob's own drop shadow.
const KNOB_SHADOW_OFFSET_Y: f32 = 0.5;
const KNOB_SHADOW_BLUR: f32 = 1.5;

// The knob has to fill the track's height exactly once its insets are
// counted — that near-fit is what makes the control read as macOS rather
// than Fluent.
const _: () = assert!(KNOB + KNOB_INSET * 2.0 == TRACK_H);
// …and it has to have somewhere to travel.
const _: () = assert!(TRACK_W > TRACK_H);
// The proportion that distinguishes this switch from Fluent's 12-in-20 dot:
// a macOS knob *fills* its track.
const _: () = assert!(KNOB * 20.0 > TRACK_H * 12.0);

/// macOS `ToggleStyle` — the AppKit `NSSwitch`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsToggleStyle;

impl ToggleStyle for MacOsToggleStyle {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let initial = if cfg.is_on.get() { 1.0 } else { 0.0 };
        let knob_position = ctx.animated_signal(initial);
        // Core Animation's default 0.25 s on `EaseInEaseOut` — the curve
        // `crate::motion` installs as the theme's standard easing.
        let slide = ctx.animate().normal().cubic_bezier(0.42, 0.0, 0.58, 1.0);
        {
            let knob_position = knob_position.clone();
            let slide = slide.clone();
            ctx.effect(&cfg.is_on, move |on| {
                slide.to_or_snap(&knob_position, if *on { 1.0 } else { 0.0 });
            });
        }

        let show_ring = cfg
            .is_focused
            .zip3(&cfg.is_focus_visible, &cfg.is_disabled)
            .map(|(focused, visible, disabled)| *focused && *visible && !*disabled);

        let state = MacOsState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);

        ctx.add(MacOsSwitchBody {
            knob_position,
            state,
            show_ring,
        })
    }
}

struct MacOsSwitchBody {
    knob_position: Signal<f32>,
    state: Signal<MacOsState>,
    show_ring: Signal<bool>,
}

impl std::fmt::Debug for MacOsSwitchBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsSwitchBody").finish()
    }
}

/// The track colours for a state, resolved once per paint.
///
/// Accent-derived values come from `theme.colors` (already projected for
/// an inactive window); neutral ones from the [`MacOsPalette`] extension,
/// with theme-role fallbacks so the style degrades rather than panics
/// under a non-macOS theme.
struct TrackColors {
    off: Color,
    on: Color,
    outline: Color,
}

impl TrackColors {
    fn resolve(ctx: &PaintContext, state: MacOsState) -> Self {
        let c = &ctx.theme.colors;
        let enabled = state != MacOsState::Disabled;
        let off = match ctx.theme.extension::<MacOsPalette>() {
            Some(p) if enabled => p.control_track,
            Some(p) => p.control_track.with_alpha(p.control_track.a() * 0.5),
            None if enabled => c.surface_sunken,
            None => c.surface_disabled,
        };
        Self {
            off,
            on: match state {
                MacOsState::Rest => c.accent,
                MacOsState::Hover => c.accent_hover,
                MacOsState::Pressed => c.accent_pressed,
                MacOsState::Disabled => c.accent_disabled,
            },
            outline: if enabled {
                c.border_strong
            } else {
                c.border_disabled
            },
        }
    }
}

impl Widget for MacOsSwitchBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.knob_position
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_ring
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(TRACK_W, TRACK_H).into()
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
        let t = self.knob_position.get().clamp(0.0, 1.0);
        let colors = TrackColors::resolve(ctx, state);

        let track = Rect::new(
            bounds.x,
            bounds.y + (bounds.height - TRACK_H) * 0.5,
            TRACK_W,
            TRACK_H,
        );
        let pill = CornerRadius::uniform(TRACK_H * 0.5);

        // The off track is a translucent wash, so it is painted first and
        // the accent is cross-faded in over it as the knob travels —
        // mixing the two directly would fade the accent's own alpha.
        canvas.fill_rounded_rect(track, pill, colors.off);
        if t > 0.0 {
            canvas.fill_rounded_rect(track, pill, colors.on.with_alpha(colors.on.a() * t));
        }

        // The WCAG hairline, fading out as the accent takes over — see the
        // module doc.
        let outline_alpha = (1.0 - t) * colors.outline.a();
        if outline_alpha > 0.004 {
            canvas.stroke_rounded_rect(
                track,
                pill,
                colors.outline.with_alpha(outline_alpha),
                TRACK_STROKE,
            );
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, track, TRACK_H * 0.5, ctx);
        }

        paint_knob(canvas, track, t, state, ctx);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The owning `Toggle` emits the switch role; the body is chrome.
        builder.set_hidden();
    }
}

/// The bezelled knob: a shadow, a graded face and a hairline — the same
/// three layers a push button wears, at 18 dp and round.
fn paint_knob(canvas: &mut Canvas, track: Rect, t: f32, state: MacOsState, ctx: &PaintContext) {
    let bezel = resolve_bezel(ctx, state);
    let radius = KNOB * 0.5;
    let cx = lerp(
        track.x + KNOB_INSET + radius,
        track.x + TRACK_W - KNOB_INSET - radius,
        t,
    );
    let cy = track.y + TRACK_H * 0.5;
    let knob_rect = Rect::new(cx - radius, cy - radius, KNOB, KNOB);

    if bezel.shadow.a() > 0.0 {
        canvas.draw_shadow(
            knob_rect,
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
        Point::new(cx, cy),
        radius,
        vertical_gradient(bezel.face_top, bezel.face_bottom, KNOB),
    );
    if bezel.stroke.a() > 0.0 && ctx.theme.shape.border_width > 0.0 {
        canvas.stroke_circle(
            Point::new(cx, cy),
            radius,
            bezel.stroke,
            ctx.theme.shape.border_width,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_the_measured_ns_switch() {
        // That the knob fills the track's height and the track is wider
        // than it is tall are compile-time invariants above; this pins the
        // literals.
        assert_eq!(TRACK_W, 38.0);
        assert_eq!(TRACK_H, 22.0);
        assert_eq!(KNOB, 18.0);
        assert_eq!(KNOB_INSET, 2.0);
    }

    /// The proportion that distinguishes this switch from Fluent's.
    ///
    /// That it beats Fluent's 12-in-20 ratio is a compile-time invariant
    /// above; this pins how far past it the knob sits.
    #[test]
    fn the_knob_nearly_fills_the_track() {
        let fill_ratio = KNOB / TRACK_H;
        assert!(
            fill_ratio > 0.75,
            "a macOS knob fills its track; {fill_ratio} reads as Fluent's dot"
        );
    }

    #[test]
    fn the_knob_has_room_to_travel() {
        let travel = TRACK_W - KNOB_INSET * 2.0 - KNOB;
        assert!(travel > 0.0);
        // …and enough of it that the transition is legible.
        assert!(travel >= KNOB * 0.5);
    }

    #[test]
    fn the_switch_matches_the_standard_control_height() {
        assert_eq!(TRACK_H, MACOS_CONTROL_HEIGHT);
    }
}
