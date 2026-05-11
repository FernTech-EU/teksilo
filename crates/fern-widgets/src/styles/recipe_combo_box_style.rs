//! Default `ComboBoxStyle` impl driven by paint-recipe data.
//!
//! `RecipeComboBoxStyle` ships the IntUI trigger look: a panel-bg
//! rectangle with theme-driven border, a label / Spacer / vertical
//! divider / chevron row, and the standard "border thickens on focus"
//! convention. The actual dropdown popup is owned by the widget — the
//! style only paints the trigger.
//!
//! The trigger composes:
//!
//! ```text
//! ZStack {
//!   RectWidget(bg_role, border_role, border_width, corner_radius)
//!   Padding(horizontal/2, horizontal) {
//!     HStack(spacing=8) {
//!       <selected_label>          ← from cfg
//!       Spacer
//!       Divider                   ← FixedSize(border_width × 0.6h, fill)
//!       Chevron (icon, 12 px)
//!     }
//!   }
//! }
//! ```
//!
//! All four interaction signals (`is_open`, `is_hovered`, `is_focused`,
//! `is_disabled`) feed into derived role signals that drive the bg /
//! border / text recolouring. Apps that want a different look write
//! their own `impl ComboBoxStyle` block — the trait surface is a single
//! `WidgetId` return so they can compose anything.

use fern_core::build_context::BuildContext;
use fern_core::styles::{ComboBoxStyle, ComboBoxStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{FixedSize, HStack, IconWidget, Padding, RectWidget, Spacer, ZStack};

/// Default `ComboBoxStyle` shipped with FernUI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeComboBoxStyle;

impl ComboBoxStyle for RecipeComboBoxStyle {
    fn make_body(&self, cfg: &ComboBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let combo_style = theme.components.combo_box;
        let border_width = theme.shape.border_width;
        let focus_ring_width = theme.shape.focus_ring_width;
        let divider_height = combo_style.height * 0.6;
        let padding_h = combo_style.padding_horizontal;
        let corner_radius = combo_style.corner_radius;

        // Derived role signals. Roles encode "what this colour means";
        // the theme maps them to concrete colours at paint time, so the
        // result follows theme switches reactively.
        //
        // bg: Hover whenever the popup is open OR the pointer is over
        // the trigger; AccentDisabled when disabled; Main otherwise.
        let bg_role = cfg
            .is_open
            .zip3(&cfg.is_hovered, &cfg.is_disabled)
            .map(|(open, hovered, disabled)| {
                if *disabled {
                    SurfaceRole::AccentDisabled
                } else if *open || *hovered {
                    SurfaceRole::Hover
                } else {
                    SurfaceRole::Main
                }
            });

        // border: thicker accent ring on focus, dimmed on disabled,
        // default border in any other state. IntUI doesn't paint a
        // separate focus ring around combo boxes — the border itself
        // is the focus indicator.
        let border_role = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| {
                if *disabled {
                    BorderRole::AccentDisabled
                } else if *focused {
                    BorderRole::Focused
                } else {
                    BorderRole::Default
                }
            });

        let border_width_signal = cfg.is_focused.map(move |focused| {
            if *focused {
                focus_ring_width
            } else {
                border_width
            }
        });

        // Divider between the selected-value area and the chevron,
        // mirroring SplitButton's split rule. Label text colour is
        // bound by the widget on `selected_label` itself (it owns the
        // TextWidget) — the style only paints the chrome.
        let divider_fill_id = ctx.add(RectWidget::new().background(BorderRole::Default));
        let divider_id = ctx.add(
            FixedSize::new()
                .bind_width(border_width)
                .bind_height(divider_height)
                .child_id(divider_fill_id),
        );

        // Chevron colour: `text_primary` at 50 % alpha. No role
        // captures this blend, so we derive a `Signal<Color>` off
        // theme_signal directly.
        let chevron_color = ctx
            .theme_signal()
            .map(|t| t.colors.text_primary.with_alpha(0.5));
        let chevron = IconWidget::chevron_down(12.0).bind_color(chevron_color);
        let chevron_id = ctx.add(chevron);

        let row = HStack::new()
            .spacing(8.0)
            .add_child(cfg.selected_label)
            .child(Spacer::new())
            .add_child(divider_id)
            .add_child(chevron_id);
        let row_id = ctx.add(row);

        let padding_id = ctx.add(Padding::symmetric(padding_h * 0.5, padding_h).child_id(row_id));

        let bg = RectWidget::new()
            .bind_background(bg_role)
            .bind_border_color(border_role)
            .bind_border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(corner_radius));
        let bg_id = ctx.add(bg);

        let visual_id = ctx.add(ZStack::new().add_child(bg_id).add_child(padding_id));
        ctx.add(crate::primitives::MinSize::new(0.0, combo_style.height).child_id(visual_id))
    }
}

