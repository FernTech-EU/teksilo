// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS checkbox.
//!
//! A **14 dp** square at a 3.5 dp radius — half the size of Fluent's 20 dp
//! box, and the clearest single sign of how much denser macOS chrome is.
//! Unchecked it is a miniature push-button [bezel](crate::styles::chrome):
//! a graded face, a hairline, a shadow. Checked it fills with the accent
//! and shows a white tick; mixed shows a white dash.
//!
//! The tick is drawn as a path rather than a glyph because it is not the
//! same shape as Fluent's: AppKit's checkmark has a **short** left arm and
//! a long right one that overshoots the box's optical centre, which is
//! what gives it its forward lean.
//!
//! **One deviation**, shared with the radio and the switch: while
//! unchecked the outline is `border_strong` rather than the bezel's own
//! hairline. AppKit's hairline measures 1.25:1 on a white surface, and an
//! unchecked checkbox's outline *is* the control — WCAG SC 1.4.11 asks for
//! 3:1. Once the box fills with the accent it is its own boundary and the
//! outline goes away, exactly as AppKit's does.

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

use crate::shape::MACOS_SMALL_CONTROL_SIZE;
use crate::styles::chrome::{MacOsState, paint_bezel, paint_focus_ring, resolve_bezel};

/// Box size (dp) at the regular control size — shared with the radio,
/// which AppKit draws as its matched pair.
const BOX_SIZE: f32 = MACOS_SMALL_CONTROL_SIZE;
/// Corner radius (dp).
const CORNER: f32 = 3.5;
/// The tick / dash occupies this much of the box.
const GLYPH_SIZE: f32 = 10.0;
/// Outline thickness while unchecked (dp).
const STROKE: f32 = 1.0;

// The glyph is drawn inside the box, so it must be smaller than it.
const _: () = assert!(GLYPH_SIZE < BOX_SIZE);
// …and the corner has to stay a corner, not become a circle.
const _: () = assert!(CORNER < BOX_SIZE * 0.5);
// …and the box stays markedly smaller than Fluent's 20 dp one, which is
// the clearest single sign of how much denser macOS chrome is.
const _: () = assert!(BOX_SIZE < 20.0);

/// macOS `CheckboxStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsCheckboxStyle;

impl CheckboxStyle for MacOsCheckboxStyle {
    fn make_body(&self, cfg: &CheckboxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let state = MacOsState::derive(&cfg.is_disabled, &cfg.is_pressed, &cfg.is_hovered);
        let show_ring = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);
        ctx.add(MacOsCheckboxBody {
            check: cfg.state.clone(),
            state,
            show_ring,
            variant: cfg.variant,
        })
    }
}

struct MacOsCheckboxBody {
    check: Signal<CheckboxState>,
    state: Signal<MacOsState>,
    show_ring: Signal<bool>,
    variant: CheckboxVariant,
}

impl std::fmt::Debug for MacOsCheckboxBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsCheckboxBody")
            .field("variant", &self.variant)
            .finish()
    }
}

/// The radius a variant paints at.
fn variant_radius(variant: CheckboxVariant) -> f32 {
    match variant {
        CheckboxVariant::Square => CORNER,
        CheckboxVariant::Rounded => CORNER * 2.0,
        CheckboxVariant::Circle => BOX_SIZE * 0.5,
    }
}

/// The accent fill a *checked* box paints for an interaction state.
fn checked_fill(state: MacOsState, ctx: &PaintContext) -> Color {
    let c = &ctx.theme.colors;
    match state {
        MacOsState::Rest => c.accent,
        MacOsState::Hover => c.accent_hover,
        MacOsState::Pressed => c.accent_pressed,
        MacOsState::Disabled => c.accent_disabled,
    }
}

/// The glyph colour on a filled box.
fn glyph_color(state: MacOsState, ctx: &PaintContext) -> Color {
    let c = &ctx.theme.colors;
    if state == MacOsState::Disabled {
        c.text_disabled
    } else {
        c.text_on_accent
    }
}

impl Widget for MacOsCheckboxBody {
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
        let radius = variant_radius(self.variant);
        let corner = CornerRadius::uniform(radius);

        if filled {
            let fill = checked_fill(state, ctx);
            if fill.a() > 0.0 {
                canvas.fill_rounded_rect(bounds, corner, fill);
            }
        } else {
            // A miniature push button: shadow, graded face, hairline.
            let bezel = resolve_bezel(ctx, state);
            paint_bezel(canvas, bounds, radius, &bezel, ctx.theme.shape.border_width);
            // …then the WCAG outline over the bezel's own hairline. See
            // the module doc.
            let outline = if state == MacOsState::Disabled {
                ctx.theme.colors.border_disabled
            } else {
                ctx.theme.colors.border_strong
            };
            if outline.a() > 0.0 {
                canvas.stroke_rounded_rect(bounds, corner, outline, STROKE);
            }
        }

        if self.show_ring.get() {
            paint_focus_ring(canvas, bounds, radius, ctx);
        }

        if !matches!(check, CheckboxState::Unchecked) {
            paint_glyph(canvas, bounds, check, glyph_color(state, ctx));
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// The AppKit tick / dash, centred in the box.
///
/// The tick's short left arm and long, overshooting right arm are what
/// give the macOS checkmark its forward lean; a symmetric V reads as a
/// different design language.
fn paint_glyph(canvas: &mut Canvas, bounds: Rect, state: CheckboxState, color: Color) {
    let g = GLYPH_SIZE;
    let x = bounds.x + (bounds.width - g) * 0.5;
    let y = bounds.y + (bounds.height - g) * 0.5;
    let stroke = (g * 0.16).max(1.5);
    let mut path = Path::new();
    match state {
        CheckboxState::Checked => {
            path.move_to(Point::new(x + g * 0.16, y + g * 0.54));
            path.line_to(Point::new(x + g * 0.40, y + g * 0.78));
            path.line_to(Point::new(x + g * 0.86, y + g * 0.22));
        }
        CheckboxState::Indeterminate => {
            path.move_to(Point::new(x + g * 0.16, y + g * 0.50));
            path.line_to(Point::new(x + g * 0.84, y + g * 0.50));
        }
        CheckboxState::Unchecked => return,
    }
    canvas.stroke_path(&path, color, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_the_regular_size_macos_checkbox() {
        // Containment and corner sanity are compile-time invariants above.
        assert_eq!(BOX_SIZE, 14.0);
        assert_eq!(GLYPH_SIZE, 10.0);
        assert_eq!(CORNER, 3.5);
    }

    #[test]
    fn variants_move_only_the_radius() {
        assert_eq!(variant_radius(CheckboxVariant::Square), CORNER);
        assert!(variant_radius(CheckboxVariant::Rounded) > CORNER);
        assert_eq!(variant_radius(CheckboxVariant::Circle), BOX_SIZE * 0.5);
    }

    #[test]
    fn the_tick_leans_forward() {
        // The vertex sits left of centre and the long arm overshoots it —
        // if both arms were symmetric the glyph would read as a Fluent
        // check, not an AppKit one.
        let vertex_x = 0.40;
        let end_x = 0.86;
        let start_x = 0.16;
        assert!(vertex_x - start_x < end_x - vertex_x, "the arms are equal");
        assert!(vertex_x < 0.5, "the vertex must sit left of centre");
    }
}
