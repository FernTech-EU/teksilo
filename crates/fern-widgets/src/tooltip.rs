//! Tooltip system — hover-triggered overlay with configurable delay.
//!
//! A tooltip is attached to any widget via the WidgetTree API. The
//! `TooltipAttachment` stores the tooltip state and is processed by
//! the tree during event dispatch.

use std::time::{Duration, Instant};

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::overlay::{
    DismissBehavior, OverlayId, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

/// A tooltip content widget — a themed rounded rect with text.
#[derive(Debug)]
pub struct TooltipWidget {
    text: String,
}

impl TooltipWidget {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl Widget for TooltipWidget {
    fn size_that_fits(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let pad = 8.0;
        if let Some(backend) = ctx.text_backend {
            let mut backend = backend.borrow_mut();
            let layout = backend.layout_single_line(
                &self.text,
                &ctx.theme.typography.body_small,
                None,
            );
            Size::new(layout.width + pad * 2.0, layout.height + pad * 2.0)
        } else {
            let text_width = self.text.len() as f32 * 7.0;
            let text_height = ctx.theme.typography.body_small.size;
            Size::new(text_width + pad * 2.0, text_height + pad * 2.0)
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(ctx.theme.shape.radius_sm);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_surface);
        let text_bounds = Rect::new(
            bounds.x + 8.0,
            bounds.y + 8.0,
            bounds.width - 16.0,
            bounds.height - 16.0,
        );
        canvas.draw_text(
            &self.text,
            text_bounds,
            &ctx.theme.typography.body_small,
            ctx.theme.colors.tooltip_text,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Tooltip);
        builder.set_name(&self.text);
    }
}

/// Tracks tooltip hover state for a widget.
/// Stored on the WidgetTree and processed during event dispatch.
pub(crate) struct TooltipState {
    /// The widget this tooltip is attached to.
    pub anchor_id: WidgetId,
    /// The pre-created tooltip content widget ID (starts dormant).
    pub content_id: WidgetId,
    /// The tooltip text.
    pub text: String,
    /// Hover delay before showing.
    pub delay: Duration,
    /// When the pointer entered the anchor (None if not hovering).
    pub hover_start: Option<Instant>,
    /// Active overlay ID (Some if tooltip is currently shown).
    pub overlay_id: Option<OverlayId>,
}

impl std::fmt::Debug for TooltipState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipState")
            .field("anchor_id", &self.anchor_id)
            .field("text", &self.text)
            .field("is_shown", &self.overlay_id.is_some())
            .finish()
    }
}
