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

use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::styles::{
    ButtonLayout, SharedSpinBoxStyle, SpinBoxStyle, SpinBoxStyleConfig,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{Divider, Expand, HStack, Padding, RectWidget, VStack, ZStack};
use crate::styles::recipe_text_input_style as field_dims;

/// Default `SpinBoxStyle` shipped with FernUI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeSpinBoxStyle;

impl SpinBoxStyle for RecipeSpinBoxStyle {
    fn make_body(&self, cfg: &SpinBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // ── Step button column (when not Hidden) ─────────────────
        let buttons_id_opt: Option<WidgetId> = match cfg.layout {
            ButtonLayout::Hidden => None,
            ButtonLayout::Stacked => match (cfg.step_up, cfg.step_down) {
                (Some(up), Some(down)) => Some(
                    ctx.add(
                        VStack::new()
                            .spacing(0.0)
                            .add_child(up)
                            .add_child(down),
                    ),
                ),
                _ => None,
            },
        };

        // ── Row: field | divider | buttons ────────────────────────
        // `Expand::horizontal()` defaults to flex=1 with zero-basis: the
        // wrapped field's natural default does NOT enter the rigid pool,
        // so the field gets exactly the leftover width inside the
        // SpinBox's MaxSize-capped bounds.
        let expanded_field_id = ctx.add(Expand::horizontal().child_id(cfg.field));
        let row_id = {
            let mut row = HStack::new().spacing(0.0);
            row = row.add_child(expanded_field_id);
            if let Some(buttons_id) = buttons_id_opt {
                // Thin vertical divider between text and buttons so
                // the click targets read as distinct affordances.
                let divider = Divider::vertical()
                    .thickness(1.0)
                    .color(BorderRole::Default);
                let divider_id = ctx.add(Padding::new(2.0, 0.0, 2.0, 0.0).child(divider));
                row = row.add_child(divider_id).add_child(buttons_id);
            }
            ctx.add(row)
        };

        // Symmetric horizontal padding — same TextInput chrome math
        // (`padding_horizontal * 2.0`) so SpinBox and TextInput line
        // up on forms.
        let padded_row_id = ctx.add(
            Padding::new(
                0.0,
                field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
                0.0,
                field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
            )
            .child_id(row_id),
        );

        // ── Frame: focus-aware border + background ───────────────
        // Int UI convention: the focus indicator IS the border —
        // accent + `focus_ring_width` when focused, default border
        // color + `border_width` otherwise.
        let theme = ctx.theme_signal().get();
        let focus_ring_width = theme.shape.focus_ring_width;
        let field_border_width = field_dims::TEXT_FIELD_BORDER_WIDTH;
        let border_role = cfg.is_focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Default
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
            .background(SurfaceRole::Content)
            .border_color(ColorProp::DynamicBorderRole(border_role))
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field_dims::TEXT_FIELD_CORNER_RADIUS));
        let bg_id = ctx.add(bg);

        ctx.add(ZStack::new().add_child(bg_id).add_child(padded_row_id))
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
