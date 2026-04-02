use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::state::State;
use fern_core::widget::{EventContext, IntoWidgetTree, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::WidgetId;

/// A layout container that adds padding (insets) around a single child.
#[derive(Debug)]
pub struct Padding {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    visible_when_state: Option<State<bool>>,
    enabled_when_state: Option<State<bool>>,
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
            visible_when_state: None,
            enabled_when_state: None,
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

    fn horizontal_inset(&self) -> f32 {
        self.left + self.right
    }

    fn vertical_inset(&self) -> f32 {
        self.top + self.bottom
    }
}

impl Widget for Padding {
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

    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

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
}
