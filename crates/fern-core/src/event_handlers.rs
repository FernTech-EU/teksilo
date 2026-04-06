//! Attached event handlers for V2 widgets.
//!
//! `EventHandlers` stores optional closures for each handler type.
//! These are stored on the `WidgetNode` in the arena and dispatched
//! by the framework during event passes.

use fern_canvas::Point;

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
            .finish()
    }
}
