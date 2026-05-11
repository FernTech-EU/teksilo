//! Tooltip system — hover-triggered overlays with configurable delay.
//!
//! Three tiers, increasing in expressive power:
//!
//! - [`TooltipWidget`] — single line of localized text in a themed
//!   rounded rect. Attached via the per-widget `.tooltip(...)` setter.
//! - [`RichTooltipWidget`] — `TooltipContent`-driven (body + optional
//!   long-form "more" disclosure + shortcut chip), inline-markup body
//!   so `[label](:key)` cascade links resolve against
//!   [`TooltipRegistry`]. Attached via `.rich_tooltip(key)` /
//!   `.rich_tooltip_content(content)`. Promotes to a focusable
//!   `Role::Dialog` on dwell.
//! - [`composite::CompositeTooltipWidget`] — hosts an arbitrary
//!   `impl Widget + 'static` body inside the same chrome with a
//!   larger surface budget. Crusader Kings 3-style: tabbed sections,
//!   charts, progress bars, conditional rows. Attached via
//!   `.composite_tooltip(content)`. "Primary-only" by construction —
//!   has no inline-markup body and no registry key, so it cannot be
//!   the target of a `[label](:key)` cascade. Child widgets *inside*
//!   the body keep their own tooltip setters and cascade normally.
//!
//! All three tiers share the same overlay machinery, hover/focus
//! tracking, and dwell-promotion timer in `fern-core`. The per-widget
//! setters (`.tooltip` / `.rich_tooltip` / `.composite_tooltip`) are
//! mutually exclusive (last-one-wins): each setter clears the others.

pub mod attach;
pub mod composite;
pub(crate) mod dwell_indicator;
pub mod registry;
pub mod rich;

pub use attach::{
    DEFAULT_COMPOSITE_TOOLTIP_DELAY, DEFAULT_RICH_TOOLTIP_DELAY, RichTooltipSource,
    attach_composite_tooltip, attach_composite_tooltip_boxed, attach_rich_tooltip,
    attach_rich_tooltip_content, attach_rich_tooltip_source,
};
pub use composite::CompositeTooltipWidget;
pub use registry::{
    TooltipContent, TooltipRegistry, install_tooltip_registry, with_tooltip_registry,
};
pub use rich::RichTooltipWidget;

use std::rc::Rc;
use std::time::{Duration, Instant};

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::overlay::OverlayId;
use fern_core::styles::{SharedTooltipStyle, TooltipStyleConfig};
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, TextRole, TextStyleRole};

use crate::primitives::TextWidget;
use crate::shadow::paint_layered_shadow;

/// Tooltip-specific wrapper around [`paint_layered_shadow`] — pulls the
/// xs outer + inner shadow tokens and the per-component
/// `shadow_density` from the theme.
pub(crate) fn paint_tooltip_shadows(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: CornerRadius,
    ctx: &PaintContext,
) {
    paint_layered_shadow(
        canvas,
        bounds,
        radius,
        &ctx.theme.shape.shadow_xs,
        &ctx.theme.shape.shadow_inner_xs,
        ctx.theme.components.tooltip.shadow_density,
        None,
    );
}

/// Composite-tooltip variant of [`paint_tooltip_shadows`] — uses the
/// medium shadow tier (the larger CK3-style surface deserves more
/// presence than the punchy `xs` rim of plain tooltips).
pub(crate) fn paint_composite_tooltip_shadows(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: CornerRadius,
    ctx: &PaintContext,
) {
    paint_layered_shadow(
        canvas,
        bounds,
        radius,
        &ctx.theme.shape.shadow_md,
        &ctx.theme.shape.shadow_inner_md,
        ctx.theme.components.composite_tooltip.shadow_density,
        None,
    );
}

/// A tooltip content widget — a themed rounded rect with text.
///
/// Composes a `TextWidget` with `Small` typography in `tooltip_text` color,
/// then delegates the chrome (shadow, dark background, corner radius,
/// padding) to the active `TooltipStyle` (default
/// [`crate::styles::RecipeTooltipStyle`]). Apps install per-call
/// (`TooltipWidget::new(...).style(impl TooltipStyle)`) or theme-wide
/// (step 8's `ComponentStyles.tooltip = Rc::new(MyTooltip)`).
pub struct TooltipWidget {
    text: String,
    style_override: Option<SharedTooltipStyle>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for TooltipWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TooltipWidget")
            .field("text", &self.text)
            .finish()
    }
}

impl TooltipWidget {
    pub fn new(text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        Self {
            text: ls.resolve_now(),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw string in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(text: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(text))
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `TooltipStyle` for just this TooltipWidget instance.
    pub fn style(mut self, style: impl fern_core::styles::TooltipStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }
}

impl Widget for TooltipWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let text = TextWidget::new_literal(&self.text)
            .style(TextStyleRole::Small)
            .color(TextRole::TooltipText)
            .single_line();
        let text_id = ctx.add(text);

        let style: SharedTooltipStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.tooltip.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTooltipStyle::default()));
        let cfg = TooltipStyleConfig { content: text_id };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size.into();
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Tooltip);
        builder.set_name(&self.text);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[test]
    fn tooltip_widget_emits_shadow() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let _ = tree.add(TooltipWidget::new_literal("hello"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        assert!(
            !frame.shadows.is_empty(),
            "tooltip should emit at least one shadow"
        );
    }

    #[test]
    fn tooltip_overlay_emits_shadow_through_fade() {
        // End-to-end-ish: anchor + tooltip overlay with a fade scope
        // applied (the production overlay path). Shadow must still
        // land in the rendered frame.
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let anchor = tree.add(TooltipWidget::new_literal("anchor"));
        let tip = tree.add(TooltipWidget::new_literal("hello"));
        tree.set_dormant(tip);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.show_overlay(OverlayRequest {
            content_id: tip,
            anchor,
            placement: OverlayPlacement::NearAnchor {
                offset: fern_canvas::Vec2::new(0.0, 8.0),
            },
            dismiss: DismissBehavior::PointerLeave {
                delay: std::time::Duration::from_millis(100),
            },
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: Some(std::time::Duration::from_millis(120)),
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let frame = tree.render();
        assert!(
            !frame.shadows.is_empty(),
            "tooltip overlay should emit at least one shadow even under fade scope"
        );
    }
}
