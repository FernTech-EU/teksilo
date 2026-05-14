//! Default `DialogStyle` impl driven by paint-recipe data.
//!
//! `RecipeDialogStyle` ships the IntUI modal chrome:
//! [`DialogStyle::make_panel`] builds a `DialogPanel` frame — a rounded
//! `surface_main` panel with a `border_strong` stroke and the dialog
//! content-padding inset — and [`DialogStyle::make_scrim`] builds the
//! full-window dimming scrim (`SurfaceRole::Scrim`).
//!
//! The modal-presentation pipeline owns *mounting* both surfaces;
//! `RecipeDialogStyle` only owns their look. Apps that want a different
//! modal chrome (frosted-glass panel, no scrim, branded border) write
//! their own `impl DialogStyle` block and install it per-call or
//! theme-wide (`theme.style_slots.dialog`).

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::styles::{DialogStyle, DialogStyleConfig};
use fern_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::RectWidget;

// IntUI design tokens for Dialog. Relocated from
// `theme.components.dialog` in Stage B of the group-5 styling
// migration — the recipe owns its own dimensions.
pub const DIALOG_CONTENT_PADDING: f32 = 24.0;
pub const DIALOG_MIN_WIDTH: f32 = 280.0;
pub const DIALOG_CORNER_RADIUS: f32 = 8.0;

/// Default `DialogStyle` shipped with FernUI. Panel chrome is the
/// rounded `surface_main` surface + `border_strong` stroke; the scrim
/// is a plain `SurfaceRole::Scrim` fill.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeDialogStyle;

impl DialogStyle for RecipeDialogStyle {
    fn make_panel(&self, cfg: &DialogStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(DialogPanel {
            child_id: None,
            pending_child: Some(PendingChild::Id(cfg.content)),
            padding: cfg.padding_override.unwrap_or(DIALOG_CONTENT_PADDING),
            min_width: cfg.min_width_override.unwrap_or(DIALOG_MIN_WIDTH),
        })
    }

    fn make_scrim(&self, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(RectWidget::new().background(SurfaceRole::Scrim))
    }
}

/// Internal container that paints the modal panel chrome (rounded
/// `surface_main` fill + `border_strong` stroke) and positions the
/// content with the dialog padding inset. Mirrors the pre-migration
/// `ModalContainer` layout exactly (single widget so proposal-resolve
/// and the `min_width` clamp behave identically).
struct DialogPanel {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    padding: f32,
    min_width: f32,
}

impl std::fmt::Debug for DialogPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogPanel")
            .field("padding", &self.padding)
            .field("min_width", &self.min_width)
            .finish()
    }
}

impl Widget for DialogPanel {
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
        let inset = self.padding * 2.0;
        let content = self
            .child_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: proposal.width.map(|width| (width - inset).max(0.0)),
                        height: proposal.height.map(|height| (height - inset).max(0.0)),
                    },
                )
            })
            .unwrap_or_else(|| proposal.resolve(240.0, 120.0));

        Size::new(
            (content.width + inset).max(self.min_width),
            content.height + inset,
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let pad = self.padding;
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x + pad, bounds.y + pad);
            child.size = Size::new(
                (bounds.width - pad * 2.0).max(0.0),
                (bounds.height - pad * 2.0).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(DIALOG_CORNER_RADIUS);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.surface_main);
        canvas.stroke_rounded_rect(
            bounds,
            radius,
            ctx.theme.colors.border_strong,
            ctx.theme.shape.border_width,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational — the parent `ModalContainer` emits the modal
        // `Role::Dialog` node with the accessible name.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
