// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code editor's per-frame step.
//!
//! Runs on every frame the tree was asked to pump, and returns whether it needs
//! another. That return value is the whole contract: Bastyde is draw-when-needed,
//! so an idle unfocused editor must stop asking for frames or it burns a core
//! doing nothing.
//!
//! Order matters and is not arbitrary:
//!
//! 1. Flush typed characters *before* draining events, so a burst of keystrokes
//!    becomes one `insert_text` and therefore one `ContentsChanged` — which is
//!    what keeps relayout O(burst) instead of O(keystrokes).
//! 2. Drain document events, learning whether one block changed or many.
//! 3. Blink.
//! 4. Lay out: full, one-block incremental, or not at all.
//! 5. Recolour, if a highlighter repainted without the text changing.
//! 6. Publish the carets to the engine.
//! 7. Publish scroll metrics, apply drag auto-scroll.
//! 8. Drain the debounce window.

use super::state::{CodeEditorState, DragState};
use crate::common::editor_runtime::ScrollMetrics;

/// Run one frame step. `delta` is seconds since the previous tick. Returns
/// `true` when the editor still has work and needs another frame.
pub(crate) fn tick(state: &mut CodeEditorState, delta: f32) -> bool {
    // 1. Flush typed characters as one insert. Typing over a selection replaces
    //    it, which `insert_text` already does.
    if !state.pending_chars.is_empty() {
        let batch = std::mem::take(&mut state.pending_chars);
        insert_at_every_caret(state, &batch);
        state.pending_text_changed = true;
    }

    // 2. Drain.
    let (had_events, single_pos) = state.drain_events();

    // 3. Blink. Gated on focus AND window activity — a caret in an inactive
    //    window is hidden on every desktop platform.
    let caret_active = state.has_focus && state.window_active;
    let policy = state.policy.caret_policy;
    let caret_visible = state.caret_visible.clone();
    let wake = state.frame_wake_at.clone();
    state
        .blink
        .tick(policy, caret_active, &caret_visible, wake.as_ref());

    let viewport_width = state.viewport_width;
    let viewport_height = state.viewport_height;
    let viewport_ready = viewport_width > 0.0 && viewport_height > 0.0;
    if viewport_ready {
        state.engine.set_viewport(viewport_width, viewport_height);
    }

    // 4. Layout. Gated on a real viewport: the tree's first `layout()` fires
    //    this effect before any paint has recorded bounds, and laying out at a
    //    zero viewport poisons the engine's glyph cache with a degenerate
    //    result that later renders inherit.
    //
    //    `has_full_layout()` also goes false when the shared typesetter now
    //    belongs to another engine — two editors over one document share the
    //    atlas but own independent flow state — so re-check it rather than
    //    trusting our own flag, or we read the *other* editor's content height
    //    below and compute a wrong max_scroll.
    let layout_stale = viewport_ready && !state.engine.has_full_layout();
    if viewport_ready && (state.needs_full_layout || layout_stale) {
        let flow = state.document.snapshot_flow();
        state.engine.layout_full(&flow);
        state.needs_full_layout = false;
        state.last_relayout_block_id = None;
        state.content_dirty = true;
        state.pending_full_render = true;
    } else if viewport_ready && let Some(pos) = single_pos {
        // Every highlight session: a code editor's highlighter *is* the point,
        // unlike a prose viewer that may want to render bare.
        let mask = bastyde_text::text_document::HighlightMask::all();
        match state
            .engine
            .relayout_block_snapshot(&state.document, pos, &mask)
        {
            Ok(block_id) => {
                state.last_relayout_block_id = Some(block_id);
                state.content_dirty = true;
            }
            Err(_) => {
                // The block vanished between the event and now: fall back to a
                // full layout next frame rather than rendering a stale one.
                state.needs_full_layout = true;
            }
        }
    }

    // 5. Recolour. A highlighter that repainted without editing changed only
    //    colours, and a colour cannot change a glyph advance — so re-derive the
    //    cached layout's colours and re-render, without reshaping. Skipped if a
    //    full layout already ran this frame, which re-baked them anyway.
    if state.pending_recolor {
        if !state.needs_full_layout
            && !state.pending_full_render
            && viewport_ready
            && state.engine.has_full_layout()
        {
            let flow = state.document.snapshot_flow();
            state.engine.apply_paint_highlights(&flow);
            state.content_dirty = true;
            state.pending_full_render = true;
        }
        state.pending_recolor = false;
    }

    // 6. Publish the carets. Only once a layout exists: setting a cursor on an
    //    unlaid-out engine poisons its render state (subsequent renders return
    //    no glyphs even after a correct layout).
    //
    //    Deliberately no `ensure_caret_visible` here. If the user scrolls away
    //    from the caret with the wheel, doing it every tick would drag the view
    //    straight back and fight them. Chasing the caret is a caret-*moved*
    //    concern and belongs to the keyboard handler.
    if state.engine.has_full_layout() {
        let caret_on = state.caret_visible.get() && state.has_focus && state.window_active;
        let affinity = state.cursor_affinity;
        let cursors: Vec<bastyde_text::CursorDisplay> = state
            .all_carets()
            .map(|c| bastyde_text::CursorDisplay {
                position: c.position(),
                anchor: c.anchor(),
                affinity,
                visible: caret_on,
                selected_cells: Vec::new(),
            })
            .collect();
        state.engine.set_cursors(&cursors);
    }

    // 7. Scroll metrics.
    let metrics = ScrollMetrics::compute(
        state.engine.content_height(),
        state.engine.max_content_width(),
        viewport_width,
        viewport_height,
    );

    // Drag-select auto-scroll: the pointer handler stored a velocity when the
    // pointer neared an edge; applying it per-tick is what lets the selection
    // keep growing while the pointer is held still. Only a non-zero velocity
    // keeps the loop pumping — a held button with no motion must not.
    let mut drag_active = false;
    if let DragState::Selecting {
        auto_scroll_v_per_s,
    } = state.drag_state
        && auto_scroll_v_per_s.abs() > 0.0
    {
        drag_active = true;
        let new_y = (state.scroll_y.get() + auto_scroll_v_per_s * delta).clamp(0.0, metrics.max_y);
        state.scroll_y.set_if_changed(new_y);
    }

    // Publishes the limits and clamps the live offsets. After the drag step so
    // the clamp stays last: deleting text shrinks max_y, and an offset left
    // past it would park the view beyond the end of the document.
    metrics.publish(
        &state.scroll_x,
        &state.scroll_y,
        &state.max_scroll_x,
        &state.max_scroll_y,
        &state.viewport_ratio_x,
        &state.viewport_ratio_y,
    );

    // 8. Debounce. Coalesces an edit burst into one toolbar update per window.
    if state.debounce.tick(delta) {
        state.pending_text_changed = false;
        if let Some((cu, cr)) = state.pending_undo_redo.take() {
            state.can_undo.set_if_changed(cu);
            state.can_redo.set_if_changed(cr);
        }
    }
    let debounce_pending = state.pending_text_changed || state.pending_undo_redo.is_some();

    // Blinking is deliberately absent from this list: it schedules a one-shot
    // wake-up instead, so the loop can idle between toggles rather than pump at
    // the display's rate to catch a transition that happens twice a second.
    had_events || debounce_pending || drag_active
}

/// Insert `text` at every caret.
///
/// Applied back-to-front so that each insertion cannot invalidate the positions
/// of the carets not yet handled: inserting at offset 10 shifts everything after
/// it, so an ascending walk would have every later caret land N characters too
/// early. Descending, each caret's position is still valid when its turn comes.
///
/// The whole batch is one undo step, which is what a user means by "undo my
/// typing" when they typed into three places at once.
pub(crate) fn insert_at_every_caret(state: &mut CodeEditorState, text: &str) {
    // Enforce the no-two-carets-at-one-offset invariant *before* inserting.
    // Carets collide by ordinary use — two on one line both pressing Home land
    // on the same column — and two stacked carets each insert the character,
    // so the user gets "ZZ" for one keypress. Merging only after a *move* is
    // not enough: a caret set programmatically or by alt-click never moved.
    state.merge_collided_carets();

    if state.extra_carets.is_empty() {
        let _ = state.cursor.insert_text(text);
        return;
    }

    state.cursor.begin_edit_block();
    // Collect and sort descending. The primary caret is in the set: it is not
    // special for *editing*, only for reporting.
    let mut ordered: Vec<usize> = (0..=state.extra_carets.len()).collect();
    ordered.sort_by_key(|&i| std::cmp::Reverse(caret_position(state, i)));
    for i in ordered {
        let cursor = caret_mut(state, i);
        let _ = cursor.insert_text(text);
    }
    state.cursor.end_edit_block();
}

/// Position of caret `i`, where 0 is the primary.
fn caret_position(state: &CodeEditorState, i: usize) -> usize {
    if i == 0 {
        state.cursor.position()
    } else {
        state.extra_carets[i - 1].position()
    }
}

/// Caret `i` by index, where 0 is the primary.
fn caret_mut(
    state: &mut CodeEditorState,
    i: usize,
) -> &mut bastyde_text::text_document::TextCursor {
    if i == 0 {
        &mut state.cursor
    } else {
        &mut state.extra_carets[i - 1]
    }
}
