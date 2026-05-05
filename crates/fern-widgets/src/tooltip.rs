//! Tooltip system — hover-triggered overlay with configurable delay.
//!
//! A tooltip is attached to any widget via the WidgetTree API. The
//! `TooltipAttachment` stores the tooltip state and is processed by
//! the tree during event dispatch.

pub mod attach;
pub(crate) mod dwell_indicator;
pub mod registry;
pub mod rich;

pub use attach::{
    DEFAULT_RICH_TOOLTIP_DELAY, RichTooltipSource, attach_rich_tooltip,
    attach_rich_tooltip_content, attach_rich_tooltip_source,
};
pub use registry::{
    TooltipContent, TooltipRegistry, install_tooltip_registry, with_tooltip_registry,
};
pub use rich::RichTooltipWidget;

use std::time::{Duration, Instant};

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::overlay::OverlayId;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_tokens::CornerRadius;

/// A tooltip content widget — a themed rounded rect with text.
#[derive(Debug)]
pub struct TooltipWidget {
    text: String,
}

impl TooltipWidget {
    pub fn new(text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        Self {
            text: ls.resolve_now(),
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw string in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(text: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(text))
    }
}

impl Widget for TooltipWidget {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let style = ctx.theme.components.tooltip;
        let pad_h = style.padding_horizontal;
        let pad_v = style.padding_vertical;
        if let Some(backend) = ctx.text_backend {
            let mut backend = backend.borrow_mut();
            let layout = backend.layout_single_line(&self.text, &ctx.theme.typography.small, None);
            Size::new(layout.width + pad_h * 2.0, layout.height + pad_v * 2.0)
        } else {
            let text_width = self.text.len() as f32 * 7.0;
            let text_height = ctx.theme.typography.small.size;
            Size::new(text_width + pad_h * 2.0, text_height + pad_v * 2.0)
        }
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let style = ctx.theme.components.tooltip;
        let radius = CornerRadius::uniform(style.corner_radius);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        let text_bounds = Rect::new(
            bounds.x + style.padding_horizontal,
            bounds.y + style.padding_vertical,
            bounds.width - style.padding_horizontal * 2.0,
            bounds.height - style.padding_vertical * 2.0,
        );
        canvas.draw_text(
            &self.text,
            text_bounds,
            &ctx.theme.typography.small,
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
#[allow(dead_code)] // Part of tooltip system, used when tooltip overlays are wired up
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
