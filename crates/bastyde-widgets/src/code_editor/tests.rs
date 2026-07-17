// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless tests for the code editor core.
//!
//! These run against `MockTextBackend`-class metrics via the private engine —
//! they verify the editor's own logic (viewport adoption, caret bookkeeping,
//! event classification, policy gating), not shaping, which is text-typeset's
//! own suite's job.

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::widget::Widget;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_text::WrapMode;
use bastyde_text::text_document::{MoveMode, TextDocument};

use super::config::CodeConfig;
use super::policy::{CODE_EDITOR_PRESET, CODE_READ_ONLY_PRESET};
use super::state::{CodeEditorState, SharedState};
use super::{body_for, construct};

fn editor_state(text: &str) -> SharedState {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    construct(
        doc,
        CODE_EDITOR_PRESET,
        CodeConfig::default(),
        WrapMode::None,
    )
}

fn viewer_state(text: &str) -> SharedState {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    construct(
        doc,
        CODE_READ_ONLY_PRESET,
        CodeConfig::default(),
        WrapMode::None,
    )
}

// --- Construction ---------------------------------------------------------

/// A viewer must not flash a caret on its first frame, before any tick can
/// hide it — so the signal is seeded from the policy at construction.
#[test]
fn a_viewer_starts_with_its_caret_already_hidden() {
    let st = viewer_state("fn main() {}");
    assert!(!st.borrow().caret_visible.get());
}

#[test]
fn an_editor_starts_with_its_caret_shown() {
    let st = editor_state("fn main() {}");
    assert!(st.borrow().caret_visible.get());
}

/// A fresh editor has exactly one caret, and `all_carets` yields it. The empty
/// `extra_carets` is the common case and must not need special handling.
#[test]
fn a_fresh_editor_has_exactly_one_caret() {
    let st = editor_state("hello");
    let s = st.borrow();
    assert_eq!(s.all_carets().count(), 1);
    assert_eq!(s.caret_count.get(), 1);
}

/// A code document defaults to no wrapping: a wrapped source line breaks the
/// visual column/line correspondence the gutter and the reader both rely on.
#[test]
fn the_code_editor_does_not_wrap_by_default() {
    let st = editor_state("a very long line of source code that would wrap in prose");
    assert_eq!(st.borrow().engine.wrap_mode(), WrapMode::None);
}

// --- Viewport -------------------------------------------------------------

/// `sync_viewport` is the sole writer of the viewport, and it must pair the
/// size change with the relayout flag — splitting them is how a resize lays
/// text out at the old width.
#[test]
fn syncing_a_new_viewport_requests_a_relayout() {
    let st = editor_state("x");
    {
        let mut s = st.borrow_mut();
        s.needs_full_layout = false;
        assert!(s.sync_viewport(Rect::new(0.0, 0.0, 400.0, 300.0)));
        assert_eq!(s.viewport_width, 400.0);
        assert_eq!(s.viewport_height, 300.0);
        assert!(
            s.needs_full_layout,
            "a resized viewport must force a relayout, or the text stays laid \
             out at the old width"
        );
    }
}

/// Re-adopting identical bounds must not request a relayout: `paint` echoes
/// `place_children`'s call every frame, so a naive implementation would
/// relayout the document on every single frame.
#[test]
fn re_syncing_the_same_viewport_is_a_no_op() {
    let st = editor_state("x");
    let mut s = st.borrow_mut();
    s.sync_viewport(Rect::new(0.0, 0.0, 400.0, 300.0));
    s.needs_full_layout = false;
    assert!(!s.sync_viewport(Rect::new(0.0, 0.0, 400.0, 300.0)));
    assert!(
        !s.needs_full_layout,
        "an unchanged viewport must not relayout — paint re-syncs every frame"
    );
}

/// The origin tracks even when the size does not: a scrolled or moved pane
/// keeps its size but the engine still needs to know where it now is.
#[test]
fn syncing_tracks_the_origin_even_when_the_size_is_unchanged() {
    let st = editor_state("x");
    let mut s = st.borrow_mut();
    s.sync_viewport(Rect::new(0.0, 0.0, 400.0, 300.0));
    s.sync_viewport(Rect::new(17.0, 42.0, 400.0, 300.0));
    assert_eq!(s.viewport_origin.x, 17.0);
    assert_eq!(s.viewport_origin.y, 42.0);
}

// --- Carets ---------------------------------------------------------------

#[test]
fn clearing_extra_carets_reports_whether_any_existed() {
    let st = editor_state("one\ntwo\nthree");
    let mut s = st.borrow_mut();
    assert!(
        !s.clear_extra_carets(),
        "clearing with only the primary caret must report no change, so Escape \
         can fall through to whatever else wants it"
    );
    let extra = s.document.cursor();
    s.extra_carets.push(extra);
    assert!(s.clear_extra_carets());
    assert_eq!(s.all_carets().count(), 1);
}

// --- Event classification -------------------------------------------------

/// A colour-only highlight change must recolour, never reshape. This is the
/// whole reason the event is distinct from FormatChanged, and it is what makes
/// syntax-highlighting a large file affordable: a colour cannot change a glyph
/// advance, so re-shaping would be pure waste on every keystroke.
#[test]
fn a_paint_only_highlight_change_recolours_without_relayout() {
    let st = editor_state("fn main() {}");
    {
        let mut s = st.borrow_mut();
        s.needs_full_layout = false;
        s.event_queue.lock().unwrap().push_back(
            bastyde_text::text_document::DocumentEvent::HighlightPaintChanged {
                position: 0,
                length: 2,
            },
        );
    }
    let (had, single) = st.borrow_mut().drain_events();
    let s = st.borrow();
    assert!(had);
    assert_eq!(single, None);
    assert!(
        s.pending_recolor,
        "a highlight repaint must request a recolour"
    );
    assert!(
        !s.needs_full_layout,
        "a colour change must not reshape — it cannot alter a glyph advance"
    );
}

/// A single-block edit yields the position hint that lets the frame loop
/// relayout one block instead of the document.
#[test]
fn a_single_block_edit_yields_a_relayout_hint() {
    let st = editor_state("a\nb");
    {
        let mut s = st.borrow_mut();
        s.needs_full_layout = false;
        s.event_queue.lock().unwrap().push_back(
            bastyde_text::text_document::DocumentEvent::ContentsChanged {
                position: 1,
                chars_removed: 0,
                chars_added: 1,
                blocks_affected: 1,
            },
        );
    }
    let (_, single) = st.borrow_mut().drain_events();
    assert_eq!(single, Some(1));
    assert!(!st.borrow().needs_full_layout);
}

/// A multi-block edit must escalate to a full relayout: the single-block fast
/// path cannot express "these three blocks moved".
#[test]
fn a_multi_block_edit_forces_a_full_relayout() {
    let st = editor_state("a\nb\nc");
    {
        let mut s = st.borrow_mut();
        s.needs_full_layout = false;
        s.event_queue.lock().unwrap().push_back(
            bastyde_text::text_document::DocumentEvent::ContentsChanged {
                position: 0,
                chars_removed: 0,
                chars_added: 4,
                blocks_affected: 3,
            },
        );
    }
    let (_, single) = st.borrow_mut().drain_events();
    assert_eq!(single, None);
    assert!(st.borrow().needs_full_layout);
}

/// An edit invalidates the cached accessibility snapshot: new text means new
/// runs, so a stale snapshot would report the previous edit's AT tree.
#[test]
fn an_edit_invalidates_the_accessibility_snapshot() {
    let st = editor_state("hello");
    {
        let s = st.borrow();
        *s.accessibility_flow_snapshot.borrow_mut() = Some(s.document.snapshot_flow());
        assert!(s.accessibility_flow_snapshot.borrow().is_some());
    }
    {
        let mut s = st.borrow_mut();
        s.event_queue.lock().unwrap().push_back(
            bastyde_text::text_document::DocumentEvent::ContentsChanged {
                position: 0,
                chars_removed: 0,
                chars_added: 1,
                blocks_affected: 1,
            },
        );
        s.drain_events();
    }
    assert!(
        st.borrow().accessibility_flow_snapshot.borrow().is_none(),
        "a stale AT snapshot would report the previous edit's tree"
    );
}

/// Draining an empty queue must report no events, so the frame loop can idle
/// rather than spin.
#[test]
fn draining_an_empty_queue_reports_no_work() {
    let st = editor_state("x");
    let (had, single) = st.borrow_mut().drain_events();
    assert!(!had);
    assert_eq!(single, None);
}

#[test]
fn undo_state_is_debounced_rather_than_published_immediately() {
    let st = editor_state("x");
    {
        let mut s = st.borrow_mut();
        s.event_queue.lock().unwrap().push_back(
            bastyde_text::text_document::DocumentEvent::UndoRedoChanged {
                can_undo: true,
                can_redo: false,
            },
        );
        s.drain_events();
    }
    let s = st.borrow();
    assert_eq!(
        s.pending_undo_redo,
        Some((true, false)),
        "undo state must be parked for the debounce window, not fanned out to \
         every toolbar observer per keystroke"
    );
}

// --- Widget integration ---------------------------------------------------

/// The body must take the space it is given: it is normally the scrollable
/// region of a pane, so it fills and scrolls rather than sizing to content.
#[test]
fn the_body_fills_its_proposal_by_default() {
    let mut tree = WidgetTree::new();
    let st = editor_state("fn main() {}");
    let id = tree.add(body_for(&st, None, None));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let b = tree.bounds(id);
    assert!((b.width - 400.0).abs() < 0.01);
    assert!((b.height - 300.0).abs() < 0.01);
}

/// Laying the body out must reach `sync_viewport`, so the engine knows its
/// width before anything tries to paint.
#[test]
fn laying_out_the_body_adopts_the_viewport() {
    let mut tree = WidgetTree::new();
    let st = editor_state("fn main() {}");
    tree.add(body_for(&st, None, None));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(st.borrow().viewport_width, 400.0);
    assert_eq!(
        st.borrow().viewport_height,
        300.0,
        "layout must adopt the viewport — paint runs after, and would otherwise \
         be the first to learn the width"
    );
}

/// Lay the engine out so `content_height` is real. Intrinsic sizing reads it,
/// and an engine that has never laid out reports zero — which would let a
/// "caps at N lines" assertion pass for the wrong reason.
fn force_layout(st: &SharedState, width: f32, height: f32) {
    let mut s = st.borrow_mut();
    s.engine.set_viewport(width, height);
    let flow = s.document.snapshot_flow();
    s.engine.layout_full(&flow);
}

/// `min_lines` floors an intrinsic-mode body: an empty composer still shows
/// its configured height rather than collapsing to nothing.
///
/// The proposal leaves height unbounded, which is what a `VStack` offers a
/// non-`Expand` child — bound it (`exact`) and the tree hands the root the
/// proposal outright, so the intrinsic response never shows.
#[test]
fn min_lines_floors_an_empty_intrinsic_body() {
    let mut tree = WidgetTree::new();
    let st = editor_state("");
    let line_h = st.borrow().engine.default_line_height();
    assert!(line_h > 0.0, "the embedded font must report a line height");
    let id = tree.add(body_for(&st, Some(3), None));
    tree.layout(SizeProposal::with_width(400.0));
    let h = tree.bounds(id).height;
    assert!(
        (h - 3.0 * line_h).abs() < 1.0,
        "an empty composer must still stand 3 lines tall, expected ~{}, got {h}",
        3.0 * line_h
    );
}

/// `max_lines` caps growth so a long document scrolls instead of pushing its
/// siblings off the pane.
#[test]
fn max_lines_caps_an_intrinsic_body() {
    let mut tree = WidgetTree::new();
    let st = editor_state("a\nb\nc\nd\ne\nf\ng\nh\ni\nj");
    force_layout(&st, 400.0, 600.0);
    let (line_h, content_h) = {
        let s = st.borrow();
        (s.engine.default_line_height(), s.engine.content_height())
    };
    assert!(
        content_h > 2.0 * line_h,
        "the fixture must genuinely overflow the cap ({content_h} vs {}), else \
         this proves nothing",
        2.0 * line_h
    );
    let id = tree.add(body_for(&st, None, Some(2)));
    tree.layout(SizeProposal::with_width(400.0));
    let h = tree.bounds(id).height;
    assert!(
        h <= 2.0 * line_h + 1.0,
        "10 lines under max_lines(2) must cap at ~{}, got {h}",
        2.0 * line_h
    );
}

/// The composer pattern: growth is clamped into `[min, max]` however much
/// vertical space the parent offers.
#[test]
fn min_and_max_lines_clamp_growth_within_the_window() {
    let mut tree = WidgetTree::new();
    let st = editor_state("one\ntwo\nthree\nfour\nfive\nsix");
    force_layout(&st, 400.0, 600.0);
    let line_h = st.borrow().engine.default_line_height();
    let id = tree.add(body_for(&st, Some(1), Some(4)));
    tree.layout(SizeProposal::with_width(400.0));
    let h = tree.bounds(id).height;
    assert!(
        h >= line_h - 0.5 && h <= 4.0 * line_h + 0.5,
        "intrinsic height must land in [1, 4] × {line_h}, got {h}"
    );
}

/// The body clips: a long unwrapped line must not paint over the pane beside
/// it.
#[test]
fn the_body_clips_its_content() {
    let st = editor_state("x");
    assert!(body_for(&st, None, None).clips_children());
}

// --- Accessibility --------------------------------------------------------

#[test]
fn an_editable_body_reports_a_multiline_text_input() {
    use bastyde_core::accesskit::{Action, Role};
    let st = editor_state("fn main() {}");
    let body = body_for(&st, None, None);
    let mut b = bastyde_core::accessibility::AccessNodeBuilder::new();
    body.accessibility(&mut b);
    assert_eq!(b.role(), Role::MultilineTextInput);
    assert!(b.actions().contains(&Action::SetValue));
    assert!(b.actions().contains(&Action::SetTextSelection));
}

/// A viewer reports `Document`, not `Code` or `Log`: those are outside
/// accesskit_consumer's `supports_text_ranges()` set, so a reader could
/// announce the text once and then never track the caret through it.
#[test]
fn a_viewer_reports_a_text_range_capable_role_and_no_set_value() {
    use bastyde_core::accesskit::{Action, Role};
    let st = viewer_state("2026-07-17 INFO ready");
    let body = body_for(&st, None, None);
    let mut b = bastyde_core::accessibility::AccessNodeBuilder::new();
    body.accessibility(&mut b);
    assert_eq!(b.role(), Role::Document);
    assert!(
        !b.actions().contains(&Action::SetValue),
        "a read-only surface must not advertise SetValue"
    );
    assert!(
        b.actions().contains(&Action::SetTextSelection),
        "a viewer must still support selection — that is what makes its text \
         readable and copyable through AT"
    );
}

/// Live-region announcement is opt-in. A build log at fifty lines a second
/// with an implicit live region floods a screen reader into uselessness, and
/// the widget cannot tell that case from a handful of meaningful events.
#[test]
fn appends_are_not_announced_unless_asked_for() {
    let st = viewer_state("ready");
    assert!(
        !st.borrow().announce_appends,
        "a live region must be opt-in — an unasked-for one is a denial of \
         service against the screen reader"
    );
}

// --- Config ---------------------------------------------------------------

/// The state carries the app's config verbatim; there is no language guessing
/// anywhere in the construction path.
#[test]
fn construction_carries_the_injected_config() {
    let doc = TextDocument::new();
    let cfg = CodeConfig {
        line_comment: Some("--".to_string()),
        ..CodeConfig::default()
    };
    let st = construct(doc, CODE_EDITOR_PRESET, cfg, WrapMode::None);
    assert_eq!(st.borrow().config.line_comment.as_deref(), Some("--"));
}

fn _assert_state_is_sized(_: &CodeEditorState) {}

// --- Frame loop -----------------------------------------------------------

use super::frame_loop;

/// The tick must report "no more work" for an idle editor, or the tree never
/// stops pumping frames and the app burns a core sitting still.
#[test]
fn an_idle_unfocused_editor_stops_asking_for_frames() {
    let st = editor_state("fn main() {}");
    force_layout(&st, 400.0, 300.0);
    let more = frame_loop::tick(&mut st.borrow_mut(), 0.016);
    assert!(
        !more,
        "an idle editor must let the frame loop go quiet — Bastyde is \
         draw-when-needed and this is what makes it so"
    );
}

/// Typed characters are buffered and flushed by the tick, not applied on the
/// keystroke — that is what collapses a burst into one relayout.
#[test]
fn the_tick_flushes_buffered_typing_into_the_document() {
    let st = editor_state("");
    force_layout(&st, 400.0, 300.0);
    st.borrow_mut().pending_chars.push_str("let x = 1;");
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    assert_eq!(st.borrow().document.to_plain_text().unwrap(), "let x = 1;");
    assert!(
        st.borrow().pending_chars.is_empty(),
        "the buffer must be drained, or the next tick types it again"
    );
}

/// A burst of keystrokes must reach the document as ONE edit. If each character
/// were applied separately the editor would relayout per keystroke, which is
/// the difference between typing smoothly and typing in a large file at all.
#[test]
fn a_burst_of_typing_becomes_a_single_document_edit() {
    let st = editor_state("");
    force_layout(&st, 400.0, 300.0);
    for c in "hello".chars() {
        st.borrow_mut().pending_chars.push(c);
    }
    // One tick, one insert — evidenced by a single ContentsChanged reaching
    // the queue rather than five.
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    let queued = st.borrow().event_queue.lock().unwrap().len();
    assert!(
        queued <= 1,
        "a 5-character burst must produce at most one document event, got \
         {queued} — per-keystroke relayout is what makes a big file unusable"
    );
}

/// Drag auto-scroll integrates its velocity per tick, so the selection keeps
/// growing while the pointer is held still past the edge.
#[test]
fn drag_auto_scroll_advances_over_time_and_keeps_the_loop_alive() {
    let st = editor_state("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np");
    force_layout(&st, 400.0, 40.0);
    {
        let mut s = st.borrow_mut();
        s.viewport_width = 400.0;
        s.viewport_height = 40.0;
        s.drag_state = super::state::DragState::Selecting {
            auto_scroll_v_per_s: 600.0,
        };
    }
    let more = frame_loop::tick(&mut st.borrow_mut(), 0.1);
    assert!(
        more,
        "an active auto-scroll must keep the frame loop pumping"
    );
    assert!(
        st.borrow().scroll_y.get() > 0.0,
        "velocity must integrate into an actual scroll"
    );
}

/// A held button with no motion must NOT keep the loop pumping — otherwise
/// resting the mouse after a click spins the CPU indefinitely.
#[test]
fn a_held_drag_with_no_velocity_lets_the_loop_idle() {
    let st = editor_state("hello");
    force_layout(&st, 400.0, 300.0);
    st.borrow_mut().drag_state = super::state::DragState::Selecting {
        auto_scroll_v_per_s: 0.0,
    };
    assert!(
        !frame_loop::tick(&mut st.borrow_mut(), 0.016),
        "holding the button still must not pump frames"
    );
}

/// Scroll offsets are clamped to the live maxima each tick: deleting text
/// shrinks the document, and an offset left past the new end parks the view in
/// blank space below the last line.
#[test]
fn the_tick_clamps_a_scroll_offset_past_the_end() {
    let st = editor_state("one line");
    force_layout(&st, 400.0, 300.0);
    {
        let mut s = st.borrow_mut();
        s.viewport_width = 400.0;
        s.viewport_height = 300.0;
        s.scroll_y.set(5000.0);
    }
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    assert_eq!(
        st.borrow().scroll_y.get(),
        0.0,
        "a one-line document cannot scroll — the offset must be clamped back"
    );
}

// --- Multi-caret editing --------------------------------------------------

/// Insert at several carets at once. The back-to-front order is what makes this
/// correct: applied ascending, the second insertion would land at an offset the
/// first had already shifted.
#[test]
fn typing_with_several_carets_inserts_at_each_one() {
    let st = editor_state("ab");
    force_layout(&st, 400.0, 300.0);
    {
        let mut s = st.borrow_mut();
        // Carets at offset 0 and offset 1 (before 'a', before 'b').
        s.cursor.set_position(0, MoveMode::MoveAnchor);
        let extra = s.document.cursor();
        extra.set_position(1, MoveMode::MoveAnchor);
        s.extra_carets.push(extra);
        s.pending_chars.push('X');
    }
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    assert_eq!(
        st.borrow().document.to_plain_text().unwrap(),
        "XaXb",
        "each caret must get the character — an ascending walk would shift the \
         later carets and misplace them"
    );
}

/// Multi-caret typing is one undo step: a user who typed into three places at
/// once means all three when they press Ctrl+Z.
#[test]
fn multi_caret_typing_undoes_as_one_step() {
    let st = editor_state("ab");
    force_layout(&st, 400.0, 300.0);
    {
        let mut s = st.borrow_mut();
        s.cursor.set_position(0, MoveMode::MoveAnchor);
        let extra = s.document.cursor();
        extra.set_position(1, MoveMode::MoveAnchor);
        s.extra_carets.push(extra);
        s.pending_chars.push('X');
    }
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    assert_eq!(st.borrow().document.to_plain_text().unwrap(), "XaXb");
    let _ = st.borrow().document.undo();
    assert_eq!(
        st.borrow().document.to_plain_text().unwrap(),
        "ab",
        "one undo must revert the whole multi-caret insert, not one caret of it"
    );
}

/// Consecutive single-caret typing across several ticks lands as one run of
/// text, and text-document coalesces it into one undo step.
///
/// That coalescing is the document's, not ours, and it is the behaviour a user
/// expects — Ctrl+Z after typing a word removes the word, not its last letter.
/// Pinned here because the multi-caret path deliberately wraps its inserts in
/// an explicit edit block, and this records that the single-caret path needs no
/// such thing to get the same granularity.
#[test]
fn consecutive_typing_ticks_coalesce_into_one_undo_step() {
    let st = editor_state("");
    force_layout(&st, 400.0, 300.0);
    st.borrow_mut().pending_chars.push('a');
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    st.borrow_mut().pending_chars.push('b');
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    assert_eq!(st.borrow().document.to_plain_text().unwrap(), "ab");

    let _ = st.borrow().document.undo();
    assert_eq!(
        st.borrow().document.to_plain_text().unwrap(),
        "",
        "typing coalesces: one undo removes the run, which is what a user means \
         by undoing their typing"
    );
}

// --- Caret merging --------------------------------------------------------

/// Two carets that collide must merge. Left stacked, the next character is
/// inserted twice at one spot — which is why this is correctness, not tidiness.
#[test]
fn carets_that_collide_are_merged() {
    let st = editor_state("hello world");
    force_layout(&st, 400.0, 300.0);
    {
        let mut s = st.borrow_mut();
        s.cursor.set_position(3, MoveMode::MoveAnchor);
        let extra = s.document.cursor();
        extra.set_position(5, MoveMode::MoveAnchor);
        s.extra_carets.push(extra);
    }
    // Both to the start of the same line: they now coincide.
    {
        let s = st.borrow_mut();
        s.cursor.set_position(0, MoveMode::MoveAnchor);
        s.extra_carets[0].set_position(0, MoveMode::MoveAnchor);
    }
    // Typing runs the merge via the insert path.
    st.borrow_mut().pending_chars.push('Z');
    frame_loop::tick(&mut st.borrow_mut(), 0.016);
    let text = st.borrow().document.to_plain_text().unwrap();
    assert_eq!(
        text, "Zhello world",
        "two carets at one offset must insert one character, not two — got {text:?}"
    );
}

// --- Alt-click ------------------------------------------------------------

/// Alt-click adds a caret; Alt-clicking it again removes it — the undo for a
/// click that landed wrong.
#[test]
fn alt_click_adds_then_removes_a_caret() {
    let st = editor_state("hello world");
    force_layout(&st, 400.0, 300.0);
    {
        let mut s = st.borrow_mut();
        s.cursor.set_position(0, MoveMode::MoveAnchor);
        super::mouse::add_caret_at(&mut s, 6);
        assert_eq!(s.extra_carets.len(), 1);
        super::mouse::add_caret_at(&mut s, 6);
        assert_eq!(
            s.extra_carets.len(),
            0,
            "alt-clicking an existing caret must remove it"
        );
    }
}

/// Alt-clicking the primary caret is a no-op: removing it would leave the
/// editor with nowhere to type.
#[test]
fn alt_click_on_the_primary_caret_is_ignored() {
    let st = editor_state("hello");
    let mut s = st.borrow_mut();
    s.cursor.set_position(2, MoveMode::MoveAnchor);
    super::mouse::add_caret_at(&mut s, 2);
    assert!(
        s.extra_carets.is_empty(),
        "the primary caret must survive an alt-click on itself"
    );
    assert_eq!(s.all_carets().count(), 1);
}

// --- IME ------------------------------------------------------------------

/// A cancelled composition must leave the document exactly as it was — the
/// tentative text was never the user's.
#[test]
fn a_cancelled_composition_leaves_the_document_clean() {
    let st = editor_state("ab");
    force_layout(&st, 400.0, 300.0);
    {
        let mut s = st.borrow_mut();
        s.cursor.set_position(1, MoveMode::MoveAnchor);
        // Simulate a landed preedit.
        let start = s.cursor.position();
        let _ = s.cursor.insert_text("ん");
        let end = s.cursor.position();
        s.ime_preedit = Some("ん".to_string());
        s.ime_preedit_range = Some(start..end);
    }
    assert_eq!(st.borrow().document.to_plain_text().unwrap(), "aんb");
    super::keyboard::clear_ime_preedit(&st);
    assert_eq!(
        st.borrow().document.to_plain_text().unwrap(),
        "ab",
        "cancelling must remove the tentative text — leaving it would make an \
         abandoned composition permanent"
    );
    assert!(st.borrow().ime_preedit.is_none());
    assert!(st.borrow().ime_preedit_range.is_none());
}

/// A stale preedit range must not delete the wrong text when the document
/// shrank underneath it (an undo, a programmatic edit).
#[test]
fn a_stale_preedit_range_cannot_delete_past_the_end() {
    let st = editor_state("ab");
    {
        let mut s = st.borrow_mut();
        // A range describing a document that no longer exists.
        s.ime_preedit_range = Some(100..200);
        s.ime_preedit = Some("x".to_string());
    }
    super::keyboard::clear_ime_preedit(&st);
    assert_eq!(
        st.borrow().document.to_plain_text().unwrap(),
        "ab",
        "a stale range must clamp to the live length rather than corrupt"
    );
}

// --- Smart Home -----------------------------------------------------------

/// Home goes to the first non-whitespace character; Home again goes to column
/// 0. Derived from the caret's position, not a remembered flag — so it is
/// still right after the caret was moved by a click or programmatically.
#[test]
fn home_toggles_between_the_indent_and_column_zero() {
    let st = editor_state("    indented");
    force_layout(&st, 400.0, 300.0);

    // From the end of the line, Home lands on the first real character.
    st.borrow().cursor.set_position(12, MoveMode::MoveAnchor);
    super::keyboard::smart_home_for_test(&st);
    assert_eq!(
        st.borrow().cursor.position(),
        4,
        "Home must land on the content, not the margin — that is where the \
         caret is wanted nine times out of ten"
    );

    // Already there: Home again goes to the true start.
    super::keyboard::smart_home_for_test(&st);
    assert_eq!(
        st.borrow().cursor.position(),
        0,
        "a second Home must reach column 0, or re-indenting is impossible"
    );

    // And back again — the toggle is symmetric.
    super::keyboard::smart_home_for_test(&st);
    assert_eq!(st.borrow().cursor.position(), 4);
}

/// A line with no indent has both targets in the same place, so Home is simply
/// column 0 and never appears to do nothing.
#[test]
fn home_on_an_unindented_line_goes_to_column_zero() {
    let st = editor_state("flush");
    force_layout(&st, 400.0, 300.0);
    st.borrow().cursor.set_position(3, MoveMode::MoveAnchor);
    super::keyboard::smart_home_for_test(&st);
    assert_eq!(st.borrow().cursor.position(), 0);
    // Already at 0: stays there rather than jumping somewhere surprising.
    super::keyboard::smart_home_for_test(&st);
    assert_eq!(st.borrow().cursor.position(), 0);
}

/// Each caret toggles about *its own* line. A single remembered flag would
/// drive every caret off whatever the primary happened to be doing.
#[test]
fn home_toggles_each_caret_about_its_own_line() {
    let st = editor_state("    alpha\nbeta");
    force_layout(&st, 400.0, 300.0);
    {
        let s = st.borrow_mut();
        // Primary at the end of the indented line; extra at the end of the
        // unindented one.
        s.cursor.set_position(9, MoveMode::MoveAnchor);
    }
    {
        let mut s = st.borrow_mut();
        let extra = s.document.cursor();
        extra.set_position(14, MoveMode::MoveAnchor);
        s.extra_carets.push(extra);
    }
    super::keyboard::smart_home_for_test(&st);
    let s = st.borrow();
    assert_eq!(s.cursor.position(), 4, "indented line → its indent");
    assert_eq!(
        s.extra_carets[0].position(),
        10,
        "unindented line → its column 0, independently of the primary"
    );
}

// --- Gutter ---------------------------------------------------------------

use super::gutter::CodeGutter;

/// The line count must come from the event, never from `block_count()`.
///
/// `TextDocument::block_count()` advertises "O(1) — reads cached value" and then
/// fetches every block, reads each one's content from the rope, and word-counts
/// it before returning the cached number. A gutter sizing itself from that would
/// word-count the whole document on every layout.
#[test]
fn the_line_count_tracks_the_document_via_the_event() {
    let st = editor_state("a\nb\nc");
    assert_eq!(
        st.borrow().line_count.get(),
        3,
        "seeded once at construction"
    );

    {
        let mut s = st.borrow_mut();
        s.event_queue
            .lock()
            .unwrap()
            .push_back(bastyde_text::text_document::DocumentEvent::BlockCountChanged(9));
        s.drain_events();
    }
    assert_eq!(
        st.borrow().line_count.get(),
        9,
        "BlockCountChanged carries the count — that is the only affordable way \
         to learn it"
    );
}

/// Adding or removing lines moves every line below them, so the count changing
/// must force a relayout.
#[test]
fn a_changed_line_count_forces_a_relayout() {
    let st = editor_state("a\nb");
    {
        let mut s = st.borrow_mut();
        s.needs_full_layout = false;
        s.event_queue
            .lock()
            .unwrap()
            .push_back(bastyde_text::text_document::DocumentEvent::BlockCountChanged(5));
        s.drain_events();
    }
    assert!(st.borrow().needs_full_layout);
}

/// The gutter is presentational: a reader must not have to arrow past thirty
/// numbers to reach the code. Line position is conveyed on the paragraph nodes
/// instead, where it is spoken *with* the line.
#[test]
fn the_gutter_is_hidden_from_assistive_technology() {
    let st = editor_state("a\nb\nc");
    let g = CodeGutter::new(&st);
    let mut b = bastyde_core::accessibility::AccessNodeBuilder::new();
    g.accessibility(&mut b);
    assert!(
        b.is_hidden(),
        "the gutter must be hidden — its numbers belong on the paragraphs, not \
         as thirty nodes in the reader's path"
    );
}

/// Width comes from the total line count, not what is on screen. Sizing to the
/// visible maximum would make the gutter breathe as the user scrolls past line
/// 99 into 100, shifting the code sideways under the caret.
#[test]
fn the_gutter_widens_with_the_total_line_count_not_the_visible_one() {
    let mut tree = WidgetTree::new();
    let small = editor_state("a\nb\nc");
    let id_small = tree.add(CodeGutter::new(&small));
    tree.layout(SizeProposal::with_height(300.0));
    let w_small = tree.bounds(id_small).width;

    let mut tree2 = WidgetTree::new();
    let big = editor_state("x");
    big.borrow().line_count.set(100_000);
    let id_big = tree2.add(CodeGutter::new(&big));
    tree2.layout(SizeProposal::with_height(300.0));
    let w_big = tree2.bounds(id_big).width;

    assert!(
        w_big > w_small,
        "a 100k-line document needs a wider gutter than a 3-line one ({w_big} \
         vs {w_small})"
    );
}

/// The width must not depend on the scroll position — that is the same
/// assertion from the other side, and it is what "no jitter" means concretely.
#[test]
fn the_gutter_width_does_not_change_when_scrolled() {
    let mut tree = WidgetTree::new();
    let st = editor_state("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl");
    let id = tree.add(CodeGutter::new(&st));
    tree.layout(SizeProposal::with_height(40.0));
    let before = tree.bounds(id).width;

    st.borrow().scroll_y.set(500.0);
    tree.layout(SizeProposal::with_height(40.0));
    let after = tree.bounds(id).width;

    assert_eq!(
        before, after,
        "scrolling must never resize the gutter — the code would shift sideways"
    );
}

/// A gutter with nothing laid out yet must draw nothing rather than place
/// numbers at invented positions for one frame.
#[test]
fn the_gutter_paints_nothing_before_the_first_layout() {
    let st = editor_state("a\nb\nc");
    // No force_layout: the engine has never laid out.
    assert!(!st.borrow().engine.has_full_layout());
    let mut tree = WidgetTree::new();
    tree.add(CodeGutter::new(&st));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Painting must not panic, and must not invent geometry.
    let _ = tree.render();
}
