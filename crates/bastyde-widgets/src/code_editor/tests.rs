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

// ══════════════════════════════════════════════════════════════════════════
// Code semantics (Phase 4d)
//
// These exercise the config-driven editing operations directly on the state —
// they touch only the document, never layout or paint, so they run without a
// backend. Each asserts the resulting text AND the caret, because a right edit
// with the caret left in the wrong place is a bug the next keystroke reveals.
// ══════════════════════════════════════════════════════════════════════════

use super::config::{BracketPair, COMMON_BRACKETS};
use super::semantics::{self, MoveDir};

/// An editor over `text` with an explicit config.
fn editor_cfg(text: &str, config: CodeConfig) -> SharedState {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    construct(doc, CODE_EDITOR_PRESET, config, WrapMode::None)
}

/// The document's full text.
fn text_of(st: &SharedState) -> String {
    st.borrow().document.to_plain_text().unwrap()
}

/// Move the primary caret with no selection.
fn set_caret(st: &SharedState, pos: usize) {
    st.borrow().cursor.set_position(pos, MoveMode::MoveAnchor);
}

/// Select `[a, b]` on the primary caret.
fn set_selection(st: &SharedState, a: usize, b: usize) {
    let s = st.borrow();
    s.cursor.set_position(a, MoveMode::MoveAnchor);
    s.cursor.set_position(b, MoveMode::KeepAnchor);
}

/// The primary caret's position.
fn caret(st: &SharedState) -> usize {
    st.borrow().cursor.position()
}

/// Push an extra caret carrying a selection `[a, b]`.
fn add_extra_selection(st: &SharedState, a: usize, b: usize) {
    let mut s = st.borrow_mut();
    let c = s.document.cursor();
    c.set_position(a, MoveMode::MoveAnchor);
    c.set_position(b, MoveMode::KeepAnchor);
    s.extra_carets.push(c);
}

/// Run a semantics operation with the state borrowed mutably.
fn run(st: &SharedState, f: impl FnOnce(&mut CodeEditorState)) {
    let mut s = st.borrow_mut();
    f(&mut s);
}

/// A config with the common bracket pairs and auto-closing on.
fn bracket_config() -> CodeConfig {
    CodeConfig {
        brackets: COMMON_BRACKETS.to_vec(),
        auto_close_brackets: true,
        match_brackets: true,
        ..CodeConfig::default()
    }
}

// --- Auto-indent on Enter -------------------------------------------------

/// Enter carries the current line's indentation onto the new line, so the caret
/// lands under the code it came from rather than at column 0.
#[test]
fn enter_carries_leading_whitespace() {
    let st = editor_cfg("    foo", CodeConfig::default());
    set_caret(&st, 7); // end of "    foo"
    run(&st, semantics::newline);
    assert_eq!(text_of(&st), "    foo\n    ");
    assert_eq!(caret(&st), 12, "caret sits after the carried indent");
}

/// With auto-indent off, Enter is a plain break — the new line starts empty even
/// under indented code.
#[test]
fn enter_without_auto_indent_is_a_plain_break() {
    let st = editor_cfg(
        "    foo",
        CodeConfig {
            auto_indent: false,
            ..CodeConfig::default()
        },
    );
    set_caret(&st, 7);
    run(&st, semantics::newline);
    assert_eq!(text_of(&st), "    foo\n");
    assert_eq!(caret(&st), 8);
}

/// Enter with the caret between a configured pair opens a three-line block: the
/// opener's line, an indented empty middle line with the caret, and the closer
/// on its own line at the original indentation.
#[test]
fn enter_between_brackets_opens_a_block() {
    let st = editor_cfg("{}", bracket_config());
    set_caret(&st, 1); // between { and }
    run(&st, semantics::newline);
    assert_eq!(text_of(&st), "{\n    \n}");
    assert_eq!(caret(&st), 6, "caret rests on the indented middle line");
}

/// The block expansion carries the opener line's own indentation, so a nested
/// block indents one level deeper than its surroundings.
#[test]
fn enter_between_brackets_respects_existing_indentation() {
    let st = editor_cfg("    {}", bracket_config());
    set_caret(&st, 5); // between { and }
    run(&st, semantics::newline);
    // opener keeps 4, middle gets 8, closer returns to 4.
    assert_eq!(text_of(&st), "    {\n        \n    }");
}

// --- Smart Tab ------------------------------------------------------------

/// Tab with no selection inserts to the next tab stop, not a fixed run — from
/// column 0 that is a full level.
#[test]
fn tab_with_no_selection_inserts_to_the_next_stop() {
    let st = editor_cfg("", CodeConfig::default());
    set_caret(&st, 0);
    run(&st, semantics::indent_or_tab);
    assert_eq!(text_of(&st), "    ");
    assert_eq!(caret(&st), 4);
}

/// From a ragged column Tab inserts only enough to reach the stop, so
/// indentation stays on the grid.
#[test]
fn tab_from_a_ragged_column_reaches_the_stop() {
    let st = editor_cfg("ab", CodeConfig::default());
    set_caret(&st, 2); // column 2
    run(&st, semantics::indent_or_tab);
    assert_eq!(text_of(&st), "ab  ", "two spaces reach column 4");
    assert_eq!(caret(&st), 4);
}

/// Tab with a selection indents every touched line by a level, and the
/// selection still covers them afterward so a repeat keeps indenting.
#[test]
fn tab_with_a_selection_indents_every_touched_line() {
    let st = editor_cfg("a\nb", CodeConfig::default());
    set_selection(&st, 0, 3); // the whole document
    run(&st, semantics::indent_or_tab);
    assert_eq!(text_of(&st), "    a\n    b");
    // The selection grew to keep both indented lines covered.
    let s = st.borrow();
    assert_eq!(s.cursor.selection_start(), 0);
    assert_eq!(s.cursor.selection_end(), 11);
}

/// A selection ending exactly at the start of a line does not drag that line in.
#[test]
fn a_selection_ending_at_a_line_start_does_not_indent_that_line() {
    let st = editor_cfg("a\nb\nc", CodeConfig::default());
    // "a\nb\nc": block starts at 0, 2, 4. Select from 0 to 4 (the start of "c").
    set_selection(&st, 0, 4);
    run(&st, semantics::indent_or_tab);
    assert_eq!(text_of(&st), "    a\n    b\nc", "line c is untouched");
}

// --- Dedent ---------------------------------------------------------------

/// Shift+Tab removes one level from the touched lines.
#[test]
fn shift_tab_dedents_a_level() {
    let st = editor_cfg("        x", CodeConfig::default());
    set_caret(&st, 9);
    run(&st, semantics::dedent);
    assert_eq!(text_of(&st), "    x", "8 spaces → 4");
}

/// Dedent stops at the content: an under-indented line loses only the
/// whitespace it has, never a character of code.
#[test]
fn dedent_never_eats_content() {
    let st = editor_cfg("  x", CodeConfig::default());
    set_caret(&st, 3);
    run(&st, semantics::dedent);
    assert_eq!(text_of(&st), "x", "two spaces removed, the x kept");
}

/// A line with no indentation is left alone by dedent rather than erroring.
#[test]
fn dedent_of_an_unindented_line_is_a_no_op() {
    let st = editor_cfg("x", CodeConfig::default());
    set_caret(&st, 0);
    run(&st, semantics::dedent);
    assert_eq!(text_of(&st), "x");
}

// --- Toggle line comment --------------------------------------------------

/// Commenting then commenting again round-trips exactly, including the single
/// space after the token.
#[test]
fn comment_then_uncomment_round_trips() {
    let cfg = CodeConfig {
        line_comment: Some("//".to_string()),
        ..CodeConfig::default()
    };
    let st = editor_cfg("foo", cfg);
    set_caret(&st, 0);
    run(&st, semantics::toggle_line_comment);
    assert_eq!(text_of(&st), "// foo");
    run(&st, semantics::toggle_line_comment);
    assert_eq!(text_of(&st), "foo", "a second toggle restores the original");
}

/// A comment token with no configured comment is a no-op, not a guess — the
/// editor never invents `//` for a language whose comment is something else.
#[test]
fn comment_without_a_configured_token_does_nothing() {
    let st = editor_cfg("foo", CodeConfig::default());
    set_caret(&st, 0);
    run(&st, semantics::toggle_line_comment);
    assert_eq!(text_of(&st), "foo");
}

/// Comments align at the shallowest common indentation of the block, so a
/// nested region reads as one commented column rather than a ragged staircase.
#[test]
fn comment_aligns_at_the_shallowest_indent() {
    let cfg = CodeConfig {
        line_comment: Some("//".to_string()),
        ..CodeConfig::default()
    };
    // "  a" (indent 2) and "    b" (indent 4): min indent is 2.
    let st = editor_cfg("  a\n    b", cfg);
    set_selection(&st, 0, 9);
    run(&st, semantics::toggle_line_comment);
    assert_eq!(text_of(&st), "  // a\n  //   b");
}

/// A partly-commented block comments the rest rather than uncommenting the few:
/// the toggle only uncomments when every non-blank line already carries the
/// token.
#[test]
fn a_partly_commented_block_comments_the_remainder() {
    let cfg = CodeConfig {
        line_comment: Some("//".to_string()),
        ..CodeConfig::default()
    };
    let st = editor_cfg("// a\nb", cfg);
    set_selection(&st, 0, 6);
    run(&st, semantics::toggle_line_comment);
    assert_eq!(text_of(&st), "// // a\n// b");
}

// --- Duplicate ------------------------------------------------------------

/// Duplicating with no selection copies the whole line below, and the caret
/// follows the copy at the same column so a held key walks copies downward.
#[test]
fn duplicate_line_copies_below_and_follows_the_caret() {
    let st = editor_cfg("foo", CodeConfig::default());
    set_caret(&st, 1);
    run(&st, semantics::duplicate);
    assert_eq!(text_of(&st), "foo\nfoo");
    assert_eq!(caret(&st), 5, "caret is on the copy at the same column");
}

/// Duplicating a selection inserts the copy right after it and leaves the copy
/// selected, so repeating keeps duplicating.
#[test]
fn duplicate_selection_copies_after_and_reselects() {
    let st = editor_cfg("abcdef", CodeConfig::default());
    set_selection(&st, 1, 4); // "bcd"
    run(&st, semantics::duplicate);
    assert_eq!(text_of(&st), "abcdbcdef");
    let s = st.borrow();
    assert_eq!(s.cursor.selection_start(), 4);
    assert_eq!(s.cursor.selection_end(), 7, "the copy is selected");
}

// --- Move line ------------------------------------------------------------

/// Moving a line up swaps it with the line above, and the caret rides with it.
#[test]
fn move_line_up_swaps_with_the_line_above() {
    let st = editor_cfg("a\nb\nc", CodeConfig::default());
    set_caret(&st, 4); // on "c"
    run(&st, |s| semantics::move_lines(s, MoveDir::Up));
    assert_eq!(text_of(&st), "a\nc\nb");
    assert_eq!(caret(&st), 2, "caret still on the moved line");
}

/// Moving a line down swaps it with the line below.
#[test]
fn move_line_down_swaps_with_the_line_below() {
    let st = editor_cfg("a\nb\nc", CodeConfig::default());
    set_caret(&st, 0); // on "a"
    run(&st, |s| semantics::move_lines(s, MoveDir::Down));
    assert_eq!(text_of(&st), "b\na\nc");
    assert_eq!(caret(&st), 2);
}

/// Moving the top line up is a no-op — there is nothing to swap with.
#[test]
fn move_line_up_at_the_top_is_a_no_op() {
    let st = editor_cfg("a\nb", CodeConfig::default());
    set_caret(&st, 0);
    run(&st, |s| semantics::move_lines(s, MoveDir::Up));
    assert_eq!(text_of(&st), "a\nb");
}

/// Moving the bottom line down is a no-op.
#[test]
fn move_line_down_at_the_bottom_is_a_no_op() {
    let st = editor_cfg("a\nb", CodeConfig::default());
    set_caret(&st, 2); // on "b"
    run(&st, |s| semantics::move_lines(s, MoveDir::Down));
    assert_eq!(text_of(&st), "a\nb");
}

/// A multi-line selection moves as a block and stays selected.
#[test]
fn move_up_carries_a_multi_line_selection() {
    let st = editor_cfg("a\nb\nc\nd", CodeConfig::default());
    // Select "b\nc": block starts 0,2,4,6. Select 2..5.
    set_selection(&st, 2, 5);
    run(&st, |s| semantics::move_lines(s, MoveDir::Up));
    assert_eq!(text_of(&st), "b\nc\na\nd");
    let s = st.borrow();
    assert_eq!(s.cursor.selection_start(), 0);
    assert_eq!(
        s.cursor.selection_end(),
        3,
        "the moved block stays selected"
    );
}

// --- Brackets: auto-close, type-over, surround, pair-backspace ------------

/// Typing an opener inserts the closing partner and leaves the caret between.
#[test]
fn auto_close_inserts_the_pair_and_sits_between() {
    let st = editor_cfg("", bracket_config());
    set_caret(&st, 0);
    run(&st, |s| semantics::type_bracket_char(s, '('));
    assert_eq!(text_of(&st), "()");
    assert_eq!(caret(&st), 1, "caret rests between the pair");
}

/// Auto-close is suppressed when a word follows: typing `(` before `foo` must
/// not produce `(foo)`, which would swallow the identifier.
#[test]
fn auto_close_is_suppressed_before_a_word() {
    let st = editor_cfg("foo", bracket_config());
    set_caret(&st, 0);
    run(&st, |s| semantics::type_bracket_char(s, '('));
    assert_eq!(text_of(&st), "(foo", "no closer inserted before the word");
    assert_eq!(caret(&st), 1);
}

/// Typing an opener with a selection surrounds it and keeps it selected.
#[test]
fn auto_close_surrounds_a_selection() {
    let st = editor_cfg("abc", bracket_config());
    set_selection(&st, 0, 3);
    run(&st, |s| semantics::type_bracket_char(s, '('));
    assert_eq!(text_of(&st), "(abc)");
    let s = st.borrow();
    assert_eq!(s.cursor.selection_start(), 1);
    assert_eq!(
        s.cursor.selection_end(),
        4,
        "the wrapped text stays selected"
    );
}

/// Typing a closer where one already sits steps over it, so the auto-inserted
/// closer is not doubled.
#[test]
fn typing_a_closer_steps_over_the_auto_closed_one() {
    let st = editor_cfg("()", bracket_config());
    set_caret(&st, 1); // between the pair
    run(&st, |s| semantics::type_bracket_char(s, ')'));
    assert_eq!(text_of(&st), "()", "no second closer inserted");
    assert_eq!(caret(&st), 2, "caret stepped past the closer");
}

/// Backspace between an empty auto-closed pair deletes both.
#[test]
fn pair_backspace_deletes_both() {
    let st = editor_cfg("()", bracket_config());
    set_caret(&st, 1);
    let consumed = run_bool(&st, semantics::try_pair_backspace);
    assert!(consumed, "pair-backspace claims the keystroke");
    assert_eq!(text_of(&st), "");
}

/// Pair-backspace declines when the caret is not between a matching pair, so the
/// ordinary delete runs instead.
#[test]
fn pair_backspace_declines_when_not_between_a_pair() {
    let st = editor_cfg("ab", bracket_config());
    set_caret(&st, 1);
    let consumed = run_bool(&st, semantics::try_pair_backspace);
    assert!(!consumed, "not a pair → declines");
    assert_eq!(text_of(&st), "ab", "and changes nothing");
}

/// Run a semantics operation that returns a bool.
fn run_bool(st: &SharedState, f: impl FnOnce(&mut CodeEditorState) -> bool) -> bool {
    let mut s = st.borrow_mut();
    f(&mut s)
}

// --- Add caret above / below ----------------------------------------------

/// Adding a caret below lands it at the same column on the next line.
#[test]
fn add_caret_below_lands_at_the_same_column() {
    let st = editor_cfg("abc\ndef", CodeConfig::default());
    set_caret(&st, 1); // column 1 of "abc"
    run(&st, semantics::add_caret_below);
    let s = st.borrow();
    assert_eq!(s.all_carets().count(), 2);
    // "abc\ndef": "def" starts at 4, so column 1 is position 5.
    assert!(s.extra_carets.iter().any(|c| c.position() == 5));
}

/// Adding a caret above lands it at the same column on the previous line, and
/// clamps to that line's end when it is shorter.
#[test]
fn add_caret_above_clamps_to_a_shorter_line() {
    let st = editor_cfg("ab\nwxyz", CodeConfig::default());
    set_caret(&st, 7); // column 3 of "wxyz" (starts at 3)
    run(&st, semantics::add_caret_above);
    let s = st.borrow();
    assert_eq!(s.all_carets().count(), 2);
    // "ab" is only two long, so column 3 clamps to its end at position 2.
    assert!(s.extra_carets.iter().any(|c| c.position() == 2));
}

/// Adding a caret below at the last line is a no-op — there is no line under it.
#[test]
fn add_caret_below_at_the_last_line_is_a_no_op() {
    let st = editor_cfg("only", CodeConfig::default());
    set_caret(&st, 2);
    run(&st, semantics::add_caret_below);
    assert_eq!(st.borrow().all_carets().count(), 1);
}

// --- Multi-caret ----------------------------------------------------------

/// A line operation covers the union of the lines every caret touches, and only
/// those — an untouched line in the middle stays put.
#[test]
fn multi_caret_indent_covers_each_carets_line_only() {
    let st = editor_cfg("aa\nbb\ncc", CodeConfig::default());
    // "aa\nbb\ncc": blocks at 0, 3, 6.
    set_selection(&st, 0, 1); // in "aa"
    add_extra_selection(&st, 6, 7); // in "cc"
    run(&st, semantics::indent_or_tab);
    assert_eq!(
        text_of(&st),
        "    aa\nbb\n    cc",
        "aa and cc indent; the untouched bb does not"
    );
}

/// Typing a bracket at several carets closes the pair at each — the column-edit
/// case.
#[test]
fn multi_caret_auto_close_inserts_a_pair_at_each() {
    let st = editor_cfg("a\nb", bracket_config());
    set_caret(&st, 1); // end of "a"
    add_extra_selection(&st, 3, 3); // end of "b" (empty selection = bare caret)
    run(&st, |s| semantics::type_bracket_char(s, '('));
    assert_eq!(text_of(&st), "a()\nb()");
}

// --- Bracket matching -----------------------------------------------------

/// The match of the delimiter next to the caret is found, preferring the one
/// just behind the caret (where it sits after typing).
#[test]
fn bracket_match_finds_the_partner() {
    let st = editor_cfg("(a)", bracket_config());
    set_caret(&st, 1); // just after '('
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, Some((0, 2)), "the '(' at 0 matches the ')' at 2");
}

/// Matching counts nesting, so an inner pair does not steal the outer match.
#[test]
fn bracket_match_respects_nesting() {
    let st = editor_cfg("(())", bracket_config());
    set_caret(&st, 1); // after the outer '('
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, Some((0, 3)), "the outer '(' matches the outer ')'");
}

/// A caret just after a closer matches backward to its opener — the scan runs
/// both directions, and this is the one that walks toward the start.
#[test]
fn bracket_match_scans_backward_from_a_closer() {
    let st = editor_cfg("(a)", bracket_config());
    set_caret(&st, 3); // just after ')'
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, Some((2, 0)), "the ')' at 2 matches the '(' at 0");
}

/// The backward scan crosses line boundaries, matching a closer on one line to
/// an opener on a line above.
#[test]
fn bracket_match_crosses_lines_backward() {
    let st = editor_cfg("(\n)", bracket_config());
    // "(\n)": block "(" at 0, block ")" at 2. Caret after ')'.
    set_caret(&st, 3);
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(
        m,
        Some((2, 0)),
        "the ')' on line 2 matches the '(' on line 1"
    );
}

/// The forward scan crosses line boundaries too.
#[test]
fn bracket_match_crosses_lines_forward() {
    let st = editor_cfg("(\n)", bracket_config());
    set_caret(&st, 1); // just after '(' on the first line
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, Some((0, 2)));
}

/// An unbalanced bracket has no match, and the scan reports none rather than
/// running to the end of the document.
#[test]
fn bracket_match_is_none_when_unbalanced() {
    let st = editor_cfg("(a", bracket_config());
    set_caret(&st, 1);
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, None);
}

/// With no configured pairs there is no matching, whatever the text.
#[test]
fn bracket_match_needs_configured_pairs() {
    let st = editor_cfg("(a)", CodeConfig::default());
    set_caret(&st, 1);
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, None);
}

// --- Undo grouping --------------------------------------------------------

/// A single-caret Enter is one undo step even though auto-indent makes it two
/// mutations (the break and the carried indent) — "undo my Enter" must reverse
/// both at once, not leave a stray blank indent behind.
#[test]
fn a_single_caret_enter_is_one_undo_step() {
    let st = editor_cfg("    foo", CodeConfig::default());
    set_caret(&st, 7);
    run(&st, semantics::newline);
    assert_eq!(text_of(&st), "    foo\n    ");
    assert!(st.borrow().document.undo().is_ok());
    assert_eq!(
        text_of(&st),
        "    foo",
        "one undo restores the pre-Enter text"
    );
}

/// The bracket-block expansion is four mutations, and one undo reverses all of
/// them.
#[test]
fn a_bracket_expand_enter_is_one_undo_step() {
    let st = editor_cfg("{}", bracket_config());
    set_caret(&st, 1);
    run(&st, semantics::newline);
    assert_eq!(text_of(&st), "{\n    \n}");
    assert!(st.borrow().document.undo().is_ok());
    assert_eq!(text_of(&st), "{}", "one undo restores the collapsed pair");
}

/// A single-caret duplicate is one undo step (break + copied text undo
/// together), not two.
#[test]
fn a_single_caret_duplicate_is_one_undo_step() {
    let st = editor_cfg("foo", CodeConfig::default());
    set_caret(&st, 1);
    run(&st, semantics::duplicate);
    assert_eq!(text_of(&st), "foo\nfoo");
    assert!(st.borrow().document.undo().is_ok());
    assert_eq!(text_of(&st), "foo", "one undo removes the whole copy");
}

/// Surrounding a selection is one undo step, not one per inserted delimiter.
#[test]
fn a_single_caret_surround_is_one_undo_step() {
    let st = editor_cfg("abc", bracket_config());
    set_selection(&st, 0, 3);
    run(&st, |s| semantics::type_bracket_char(s, '('));
    assert_eq!(text_of(&st), "(abc)");
    assert!(st.borrow().document.undo().is_ok());
    assert_eq!(text_of(&st), "abc", "one undo strips both delimiters");
}

/// An unmatched opener whose scan must cross line boundaries to reach the end of
/// the document reports no match — and, with the clamp-aware crossing, does so by
/// terminating at end-of-document rather than spinning the last line to the cap.
#[test]
fn bracket_match_unmatched_opener_across_lines_terminates() {
    let st = editor_cfg("(\na\nb", bracket_config());
    set_caret(&st, 1); // just after the unmatched '('
    let m = semantics::current_bracket_match(&st.borrow());
    assert_eq!(m, None, "no closer anywhere below → no match");
}

/// A multi-line indent is one undo step, so undo restores the whole block in a
/// single press rather than one line at a time.
#[test]
fn a_multi_line_indent_is_one_undo_step() {
    let st = editor_cfg("a\nb", CodeConfig::default());
    set_selection(&st, 0, 3);
    run(&st, semantics::indent_or_tab);
    assert_eq!(text_of(&st), "    a\n    b");
    let ok = st.borrow().document.undo();
    assert!(ok.is_ok());
    assert_eq!(text_of(&st), "a\nb", "one undo restores both lines");
}

/// Bracket configuration is a value, not a language: a widget-declared
/// `BracketPair` closes the way the app said, with no built-in table.
#[test]
fn brackets_come_from_configuration_not_a_language() {
    let cfg = CodeConfig {
        brackets: vec![BracketPair::new('«', '»')],
        auto_close_brackets: true,
        ..CodeConfig::default()
    };
    let st = editor_cfg("", cfg);
    set_caret(&st, 0);
    run(&st, |s| semantics::type_bracket_char(s, '«'));
    assert_eq!(text_of(&st), "«»", "the app's own pair auto-closes");
}
