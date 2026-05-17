use bastyde_canvas::{Point, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

/// Convenience layout widget that centers a single child within the
/// available space. Equivalent to `Expand::new().content_alignment(Alignment::CENTER)`.
#[derive(Debug)]
pub struct Center {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl Center {
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
        }
    }

    /// Set child by pre-registered ID.
    pub fn child_id(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Center {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Claim all offered space.
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            let child_size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(bounds.size());
            let dx = (bounds.width - child_size.width) / 2.0;
            let dy = (bounds.height - child_size.height) / 2.0;
            child.origin = Point::new(bounds.x + dx, bounds.y + dy);
            child.size = child_size;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Size;
    use bastyde_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    #[test]
    fn centers_child() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let _center = tree.add(Center::new().child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        assert!((cb.x - 80.0).abs() < 0.01); // (200-40)/2
        assert!((cb.y - 40.0).abs() < 0.01); // (100-20)/2
    }

    #[test]
    fn claims_full_space() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let center = tree.add(Center::new().child_id(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(center);
        assert!((cb.width - 200.0).abs() < 0.01);
        assert!((cb.height - 100.0).abs() < 0.01);
    }
}
