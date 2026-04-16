//! Pointer dispatch for the text input widget.
//!
//! Simplified from [`rich_text::mouse`]: no scroll handling (single-line),
//! no auto-scroll velocity, no link/image click detection.
//! Right-click context menu support is wired in `field.rs` build()
//! where we have `BuildContext` access to pre-create the menu widget.

use fern_canvas::Point;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::widget::EventContext;
use fern_text::text_document::{MoveMode, SelectionType};

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
            if *button == PointerButton::Secondary {
                return handle_secondary_click(state, position, ctx);
            }
            if *button != PointerButton::Primary {
                return EventResponse::Ignored;
            }
            let shift = modifiers.shift();
            let local = to_local(state, position);
            let hit = hit_test(state, &local);
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
            let local = to_local(state, position);
            let hit = hit_test(state, &local);
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
    let local = to_local(state, pos);
    let hit = hit_test(state, &local);
    if let Some(hit_pos) = hit {
        let st = state.borrow();
        st.cursor.set_position(hit_pos, MoveMode::MoveAnchor);
        st.cursor.select(kind);
        drop(st);
        sync_cursor_signals(state);
    }
}

/// Convert window-space pointer to widget-local coordinates.
fn to_local(state: &SharedState, position: &Point) -> Point {
    let st = state.borrow();
    Point::new(
        position.x - st.viewport_origin.x,
        position.y - st.viewport_origin.y,
    )
}

/// Hit-test in engine space. The engine lays out text from x=0 without
/// wrapping. Our manual horizontal scroll means pointer local.x must
/// be adjusted by `scroll_x` so the engine resolves the correct
/// character position.
fn hit_test(state: &SharedState, local: &Point) -> Option<usize> {
    let st = state.borrow();
    let adjusted_x = local.x + st.scroll_x;
    let result = st.engine.hit_test(adjusted_x, local.y)?;
    Some(result.position)
}

/// Handle right-click: if the click lands outside the current selection,
/// move caret to click position. Then show the pre-built context menu.
fn handle_secondary_click(
    state: &SharedState,
    position: &Point,
    ctx: &mut EventContext,
) -> EventResponse {
    let local = to_local(state, position);
    let hit = hit_test(state, &local);
    if let Some(hit_pos) = hit {
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

    // Show the pre-built context menu. The menu_id is stashed on the
    // state by field.rs build(). If it's not there (shouldn't happen),
    // just eat the event.
    let st = state.borrow();
    let menu_id = st.context_menu_id;
    let anchor = st.field_widget_id;
    drop(st);
    if let (Some(menu_id), Some(anchor)) = (menu_id, anchor) {
        use fern_core::overlay::{
            DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest,
        };
        ctx.activate(menu_id);
        ctx.show_overlay(OverlayRequest {
            content_id: menu_id,
            anchor,
            placement: OverlayPlacement::AtPointer(*position),
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
        });
    }

    ctx.request_frame();
    EventResponse::Handled
}
