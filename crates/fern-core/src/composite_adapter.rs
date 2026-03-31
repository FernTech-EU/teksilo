//! Bridges CompositeWidget into the Widget trait so both are interchangeable
//! in the arena. The adapter calls `build()` to construct the subtree, then
//! delegates layout/paint to the root child. Events and accessibility are
//! handled by the composite itself.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::composite_widget::CompositeWidget;
use crate::event::{EventResponse, WidgetEvent};
use crate::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

/// A Widget wrapper around a CompositeWidget.
/// Stores the composite and, after build(), the root child WidgetId.
pub(crate) struct CompositeWidgetAdapter {
    composite: Box<dyn CompositeWidget>,
    /// The root widget ID returned by build(). None until build() is called.
    root_child: Option<WidgetId>,
    /// Whether build() has been called.
    built: bool,
}

impl CompositeWidgetAdapter {
    pub fn new(composite: Box<dyn CompositeWidget>) -> Self {
        Self {
            composite,
            root_child: None,
            built: false,
        }
    }

    /// Call build() to construct the subtree. Must be called with access to the tree.
    /// Returns the root child ID so the caller can set it up as a child.
    pub fn build(
        &mut self,
        ctx: &mut crate::composite_widget::BuildContext,
    ) -> WidgetId {
        let root = self.composite.build(ctx);
        self.root_child = Some(root);
        self.built = true;
        root
    }

    pub fn root_child(&self) -> Option<WidgetId> {
        self.root_child
    }

    pub fn is_built(&self) -> bool {
        self.built
    }

    /// Re-run build() to reconstruct the subtree with fresh environment.
    /// Returns the old root child (for destruction) and the new root child.
    pub fn rebuild(
        &mut self,
        ctx: &mut crate::composite_widget::BuildContext,
    ) -> (Option<WidgetId>, WidgetId) {
        let old_root = self.root_child;
        let new_root = self.composite.build(ctx);
        self.root_child = Some(new_root);
        (old_root, new_root)
    }
}

impl std::fmt::Debug for CompositeWidgetAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeWidgetAdapter")
            .field("composite", &self.composite)
            .field("root_child", &self.root_child)
            .finish()
    }
}

impl Widget for CompositeWidgetAdapter {
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        // Delegate to the root child's size_that_fits via the arena.
        // This is how the composite's size comes from its composed subtree.
        if let Some(root_id) = self.root_child {
            if let Some(size) = ctx.child_size(root_id, proposal) {
                return size;
            }
        }
        proposal.resolve(0.0, 0.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // The single child (root of the built subtree) fills our bounds.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {
        // Composite itself has no visual — its children paint themselves.
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        self.composite.event(event, ctx)
    }

    fn preview_event(
        &mut self,
        event: &WidgetEvent,
        ctx: &mut EventContext,
    ) -> EventResponse {
        // Composites don't have a separate preview_event — they use event()
        // in the bubble pass. The preview pass is for the Widget trait.
        let _ = (event, ctx);
        EventResponse::Ignored
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        self.composite.accessibility(builder);
    }

    fn is_focusable(&self) -> bool {
        self.composite.is_focusable()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child.into_iter().collect()
    }

    fn is_composite(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
