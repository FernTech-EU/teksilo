use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use fern_core::WidgetId;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};

/// A layout container that adds padding (insets) around a single child.
/// Each inset accepts a static `f32` or a reactive `Signal<f32>` so a
/// theme-derived inset can update without rebuilding the widget tree.
#[derive(Debug)]
pub struct Padding {
    top: Prop<f32>,
    right: Prop<f32>,
    bottom: Prop<f32>,
    left: Prop<f32>,
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl Padding {
    pub fn new(
        top: impl Into<Prop<f32>>,
        right: impl Into<Prop<f32>>,
        bottom: impl Into<Prop<f32>>,
        left: impl Into<Prop<f32>>,
    ) -> Self {
        Self {
            top: top.into(),
            right: right.into(),
            bottom: bottom.into(),
            left: left.into(),
            child_id: None,
            pending_child: None,
        }
    }

    pub fn uniform(amount: impl Into<Prop<f32>>) -> Self {
        let amount = amount.into();
        Self {
            top: amount.clone(),
            right: amount.clone(),
            bottom: amount.clone(),
            left: amount,
            child_id: None,
            pending_child: None,
        }
    }

    pub fn symmetric(vertical: impl Into<Prop<f32>>, horizontal: impl Into<Prop<f32>>) -> Self {
        let vertical = vertical.into();
        let horizontal = horizontal.into();
        Self {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical,
            left: horizontal,
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

    fn horizontal_inset(&self) -> f32 {
        self.left.get() + self.right.get()
    }

    fn vertical_inset(&self) -> f32 {
        self.top.get() + self.bottom.get()
    }
}

impl Widget for Padding {
    fn build(&mut self, ctx: &mut fern_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        // Register each inset prop for dirty-tracking so bound insets
        // (e.g. a theme-derived signal) trigger a relayout when they fire.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.top
            .register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        self.right
            .register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        self.bottom
            .register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        self.left
            .register_if_bound(self_id, registry, fern_core::binding::BindingLevel::Relayout);
        self.child_id.into_iter().collect()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let h_inset = self.horizontal_inset();
        let v_inset = self.vertical_inset();

        // Query child size if available, then add insets.
        if let Some(child_id) = self.child_id {
            let inner_proposal = SizeProposal {
                width: proposal.width.map(|w| (w - h_inset).max(0.0)),
                height: proposal.height.map(|h| (h - v_inset).max(0.0)),
            };
            if let Some(child_size) = ctx.child_size(child_id, inner_proposal) {
                return Size::new(child_size.width + h_inset, child_size.height + v_inset);
            }
        }

        let size = proposal.resolve(h_inset, v_inset);
        Size::new(size.width.max(h_inset), size.height.max(v_inset))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let top = self.top.get();
        let left = self.left.get();
        let h_inset = self.horizontal_inset();
        let v_inset = self.vertical_inset();
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + left, bounds.y + top);
            child.size = Size::new(
                (bounds.width - h_inset).max(0.0),
                (bounds.height - v_inset).max(0.0),
            );
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
