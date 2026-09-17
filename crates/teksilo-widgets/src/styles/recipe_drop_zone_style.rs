// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `DropZoneStyle` impl driven by paint-recipe data.
//!
//! `RecipeDropZoneStyle` ships the IntUI drop-zone chrome: a rounded,
//! bordered region whose surface tint and border color track the
//! interaction state (idle / accepting / rejecting) via the config's
//! reactive `Signal<DropZoneVisualState>`. The content column (prompt,
//! subtitle, status line, Browse button) is centered and inset by the
//! zone padding.
//!
//! Apps wanting a different look (dashed border, full-bleed dropzone,
//! image-backed) write their own `impl DropZoneStyle` block and install it
//! per-call (`DropZone::style(...)`) or theme-wide
//! (`theme.style_slots.drop_zone = Some(Rc::new(...))`).

use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::{DropZoneStyle, DropZoneStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, InputTokens};

use crate::primitives::{Center, Padding, RectWidget, ZStack};
use teksilo_core::styles::density::spacing;

/// Corner radius of the zone's rounded rectangle.
pub const DROP_ZONE_CORNER_RADIUS: f32 = 12.0;
/// Border thickness in logical pixels.
pub const DROP_ZONE_BORDER_WIDTH: f32 = 2.0;
/// Inner padding between the border and the content column.
pub const DROP_ZONE_PADDING: f32 = 20.0;

/// Configurable dimensions for [`RecipeDropZoneStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropZoneRecipe {
    /// Corner radius of the zone's rounded rectangle.
    pub corner_radius: f32,
    /// Border thickness in logical pixels.
    pub border_width: f32,
    /// Inner padding between the border and the content column.
    pub padding: f32,
}

impl DropZoneRecipe {
    /// This recipe's dimensions resolved against a density's [`InputTokens`].
    ///
    /// [`Default`] is `for_tokens(&InputTokens::default())` — the Compact
    /// ladder — so the shipped values below are the Compact column by
    /// construction and cannot drift from it.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            corner_radius: DROP_ZONE_CORNER_RADIUS,
            border_width: DROP_ZONE_BORDER_WIDTH,
            padding: spacing(DROP_ZONE_PADDING, tokens),
        }
    }
}

impl Default for DropZoneRecipe {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

/// Default `DropZoneStyle` shipped with Teksilo.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeDropZoneStyle {
    /// Dimension recipe used when painting the drop zone chrome.
    pub recipe: DropZoneRecipe,
}

impl RecipeDropZoneStyle {
    /// Create a style with a custom dimension recipe.
    pub fn new(recipe: DropZoneRecipe) -> Self {
        Self { recipe }
    }

    /// This style with every dimension resolved against a density's
    /// [`InputTokens`], as `RecipeDropZoneStyle::for_tokens(&ctx.theme().input)` at
    /// the widget's own build site.
    ///
    /// [`Default`] is the `TargetDensity::Compact` projection, so a Compact
    /// tree gets exactly the values this module documents.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            recipe: DropZoneRecipe::for_tokens(tokens),
        }
    }
}

impl DropZoneStyle for RecipeDropZoneStyle {
    fn make_body(&self, cfg: &DropZoneStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Surface + border colors track the reactive interaction state.
        let bg = cfg.state.map(|s| s.surface_role());
        let border = cfg.state.map(|s| s.border_role());

        let rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .border_color(border)
                .border_width(self.recipe.border_width)
                .corner_radius(CornerRadius::uniform(self.recipe.corner_radius)),
        );

        let centered =
            Center::new().child(Padding::uniform(self.recipe.padding).child(cfg.content));

        ctx.add(ZStack::new().child(rect).child(centered))
    }
}
