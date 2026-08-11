// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent `RadioButton`.
//!
//! A 20 dp ring with a 1 dp stroke. Unselected it is a
//! `ControlAltFillColorSecondary` disc inside a
//! `ControlStrongStrokeColorDefault` outline — the same "the outline *is*
//! the affordance" model as the checkbox, and the same reason IntUI's
//! `border`-hairline recipe cannot stand in for it.
//!
//! Selected, the ring fills with the accent and a
//! `TextOnAccentFillColorPrimary` dot appears inside it, sized **12 dp at
//! rest, 14 dp on hover and 10 dp while pressed**
//! (`RadioButtonCheckGlyphSize` and its two siblings) — the dot swelling
//! under the pointer and shrinking under the press is the control's entire
//! interaction language.
//!
//! WinUI tweens between those three sizes over
//! `ControlNormalAnimationDuration`; here the dot **snaps**. Every signal in
//! `RadioStyleConfig` is a derived (`ReadOnly`) projection of the widget's
//! own state — `is_selected` is `selected.map(|s| *s == value)`, the rest
//! come off a mapped `InteractionState` — and `Signal::observe`, which
//! `BuildContext::effect` is built on, only accepts a mutable signal. There
//! is no source here to drive a tween from without either a per-radio
//! frame-tick subscription (heavy for a control this small) or widening the
//! style-config surface. The shipped `RecipeRadioStyle` does not animate its
//! glyph either, so this matches the framework's own baseline.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{RadioStyle, RadioStyleConfig, RadioVariant};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};

use crate::palette::FluentPalette;
use crate::shape::FLUENT_CONTROL_CORNER_RADIUS;
use crate::styles::chrome::{FluentState, paint_focus_ring};

/// `OuterEllipse` diameter (dp).
const OUTER: f32 = 20.0;
/// `RadioButtonBorderThemeThickness` (dp).
const STROKE: f32 = 1.0;
/// `RadioButtonCheckGlyphSize` (dp).
const DOT: f32 = 12.0;
/// `RadioButtonCheckGlyphPointerOverSize` (dp).
const DOT_HOVER: f32 = 14.0;
/// `RadioButtonCheckGlyphPressedOverSize` (dp).
const DOT_PRESSED: f32 = 10.0;

// Every dot size has to clear the ring's stroke on both sides, and the
// three have to stay ordered — pressed shrinks, hover grows.
const _: () = assert!(DOT + STROKE * 2.0 <= OUTER);
const _: () = assert!(DOT_HOVER + STROKE * 2.0 <= OUTER);
const _: () = assert!(DOT_PRESSED < DOT);
const _: () = assert!(DOT < DOT_HOVER);

/// Fluent `RadioStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentRadioStyle;

impl RadioStyle for FluentRadioStyle {
    fn make_body(&self, cfg: &RadioStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let state = FluentState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);
        let show_ring = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);

        ctx.add(FluentRadioBody {
            is_selected: cfg.is_selected.clone(),
            state,
            show_ring,
            variant: cfg.variant,
        })
    }
}

/// The dot diameter for an interaction state. Zero while unselected, so the
/// same animated signal drives the select-in growth.
fn dot_diameter(state: FluentState, selected: bool) -> f32 {
    if !selected {
        return 0.0;
    }
    match state {
        FluentState::Hover => DOT_HOVER,
        FluentState::Pressed => DOT_PRESSED,
        FluentState::Rest | FluentState::Disabled => DOT,
    }
}

struct FluentRadioBody {
    is_selected: Signal<bool>,
    state: Signal<FluentState>,
    show_ring: Signal<bool>,
    variant: RadioVariant,
}

impl std::fmt::Debug for FluentRadioBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentRadioBody")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for FluentRadioBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_ring
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(OUTER, OUTER).into()
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
        let selected = self.is_selected.get();
        let state = self.state.get();
        let c = &ctx.theme.colors;
        let p = ctx.theme.extension::<FluentPalette>();

        let radius = match self.variant {
            RadioVariant::Circle => OUTER * 0.5,
            RadioVariant::Rounded => FLUENT_CONTROL_CORNER_RADIUS * 2.0,
            RadioVariant::Square => FLUENT_CONTROL_CORNER_RADIUS,
        };
        let corner = CornerRadius::uniform(radius);

        let fill = if selected {
            match state {
                FluentState::Rest => c.accent,
                FluentState::Hover => c.accent_hover,
                FluentState::Pressed => c.accent_pressed,
                FluentState::Disabled => c.accent_disabled,
            }
        } else {
            match (p, state) {
                (Some(p), FluentState::Rest) => p.control_alt_fill_secondary,
                (Some(p), FluentState::Hover) => p.control_alt_fill_tertiary,
                (Some(p), FluentState::Pressed) => p.control_alt_fill_quarternary,
                (Some(p), FluentState::Disabled) => p.control_alt_fill_disabled,
                (None, FluentState::Disabled) => c.surface_disabled,
                (None, _) => c.surface_content,
            }
        };
        if fill.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, corner, fill);
        }

        // Outline: the strong stroke while unselected, the accent itself
        // once selected (`RadioButtonOuterEllipseCheckedStroke`).
        let outline = if selected {
            fill
        } else {
            match (p, state) {
                (Some(p), FluentState::Disabled) => p.control_strong_stroke_disabled,
                (Some(p), _) => p.control_strong_stroke_default,
                (None, FluentState::Disabled) => c.border_disabled,
                (None, _) => c.border_strong,
            }
        };
        if outline.a() > 0.0 {
            canvas.stroke_rounded_rect(bounds, corner, outline, STROKE);
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, bounds, radius, ctx);
        }

        let d = dot_diameter(state, selected);
        if d > 0.25 {
            let dot_color: Color = match (p, state) {
                (Some(p), FluentState::Disabled) => p.text_on_accent_disabled,
                (Some(p), _) => p.text_on_accent_primary,
                (None, FluentState::Disabled) => c.text_disabled,
                (None, _) => c.text_on_accent,
            };
            canvas.fill_circle(
                Point::new(
                    bounds.x + bounds.width * 0.5,
                    bounds.y + bounds.height * 0.5,
                ),
                d * 0.5,
                dot_color,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_grows_on_hover_and_shrinks_on_press() {
        // The size *ordering* is a compile-time invariant above; this pins
        // which state selects which size.
        assert_eq!(dot_diameter(FluentState::Rest, true), DOT);
        assert_eq!(dot_diameter(FluentState::Hover, true), DOT_HOVER);
        assert_eq!(dot_diameter(FluentState::Pressed, true), DOT_PRESSED);
        assert_eq!(dot_diameter(FluentState::Disabled, true), DOT);
    }

    #[test]
    fn unselected_has_no_dot_in_any_state() {
        for s in [
            FluentState::Rest,
            FluentState::Hover,
            FluentState::Pressed,
            FluentState::Disabled,
        ] {
            assert_eq!(dot_diameter(s, false), 0.0);
        }
    }

    #[test]
    fn ring_is_the_winui_twenty_dp_circle() {
        assert_eq!(OUTER, 20.0);
        assert_eq!(STROKE, 1.0);
    }
}
