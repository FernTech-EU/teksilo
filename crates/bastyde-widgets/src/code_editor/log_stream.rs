// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The streaming machinery behind [`LogView`](super::LogView).
//!
//! A log view is the code editor's core turned inside out: instead of a bounded
//! document the user edits, it is an unbounded one the *program* appends to and
//! the user only reads. That one difference — content arrives faster than a
//! person types, forever — is why it cannot share the editor's frame step.
//!
//! Two costs would otherwise grow without bound, and this module is the answer
//! to each:
//!
//! - **Laying out every line.** A fully-shaped 100 000-line buffer costs ~623 MB;
//!   a viewport-sized window costs ~4 MB (`text-typeset`'s
//!   `streaming-baseline.md`). So the view never lays out the whole document —
//!   [`ensure_window`] shapes only the rows the viewport can show, via
//!   [`RichTextEngine::layout_window_from_snapshots`](bastyde_text::RichTextEngine::layout_window_from_snapshots),
//!   and the scrollbar spans the rest arithmetically.
//! - **Relaying out on every append.** The editor's `drain_events` maps a block
//!   count change to a full relayout — correct for editing, ruinous for a line
//!   a millisecond. Here `drain_events` is told the state is streaming (via
//!   [`CodeEditorState::is_streaming`]) and sets a *re-window* flag instead, and
//!   [`tick`] batches a frame's worth of arrivals into one `append_lines`.
//!
//! Following the tail is deliberately **not** a mode flag that fights the user.
//! It is *derived from scroll position*: the view sticks to the bottom only
//! while it is already at the bottom, so scrolling up to read history pauses it
//! and scrolling back resumes it — the behaviour of every terminal, and the one
//! that composes correctly with a stray key or click nudging the viewport.

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use bastyde_core::Signal;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::widget::EventContext;
use bastyde_text::text_document::{MoveMode, SelectionType};
use bastyde_tokens::Color;

use super::state::{CodeEditorState, DragState, SharedState};
use crate::common::editor_runtime::ScrollMetrics;

/// A per-line severity classifier the application supplies: given a line's text,
/// the colour to paint it (an error line red), or `None` to leave it in the
/// document's own colour. Language-agnostic — the view knows how to colour a
/// line, the application knows what an error line looks like.
pub(crate) type SeverityFn = Rc<dyn Fn(&str) -> Option<Color>>;

/// Extra rows shaped above and below the strictly-visible range, so a small
/// scroll reveals already-shaped rows rather than a one-frame blank band.
const OVERSCAN_ROWS: usize = 8;

/// How close to the bottom still counts as "at the bottom" for tail-following,
/// in logical pixels — a hair of slack so sub-pixel scroll maxima don't read as
/// "scrolled up" and silently stop the follow.
const FOLLOW_EPSILON: f32 = 1.5;

/// The largest slack band eviction amortises over. `truncate_front` is
/// O(remaining), so trimming one line per append would pay that cost every
/// frame; overshooting by a batch amortises it. The actual band scales down with
/// the cap (see `enforce_scrollback`) so a small cap is still honoured tightly.
const EVICT_SLACK: usize = 256;

/// How far the cached anchor may be *behind* the target row before a fresh O(n)
/// locate beats walking forward one O(log n) snapshot at a time. Chosen near the
/// `n / log n` crossover for the target scale (~100k rows).
const WALK_LIMIT: usize = 4096;

/// Everything a [`LogView`](super::LogView) needs on top of the shared editor
/// state: the append queue, the follow/scrollback policy, the severity
/// classifier, and the small caches that keep windowing O(window).
pub(crate) struct LogStreamState {
    /// Lines awaiting append, drained once per frame into a single
    /// `append_lines`. Batching a frame's arrivals into one call is what keeps a
    /// burst O(burst) rather than O(burst) relayouts. The `Arc<Mutex<…>>` is
    /// thread-safe storage, but the handle that fills it is UI-thread (`Rc`), so
    /// enqueue, drain, and every document mutation all run on the UI thread.
    pub pending: Arc<Mutex<VecDeque<String>>>,
    /// Whether appends stick the view to the bottom *when it is already there*.
    pub follow_enabled: bool,
    /// Maximum retained lines, or `None` for unbounded. Unbounded is memory-safe
    /// here: only the window is shaped, and the raw text in the rope is ~65 B a
    /// line. A cap bounds the rope too.
    pub scrollback_limit: Option<usize>,
    /// Per-line severity colour, or `None` for no colouring.
    pub severity: Option<SeverityFn>,
    /// The uniform row height, learned from the engine's line height once a
    /// viewport exists. `0.0` until then.
    pub row_height: f32,
    /// `(row, char position)` of the window's first row, cached across frames so
    /// a forward scroll chains snapshots from it (O(delta·log n)) instead of
    /// re-locating from scratch (O(n)). Dropped on eviction (which shifts
    /// positions) and on a reset.
    pub anchor: Option<(usize, usize)>,
    /// The `(first_row, count)` currently shaped, so an unchanged window is not
    /// needlessly re-shaped every frame.
    pub last_window: Option<(usize, usize)>,
    /// Set by `drain_events` (streaming branch) and eviction when the shaped
    /// window's *content* may have changed even though its range did not.
    pub needs_rewindow: bool,
    /// Whether nothing has been appended yet. The first append fills the
    /// document's initial empty block rather than adding after it, so a fresh
    /// log does not show a blank first line.
    pub pristine: bool,
    /// The authoritative line count. The document's `block_count` stat does not
    /// count its initial empty block (a fresh document reports zero blocks yet
    /// has one), so every count derived from it is off by one and would drop the
    /// last row and misplace the scrollbar. This is maintained directly from the
    /// appends and evictions instead, and published to `line_count`.
    pub total: usize,
    /// Bumped only when the *visible window* changes — the accessibility tree
    /// binds this at `AccessibilityOnly` instead of `scroll_y` / `document_version`
    /// so it re-walks (a whole-tree rebuild, since the framework has no per-widget
    /// a11y dirty tracking) only when the exposed lines actually change: a scroll
    /// crossing a row, a following-tail append, or an eviction. A pixel-scroll that
    /// stays on the same rows, or a tail append while scrolled away, does not. See
    /// [`bump_a11y_if_window_changed`].
    pub a11y_version: Signal<u64>,
    /// The `(first, count)` the a11y tree was last built for, to detect a real
    /// window change. Deliberately excludes `total`: the per-line "of N" count
    /// then lags a tail append while scrolled up, which refreshes on the next
    /// scroll — cheap staleness against a per-append whole-tree rebuild.
    pub last_a11y_sig: Option<(usize, usize)>,
}

impl LogStreamState {
    pub(crate) fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(VecDeque::new())),
            follow_enabled: true,
            scrollback_limit: None,
            severity: None,
            row_height: 0.0,
            anchor: None,
            last_window: None,
            needs_rewindow: true,
            pristine: true,
            total: 0,
            a11y_version: Signal::new(0),
            last_a11y_sig: None,
        }
    }
}

/// One frame step for a log view. Returns whether another frame is wanted.
///
/// Order matters: drain arrivals and enforce the cap *before* deciding whether
/// we were at the bottom, so the follow decision is made against the pre-growth
/// scroll maximum; window *after* the content settles; stick to the bottom
/// *after* the new maximum is known.
pub(crate) fn tick(st: &mut CodeEditorState, delta: f32) -> bool {
    // 0. Were we at the bottom? This MUST be read before anything moves
    //    `scroll_y` or `max_scroll_y` this frame — eviction (step 2) shifts
    //    `scroll_y` down, and reading `was_at_bottom` after that against the
    //    still-stale `max_scroll_y` would make a genuinely-following view look
    //    scrolled-up and silently, permanently stop following.
    let was_at_bottom = at_bottom(st);

    // 1. Drain a frame's worth of arrivals and apply them as one batch.
    let batch = drain_pending(st);
    let mut content_changed = !batch.is_empty();
    if content_changed {
        apply_appends(st, batch);
    }

    // 2. Enforce the scrollback cap (amortised), before windowing.
    if enforce_scrollback(st) {
        content_changed = true;
    }

    // 3. Drain document events. Streaming's branch in `drain_events` sets the
    //    re-window flag instead of forcing a full relayout, and still bumps the
    //    version signal so the accessibility tree re-walks.
    let (had_events, _) = st.drain_events();

    // Drain and discard the document's poll-event path too. We consume events
    // through the `on_change` subscription (the queue `drain_events` reads), but
    // the document only trims its internal pending-event buffer once *both* its
    // delivery paths have consumed — so a purely callback-driven view would leak
    // that buffer without end over a long stream. This drives the poll cursor
    // forward so the buffer trims. Cheap: the events are already handled.
    let _ = st.document.poll_events();

    // Publish the authoritative count (the document's stat undercounts its
    // initial block, so we own it here rather than trusting `drain_events`).
    let total = st.log.as_ref().map_or(0, |l| l.total);
    st.line_count.set_if_changed(total);

    let viewport_ready = st.viewport_width > 0.0 && st.viewport_height > 0.0;
    if viewport_ready {
        st.engine
            .set_viewport(st.viewport_width, st.viewport_height);
    }

    // 4. Window the visible range (learns the row height on the first shape).
    let mut windowed = viewport_ready && ensure_window(st, false);

    // 6. Fresh metrics — content_height now spans total_rows * row_height.
    let metrics = compute_metrics(st);

    // 7. Stick to the bottom iff enabled and we were there. Re-window at the new
    //    offset so the tail is actually shaped this same frame, not next.
    let follow = st.log.as_ref().is_some_and(|l| l.follow_enabled);
    if follow && was_at_bottom && metrics.max_y > st.scroll_y.get() {
        st.scroll_y.set_if_changed(metrics.max_y);
        if viewport_ready {
            windowed |= ensure_window(st, false);
        }
    }

    // 8. Drag-select auto-scroll: the pointer handler left a per-second velocity
    //    when the drag neared an edge; integrate it here so the selection keeps
    //    growing while the pointer is held still. Re-window as it scrolls.
    let mut drag_active = false;
    if let DragState::Selecting {
        auto_scroll_v_per_s,
    } = st.drag_state
        && auto_scroll_v_per_s.abs() > 0.0
    {
        drag_active = true;
        let ny = (st.scroll_y.get() + auto_scroll_v_per_s * delta).clamp(0.0, metrics.max_y);
        st.scroll_y.set_if_changed(ny);
        if viewport_ready {
            windowed |= ensure_window(st, false);
        }
    }

    // 9. Publish limits and clamp the live offsets last, so a shrunken document
    //    (heavy eviction) cannot leave the view parked past its end.
    metrics.publish(
        &st.scroll_x,
        &st.scroll_y,
        &st.max_scroll_x,
        &st.max_scroll_y,
        &st.viewport_ratio_x,
        &st.viewport_ratio_y,
    );

    // 10. Re-walk the accessibility tree only if the visible window changed —
    //     after the scroll has settled (follow-tail / drag), so `first` is final.
    bump_a11y_if_window_changed(st);

    content_changed || had_events || drag_active || windowed
}

/// Take everything queued for append, leaving the queue empty.
fn drain_pending(st: &CodeEditorState) -> Vec<String> {
    let Some(log) = st.log.as_ref() else {
        return Vec::new();
    };
    let mut q = log.pending.lock().expect("log append queue poisoned");
    q.drain(..).collect()
}

/// Append a batch to the document, keeping `total` in step.
///
/// The first batch *fills* the document's initial empty block with its first
/// line (via a cursor) rather than appending after it, so a fresh log opens on
/// real content, not a blank line; the rest of the batch, and every later batch,
/// is one `append_lines`.
fn apply_appends(st: &mut CodeEditorState, batch: Vec<String>) {
    if batch.is_empty() {
        return;
    }
    let pristine = st.log.as_ref().is_some_and(|l| l.pristine);
    if pristine {
        // Fill the lone empty block 0 with the first line — a cursor edit, so it
        // dispatches its own change events and does not add a block.
        let cursor = st.document.cursor();
        cursor.set_position(0, MoveMode::MoveAnchor);
        let _ = cursor.insert_text(&batch[0]);
        if batch.len() > 1 {
            let _ = st.document.append_lines(batch[1..].iter());
        }
    } else {
        let _ = st.document.append_lines(batch.iter());
    }
    if let Some(l) = st.log.as_mut() {
        l.total += batch.len();
        l.pristine = false;
        l.needs_rewindow = true;
    }
}

/// Trim the front to the scrollback cap, amortised. Returns whether it evicted.
///
/// Evicting shifts every surviving row down by the removed count, so the scroll
/// offset and the window anchor are shifted with it to keep the view on the same
/// content; the document shifts the cursors itself.
fn enforce_scrollback(st: &mut CodeEditorState) -> bool {
    let (limit, total) = match st.log.as_ref() {
        Some(l) => match l.scrollback_limit {
            Some(limit) => (limit, l.total),
            None => return false,
        },
        None => return false,
    };
    // Amortise eviction over a slack band, but scale the slack to the cap: a
    // fixed 256 would let a cap of 10 briefly hold 266 (tens of times over), so
    // a small cap gets a small band and is honoured tightly. `saturating_add`
    // guards a cap set near `usize::MAX`.
    let slack = (limit / 4).clamp(1, EVICT_SLACK);
    if total <= limit.saturating_add(slack) {
        return false;
    }
    let excess = total - limit;
    let removed = match st.document.truncate_front(excess) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if removed == 0 {
        return false;
    }

    let row_h = st.log.as_ref().map_or(0.0, |l| l.row_height);
    if row_h > 0.0 {
        let ny = (st.scroll_y.get() - removed as f32 * row_h).max(0.0);
        st.scroll_y.set_if_changed(ny);
    }
    if let Some(l) = st.log.as_mut() {
        l.total = l.total.saturating_sub(removed);
        // The anchor is a (row, char-position) pair; eviction shifts both by an
        // amount we do not track in characters, so drop it — the next window
        // re-locates from scratch (amortised, since eviction is).
        l.anchor = None;
        l.needs_rewindow = true;
    }
    true
}

/// The visible window `(first, count, total)` for the current scroll, or `None`
/// when there is nothing to show (no rows, or no row height learned yet). Shared
/// by the render window, the a11y window, and the a11y-change check, so the three
/// always agree on which rows are visible.
fn window_bounds(st: &CodeEditorState) -> Option<(usize, usize, usize)> {
    let total = st.log.as_ref().map_or(0, |l| l.total);
    if total == 0 {
        return None;
    }
    let row_h = current_row_height(st);
    if row_h <= 0.0 {
        return None;
    }
    let first = ((st.scroll_y.get() / row_h).floor() as usize).min(total - 1);
    let visible = (st.viewport_height / row_h).ceil() as usize + 1;
    let count = (visible + OVERSCAN_ROWS).min(total - first).max(1);
    Some((first, count, total))
}

/// Shape the rows the viewport can show, and only those. Returns whether it
/// actually (re)shaped. Also called from the body's paint, where the scroll
/// offset is authoritative, so it must be idempotent for an unchanged window.
pub(crate) fn ensure_window(st: &mut CodeEditorState, force: bool) -> bool {
    let row_h = current_row_height(st);
    if row_h <= 0.0 {
        return false;
    }
    let Some((first, count, total)) = window_bounds(st) else {
        // Nothing appended yet — the document's lone empty block is not a row.
        st.engine.set_uniform_extent(0, row_h);
        if let Some(l) = st.log.as_mut() {
            l.last_window = None;
            l.row_height = row_h;
        }
        return false;
    };

    let (needs_flag, same_window, same_height) = {
        let l = st.log.as_ref();
        (
            l.is_some_and(|l| l.needs_rewindow),
            l.is_some_and(|l| l.last_window == Some((first, count))),
            l.is_some_and(|l| (l.row_height - row_h).abs() <= 0.01),
        )
    };
    if !force && !needs_flag && same_window && same_height {
        // Nothing to reshape, but the total may have grown while the visible
        // range held (an append while scrolled away): keep the extent honest so
        // the scrollbar still spans the whole document.
        st.engine.set_uniform_extent(total, row_h);
        return false;
    }

    let rows = collect_window(st, first, count);
    st.engine.layout_window_from_snapshots(&rows, total, row_h);
    // Learn the exact shaped row height from the first shaped row, so the next
    // frame's placement and scroll arithmetic use the real height rather than
    // the estimate `current_row_height` derived from the font metrics.
    let learned = rows
        .first()
        .and_then(|(_, snap, _)| st.engine.block_visual_info(snap.block_id))
        .map(|info| info.height)
        .filter(|h| *h > 0.0)
        .unwrap_or(row_h);
    if let Some(l) = st.log.as_mut() {
        l.last_window = Some((first, count));
        l.row_height = learned;
        l.needs_rewindow = false;
    }
    true
}

/// The visible window's block snapshots for the accessibility walk, read-only.
///
/// Returns `(first_row, total_rows, snapshots)`. The log's a11y tree is
/// *windowed* like its render: emitting a paragraph per line of a 100k-line
/// buffer on every append (each append re-walks the AT tree) would be O(N) per
/// line. This resolves the same visible range the render uses — through the
/// cached anchor, without mutating it — so the AT tree tracks what is on screen.
pub(crate) fn a11y_window(
    st: &CodeEditorState,
) -> (
    usize,
    usize,
    Vec<bastyde_text::text_document::BlockSnapshot>,
) {
    let Some((first, count, total)) = window_bounds(st) else {
        let total = st.log.as_ref().map_or(0, |l| l.total);
        return (0, total, Vec::new());
    };
    let anchor = st.log.as_ref().and_then(|l| l.anchor);
    let Some(mut pos) = resolve_row_position(&st.document, first, anchor) else {
        return (first, total, Vec::new());
    };
    let mut snaps = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(snap) = st
            .document
            .snapshot_block_at_position_without_highlights(pos)
        else {
            break;
        };
        pos = snap.position + snap.length + 1;
        snaps.push(snap);
    }
    (first, total, snaps)
}

/// Bump `a11y_version` when the visible window changed, so the accessibility tree
/// re-walks only then. Compares the current `(first, count)` to the last one the
/// tree was built for — a scroll that stays on the same rows, or a tail append
/// while scrolled away, leaves it unchanged and triggers no rebuild.
fn bump_a11y_if_window_changed(st: &mut CodeEditorState) {
    let sig = window_bounds(st).map(|(first, count, _total)| (first, count));
    let version = match st.log.as_mut() {
        Some(l) if l.last_a11y_sig != sig => {
            l.last_a11y_sig = sig;
            l.a11y_version.clone()
        }
        _ => return,
    };
    // Set outside the mutable borrow: the AccessibilityOnly observers this wakes
    // only flip the tree's a11y-dirty flag, never re-enter the state.
    version.set(version.get() + 1);
}

/// Gather `(row, snapshot, tint)` for `count` rows from `first`, chaining forward
/// by character position — each row is one O(log n) rope-backed snapshot, not the
/// O(n) block walk `TextBlock::next` would be — and refreshing the `(row, char
/// position)` anchor to `first`.
fn collect_window(
    st: &mut CodeEditorState,
    first: usize,
    count: usize,
) -> Vec<(
    usize,
    bastyde_text::text_document::BlockSnapshot,
    Option<[f32; 4]>,
)> {
    let severity = st.log.as_ref().and_then(|l| l.severity.clone());
    let anchor = st.log.as_ref().and_then(|l| l.anchor);

    let Some(mut pos) = resolve_row_position(&st.document, first, anchor) else {
        return Vec::new();
    };
    if let Some(l) = st.log.as_mut() {
        l.anchor = Some((first, pos));
    }

    let mut rows = Vec::with_capacity(count);
    let mut row = first;
    while row < first + count {
        let Some(snap) = st
            .document
            .snapshot_block_at_position_without_highlights(pos)
        else {
            break;
        };
        // The next block starts one position past this block's last character —
        // the single position the block separator occupies.
        let next_pos = snap.position + snap.length + 1;
        let tint = severity
            .as_ref()
            .and_then(|f| f(&snap.text))
            .map(|c| c.to_array());
        rows.push((row, snap, tint));
        pos = next_pos;
        row += 1;
    }
    rows
}

/// The character position of visual `target` row's block start.
///
/// Forward-chains from the cached `(row, position)` anchor when the target is
/// ahead of it and within `WALK_LIMIT` — the tail-following hot path, each step a
/// single O(log n) rope snapshot. Otherwise (a scroll up, a far jump, or a
/// dropped anchor) it locates the block by number once, O(n); acceptable because
/// those are user-paced or amortised against eviction.
fn resolve_row_position(
    doc: &bastyde_text::text_document::TextDocument,
    target: usize,
    anchor: Option<(usize, usize)>,
) -> Option<usize> {
    if let Some((arow, apos)) = anchor {
        if target == arow {
            return Some(apos);
        }
        if target > arow && target - arow <= WALK_LIMIT {
            let mut pos = apos;
            for _ in arow..target {
                let snap = doc.snapshot_block_at_position_without_highlights(pos)?;
                pos = snap.position + snap.length + 1;
            }
            return Some(pos);
        }
    }
    doc.block_by_number(target).map(|b| b.position())
}

/// The uniform row height: the learned value if we have one, else the engine's
/// line height (known without shaping — it is the font's, not a laid-out row's).
///
/// The estimate must include the engine's `font_scale` (the global
/// accessibility text scale), because `layout_window` shapes each row *at* that
/// scale — `default_line_height` reports the unscaled height, so an unscaled
/// estimate would place rows at the wrong pitch until a shaped height is learned.
fn current_row_height(st: &CodeEditorState) -> f32 {
    let learned = st.log.as_ref().map_or(0.0, |l| l.row_height);
    if learned > 0.0 {
        learned
    } else {
        st.engine.default_line_height() * st.engine.font_scale().max(0.01)
    }
}

fn compute_metrics(st: &CodeEditorState) -> ScrollMetrics {
    ScrollMetrics::compute(
        st.engine.content_height(),
        st.engine.max_content_width(),
        st.engine.zoom(),
        st.viewport_width,
        st.viewport_height,
    )
}

/// Whether the view is at the bottom (within the follow slack), using the
/// currently-published maximum.
fn at_bottom(st: &CodeEditorState) -> bool {
    st.scroll_y.get() >= st.max_scroll_y.get() - FOLLOW_EPSILON
}

/// Read-only keyboard for a log view: the keys *scroll*, they do not drive an
/// invisible caret (which would scroll to a target outside the shaped window,
/// where its geometry is unknown). Select-all and copy work because they need no
/// geometry — copy reads the document by character range, correct across any
/// range whether shaped or not.
pub(crate) fn handle_log_key(
    state: &SharedState,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
        return EventResponse::Ignored;
    };
    let ctrl = modifiers.ctrl() || modifiers.super_key();

    enum Act {
        Scroll,
        SelectAll,
        Copy,
        None,
    }

    let act = {
        let mut st = state.borrow_mut();
        let row_h = current_row_height(&st).max(1.0);
        // A page keeps one row of context, matching a scrollbar page.
        let page = (st.viewport_height - row_h).max(row_h);
        // A horizontal step of a few columns, reusing the row height as a
        // rough column proxy (a monospace log has no per-column metric here).
        let step_x = row_h * 4.0;
        let (max_y, max_x) = (st.max_scroll_y.get(), st.max_scroll_x.get());
        let (cy, cx) = (st.scroll_y.get(), st.scroll_x.get());
        match key {
            Key::ArrowDown if !ctrl => {
                st.scroll_y.set_if_changed((cy + row_h).min(max_y));
                Act::Scroll
            }
            Key::ArrowUp if !ctrl => {
                st.scroll_y.set_if_changed((cy - row_h).max(0.0));
                Act::Scroll
            }
            Key::PageDown => {
                st.scroll_y.set_if_changed((cy + page).min(max_y));
                Act::Scroll
            }
            Key::PageUp => {
                st.scroll_y.set_if_changed((cy - page).max(0.0));
                Act::Scroll
            }
            Key::Home if ctrl => {
                st.scroll_y.set_if_changed(0.0);
                Act::Scroll
            }
            Key::End if ctrl => {
                st.scroll_y.set_if_changed(max_y);
                Act::Scroll
            }
            Key::Home if !ctrl => {
                st.scroll_x.set_if_changed(0.0);
                Act::Scroll
            }
            Key::End if !ctrl => {
                st.scroll_x.set_if_changed(max_x);
                Act::Scroll
            }
            Key::ArrowLeft if !ctrl => {
                st.scroll_x.set_if_changed((cx - step_x).max(0.0));
                Act::Scroll
            }
            Key::ArrowRight if !ctrl => {
                st.scroll_x.set_if_changed((cx + step_x).min(max_x));
                Act::Scroll
            }
            Key::A if ctrl => {
                st.clear_extra_carets();
                st.cursor.select(SelectionType::Document);
                Act::SelectAll
            }
            Key::C if ctrl => Act::Copy,
            _ => Act::None,
        }
    };

    match act {
        Act::Scroll => {
            ctx.request_frame();
            EventResponse::Handled
        }
        Act::SelectAll => {
            super::sync_cursor_signals(state);
            ctx.request_frame();
            EventResponse::Handled
        }
        Act::Copy => {
            super::clipboard::copy(&state.borrow(), ctx);
            EventResponse::Handled
        }
        Act::None => EventResponse::Ignored,
    }
}
