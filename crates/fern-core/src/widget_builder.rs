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
use crate::signal::Prop;
use crate::widget::{CursorIcon, EventContext, Widget};

// ---------------------------------------------------------------------------
// HandlerSet — temporary storage before arena insertion
// ---------------------------------------------------------------------------

/// Temporary storage for handlers and metadata accumulated via builder
/// methods. Transferred to the `WidgetNode` during arena insertion.
pub struct HandlerSet {
    pub(crate) handlers: EventHandlers,
    pub(crate) focusable: Option<bool>,
    pub(crate) tab_index: Option<i32>,
    pub(crate) cursor: Option<CursorIcon>,
    pub(crate) visible_when: Option<Prop<bool>>,
    pub(crate) enabled_when: Option<Prop<bool>>,
    pub(crate) tooltip_text: Option<String>,
    pub(crate) clips_children: Option<bool>,
}

impl HandlerSet {
    fn new() -> Self {
        Self {
            handlers: EventHandlers::new(),
            focusable: None,
            tab_index: None,
            cursor: None,
            visible_when: None,
            enabled_when: None,
            tooltip_text: None,
            clips_children: None,
        }
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

    pub fn visible_when(mut self, signal: impl Into<Prop<bool>>) -> Self {
        self.handler_set.visible_when = Some(signal.into());
        self
    }

    pub fn enabled_when(mut self, signal: impl Into<Prop<bool>>) -> Self {
        self.handler_set.enabled_when = Some(signal.into());
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.handler_set.tooltip_text = Some(text.into());
        self
    }

    pub fn clips_children(mut self, clips: bool) -> Self {
        self.handler_set.clips_children = Some(clips);
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

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        self.widget.event(event, ctx)
    }

    fn preview_event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        self.widget.preview_event(event, ctx)
    }

    fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
        self.widget.accessibility(builder)
    }

    fn is_focusable(&self) -> bool {
        self.handler_set
            .focusable
            .unwrap_or_else(|| self.widget.is_focusable())
    }

    fn tab_index(&self) -> Option<i32> {
        self.handler_set
            .tab_index
            .or_else(|| self.widget.tab_index())
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

    fn is_composite(&self) -> bool {
        self.widget.is_composite()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn register_bindings(
        &self,
        id: crate::widget_id::WidgetId,
        registry: &crate::state::BindingRegistry,
    ) {
        self.widget.register_bindings(id, registry)
    }

    fn animated_states(&self) -> Vec<crate::state::State<f32>> {
        self.widget.animated_states()
    }

    fn animated_signals(&self) -> Vec<crate::signal::Signal<f32>> {
        self.widget.animated_signals()
    }

    fn take_pending_children(&mut self) -> Vec<crate::widget::PendingChild> {
        self.widget.take_pending_children()
    }

    fn set_resolved_children(&mut self, ids: Vec<crate::widget_id::WidgetId>) {
        self.widget.set_resolved_children(ids)
    }

    fn take_visible_when(&mut self) -> Option<crate::state::Reactive<bool>> {
        // Prefer V2 handler_set, fall back to V1
        if let Some(prop) = self.handler_set.visible_when.take() {
            Some(prop.into())
        } else {
            self.widget.take_visible_when()
        }
    }

    fn take_enabled_when(&mut self) -> Option<crate::state::Reactive<bool>> {
        if let Some(prop) = self.handler_set.enabled_when.take() {
            Some(prop.into())
        } else {
            self.widget.take_enabled_when()
        }
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

    fn v2_focusable(self, focusable: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).focusable(focusable)
    }

    fn v2_tab_index(self, index: i32) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).tab_index(index)
    }

    fn v2_cursor(self, cursor: CursorIcon) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).cursor(cursor)
    }

    fn v2_visible_when(self, signal: impl Into<Prop<bool>>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).visible_when(signal)
    }

    fn v2_enabled_when(self, signal: impl Into<Prop<bool>>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).enabled_when(signal)
    }

    fn v2_tooltip(self, text: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).tooltip(text)
    }

    fn v2_clips_children(self, clips: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).clips_children(clips)
    }
}

// Blanket implementation for all Widget types.
impl<W: Widget + Sized + 'static> WidgetBuilder for W {}
