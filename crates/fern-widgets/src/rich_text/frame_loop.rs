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
//! flight). The caller re-arms the frame request so FernUI stays
//! draw-when-needed: an unfocused, idle viewer stops pumping as soon
//! as `tick()` returns `false`.

use super::policy::CaretPolicy;
use super::state::{DragState, EditorState};

pub(crate) const SCROLLBAR_THICKNESS: f32 = 12.0;
/// Caret blink half-period (time between on/off toggles), measured
/// against wall-clock time so the visible cadence is independent of
/// frame pacing. 500 ms ≈ a full 1 s blink cycle, the common editor
/// default.
pub(crate) const CARET_BLINK_INTERVAL: f32 = 0.5;

/// Debounce window for coalesced signal emission (text_changed,
/// format_changed, undo_redo_changed). Matches the godot reference
/// (rich_text_edit.rs:401). Non-debounced events — document_loaded,
/// selection_changed, caret_changed — fire immediately.
pub(crate) const DEBOUNCE_WINDOW_SECS: f32 = 0.150;

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
        // one exists.
        let _ = state.cursor.insert_text(&batch);
        state.pending_text_changed = true;
    }

    // Step 2: drain the per-widget event queue populated by on_change.
    let (had_events, single_pos) = state.drain_events();

    // Caret blink driven by wall-clock time. Every tick we compare
    // `Instant::now()` with `blink_last_toggle` and toggle whenever
    // the elapsed time exceeds `CARET_BLINK_INTERVAL`. If frame
    // pumps are irregular the blink catches up on the next tick —
    // the visible cadence stays locked to real seconds regardless
    // of how the frame scheduler behaves.
    let blinking_active =
        state.has_focus && matches!(state.policy.caret_policy, CaretPolicy::Blinking);
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
        // Schedule a one-shot wake-up at the next blink toggle so the
        // event loop can idle in `WaitUntil` between toggles instead
        // of being forced into `Poll` mode. Without this, returning
        // `true` from this tick keeps `any_frame_requested=true` which
        // burns CPU pumping frames at the OS's max rate (observed
        // ~90 fps) between the 500 ms toggle events.
        if let (Some(last), Some(wake)) = (state.blink_last_toggle, &state.frame_wake_at) {
            let next = last + interval;
            let merged = match wake.get() {
                Some(existing) if existing <= next => existing,
                _ => next,
            };
            wake.set(Some(merged));
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
    // caret-moved concern, owned by `keyboard::handle_key` after
    // arrow/Home/End/PageUp/PageDown navigation.
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
        if (new_y - state.scroll_y.get()).abs() > f32::EPSILON {
            state.scroll_y.set(new_y);
        }
    }

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
    state.debounce_timer += delta;
    let debounce_ready = state.debounce_timer >= DEBOUNCE_WINDOW_SECS;
    if debounce_ready {
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
        state.debounce_timer = 0.0;
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
