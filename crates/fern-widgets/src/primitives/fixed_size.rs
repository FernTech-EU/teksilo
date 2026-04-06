use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Layout modifier that prevents a widget from expanding beyond its natural size,
/// or constrains it to specific reactive dimensions.
///
/// Without bindings, reports the child's natural size (ignoring parent proposal).
/// With `bind_width`/`bind_height`, constrains to the bound values.
#[derive(Debug)]
pub struct FixedSize {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    width: Option<Prop<f32>>,
    height: Option<Prop<f32>>,
}

impl FixedSize {
    pub fn new() -> Self {
        Self {
            child_id: None,
            pending_child: None,
            width: None,
            height: None,
        }
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

    /// Bind width to a reactive state. When the state changes, relayout is triggered.
    pub fn bind_width(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.width = Some(state.into());
        self
    }

    /// Bind height to a reactive state. When the state changes, relayout is triggered.
    pub fn bind_height(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.height = Some(state.into());
        self
    }
}

impl Default for FixedSize {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for FixedSize {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Register reactive bindings
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(ref w) = self.width {
            w.register_if_bound(self_id, registry, fern_core::state::BindingLevel::Relayout);
        }
        if let Some(ref h) = self.height {
            h.register_if_bound(self_id, registry, fern_core::state::BindingLevel::Relayout);
        }
        self.child_id.into_iter().collect()
    }

    fn size_that_fits(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let child_size = self
            .child_id
            .and_then(|id| ctx.child_size(id, SizeProposal::unspecified()))
            .unwrap_or(Size::ZERO);

        let w = self
            .width
            .as_ref()
            .map(|r| r.get())
            .unwrap_or(child_size.width);
        let h = self
            .height
            .as_ref()
            .map(|r| r.get())
            .unwrap_or(child_size.height);
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
    use fern_core::state::State;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn reports_child_natural_size() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let fixed = tree.add(FixedSize::new().set_child(child));
        tree.layout(SizeProposal::unspecified());

        let fb = tree.bounds(fixed);
        assert!((fb.width - 40.0).abs() < 0.01);
        assert!((fb.height - 20.0).abs() < 0.01);
    }

    #[test]
    fn ignores_parent_proposal() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let fixed = tree.add(FixedSize::new().set_child(child));
        tree.layout(SizeProposal::unspecified());

        let fb = tree.bounds(fixed);
        assert!((fb.width - 40.0).abs() < 0.01);
        assert!((fb.height - 20.0).abs() < 0.01);
    }

    #[test]
    fn bind_width_constrains_size() {
        let width = State::new(150.0_f32);
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let fixed = tree.add(FixedSize::new().bind_width(width.clone()).set_child(child));
        tree.layout(SizeProposal::unspecified());

        let fb = tree.bounds(fixed);
        assert!((fb.width - 150.0).abs() < 0.01); // bound width
        assert!((fb.height - 20.0).abs() < 0.01); // child's natural height
    }

    #[test]
    fn bind_width_triggers_relayout_on_change() {
        let width = State::new(200.0_f32);
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(40.0, 20.0));
        let fixed = tree.add(FixedSize::new().bind_width(width.clone()).set_child(child));
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(fixed).width - 200.0).abs() < 0.01);

        width.set(100.0);
        tree.layout(SizeProposal::unspecified());
        assert!((tree.bounds(fixed).width - 100.0).abs() < 0.01);
    }
}
