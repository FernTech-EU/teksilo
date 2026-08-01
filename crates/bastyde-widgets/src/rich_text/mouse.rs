// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//!    cooperative recognizers in `bastyde-core::gesture` guarantee that
//!    both fire in an escalating click sequence.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect};
use bastyde_core::event::{EventResponse, PointerButton, ScrollDelta, WidgetEvent};
use bastyde_core::widget::EventContext;
use bastyde_text::text_document::{MoveMode, SelectionType};

use super::hit_test;
use super::state::{DragState, SharedState};
use super::sync_cursor_signals;

/// Convert a **wrapper-node-local** pointer position (as delivered by the
/// framework dispatch) into the **engine/body-local** space that
/// text-typeset's `hit_test` expects. The body is inset within the
/// wrapper, so reconstruct the window point (`position + node_origin`)
/// and subtract the body origin (`viewport_origin`). At wrapper-origin 0
/// this reduces to `position - viewport_origin`.
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
                // Secondary / middle are for the application's own
                // context menu; let them bubble.
                return EventResponse::Ignored;
            }
            // The wrapper's `on_pointer_event` runs in the preview
            // pass on every event aimed at a descendant, including
            // the overlay scrollbars. A press here would otherwise
            // latch `drag_state = Selecting` against the text under
            // the bar and then `EventResponse::Handled` would steal
            // the subsequent PointerMove from the scrollbar's
            // gesture arena.
            if v_scrollbar_bounds.get().contains(*position)
                || h_scrollbar_bounds.get().contains(*position)
            {
                return EventResponse::Ignored;
            }
            let shift = modifiers.shift();
            let local = to_engine_local(state, position);
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
                bastyde_text::HitRegion::Link { href } => {
                    // Link click: do not move the caret. Dispatch to
                    // the widget's installed `on_link_activated`
                    // callback (if any) so applications can open the
                    // link / route to their router. Clone the `Rc`
                    // out of the state borrow before invoking so the
                    // handler can mutate widget state if it wants.
                    let callback = state.borrow().on_link_activated.clone();
                    if let Some(cb) = callback {
                        cb(href.as_str(), ctx);
                    }
                    ctx.request_frame();
                    return EventResponse::Ignored;
                }
                bastyde_text::HitRegion::Image { name } => {
                    let callback = state.borrow().on_image_activated.clone();
                    if let Some(cb) = callback {
                        cb(name.as_str(), ctx);
                    }
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
                // Affinity: at a soft-wrap boundary the typesetter
                // returned Upstream when the click landed on line
                // K+1's left edge (the visual START of the wrapped
                // line). At every other position the hit-test returns
                // Downstream and there is nothing to change.
                st.cursor_affinity = hit.affinity;
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
                // The pointer now owns the caret: stand the typewriter pin down
                // so this click *becomes* the resting position instead of the
                // page lurching to re-centre on it. Cleared by the next
                // keyboard-driven caret move, which resumes pinning.
                st.mouse_anchored = true;
            }
            sync_cursor_signals(state);
            // Reveal the placed caret in any enclosing scroll area, so
            // click-placement is consistent with keyboard caret motion (no-op
            // when the caret is already visible / follow disabled).
            super::keyboard::chase_caret_into_view(state, ctx);
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
            let local = to_engine_local(state, position);
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
                {
                    let mut st = state.borrow_mut();
                    st.cursor.set_position(hit.position, MoveMode::KeepAnchor);
                    st.cursor_affinity = hit.affinity;
                }
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
                    let intensity = ((local.y - (viewport_height - 20.0)) / 20.0).clamp(0.0, 1.0);
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
    overscroll: crate::common::scroll::OverscrollBehavior,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::Scroll { delta, .. } = event else {
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
    // Clamp each axis and learn whether it could absorb any of the delta.
    // Using the shared helper keeps the editor's boundary behaviour bit-for-bit
    // identical to `ScrollArea` / `ListView` / `TableView`.
    let (new_x, moved_x) =
        crate::common::scroll::scroll_clamp_axis(st.scroll_x.get(), dx, st.max_scroll_x.get());
    let (new_y, moved_y) =
        crate::common::scroll::scroll_clamp_axis(st.scroll_y.get(), dy, st.max_scroll_y.get());
    // Guard each `set` on an actual change: `Signal::set` fans out to every
    // observer unconditionally, so skipping the no-op write matters.
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
    // Decline (`Ignored`) when the editor is fully clamped on both axes so the
    // wheel chains to an ancestor scrollable — the editor embedded in a
    // scrolling form/page hands the leftover scroll to the page. Absorbing any
    // movement (`Handled`) keeps the event. `OverscrollBehavior::Contain`
    // always keeps the event at the editor. Shared boundary rule with every
    // other scrollable via `scroll_response`.
    crate::common::scroll::scroll_response(
        moved_x || moved_y,
        overscroll == crate::common::scroll::OverscrollBehavior::Contain,
    )
}

/// Select word under the caret on double-click.
pub(super) fn handle_double_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::WordUnderCursor);
    super::keyboard::chase_caret_into_view(state, ctx);
    ctx.request_frame();
}

/// Select block under the caret on triple-click. Matches godot
/// rich_text_edit.rs:782.
pub(super) fn handle_triple_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::BlockUnderCursor);
    super::keyboard::chase_caret_into_view(state, ctx);
    ctx.request_frame();
}

fn tap_select(state: &SharedState, pos: Point, kind: SelectionType) {
    // Double/triple-click is pointer-driven selection: same rule as a plain
    // click — the pin stands down rather than yanking the page under a
    // selection the user is making with the mouse.
    state.borrow_mut().mouse_anchored = true;
    let local = to_engine_local(state, &pos);
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

/// Convert a **window**-space point (the coordinate a context-menu factory is
/// handed by `show_context_menu_for`) into engine/body-local space. The body's
/// top-left in the window is `viewport_origin`, so `window - viewport_origin`
/// lands in the space `hit_test` expects — no `node_origin` term, because the
/// input is already a window point (unlike [`to_engine_local`], whose input is
/// wrapper-local).
fn engine_local_of_window(state: &SharedState, window_position: Point) -> Point {
    let st = state.borrow();
    Point::new(
        window_position.x - st.viewport_origin.x,
        window_position.y - st.viewport_origin.y,
    )
}

/// Reposition the caret to a right-click point **in window coordinates** when
/// the click lands *outside* the current selection — the platform convention
/// for "right-click, then Cut / Copy / Paste (/ add word) at the new caret". A
/// click *inside* the selection preserves it, so the menu's Cut/Copy still act
/// on the selection.
///
/// This has to run from inside the context-menu factory: the editor never sees
/// the Secondary `PointerDown` through its own pointer handler, because
/// `bastyde-core`'s `show_context_menu_for` consumes it (and returns early)
/// before `dispatch_to_widget` is ever called. Mirrors the single-line
/// [`TextInputField`](crate::primitives::text_input_field)'s behavior.
pub(super) fn reposition_caret_for_context_menu(state: &SharedState, window_position: Point) {
    let local = engine_local_of_window(state, window_position);
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
    };
    let Some(hit) = hit else {
        return;
    };
    {
        let st = state.borrow();
        if st.cursor.has_selection() {
            let (lo, hi) = (st.cursor.selection_start(), st.cursor.selection_end());
            if hit.position >= lo && hit.position <= hi {
                // Click inside the selection — keep it so Cut/Copy act on it.
                return;
            }
        }
    }
    {
        let mut st = state.borrow_mut();
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor_affinity = hit.affinity;
        st.preferred_x = None;
        st.select_all_level = 0;
        st.select_all_anchor_cell = None;
    }
    sync_cursor_signals(state);
}

/// Hit-test a **window**-space point to a document char offset — the primitive
/// behind [`EditorHandle::offset_at_point`](super::EditorHandle::offset_at_point).
/// `None` when the point resolves to no text.
pub(super) fn offset_at_window_point(state: &SharedState, window_position: Point) -> Option<usize> {
    let local = engine_local_of_window(state, window_position);
    let st = state.borrow();
    hit_test::hit_test_at(&st.engine, local, 0.0, 0.0).map(|hit| hit.position)
}
