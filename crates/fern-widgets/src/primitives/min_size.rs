use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::state::{BindingLevel, Reactive, State};
use fern_core::widget::{IntoWidgetTree, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

/// Layout modifier that enforces minimum dimensions on a child widget.
/// Constraints can be static or bound to reactive state for dynamic resizing.
#[derive(Debug)]
pub struct MinSize {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    min_width: Option<Reactive<f32>>,
    min_height: Option<Reactive<f32>>,
    visible_when_state: Option<State<bool>>,
    enabled_when_state: Option<State<bool>>,
}

impl MinSize {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: Some(Reactive::Static(width)),
            min_height: Some(Reactive::Static(height)),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn width(width: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: Some(Reactive::Static(width)),
            min_height: None,
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn height(height: f32) -> Self {
        Self {
            child_id: None,
            pending_child: None,
            min_width: None,
            min_height: Some(Reactive::Static(height)),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    /// Bind min width to a reactive state.
    pub fn bind_min_width(mut self, state: impl Into<Reactive<f32>>) -> Self {
        self.min_width = Some(state.into());
        self
    }

    /// Bind min height to a reactive state.
    pub fn bind_min_height(mut self, state: impl Into<Reactive<f32>>) -> Self {
        self.min_height = Some(state.into());
        self
    }

    /// Set child by pre-registered ID.
    pub fn set_child(mut self, id: WidgetId) -> Self {
        self.pending_child = Some(PendingChild::Id(id));
        self
    }

    /// Set an inline child widget (deferred insertion).
    pub fn child(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending_child = Some(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Bind visibility to a boolean state (toggles dormant/active).
    pub fn visible_when(mut self, state: State<bool>) -> Self {
        self.visible_when_state = Some(state);
        self
    }

    /// Bind enabled state to a boolean state.
    pub fn enabled_when(mut self, state: State<bool>) -> Self {
        self.enabled_when_state = Some(state);
        self
    }
}

impl Widget for MinSize {
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

    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        self.pending_child.take().into_iter().collect()
    }

    fn set_resolved_children(&mut self, ids: Vec<WidgetId>) {
        self.child_id = ids.into_iter().next();
    }

    fn take_visible_when(&mut self) -> Option<State<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<State<bool>> {
        self.enabled_when_state.take()
    }

    fn register_bindings(
        &self,
        id: WidgetId,
        registry: &fern_core::state::BindingRegistry,
    ) {
        if let Some(ref w) = self.min_width {
            w.register_if_bound(id, registry, BindingLevel::Relayout);
        }
        if let Some(ref h) = self.min_height {
            h.register_if_bound(id, registry, BindingLevel::Relayout);
        }
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
    fn clamps_small_child_to_minimum() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::new(48.0, 48.0).set_child(child));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let mb = tree.bounds(min);
        assert!((mb.width - 48.0).abs() < 0.01);
        assert!((mb.height - 48.0).abs() < 0.01);
    }

    #[test]
    fn large_child_is_not_clamped() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(100.0, 80.0));
        let min = tree.add(MinSize::new(48.0, 48.0).set_child(child));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let mb = tree.bounds(min);
        assert!((mb.width - 100.0).abs() < 0.01);
        assert!((mb.height - 80.0).abs() < 0.01);
    }

    #[test]
    fn min_width_only() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::width(48.0).set_child(child));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let mb = tree.bounds(min);
        assert!((mb.width - 48.0).abs() < 0.01);
        assert!((mb.height - 10.0).abs() < 0.01);
    }

    #[test]
    fn bind_min_width_dynamic() {
        let min_w = State::new(48.0_f32);
        let mut tree = WidgetTree::new();
        let child = tree.add(FixedLeaf(20.0, 10.0));
        let min = tree.add(MinSize::width(0.0).bind_min_width(min_w.clone()).set_child(child));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!((tree.bounds(min).width - 48.0).abs() < 0.01);

        min_w.set(80.0);
        tree.layout(SizeProposal::exact(200.0, 200.0));
        assert!((tree.bounds(min).width - 80.0).abs() < 0.01);
    }
}
