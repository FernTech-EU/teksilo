// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer and wheel dispatch.
//!
//! Three entry points the wrapper installs: pointer events (caret placement,
//! drag-select, Alt-click caret adding), wheel scrolling, and the double/triple
//! tap word/line selections.
//!
//! Simpler than the rich text editor's equivalent in one respect — a source
//! document has no links or inline images, so there is no hit-region dispatch,
//! only text. It is richer in another: Alt-click adds a caret.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect};
use bastyde_core::event::{EventResponse, PointerButton, ScrollDelta, WidgetEvent};
use bastyde_core::widget::EventContext;
use bastyde_text::text_document::{MoveMode, SelectionType};

use super::state::{DragState, SharedState};
use super::sync_cursor_signals;
use crate::common::scroll::{OverscrollBehavior, scroll_clamp_axis, scroll_response};
use crate::rich_text::hit_test;

/// Pointer positions arrive **wrapper-local**; the engine wants **body-local**.
/// The body is inset within the wrapper, so reconstruct the window point
/// (`position + node_origin`) and subtract the body's origin.
fn to_engine_local(state: &SharedState, position: &Point) -> Point {
    let st = state.borrow();
    Point::new(
        position.x + st.node_origin.x - st.viewport_origin.x,
        position.y + st.node_origin.y - st.viewport_origin.y,
    )
}

pub(super) fn handle_pointer_event(
    state: &SharedState,
    v_scrollbar_bounds: &Rc<Cell<Rect>>,
    h_scrollbar_bounds: &Rc<Cell<Rect>>,
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
                // Secondary / middle belong to the context menu; let them bubble.
                return EventResponse::Ignored;
            }
            // This handler runs in the preview pass for every event aimed at a
            // descendant, including the overlay scroll bars. Without this a
            // press on the bar would latch a drag-select against the text
            // underneath and then steal the bar's own PointerMove.
            if v_scrollbar_bounds.get().contains(*position)
                || h_scrollbar_bounds.get().contains(*position)
            {
                return EventResponse::Ignored;
            }

            let local = to_engine_local(state, position);
            let hit = {
                let st = state.borrow();
                hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
            };
            let Some(hit) = hit else {
                // Ignored, not Handled: a click that missed text still needs to
                // reach the gesture arena, whose double/triple-tap recognizers
                // advance their state machines on every press.
                return EventResponse::Ignored;
            };

            let shift = modifiers.shift();
            let alt = modifiers.alt();
            {
                let mut st = state.borrow_mut();
                if alt {
                    // Alt-click: add a caret rather than move the primary. The
                    // near-universal multi-caret gesture.
                    add_caret_at(&mut st, hit.position);
                } else {
                    let mode = if shift {
                        MoveMode::KeepAnchor
                    } else {
                        MoveMode::MoveAnchor
                    };
                    // A plain or shift click collapses back to one caret: the
                    // user is pointing at where they want to be.
                    st.clear_extra_carets();
                    st.cursor.set_position(hit.position, mode);
                }
                st.cursor_affinity = hit.affinity;
                st.drag_state = DragState::Selecting {
                    auto_scroll_v_per_s: 0.0,
                };
                st.preferred_x = None;
            }
            sync_cursor_signals(state);
            super::keyboard::ensure_caret_visible(state);
            ctx.request_frame();
            // Ignored so the arena still sees the press — returning Handled
            // would consume it and double/triple tap would never fire.
            EventResponse::Ignored
        }

        WidgetEvent::PointerMove { position } => {
            let (dragging, viewport_height) = {
                let st = state.borrow();
                (
                    matches!(st.drag_state, DragState::Selecting { .. }),
                    st.viewport_height,
                )
            };
            if !dragging {
                return EventResponse::Ignored;
            }
            let local = to_engine_local(state, position);
            // Clamp into the viewport before hit-testing so a drag that leaves
            // the widget still resolves to the edge line rather than nothing.
            let clamped = Point::new(
                local.x,
                local.y.clamp(2.0, (viewport_height - 2.0).max(2.0)),
            );
            let hit = {
                let st = state.borrow();
                hit_test::hit_test_at(&st.engine, clamped, 0.0, 0.0)
            };
            if let Some(hit) = hit {
                {
                    let mut st = state.borrow_mut();
                    st.cursor.set_position(hit.position, MoveMode::KeepAnchor);
                    st.cursor_affinity = hit.affinity;
                }
                sync_cursor_signals(state);
            }
            // Store an edge-proximity velocity for the frame loop to apply per
            // tick. That is what lets the selection keep growing while the
            // pointer is held still past the edge — velocity here, integration
            // there, so the rate is time-based rather than motion-based.
            {
                let mut st = state.borrow_mut();
                let v = auto_scroll_velocity(local.y, viewport_height);
                st.drag_state = DragState::Selecting {
                    auto_scroll_v_per_s: v,
                };
                if v != 0.0
                    && let Some(handle) = &st.frame_request
                {
                    // Entering the zone restarts the loop from idle.
                    handle.set(true);
                }
            }
            ctx.request_frame();
            EventResponse::Handled
        }

        WidgetEvent::PointerUp { .. } => {
            state.borrow_mut().drag_state = DragState::Idle;
            EventResponse::Ignored
        }

        _ => EventResponse::Ignored,
    }
}

/// Edge-proximity scroll velocity in px/s, ramped over a 20 px margin.
///
/// Expressed per *second* rather than per frame so the rate does not depend on
/// the display's refresh rate.
fn auto_scroll_velocity(y: f32, viewport_height: f32) -> f32 {
    const MARGIN: f32 = 20.0;
    const MAX_PER_SEC: f32 = 60.0 * 60.0;
    if y < MARGIN {
        let intensity = ((MARGIN - y) / MARGIN).clamp(0.0, 1.0);
        -MAX_PER_SEC * intensity
    } else if y > viewport_height - MARGIN {
        let intensity = ((y - (viewport_height - MARGIN)) / MARGIN).clamp(0.0, 1.0);
        MAX_PER_SEC * intensity
    } else {
        0.0
    }
}

/// Add a caret at `pos`, or remove it if one is already there.
///
/// Alt-clicking an existing caret removes it, which is how every editor with
/// this gesture behaves — it is the undo for an Alt-click that landed wrong. Alt-clicking
/// the primary is ignored rather than removing it: something has to stay.
pub(super) fn add_caret_at(st: &mut super::state::CodeEditorState, pos: usize) {
    if st.cursor.position() == pos {
        return;
    }
    if let Some(i) = st.extra_carets.iter().position(|c| c.position() == pos) {
        st.extra_carets.remove(i);
        return;
    }
    let c = st.document.cursor();
    c.set_position(pos, MoveMode::MoveAnchor);
    st.extra_carets.push(c);
}

pub(super) fn handle_scroll(
    state: &SharedState,
    overscroll: OverscrollBehavior,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::Scroll { delta, .. } = event else {
        return EventResponse::Ignored;
    };
    // 16 px per line matches ScrollArea's default, so the editor scrolls at the
    // same rate as every other scrollable in the app.
    let (dx, dy) = match delta {
        ScrollDelta::Lines { x, y } => (*x * 16.0, *y * 16.0),
        ScrollDelta::Pixels { x, y } => (*x, *y),
    };
    let st = state.borrow();
    let (new_x, moved_x) = scroll_clamp_axis(st.scroll_x.get(), dx, st.max_scroll_x.get());
    let (new_y, moved_y) = scroll_clamp_axis(st.scroll_y.get(), dy, st.max_scroll_y.get());
    if moved_x {
        st.scroll_x.set(new_x);
    }
    if moved_y {
        st.scroll_y.set(new_y);
    }
    drop(st);
    if moved_x || moved_y {
        ctx.request_frame();
    }
    // Fully clamped on both axes: decline, so the wheel chains to an enclosing
    // scrollable — an editor inside a scrolling page hands the page its
    // leftover. Same boundary rule as every other scrollable.
    scroll_response(
        moved_x || moved_y,
        overscroll == OverscrollBehavior::Contain,
    )
}

/// Double-click selects the word.
pub(super) fn handle_double_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::WordUnderCursor);
    ctx.request_frame();
}

/// Triple-click selects the line.
///
/// The line, not the block: in a code document one line *is* one block, but
/// saying so explicitly keeps this correct if a wrapped plain-text editor ever
/// makes the two diverge.
pub(super) fn handle_triple_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::LineUnderCursor);
    ctx.request_frame();
}

fn tap_select(state: &SharedState, pos: Point, kind: SelectionType) {
    let local = to_engine_local(state, &pos);
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
    };
    let Some(hit) = hit else {
        return;
    };
    {
        let mut st = state.borrow_mut();
        st.clear_extra_carets();
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor.select(kind);
    }
    sync_cursor_signals(state);
}
