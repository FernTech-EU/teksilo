use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Layout modifier that enforces minimum dimensions on a child widget.
/// Constraints can be static or bound to reactive state for dynamic resizing.
#[derive(Debug)]
pub struct MinSize {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    min_width: Option<Prop<f32>>,
    min_height: Option<Prop<f32>>,
}

impl MinSize {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: Some(Prop::Static(width)),
            min_height: Some(Prop::Static(height)),
        }
    }

    pub fn width(width: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: Some(Prop::Static(width)),
            min_height: None,
        }
    }

    pub fn height(height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: None,
            min_height: Some(Prop::Static(height)),
        }
    }

    /// Bind min width to a reactive state.
    pub fn bind_min_width(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.min_width = Some(state.into());
        self
    }

    /// Bind min height to a reactive state.
    pub fn bind_min_height(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.min_height = Some(state.into());
        self
    }

    /// Set child by pre-registered ID.
    pub fn set_child(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }
}

impl Widget for MinSize {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(ref w) = self.min_width {
            w.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        }
        if let Some(ref h) = self.min_height {
            h.register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        }
        self.child_id.into_iter().collect()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let child_size = self
            .child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or(Size::ZERO);

        let w = match &self.min_width {
            Some(r) => child_size.width.max(r.get()),
            None => child_size.width,
        };
        let h = match &self.min_height {
            Some(r) => child_size.height.max(r.get()),
            None => child_size.height,
        };
        Size::new(w, h)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
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
    use fern_core::signal::Signal;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn clamps_small_child_to_minimum() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::new(48.0, 48.0).set_child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!((mb.width - 48.0).abs() < 0.01);
        assert!((mb.height - 48.0).abs() < 0.01);
    }

    #[test]
    fn large_child_is_not_clamped() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(100.0, 80.0));
        let min = tree.add(MinSize::new(48.0, 48.0).set_child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!((mb.width - 100.0).abs() < 0.01);
        assert!((mb.height - 80.0).abs() < 0.01);
    }

    #[test]
    fn min_width_only() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::width(48.0).set_child(child));
        tree.layout(SizeProposal::unspecified());

        let mb = tree.bounds(min);
        assert!((mb.width - 48.0).abs() < 0.01);
        assert!((mb.height - 10.0).abs() < 0.01);
    }

    #[test]
    fn bind_min_width_dynamic() {
        let min_w = Signal::new(48.0_f32);
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(
            MinSize::width(0.0)
                .bind_min_width(min_w.clone())
                .set_child(child),
        );
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(min).width - 48.0).abs() < 0.01);

        min_w.set(80.0);
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(min).width - 80.0).abs() < 0.01);
    }
}
