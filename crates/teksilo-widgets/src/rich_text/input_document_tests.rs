// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The editor's document as the platform's text input system reads it: the
//! selection, the text, the composition, where a range is drawn, the offset
//! under a point, and the UTF-16 offsets the platform counts in.
//!
//! Geometry is checked against the editor's own caret rectangles
//! ([`EditorHandle::offset_rect`]), never against fixed pixel values: the
//! private engine shapes with real font metrics.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{ImeOrigin, WidgetEvent};
use teksilo_core::text_input_document::TextInputDocument;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, TextDocument};

use super::{EditorHandle, RichTextEditor};
use crate::EditSource;

const HEIGHT: f32 = 300.0;

struct Editor {
    tree: WidgetTree,
    id: WidgetId,
    handle: EditorHandle,
    doc: TextDocument,
    width: f32,
    arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
}

impl Editor {
    /// A focused, laid-out editor on `doc`, `width` wide.
    fn mount(
        doc: TextDocument,
        width: f32,
        make: impl FnOnce(RichTextEditor) -> RichTextEditor,
    ) -> Self {
        let arrivals = Rc::new(RefCell::new(Vec::new()));
        let seen = arrivals.clone();
        let editor = make(RichTextEditor::editor(doc.clone()))
            .on_text_inserted(move |source, chars| seen.borrow_mut().push((source, chars)));
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(width, HEIGHT));
        tree.focus(id);
        let mut editor = Self {
            tree,
            id,
            handle,
            doc,
            width,
            arrivals,
        };
        editor.settle();
        editor
    }

    fn with_text(text: &str, width: f32) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::mount(doc, width, |editor| editor)
    }

    fn frame(&mut self) {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(16));
        self.tree.layout(SizeProposal::exact(self.width, HEIGHT));
    }

    /// Run the frame loop until the typing is in, the events drained and the
    /// text laid out and painted.
    fn settle(&mut self) {
        for _ in 0..3 {
            self.frame();
        }
        let _ = self.tree.render();
    }

    /// The editor as the input system sees it. Through the trait object on
    /// purpose: `EditorHandle` has inherent methods of the same names
    /// (`selection`, `offset_at_point`) that answer other questions.
    fn doc(&self) -> &dyn TextInputDocument {
        &self.handle
    }

    fn caret_at(&self, offset: usize) -> Rect {
        self.handle
            .offset_rect(offset)
            .expect("a laid-out editor has caret geometry")
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

// ── Reached through the window's registry ───────────────────────────────────

#[test]
fn the_focused_editor_answers_through_the_window_registry() {
    let editor = Editor::with_text("Hello world", 400.0);
    // Anchor after "w", caret after "He": the selection reads lowest first.
    editor.handle.select_range(7, 2);

    let answers = editor
        .tree
        .text_surfaces()
        .with_focused_input_document(|d| {
            (
                d.char_count(),
                d.selection(),
                d.caret(),
                d.text_in(2..7),
                d.is_editable(),
                d.is_secure(),
                d.writing_tools_allowed(),
            )
        })
        .expect("the focused editor describes its document");
    assert_eq!(
        answers,
        (
            Some(11),
            Some(2..7),
            Some(2),
            Some("llo w".to_string()),
            true,
            false,
            true
        )
    );
}

// ── The text ────────────────────────────────────────────────────────────────

#[test]
#[expect(
    clippy::reversed_empty_ranges,
    reason = "a reversed range is one of the inputs under test"
)]
fn text_is_read_in_the_documents_offsets_and_clamped() {
    let editor = Editor::with_text("ab\ncd", 400.0);
    let d = editor.doc();
    assert_eq!(
        d.char_count(),
        Some(5),
        "a paragraph break is one character"
    );
    assert_eq!(d.text_in(1..4).as_deref(), Some("b\nc"));
    assert_eq!(
        d.text_in(3..99).as_deref(),
        Some("cd"),
        "clamped at the end"
    );
    assert_eq!(d.text_in(99..120).as_deref(), Some(""));
    assert_eq!(
        d.text_in(4..2).as_deref(),
        Some(""),
        "a reversed range is empty"
    );
}

#[test]
fn a_line_break_inside_a_paragraph_is_one_character() {
    let editor = Editor::with_text("abcd", 400.0);
    editor.handle.select_range(2, 2);
    assert!(editor.handle.insert_line_break());
    let d = editor.doc();
    assert_eq!(d.char_count(), Some(5));
    assert_eq!(d.text_in(0..5).as_deref(), Some("ab\u{2028}cd"));
    assert_eq!(d.char_to_utf16(5), Some(5), "U+2028 is one UTF-16 unit");
    assert_eq!(d.caret(), Some(3), "after the break");
}

/// A table occupies its `U+FFFC` anchor (and a separator) in the document's
/// offsets, which `to_plain_text` leaves out. Every answer here is in the
/// document's offsets, so an emoji after a table is found where the caret
/// would find it.
#[test]
fn a_table_anchor_counts_in_the_text_and_the_utf16_index() {
    let doc = TextDocument::new();
    doc.set_plain_text("Intro\nx\u{1F600}y").unwrap();
    {
        let cursor = doc.cursor_at(0);
        cursor.set_position(5, MoveMode::MoveAnchor);
        cursor.insert_table(1, 1).unwrap();
    }
    let editor = Editor::mount(doc.clone(), 400.0, |editor| editor);
    let d = editor.doc();

    let addressable = doc.to_addressable_text().unwrap();
    assert!(
        addressable.contains('\u{FFFC}'),
        "premise: the anchor is there"
    );
    let count = addressable.chars().count();
    assert_eq!(d.char_count(), Some(count));
    assert_eq!(d.text_in(0..count).as_deref(), Some(addressable.as_str()));

    let emoji = addressable
        .chars()
        .position(|c| c == '\u{1F600}')
        .expect("the emoji");
    let plain = doc.to_plain_text().unwrap();
    assert_ne!(
        plain.chars().position(|c| c == '\u{1F600}'),
        Some(emoji),
        "premise: the plain-text export would have put it elsewhere"
    );
    assert_eq!(d.text_in(emoji..emoji + 1).as_deref(), Some("\u{1F600}"));
    assert_eq!(
        d.char_to_utf16(emoji),
        Some(emoji),
        "everything before is BMP"
    );
    assert_eq!(d.char_to_utf16(emoji + 1), Some(emoji + 2));
    assert_eq!(d.utf16_to_char(emoji + 2), Some(emoji + 1));
    assert_eq!(
        d.utf16_range_to_chars(emoji..emoji + 2),
        Some(emoji..emoji + 1)
    );

    // Every offset, against the standard library's own encoding.
    let index = d.utf16_index().expect("an index");
    assert_eq!(index.utf16_len(), addressable.encode_utf16().count());
    for c in 0..=count {
        let expected: usize = addressable.chars().take(c).map(char::len_utf16).sum();
        assert_eq!(index.char_to_utf16(c), expected, "char {c}");
        assert_eq!(index.utf16_to_char(expected), c, "back from {expected}");
    }
}

#[test]
fn the_utf16_index_is_kept_until_an_edit_and_rebuilt_after_it() {
    let mut editor = Editor::with_text("a\u{1F600}", 400.0);
    let first = editor.doc().utf16_index().expect("an index");
    assert!(
        Rc::ptr_eq(&first, &editor.doc().utf16_index().expect("an index")),
        "asked twice with no edit between, the same index"
    );
    assert_eq!(editor.doc().char_to_utf16(2), Some(3));

    editor.handle.select_range(2, 2);
    editor.tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "\u{1F600}".to_string(),
        origin: ImeOrigin::Insertion,
        replacement: None,
    });
    editor.settle();

    let after = editor.doc().utf16_index().expect("an index");
    assert!(!Rc::ptr_eq(&first, &after), "an edit rebuilds it");
    assert_eq!(after.char_count(), 3);
    assert_eq!(after.utf16_len(), 5);
    assert_eq!(editor.doc().char_to_utf16(3), Some(5));
    assert_eq!(
        editor.doc().utf16_to_char(4),
        Some(2),
        "inside the new pair"
    );
}

// ── Typing not yet inserted ─────────────────────────────────────────────────

#[test]
fn typing_not_yet_inserted_is_answered_as_inserted() {
    let mut editor = Editor::with_text("ab", 400.0);
    editor.handle.select_range(2, 2);
    editor.tree.type_text(editor.id, "cd");
    assert_eq!(
        editor.handle.state.borrow().pending_chars,
        "cd",
        "premise: the keys wait for the frame tick"
    );

    assert_eq!(editor.doc().caret(), Some(4), "after the typing");
    assert_eq!(editor.doc().text_in(0..4).as_deref(), Some("abcd"));
    assert!(editor.handle.state.borrow().pending_chars.is_empty());

    editor.settle();
    assert_eq!(
        editor.doc.to_plain_text().unwrap(),
        "abcd",
        "the tick does not insert the typing a second time"
    );
    assert_eq!(
        *editor.arrivals.borrow(),
        vec![(EditSource::Keyboard, 2)],
        "reported once, as typed"
    );
}

/// The accent popup's sequence: the letter is typed, the input method asks
/// where the selection is, then replaces the letter by naming its range. The
/// range the answer implies must be the letter.
#[test]
fn a_replacement_aimed_by_the_answer_replaces_the_typed_letter() {
    let mut editor = Editor::with_text("caf", 400.0);
    editor.handle.select_range(3, 3);
    editor.tree.type_text(editor.id, "e");

    let caret = editor.doc().caret().expect("an answer");
    editor.tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "\u{E9}".to_string(),
        origin: ImeOrigin::Insertion,
        replacement: Some(caret - 1..caret),
    });
    editor.settle();
    assert_eq!(editor.doc.to_plain_text().unwrap(), "caf\u{E9}");
}

/// Writing Tools replaces the range it read from these answers. One that runs
/// to the end of a later paragraph is replaced whole, not cut short by the
/// paragraph breaks before it.
#[test]
fn a_replacement_running_to_the_end_of_a_later_paragraph_is_replaced_whole() {
    let mut editor = Editor::with_text("one\ntwo", 400.0);
    let end = editor.doc().char_count().expect("an answer");
    assert_eq!(end, 7);
    assert_eq!(editor.doc().text_in(4..end).as_deref(), Some("two"));

    editor.tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "2".to_string(),
        origin: ImeOrigin::WritingTools,
        replacement: Some(4..end),
    });
    editor.settle();
    assert_eq!(editor.doc.to_plain_text().unwrap(), "one\n2");
}

// ── The composition ─────────────────────────────────────────────────────────

#[test]
fn a_composition_in_progress_is_its_range() {
    let mut editor = Editor::with_text("ab", 400.0);
    editor.handle.select_range(1, 1);
    assert_eq!(editor.doc().composition_range(), None);

    editor.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: None,
    });
    assert_eq!(editor.doc().composition_range(), Some(1..3));
    assert_eq!(editor.doc().text_in(1..3).as_deref(), Some("ni"));

    editor
        .tree
        .dispatch_event(WidgetEvent::ime_commit("\u{4F60}"));
    editor.settle();
    assert_eq!(editor.doc().composition_range(), None);
}

/// A composition at the end of a later paragraph lies past
/// `character_count`, which leaves the paragraph breaks out. Each new
/// candidate, and the commit, must still take the whole preedit away.
#[test]
fn a_composition_at_the_end_of_a_later_paragraph_is_replaced_whole() {
    let mut editor = Editor::with_text("one\ntwo", 400.0);
    editor.handle.select_range(7, 7);
    editor.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: None,
    });
    assert_eq!(editor.doc().composition_range(), Some(7..9));
    editor.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "nih".to_string(),
        cursor: None,
    });
    assert_eq!(editor.doc().composition_range(), Some(7..10));
    assert_eq!(editor.doc().text_in(4..10).as_deref(), Some("twonih"));

    editor
        .tree
        .dispatch_event(WidgetEvent::ime_commit("\u{4F60}"));
    editor.settle();
    assert_eq!(editor.doc.to_plain_text().unwrap(), "one\ntwo\u{4F60}");
}

/// Each candidate tells the input method where the composition is: at the
/// caret after the preedit, with the preedit laid out. Read as the event is
/// handled, before any frame runs, because that is when the platform places
/// its candidate window.
///
/// The preedit went into the document, but the text was laid out again only
/// at the next frame, so the area was read off the text as it stood without
/// the preedit. At the end of a paragraph the preedit's end lies past that
/// paragraph there, and the area landed in the next paragraph near its start:
/// Microsoft Pinyin, which lists candidates from the first key, opened them
/// at the editor's left edge.
#[test]
fn each_candidate_reports_the_caret_after_the_preedit() {
    let mut editor = Editor::with_text("one\ntwo three four", 400.0);
    editor.handle.select_range(3, 3);
    for preedit in ["n", "ni", "nih", "niha", "nihao"] {
        // Each key of the syllable is a new candidate: measure its own report.
        editor.handle.state.borrow_mut().last_ime_area = None;
        editor.tree.dispatch_event(WidgetEvent::ImeComposition {
            text: preedit.to_string(),
            cursor: None,
        });
        let reported = editor
            .handle
            .state
            .borrow()
            .last_ime_area
            .expect("a composition reports where it is");
        editor.settle();
        let caret = editor.caret_at(3 + preedit.chars().count());
        assert!(
            close(reported.x, caret.x) && close(reported.y, caret.y),
            "{preedit:?}: reported {reported:?}, the caret after it is at {caret:?}"
        );
    }
    assert_eq!(editor.doc().composition_range(), Some(3..8));
}

// ── What the input system may do ────────────────────────────────────────────

#[test]
fn a_read_only_editor_is_not_editable() {
    let doc = TextDocument::new();
    doc.set_plain_text("read me").unwrap();
    let editor = RichTextEditor::read_only(doc);
    let handle = editor.handle();
    let mut tree = WidgetTree::new();
    tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, HEIGHT));
    let d: &dyn TextInputDocument = &handle;
    assert!(!d.is_editable());
    assert_eq!(d.text_in(0..4).as_deref(), Some("read"), "but readable");
}

#[test]
fn writing_tools_follow_the_editors_setting() {
    let off = Editor::mount(TextDocument::new(), 400.0, |e| e.writing_tools(false));
    assert!(!off.doc().writing_tools_allowed());
    assert!(off.doc().is_editable());
    assert!(!off.doc().is_secure());
}

// ── Geometry ────────────────────────────────────────────────────────────────

#[test]
fn a_range_on_one_line_is_its_own_rect() {
    let editor = Editor::with_text("Hello world", 400.0);
    let line = editor
        .doc()
        .first_line_rect(0..5)
        .expect("a laid-out editor");
    assert_eq!(line.covered, 0..5);
    let expected = editor
        .handle
        .range_rect(0, 5)
        .expect("the union of its carets");
    assert!(close(line.rect.x, expected.x), "{line:?} vs {expected:?}");
    assert!(close(line.rect.y, expected.y), "{line:?} vs {expected:?}");
    assert!(
        close(line.rect.width, expected.width),
        "{line:?} vs {expected:?}"
    );
    assert!(
        close(line.rect.height, expected.height),
        "{line:?} vs {expected:?}"
    );
    assert!(line.rect.width > 1.0, "five characters have width");
}

#[test]
fn an_empty_range_is_the_caret_rect() {
    let editor = Editor::with_text("Hello world", 400.0);
    let line = editor.doc().first_line_rect(3..3).expect("laid out");
    assert_eq!(line.covered, 3..3);
    let caret = editor.caret_at(3);
    assert!(close(line.rect.x, caret.x) && close(line.rect.y, caret.y));
    assert!(close(line.rect.height, caret.height));
    assert!(line.rect.width >= 1.0, "a caret has a width");
}

/// A range that wraps answers for its first line only, and says where that
/// line ends, so the caller can ask again from there.
#[test]
fn a_wrapped_range_answers_for_its_first_line() {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj";
    let len = text.chars().count();
    let editor = Editor::with_text(text, 150.0);

    // The first line ends at the first offset whose caret (drawn at the END
    // of a line at a wrap) is followed by one on a lower line.
    let top = editor.caret_at(0).y;
    let wrap = (1..len)
        .find(|&n| editor.caret_at(n + 1).y > top + 0.5)
        .expect("premise: the text wraps at this width");

    let line = editor.doc().first_line_rect(0..len).expect("laid out");
    assert_eq!(line.covered, 0..wrap, "up to the end of the first line");
    assert!(close(line.rect.y, top));
    assert!(close(line.rect.height, editor.caret_at(0).height));
    assert!(
        line.rect.bottom() <= editor.caret_at(wrap + 1).y + 0.5,
        "one line tall: {line:?}"
    );
    assert!(close(line.rect.x, editor.caret_at(0).x));
    assert!(
        close(line.rect.right(), editor.caret_at(wrap).x),
        "to the end of the line: {line:?}"
    );

    // Asked again from there, the next line.
    let next = editor.doc().first_line_rect(wrap..len).expect("laid out");
    assert_eq!(next.covered.start, wrap);
    assert!(next.covered.end > wrap, "a walk always advances");
    assert!(
        close(next.rect.y, editor.caret_at(wrap + 1).y),
        "the second line: {next:?}"
    );
}

#[test]
fn a_range_in_the_second_paragraph_is_drawn_there() {
    let editor = Editor::with_text("First paragraph\nSecond one", 400.0);
    let second = editor.doc().first_line_rect(16..22).expect("laid out");
    assert_eq!(second.covered, 16..22);
    let expected = editor.handle.range_rect(16, 22).expect("laid out");
    assert!(
        close(second.rect.y, expected.y),
        "{second:?} vs {expected:?}"
    );
    assert!(
        close(second.rect.x, expected.x),
        "{second:?} vs {expected:?}"
    );
    assert!(
        close(second.rect.width, expected.width),
        "{second:?} vs {expected:?}"
    );
    let first = editor.doc().first_line_rect(0..5).expect("laid out");
    assert!(second.rect.y > first.rect.y, "below the first paragraph");

    // A range starting on the paragraph break covers that one character, at
    // the end of the first paragraph, so a walk moves on.
    let at_break = editor.doc().first_line_rect(15..20).expect("laid out");
    assert_eq!(at_break.covered, 15..16);
    assert!(close(at_break.rect.y, editor.caret_at(15).y));
}

#[test]
fn no_geometry_before_the_first_layout() {
    let doc = TextDocument::new();
    doc.set_plain_text("hello").unwrap();
    let handle = RichTextEditor::editor(doc).handle();
    let d: &dyn TextInputDocument = &handle;
    assert_eq!(d.first_line_rect(0..2), None);
    assert_eq!(d.offset_at_point(Point::new(5.0, 5.0)), None);
    assert_eq!(d.visible_text_rect(), None);
}

#[test]
fn the_offset_at_a_point_inside_beside_and_outside_the_text() {
    let editor = Editor::with_text("Hello world", 400.0);
    let visible = editor.doc().visible_text_rect().expect("laid out");
    let caret = editor.caret_at(3);
    let mid_line = caret.y + caret.height / 2.0;

    assert_eq!(
        editor
            .doc()
            .offset_at_point(Point::new(caret.x + 1.0, mid_line)),
        Some(3),
        "just after the caret at 3"
    );
    assert_eq!(
        editor
            .doc()
            .offset_at_point(Point::new(visible.x + 0.5, mid_line)),
        Some(0),
        "at the left edge, before the text"
    );
    assert_eq!(
        editor
            .doc()
            .offset_at_point(Point::new(visible.right() - 2.0, mid_line)),
        Some(11),
        "past the end of the line"
    );
    assert_eq!(
        editor
            .doc()
            .offset_at_point(Point::new(caret.x, visible.bottom() - 2.0)),
        Some(11),
        "below the last line"
    );
    assert_eq!(
        editor
            .doc()
            .offset_at_point(Point::new(visible.right() + 10.0, mid_line)),
        None,
        "outside the editor"
    );
}

#[test]
fn the_visible_rect_holds_the_text() {
    let editor = Editor::with_text("Hello", 400.0);
    let visible = editor.doc().visible_text_rect().expect("laid out");
    assert!(visible.width > 0.0 && visible.height > 0.0);
    let caret = editor.caret_at(0);
    assert!(visible.contains(Point::new(caret.x + 0.5, caret.y + caret.height / 2.0)));
}

// ── Busy ────────────────────────────────────────────────────────────────────

/// The platform asks whenever it likes, including while the editor is in the
/// middle of its own work: the answer is "not now", never a panic.
#[test]
fn a_busy_editor_answers_nothing_instead_of_panicking() {
    let mut editor = Editor::with_text("Hello", 400.0);
    editor.handle.select_range(5, 5);
    {
        let _held = editor.handle.state.borrow_mut();
        let d = editor.doc();
        assert_eq!(d.char_count(), None);
        assert_eq!(d.text_in(0..2), None);
        assert_eq!(d.selection(), None);
        assert_eq!(d.caret(), None);
        assert_eq!(d.first_line_rect(0..2), None);
        assert_eq!(d.offset_at_point(Point::new(5.0, 5.0)), None);
        assert_eq!(d.utf16_index(), None);
        assert_eq!(d.char_to_utf16(1), None);
        assert!(!d.is_editable());
        assert!(!d.writing_tools_allowed());
    }

    // A shared borrow is enough while there is no typing to insert...
    {
        let _reading = editor.handle.state.borrow();
        assert_eq!(editor.doc().caret(), Some(5));
    }
    // ...and not when there is.
    editor.tree.type_text(editor.id, "x");
    {
        let _reading = editor.handle.state.borrow();
        assert_eq!(editor.doc().caret(), None);
    }
    assert_eq!(editor.doc().caret(), Some(6));
}
