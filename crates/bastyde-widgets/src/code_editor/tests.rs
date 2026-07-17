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
use bastyde_text::text_document::TextDocument;

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
