//! Card — a Panel with shadow and optional header/content/footer slots.

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::color_prop::ColorProp;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{CornerRadius, Shadow};

/// A card container with shadow, background, and optional header/content/footer.
#[derive(Debug)]
pub struct Card {
    header_id: Option<WidgetId>,
    content_id: Option<WidgetId>,
    footer_id: Option<WidgetId>,
    pending_header: Option<PendingChild>,
    pending_content: Option<PendingChild>,
    pending_footer: Option<PendingChild>,
    shadow: Option<Shadow>,
    background: Option<ColorProp>,
    corner_radius: Option<Prop<f32>>,
    padding: Option<Prop<f32>>,
}

impl Card {
    pub fn new() -> Self {
        Self {
            header_id: None,
            content_id: None,
            footer_id: None,
            pending_header: None,
            pending_content: None,
            pending_footer: None,
            shadow: None,
            background: None,
            corner_radius: None,
            padding: None,
        }
    }

    pub fn header(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_header = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn header_id(mut self, id: WidgetId) -> Self {
        self.pending_header = Some(PendingChild::Id(id));
        self
    }

    pub fn content(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn content_id(mut self, id: WidgetId) -> Self {
        self.pending_content = Some(PendingChild::Id(id));
        self
    }

    pub fn footer(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_footer = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn footer_id(mut self, id: WidgetId) -> Self {
        self.pending_footer = Some(PendingChild::Id(id));
        self
    }

    pub fn shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = Some(shadow);
        self
    }

    /// Override the background. Default (unset) is `SurfaceRole::Main`.
    /// Accepts `Color`, a role (`SurfaceRole`, …), or `Signal<Color>`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Override the corner radius (default: theme `shape.radius_popup`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn corner_radius(mut self, radius: impl Into<Prop<f32>>) -> Self {
        self.corner_radius = Some(radius.into());
        self
    }

    /// Override the padding (default: theme `components.card.padding`).
    /// Accepts a static `f32` or a reactive `Signal<f32>`.
    pub fn padding(mut self, padding: impl Into<Prop<f32>>) -> Self {
        self.padding = Some(padding.into());
        self
    }

    fn resolve_padding(&self, theme: &fern_core::Theme) -> f32 {
        self.padding
            .as_ref()
            .map(|p| p.get())
            .unwrap_or(theme.components.card.padding)
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(h) = self.pending_header.take() {
            self.header_id = Some(match h {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        if let Some(c) = self.pending_content.take() {
            self.content_id = Some(match c {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        if let Some(f) = self.pending_footer.take() {
            self.footer_id = Some(match f {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Register reactive props for dirty-tracking.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(p) = &self.background {
            p.register_if_bound(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }
        if let Some(p) = &self.corner_radius {
            p.register_if_bound(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }
        if let Some(p) = &self.padding {
            p.register_if_bound(
                self_id,
                registry,
                fern_core::binding::BindingLevel::Relayout,
            );
        }
        [self.header_id, self.content_id, self.footer_id]
            .into_iter()
            .flatten()
            .collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let pad = self.resolve_padding(ctx.theme);
        let inset = pad * 2.0;
        let inner_width = proposal.width.map(|w| (w - inset).max(0.0));

        let mut total_height = 0.0_f32;
        let children = [self.header_id, self.content_id, self.footer_id];
        let mut child_count = 0;

        for child_id in children.into_iter().flatten() {
            let child_proposal = SizeProposal {
                width: inner_width,
                height: None,
            };
            if let Some(child_size) = ctx.child_size(child_id, child_proposal) {
                total_height += child_size.height;
                child_count += 1;
            }
        }

        // Add spacing between sections
        if child_count > 1 {
            total_height += (child_count as f32 - 1.0) * pad * 0.5;
        }

        let width = proposal.width.unwrap_or(inset);
        let height = total_height + inset;
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let pad = self.resolve_padding(ctx.theme);
        let inner_width = (bounds.width - pad * 2.0).max(0.0);
        let spacing = pad * 0.5;
        let mut y = bounds.y + pad;

        for child in children.iter_mut() {
            let child_proposal = SizeProposal {
                width: Some(inner_width),
                height: None,
            };
            let child_size = ctx
                .child_size(child.id, child_proposal)
                .unwrap_or(Size::ZERO);
            child.origin = Point::new(bounds.x + pad, y);
            child.size = Size::new(inner_width, child_size.height);
            y += child_size.height + spacing;
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = self
            .corner_radius
            .as_ref()
            .map(|p| p.get())
            .unwrap_or(ctx.theme.shape.radius_popup);
        let cr = CornerRadius::uniform(radius);

        // Shadow — outer + inner pair from theme, density from CardStyle.
        let outer = self.shadow.unwrap_or(ctx.theme.shape.shadow_md);
        crate::shadow::paint_layered_shadow(
            canvas,
            bounds,
            cr,
            &outer,
            &ctx.theme.shape.shadow_inner_md,
            ctx.theme.components.card.shadow_density,
            None,
        );

        // Background
        let bg = self
            .background
            .as_ref()
            .map(|p| p.resolve(ctx.theme))
            .unwrap_or(ctx.theme.colors.surface_main);
        canvas.fill_rounded_rect(bounds, cr, bg);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        [self.header_id, self.content_id, self.footer_id]
            .into_iter()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn card_renders_shadow_and_background() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let _content = tree.add(FixedLeaf(100.0, 50.0));
        tree.add(Card::new().content(FixedLeaf(100.0, 50.0)));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let frame = tree.render();
        // Should have shapes for shadow + background
        assert!(
            !frame.shapes.is_empty() || !frame.shadows.is_empty(),
            "card should render shadow and/or background"
        );
    }

    #[test]
    fn card_with_header_and_content() {
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Card::new()
                .header(FixedLeaf(100.0, 30.0))
                .content(FixedLeaf(100.0, 50.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        // Should not crash and should layout
    }
}
