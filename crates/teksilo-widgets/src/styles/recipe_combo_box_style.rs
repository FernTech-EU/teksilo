// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{ComboBoxStyle, ComboBoxStyleConfig, ComboBoxVariant};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, InputTokens, SurfaceRole, TargetRole};

use crate::primitives::{FixedSize, HStack, IconWidget, Padding, RectWidget, Spacer, ZStack};
use teksilo_core::styles::density::{dp, spacing};

// IntUI design tokens for ComboBox. The recipe owns its own dimensions.
pub const COMBO_BOX_HEIGHT: f32 = 28.0;
pub const COMBO_BOX_PADDING_HORIZONTAL: f32 = 9.0;
pub const COMBO_BOX_ARROW_COLUMN_WIDTH: f32 = 23.0;
pub const COMBO_BOX_CORNER_RADIUS: f32 = 4.0;

/// Configurable dimensions for [`RecipeComboBoxStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComboBoxRecipe {
    pub height: f32,
    pub padding_horizontal: f32,
    pub arrow_column_width: f32,
    pub corner_radius: f32,
}

impl ComboBoxRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            height: dp(COMBO_BOX_HEIGHT, TargetRole::Target, tokens),
            padding_horizontal: spacing(COMBO_BOX_PADDING_HORIZONTAL, tokens),
            arrow_column_width: COMBO_BOX_ARROW_COLUMN_WIDTH,
            corner_radius: COMBO_BOX_CORNER_RADIUS,
        }
    }
}

impl Default for ComboBoxRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `ComboBoxStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeComboBoxStyle {
    pub recipe: ComboBoxRecipe,
}

impl RecipeComboBoxStyle {
    pub fn new(recipe: ComboBoxRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeComboBoxStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: ComboBoxRecipe::for_tokens(tokens),
        }
    }
}

impl ComboBoxStyle for RecipeComboBoxStyle {
    fn make_body(&self, cfg: &ComboBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let border_width = theme.shape.border_width;
        let focus_ring_width = theme.shape.focus_ring_width;
        let height = self.recipe.height;
        let divider_height = height * 0.6;
        let padding_h = self.recipe.padding_horizontal;
        let corner_radius = self.recipe.corner_radius;

        // Plain variant — no chrome at all. Hand the label back
        // wrapped only in the min-height enforcement; callers using
        // this variant are responsible for any surrounding visuals.
        if matches!(cfg.variant, ComboBoxVariant::Plain) {
            let row_id = build_inner_row(ctx, cfg.selected_label, border_width, divider_height);
            let padded_id = ctx.add(Padding::symmetric(padding_h * 0.5, padding_h).child(row_id));
            return ctx.add(crate::primitives::MinSize::new(0.0, height).child(padded_id));
        }

        // Derived role signals. Roles encode "what this colour means";
        // the theme maps them to concrete colours at paint time, so the
        // result follows theme switches reactively.
        //
        // bg: Filled variants always use the Hover surface (their idle
        // and hover states share the same tinted fill — Material 3
        // convention). Outlined / Underline use Main idle, Hover when
        // open or hovered. AccentDisabled overrides everything when
        // disabled.
        let variant = cfg.variant;
        let bg_role = cfg.is_open.zip3(&cfg.is_hovered, &cfg.is_disabled).map(
            move |(open, hovered, disabled)| {
                if *disabled {
                    // Neutral inert grey — NOT `AccentDisabled`, which is a
                    // washed-out *accent* (pale cyan in IntUI) and belongs on
                    // accent-filled controls like a Filled Button. A ComboBox
                    // is a neutral field and must grey out like its SpinBox /
                    // TextInput neighbours on the same form.
                    SurfaceRole::Disabled
                } else if matches!(variant, ComboBoxVariant::Filled) {
                    SurfaceRole::Hover
                } else if *open || *hovered {
                    SurfaceRole::Hover
                } else {
                    SurfaceRole::Main
                }
            },
        );

        // border: thicker accent ring on focus, dimmed on disabled,
        // default border in any other state. IntUI doesn't paint a
        // separate focus ring around combo boxes — the border itself
        // is the focus indicator. Filled has no border at all.
        let border_role = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(move |(focused, disabled)| {
                if matches!(variant, ComboBoxVariant::Filled) {
                    // The Filled variant never paints a border; the
                    // role we return here is ignored because
                    // border_width is forced to 0 below.
                    BorderRole::Default
                } else if *disabled {
                    BorderRole::Disabled
                } else if *focused {
                    BorderRole::Focused
                } else {
                    BorderRole::Default
                }
            });

        let border_width_signal = cfg.is_focused.map(move |focused| match variant {
            ComboBoxVariant::Filled => 0.0,
            _ => {
                if *focused {
                    focus_ring_width
                } else {
                    border_width
                }
            }
        });

        let row_id = build_inner_row(ctx, cfg.selected_label, border_width, divider_height);
        let padding_id = ctx.add(Padding::symmetric(padding_h * 0.5, padding_h).child(row_id));

        let bg = RectWidget::new()
            .background(bg_role)
            .border_color(border_role)
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(corner_radius));
        let bg_id = ctx.add(bg);

        let visual_id = ctx.add(ZStack::new().child(bg_id).child(padding_id));
        ctx.add(crate::primitives::MinSize::new(0.0, height).child(visual_id))
    }
}

/// Build the trigger's inner row: `[selected_label | Spacer |
/// vertical divider | chevron icon]`. Shared between every variant —
/// only the surrounding chrome (bg / border / corner radius) varies.
fn build_inner_row(
    ctx: &mut BuildContext,
    selected_label: WidgetId,
    border_width: f32,
    divider_height: f32,
) -> WidgetId {
    let divider_fill_id = ctx.add(RectWidget::new().background(BorderRole::Default));
    let divider_id = ctx.add(
        FixedSize::new()
            .width(border_width)
            .height(divider_height)
            .child(divider_fill_id),
    );

    // Chevron colour: `text_primary` at 50 % alpha. No role captures
    // this blend, so we derive a `Signal<Color>` off `theme_signal`
    // directly.
    let chevron_color = ctx
        .theme_signal()
        .map(|t| t.colors.text_primary.with_alpha(0.5));
    let chevron = IconWidget::chevron_down(12.0).color(chevron_color);
    let chevron_id = ctx.add(chevron);

    ctx.add(
        HStack::new()
            .spacing(8.0)
            .child(selected_label)
            .child(Spacer::new())
            .child(divider_id)
            .child(chevron_id),
    )
}
