//! Pointer dispatch for the text input widget.
//!
//! Simplified from `rich_text::mouse`: no scroll handling (single-line),
//! no auto-scroll velocity, no link/image click detection. Right-click
//! routes through the framework's `.context_menu(...)` plumbing — see
//! `field.rs` build() — so this module only handles primary-button
//! selection and drag.

use bastyde_canvas::Point;
use bastyde_core::event::{EventResponse, PointerButton, WidgetEvent};
use bastyde_core::widget::EventContext;
use bastyde_text::text_document::{MoveMode, SelectionType};

use super::state::{DragState, SharedState, sync_cursor_signals};

pub(crate) fn handle_pointer_event(
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
                return EventResponse::Ignored;
            }
            let shift = modifiers.shift();
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
        WidgetEvent::PointerMove { position } => {
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
fn hit_test(state: &SharedState, local: &Point) -> Option<usize> {
    let st = state.borrow();
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
