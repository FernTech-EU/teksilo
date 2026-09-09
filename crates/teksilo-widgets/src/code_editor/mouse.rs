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
//!
//! Shared by all three faces: [`CodeEditor`](super::CodeEditor),
//! [`PlainTextEditor`](super::PlainTextEditor) and
//! [`LogView`](super::LogView) install the same three entry points, so one
//! adoption of the touch contract here reaches all of them.
//!
//! # Two devices, two commit points
//!
//! A **precise** pointer commits on the press, exactly as it always has.
//!
//! A **direct** pointer — a finger, a pen — defers the whole decision to the
//! release, because the same contact is the opening sample of a *pan* and a
//! panning finger must leave the caret and the selection exactly as it found
//! them. No release-time predicate can rescue a caret already written on
//! `PointerDown`, so the write itself moves to the release, gated on
//! [`release_completes_the_press`](crate::data_views::release_completes_the_press)
//! — the same rule, and the same predicate, the five data views adopted for
//! their row selection and the single-line stack for its caret.
//!
//! Two press-time commitments therefore have no direct-pointer form, and both
//! are deliberate: **drag-select** (a finger's drag pans; the range is chosen
//! with the selection handles the hold raises) and **Alt-click's extra caret**
//! (there is no Alt on a touch screen, and multi-caret editing is a keyboard
//! and mouse affordance — `Ctrl+Alt+↑/↓` remains the route).

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::widget::EventContext;
use teksilo_text::text_document::{MoveMode, SelectionType};

use super::state::{DragState, SharedState};
use super::sync_cursor_signals;
use crate::rich_text::hit_test;
use crate::rich_text::touch_mount::{EditorTouch, ToolbarIntent};

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
    touch: &Rc<EditorTouch>,
    v_scrollbar_bounds: &Rc<Cell<Rect>>,
    h_scrollbar_bounds: &Rc<Cell<Rect>>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    if ctx.pointer_kind().is_direct() {
        return handle_direct_pointer_event(
            state,
            touch,
            v_scrollbar_bounds,
            h_scrollbar_bounds,
            event,
            ctx,
        );
    }
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
            // A cursor has taken over. Touch chrome — handles, the selection
            // toolbar — is standing on the text it is trying to reach, and the
            // affordance band is exempt from outside-press dismissal, so nothing
            // else on a hybrid machine would ever remove it. Free when there is
            // nothing raised; see `EditorTouch::dismiss`.
            touch.dismiss();
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
            // The caret moved, so the OS IME candidate window has to move with
            // it — every *keyboard* caret move already reports this and a
            // pointer placement did not, which left the candidate list beside
            // wherever the caret was last typed to. The surface's own reporter,
            // never the controller's: this one holds the focus / read-only /
            // layout guard and the ibus-feedback-loop dedup.
            super::keyboard::report_ime_cursor_area(state, ctx);
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

/// A finger or a pen: nothing on the press, everything on a release that still
/// belongs to it.
///
/// `drag_state` is deliberately never armed here, so the precise-pointer
/// `PointerMove` arm above stays inert for a direct pointer without needing a
/// second guard of its own — which also means the edge auto-scroll ramp is
/// unreachable for a finger, as its own comment says it should be.
///
/// Every arm answers `Ignored`: the press has to keep reaching the gesture arena
/// (the hold that selects a word, the double and triple taps), the release has
/// to keep reaching the tap recognizer, and the whole contact has to stay
/// available to the pan claim the surface installs.
fn handle_direct_pointer_event(
    state: &SharedState,
    touch: &Rc<EditorTouch>,
    v_scrollbar_bounds: &Rc<Cell<Rect>>,
    h_scrollbar_bounds: &Rc<Cell<Rect>>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    match event {
        WidgetEvent::PointerDown { position, .. } => {
            // The overlay scroll bars run their own drags; a press on one is
            // theirs, and must not clear this contact's hold record.
            if v_scrollbar_bounds.get().contains(*position)
                || h_scrollbar_bounds.get().contains(*position)
            {
                return EventResponse::Ignored;
            }
            // A fresh press: forget any hold this contact's id carried from a
            // previous gesture, so a stale record can never eat a real tap.
            touch.take_hold_consumed(ctx.pointer().id);
            // …and remember where it landed, so the release can tell a tap from
            // a pan. See `EditorTouch::press_is_still_a_tap` for why the
            // framework's coarse tap boundary cannot answer that here.
            touch.mark_press(ctx.pointer().id, ctx.pointer_position());
            // …and take the toolbar down now rather than on the release. Its
            // commands are aimed at a selection this press is about to replace,
            // and a menu that lingers under the finger through the whole press
            // reads as the press having missed.
            touch.hide_toolbar();
            EventResponse::Ignored
        }
        WidgetEvent::PointerUp { button, .. } => {
            if *button != PointerButton::Primary {
                return EventResponse::Ignored;
            }
            // A hold fires from the gesture timer, so its release arrives here
            // after the word is already selected. Placing a caret now would
            // collapse it.
            if touch.take_hold_consumed(ctx.pointer().id) {
                return EventResponse::Ignored;
            }
            // A contact whose press a scrollable above claimed was panning.
            // Nothing it did is a caret placement.
            if !crate::data_views::release_completes_the_press(ctx) {
                return EventResponse::Ignored;
            }
            let Some(window) = ctx.pointer_position() else {
                return EventResponse::Ignored;
            };
            // …and a contact that travelled further than a tap of its kind may
            // was panning *this* surface, which the predicate above cannot see:
            // the surface is both the press's owner and the pan's claimant, so
            // nothing was claimed elsewhere, and a coarse pointer's tap boundary
            // is the node's whole rectangle.
            let tap_slop = {
                let st = state.borrow();
                st.input_tokens.profile(ctx.pointer_kind()).tap_slop
            };
            if !touch.press_is_still_a_tap(ctx.pointer().id, window, tap_slop) {
                return EventResponse::Ignored;
            }
            let hit = {
                let st = state.borrow();
                let local = super::touch::window_to_engine_local(&st, window);
                hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
            };
            let Some(hit) = hit else {
                return EventResponse::Ignored;
            };
            {
                let mut st = state.borrow_mut();
                st.clear_extra_carets();
                st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
                st.cursor_affinity = hit.affinity;
                st.preferred_x = None;
            }
            sync_cursor_signals(state);
            super::keyboard::ensure_caret_visible(state);
            super::keyboard::report_ime_cursor_area(state, ctx);
            // A tap places a caret; it does not ask for a menu. The toolbar
            // belongs to a deliberate selection — a hold, a multi-tap, or the
            // end of a handle drag.
            touch.raise(ctx, ToolbarIntent::Hide);
            ctx.request_frame();
            EventResponse::Ignored
        }
        _ => EventResponse::Ignored,
    }
}

/// Select the word under `event` and raise the affordances — the touch hold.
///
/// The mouse refusal is
/// [`TouchSelection::on_long_press`](teksilo_core::text_touch::TouchSelection::on_long_press)'s,
/// not a second copy here: the gesture's own pointer is handed to it and it
/// guards on that rather than on the context, which on a timer-recognised
/// gesture used to answer for the wrong device entirely.
///
/// What stays here is the part core cannot do: the coordinate conversion. No
/// sample is being dispatched, so `pointer_position` is `None`; the tap's own
/// position is wrapper-local and the *surface* does not move mid-press, so
/// `local + node_origin` is exact — the same arithmetic
/// [`to_engine_local`] already does in the other direction.
pub(super) fn handle_long_press(
    state: &SharedState,
    touch: &Rc<EditorTouch>,
    event: &teksilo_core::gesture::TapEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let window = {
        let st = state.borrow();
        Point::new(
            event.position.x + st.node_origin.x,
            event.position.y + st.node_origin.y,
        )
    };
    if !touch.select_word_at(event.pointer, window, ctx) {
        return EventResponse::Ignored;
    }
    touch.mark_hold_consumed(event.pointer.id);
    ctx.request_frame();
    EventResponse::Handled
}

/// Edge-proximity scroll velocity in px/s, ramped over a 20 px margin.
///
/// Expressed per *second* rather than per frame so the rate does not depend on
/// the display's refresh rate.
fn auto_scroll_velocity(y: f32, viewport_height: f32) -> f32 {
    /// The editor's own, tighter edge band: a caret drag inside text wants to
    /// start scrolling later than a row drag over a list, so this is 20 dp
    /// rather than [`crate::common::drag_autoscroll::EDGE_BAND_PRECISE`]'s 32.
    /// Kind-widening is the coarse-pointer path P26 owns (a finger selecting
    /// text uses selection handles, not this ramp).
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
