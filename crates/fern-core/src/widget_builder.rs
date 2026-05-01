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
use crate::gesture::{DragPhase, PinchPhase, SwipeDirection};
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
    /// User-bound signal that the framework writes whenever the
    /// focused widget is a strict descendant of this node. See
    /// [`HandlerSet::focus_within`].
    pub(crate) focus_within: Option<crate::signal::Signal<bool>>,
    /// User-bound signal that the framework writes whenever the
    /// hovered widget is a strict descendant of this node. See
    /// [`HandlerSet::hover_within`].
    pub(crate) hover_within: Option<crate::signal::Signal<bool>>,
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
            focus_within: None,
            hover_within: None,
        }
    }

    // -- Builder methods (mirror WidgetWithHandlers) --

    /// Set the on_tap handler. The closure receives the tap position
    /// (in widget-local coordinates inside the widget's bounds).
    pub fn on_tap(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handlers.on_tap = Some(Box::new(f));
        self
    }

    /// Set the on_double_tap handler.
    pub fn on_double_tap(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handlers.on_double_tap = Some(Box::new(f));
        self
    }

    /// Set the on_triple_tap handler — fires on the third click within the
    /// recognizer's window (same 300 ms / 10 px defaults as double tap).
    /// Runs independently of `on_double_tap` via cooperative gesture
    /// recognizers (`GestureRecognizer::resets_on_peer_recognition`).
    pub fn on_triple_tap(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handlers.on_triple_tap = Some(Box::new(f));
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

    /// Set the strict-ancestor key preview handler. Fires on every
    /// ancestor of the focused widget (root → parent-of-target)
    /// before the focused widget's `on_key` runs. Return
    /// `EventResponse::Handled` to consume the event.
    pub fn on_key_preview(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_key_preview = Some(Box::new(f));
        self
    }

    /// Set the on_drag handler (gesture-based drag). The closure receives
    /// a [`DragPhase`] per architecture §28.3 — `Started`, then zero or
    /// more `Moved`, then `Ended`.
    pub fn on_drag(mut self, f: impl FnMut(DragPhase, &mut EventContext) + 'static) -> Self {
        self.handlers.on_drag = Some(Box::new(f));
        self
    }

    /// Set the on_swipe handler. Fires once per swipe with the direction
    /// and velocity (pixels/second).
    pub fn on_swipe(
        mut self,
        f: impl FnMut(SwipeDirection, f32, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.on_swipe = Some(Box::new(f));
        self
    }

    /// Set the on_pinch handler. On desktop the phases are produced from
    /// OS trackpad gestures (winit `TouchpadMagnify` / `RotationGesture`).
    pub fn on_pinch(mut self, f: impl FnMut(PinchPhase, &mut EventContext) + 'static) -> Self {
        self.handlers.on_pinch = Some(Box::new(f));
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

    /// Set the full AccessKit action-request handler. Receives the
    /// action, target NodeId (may be a synthetic widget-emitted
    /// child), and optional `ActionData` payload (e.g.
    /// `SetTextSelection(TextSelection)` or `Value(Box<str>)`).
    /// When this slot is set it's called INSTEAD of
    /// `on_access_action` for the same event.
    pub fn on_access_action_request(
        mut self,
        f: impl FnMut(
                accesskit::Action,
                accesskit::NodeId,
                Option<accesskit::ActionData>,
                &mut EventContext,
            ) -> EventResponse
            + 'static,
    ) -> Self {
        self.handlers.on_access_action_request = Some(Box::new(f));
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

    /// Bind a user-owned [`Signal<bool>`] that the framework will set
    /// to `true` whenever the focused widget is a *strict descendant*
    /// of this node, and `false` otherwise. Useful for unified focus
    /// halos around composite widgets (a chat composer that highlights
    /// when its `RichTextEditor` or "Send" button is focused, a
    /// `Panel` wrapping a `SpinBox`, etc).
    ///
    /// Strict-ancestors only — a widget that *is* itself focused does
    /// not also see its own `focus_within` signal flipped to `true`.
    /// Combine with `on_focus` if you want both behaviours.
    pub fn focus_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.focus_within = Some(signal);
        self
    }

    /// Bind a user-owned [`Signal<bool>`] that the framework will set
    /// to `true` whenever the hovered widget is a *strict descendant*
    /// of this node. Symmetric to [`focus_within`](Self::focus_within).
    pub fn hover_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.hover_within = Some(signal);
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

    /// Set the drag-leave handler. Fires when a drag that was over this
    /// widget moves to another target, completes (drop on any target), or
    /// is cancelled. Widgets that stash transient feedback state in
    /// `on_drag_hover` must clear it here.
    pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handlers.on_drag_leave = Some(Box::new(f));
        self
    }

    /// Set the per-frame drag-tick handler. Fires once per frame while a
    /// drag is active and this widget is the current drop target. The
    /// closure receives the current pointer position in widget-local
    /// coordinates. Use for behaviours that must keep running even when
    /// the pointer is stationary — viewport-edge auto-scroll and
    /// spring-loaded folders.
    pub fn on_drag_tick(
        mut self,
        f: impl FnMut(fern_canvas::Point, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.on_drag_tick = Some(Box::new(f));
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

    pub fn on_tap(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_tap = Some(Box::new(f));
        self
    }

    pub fn on_double_tap(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_double_tap = Some(Box::new(f));
        self
    }

    pub fn on_triple_tap(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_triple_tap = Some(Box::new(f));
        self
    }

    pub fn on_long_press(mut self, f: impl FnMut(Point, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_long_press = Some(Box::new(f));
        self
    }

    pub fn on_drag(mut self, f: impl FnMut(DragPhase, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_drag = Some(Box::new(f));
        self
    }

    pub fn on_swipe(
        mut self,
        f: impl FnMut(SwipeDirection, f32, &mut EventContext) + 'static,
    ) -> Self {
        self.handler_set.handlers.on_swipe = Some(Box::new(f));
        self
    }

    pub fn on_pinch(mut self, f: impl FnMut(PinchPhase, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_pinch = Some(Box::new(f));
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

    /// Set the strict-ancestor key preview handler. See
    /// [`HandlerSet::on_key_preview`].
    pub fn on_key_preview(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_key_preview = Some(Box::new(f));
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

    pub fn on_access_action_request(
        mut self,
        f: impl FnMut(
                accesskit::Action,
                accesskit::NodeId,
                Option<accesskit::ActionData>,
                &mut EventContext,
            ) -> EventResponse
            + 'static,
    ) -> Self {
        self.handler_set.handlers.on_access_action_request = Some(Box::new(f));
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

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant has focus. See [`HandlerSet::focus_within`].
    pub fn focus_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.handler_set.focus_within = Some(signal);
        self
    }

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant is hovered. See [`HandlerSet::hover_within`].
    pub fn hover_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.handler_set.hover_within = Some(signal);
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

    /// Set the drag-leave handler. See [`HandlerSet::on_drag_leave`].
    pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_drag_leave = Some(Box::new(f));
        self
    }

    /// Set the per-frame drag-tick handler. See [`HandlerSet::on_drag_tick`].
    pub fn on_drag_tick(
        mut self,
        f: impl FnMut(fern_canvas::Point, &mut EventContext) + 'static,
    ) -> Self {
        self.handler_set.handlers.on_drag_tick = Some(Box::new(f));
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

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        self.widget.as_any()
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
    fn external_handlers_survive_rebuild() {
        // Regression check: handlers attached externally via the
        // `WidgetBuilder` builder (e.g. `MyCompositeWidget::new().on_tap(...)`)
        // must continue to fire after the widget rebuilds in place.
        // My handler-clearing fix in `rebuild_single_widget` wiped
        // `node.handlers` to stop accumulation of `apply_self_handlers`
        // calls across rebuilds — but the extracted-once-at-insertion
        // HandlerSet is gone by rebuild time and would be lost.
        use std::cell::Cell;
        use std::rc::Rc;

        let tap_count = Rc::new(Cell::new(0_u32));
        let tc = tap_count.clone();

        let mut tree = WidgetTree::new();
        let id = tree.add(
            CompositeLeaf::new().on_tap(move |_pos, _ctx| {
                tc.set(tc.get() + 1);
            }),
        );
        tree.layout(fern_canvas::SizeProposal::exact(200.0, 100.0));

        // Trip a rebuild of the composite — its child gets torn down &
        // rebuilt; node.handlers gets cleared and reset.
        tree.arena_mark_needs_rebuild_for_testing(id);
        tree.layout(fern_canvas::SizeProposal::exact(200.0, 100.0));

        // Click through the composite; the externally-attached on_tap
        // must still be wired up.
        tree.click(id);
        assert_eq!(
            tap_count.get(),
            1,
            "externally-attached on_tap must survive a rebuild"
        );
    }

    #[test]
    fn wrapped_composite_widget_still_builds_children() {
        let mut tree = WidgetTree::new();
        let root = tree.add(CompositeLeaf::new().on_tap(|_pos, _ctx| {}));
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
    fn on_tap(
        self,
        f: impl FnMut(Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_tap(f)
    }

    fn on_double_tap(
        self,
        f: impl FnMut(Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_double_tap(f)
    }

    fn on_triple_tap(
        self,
        f: impl FnMut(Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_triple_tap(f)
    }

    fn on_long_press(
        self,
        f: impl FnMut(Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_long_press(f)
    }

    fn on_drag(
        self,
        f: impl FnMut(DragPhase, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag(f)
    }

    fn on_swipe(
        self,
        f: impl FnMut(SwipeDirection, f32, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_swipe(f)
    }

    fn on_pinch(
        self,
        f: impl FnMut(PinchPhase, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_pinch(f)
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

    /// Strict-ancestor key preview. See [`HandlerSet::on_key_preview`].
    fn on_key_preview(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_key_preview(f)
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

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant has focus. See [`HandlerSet::focus_within`].
    fn focus_within(self, signal: crate::signal::Signal<bool>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).focus_within(signal)
    }

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant is hovered. See [`HandlerSet::hover_within`].
    fn hover_within(self, signal: crate::signal::Signal<bool>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).hover_within(signal)
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

    fn on_drag_leave(
        self,
        f: impl FnMut(&mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag_leave(f)
    }

    fn on_drag_tick(
        self,
        f: impl FnMut(fern_canvas::Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag_tick(f)
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
