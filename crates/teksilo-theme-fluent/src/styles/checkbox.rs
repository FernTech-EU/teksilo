// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent `CheckBox`.
//!
//! A WinUI checkbox is 20 × 20 dp at `ControlCornerRadius` (4 dp) with a
//! 1 dp stroke and a 12 dp glyph. The part IntUI's recipe cannot express is
//! the **unchecked** state: IntUI leaves the box transparent behind a
//! `border` hairline, while Fluent gives it a real
//! `ControlAltFillColorSecondary` fill inside a
//! `ControlStrongStrokeColorDefault` outline. That outline is the whole
//! affordance — at 5.9 % black, IntUI's `border` token would be effectively
//! invisible on a light Fluent surface and would fail WCAG 1.4.11's 3:1
//! floor for a control boundary.
//!
//! The alt-fill ramp deepens with interaction (`Secondary` → `Tertiary` on
//! hover → `Quarternary` on press) while unchecked; once checked the box
//! fills with the accent ramp instead and the outline disappears, exactly
//! as `CheckBoxCheckBackgroundStroke*` does.

use teksilo_canvas::Path;
use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{CheckboxState, CheckboxStyle, CheckboxStyleConfig, CheckboxVariant};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};

use crate::palette::FluentPalette;
use crate::shape::FLUENT_CONTROL_CORNER_RADIUS;
use crate::styles::chrome::{FluentState, paint_focus_ring};

/// `CheckBoxSize` (dp).
const BOX_SIZE: f32 = 20.0;
/// `CheckBoxGlyphSize` (dp).
const GLYPH_SIZE: f32 = 12.0;
/// `CheckBoxBorderThickness` (dp).
const STROKE: f32 = 1.0;

// The glyph is drawn inside the box, so it must be smaller than it.
const _: () = assert!(GLYPH_SIZE < BOX_SIZE);

/// Fluent `CheckboxStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentCheckboxStyle;

impl CheckboxStyle for FluentCheckboxStyle {
    fn make_body(&self, cfg: &CheckboxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let state = FluentState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);
        let show_ring = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);
        ctx.add(FluentCheckboxBody {
            check: cfg.state.clone(),
            state,
            show_ring,
            variant: cfg.variant,
        })
    }
}

struct FluentCheckboxBody {
    check: Signal<CheckboxState>,
    state: Signal<FluentState>,
    show_ring: Signal<bool>,
    variant: CheckboxVariant,
}

impl std::fmt::Debug for FluentCheckboxBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentCheckboxBody")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for FluentCheckboxBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.check.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_ring
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(BOX_SIZE, BOX_SIZE).into()
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
        let check = self.check.get();
        let state = self.state.get();
        let filled = matches!(check, CheckboxState::Checked | CheckboxState::Indeterminate);
        let c = &ctx.theme.colors;
        let p = ctx.theme.extension::<FluentPalette>();

        let radius = match self.variant {
            CheckboxVariant::Square => FLUENT_CONTROL_CORNER_RADIUS,
            CheckboxVariant::Rounded => FLUENT_CONTROL_CORNER_RADIUS * 2.0,
            CheckboxVariant::Circle => BOX_SIZE * 0.5,
        };
        let corner = CornerRadius::uniform(radius);

        // Fill: the accent ramp once checked, the neutral alt-fill ramp
        // while unchecked. Both already carry Fluent's interaction grading.
        let fill = if filled {
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

        // Outline: only while unchecked — a filled box is its own boundary.
        if !filled {
            let outline = match (p, state) {
                (Some(p), FluentState::Disabled) => p.control_strong_stroke_disabled,
                (Some(p), _) => p.control_strong_stroke_default,
                (None, FluentState::Disabled) => c.border_disabled,
                (None, _) => c.border_strong,
            };
            if outline.a() > 0.0 {
                canvas.stroke_rounded_rect(bounds, corner, outline, STROKE);
            }
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, bounds, radius, ctx);
        }

        if matches!(check, CheckboxState::Unchecked) {
            return;
        }

        // Glyph: `TextOnAccentFillColorPrimary` on the accent fill, which
        // flips from white to black between the two appearances.
        let glyph = match (p, state) {
            (Some(p), FluentState::Disabled) => p.text_on_accent_disabled,
            (Some(p), _) => p.text_on_accent_primary,
            (None, FluentState::Disabled) => c.text_disabled,
            (None, _) => c.text_on_accent,
        };
        paint_glyph(canvas, bounds, check, glyph);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// The 12 dp check / dash, centred in the box.
fn paint_glyph(canvas: &mut Canvas, bounds: Rect, state: CheckboxState, color: Color) {
    let g = GLYPH_SIZE;
    let x = bounds.x + (bounds.width - g) * 0.5;
    let y = bounds.y + (bounds.height - g) * 0.5;
    let stroke = (g * 0.13).max(1.25);
    let mut path = Path::new();
    match state {
        CheckboxState::Checked => {
            path.move_to(Point::new(x + g * 0.17, y + g * 0.52));
            path.line_to(Point::new(x + g * 0.40, y + g * 0.76));
            path.line_to(Point::new(x + g * 0.83, y + g * 0.26));
        }
        CheckboxState::Indeterminate => {
            path.move_to(Point::new(x + g * 0.17, y + g * 0.50));
            path.line_to(Point::new(x + g * 0.83, y + g * 0.50));
        }
        CheckboxState::Unchecked => return,
    }
    canvas.stroke_path(&path, color, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_is_the_winui_twenty_dp_square() {
        // `CheckBoxSize` / `CheckBoxGlyphSize` / `CheckBoxBorderThickness`.
        // That the glyph fits inside the box is a compile-time invariant
        // above.
        assert_eq!(BOX_SIZE, 20.0);
        assert_eq!(GLYPH_SIZE, 12.0);
        assert_eq!(STROKE, 1.0);
    }
}
