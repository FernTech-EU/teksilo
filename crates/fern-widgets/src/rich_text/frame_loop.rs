//! The editor's per-frame effect body.
//!
//! Follows the numbered steps in §27.10.3 of the architecture doc. For
//! the M8a read-only preset we skip everything that applies only to
//! editing (pending_chars flush, debounced text_changed signal,
//! drag-select auto-scroll); those will layer on in M8b without
//! modifying the read-only path.
//!
//! Returns `true` if the state has pending work that needs another
//! frame (document events still arriving, content not yet marked
//! clean). The caller re-arms the frame request so FernUI stays
//! draw-when-needed: an unfocused, idle viewer stops pumping as soon
//! as `tick()` returns `false`.

use super::policy::CaretPolicy;
use super::state::EditorState;

pub(crate) const SCROLLBAR_THICKNESS: f32 = 12.0;
/// Caret blink half-period (time between on/off toggles), measured
/// against wall-clock time so the visible cadence is independent of
/// frame pacing. 500 ms ≈ a full 1 s blink cycle, the common editor
/// default.
pub(crate) const CARET_BLINK_INTERVAL: f32 = 0.5;

/// Run one frame-tick step. `delta` is the time since the previous
/// tick in seconds (clamped by the tree). Returns `true` when another
/// frame is needed (the editor has ongoing work).
pub(crate) fn tick(state: &mut EditorState, _delta: f32) -> bool {
    // Step 2: drain the per-widget event queue populated by on_change.
    let (had_events, single_pos) = state.drain_events();

    // Caret blink driven by wall-clock time. Every tick we compare
    // `Instant::now()` with `blink_last_toggle` and toggle whenever
    // the elapsed time exceeds `CARET_BLINK_INTERVAL`. If frame
    // pumps are irregular the blink catches up on the next tick —
    // the visible cadence stays locked to real seconds regardless
    // of how the frame scheduler behaves.
    let blinking_active = state.has_focus
        && matches!(state.policy.caret_policy, CaretPolicy::Blinking);
    if blinking_active {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_secs_f32(CARET_BLINK_INTERVAL);
        match state.blink_last_toggle {
            None => {
                state.blink_last_toggle = Some(now);
            }
            Some(last) if now.saturating_duration_since(last) >= interval => {
                state.blink_last_toggle = Some(now);
                let was = state.caret_visible.get();
                state.caret_visible.set(!was);
            }
            _ => {}
        }
    } else {
        state.blink_last_toggle = None;
        if matches!(state.policy.caret_policy, CaretPolicy::Blinking) && !state.has_focus {
            // Unfocused: caret off.
            if state.caret_visible.get() {
                state.caret_visible.set(false);
            }
        } else if matches!(state.policy.caret_policy, CaretPolicy::StaticVisible)
            && !state.caret_visible.get()
        {
            state.caret_visible.set(true);
        }
    }

    // Step 3: word-wrap viewport pre-adjustment.
    //
    // If a vertical scrollbar is currently visible, reserve space for
    // it so the word-wrap layout produces the right line breaks. The
    // decision is one frame stale (it looks at last frame's max_scroll),
    // which is exactly the converging behaviour described in §27.10.5.
    let viewport_width = state.viewport_width;
    let viewport_height = state.viewport_height;

    if viewport_width > 0.0 && viewport_height > 0.0 {
        let v_visible = state.max_scroll_y.get() > 0.0;
        let h_visible = state.max_scroll_x.get() > 0.0;
        let effective_w = if v_visible {
            (viewport_width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            viewport_width
        };
        let effective_h = if h_visible {
            (viewport_height - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            viewport_height
        };

        // set_viewport in text-typeset is cheap; call unconditionally
        // so zoom changes and resizes both propagate.
        state.engine.set_viewport(effective_w, effective_h);
    }

    // Step 4: apply layout strategy. Gated on a non-zero viewport
    // because the first tree-level `tree.layout()` call fires the
    // frame-tick effect *before* `paint()` has had a chance to record
    // the widget bounds. Running `layout_full` with a zero viewport
    // produces a degenerate layout that text-typeset's glyph cache
    // carries into subsequent renders. paint() owns the first layout
    // pass in M8a; the tick only layouts on later edits.
    let viewport_ready = viewport_width > 0.0 && viewport_height > 0.0;
    if viewport_ready && state.needs_full_layout {
        let flow = state.document.snapshot_flow();
        state.engine.layout_full(&flow);
        state.needs_full_layout = false;
        state.last_relayout_block_id = None;
        state.content_dirty = true;
    } else if viewport_ready && let Some(pos) = single_pos {
        // Incremental path. Falls back to layout_full internally on
        // the first call (subtle-correctness item 25).
        match state.engine.relayout_block_snapshot(&state.document, pos) {
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
    // caret-moved concern, so the key handler owns it (see
    // `on_key` in `widget.rs`).
    if state.engine.has_full_layout() {
        let caret_on = state.caret_visible.get() && state.has_focus;
        let cursor_display = fern_text::CursorDisplay {
            position: state.cursor.position(),
            anchor: state.cursor.anchor(),
            visible: caret_on,
            selected_cells: Vec::new(),
        };
        state.engine.set_cursor(&cursor_display);
    }

    // Step 7: update scroll signals from current content metrics.
    let content_height = state.engine.content_height();
    let max_content_width = state.engine.max_content_width();
    let zoom = state.engine.zoom();

    let max_y = (content_height * zoom - viewport_height).max(0.0);
    let max_x = (max_content_width * zoom - viewport_width).max(0.0);
    state.max_scroll_y.set(max_y);
    state.max_scroll_x.set(max_x);

    let ratio_y = if content_height > 0.0 && viewport_height > 0.0 {
        (viewport_height / (content_height * zoom)).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let ratio_x = if max_content_width > 0.0 && viewport_width > 0.0 {
        (viewport_width / (max_content_width * zoom)).clamp(0.0, 1.0)
    } else {
        1.0
    };
    state.viewport_ratio_y.set(ratio_y);
    state.viewport_ratio_x.set(ratio_x);

    // Clamp scroll offsets to the fresh maxima (subtle-correctness #2
    // and #5): deleting text must not leave us scrolled past the end.
    let clamped_y = state.scroll_y.get().clamp(0.0, max_y);
    if (clamped_y - state.scroll_y.get()).abs() > f32::EPSILON {
        state.scroll_y.set(clamped_y);
    }
    let clamped_x = state.scroll_x.get().clamp(0.0, max_x);
    if (clamped_x - state.scroll_x.get()).abs() > f32::EPSILON {
        state.scroll_x.set(clamped_x);
    }

    // Step 9: return whether more work is pending. Blinking keeps
    // the loop running for as long as the widget is focused; a rapid
    // burst of document changes keeps pumping until the queue drains;
    // otherwise the tree goes idle.
    had_events || blinking_active
}
