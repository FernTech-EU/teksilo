use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Convenience layout widget that centers a single child within the
/// available space. Equivalent to `Expand::new().content_alignment(Alignment::CENTER)`.
#[derive(Debug)]
pub struct Center {
    child_id: Option<WidgetId>,
}

impl Center {
    pub fn new() -> Self {
        Self { child_id: None }
    }

    pub fn set_child(mut self, id: WidgetId) -> Self {
        self.child_id = Some(id);
        self
    }
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Center {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Claim all offered space.
        proposal.resolve(0.0, 0.0)
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

    fn paint(&self, _bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn centers_child() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let center = tree.add(Center::new().set_child(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(child);
        assert!((cb.x - 80.0).abs() < 0.01); // (200-40)/2
        assert!((cb.y - 40.0).abs() < 0.01); // (100-20)/2
    }

    #[test]
    fn claims_full_space() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let center = tree.add(Center::new().set_child(child));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let cb = tree.bounds(center);
        assert!((cb.width - 200.0).abs() < 0.01);
        assert!((cb.height - 100.0).abs() < 0.01);
    }
}
