// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `SpinBoxStyle` impl driven by paint-recipe data.
//!
//! `RecipeSpinBoxStyle::make_body` ports the IntUI spin-box chrome:
//! a focus-aware bordered rounded rect that wraps `field | divider |
//! [up / down]`. The field, up button, and down button arrive
//! pre-built from the widget — the recipe owns the row layout,
//! the divider between the field and the buttons, the column
//! arrangement of the two step buttons, and the bordered surface
//! that frames the whole control as one input.
//!
//! Reads the shared text-field dimensions (height, corner radius,
//! padding, border width) from `recipe_text_input_style` so SpinBox
//! and `TextInput` sit on the same baseline.

use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::styles::{ButtonLayout, SharedSpinBoxStyle, SpinBoxStyle, SpinBoxStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, InputTokens, SurfaceRole};

use crate::primitives::{Divider, Expand, HStack, Padding, RectWidget, VStack, ZStack};
use crate::styles::TextInputRecipe;

/// Default `SpinBoxStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSpinBoxStyle;

impl RecipeSpinBoxStyle {
    /// This style resolved against a density's [`InputTokens`], as
    /// `RecipeSpinBoxStyle::for_tokens(&ctx.theme().input)` at the widget's own
    /// build site.
    ///
    /// The style is a unit struct: it borrows the text-field dimensions
    /// wholesale, and resolves them per density inside
    /// [`make_body`](SpinBoxStyle::make_body) from `ctx.theme().input`, so
    /// there is nothing to bake here.
    pub fn for_tokens(_tokens: &InputTokens) -> Self {
        Self
    }
}

impl SpinBoxStyle for RecipeSpinBoxStyle {
    fn make_body(&self, cfg: &SpinBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // The field metrics come from the same recipe `TextInput` uses,
        // resolved against the active density so the two stay on one baseline.
        let field = TextInputRecipe::for_tokens(&ctx.theme().input);

        // ── Step button column (when not Hidden) ─────────────────
        let buttons_id_opt: Option<WidgetId> = match cfg.layout {
            ButtonLayout::Hidden => None,
            ButtonLayout::Stacked => match (cfg.step_up, cfg.step_down) {
                (Some(up), Some(down)) => {
                    Some(ctx.add(VStack::new().spacing(0.0).child(up).child(down)))
                }
                _ => None,
            },
        };

        // ── Row: field | divider | buttons ────────────────────────
        // `Expand::horizontal()` defaults to flex=1 with zero-basis: the
        // wrapped field's natural default does NOT enter the rigid pool,
        // so the field gets exactly the leftover width inside the
        // SpinBox's MaxSize-capped bounds.
        let expanded_field_id = ctx.add(Expand::horizontal().child(cfg.field));
        let row_id = {
            let mut row = HStack::new().spacing(0.0);
            row = row.child(expanded_field_id);
            if let Some(buttons_id) = buttons_id_opt {
                // Thin vertical divider between text and buttons so
                // the click targets read as distinct affordances. `Field`
                // so it dims with the rest of the frame — a live rule
                // inside an inert field reads as a rendering glitch.
                let divider = Divider::vertical().thickness(1.0).color(BorderRole::Field);
                let divider_id = ctx.add(Padding::new(2.0, 0.0, 2.0, 0.0).child(divider));
                row = row.child(divider_id).child(buttons_id);
            }
            ctx.add(row)
        };

        // Symmetric horizontal padding — same TextInput chrome math
        // (`padding_horizontal * 2.0`) so SpinBox and TextInput line
        // up on forms.
        let padded_row_id = ctx.add(
            Padding::new(0.0, field.padding_horizontal, 0.0, field.padding_horizontal)
                .child(row_id),
        );

        // ── Frame: focus-aware border + background ───────────────
        // Int UI convention: the focus indicator IS the border —
        // accent + `focus_ring_width` when focused, default border
        // color + `border_width` otherwise.
        let theme = ctx.theme_signal().get();
        let focus_ring_width = theme.shape.focus_ring_width;
        let field_border_width = field.border_width;
        // `Field` is `Content`'s twin for *interactive* surfaces: the same
        // colour while enabled, dimming to `SurfaceRole::Disabled` inside
        // `ColorProp::resolve` at paint. Resolving there — off the live arena
        // chain — rather than switching roles from `cfg.is_disabled` is what
        // lets a SpinBox dim when an *ancestor* is disabled: `is_disabled`
        // comes from `effective_enabled_signal`, which cannot see ancestors
        // (a widget's parent is not wired during its own `build()`), so it
        // only ever reflects the SpinBox's own `enabled` prop.
        //
        // The border still consults `is_disabled` so that disabled outranks
        // *focus*; `Field` covers the resting case.
        let border_role = cfg.is_focused.zip(&cfg.is_disabled).map(|(f, d)| {
            if *d {
                BorderRole::Disabled
            } else if *f {
                BorderRole::Focused
            } else {
                BorderRole::Field
            }
        });
        let border_width_signal = cfg.is_focused.map(move |f| {
            if *f {
                focus_ring_width
            } else {
                field_border_width
            }
        });
        let bg = RectWidget::new()
            .background(SurfaceRole::Field)
            .border_color(ColorProp::DynamicBorderRole(border_role))
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field.corner_radius));
        let bg_id = ctx.add(bg);

        ctx.add(ZStack::new().child(bg_id).child(padded_row_id))
    }
}

/// Convenience for callers that need to resolve the active style
/// (per-call override → theme slot → default `RecipeSpinBoxStyle`).
pub fn resolve_spin_box_style(
    override_: &Option<SharedSpinBoxStyle>,
    ctx: &BuildContext,
) -> SharedSpinBoxStyle {
    if let Some(s) = override_.clone() {
        return s;
    }
    ctx.theme_signal()
        .get()
        .style_slots
        .spin_box
        .clone()
        .unwrap_or_else(|| std::rc::Rc::new(RecipeSpinBoxStyle) as SharedSpinBoxStyle)
}
