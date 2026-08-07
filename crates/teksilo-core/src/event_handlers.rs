// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Attached event handlers for V2 widgets.
//!
//! `EventHandlers` stores optional closures for each handler type.
//! These are stored on the `WidgetNode` in the arena and dispatched
//! by the framework during event passes.
//!
//! The four click-style handlers (`on_tap` / `on_double_tap` /
//! `on_triple_tap` / `on_long_press`) share a [`TapHandler`] alias —
//! `Box<dyn FnMut(&TapEvent, &mut EventContext)>` — and are
//! complemented by four `*_buttons: Option<ButtonMask>` fields that
//! customise the auto-wired recognizers' acceptance sets. Default
//! is [`ButtonMask::PRIMARY`] for every recognizer; widen via the
//! matching `accept_*_buttons(...)` builder method on `HandlerSet`,
//! `WidgetBuilder`, or `WidgetWithHandlers`.

use teksilo_canvas::Point;

use crate::drag_payload::{DragPayload, DropOutcome};
use crate::drag_state::DropFeedback;
use crate::event::{ButtonMask, EventResponse, WidgetEvent};
use crate::gesture::{DragPhase, GestureArena, PinchPhase, SwipeDirection, TapEvent};
use crate::widget::EventContext;

/// Type alias for the four tap-family handler closures
/// (`on_tap` / `on_double_tap` / `on_triple_tap` / `on_long_press`).
/// They all share the same shape: take a borrowed `TapEvent` (carrying
/// position, button, and modifiers at the finalising event) plus the
/// usual `EventContext`.
pub(crate) type TapHandler = Box<dyn FnMut(&TapEvent, &mut EventContext)>;

/// Event handlers attached to a widget node. Each field is an optional
/// closure dispatched by the framework during the event pass.
#[allow(clippy::type_complexity)]
pub(crate) struct EventHandlers {
    pub on_tap: Option<TapHandler>,
    pub on_double_tap: Option<TapHandler>,
    pub on_triple_tap: Option<TapHandler>,
    pub on_long_press: Option<TapHandler>,
    /// Per-recognizer button-acceptance overrides for the auto-wired
    /// gesture arena. `None` → recognizer uses its default
    /// (`ButtonMask::PRIMARY`). Set via `accept_tap_buttons(...)` /
    /// `accept_double_tap_buttons(...)` / `accept_triple_tap_buttons(...)`
    /// / `accept_long_press_buttons(...)` on the widget builder or
    /// `HandlerSet`. Read by `ensure_gesture_arena` when constructing
    /// the arena.
    pub tap_buttons: Option<ButtonMask>,
    pub double_tap_buttons: Option<ButtonMask>,
    pub triple_tap_buttons: Option<ButtonMask>,
    pub long_press_buttons: Option<ButtonMask>,
    pub on_drag: Option<Box<dyn FnMut(DragPhase, &mut EventContext)>>,
    pub on_swipe: Option<Box<dyn FnMut(SwipeDirection, f32, &mut EventContext)>>,
    pub on_pinch: Option<Box<dyn FnMut(PinchPhase, &mut EventContext)>>,
    pub on_hover: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    pub on_key: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    /// Strict-ancestor-only preview pass for `KeyDown` / `KeyUp` /
    /// IME events. Fires on every ancestor of the focused widget,
    /// in root → parent-of-target order, *before* the focused
    /// widget's `on_key` runs. Returning `EventResponse::Handled`
    /// consumes the event and stops both the rest of the preview
    /// walk and the focused widget's `on_key`.
    ///
    /// Mirrors how `on_pointer_event` behaves on the pointer side.
    /// The focused widget itself does NOT see its own
    /// `on_key_preview` — set `on_key` on the target if you need a
    /// per-widget hook.
    pub on_key_preview: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    pub on_focus: Option<Box<dyn FnMut(bool, &mut EventContext)>>,
    /// Low-level escape hatch for the raw pointer stream, run *before* gesture
    /// recognition. Unlike most handlers it fires in the **preview pass** on
    /// every strict ancestor of the target (root → parent-of-target) for
    /// `PointerMove` / `PointerDown` / `PointerUp` and `Scroll`; an ancestor
    /// returning `EventResponse::Handled` consumes the event and stops it
    /// reaching the target. This is what lets a `Splitter` divider or a tab
    /// bar claim a drag/wheel before a descendant `ScrollArea` does.
    ///
    /// `PointerEnter` / `PointerLeave` are deliberately **excluded** from the
    /// preview pass — they are per-node hover transitions, so they only reach
    /// the hovered node itself (via its bubble pass) and can never be swallowed
    /// by an ancestor's `on_pointer_event`.
    pub on_pointer_event: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    pub on_scroll: Option<Box<dyn FnMut(&WidgetEvent, &mut EventContext) -> EventResponse>>,
    pub on_access_action:
        Option<Box<dyn FnMut(accesskit::Action, &mut EventContext) -> EventResponse>>,
    /// Full AccessKit action request handler, with the complete
    /// `ActionRequest` payload (`target_node` and `data`). Used by
    /// widgets that care about `SetTextSelection` / `SetValue` /
    /// `SetScrollOffset` — the bare `on_access_action` handler
    /// drops the payload. When this slot is set, it is called
    /// INSTEAD of `on_access_action` for the same event.
    #[allow(clippy::type_complexity)]
    pub on_access_action_request: Option<
        Box<
            dyn FnMut(
                accesskit::Action,
                accesskit::NodeId,
                Option<accesskit::ActionData>,
                &mut EventContext,
            ) -> EventResponse,
        >,
    >,
    // --- Drag and Drop handlers ---
    /// Called when a compatible drag payload hovers over this widget.
    /// Returns `DropFeedback` to indicate acceptance and visual feedback.
    pub on_drag_hover:
        Option<Box<dyn FnMut(&DragPayload, Point, &mut EventContext) -> DropFeedback>>,
    /// Called when a drag leaves this widget — either because the pointer
    /// moved to a different drop target, the drop completed (on this or
    /// another target), or the drag was cancelled (Escape, drop outside,
    /// or source widget destroyed mid-drag). Widgets that set transient
    /// feedback state in `on_drag_hover` (insertion lines, highlight
    /// rectangles) MUST clear it here; the framework does not touch
    /// widget-owned state.
    pub on_drag_leave: Option<Box<dyn FnMut(&mut EventContext)>>,
    /// Called once per frame during an active drag session, on whichever
    /// widget is currently the drop target, with the pointer position in
    /// widget-local coordinates. Used for per-frame behaviours that must
    /// keep running when the pointer is stationary — viewport-edge
    /// auto-scroll, spring-loaded folders, etc.
    pub on_drag_tick: Option<Box<dyn FnMut(Point, &mut EventContext)>>,
    /// Called when a payload is dropped on this widget.
    /// Returns `true` if the drop was accepted.
    pub on_drop: Option<Box<dyn FnMut(DragPayload, Point, &mut EventContext) -> bool>>,
    /// Called on the **source** widget when a drag it started ends, with the
    /// outcome (dropped in-app, exported to the OS as copy/move, or
    /// cancelled). The single completion hook for the drag's originator —
    /// used e.g. to remove the dragged item on an `OsMove`.
    pub on_drag_ended: Option<Box<dyn FnMut(DropOutcome, &mut EventContext)>>,

    pub gesture_arena: Option<GestureArena>,
}

impl EventHandlers {
    pub fn new() -> Self {
        Self {
            on_tap: None,
            on_double_tap: None,
            on_triple_tap: None,
            on_long_press: None,
            tap_buttons: None,
            double_tap_buttons: None,
            triple_tap_buttons: None,
            long_press_buttons: None,
            on_drag: None,
            on_swipe: None,
            on_pinch: None,
            on_hover: None,
            on_key: None,
            on_key_preview: None,
            on_focus: None,
            on_pointer_event: None,
            on_scroll: None,
            on_access_action: None,
            on_access_action_request: None,
            on_drag_hover: None,
            on_drag_leave: None,
            on_drag_tick: None,
            on_drop: None,
            on_drag_ended: None,
            gesture_arena: None,
        }
    }

    pub fn merge(self, other: EventHandlers) -> EventHandlers {
        EventHandlers {
            on_tap: merge_tap_handler(self.on_tap, other.on_tap),
            on_double_tap: merge_tap_handler(self.on_double_tap, other.on_double_tap),
            on_triple_tap: merge_tap_handler(self.on_triple_tap, other.on_triple_tap),
            on_long_press: merge_tap_handler(self.on_long_press, other.on_long_press),
            tap_buttons: other.tap_buttons.or(self.tap_buttons),
            double_tap_buttons: other.double_tap_buttons.or(self.double_tap_buttons),
            triple_tap_buttons: other.triple_tap_buttons.or(self.triple_tap_buttons),
            long_press_buttons: other.long_press_buttons.or(self.long_press_buttons),
            on_drag: merge_drag_handler(self.on_drag, other.on_drag),
            on_swipe: merge_swipe_handler(self.on_swipe, other.on_swipe),
            on_pinch: merge_pinch_handler(self.on_pinch, other.on_pinch),
            on_hover: merge_hover_handler(self.on_hover, other.on_hover),
            on_key: merge_event_handler(self.on_key, other.on_key),
            on_key_preview: merge_event_handler(self.on_key_preview, other.on_key_preview),
            on_focus: merge_focus_handler(self.on_focus, other.on_focus),
            on_pointer_event: merge_event_handler(self.on_pointer_event, other.on_pointer_event),
            on_scroll: merge_event_handler(self.on_scroll, other.on_scroll),
            on_access_action: merge_access_handler(self.on_access_action, other.on_access_action),
            on_access_action_request: other
                .on_access_action_request
                .or(self.on_access_action_request),
            on_drag_hover: other.on_drag_hover.or(self.on_drag_hover),
            on_drag_leave: merge_ctx_handler(self.on_drag_leave, other.on_drag_leave),
            on_drag_tick: merge_point_handler(self.on_drag_tick, other.on_drag_tick),
            on_drop: other.on_drop.or(self.on_drop),
            on_drag_ended: merge_outcome_handler(self.on_drag_ended, other.on_drag_ended),
            gesture_arena: other.gesture_arena.or(self.gesture_arena),
        }
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

fn merge_outcome_handler(
    existing: Option<Box<dyn FnMut(DropOutcome, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(DropOutcome, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(DropOutcome, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |outcome, ctx| {
            existing(outcome, ctx);
            incoming(outcome, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_tap_handler(
    existing: Option<TapHandler>,
    incoming: Option<TapHandler>,
) -> Option<TapHandler> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |event, ctx| {
            existing(event, ctx);
            incoming(event, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_ctx_handler(
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

fn merge_drag_handler(
    existing: Option<Box<dyn FnMut(DragPhase, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(DragPhase, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(DragPhase, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |phase, ctx| {
            existing(phase, ctx);
            incoming(phase, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_swipe_handler(
    existing: Option<Box<dyn FnMut(SwipeDirection, f32, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(SwipeDirection, f32, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(SwipeDirection, f32, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |dir, velocity, ctx| {
            existing(dir, velocity, ctx);
            incoming(dir, velocity, ctx);
        })),
        (Some(existing), None) => Some(existing),
        (None, Some(incoming)) => Some(incoming),
        (None, None) => None,
    }
}

fn merge_pinch_handler(
    existing: Option<Box<dyn FnMut(PinchPhase, &mut EventContext)>>,
    incoming: Option<Box<dyn FnMut(PinchPhase, &mut EventContext)>>,
) -> Option<Box<dyn FnMut(PinchPhase, &mut EventContext)>> {
    match (existing, incoming) {
        (Some(mut existing), Some(mut incoming)) => Some(Box::new(move |phase, ctx| {
            existing(phase, ctx);
            incoming(phase, ctx);
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
            .field("on_triple_tap", &self.on_triple_tap.is_some())
            .field("on_long_press", &self.on_long_press.is_some())
            .field("tap_buttons", &self.tap_buttons)
            .field("double_tap_buttons", &self.double_tap_buttons)
            .field("triple_tap_buttons", &self.triple_tap_buttons)
            .field("long_press_buttons", &self.long_press_buttons)
            .field("on_drag", &self.on_drag.is_some())
            .field("on_swipe", &self.on_swipe.is_some())
            .field("on_pinch", &self.on_pinch.is_some())
            .field("on_hover", &self.on_hover.is_some())
            .field("on_key", &self.on_key.is_some())
            .field("on_key_preview", &self.on_key_preview.is_some())
            .field("on_focus", &self.on_focus.is_some())
            .field("on_pointer_event", &self.on_pointer_event.is_some())
            .field("on_scroll", &self.on_scroll.is_some())
            .field("on_access_action", &self.on_access_action.is_some())
            .field(
                "on_access_action_request",
                &self.on_access_action_request.is_some(),
            )
            .field("on_drag_hover", &self.on_drag_hover.is_some())
            .field("on_drag_leave", &self.on_drag_leave.is_some())
            .field("on_drag_tick", &self.on_drag_tick.is_some())
            .field("on_drop", &self.on_drop.is_some())
            .finish()
    }
}
