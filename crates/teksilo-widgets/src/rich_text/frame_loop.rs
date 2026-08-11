// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The editor's per-frame effect body.
//!
//! Follows the numbered steps in §27.10.3 of the architecture doc.
//! Runs on every frame the widget tree was asked to pump (see
//! `BuildContext::frame_tick`). Steps 1–9 mirror the godot reference's
//! `_process` ordering: flush pending typed characters, drain queued
//! document events, blink the caret, reserve viewport width for a
//! visible scroll bar, apply the full-vs-incremental relayout
//! strategy, update the typesetter's cursor display, publish scroll
//! metrics, apply drag-select auto-scroll velocity, and drain the
//! 150 ms debounce window for `can_undo`/`can_redo`/text-changed
//! signals.
//!
//! Returns `true` if the state has pending work that needs another
//! frame (document events still arriving, caret blinking in a focused
//! editor, drag-select auto-scroll active, debounced signals in
//! flight). The caller re-arms the frame request so Teksilo stays
//! draw-when-needed: an unfocused, idle viewer stops pumping as soon
//! as `tick()` returns `false`.

use super::state::{DragState, EditorState};
use crate::common::editor_runtime::ScrollMetrics;

pub(crate) const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Run one frame-tick step. `delta` is the time since the previous
/// tick in seconds (clamped by the tree). Returns `true` when another
/// frame is needed (the editor has ongoing work).
pub(crate) fn tick(state: &mut EditorState, delta: f32) -> bool {
    // Step 1 (NEW for M8b): flush pending_chars BEFORE draining events.
    // Batching keystrokes into a single `insert_text` makes the
    // subsequent `drain_events` see one `ContentsChanged` instead of
    // N, which matters for the incremental-relayout code path and
    // for debounced `text_changed` coalescing.
    if !state.pending_chars.is_empty() {
        let batch = std::mem::take(&mut state.pending_chars);
        // Replacing an active selection with typed input is the
        // expected editor behaviour (QTextEdit / every major editor).
        // `insert_text` already removes the selection first when
        // one exists — which is precisely why a forward-only filter has
        // to collapse the selection here first, one layer above the
        // insert that would otherwise swallow it.
        super::keyboard::collapse_selection_before_insert(state);
        let _ = state.cursor.insert_text(&batch);
        state.pending_text_changed = true;
    }

    // Step 2: drain the per-widget event queue populated by on_change.
    let (mut had_events, mut single_pos) = state.drain_events();

    // Step 2b: the ambient caret band (the sentence or paragraph being written in).
    //
    // Resolved from `state.cursor`, which is authoritative right now — the caret *signal* lags a
    // frame behind a just-typed character — and re-resolved every tick rather than on caret
    // moves, because an edit *ahead* of the band moves it without the caret moving at all.
    //
    // **Before the layout step on purpose.** The push emits `HighlightPaintChanged`, and the
    // layout below snapshots the document. Resolving the band *after* the layout let
    // `layout_full` bake the PREVIOUS band and then let step 4b discard the correction as
    // redundant (it skips whenever a full layout ran this frame), so a block-count-changing edit
    // — pressing Enter, pasting paragraphs, undo — left the band on the old extent until some
    // unrelated event happened to pump another recolor.
    if state.caret_highlight.is_some() {
        // A selection suppresses the band as surely as losing focus does: it already says where
        // the writer is, more precisely, and the band would otherwise chase the selection's
        // moving end from sentence to sentence underneath it.
        let active = state.has_focus && !state.cursor.has_selection();
        let caret = state.cursor.position();
        let band = state.caret_highlight.as_ref().expect("checked above");
        let mut changed = false;
        if state.caret_highlight_active != active {
            changed |= band.set_active(active);
        }
        changed |= band.refresh(caret);
        state.caret_highlight_active = active;
        if changed {
            // Drain the event the push just queued, so this tick's layout/recolor sees it.
            let (more, more_pos) = state.drain_events();
            had_events |= more;
            // The band's own push is paint-only, so it never reports a block position. Anything
            // that does arrive here came from another view of the shared document between the
            // two drains, and cannot be merged with the first drain's single-block answer —
            // take the whole-document path rather than relayout the wrong block.
            if more_pos.is_some() {
                if single_pos.is_none() {
                    single_pos = more_pos;
                } else if more_pos != single_pos {
                    state.needs_full_layout = true;
                }
            }
        }
    }

    // Caret blink — see `common::editor_runtime::CaretBlink`. Driven by
    // wall-clock time (not accumulated delta) so the cadence stays locked to
    // real seconds under irregular frame pacing, and gated on
    // `has_focus && window_active` because a caret in an inactive window is
    // hidden on every desktop platform.
    let caret_active = state.has_focus && state.window_active;
    let policy = state.policy.caret_policy;
    let caret_visible = state.caret_visible.clone();
    let wake = state.frame_wake_at.clone();
    state
        .blink
        .tick(policy, caret_active, &caret_visible, wake.as_ref());

    // Step 3: forward the viewport to the typesetter.
    //
    // Overlay scrollbars (the editor's default) float on top of the
    // content and reserve no gutter — the wrap width equals the full
    // viewport width. Before overlay scrollbars were wired in, this
    // step shaved off SCROLLBAR_THICKNESS whenever `max_scroll_y > 0`,
    // which caused a one-frame-late re-wrap on the first edit (full
    // width on the initial paint → reduced width on the next tick after
    // max_scroll crossed zero → all blocks visibly shifted on the first
    // keystroke as `relayout_block_snapshot` ran at the new width).
    let viewport_width = state.viewport_width;
    let viewport_height = state.viewport_height;

    if viewport_width > 0.0 && viewport_height > 0.0 {
        // set_viewport in text-typeset is cheap; call unconditionally
        // so zoom changes and resizes both propagate.
        state.engine.set_viewport(viewport_width, viewport_height);
    }

    // Step 4: apply layout strategy. Gated on a non-zero viewport
    // because the first tree-level `tree.layout()` call fires the
    // frame-tick effect *before* `paint()` has had a chance to record
    // the widget bounds. Running `layout_full` with a zero viewport
    // produces a degenerate layout that text-typeset's glyph cache
    // carries into subsequent renders. paint() owns the first layout
    // pass in M8a; the tick only layouts on later edits.
    let viewport_ready = viewport_width > 0.0 && viewport_height > 0.0;
    // Ownership-stale check: two rich-text widgets viewing the same
    // document share a `TypesetterBridge` so glyphs end up in the
    // same GPU atlas, but they each own independent flow-layout
    // state. `has_full_layout()` returns `false` when the bridge
    // now belongs to another engine — in that case we must re-run
    // `layout_full` before reading `content_height` /
    // `max_content_width` below, otherwise we read the other
    // widget's metrics and compute a wrong `max_scroll_y`.
    let layout_stale = viewport_ready && !state.engine.has_full_layout();
    if viewport_ready && (state.needs_full_layout || layout_stale) {
        let flow = state.flow_snapshot();
        state.engine.layout_full(&flow);
        state.needs_full_layout = false;
        state.last_relayout_block_id = None;
        state.content_dirty = true;
        // Tell paint() to use RenderChoice::Full this frame —
        // `needs_full_layout` is cleared above so paint can no longer
        // infer the full-layout-just-happened condition from it.
        state.pending_full_render = true;
    } else if viewport_ready && let Some(pos) = single_pos {
        // Incremental path. Falls back to layout_full internally on
        // the first call (subtle-correctness item 25).
        //
        // Thread the SAME per-view mask the full-layout path uses (`state.flow_snapshot()` via
        // `effective_mask`). This is the second, gateway-bypassing snapshot path: a mask
        // applied only to `flow_snapshot()` would silently vanish on the next keystroke's
        // incremental relayout, taking two panes' divergent find highlights with it.
        let mask = state.effective_mask();
        match state
            .engine
            .relayout_block_snapshot(&state.document, pos, &mask)
        {
            Ok(block_id) => {
                state.last_relayout_block_id = Some(block_id);
                state.content_dirty = true;
            }
            Err(_) => {
                // Block vanished between the event firing and now:
                // fall back to a full layout next frame.
                state.needs_full_layout = true;
            }
        }
    }

    // Step 4b: paint-only highlight recolor. A `HighlightPaintChanged` event
    // only changes colors, so re-derive the cached layout's colors WITHOUT
    // reshaping/reflowing, then force a full re-render (re-bakes glyph &
    // decoration colors from the cached layout — no shaping). If a full layout
    // already ran this frame it re-baked from the fresh snapshot's paint spans,
    // so the recolor would be redundant — just clear the flag.
    if state.pending_recolor {
        if !state.needs_full_layout
            && !state.pending_full_render
            && viewport_ready
            && state.engine.has_full_layout()
        {
            // Prefer recoloring the one block the change actually covers. `flow_snapshot()`
            // materializes the text and fragments of EVERY block, so on a long scene the
            // whole-document path costs more per keystroke than the edit itself — and a caret
            // band, a find match and a spell squiggle all push on nearly every keystroke.
            // `None` (an unknown extent, or one straddling blocks) falls back to the full pass.
            let mask = state.effective_mask();
            let scoped = state
                .pending_recolor_range
                .and_then(|(position, length)| {
                    state.engine.apply_paint_highlights_for_range(
                        &state.document,
                        position,
                        length,
                        &mask,
                    )
                })
                .is_some();
            if !scoped {
                let flow = state.flow_snapshot();
                state.engine.apply_paint_highlights(&flow);
            }
            state.content_dirty = true;
            state.pending_full_render = true;
        }
        state.pending_recolor = false;
        state.pending_recolor_range = None;
    }

    // Step 5: update cursor display on the typesetter — but only if
    // the engine already has a full layout. Calling `set_cursor`
    // before `layout_full` poisons the typesetter's render state
    // (observed: subsequent `render()` calls return zero glyphs even
    // after a correct layout).
    //
    // Note: we deliberately do **not** call `ensure_caret_visible`
    // here. If the caret is at document start and the user scrolls
    // down with the wheel, the caret falls outside the viewport;
    // `ensure_caret_visible` would then pull the scroll back to 0
    // every tick, fighting the wheel. Auto-scroll-to-caret is a
    // caret-moved concern, owned by `keyboard::handle_key` after
    // arrow/Home/End/PageUp/PageDown navigation.
    if state.engine.has_full_layout() {
        let caret_on = state.caret_visible.get() && state.has_focus;
        let cursor_display = teksilo_text::CursorDisplay {
            position: state.cursor.position(),
            anchor: state.cursor.anchor(),
            affinity: state.cursor_affinity,
            visible: caret_on,
            selected_cells: Vec::new(),
        };
        state.engine.set_cursor(&cursor_display);
    }

    // Step 7: update scroll signals from current content metrics.
    // See `common::editor_runtime::ScrollMetrics` — the publish step guards
    // every Signal::set with a change-check (Signal::set has no internal
    // PartialEq skip, so an unchanged write still fans out to every scroll bar
    // and layout listener; that showed as ~5% of frame CPU) and clamps the
    // live offsets to the fresh maxima.
    let metrics = ScrollMetrics::compute(
        state.engine.content_height(),
        state.engine.max_content_width(),
        viewport_width,
        viewport_height,
    );
    let max_y = metrics.max_y;

    // Drag-select auto-scroll. While the user is dragging near the
    // top or bottom viewport edge, `mouse::handle_pointer_event`
    // stores a per-second velocity on `drag_state`; the frame loop
    // applies it each tick so scrolling continues without requiring
    // further mouse motion. Matches godot rich_text_edit.rs:1812-1845.
    //
    // `drag_active` drives whether we keep the frame loop pumping.
    // We only return true when velocity is actually non-zero — a
    // user who holds the button but doesn't move should not consume
    // frame ticks. PointerMove itself calls `request_frame()` when
    // it enters the auto-scroll zone, so entering the zone restarts
    // the loop from idle.
    let mut drag_active = false;
    if let DragState::Selecting {
        auto_scroll_v_per_s,
    } = state.drag_state
        && auto_scroll_v_per_s.abs() > 0.0
    {
        drag_active = true;
        let new_y = (state.scroll_y.get() + auto_scroll_v_per_s * delta).clamp(0.0, max_y);
        state.scroll_y.set_if_changed(new_y);
    }

    // Publish limits + ratios and clamp the live offsets (subtle-correctness
    // #2 and #5): deleting text must not leave us scrolled past the end.
    //
    // Runs after the drag step so the clamp stays last, as it was before the
    // publish moved into `ScrollMetrics`. The drag block reads the local
    // `max_y` rather than the signal, so it cannot see the reorder, and
    // end-of-tick values are unchanged. It is not *entirely* invisible though:
    // `Signal::set` fans out synchronously, so a `max_scroll_y` observer that
    // reads `scroll_y` back now sees the post-drag value where it used to see
    // the pre-drag one. Today the only observer is a Relayout binding that
    // just marks dirty; a future synchronous observer would inherit this.
    metrics.publish(
        &state.scroll_x,
        &state.scroll_y,
        &state.max_scroll_x,
        &state.max_scroll_y,
        &state.viewport_ratio_x,
        &state.viewport_ratio_y,
    );

    // Step 8 (NEW for M8b): debounce drain. Coalesces rapid bursts
    // of text/format/undo-redo change notifications into one
    // application-visible command per 150 ms window.
    //
    // Command emission from within the frame-tick effect is currently
    // out of reach — the effect closure only receives `&delta`, not
    // an `EventContext`. So we publish the debounced state through
    // the reactive signals (`can_undo`, `can_redo`, `document_version`)
    // and let toolbars observe those directly. The typed-command
    // emission (`on_text_changed`, `on_format_changed`,
    // `on_undo_redo_changed`) is Phase B and will thread a
    // command queue through the effect.
    if state.debounce.tick(delta) {
        if state.pending_text_changed || state.pending_format_changed {
            // The `document_version` signal was bumped inside
            // `drain_events` already — toolbars bound to that get
            // their update. Just clear the flags here so the next
            // window starts fresh.
            state.pending_text_changed = false;
            state.pending_format_changed = false;
        }
        if let Some((cu, cr)) = state.pending_undo_redo.take() {
            if state.can_undo.get() != cu {
                state.can_undo.set(cu);
            }
            if state.can_redo.get() != cr {
                state.can_redo.set(cr);
            }
        }
    }
    let debounce_work_pending = state.pending_text_changed
        || state.pending_format_changed
        || state.pending_undo_redo.is_some();

    // Step 9: return whether more work is pending. A rapid burst of
    // document changes keeps pumping until the queue drains;
    // debounced signals in flight keep pumping until they publish;
    // an active drag-select auto-scroll keeps the loop pumping so
    // the scroll rate is delta-based and independent of the user's
    // mouse motion; otherwise the tree goes idle.
    //
    // Blinking is deliberately NOT in this list. The blink path
    // above schedules a one-shot wake-up via `frame_wake_at` so the
    // event loop can idle in `WaitUntil` between 500 ms toggles
    // instead of pumping at the OS's max rate.
    had_events || debounce_work_pending || drag_active
}
