//! Pointer and scroll dispatch for the rich text editor.
//!
//! Owns three handler entry points that the widget installs in
//! `build()`:
//!
//!  * [`handle_pointer_event`] — PointerDown (caret placement +
//!    drag-select start), PointerMove (drag-select extension and
//!    auto-scroll velocity computation), PointerUp (drag teardown).
//!    Returns `EventResponse::Ignored` for PointerDown so the
//!    gesture arena's `DoubleTapRecognizer` / `TripleTapRecognizer`
//!    also see the event.
//!  * [`handle_scroll`] — mouse wheel / trackpad translation into
//!    `scroll_x` / `scroll_y` signal updates.
//!  * [`handle_double_tap`] / [`handle_triple_tap`] — word and
//!    paragraph selection on successive clicks. The independent
//!    cooperative recognizers in `fern-core::gesture` guarantee that
//!    both fire in an escalating click sequence.

use fern_canvas::Point;
use fern_core::event::{EventResponse, PointerButton, ScrollDelta, WidgetEvent};
use fern_core::widget::EventContext;
use fern_text::text_document::{MoveMode, SelectionType};

use super::hit_test;
use super::state::{DragState, SharedState};
use super::sync_cursor_signals;

pub(super) fn handle_pointer_event(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    match event {
        WidgetEvent::PointerDown {
            position,
            button,
            modifiers,
        } => {
            if *button != PointerButton::Primary {
                // Secondary / middle are for the application's own
                // context menu; let them bubble.
                return EventResponse::Ignored;
            }
            let shift = modifiers.shift();
            let local = {
                let st = state.borrow();
                Point::new(
                    position.x - st.viewport_origin.x,
                    position.y - st.viewport_origin.y,
                )
            };
            let hit = {
                let st = state.borrow();
                hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
            };
            let Some(hit) = hit else {
                // Return Ignored so the gesture arena still sees the
                // event — a click that missed text still activates
                // the widget, and the double/triple tap recognizers
                // need every press to progress their state machines.
                return EventResponse::Ignored;
            };
            match &hit.region {
                fern_text::HitRegion::Link { href: _ }
                | fern_text::HitRegion::Image { name: _ } => {
                    // Link / image click: do not move the caret. The
                    // typed-command emission is deferred. Return
                    // Ignored so the arena still processes the event
                    // (TapRecognizer won't be installed when
                    // on_double_tap is wired, but the double/triple
                    // machines need to see this press).
                    ctx.request_frame();
                    return EventResponse::Ignored;
                }
                _ => {}
            }
            // Place the cursor. Shift+click extends the selection
            // from the existing anchor; plain click collapses it.
            {
                let mut st = state.borrow_mut();
                let mode = if shift {
                    MoveMode::KeepAnchor
                } else {
                    MoveMode::MoveAnchor
                };
                st.cursor.set_position(hit.position, mode);
                // A fresh press starts a drag-select session.
                // Stored velocity is 0 until PointerMove detects an
                // auto-scroll zone.
                st.drag_state = DragState::Selecting {
                    auto_scroll_v_per_s: 0.0,
                };
                st.preferred_x = None;
                // Click resets the Ctrl+A ladder so a follow-up
                // Ctrl+A starts fresh at level 1.
                st.select_all_level = 0;
                st.select_all_anchor_cell = None;
            }
            sync_cursor_signals(state);
            ctx.request_frame();
            // Return Ignored so the gesture arena (DoubleTap /
            // TripleTap) also sees this PointerDown. Returning
            // Handled here would consume the event and the arena
            // would never fire `on_double_tap` / `on_triple_tap`.
            EventResponse::Ignored
        }
        WidgetEvent::PointerMove { position } => {
            // Drag-select extension. The `drag_state` field tells us
            // whether a primary button is still held; if it isn't,
            // we ignore the move.
            let (is_dragging, viewport_height) = {
                let st = state.borrow();
                let dragging = matches!(st.drag_state, DragState::Selecting { .. });
                (dragging, st.viewport_height)
            };
            if !is_dragging {
                return EventResponse::Ignored;
            }
            let local = {
                let st = state.borrow();
                Point::new(
                    position.x - st.viewport_origin.x,
                    position.y - st.viewport_origin.y,
                )
            };
            // Clamp y into [2.0, viewport_height - 2.0] before
            // hit-testing so a drag that leaves the viewport still
            // resolves to a valid position on the edge line
            // (matches godot rich_text_edit.rs:1830).
            let clamped = Point::new(
                local.x,
                local.y.clamp(2.0, (viewport_height - 2.0).max(2.0)),
            );
            let hit = {
                let st = state.borrow();
                hit_test::hit_test_at(&st.engine, clamped, 0.0, 0.0)
            };
            if let Some(hit) = hit {
                let st = state.borrow();
                st.cursor.set_position(hit.position, MoveMode::KeepAnchor);
                drop(st);
                sync_cursor_signals(state);
            }
            // Compute auto-scroll velocity for the frame loop. The
            // 20 px margin and 60 px/frame max (normalized to
            // 60 * 60 = 3600 px/s so delta scaling matches the
            // reference without depending on refresh rate) come
            // from godot rich_text_edit.rs:1812-1845.
            {
                let mut st = state.borrow_mut();
                let v = if local.y < 20.0 {
                    let intensity = ((20.0 - local.y) / 20.0).clamp(0.0, 1.0);
                    -60.0 * 60.0 * intensity
                } else if local.y > viewport_height - 20.0 {
                    let intensity =
                        ((local.y - (viewport_height - 20.0)) / 20.0).clamp(0.0, 1.0);
                    60.0 * 60.0 * intensity
                } else {
                    0.0
                };
                st.drag_state = DragState::Selecting {
                    auto_scroll_v_per_s: v,
                };
                if v != 0.0 {
                    // Drag near an edge keeps the frame loop pumping
                    // so auto-scroll continues without needing the
                    // user to wiggle the mouse.
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                }
            }
            ctx.request_frame();
            EventResponse::Handled
        }
        WidgetEvent::PointerUp { .. } => {
            let mut st = state.borrow_mut();
            st.drag_state = DragState::Idle;
            EventResponse::Ignored
        }
        _ => EventResponse::Ignored,
    }
}

pub(super) fn handle_scroll(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::Scroll { delta } = event else {
        return EventResponse::Ignored;
    };
    // Match `ScrollArea`'s sign convention: `delta.y` is the scroll
    // distance in document pixels per unit of wheel / trackpad
    // movement, already oriented so that positive means "scroll
    // content up" (i.e. increase scroll_y). For line-based events
    // the line_height multiplier is 16 px to match ScrollArea's
    // default.
    let (dx, dy) = match delta {
        ScrollDelta::Lines { x, y } => (*x * 16.0, *y * 16.0),
        ScrollDelta::Pixels { x, y } => (*x, *y),
    };
    let st = state.borrow();
    let new_y = (st.scroll_y.get() + dy).clamp(0.0, st.max_scroll_y.get());
    let new_x = (st.scroll_x.get() + dx).clamp(0.0, st.max_scroll_x.get());
    st.scroll_y.set(new_y);
    st.scroll_x.set(new_x);
    drop(st);
    ctx.request_frame();
    EventResponse::Handled
}

/// Select word under the caret on double-click.
pub(super) fn handle_double_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::WordUnderCursor);
    ctx.request_frame();
}

/// Select block under the caret on triple-click. Matches godot
/// rich_text_edit.rs:782.
pub(super) fn handle_triple_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::BlockUnderCursor);
    ctx.request_frame();
}

fn tap_select(state: &SharedState, pos: Point, kind: SelectionType) {
    let local = {
        let st = state.borrow();
        Point::new(pos.x - st.viewport_origin.x, pos.y - st.viewport_origin.y)
    };
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
    };
    if let Some(hit) = hit {
        let st = state.borrow();
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor.select(kind);
        drop(st);
        sync_cursor_signals(state);
    }
}
