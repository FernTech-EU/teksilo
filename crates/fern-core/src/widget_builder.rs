//! WidgetBuilder trait — blanket-implemented for all Widget types.
//!
//! Provides attached event handler methods and framework-level properties.
//! Each method wraps the widget in a `WidgetWithHandlers<W>` that stores
//! the handlers and metadata alongside the widget. When the widget is
//! inserted into the arena, the handler set is extracted and applied to
//! the `WidgetNode`.

use fern_canvas::Point;

use crate::event::{EventResponse, WidgetEvent};
use crate::event_handlers::EventHandlers;
use crate::gesture::GestureEvent;
use crate::widget::{CursorIcon, EventContext, Widget};

// ---------------------------------------------------------------------------
// HandlerSet — temporary storage before arena insertion
// ---------------------------------------------------------------------------

/// Temporary storage for handlers and metadata accumulated via builder
/// methods. Transferred to the `WidgetNode` during arena insertion.
/// Type alias for a context menu content factory.
pub type ContextMenuFactory = Box<dyn Fn() -> Box<dyn Widget>>;

pub struct HandlerSet {
    pub(crate) handlers: EventHandlers,
    pub(crate) focusable: Option<bool>,
    pub(crate) tab_index: Option<i32>,
    pub(crate) cursor: Option<CursorIcon>,
    pub(crate) clips_children: Option<bool>,
    pub(crate) context_menu_factory: Option<ContextMenuFactory>,
}

impl HandlerSet {
    /// Create an empty handler set for use in `BuildContext::apply_self_handlers()`.
    pub fn new() -> Self {
        Self {
            handlers: EventHandlers::new(),
            focusable: None,
            tab_index: None,
            cursor: None,
            clips_children: None,
            context_menu_factory: None,
        }
    }

    // -- Builder methods (mirror WidgetWithHandlers) --

    /// Set the on_tap handler.
    pub fn on_tap(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handlers.on_tap = Some(Box::new(f));
        self
    }

    /// Set the on_double_tap handler.
    pub fn on_double_tap(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handlers.on_double_tap = Some(Box::new(f));
        self
    }

    /// Set the on_long_press handler.
    pub fn on_long_press(
        mut self,
        f: impl FnMut(Point, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.on_long_press = Some(Box::new(f));
        self
    }

    /// Set the on_hover handler.
    pub fn on_hover(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handlers.on_hover = Some(Box::new(f));
        self
    }

    /// Set the on_key handler.
    pub fn on_key(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_key = Some(Box::new(f));
        self
    }

    /// Set the on_drag handler (gesture-based drag).
    pub fn on_drag(
        mut self,
        f: impl FnMut(crate::gesture::GestureEvent, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.on_drag = Some(Box::new(f));
        self
    }

    /// Set the on_focus handler.
    pub fn on_focus(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handlers.on_focus = Some(Box::new(f));
        self
    }

    /// Set the on_pointer_event handler (low-level escape hatch).
    pub fn on_pointer_event(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_pointer_event = Some(Box::new(f));
        self
    }

    /// Set the on_scroll handler.
    pub fn on_scroll(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_scroll = Some(Box::new(f));
        self
    }

    /// Set the on_access_action handler.
    pub fn on_access_action(
        mut self,
        f: impl FnMut(accesskit::Action, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_access_action = Some(Box::new(f));
        self
    }

    /// Set the focusable flag.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = Some(focusable);
        self
    }

    /// Set the cursor icon.
    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Set the clips_children flag.
    pub fn clips_children(mut self, clips: bool) -> Self {
        self.clips_children = Some(clips);
        self
    }

    /// Set a context menu factory. The factory is invoked on right-click
    /// to produce the overlay content widget (typically a `MenuList`).
    pub fn context_menu(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.context_menu_factory = Some(Box::new(factory));
        self
    }

    /// Set the drag hover handler. Called when a drag payload hovers over this widget.
    /// Return `DropFeedback` to indicate acceptance and visual feedback.
    pub fn on_drag_hover(
        mut self,
        f: impl FnMut(
            &crate::drag_payload::DragPayload,
            fern_canvas::Point,
            &mut EventContext,
        ) -> crate::drag_state::DropFeedback
        + 'static,
    ) -> Self {
        self.handlers.on_drag_hover = Some(Box::new(f));
        self
    }

    /// Set the drop handler. Called when a payload is dropped on this widget.
    /// Return `true` if the drop was accepted.
    pub fn on_drop(
        mut self,
        f: impl FnMut(crate::drag_payload::DragPayload, fern_canvas::Point, &mut EventContext) -> bool
        + 'static,
    ) -> Self {
        self.handlers.on_drop = Some(Box::new(f));
        self
    }
}

impl Default for HandlerSet {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HandlerSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerSet")
            .field("handlers", &self.handlers)
            .field("focusable", &self.focusable)
            .field("tab_index", &self.tab_index)
            .field("cursor", &self.cursor)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// WidgetWithHandlers<W> — wrapper storing widget + accumulated handlers
// ---------------------------------------------------------------------------

/// A widget wrapped with attached event handlers and framework metadata.
/// Created by calling builder methods from `WidgetBuilder` on any widget.
pub struct WidgetWithHandlers<W: Widget> {
    pub(crate) widget: W,
    pub(crate) handler_set: HandlerSet,
}

impl<W: Widget> WidgetWithHandlers<W> {
    fn new(widget: W) -> Self {
        Self {
            widget,
            handler_set: HandlerSet::new(),
        }
    }

    /// Take the handler set out, leaving defaults.
    #[allow(dead_code)] // V2 API: used during widget insertion to extract handlers
    pub(crate) fn take_handler_set(&mut self) -> HandlerSet {
        std::mem::replace(&mut self.handler_set, HandlerSet::new())
    }

    // -- Gesture handlers --

    pub fn on_tap(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_tap = Some(Box::new(f));
        self
    }

    pub fn on_double_tap(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_double_tap = Some(Box::new(f));
        self
    }

    pub fn on_long_press(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_long_press = Some(Box::new(f));
        self
    }

    pub fn on_drag(mut self, f: impl FnMut(GestureEvent, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_drag = Some(Box::new(f));
        self
    }

    // -- Focus and keyboard --

    pub fn on_focus(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_focus = Some(Box::new(f));
        self
    }

    pub fn on_key(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_key = Some(Box::new(f));
        self
    }

    pub fn focusable(mut self, focusable: bool) -> Self {
        self.handler_set.focusable = Some(focusable);
        self
    }

    pub fn tab_index(mut self, index: i32) -> Self {
        self.handler_set.tab_index = Some(index);
        self
    }

    // -- Pointer (low-level escape hatch) --

    pub fn on_pointer_event(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_pointer_event = Some(Box::new(f));
        self
    }

    pub fn on_hover(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_hover = Some(Box::new(f));
        self
    }

    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.handler_set.cursor = Some(cursor);
        self
    }

    // -- Scroll --

    pub fn on_scroll(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_scroll = Some(Box::new(f));
        self
    }

    // -- Accessibility actions --

    pub fn on_access_action(
        mut self,
        f: impl FnMut(accesskit::Action, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_access_action = Some(Box::new(f));
        self
    }

    // -- Framework-level properties --

    pub fn clips_children(mut self, clips: bool) -> Self {
        self.handler_set.clips_children = Some(clips);
        self
    }

    pub fn context_menu(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.handler_set.context_menu_factory = Some(Box::new(factory));
        self
    }

    /// Set the drag hover handler. Called when a drag payload hovers over this widget.
    pub fn on_drag_hover(
        mut self,
        f: impl FnMut(
            &crate::drag_payload::DragPayload,
            fern_canvas::Point,
            &mut EventContext,
        ) -> crate::drag_state::DropFeedback
        + 'static,
    ) -> Self {
        self.handler_set.handlers.on_drag_hover = Some(Box::new(f));
        self
    }

    /// Set the drop handler. Called when a payload is dropped on this widget.
    pub fn on_drop(
        mut self,
        f: impl FnMut(crate::drag_payload::DragPayload, fern_canvas::Point, &mut EventContext) -> bool
        + 'static,
    ) -> Self {
        self.handler_set.handlers.on_drop = Some(Box::new(f));
        self
    }
}

// Delegate all Widget trait methods to the inner widget.
impl<W: Widget> std::fmt::Debug for WidgetWithHandlers<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetWithHandlers")
            .field("widget", &self.widget)
            .field("handler_set", &self.handler_set)
            .finish()
    }
}

impl<W: Widget + 'static> Widget for WidgetWithHandlers<W> {
    fn build(
        &mut self,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        self.widget.build(ctx)
    }

    fn size_that_fits(
        &self,
        proposal: fern_canvas::SizeProposal,
        ctx: &crate::widget::LayoutContext,
    ) -> fern_canvas::Size {
        self.widget.size_that_fits(proposal, ctx)
    }

    fn place_children(
        &self,
        bounds: fern_canvas::Rect,
        proposal: fern_canvas::SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        ctx: &crate::widget::LayoutContext,
    ) {
        self.widget.place_children(bounds, proposal, children, ctx)
    }

    fn paint(
        &self,
        bounds: fern_canvas::Rect,
        canvas: &mut fern_canvas::Canvas,
        ctx: &crate::widget::PaintContext,
    ) {
        self.widget.paint(bounds, canvas, ctx)
    }

    fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
        self.widget.accessibility(builder)
    }

    fn children(&self) -> Vec<crate::widget_id::WidgetId> {
        self.widget.children()
    }

    fn is_spacer(&self) -> bool {
        self.widget.is_spacer()
    }

    fn clips_children(&self) -> bool {
        self.handler_set
            .clips_children
            .unwrap_or_else(|| self.widget.clips_children())
    }

    fn take_handler_set(&mut self) -> Option<HandlerSet> {
        Some(self.take_handler_set())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::WidgetPlacement;
    use crate::widget_id::WidgetId;
    use crate::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct CompositeLeaf {
        child_id: Option<WidgetId>,
    }

    impl CompositeLeaf {
        fn new() -> Self {
            Self { child_id: None }
        }
    }

    impl Widget for CompositeLeaf {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let child = ctx.add(crate::test_widgets::FillWidget::new());
            self.child_id = Some(child);
            vec![child]
        }

        fn size_that_fits(
            &self,
            proposal: fern_canvas::SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> fern_canvas::Size {
            proposal.resolve(120.0, 40.0)
        }

        fn place_children(
            &self,
            bounds: fern_canvas::Rect,
            _proposal: fern_canvas::SizeProposal,
            children: &mut [WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.child_id.into_iter().collect()
        }
    }

    #[test]
    fn wrapped_composite_widget_still_builds_children() {
        let mut tree = WidgetTree::new();
        let root = tree.add(CompositeLeaf::new().on_tap(|_ctx| {}));
        tree.layout(fern_canvas::SizeProposal::exact(200.0, 100.0));

        assert_eq!(tree.children(root).len(), 1);
    }
}

// ---------------------------------------------------------------------------
// WidgetBuilder trait — the entry point
// ---------------------------------------------------------------------------

/// Blanket trait providing attached handler methods for all Widget types.
/// The first builder method call wraps the widget in `WidgetWithHandlers`.
pub trait WidgetBuilder: Widget + Sized + 'static {
    fn on_tap(self, f: impl FnMut(&mut EventContext) + 'static) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_tap(f)
    }

    fn on_double_tap(self, f: impl FnMut(&mut EventContext) + 'static) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_double_tap(f)
    }

    fn on_long_press(
        self,
        f: impl FnMut(Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_long_press(f)
    }

    fn on_drag(
        self,
        f: impl FnMut(GestureEvent, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag(f)
    }

    fn on_focus(
        self,
        f: impl FnMut(bool, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_focus(f)
    }

    fn on_key(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_key(f)
    }

    fn on_pointer_event(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_pointer_event(f)
    }

    fn on_hover(
        self,
        f: impl FnMut(bool, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_hover(f)
    }

    fn on_scroll(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_scroll(f)
    }

    fn on_access_action(
        self,
        f: impl FnMut(accesskit::Action, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_access_action(f)
    }

    fn focusable(self, focusable: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).focusable(focusable)
    }

    fn tab_index(self, index: i32) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).tab_index(index)
    }

    fn cursor(self, cursor: CursorIcon) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).cursor(cursor)
    }

    fn clips_children_on(self, clips: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).clips_children(clips)
    }

    fn context_menu(
        self,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).context_menu(factory)
    }

    fn on_drag_hover(
        self,
        f: impl FnMut(
            &crate::drag_payload::DragPayload,
            fern_canvas::Point,
            &mut EventContext,
        ) -> crate::drag_state::DropFeedback
        + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag_hover(f)
    }

    fn on_drop(
        self,
        f: impl FnMut(crate::drag_payload::DragPayload, fern_canvas::Point, &mut EventContext) -> bool
        + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drop(f)
    }
}

// Blanket implementation for all Widget types.
impl<W: Widget + Sized + 'static> WidgetBuilder for W {}
