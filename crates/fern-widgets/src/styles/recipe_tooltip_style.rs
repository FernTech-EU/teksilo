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

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::styles::{TooltipStyle, TooltipStyleConfig};
use fern_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

/// Default `TooltipStyle` shipped with FernUI. Reads dimensions from
/// `theme.components.tooltip` (padding + corner_radius) and chrome
/// from `theme.colors.tooltip_bg` + the `xs` shadow tier.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTooltipStyle;

impl TooltipStyle for RecipeTooltipStyle {
    fn make_body(&self, cfg: &TooltipStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let frame = TooltipFrame {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
        };
        ctx.add(frame)
    }
}

/// Internal container that paints the tooltip chrome (shadow + dark
/// background + corner radius) and lays out the content with the
/// tooltip padding inset. Sizing reads `theme.components.tooltip` at
/// layout time so theme changes refresh on the next pass.
struct TooltipFrame {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
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
        let style = ctx.theme.components.tooltip;
        let pad_h = style.padding_horizontal;
        let pad_v = style.padding_vertical;
        let inset_w = pad_h * 2.0;
        let inset_h = pad_v * 2.0;
        if let Some(child_id) = self.child_id {
            let inner = SizeProposal {
                width: proposal.width.map(|w| (w - inset_w).max(0.0)),
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
        ctx: &LayoutContext,
    ) {
        let style = ctx.theme.components.tooltip;
        let pad_h = style.padding_horizontal;
        let pad_v = style.padding_vertical;
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x + pad_h, bounds.y + pad_v);
            child.size = Size::new(
                (bounds.width - pad_h * 2.0).max(0.0),
                (bounds.height - pad_v * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let style = ctx.theme.components.tooltip;
        let radius = CornerRadius::uniform(style.corner_radius);
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
