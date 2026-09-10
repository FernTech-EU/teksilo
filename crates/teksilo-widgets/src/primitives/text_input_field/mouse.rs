// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer dispatch for the text input widget.
//!
//! Simplified from `rich_text::mouse`: no scroll handling (single-line),
//! no auto-scroll velocity, no link/image click detection. Right-click
//! routes through the framework's `.context_menu(...)` plumbing — see
//! `field.rs` build() — so this module only handles primary-button
//! selection and drag.
//!
//! # Two devices, two commit points
//!
//! A **precise** pointer commits on the press, exactly as it always has: a
//! click is a click, and a press that never becomes anything else is still one.
//!
//! A **direct** pointer — a finger, a pen — defers the whole decision to the
//! release, because the same contact is the opening sample of a scroll and a
//! scrolling finger must leave the caret and the selection exactly as it found
//! them. No release-time predicate can rescue a caret already written on
//! `PointerDown`, so the write itself moves to the release, gated on
//! [`release_completes_the_press`](crate::data_views::release_completes_the_press).
//! Same rule, and the same predicate, that the five data views adopted for
//! their row selection.

use teksilo_canvas::Point;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::widget::EventContext;
use teksilo_text::text_document::{MoveMode, SelectionType};

use super::state::{DragState, SharedState, TextInputState, sync_cursor_signals};
use super::touch::FieldTouch;

pub(crate) fn handle_pointer_event(
    state: &SharedState,
    touch: &std::rc::Rc<FieldTouch>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    if ctx.pointer_kind().is_direct() {
        return handle_direct_pointer_event(state, touch, event, ctx);
    }
    match event {
        WidgetEvent::PointerDown {
            position,
            button,
            modifiers,
            ..
        } => {
            if *button != PointerButton::Primary {
                return EventResponse::Ignored;
            }
            let shift = modifiers.shift();
            // A cursor has taken over. Touch chrome — handles, the selection
            // toolbar — is standing on the text it is trying to reach, and the
            // affordance band is exempt from outside-press dismissal, so nothing
            // else on a hybrid machine would ever remove it. Free when there is
            // nothing raised; see `FieldTouch::dismiss`.
            touch.dismiss();
            let hit = hit_test(state, position);
            let Some(hit_pos) = hit else {
                return EventResponse::Ignored;
            };
            {
                let mut st = state.borrow_mut();
                let mode = if shift {
                    MoveMode::KeepAnchor
                } else {
                    MoveMode::MoveAnchor
                };
                st.cursor.set_position(hit_pos, mode);
                st.drag_state = DragState::Selecting;
            }
            sync_cursor_signals(state);
            ctx.request_frame();
            // Ignored so gesture arena (double/triple tap) also sees this.
            EventResponse::Ignored
        }
        WidgetEvent::PointerMove { position, .. } => {
            let is_dragging = matches!(state.borrow().drag_state, DragState::Selecting);
            if !is_dragging {
                return EventResponse::Ignored;
            }
            let hit = hit_test(state, position);
            if let Some(hit_pos) = hit {
                let st = state.borrow();
                st.cursor.set_position(hit_pos, MoveMode::KeepAnchor);
                drop(st);
                sync_cursor_signals(state);
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
/// `drag_state` is deliberately never armed here, so the `PointerMove` arm above
/// stays inert for a direct pointer without needing a second guard of its own —
/// and the module's missing `PointerCancel` arm stops mattering, because there
/// is no selection session left to strand.
///
/// Every arm answers `Ignored`: the press has to keep reaching the gesture arena
/// (the hold that selects a word, the double and triple taps), and the release
/// has to keep reaching the tap recognizer.
fn handle_direct_pointer_event(
    state: &SharedState,
    touch: &std::rc::Rc<FieldTouch>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    if matches!(event, WidgetEvent::PointerDown { .. }) {
        // A fresh press: forget any hold this contact's id carried from a
        // previous gesture, so a stale record can never eat a real tap.
        touch.take_hold_consumed(ctx.pointer().id);
        // …and take the toolbar down now rather than on the release. Its
        // commands are aimed at a selection this press is about to replace, and
        // a menu that lingers under the finger through the whole press reads as
        // the press having missed.
        touch.hide_toolbar();
        return EventResponse::Ignored;
    }
    let WidgetEvent::PointerUp { button, .. } = event else {
        return EventResponse::Ignored;
    };
    if *button != PointerButton::Primary {
        return EventResponse::Ignored;
    }
    // A hold fires from the gesture timer, so its release arrives here after
    // the word is already selected. Placing a caret now would collapse it.
    if touch.take_hold_consumed(ctx.pointer().id) {
        return EventResponse::Ignored;
    }
    // A contact that left its tap boundary, or whose press a scrollable above
    // claimed, was scrolling. Nothing it did is a caret placement.
    if !crate::data_views::release_completes_the_press(ctx) {
        return EventResponse::Ignored;
    }
    // The sample's own window position, converted back to the field's space:
    // what the arm above receives is already local, and `hit_test` wants local.
    let Some(window) = ctx.pointer_position() else {
        return EventResponse::Ignored;
    };
    let local = {
        let st = state.borrow();
        super::touch::window_to_local(&st, window)
    };
    let Some(hit_pos) = hit_test(state, &local) else {
        return EventResponse::Ignored;
    };
    {
        let st = state.borrow();
        st.cursor.set_position(hit_pos, MoveMode::MoveAnchor);
    }
    sync_cursor_signals(state);
    // The caret moved, so the on-screen keyboard's candidate window has to
    // move with it. The field's own reporter, not the controller's: this one
    // holds the focus / layout guard and the ibus-feedback-loop dedup.
    super::keyboard::report_ime_cursor_area(state, ctx);
    // A tap places a caret; it does not ask for a menu. The toolbar belongs to a
    // deliberate selection — a hold, a multi-tap, or the end of a handle drag.
    touch.raise(ctx, super::touch::ToolbarIntent::Hide);
    ctx.request_frame();
    EventResponse::Ignored
}

/// Select the word under `point` and raise the affordances — the touch hold.
///
/// The mouse refusal is
/// [`TouchSelection::on_long_press`](teksilo_core::text_touch::TouchSelection::on_long_press)'s,
/// not a second copy here: the gesture's own pointer is passed to it and it
/// guards on that. Its first signature asked `EventContext::pointer_kind`
/// instead, which on this path answered **the mouse whatever the device was** —
/// a hold is recognised by the gesture timer rather than by a sample — so this
/// host carried the guard itself and drove `raise`, and core's entry point was
/// dead code. Both halves of that are fixed: the tree installs the holding
/// contact for a timer dispatch, and the guard reads the gesture.
///
/// What stays here is the part core cannot do: the coordinate conversion.
pub(crate) fn handle_long_press(
    state: &SharedState,
    touch: &std::rc::Rc<FieldTouch>,
    event: &teksilo_core::gesture::TapEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    // No sample is being dispatched, so `pointer_position` is `None` here; the
    // tap's own position is field-local and the field does not move mid-press.
    let window = {
        let st = state.borrow();
        Point::new(
            event.position.x + st.viewport_origin.x,
            event.position.y + st.viewport_origin.y,
        )
    };
    if !touch.select_word_at(event.pointer, window, ctx) {
        return EventResponse::Ignored;
    }
    touch.mark_hold_consumed(event.pointer.id);
    ctx.request_frame();
    EventResponse::Handled
}

/// Select word under the caret on double-click.
pub(crate) fn handle_double_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, &pos, SelectionType::WordUnderCursor);
    ctx.request_frame();
}

/// Select all on triple-click (single-line = whole text).
pub(crate) fn handle_triple_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, &pos, SelectionType::Document);
    ctx.request_frame();
}

fn tap_select(state: &SharedState, pos: &Point, kind: SelectionType) {
    let hit = hit_test(state, pos);
    if let Some(hit_pos) = hit {
        let st = state.borrow();
        st.cursor.set_position(hit_pos, MoveMode::MoveAnchor);
        st.cursor.select(kind);
        drop(st);
        sync_cursor_signals(state);
    }
}

/// Hit-test in engine space. The engine lays out text from x=0 without
/// wrapping. Our manual horizontal scroll means pointer local.x must
/// be adjusted by `scroll_x` so the engine resolves the correct
/// character position.
///
/// Suffix handling: when a non-editable suffix is configured, clicks
/// landing on the suffix strip or past the text end clamp to the end
/// of the document — the caret cannot enter the suffix. This matches
/// Qt's `QSpinBox` behavior: tapping the "%", "€", … suffix just
/// positions the caret after the last editable character.
pub(crate) fn hit_test(state: &SharedState, local: &Point) -> Option<usize> {
    let st = state.borrow();
    hit_test_in(&st, *local)
}

/// [`hit_test`] for a caller that already holds the borrow — the touch
/// controller's [`TextHitSource`](teksilo_core::text_touch::TextHitSource)
/// reaches this surface through a `&mut TextInputState`.
pub(crate) fn hit_test_in(st: &TextInputState, local: Point) -> Option<usize> {
    let text_viewport = (st.viewport_width - st.suffix_width).max(0.0);
    let doc_end = st
        .document
        .to_plain_text()
        .unwrap_or_default()
        .chars()
        .count();

    // Click on the suffix strip (to the right of the editable area):
    // snap to end of document.
    if st.suffix_width > 0.0 && local.x >= text_viewport {
        return Some(doc_end);
    }

    let adjusted_x = local.x + st.scroll_x;
    if let Some(result) = st.engine.hit_test(adjusted_x, local.y) {
        return Some(result.position);
    }

    // Hit-test missed (click past the last glyph but still within
    // the editable viewport). Snap to end of document — never
    // return `None` when the click is inside the widget, because
    // the caller uses `None` to ignore the event entirely.
    if local.x >= 0.0 && local.x < text_viewport {
        return Some(doc_end);
    }
    None
}

/// Reposition the caret in response to a right-click that's about to
/// open the context menu. Called from the `.context_menu(...)`
/// factory in `field.rs`.
///
/// Mirrors the platform convention: a right-click *inside* the
/// existing selection leaves the selection alone (so menu actions
/// like Cut / Copy operate on the visible selection), but a
/// right-click *outside* the selection moves the caret to the click
/// position so menu actions there target the new caret location.
///
/// `position` is **window-local**: the context-menu factory is invoked
/// straight from the right-click `PointerDown` (not the localized
/// gesture/pointer dispatch), so convert to field-local here via the
/// field's `viewport_origin` before hit-testing.
pub(crate) fn reposition_caret_for_context_menu(state: &SharedState, position: Point) {
    let local = {
        let st = state.borrow();
        Point::new(
            position.x - st.viewport_origin.x,
            position.y - st.viewport_origin.y,
        )
    };
    let Some(hit_pos) = hit_test(state, &local) else {
        return;
    };
    let st = state.borrow();
    let anchor = st.cursor.anchor();
    let caret = st.cursor.position();
    let (lo, hi) = (anchor.min(caret), anchor.max(caret));
    let in_selection = lo != hi && hit_pos >= lo && hit_pos <= hi;
    drop(st);
    if !in_selection {
        let st = state.borrow();
        st.cursor.set_position(hit_pos, MoveMode::MoveAnchor);
        drop(st);
        sync_cursor_signals(state);
    }
}
