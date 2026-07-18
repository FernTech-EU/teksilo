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
    // The walk derives synthetic ids from an owner widget, as the real tree does.
    let mut b = bastyde_core::accessibility::AccessNodeBuilder::for_widget(
        bastyde_core::widget_id::WidgetId::default(),
    );
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
    let mut b = bastyde_core::accessibility::AccessNodeBuilder::for_widget(
        bastyde_core::widget_id::WidgetId::default(),
    );
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

/// Build the body's accessibility subtree and return its collected children
/// (the paragraph and text-run nodes).
fn a11y_children(
    st: &SharedState,
) -> Vec<(
    bastyde_core::accesskit::NodeId,
    bastyde_core::accesskit::Node,
)> {
    use bastyde_core::widget_id::WidgetId;
    let body = body_for(st, None, None);
    let mut b = bastyde_core::accessibility::AccessNodeBuilder::for_widget(WidgetId::default());
    body.accessibility(&mut b);
    let (_id, _node, children) = b.build(WidgetId::default());
    children
}

fn roles(
    children: &[(
        bastyde_core::accesskit::NodeId,
        bastyde_core::accesskit::Node,
    )],
    role: bastyde_core::accesskit::Role,
) -> Vec<&bastyde_core::accesskit::Node> {
    children
        .iter()
        .filter(|(_, n)| n.role() == role)
        .map(|(_, n)| n)
        .collect()
}

/// Pull the reported caret (the selection focus) out of an AccessKit
/// `TreeUpdate` and resolve it back to an absolute document offset via the
/// state's synthetic-run map — the resolution an AT client performs. Returns
/// `None` when no node in the update reported a text selection.
fn resolved_caret(update: &bastyde_core::accesskit::TreeUpdate, st: &SharedState) -> Option<usize> {
    let sel = update.nodes.iter().find_map(|(_, n)| n.text_selection())?;
    let borrow = st.borrow();
    let map = borrow.synthetic_to_element.borrow();
    map.get(&sel.focus.node)
        .map(|er| er.absolute_start + sel.focus.character_index)
}

/// A caret-only move (no edit) must re-walk the accessibility tree so the
/// reported selection tracks the caret. The a11y walk reads the *live* cursor,
/// so this can only regress at the binding level: the caret signals must be
/// bound `AccessibilityOnly`, not repaint-only — otherwise `a11y_dirty` never
/// flips, `sync_accessibility` returns the stale cached tree, and a screen
/// reader hears the caret frozen at the last edit.
#[test]
fn a_caret_move_alone_rewalks_the_accessibility_selection() {
    let st = editor_state("hello world");
    let mut tree = WidgetTree::new();
    let _ = tree.add(body_for(&st, None, None));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Baseline walk with the caret at 0 — this fills the cached AT tree.
    let base = tree.sync_accessibility();
    assert_eq!(resolved_caret(&base, &st), Some(0), "baseline caret at 0");

    // Move the caret to 5 with NO edit, exactly as an arrow key does: mutate the
    // cursor, then publish the caret signals.
    st.borrow().cursor.set_position(5, MoveMode::MoveAnchor);
    super::sync_cursor_signals(&st);
    // The layout pass drains the AccessibilityOnly binding into `a11y_dirty`.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let after = tree.sync_accessibility();
    assert_eq!(
        resolved_caret(&after, &st),
        Some(5),
        "the AT selection must follow a caret-only move (0 = stale cached tree)",
    );
}

/// Each line becomes a `Role::Paragraph` with at least one `Role::TextRun`.
#[test]
fn the_walk_emits_a_paragraph_and_runs_per_line() {
    use bastyde_core::accesskit::Role;
    let st = editor_state("alpha\nbeta\ngamma");
    let children = a11y_children(&st);
    assert_eq!(
        roles(&children, Role::Paragraph).len(),
        3,
        "one paragraph a line"
    );
    assert!(
        roles(&children, Role::TextRun).len() >= 3,
        "at least one run a line"
    );
}

/// Every paragraph is numbered in the set ("line 2 of 3") — carried on the line,
/// not spoken from a gutter.
#[test]
fn each_line_is_numbered_in_the_set() {
    use bastyde_core::accesskit::Role;
    let st = editor_state("one\ntwo\nthree");
    let children = a11y_children(&st);
    let paras = roles(&children, Role::Paragraph);
    assert_eq!(paras.len(), 3);
    for (i, p) in paras.iter().enumerate() {
        assert_eq!(p.position_in_set(), Some(i + 1), "1-based line number");
        assert_eq!(p.size_of_set(), Some(3), "of the total line count");
    }
}

/// Each line's last run ends with the newline AccessKit's line-navigation
/// contract requires.
#[test]
fn each_line_ends_with_a_newline() {
    use bastyde_core::accesskit::Role;
    let st = editor_state("a\nb\nc");
    let children = a11y_children(&st);
    let runs = roles(&children, Role::TextRun);
    assert_eq!(runs.len(), 3, "single-char lines: one run each");
    for run in runs {
        assert!(
            run.value().is_some_and(|v| v.ends_with('\n')),
            "the line's last run must end with the newline, got {:?}",
            run.value()
        );
    }
}

/// A run longer than 255 characters is split into linked runs, so its word
/// starts (character indices stored as u8) never overflow and word navigation
/// keeps working past character 255.
#[test]
fn a_long_line_is_split_into_linked_runs() {
    use bastyde_core::accesskit::Role;
    let long: String = "x".repeat(600);
    let st = editor_state(&long);
    let children = a11y_children(&st);
    let runs = roles(&children, Role::TextRun);
    assert!(
        runs.len() >= 3,
        "600 chars must split into >=3 runs, got {}",
        runs.len()
    );
    assert!(runs[0].next_on_line().is_some(), "first run links forward");
    assert!(
        runs.last().unwrap().previous_on_line().is_some(),
        "last run links back"
    );
    for run in &runs {
        assert!(
            run.character_lengths().len() <= 256,
            "a chunk (plus its optional newline) stays within the cap"
        );
    }
}

/// The synthetic map an AT SetTextSelection resolves through points each run at
/// the right document position.
#[test]
fn the_selection_map_locates_runs_in_the_document() {
    let st = editor_state("hello\nworld");
    let _ = a11y_children(&st); // populates synthetic_to_element as a side effect
    let s = st.borrow();
    let map = s.synthetic_to_element.borrow();
    let world = map.values().find(|r| r.text == "world");
    assert!(world.is_some(), "the 'world' run must be mapped");
    assert_eq!(
        world.unwrap().absolute_start,
        6,
        "at the block's document start"
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

use super::config::{BracketPair, COMMON_BRACKETS, IndentStyle};
use super::semantics::{self, MoveDir};
use super::widget::{CodeEditor, PlainTextEditor};

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

// --- Clipboard ------------------------------------------------------------

/// Paste splits multi-line text into one block per line — never a single block
/// carrying literal newlines, which would break the one-block-per-line model.
#[test]
fn paste_splits_multiline_into_one_block_per_line() {
    let st = editor_cfg("", CodeConfig::default());
    super::clipboard::insert_multiline(&st.borrow().cursor, "a\nb\nc");
    assert_eq!(text_of(&st), "a\nb\nc");
    assert_eq!(
        st.borrow().document.block_count(),
        3,
        "three lines → three blocks, no literal newline in any block"
    );
}

/// A single-line paste stays one block.
#[test]
fn paste_of_one_line_stays_one_block() {
    let st = editor_cfg("", CodeConfig::default());
    super::clipboard::insert_multiline(&st.borrow().cursor, "hello");
    assert_eq!(text_of(&st), "hello");
    assert_eq!(st.borrow().document.block_count(), 1);
}

/// The whole paste is one undo step, however many lines it spans.
#[test]
fn a_multi_line_paste_is_one_undo_step() {
    let st = editor_cfg("start", CodeConfig::default());
    set_caret(&st, 5);
    super::clipboard::insert_multiline(&st.borrow().cursor, "\nsecond\nthird");
    assert_eq!(text_of(&st), "start\nsecond\nthird");
    assert!(st.borrow().document.undo().is_ok());
    assert_eq!(text_of(&st), "start", "one undo removes the whole paste");
}

/// Windows line endings are normalised before splitting, so a pasted CRLF file
/// leaves no stray carriage returns in the blocks.
#[test]
fn paste_normalises_crlf() {
    let st = editor_cfg("", CodeConfig::default());
    let normalized = "a\r\nb".replace("\r\n", "\n").replace('\r', "\n");
    super::clipboard::insert_multiline(&st.borrow().cursor, &normalized);
    assert_eq!(text_of(&st), "a\nb");
    assert_eq!(st.borrow().document.block_count(), 2);
}

/// Cutting a line with a following line removes the line and its trailing
/// separator, pulling the next line up.
#[test]
fn cut_line_removes_the_line_and_its_trailing_separator() {
    let st = editor_cfg("a\nb\nc", CodeConfig::default());
    set_caret(&st, 2); // on "b"
    run(&st, super::clipboard::delete_line);
    assert_eq!(text_of(&st), "a\nc");
}

/// Cutting the last line takes the leading separator instead, so the previous
/// line does not keep a dangling break.
#[test]
fn cut_the_last_line_takes_the_leading_separator() {
    let st = editor_cfg("a\nb", CodeConfig::default());
    set_caret(&st, 2); // on the last line "b"
    run(&st, super::clipboard::delete_line);
    assert_eq!(text_of(&st), "a");
}

/// Cutting the only line just clears it, leaving an empty document rather than
/// underflowing on a separator that is not there.
#[test]
fn cut_the_only_line_clears_it() {
    let st = editor_cfg("solo", CodeConfig::default());
    set_caret(&st, 2);
    run(&st, super::clipboard::delete_line);
    assert_eq!(text_of(&st), "");
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

// ══════════════════════════════════════════════════════════════════════════
// Wrapper widgets (Phase 4e): CodeEditor / PlainTextEditor
// ══════════════════════════════════════════════════════════════════════════

fn doc(text: &str) -> TextDocument {
    let d = TextDocument::new();
    d.set_plain_text(text).unwrap();
    d
}

/// A CodeEditor turns the code affordances on by default: gutter and the
/// current-line highlight.
#[test]
fn a_code_editor_defaults_current_line_on() {
    let ed = CodeEditor::new(doc("fn main() {}"));
    assert!(
        ed.handle().state_handle().borrow().current_line_highlight,
        "CodeEditor should default the current-line band on"
    );
}

/// A PlainTextEditor is the same core with the code chrome off and prose
/// wrapping on.
#[test]
fn a_plain_text_editor_defaults_to_prose() {
    let ed = PlainTextEditor::new(doc("some notes"));
    let st = ed.handle().state_handle();
    let s = st.borrow();
    assert!(!s.current_line_highlight, "no current-line band in prose");
    assert_eq!(s.wrap_mode, WrapMode::Word, "prose wraps");
}

/// Builder knobs thread into the injected config, not a hidden language table.
#[test]
fn builder_knobs_thread_into_the_config() {
    let ed = CodeEditor::new(doc("x"))
        .tab_width(2)
        .line_comment("#")
        .bracket_pairs(COMMON_BRACKETS.to_vec())
        .auto_close_brackets(true)
        .bracket_matching(true);
    let st = ed.handle().state_handle();
    let s = st.borrow();
    assert_eq!(s.config.indent, IndentStyle::Spaces(2));
    assert_eq!(s.config.line_comment.as_deref(), Some("#"));
    assert_eq!(s.config.brackets, COMMON_BRACKETS.to_vec());
    assert!(s.config.auto_close_brackets);
    assert!(s.config.match_brackets);
}

/// Soft/hard tabs flip the indent kind while keeping the width.
#[test]
fn use_soft_tabs_flips_the_indent_kind() {
    let hard = CodeEditor::new(doc("x")).tab_width(8).use_soft_tabs(false);
    assert_eq!(
        hard.handle().state_handle().borrow().config.indent,
        IndentStyle::Tabs { width: 8 }
    );
    let soft = CodeEditor::new(doc("x")).use_soft_tabs(true);
    assert!(matches!(
        soft.handle().state_handle().borrow().config.indent,
        IndentStyle::Spaces(_)
    ));
}

/// A read-only wrapper is a viewer: caret hidden, mutations rejected.
#[test]
fn a_read_only_code_editor_is_a_viewer() {
    let ed = CodeEditor::read_only(doc("fn main() {}"));
    let st = ed.handle().state_handle();
    let s = st.borrow();
    assert!(!s.caret_visible.get(), "a viewer starts with no caret");
    assert!(s.policy.is_read_only());
}

/// The editor mounts, lays out, and paints headlessly without panicking — the
/// full wrapper path (gutter + body + scrollbars + the paint band).
#[test]
fn a_code_editor_mounts_lays_out_and_paints() {
    let mut tree = WidgetTree::new();
    let ed = CodeEditor::new(doc("fn main() {\n    let x = (1 + 2);\n}"))
        .bracket_pairs(COMMON_BRACKETS.to_vec())
        .bracket_matching(true);
    let id = tree.add(ed);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let b = tree.bounds(id);
    assert_eq!(b.width, 600.0);
    assert_eq!(b.height, 400.0);
    // Paint must not panic (band + brackets are gated on layout, so this also
    // exercises the pre-layout skip path on the first frame).
    let _ = tree.render();
}

/// A gutter-less editor still mounts and fills its bounds.
#[test]
fn a_gutterless_code_editor_mounts() {
    let mut tree = WidgetTree::new();
    let ed = CodeEditor::new(doc("a\nb\nc")).gutter(false);
    let id = tree.add(ed);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tree.bounds(id).width, 400.0);
    let _ = tree.render();
}

/// The PlainTextEditor mounts and fills its bounds (it delegates to an inner
/// CodeEditor).
#[test]
fn a_plain_text_editor_mounts_and_fills() {
    let mut tree = WidgetTree::new();
    let ed = PlainTextEditor::new(doc("just some prose here"));
    let id = tree.add(ed);
    tree.layout(SizeProposal::exact(500.0, 200.0));
    assert_eq!(tree.bounds(id).width, 500.0);
    assert_eq!(tree.bounds(id).height, 200.0);
    let _ = tree.render();
}

/// With `min_lines` the editor sizes intrinsically instead of greedily filling,
/// so an unbounded-height proposal yields a bounded height.
#[test]
fn min_lines_gives_intrinsic_height() {
    let mut tree = WidgetTree::new();
    let ed = PlainTextEditor::new(doc("one line"))
        .min_lines(3)
        .max_lines(6);
    let id = tree.add(ed);
    // Unbounded height (only width fixed) — a greedy editor would take a
    // default; an intrinsic one clamps to [3, 6] lines.
    tree.layout(SizeProposal::with_width(400.0));
    let h = tree.bounds(id).height;
    assert!(h > 0.0, "intrinsic height must be positive, got {h}");
    assert!(
        h < 400.0,
        "3–6 lines must be far shorter than a page, got {h}"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// Completion (Phase 4e)
// ══════════════════════════════════════════════════════════════════════════

use super::completion::{self, CompletionContext, CompletionItem, CompletionKind, Trigger};

/// A provider that always returns `labels`, ignoring the context.
fn provider_of(labels: &[&str]) -> impl Fn(&CompletionContext) -> Vec<CompletionItem> + 'static {
    let owned: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
    move |_cx| {
        owned
            .iter()
            .map(|l| CompletionItem::new(l.clone()))
            .collect()
    }
}

/// An editor with a completion provider, caret placed, ready to evaluate.
fn completion_editor(text: &str, caret: usize, labels: &[&str]) -> SharedState {
    let st = editor_cfg(text, CodeConfig::default());
    st.borrow_mut().completion.provider = Some(std::rc::Rc::new(provider_of(labels)));
    st.borrow().cursor.set_position(caret, MoveMode::MoveAnchor);
    st
}

// --- Word prefix + accept -------------------------------------------------

/// The prefix is the identifier run immediately before the caret, and its start
/// position, stopping at a non-word character.
#[test]
fn word_prefix_extracts_the_identifier_before_the_caret() {
    let st = editor_cfg("foo.bar", CodeConfig::default());
    let (start, prefix) = semantics::word_prefix_before_caret(&st.borrow(), 7);
    assert_eq!(prefix, "bar");
    assert_eq!(start, 4, "starts after the dot");
}

/// A caret right after a non-word character has an empty prefix.
#[test]
fn word_prefix_is_empty_after_a_separator() {
    let st = editor_cfg("foo.", CodeConfig::default());
    let (start, prefix) = semantics::word_prefix_before_caret(&st.borrow(), 4);
    assert_eq!(prefix, "");
    assert_eq!(start, 4);
}

/// Underscores and digits continue an identifier; a dot does not.
#[test]
fn word_prefix_includes_underscores_and_digits() {
    let st = editor_cfg("a1_b2", CodeConfig::default());
    let (start, prefix) = semantics::word_prefix_before_caret(&st.borrow(), 5);
    assert_eq!(prefix, "a1_b2");
    assert_eq!(start, 0);
}

/// Accepting replaces the prefix with the inserted text, as one undo step.
#[test]
fn accept_replaces_the_prefix_in_one_undo_step() {
    let st = editor_cfg("list.pu", CodeConfig::default());
    st.borrow().cursor.set_position(7, MoveMode::MoveAnchor);
    {
        let mut s = st.borrow_mut();
        semantics::accept_completion(&mut s, 5, 7, "push");
    }
    assert_eq!(text_of(&st), "list.push");
    assert!(st.borrow().document.undo().is_ok());
    assert_eq!(
        text_of(&st),
        "list.pu",
        "one undo restores the pre-accept text"
    );
}

// --- Item types -----------------------------------------------------------

/// A CompletionItem's inserted text defaults to its label; the builder overrides.
#[test]
fn completion_item_defaults_and_builders() {
    let a = CompletionItem::new("foo");
    assert_eq!(a.label, "foo");
    assert_eq!(a.insert_text, "foo");
    let b = CompletionItem::new("println")
        .insert_text("println!()")
        .detail("macro")
        .kind(CompletionKind::Keyword);
    assert_eq!(b.insert_text, "println!()");
    assert_eq!(b.detail.as_deref(), Some("macro"));
    assert_eq!(b.kind, CompletionKind::Keyword);
}

// --- Filtering ------------------------------------------------------------

/// The typed prefix filters the candidate list (case-insensitive prefix match).
#[test]
fn typing_filters_the_candidate_list() {
    let st = completion_editor("pre", 3, &["prefix", "press", "bar", "Preset"]);
    completion::test_evaluate(&st, Trigger::Typed);
    let labels = st.borrow().completion.test_labels();
    assert_eq!(
        labels,
        vec!["prefix", "press", "Preset"],
        "case-insensitive prefix match on 'pre'"
    );
}

/// A prefix matching nothing yields an empty filtered list (the popup closes).
#[test]
fn a_prefix_matching_nothing_filters_to_empty() {
    let st = completion_editor("xyz", 3, &["foo", "bar"]);
    completion::test_evaluate(&st, Trigger::Typed);
    assert!(st.borrow().completion.test_labels().is_empty());
}

/// A forced trigger (Ctrl+Space) with an empty prefix shows everything.
#[test]
fn forced_trigger_with_no_prefix_shows_all() {
    let st = completion_editor("", 0, &["alpha", "beta", "gamma"]);
    completion::test_evaluate(&st, Trigger::Forced);
    assert_eq!(st.borrow().completion.test_labels().len(), 3);
}

/// An auto (typed) trigger does not fire without a provider — completion is
/// strictly opt-in.
#[test]
fn no_provider_means_no_completion() {
    let st = editor_cfg("pre", CodeConfig::default());
    st.borrow().cursor.set_position(2, MoveMode::MoveAnchor);
    assert!(!st.borrow().completion.has_provider());
    assert!(st.borrow().completion.test_labels().is_empty());
}

// --- Selection ------------------------------------------------------------

/// Arrowing the selection wraps around the filtered list.
#[test]
fn move_selection_wraps_around() {
    let st = completion_editor("pre", 3, &["prefix", "press", "preset"]);
    completion::test_evaluate(&st, Trigger::Typed);
    assert_eq!(st.borrow().completion.selected.get(), 0);
    completion::move_selection(&st, -1);
    assert_eq!(
        st.borrow().completion.selected.get(),
        2,
        "up from the top wraps to the end"
    );
    completion::move_selection(&st, 1);
    assert_eq!(
        st.borrow().completion.selected.get(),
        0,
        "down from the end wraps to the top"
    );
}

// --- Panel widget ---------------------------------------------------------

/// The panel mounts headlessly with a populated session and does not panic on
/// layout or paint.
#[test]
fn the_completion_panel_mounts_with_a_session() {
    let st = completion_editor("pre", 3, &["prefix", "press", "preset"]);
    completion::test_evaluate(&st, Trigger::Typed);
    st.borrow().completion.open.set(true);
    let mut tree = WidgetTree::new();
    tree.add(completion::CompletionPanel::new(&st));
    tree.layout(SizeProposal::with_width(360.0));
    let _ = tree.render();
}

/// An empty session paints nothing rather than an empty box.
#[test]
fn the_completion_panel_is_empty_without_a_session() {
    let st = completion_editor("", 0, &["foo"]);
    let mut tree = WidgetTree::new();
    let id = tree.add(completion::CompletionPanel::new(&st));
    tree.layout(SizeProposal::with_width(360.0));
    assert_eq!(tree.bounds(id).height, 0.0, "no rows → no height");
}

/// A CodeEditor with a provider mounts and exposes the popup a11y (has-popup),
/// and one without does not claim completion at all.
#[test]
fn a_code_editor_with_completion_mounts() {
    let mut tree = WidgetTree::new();
    let ed = CodeEditor::new(doc("fn main() {}"))
        .completion_provider(|_cx| vec![CompletionItem::new("main"), CompletionItem::new("map")]);
    let id = tree.add(ed);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    assert_eq!(tree.bounds(id).width, 600.0);
    let _ = tree.render();
}

// --- Review regression tests (adversarial-review fixes) -------------------

/// A completion provider may reach back into the editor's shared state (via a
/// captured handle) without panicking — it is called OUTSIDE the editor's
/// borrow. Regression for the RefCell re-entrancy the review found.
#[test]
fn a_provider_may_read_the_editor_state_without_panicking() {
    let st = editor_cfg("pre", CodeConfig::default());
    st.borrow().cursor.set_position(3, MoveMode::MoveAnchor);
    let probe = st.clone();
    st.borrow_mut().completion.provider = Some(std::rc::Rc::new(move |_cx| {
        // Would panic ("already mutably borrowed") if the provider ran while the
        // editor state was borrowed.
        let _ = probe.borrow().cursor.position();
        vec![CompletionItem::new("prefix")]
    }));
    completion::test_evaluate(&st, Trigger::Typed);
    assert_eq!(st.borrow().completion.test_labels(), vec!["prefix"]);
}

/// identifier_end extends past the caret through the rest of the word, so a
/// mid-word accept replaces the whole identifier.
#[test]
fn identifier_end_extends_through_the_word() {
    let st = editor_cfg("list.pushx", CodeConfig::default());
    // caret after "pu" (position 7); the identifier "pushx" ends at 10.
    assert_eq!(semantics::identifier_end(&st.borrow(), 7), 10);
}

/// Accepting with the caret mid-word replaces the entire identifier, not just up
/// to the caret — no dangling tail. Regression for the review's stale-range bug.
#[test]
fn accept_replaces_the_whole_identifier_mid_word() {
    let st = editor_cfg("list.puX", CodeConfig::default());
    // caret after "pu" (pos 7), before the trailing "X" (a leftover letter).
    st.borrow().cursor.set_position(7, MoveMode::MoveAnchor);
    {
        let mut s = st.borrow_mut();
        let end = semantics::identifier_end(&s, 7);
        semantics::accept_completion(&mut s, 5, end, "push");
    }
    assert_eq!(text_of(&st), "list.push", "the trailing X is replaced too");
}

/// Completion is single-caret: with extra carets active, it does not open (so
/// accept can never silently discard the extra carets).
#[test]
fn completion_does_not_activate_with_multiple_carets() {
    let st = completion_editor("pre", 3, &["prefix", "press"]);
    let extra = st.borrow().document.cursor();
    st.borrow_mut().extra_carets.push(extra);
    completion::test_evaluate(&st, Trigger::Typed);
    assert!(
        st.borrow().completion.test_labels().is_empty(),
        "no completion session while multiple carets are live"
    );
}

/// Ctrl+Space (Forced) lifts an Escape suppression, so the popup reopens on the
/// same word. Regression for the review's stuck-suppression bug.
#[test]
fn a_forced_trigger_lifts_escape_suppression() {
    let st = completion_editor("pre", 3, &["prefix", "press"]);
    // Suppress at the word start (position 0 — "pre" starts at 0).
    st.borrow_mut().completion.test_set_suppressed(Some(0));
    // A plain typed trigger stays suppressed.
    completion::test_evaluate(&st, Trigger::Typed);
    assert!(
        st.borrow().completion.test_labels().is_empty(),
        "suppressed: typing does not reopen"
    );
    // Ctrl+Space forces it open again.
    completion::test_evaluate(&st, Trigger::Forced);
    assert_eq!(
        st.borrow().completion.test_labels(),
        vec!["prefix", "press"],
        "Forced lifts the suppression"
    );
}

/// A caret move (Trigger::Moved) never opens a closed popup — only typing or a
/// forced request does. So navigating onto a word does not spuriously suggest.
#[test]
fn a_plain_move_does_not_open_completion() {
    let st = completion_editor("prefix", 6, &["prefix", "press"]);
    completion::test_evaluate(&st, Trigger::Moved);
    assert!(
        st.borrow().completion.test_labels().is_empty(),
        "arrowing onto a word must not open the popup"
    );
}

// --- LogView streaming ----------------------------------------------------

mod log {
    use super::*;
    use crate::code_editor::log_stream::{self, LogStreamState};

    /// A fresh streaming state with a usable viewport, ready to pump.
    fn log_state() -> SharedState {
        let doc = TextDocument::new();
        let st = construct(
            doc,
            CODE_READ_ONLY_PRESET,
            CodeConfig::default(),
            WrapMode::None,
        );
        st.borrow_mut().log = Some(LogStreamState::new());
        // A viewport a handful of rows tall, so follow / window tests have a real
        // overflow to work against.
        st.borrow_mut()
            .sync_viewport(Rect::new(0.0, 0.0, 400.0, 100.0));
        st
    }

    fn enqueue(st: &SharedState, lines: &[&str]) {
        let s = st.borrow();
        let mut q = s.log.as_ref().unwrap().pending.lock().unwrap();
        for l in lines {
            q.push_back((*l).to_string());
        }
    }

    fn enqueue_owned(st: &SharedState, lines: impl IntoIterator<Item = String>) {
        let s = st.borrow();
        let mut q = s.log.as_ref().unwrap().pending.lock().unwrap();
        for l in lines {
            q.push_back(l);
        }
    }

    fn pump(st: &SharedState) {
        let mut s = st.borrow_mut();
        log_stream::tick(&mut s, 0.016);
    }

    #[test]
    fn appending_grows_the_line_count() {
        let st = log_state();
        enqueue(&st, &["one", "two", "three"]);
        pump(&st);
        assert_eq!(st.borrow().line_count.get(), 3);
    }

    /// The first append fills the document's initial empty block instead of
    /// adding after it, so a fresh log does not open with a blank first line.
    #[test]
    fn the_first_line_is_not_preceded_by_a_blank() {
        let st = log_state();
        enqueue(&st, &["first"]);
        pump(&st);
        assert_eq!(st.borrow().line_count.get(), 1, "no phantom blank line 0");
        let text = st
            .borrow()
            .document
            .snapshot_block_at_position(0)
            .unwrap()
            .text;
        assert_eq!(text, "first");
    }

    /// The log is read-only but still tracks selection for copy. A selection
    /// change inside the visible window moves the caret without moving the
    /// window, so the log's `a11y_version` does not fire — the caret signals'
    /// own `AccessibilityOnly` binding is what must re-walk the tree here.
    #[test]
    fn selecting_within_the_window_rewalks_the_accessibility_selection() {
        let st = log_state();
        enqueue(&st, &["alpha", "bravo", "charlie"]);
        pump(&st);

        let mut tree = WidgetTree::new();
        let _ = tree.add(crate::code_editor::log_view::log_body_for(&st));
        tree.layout(SizeProposal::exact(400.0, 100.0));

        let base = tree.sync_accessibility();
        assert_eq!(
            super::resolved_caret(&base, &st),
            Some(0),
            "baseline caret at 0"
        );

        // Select inside the first visible line ("alpha", chars 0..5).
        st.borrow().cursor.set_position(2, MoveMode::MoveAnchor);
        crate::code_editor::sync_cursor_signals(&st);
        tree.layout(SizeProposal::exact(400.0, 100.0));

        let after = tree.sync_accessibility();
        assert_eq!(
            super::resolved_caret(&after, &st),
            Some(2),
            "the log's AT selection must follow a within-window caret move",
        );
    }

    /// A streaming append must not force the O(n) full relayout the editor's
    /// path takes — `drain_events` sets the re-window flag instead.
    #[test]
    fn a_streaming_append_does_not_force_a_full_relayout() {
        let st = log_state();
        st.borrow_mut().needs_full_layout = false;
        enqueue(&st, &["a", "b"]);
        pump(&st);
        assert!(
            !st.borrow().needs_full_layout,
            "streaming must re-window, never force a full relayout"
        );
    }

    /// The content height spans every line, not just the shaped window — what
    /// keeps the scrollbar honest over an unshaped document.
    #[test]
    fn the_extent_spans_the_whole_document() {
        let st = log_state();
        enqueue_owned(&st, (0..1000).map(|i| format!("line {i}")));
        pump(&st);

        let s = st.borrow();
        let row_h = s.log.as_ref().unwrap().row_height;
        assert!(row_h > 0.0, "the row height must have been learned");
        assert!(
            (s.engine.content_height() - 1000.0 * row_h).abs() < 1.0,
            "content_height must span all 1000 rows"
        );
    }

    /// Following the tail sticks the view to the bottom as it grows.
    #[test]
    fn following_the_tail_sticks_to_the_bottom() {
        let st = log_state();
        enqueue_owned(&st, (0..200).map(|i| format!("line {i}")));
        pump(&st);

        let s = st.borrow();
        assert!(s.max_scroll_y.get() > 0.0, "200 lines must overflow");
        assert!(
            (s.scroll_y.get() - s.max_scroll_y.get()).abs() < 2.0,
            "a following view must be parked at the bottom"
        );
    }

    /// Regression: follow-tail must survive a scrollback eviction that lands in
    /// the same tick as new content. `was_at_bottom` is read before eviction
    /// shifts `scroll_y`, so the view re-locks to the (new) bottom rather than
    /// jumping to the top.
    #[test]
    fn following_survives_eviction() {
        let st = log_state();
        st.borrow_mut().log.as_mut().unwrap().scrollback_limit = Some(20);
        // Reach a steady, at-bottom following state.
        enqueue_owned(&st, (0..30).map(|i| format!("line {i}")));
        pump(&st);
        assert!(
            (st.borrow().scroll_y.get() - st.borrow().max_scroll_y.get()).abs() < 2.0,
            "precondition: following at the bottom"
        );
        // A burst large enough that growth and eviction fall in one tick.
        enqueue_owned(&st, (30..400).map(|i| format!("line {i}")));
        pump(&st);
        let s = st.borrow();
        assert!(
            (s.scroll_y.get() - s.max_scroll_y.get()).abs() < 2.0,
            "eviction+growth in one tick must not break follow: scroll_y={}, max={}",
            s.scroll_y.get(),
            s.max_scroll_y.get()
        );
    }

    /// Scrolling up pauses the follow: a later append does not yank the view back
    /// to the bottom.
    #[test]
    fn scrolling_up_pauses_the_follow() {
        let st = log_state();
        enqueue_owned(&st, (0..100).map(|i| format!("line {i}")));
        pump(&st);
        st.borrow().scroll_y.set(0.0);
        pump(&st);
        enqueue(&st, &["new one", "new two"]);
        pump(&st);
        assert!(
            st.borrow().scroll_y.get() < 5.0,
            "reading history must not be interrupted by new output"
        );
    }

    /// With following off, appends never move the view even at the bottom.
    #[test]
    fn follow_disabled_holds_position() {
        let st = log_state();
        st.borrow_mut().log.as_mut().unwrap().follow_enabled = false;
        enqueue_owned(&st, (0..200).map(|i| format!("line {i}")));
        pump(&st);
        assert!(
            st.borrow().scroll_y.get() < 1.0,
            "a non-following view holds at the top as it grows"
        );
    }

    /// Windowing lands on the right rows after a scroll — the O(log n) position
    /// chain (not the O(n) block walk) must still resolve the correct block.
    #[test]
    fn windowing_resolves_the_correct_rows_after_scroll() {
        let st = log_state();
        enqueue_owned(&st, (0..500).map(|i| format!("line {i}")));
        pump(&st);
        // Scroll to a known row and re-window.
        let row_h = st.borrow().log.as_ref().unwrap().row_height;
        st.borrow().scroll_y.set(250.0 * row_h);
        pump(&st);
        // The window anchor must point at the char position of row 250, whose
        // block is "line 250".
        let (arow, apos) = st.borrow().log.as_ref().unwrap().anchor.unwrap();
        assert_eq!(arow, 250, "anchor row must match the scroll");
        let text = st
            .borrow()
            .document
            .snapshot_block_at_position(apos)
            .unwrap()
            .text;
        assert_eq!(
            text, "line 250",
            "the anchor must resolve to the right block"
        );
    }

    /// The scrollback cap evicts from the front; the buffer stays bounded.
    #[test]
    fn scrollback_limit_bounds_the_buffer() {
        let st = log_state();
        st.borrow_mut().log.as_mut().unwrap().scrollback_limit = Some(50);
        enqueue_owned(&st, (0..1000).map(|i| format!("line {i}")));
        pump(&st);
        let count = st.borrow().line_count.get();
        assert!(
            (50..=50 + 256).contains(&count),
            "bounded near the cap, was {count}"
        );
    }

    /// Regression: a small cap is honoured tightly — the slack band scales down
    /// with the cap, so a limit of 10 does not hold 266.
    #[test]
    fn a_small_scrollback_limit_is_honoured_tightly() {
        let st = log_state();
        st.borrow_mut().log.as_mut().unwrap().scrollback_limit = Some(10);
        enqueue_owned(&st, (0..500).map(|i| format!("line {i}")));
        pump(&st);
        let count = st.borrow().line_count.get();
        // slack = (10/4).clamp(1,256) = 2, so the cap band is [10, 12].
        assert!(count <= 12, "a small cap must be tight, was {count}");
    }

    /// The severity classifier is consulted for the visible lines, with their
    /// text — the hook a colouring log needs.
    #[test]
    fn the_severity_classifier_sees_line_text() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let st = log_state();
        {
            let seen = seen.clone();
            st.borrow_mut().log.as_mut().unwrap().severity = Some(Rc::new(move |text: &str| {
                seen.borrow_mut().push(text.to_string());
                None
            }));
        }
        enqueue(&st, &["alpha", "beta", "gamma"]);
        pump(&st);
        assert!(
            seen.borrow().iter().any(|t| t == "alpha"),
            "the classifier must see each visible line's text, saw {:?}",
            seen.borrow()
        );
    }

    /// The handle splits on newlines and drops a single trailing terminator.
    #[test]
    fn the_handle_splits_on_newlines() {
        let st = log_state();
        let handle = crate::code_editor::LogViewHandle::from_state_for_test(st.clone());
        handle.append("a\nb\nc\n");
        pump(&st);
        assert_eq!(
            st.borrow().line_count.get(),
            3,
            "three lines, no blank from the trailing newline"
        );
    }

    /// Clearing empties the view and returns it to its pristine state, then
    /// accepts content again with no phantom blank.
    #[test]
    fn clearing_resets_the_view() {
        let st = log_state();
        enqueue(&st, &["a", "b", "c"]);
        pump(&st);
        assert_eq!(st.borrow().line_count.get(), 3);

        let handle = crate::code_editor::LogViewHandle::from_state_for_test(st.clone());
        handle.clear();
        pump(&st);
        assert_eq!(
            st.borrow().line_count.get(),
            0,
            "an emptied log has no lines"
        );
        assert!(st.borrow().log.as_ref().unwrap().pristine, "pristine again");
        assert_eq!(st.borrow().scroll_y.get(), 0.0);

        handle.append("after clear");
        pump(&st);
        assert_eq!(st.borrow().line_count.get(), 1);
        let text = st
            .borrow()
            .document
            .snapshot_block_at_position(0)
            .unwrap()
            .text;
        assert_eq!(text, "after clear", "refill must not leave a blank line 0");
    }

    /// The log's accessibility tree is windowed like its render: a 1000-line log
    /// emits only the visible lines as paragraphs, so an append re-walks the AT
    /// tree in O(window), not O(document).
    #[test]
    fn the_a11y_tree_is_windowed() {
        use bastyde_core::accesskit::Role;
        use bastyde_core::widget_id::WidgetId;

        let st = log_state();
        enqueue_owned(&st, (0..1000).map(|i| format!("line {i}")));
        pump(&st);

        let mut b = bastyde_core::accessibility::AccessNodeBuilder::for_widget(WidgetId::default());
        crate::code_editor::a11y::build_log_a11y(&st.borrow(), &mut b);
        let (_id, _n, children) = b.build(WidgetId::default());

        let paras: Vec<_> = children
            .iter()
            .filter(|(_, n)| n.role() == Role::Paragraph)
            .map(|(_, n)| n)
            .collect();
        assert!(!paras.is_empty(), "some visible lines are exposed");
        assert!(
            paras.len() < 100,
            "the tree is windowed, not all 1000 lines: got {}",
            paras.len()
        );
        assert_eq!(
            paras[0].size_of_set(),
            Some(1000),
            "a line is 'N of 1000', not 'N of window'"
        );
    }

    /// The a11y tree re-walk is driven by the visible window, not by raw scroll /
    /// append signals: a following append and a row-crossing scroll bump
    /// `a11y_version`, but a tail append while scrolled away and a sub-row pixel
    /// scroll do not — so a streaming log does not rebuild the whole app AT tree
    /// on every pixel or every off-window line.
    #[test]
    fn a11y_version_bumps_only_when_the_window_changes() {
        let st = log_state();
        enqueue_owned(&st, (0..200).map(|i| format!("line {i}")));
        pump(&st);
        let ver = st.borrow().log.as_ref().unwrap().a11y_version.clone();

        // Following the tail: an append advances the window -> bump.
        let v0 = ver.get();
        enqueue(&st, &["new"]);
        pump(&st);
        assert!(ver.get() > v0, "a following append must bump a11y_version");

        // Scroll up and settle (no longer following).
        st.borrow().scroll_y.set(0.0);
        pump(&st);
        let v1 = ver.get();
        // A tail append now leaves the visible window unchanged.
        enqueue(&st, &["another"]);
        pump(&st);
        assert_eq!(
            ver.get(),
            v1,
            "a tail append while scrolled away must not bump"
        );

        // A sub-row pixel scroll stays on the same first row.
        let row_h = st.borrow().log.as_ref().unwrap().row_height;
        let v2 = ver.get();
        st.borrow().scroll_y.set(row_h * 0.3);
        pump(&st);
        assert_eq!(ver.get(), v2, "a sub-row pixel scroll must not bump");

        // Crossing to a new first row changes the window.
        let v3 = ver.get();
        st.borrow().scroll_y.set(row_h * 5.0);
        pump(&st);
        assert!(
            ver.get() > v3,
            "crossing to a new row must bump a11y_version"
        );
    }
}
