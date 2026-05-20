//! Default `PanelStyle` impl driven by paint-recipe data.
//!
//! `RecipePanelStyle` ships the IntUI panel chrome — variant-driven
//! background / border / corner-radius defaults, with caller overrides
//! winning when set. Custom styles compose freely (glassmorphism panel,
//! brutalist box, etc.) by writing their own `impl PanelStyle` block.
//!
//! The body is a single `PanelFrame` container widget that paints the
//! chrome AND positions the content with padding inset — done in one
//! widget so the proposal-resolve / intrinsic-size logic mirrors the
//! pre-refactor `Panel` (`Size::new(child + 2*pad, child + 2*pad)` when
//! unspecified, `proposal.resolve(...)` when bounded). Wrapping
//! Padding in a generic ZStack would break the proposal handoff: ZStack
//! measures its children with `unspecified` regardless of the incoming
//! proposal, so the chrome would inflate to the child's preferred
//! `unwrap_or(400.0, 300.0)` and overflow its parent.

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Prop;
use bastyde_core::styles::{PanelStyle, PanelStyleConfig, PanelVariant};
use bastyde_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

// IntUI design tokens for Panel. The recipe owns its own dimensions.
pub const PANEL_PADDING: f32 = 12.0;
pub const PANEL_CORNER_RADIUS: f32 = 8.0;
pub const PANEL_BORDER_WIDTH: f32 = 1.0;

/// Default `PanelStyle` shipped with Bastyde. Honours all four
/// `PanelVariant` values via background / border defaults; honours
/// caller overrides (background, border, corner radius, padding) when
/// set.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipePanelStyle;

impl PanelStyle for RecipePanelStyle {
    fn make_body(&self, cfg: &PanelStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let frame = PanelFrame {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
            variant: cfg.variant,
            background: cfg.background_override.clone(),
            border_color: cfg.border_color_override.clone(),
            border_width: cfg
                .border_width_override
                .clone()
                .unwrap_or(Prop::Static(PANEL_BORDER_WIDTH)),
            corner_radius: cfg
                .corner_radius_override
                .clone()
                .unwrap_or(Prop::Static(PANEL_CORNER_RADIUS)),
            padding: cfg
                .padding_override
                .clone()
                .unwrap_or(Prop::Static(PANEL_PADDING)),
        };
        ctx.add(frame)
    }
}

/// Internal container widget that paints the panel chrome and lays out
/// the content with padding inset. Combines what the pre-refactor
/// `Panel` did into a single Widget so proposal propagation works
/// correctly (a separate `Padding` inside a `ZStack` measures with
/// `unspecified` and inflates to the content's preferred size).
struct PanelFrame {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    variant: PanelVariant,
    background: Option<ColorProp>,
    border_color: Option<ColorProp>,
    border_width: Prop<f32>,
    corner_radius: Prop<f32>,
    padding: Prop<f32>,
}

impl std::fmt::Debug for PanelFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelFrame")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for PanelFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(p) = &self.background {
            p.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }
        if let Some(p) = &self.border_color {
            p.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }
        self.border_width
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.corner_radius
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        self.padding
            .register_if_bound(id, registry, BindingLevel::Relayout);
        self.child_id.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let pad = self.padding.get();
        let inset = pad * 2.0;
        if let Some(child_id) = self.child_id {
            let inner_proposal = SizeProposal {
                width: proposal.width.map(|w| (w - inset).max(0.0)),
                height: proposal.height.map(|h| (h - inset).max(0.0)),
            };
            if let Some(child_size) = ctx.child_size(child_id, inner_proposal) {
                return (Size::new(child_size.width + inset, child_size.height + inset)).into();
            }
        }
        proposal.resolve(inset, inset).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let pad = self.padding.get();
        for child in children.iter_mut() {
            child.origin = bastyde_canvas::Point::new(bounds.x + pad, bounds.y + pad);
            child.size = Size::new(
                (bounds.width - pad * 2.0).max(0.0),
                (bounds.height - pad * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;

        let bg = if let Some(p) = &self.background {
            p.resolve(ctx.theme, ctx.effective_enabled)
        } else {
            match self.variant {
                PanelVariant::Plain => colors.surface_main,
                PanelVariant::Sunken => colors.surface_sunken,
                PanelVariant::Raised => colors.surface_raised,
                PanelVariant::Highlighted => colors.accent_subtle_bg,
            }
        };

        let radius = self.corner_radius.get();
        let border_w = self.border_width.get();
        canvas.fill_rounded_rect(bounds, CornerRadius::uniform(radius), bg);

        if border_w > 0.0 {
            let border = if let Some(p) = &self.border_color {
                p.resolve(ctx.theme, ctx.effective_enabled)
            } else {
                match self.variant {
                    PanelVariant::Plain => colors.border,
                    PanelVariant::Sunken | PanelVariant::Raised => colors.border,
                    PanelVariant::Highlighted => colors.accent,
                }
            };
            canvas.stroke_rounded_rect(bounds, CornerRadius::uniform(radius), border, border_w);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational chrome — the parent Panel emits `Role::Group`.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
