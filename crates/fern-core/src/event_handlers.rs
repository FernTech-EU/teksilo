//! Attached event handlers for V2 widgets.
//!
//! `EventHandlers` stores optional closures for each handler type.
//! These are stored on the `WidgetNode` in the arena and dispatched
//! by the framework during event passes.

use fern_canvas::Point;

use crate::drag_payload::DragPayload;
use crate::drag_state::DropFeedback;
use crate::event::{EventResponse, WidgetEvent};
use crate::gesture::{GestureArena, GestureEvent};
use crate::widget::EventContext;

/// Event handlers attached to a widget node. Each field is an optional
/// closure dispatched by the framework during the event pass.
#[allow(clippy::type_complexity)]
pub(crate) struct EventHandlers {
    pub on_tap: Option<Box<dyn FnMut(&mut EventContext)>>,
    pub on_double_tap: Option<Box<dyn FnMut(&mut EventContext)>>,
    pub on_long_press: Option<Box<dyn FnMut(Point, &mut EventContext)>>,
    pub on_drag: Option<Box<dyn FnMut(GestureEvent, &mut EventContext)>>,
    pub on_hover: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    pub on_key: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    pub on_focus: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    pub on_pointer_event: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    pub on_scroll: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    pub on_access_action:
        Option<Box<dyn FnMut(accesskit::Action, &mut EventContext) -> EventResponse>>,
    // --- Drag and Drop handlers ---
    /// Called when a compatible drag payload hovers over this widget.
    /// Returns `DropFeedback` to indicate acceptance and visual feedback.
    pub on_drag_hover:
        Option<Box<dyn FnMut(&DragPayload, Point, &mut EventContext) -> DropFeedback>>,
    /// Called when a payload is dropped on this widget.
    /// Returns `true` if the drop was accepted.
    pub on_drop: Option<Box<dyn FnMut(DragPayload, Point, &mut EventContext) -> bool>>,

    #[allow(dead_code)] // V2 API: gesture arena for attached gesture recognizers
    pub gesture_arena: Option<GestureArena>,
}

impl EventHandlers {
    pub fn new() -> Self {
        Self {
            on_tap: None,
            on_double_tap: None,
            on_long_press: None,
            on_drag: None,
            on_hover: None,
            on_key: None,
            on_focus: None,
            on_pointer_event: None,
            on_scroll: None,
            on_access_action: None,
            on_drag_hover: None,
            on_drop: None,
            gesture_arena: None,
        }
    }

    /// Whether any handler is attached.
    #[allow(dead_code)] // V2 API: used for fast-path event dispatch skipping
    pub fn has_any(&self) -> bool {
        self.on_tap.is_some()
            || self.on_double_tap.is_some()
            || self.on_long_press.is_some()
            || self.on_drag.is_some()
            || self.on_hover.is_some()
            || self.on_key.is_some()
            || self.on_focus.is_some()
            || self.on_pointer_event.is_some()
            || self.on_scroll.is_some()
            || self.on_access_action.is_some()
            || self.on_drag_hover.is_some()
            || self.on_drop.is_some()
    }

    pub fn merge(self, other: EventHandlers) -> EventHandlers {
        EventHandlers {
            on_tap: merge_void_handler(self.on_tap, other.on_tap),
            on_double_tap: merge_void_handler(self.on_double_tap, other.on_double_tap),
            on_long_press: merge_point_handler(self.on_long_press, other.on_long_press),
            on_drag: merge_gesture_handler(self.on_drag, other.on_drag),
            on_hover: merge_hover_handler(self.on_hover, other.on_hover),
            on_key: merge_event_handler(self.on_key, other.on_key),
            on_focus: merge_focus_handler(self.on_focus, other.on_focus),
            on_pointer_event: merge_event_handler(self.on_pointer_event, other.on_pointer_event),
            on_scroll: merge_event_handler(self.on_scroll, other.on_scroll),
            on_access_action: merge_access_handler(self.on_access_action, other.on_access_action),
            on_drag_hover: other.on_drag_hover.or(self.on_drag_hover),
            on_drop: other.on_drop.or(self.on_drop),
            gesture_arena: other.gesture_arena.or(self.gesture_arena),
        }
    }
}

fn merge_void_handler(
    existing: Option<Box<dyn FnMut(&mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(&mut EventContext)>>,
) -> Option<Box<dyn FnMut(&mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |ctx| {
            existing(ctx);
            incoming(ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_point_handler(
    existing: Option<Box<dyn FnMut(Point, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(Point, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(Point, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |point, ctx| {
            existing(point, ctx);
            incoming(point, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_gesture_handler(
    existing: Option<Box<dyn FnMut(GestureEvent, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(GestureEvent, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(GestureEvent, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |event, ctx| {
            existing(event.clone(), ctx);
            incoming(event, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_hover_handler(
    existing: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(bool, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |entered, ctx| {
            existing(entered, ctx);
            incoming(entered, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_focus_handler(
    existing: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(bool, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |gained, ctx| {
            existing(gained, ctx);
            incoming(gained, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_event_handler(
    existing: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    incoming: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
) -> Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |event, ctx| {
            let first = existing(event, ctx);
            let second = incoming(event, ctx);
            if first == EventResponse::Handled || second == EventResponse::Handled {
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            }
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_access_handler(
    existing: Option<Box<dyn FnMut(accesskit::Action, &mut EventContext) -> EventResponse>>,
    incoming: Option<Box<dyn FnMut(accesskit::Action, &mut EventContext) -> EventResponse>>,
) -> Option<Box<dyn FnMut(accesskit::Action, &mut EventContext) -> EventResponse>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |action, ctx| {
            let first = existing(action, ctx);
            let second = incoming(action, ctx);
            if first == EventResponse::Handled || second == EventResponse::Handled {
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            }
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

impl Default for EventHandlers {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EventHandlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventHandlers")
            .field("on_tap", &self.on_tap.is_some())
            .field("on_double_tap", &self.on_double_tap.is_some())
            .field("on_long_press", &self.on_long_press.is_some())
            .field("on_drag", &self.on_drag.is_some())
            .field("on_hover", &self.on_hover.is_some())
            .field("on_key", &self.on_key.is_some())
            .field("on_focus", &self.on_focus.is_some())
            .field("on_pointer_event", &self.on_pointer_event.is_some())
            .field("on_scroll", &self.on_scroll.is_some())
            .field("on_access_action", &self.on_access_action.is_some())
            .field("on_drag_hover", &self.on_drag_hover.is_some())
            .field("on_drop", &self.on_drop.is_some())
            .finish()
    }
}
