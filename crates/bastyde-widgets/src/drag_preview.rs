//! Small composing widget used by `ListView` / `TreeView` to wrap the
//! delegate-built preview passed to `EventContext::start_drag_with_preview`.
//!
//! The wrapper:
//! - Fixes the preview's width/height so it has a stable footprint while
//!   floating at the pointer (otherwise a `Spacer` inside the delegate would
//!   collapse under the unbounded overlay proposal).
//! - Renders a `Raised` surface behind the content so the preview reads as a
//!   picked-up row against the window background.
//!
//! The `Box<dyn Widget>` from the delegate is absorbed into the arena as a
//! single child during `build()`. Nothing in here is user-facing — the type
//! is `pub(crate)` and only referenced from the list/tree source.

use bastyde_canvas::{Point, Rect, Size, SizeProposal};

use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::SurfaceRole;

use crate::panel::Panel;

#[derive(Debug)]
pub(crate) struct DragPreview {
    width: f32,
    height: f32,
    /// Taken by `build()` — the inner widget is absorbed into the arena
    /// as this widget's child on first build, and `None`d thereafter so
    /// subsequent rebuilds don't try to reinsert a moved value.
    inner: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
}

impl DragPreview {
    pub(crate) fn new(width: f32, height: f32, inner: Box<dyn Widget>) -> Self {
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
            inner: Some(inner),
            child_id: None,
        }
    }
}

impl Widget for DragPreview {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Wrap the delegate widget in a Raised panel so the preview
        // stands out against the window. The inner widget is absorbed
        // into the arena on the first build; subsequent rebuilds cannot
        // reinsert it (we consumed the `Box<dyn Widget>`). The drag
        // preview is short-lived — one drag session — so a rebuild
        // during that window is not expected in practice; if one did
        // fire, the arena would have already destroyed the old child
        // subtree, so `self.child_id` is stale and we must NOT return
        // it. Returning an empty children vec leaves the widget visible
        // with its own `size_that_fits` but no content — the calling
        // drag session is dismissed shortly anyway.
        let Some(inner) = self.inner.take() else {
            self.child_id = None;
            return Vec::new();
        };
        let inner_id = ctx.add_boxed(inner);
        let panel_id = ctx.add(
            Panel::new()
                .background(SurfaceRole::Raised)
                .corner_radius(6.0)
                .child_id(inner_id),
        );
        self.child_id = Some(panel_id);
        vec![panel_id]
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(self.width, self.height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
