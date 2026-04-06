use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use fern_core::WidgetId;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};

/// A layout container that adds padding (insets) around a single child.
#[derive(Debug)]
pub struct Padding {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
}

impl Padding {
    pub fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
            child_id: None,
            pending_child: None,
        }
    }

    pub fn uniform(amount: f32) -> Self {
        Self::new(amount, amount, amount, amount)
    }

    pub fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self::new(vertical, horizontal, vertical, horizontal)
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

    fn horizontal_inset(&self) -> f32 {
        self.left + self.right
    }

    fn vertical_inset(&self) -> f32 {
        self.top + self.bottom
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
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + self.left, bounds.y + self.top);
            child.size = Size::new(
                (bounds.width - self.horizontal_inset()).max(0.0),
                (bounds.height - self.vertical_inset()).max(0.0),
            );
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
