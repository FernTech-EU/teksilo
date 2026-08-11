// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent `ToggleSwitch`.
//!
//! WinUI's switch is a 40 × 20 dp pill with a 12 dp knob and, unlike the
//! IntUI toggle, a **visible outline while off** — an off switch is an
//! empty `ControlAltFillColorSecondary` track ringed in
//! `ControlStrongStrokeColorDefault`, with a *grey*
//! (`TextFillColorSecondary`) knob. Only when it turns on does the track
//! fill with the accent, drop its outline, and the knob flip to
//! `TextOnAccentFillColorPrimary`. IntUI's toggle has no outline and a
//! permanently white knob, so this needs a real `impl ToggleStyle` rather
//! than a resized [`ToggleRecipe`](teksilo_widgets::styles::ToggleRecipe).
//!
//! The knob also *morphs*: 12 dp at rest, 14 dp on hover, and a
//! non-uniform 17 × 14 dp while pressed — it squashes toward the direction
//! of travel. WinUI drives that from
//! `ControlFasterAnimationDuration` (83 ms) on the
//! `ControlFastOutSlowInKeySpline` curve, `cubic-bezier(0, 0, 0, 1)`,
//! which is what [`crate::motion`] installs as the theme's standard easing.
//!
//! **A note on the 12 dp figure.** WinUI declares the knob rectangle as
//! `Width="12" Height="12"` and then animates `ScaleX/Y` to `0.86` in the
//! Normal state, whose source comment says "relative scale from 14px to
//! 12px". The comment and the literal disagree — the scale would render
//! 10.3 dp, not 12. The shipped switch reads as a 12 dp knob, so 12 is
//! what is used here and the discrepancy is Microsoft's, not a
//! simplification.

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{ToggleStyle, ToggleStyleConfig};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius, lerp};

use crate::palette::FluentPalette;
use crate::styles::chrome::paint_focus_ring;

/// `OuterBorder` width (dp).
const TRACK_W: f32 = 40.0;
/// `OuterBorder` height (dp).
const TRACK_H: f32 = 20.0;
/// `SwitchKnobOn` / `SwitchKnobOff` declared size (dp).
const KNOB: f32 = 12.0;
/// `RadioButtonCheckGlyphPointerOverSize`-equivalent for the switch knob.
const KNOB_HOVER: f32 = 14.0;
/// Pressed knob — non-uniform, squashed along the travel axis.
const KNOB_PRESSED_W: f32 = 17.0;
const KNOB_PRESSED_H: f32 = 14.0;
/// `SwitchKnob` bounds are 20 × 20 inside a 20 dp track, so a 12 dp knob
/// sits 4 dp in from each edge.
const KNOB_INSET: f32 = 4.0;
/// Track left beyond a grown / squashed knob, matching the pressed state's
/// `Margin="0,0,3,0"` in the WinUI template.
const KNOB_MARGIN: f32 = 3.0;
/// `ToggleSwitchOuterBorderStrokeThickness` — 1 while off, 0 while on.
const TRACK_STROKE: f32 = 1.0;
/// The row a 20 dp track is centred in, so the switch lines up with a
/// 32 dp control beside it.
const ROW_H: f32 = 32.0;

// Geometry invariants, checked at compile time rather than in a test: a
// resting knob has to fill the track's height exactly once its insets are
// counted, the hover knob has to be the larger one, the pressed knob has to
// be wider than it is tall (that is what "squash" means), and the row has
// to be taller than the track it centres.
const _: () = assert!(KNOB + KNOB_INSET * 2.0 == TRACK_H);
const _: () = assert!(KNOB_HOVER > KNOB);
const _: () = assert!(KNOB_PRESSED_W > KNOB_PRESSED_H);
const _: () = assert!(KNOB_PRESSED_H == KNOB_HOVER);
const _: () = assert!(ROW_H > TRACK_H);

/// Fluent `ToggleStyle` — the WinUI `ToggleSwitch`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentToggleStyle;

impl ToggleStyle for FluentToggleStyle {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let initial = if cfg.is_on.get() { 1.0 } else { 0.0 };
        let knob_position = ctx.animated_signal(initial);
        // `ControlFastAnimationDuration` on `ControlFastOutSlowInKeySpline`.
        let slide = ctx.animate().fast().cubic_bezier(0.0, 0.0, 0.0, 1.0);
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

        ctx.add(FluentSwitchBody {
            knob_position,
            is_disabled: cfg.is_disabled.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_pressed: cfg.is_pressed.clone(),
            show_ring,
        })
    }
}

struct FluentSwitchBody {
    knob_position: Signal<f32>,
    is_disabled: Signal<bool>,
    is_hovered: Signal<bool>,
    is_pressed: Signal<bool>,
    show_ring: Signal<bool>,
}

impl std::fmt::Debug for FluentSwitchBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentSwitchBody").finish()
    }
}

/// The five colours the switch needs, resolved once per paint.
///
/// Accent-derived values come from `theme.colors` (already projected for an
/// inactive window); neutral ones from the [`FluentPalette`] extension,
/// with theme-role fallbacks so the style degrades rather than panics under
/// a non-Fluent theme.
struct SwitchColors {
    track_off: Color,
    track_on: Color,
    track_outline: Color,
    knob_off: Color,
    knob_on: Color,
}

impl SwitchColors {
    fn resolve(ctx: &PaintContext, enabled: bool) -> Self {
        let c = &ctx.theme.colors;
        match ctx.theme.extension::<FluentPalette>() {
            Some(p) if enabled => Self {
                track_off: p.control_alt_fill_secondary,
                track_on: c.accent,
                track_outline: p.control_strong_stroke_default,
                knob_off: p.text_secondary,
                knob_on: p.text_on_accent_primary,
            },
            Some(p) => Self {
                track_off: p.control_alt_fill_disabled,
                track_on: c.accent_disabled,
                track_outline: p.control_strong_stroke_disabled,
                knob_off: p.text_disabled,
                knob_on: p.text_on_accent_disabled,
            },
            None if enabled => Self {
                track_off: c.surface_sunken,
                track_on: c.accent,
                track_outline: c.border_strong,
                knob_off: c.text_secondary,
                knob_on: c.text_on_accent,
            },
            None => Self {
                track_off: c.surface_disabled,
                track_on: c.accent_disabled,
                track_outline: c.border_disabled,
                knob_off: c.text_disabled,
                knob_on: c.text_disabled,
            },
        }
    }
}

impl Widget for FluentSwitchBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.is_disabled, &self.is_hovered, &self.is_pressed] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        self.knob_position
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_ring
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(TRACK_W, ROW_H).into()
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
        let t = self.knob_position.get().clamp(0.0, 1.0);
        let colors = SwitchColors::resolve(ctx, enabled);

        let track = Rect::new(
            bounds.x,
            bounds.y + (bounds.height - TRACK_H) * 0.5,
            TRACK_W,
            TRACK_H,
        );
        let pill = CornerRadius::uniform(TRACK_H * 0.5);

        // Track: the off fill is a translucent wash, so it is painted first
        // and the accent is cross-faded in over it as the knob travels —
        // mixing the two directly would fade the accent's own alpha.
        canvas.fill_rounded_rect(track, pill, colors.track_off);
        if t > 0.0 {
            canvas.fill_rounded_rect(track, pill, colors.track_on.with_alpha(t));
        }

        // Off-state outline, fading out as the switch fills
        // (`ToggleSwitchOnStrokeThickness` is 0).
        let outline_alpha = (1.0 - t) * colors.track_outline.a();
        if outline_alpha > 0.004 {
            canvas.stroke_rounded_rect(
                track,
                pill,
                colors.track_outline.with_alpha(outline_alpha),
                TRACK_STROKE,
            );
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, track, TRACK_H * 0.5, ctx);
        }

        // Knob: 12 at rest, 14 on hover, 17 × 14 squashed while pressed.
        let (kw, kh) = if !enabled {
            (KNOB, KNOB)
        } else if self.is_pressed.get() {
            (KNOB_PRESSED_W, KNOB_PRESSED_H)
        } else if self.is_hovered.get() {
            (KNOB_HOVER, KNOB_HOVER)
        } else {
            (KNOB, KNOB)
        };

        // Travel is expressed through the knob's *centre*, then clamped so a
        // knob that has grown or squashed past its resting width keeps
        // `KNOB_MARGIN` of track beyond it instead of overhanging the pill —
        // the same effect WinUI gets from the pressed state's 3 dp margin.
        let centre_off = track.x + KNOB_INSET + KNOB * 0.5;
        let centre_on = track.x + TRACK_W - KNOB_INSET - KNOB * 0.5;
        let min_cx = track.x + KNOB_MARGIN + kw * 0.5;
        let max_cx = track.x + TRACK_W - KNOB_MARGIN - kw * 0.5;
        let cx = lerp(centre_off, centre_on, t).clamp(min_cx.min(max_cx), max_cx.max(min_cx));
        let knob = Rect::new(cx - kw * 0.5, track.y + (TRACK_H - kh) * 0.5, kw, kh);
        let knob_color = colors.knob_off.mix(colors.knob_on, t);
        canvas.fill_rounded_rect(knob, CornerRadius::uniform(kh * 0.5), knob_color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The owning `Toggle` emits the switch role; the body is chrome.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_the_winui_toggle_switch() {
        // The relationships between these (knob fills the track, hover
        // grows, press squashes, row clears the track) are compile-time
        // `const _: () = assert!(…)` invariants above; this pins the
        // literals themselves against the theme resources.
        assert_eq!(TRACK_W, 40.0);
        assert_eq!(TRACK_H, 20.0);
        assert_eq!(KNOB, 12.0);
        assert_eq!(KNOB_HOVER, 14.0);
        assert_eq!(KNOB_PRESSED_W, 17.0);
        assert_eq!(KNOB_PRESSED_H, 14.0);
        assert_eq!(ROW_H, 32.0);
    }
}
