// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `BadgeStyle` impl driven by paint-recipe data.
//!
//! `RecipeBadgeStyle` ships the IntUI badge chrome: a fully-rounded
//! (`9999` radius) pill with a soft `AccentSubtle` background by
//! default, content inset by the badge padding. Pure composition —
//! no custom paint.
//!
//! Apps that want a different badge look (square tag, status-tinted
//! pill, bordered chip) write their own `impl BadgeStyle` block and
//! install it per-call (`Badge::style(...)`) or theme-wide
//! (`theme.style_slots.badge`).

use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::styles::{BadgeStyle, BadgeStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, InputTokens, SurfaceRole};

use crate::primitives::{Padding, RectWidget, ZStack};
use teksilo_core::styles::density::spacing;

// IntUI design tokens for Badge. The recipe owns its own dimensions.
pub const BADGE_PADDING_HORIZONTAL: f32 = 6.0;
pub const BADGE_PADDING_VERTICAL: f32 = 1.0;
/// Fully-rounded pill — a large radius the renderer clamps to half the
/// shorter side.
pub const BADGE_CORNER_RADIUS: f32 = 9999.0;

/// Dimension bundle for [`RecipeBadgeStyle`]. All fields are `f32` and
/// the struct is `Copy`, so it can be passed freely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BadgeRecipe {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
}

impl BadgeRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            padding_horizontal: spacing(BADGE_PADDING_HORIZONTAL, tokens),
            padding_vertical: spacing(BADGE_PADDING_VERTICAL, tokens),
            corner_radius: BADGE_CORNER_RADIUS,
        }
    }
}

impl Default for BadgeRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `BadgeStyle` shipped with Teksilo. Background defaults to
/// `SurfaceRole::AccentSubtle` when the caller sets no override.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeBadgeStyle {
    pub recipe: BadgeRecipe,
}

impl RecipeBadgeStyle {
    pub fn new(recipe: BadgeRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeBadgeStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: BadgeRecipe::for_tokens(tokens),
        }
    }
}

impl BadgeStyle for RecipeBadgeStyle {
    fn make_body(&self, cfg: &BadgeStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let bg: ColorProp = cfg
            .background_override
            .clone()
            .unwrap_or(ColorProp::SurfaceRole(SurfaceRole::AccentSubtle));
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .corner_radius(CornerRadius::uniform(self.recipe.corner_radius)),
        );
        let padding_id = ctx.add(
            Padding::symmetric(self.recipe.padding_vertical, self.recipe.padding_horizontal)
                .child_id(cfg.content),
        );
        ctx.add(ZStack::new().add_child(bg_rect).add_child(padding_id))
    }
}
