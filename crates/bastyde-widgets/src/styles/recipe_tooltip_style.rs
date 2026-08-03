// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `TooltipStyle` impl driven by paint-recipe data.
//!
//! `RecipeTooltipStyle` paints the IntUI tooltip chrome — `shadow_xs`
//! pair + dark `tooltip_bg` (intentionally dark even in light theme,
//! the JetBrains house style). Used by all three tooltip tiers (plain,
//! rich, composite — though composite ships its own larger-shadow
//! variant via `RecipeCompositeTooltipStyle`).
//!
//! Apps that want a different look (light tooltip, branded chrome,
//! glassmorphism) write their own `impl TooltipStyle` block.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{TooltipStyle, TooltipStyleConfig};
use bastyde_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

// IntUI design tokens for plain Tooltip + CompositeTooltip. The recipe
// and tooltip widget own these constants.
pub const TOOLTIP_PADDING_HORIZONTAL: f32 = 10.0;
pub const TOOLTIP_PADDING_VERTICAL: f32 = 6.0;
pub const TOOLTIP_CORNER_RADIUS: f32 = 8.0;
pub const TOOLTIP_MAX_WIDTH: f32 = 320.0;
/// 0..=1 multiplier on `shape.shadow_inner_xs.color.a` at paint time.
pub const TOOLTIP_SHADOW_DENSITY: f32 = 1.0;

pub const COMPOSITE_TOOLTIP_PADDING_HORIZONTAL: f32 = 12.0;
pub const COMPOSITE_TOOLTIP_PADDING_VERTICAL: f32 = 12.0;
pub const COMPOSITE_TOOLTIP_CORNER_RADIUS: f32 = 8.0;
pub const COMPOSITE_TOOLTIP_MAX_WIDTH: f32 = 480.0;
pub const COMPOSITE_TOOLTIP_MAX_HEIGHT: f32 = 480.0;
/// 0..=1 multiplier on `shape.shadow_inner_md.color.a` at paint time.
pub const COMPOSITE_TOOLTIP_SHADOW_DENSITY: f32 = 0.7;

/// Configurable dimensions for [`RecipeTooltipStyle`].
///
/// Fields mirror the `TOOLTIP_*` constants defined in this module.
/// Construct via `Default` (reads the constants) or override individual
/// fields for a custom look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooltipRecipe {
    pub padding_horizontal: f32,
    pub padding_vertical: f32,
    pub corner_radius: f32,
    pub max_width: f32,
    /// 0..=1 multiplier on `shape.shadow_inner_xs.color.a` at paint time.
    pub shadow_density: f32,
}

impl Default for TooltipRecipe {
    fn default() -> Self {
        Self {
            padding_horizontal: TOOLTIP_PADDING_HORIZONTAL,
            padding_vertical: TOOLTIP_PADDING_VERTICAL,
            corner_radius: TOOLTIP_CORNER_RADIUS,
            max_width: TOOLTIP_MAX_WIDTH,
            shadow_density: TOOLTIP_SHADOW_DENSITY,
        }
    }
}

/// Default `TooltipStyle` shipped with Bastyde. Chrome from
/// `theme.colors.tooltip_bg` + the `xs` shadow tier.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTooltipStyle {
    pub recipe: TooltipRecipe,
}

impl RecipeTooltipStyle {
    pub fn new(recipe: TooltipRecipe) -> Self {
        Self { recipe }
    }
}

impl TooltipStyle for RecipeTooltipStyle {
    fn make_body(&self, cfg: &TooltipStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let frame = TooltipFrame {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
            recipe: self.recipe,
        };
        ctx.add(frame)
    }
}

/// Internal container that paints the tooltip chrome (shadow + dark
/// background + corner radius) and lays out the content with the
/// tooltip padding inset. Sizing reads fields from [`TooltipRecipe`].
struct TooltipFrame {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    recipe: TooltipRecipe,
}

impl std::fmt::Debug for TooltipFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipFrame").finish()
    }
}

impl Widget for TooltipFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let pad_h = self.recipe.padding_horizontal;
        let pad_v = self.recipe.padding_vertical;
        let inset_w = pad_h * 2.0;
        let inset_h = pad_v * 2.0;
        if let Some(child_id) = self.child_id {
            // Cap the content width at the recipe's `max_width` even when the
            // proposal is unbounded (the overlay measurement pass proposes
            // `None`), so the body wraps at the token instead of running on.
            let bounded_w = proposal
                .width
                .map(|w| w.min(self.recipe.max_width))
                .unwrap_or(self.recipe.max_width);
            let inner = SizeProposal {
                width: Some((bounded_w - inset_w).max(0.0)),
                height: proposal.height.map(|h| (h - inset_h).max(0.0)),
            };
            if let Some(child_size) = ctx.child_size(child_id, inner) {
                return Size::new(child_size.width + inset_w, child_size.height + inset_h).into();
            }
        }
        proposal.resolve(inset_w, inset_h).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let pad_h = self.recipe.padding_horizontal;
        let pad_v = self.recipe.padding_vertical;
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x + pad_h, bounds.y + pad_v);
            child.size = Size::new(
                (bounds.width - pad_h * 2.0).max(0.0),
                (bounds.height - pad_v * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(self.recipe.corner_radius);
        crate::tooltip::paint_tooltip_shadows(canvas, bounds, radius, ctx);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent TooltipWidget emits Role::Tooltip.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
