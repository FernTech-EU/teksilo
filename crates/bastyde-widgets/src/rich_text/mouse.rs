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

/// Smallest side a resize may leave, in logical pixels.
///
/// A picture dragged to nothing cannot be grabbed again — its handles would
/// have nowhere to sit — so the drag stops here rather than letting the writer
/// lose the image behind an undo.
const RESIZE_MIN_EDGE: f32 = 24.0;

/// How far outside a grip's own square a press still counts.
///
/// The grip is drawn small so it does not cover a thumbnail; this is what makes
/// it hittable without care. Generous on purpose — the cost of overshooting is
/// a resize the writer did not want, which one Escape or Ctrl+Z undoes, while
/// the cost of undershooting is a feature that feels broken.
const RESIZE_HANDLE_SLOP: f32 = 5.0;

/// The corner grip under `local`, if the selected image has one there.
///
/// Returns the image's name, its rect, its offset, and which corner was taken.
fn grabbed_handle(
    state: &SharedState,
    local: Point,
) -> Option<(String, [f32; 4], usize, (f32, f32))> {
    let selected = state.borrow().selected_image.borrow().clone()?;
    let corner = handle_at(selected.rect, local)?;
    Some((selected.name, selected.rect, selected.offset, corner))
}

/// Which corner of `rect` a press at `local` grabs, if any.
///
/// Split out from [`grabbed_handle`] so the aiming rule — the half of this that
/// decides whether the feature is usable — can be tested without a widget tree,
/// a window, or a pointer device.
fn handle_at(rect: [f32; 4], local: Point) -> Option<(f32, f32)> {
    let [x, y, w, h] = rect;
    let reach = super::paint::RESIZE_HANDLE_SIZE / 2.0 + RESIZE_HANDLE_SLOP;
    super::paint::RESIZE_CORNERS.into_iter().find(|&(fx, fy)| {
        let (cx, cy) = (x + w * fx, y + h * fy);
        (local.x - cx).abs() <= reach && (local.y - cy).abs() <= reach
    })
}

/// The rect a corner drag proposes, with the picture's proportions kept.
///
/// The corner opposite the one being dragged stays put, so the gesture reads
/// the way it does everywhere else. Proportions are kept by following whichever
/// axis the pointer moved *proportionally* further — taking width alone would
/// ignore a drag that was mostly vertical, and averaging the two makes a
/// diagonal drag lag behind the pointer on both.
///
/// The result is always positioned at the image's original top-left: the
/// picture sits in a text flow, so the layout decides where it lands once the
/// size changes. Anchoring the preview anywhere else would show the writer a
/// position the reflow is about to contradict.
fn proportional_resize(origin: [f32; 4], corner: (f32, f32), local: Point) -> [f32; 4] {
    let [x, y, w, h] = origin;
    if w <= 0.0 || h <= 0.0 {
        return origin;
    }
    // The fixed corner is the diagonal opposite of the grabbed one.
    let anchor_x = x + w * (1.0 - corner.0);
    let anchor_y = y + h * (1.0 - corner.1);
    let dragged_w = (local.x - anchor_x).abs();
    let dragged_h = (local.y - anchor_y).abs();

    let sx = dragged_w / w;
    let sy = dragged_h / h;
    let scale = if (sx - 1.0).abs() >= (sy - 1.0).abs() {
        sx
    } else {
        sy
    };
    let scale = scale.max(RESIZE_MIN_EDGE / w.min(h));
    [x, y, (w * scale).max(1.0), (h * scale).max(1.0)]
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
            // A resize grip is checked before the hit-test, because a grip sits
            // *outside* the picture: the engine reports `HitRegion::Image` only
            // within the image's own rect, so by the time the hit-test has an
            // answer the corner has already been missed.
            if let Some((name, rect, offset, corner)) = grabbed_handle(state, local) {
                let mut st = state.borrow_mut();
                st.drag_state = DragState::ResizingImage {
                    name,
                    offset,
                    origin: rect,
                    corner,
                };
                st.resize_preview.set(Some(rect));
                drop(st);
                ctx.request_frame();
                // `Ignored`, like every other press path here. Returning
                // `Handled` consumes the event, and the gesture arena that
                // consumed press never delivers the moves that follow — so the
                // grip would latch and the drag would never arrive.
                return EventResponse::Ignored;
            }
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
                        // The caret is deliberately left where it was, as for a
                        // link. The offset is handed over instead, so a host that
                        // wants the image selected can say so itself — and one
                        // that only wants to open a viewer is not left having to
                        // put the caret back.
                        cb(
                            &super::ImageActivation {
                                name: name.clone(),
                                offset: hit.position,
                            },
                            ctx,
                        );
                    }
                    // The same press bookkeeping an ordinary click does, minus
                    // the caret placement — a click on a picture is still a
                    // click, and the state it leaves behind has to say so.
                    //
                    // `drag_state` above all: without it a drag that *starts*
                    // on a picture never becomes a selection (`PointerMove`
                    // gates on `is_dragging`), so the only way to select an
                    // image together with the words after it was to start the
                    // drag somewhere else and come back over it.
                    {
                        let mut st = state.borrow_mut();
                        st.drag_state = DragState::Selecting {
                            auto_scroll_v_per_s: 0.0,
                        };
                        st.preferred_x = None;
                        st.select_all_level = 0;
                        st.select_all_anchor_cell = None;
                        st.mouse_anchored = true;
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
            let (is_dragging, resizing, viewport_height) = {
                let st = state.borrow();
                let dragging = matches!(st.drag_state, DragState::Selecting { .. });
                let resizing = match &st.drag_state {
                    DragState::ResizingImage { origin, corner, .. } => Some((*origin, *corner)),
                    _ => None,
                };
                (dragging, resizing, st.viewport_height)
            };
            if let Some((origin, corner)) = resizing {
                let local = to_engine_local(state, position);
                let proposed = proportional_resize(origin, corner, local);
                state.borrow().resize_preview.set(Some(proposed));
                ctx.request_frame();
                return EventResponse::Handled;
            }
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
            // A resize is reported once, here — not on every move. The document
            // is the durable record, and rewriting it per pointer move would put
            // a hundred entries on the undo stack for one gesture.
            let finished = {
                let st = state.borrow();
                match (&st.drag_state, st.resize_preview.get()) {
                    (DragState::ResizingImage { name, offset, .. }, Some(rect)) => {
                        Some((st.on_image_resized.clone(), name.clone(), *offset, rect))
                    }
                    _ => None,
                }
            };
            {
                let mut st = state.borrow_mut();
                st.drag_state = DragState::Idle;
                st.resize_preview.set(None);
            }
            if let Some((callback, name, offset, rect)) = finished {
                if let Some(cb) = callback {
                    cb(
                        &super::ImageResize {
                            name,
                            offset,
                            width: rect[2].round().max(1.0) as u32,
                            height: rect[3].round().max(1.0) as u32,
                        },
                        ctx,
                    );
                }
                ctx.request_frame();
                return EventResponse::Handled;
            }
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
/// Move the caret to the drop point under a hovering drag, in the coordinate
/// space the widget's own drag handlers receive (widget-local).
///
/// A drag with no caret under it asks the writer to aim at nothing: they can see
/// the pointer but not where the text will land, and the two are never the same
/// place because a caret snaps to a character boundary. So the caret follows the
/// drag, and dropping puts the payload exactly where the caret already is.
///
/// Returns `false` when the pointer is not over any text, so the caller can
/// decline the drop rather than insert somewhere arbitrary.
pub(super) fn move_caret_for_drag(state: &SharedState, local: Point) -> bool {
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, to_engine_local(state, &local), 0.0, 0.0)
    };
    let Some(hit) = hit else { return false };
    {
        let mut st = state.borrow_mut();
        // Collapsed, not extended: a drop replaces nothing, and leaving an
        // anchor behind would make the insertion look like it was about to
        // overwrite the selection it came from.
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor_affinity = hit.affinity;
    }
    sync_cursor_signals(state);
    true
}

pub(super) fn offset_at_window_point(state: &SharedState, window_position: Point) -> Option<usize> {
    let local = engine_local_of_window(state, window_position);
    let st = state.borrow();
    hit_test::hit_test_at(&st.engine, local, 0.0, 0.0).map(|hit| hit.position)
}

#[cfg(test)]
mod resize_tests {
    use super::*;

    /// A 200×100 picture at (50, 20) — deliberately not square, so a fix that
    /// silently squares it up would show.
    const IMG: [f32; 4] = [50.0, 20.0, 200.0, 100.0];
    const BOTTOM_RIGHT: (f32, f32) = (1.0, 1.0);
    const TOP_LEFT: (f32, f32) = (0.0, 0.0);

    fn ratio(rect: [f32; 4]) -> f32 {
        rect[2] / rect[3]
    }

    #[test]
    fn dragging_a_corner_keeps_the_proportions() {
        // Straight out along the diagonal, then a drag that is mostly
        // horizontal, then one that is mostly vertical. All three must keep the
        // 2:1 shape — that is the whole promise of the gesture.
        for target in [
            Point::new(450.0, 220.0),
            Point::new(450.0, 130.0),
            Point::new(260.0, 320.0),
        ] {
            let out = proportional_resize(IMG, BOTTOM_RIGHT, target);
            assert!(
                (ratio(out) - ratio(IMG)).abs() < 0.001,
                "proportions drifted: {out:?} is {:.3}, wanted {:.3}",
                ratio(out),
                ratio(IMG)
            );
        }
    }

    #[test]
    fn dragging_outward_grows_and_inward_shrinks() {
        let bigger = proportional_resize(IMG, BOTTOM_RIGHT, Point::new(450.0, 220.0));
        assert!(bigger[2] > IMG[2], "{bigger:?}");
        let smaller = proportional_resize(IMG, BOTTOM_RIGHT, Point::new(150.0, 70.0));
        assert!(smaller[2] < IMG[2], "{smaller:?}");
    }

    #[test]
    fn the_opposite_corner_is_what_stays_put() {
        // Grabbing the TOP-LEFT measures against the bottom-right, so dragging
        // up and left must GROW the picture. A version that always measured
        // from the top-left would shrink it here — the sign error that makes
        // two of the four grips feel inverted.
        let out = proportional_resize(IMG, TOP_LEFT, Point::new(0.0, 0.0));
        assert!(
            out[2] > IMG[2],
            "dragging the top-left up and out must grow: {out:?}"
        );
    }

    #[test]
    fn a_resize_cannot_shrink_the_image_out_of_reach() {
        // Dragged far past the anchor. A picture with no area has no grips, so
        // it could never be resized back — the drag has to stop short.
        let out = proportional_resize(IMG, BOTTOM_RIGHT, Point::new(51.0, 21.0));
        assert!(out[2] >= RESIZE_MIN_EDGE, "{out:?}");
        assert!(out[3] >= RESIZE_MIN_EDGE, "{out:?}");
        assert!(
            (ratio(out) - ratio(IMG)).abs() < 0.001,
            "the floor must not distort it: {out:?}"
        );
    }

    #[test]
    fn the_preview_stays_at_the_images_own_place_in_the_text() {
        // Whatever corner is dragged, the result is anchored at the original
        // top-left: the picture sits in a text flow, so the reflow decides
        // where it lands. Showing it anywhere else previews a position the
        // relayout is about to contradict.
        for corner in [TOP_LEFT, BOTTOM_RIGHT, (1.0, 0.0), (0.0, 1.0)] {
            let out = proportional_resize(IMG, corner, Point::new(400.0, 300.0));
            assert_eq!((out[0], out[1]), (IMG[0], IMG[1]), "{corner:?} moved it");
        }
    }

    #[test]
    fn a_degenerate_image_is_left_alone() {
        let zero = [10.0, 10.0, 0.0, 0.0];
        assert_eq!(
            proportional_resize(zero, BOTTOM_RIGHT, Point::new(99.0, 99.0)),
            zero
        );
    }

    // ── grabbing a grip ─────────────────────────────────────────────────

    #[test]
    fn each_corner_is_grabbable_from_either_side_of_its_edge() {
        // A grip straddles the corner, so it must answer both from inside the
        // picture and from just outside it — the outside half is the reason the
        // engine's own hit-test cannot do this job.
        for (fx, fy) in super::super::paint::RESIZE_CORNERS {
            let (cx, cy) = (IMG[0] + IMG[2] * fx, IMG[1] + IMG[3] * fy);
            for (dx, dy) in [(0.0, 0.0), (-4.0, -4.0), (4.0, 4.0)] {
                assert_eq!(
                    handle_at(IMG, Point::new(cx + dx, cy + dy)),
                    Some((fx, fy)),
                    "corner {fx},{fy} missed at offset {dx},{dy}"
                );
            }
        }
    }

    #[test]
    fn the_middle_of_the_picture_grabs_nothing() {
        // Otherwise clicking a picture to select it would start a resize.
        let centre = Point::new(IMG[0] + IMG[2] / 2.0, IMG[1] + IMG[3] / 2.0);
        assert_eq!(handle_at(IMG, centre), None);
    }

    #[test]
    fn a_press_well_clear_of_a_corner_grabs_nothing() {
        // Twenty pixels out on both axes: prose beside the picture must stay
        // ordinary prose.
        assert_eq!(
            handle_at(IMG, Point::new(IMG[0] - 20.0, IMG[1] - 20.0)),
            None
        );
        assert_eq!(
            handle_at(
                IMG,
                Point::new(IMG[0] + IMG[2] + 20.0, IMG[1] + IMG[3] + 20.0)
            ),
            None
        );
    }

    #[test]
    fn the_edges_between_corners_grab_nothing() {
        // Corners only — this resize keeps proportions, so an edge grip would
        // promise a stretch it will not perform.
        let mid_top = Point::new(IMG[0] + IMG[2] / 2.0, IMG[1]);
        let mid_left = Point::new(IMG[0], IMG[1] + IMG[3] / 2.0);
        assert_eq!(handle_at(IMG, mid_top), None);
        assert_eq!(handle_at(IMG, mid_left), None);
    }
}
