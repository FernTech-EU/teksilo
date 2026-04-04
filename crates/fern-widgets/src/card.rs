//! Card — a Panel with shadow and optional header/content/footer slots.

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{IntoWidgetTree, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, Shadow};

/// A card container with shadow, background, and optional header/content/footer.
#[derive(Debug)]
pub struct Card {
    header_id: Option<WidgetId>,
    content_id: Option<WidgetId>,
    footer_id: Option<WidgetId>,
    pending_header: Option<PendingChild>,
    pending_content: Option<PendingChild>,
    pending_footer: Option<PendingChild>,
    /// Tracks which slots had pending children for correct set_resolved_children.
    pending_slot_order: Vec<Slot>,
    shadow: Option<Shadow>,
    background: Option<Color>,
    corner_radius: Option<f32>,
    padding: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
enum Slot {
    Header,
    Content,
    Footer,
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
            pending_slot_order: Vec::new(),
            shadow: None,
            background: None,
            corner_radius: None,
            padding: None,
        }
    }

    pub fn header(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending_header = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn content(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending_content = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn footer(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending_footer = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    pub fn shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = Some(shadow);
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = Some(radius);
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = Some(padding);
        self
    }

    fn resolve_padding(&self, theme: &fern_tokens::Theme) -> f32 {
        self.padding.unwrap_or(theme.spacing.content_padding)
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
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
        Size::new(width, height)
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
        let radius = self.corner_radius.unwrap_or(ctx.theme.shape.radius_md);
        let cr = CornerRadius::uniform(radius);

        // Shadow
        let shadow = self.shadow.unwrap_or(ctx.theme.shape.shadow_md);
        canvas.draw_shadow(bounds, cr, &shadow);

        // Background
        let bg = self.background.unwrap_or(ctx.theme.colors.surface);
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

    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        let mut children = Vec::new();
        self.pending_slot_order.clear();
        if let Some(h) = self.pending_header.take() {
            children.push(h);
            self.pending_slot_order.push(Slot::Header);
        }
        if let Some(c) = self.pending_content.take() {
            children.push(c);
            self.pending_slot_order.push(Slot::Content);
        }
        if let Some(f) = self.pending_footer.take() {
            children.push(f);
            self.pending_slot_order.push(Slot::Footer);
        }
        children
    }

    fn set_resolved_children(&mut self, ids: Vec<WidgetId>) {
        for (id, slot) in ids.into_iter().zip(self.pending_slot_order.iter()) {
            match slot {
                Slot::Header => self.header_id = Some(id),
                Slot::Content => self.content_id = Some(id),
                Slot::Footer => self.footer_id = Some(id),
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn card_renders_shadow_and_background() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let content = tree.add(FixedLeaf(100.0, 50.0));
        tree.add(Card::new().content(FixedLeaf(100.0, 50.0)));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let frame = tree.render();
        // Should have shapes for shadow + background
        assert!(!frame.shapes.is_empty() || !frame.shadows.is_empty(),
            "card should render shadow and/or background");
    }

    #[test]
    fn card_with_header_and_content() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            Card::new()
                .header(FixedLeaf(100.0, 30.0))
                .content(FixedLeaf(100.0, 50.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        // Should not crash and should layout
    }
}
