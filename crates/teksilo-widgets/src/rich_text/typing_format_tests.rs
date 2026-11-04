// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Bold, Italic, Underline and Strikethrough toggled on a collapsed caret
//! format the text typed next at that caret, as in every word processor, and
//! the toggle reads On until the caret moves. See `typing_format`.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, TextDocument, TextFormat};

use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};
use crate::common::ime_ops_test::ImeOps;

fn viewport() -> SizeProposal {
    SizeProposal::exact(480.0, 300.0)
}

struct Editor {
    tree: WidgetTree,
    state: SharedState,
    handle: EditorHandle,
    doc: TextDocument,
    ops: ImeOps,
}

impl Editor {
    /// A focused editor holding `text`, its caret at `caret`, its undo
    /// history empty.
    fn new(text: &str, caret: usize) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        doc.clear_undo_redo();
        let editor = RichTextEditor::editor(doc.clone());
        let handle = editor.handle();
        let state = editor.state_handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(editor);
        tree.layout(viewport());
        let _ = tree.render();
        let focus = tree.first_focusable_descendant(id).expect("focusable");
        tree.focus(focus);
        let mut e = Self {
            tree,
            state,
            handle,
            doc,
            ops: ImeOps::default(),
        };
        e.settle();
        e.handle.select_range(caret, caret);
        e.settle();
        e
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(viewport());
        }
        let _ = self.tree.render();
    }

    fn type_keys(&mut self, text: &str) {
        let editor = self.tree.focused().expect("the editor is focused");
        self.tree.type_text(editor, text);
        self.settle();
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.settle();
    }

    fn compose(&mut self, text: &str) {
        self.tree.dispatch_event_with_ops(
            WidgetEvent::ImeComposition {
                text: text.to_string(),
                cursor: None,
                replacement: None,
            },
            &mut self.ops,
        );
    }

    fn commit(&mut self, text: &str) {
        self.compose("");
        self.tree
            .dispatch_event_with_ops(WidgetEvent::ime_commit(text), &mut self.ops);
        self.settle();
    }

    fn text(&self) -> String {
        self.doc.to_plain_text().unwrap_or_default()
    }

    /// The document's format of the character at `at`.
    fn format_at(&self, at: usize) -> TextFormat {
        let probe = self.doc.cursor();
        probe.set_position(at, MoveMode::MoveAnchor);
        probe.char_format().unwrap_or_default()
    }

    fn bold_at(&self, at: usize) -> bool {
        self.format_at(at).font_bold == Some(true)
    }

    fn undo(&mut self) {
        self.handle.undo();
        self.settle();
    }
}

/// Ctrl+B with nothing selected, then typing: the typed word is bold, the
/// text around it is not, and the toggle read On from the press on.
#[test]
fn ctrl_b_on_a_collapsed_caret_bolds_what_is_typed_next() {
    let mut e = Editor::new("one  two", 4);
    let version = e.handle.format_version().get();
    e.key(Key::B, Modifiers::COMMAND);
    assert!(e.handle.is_bold(), "the toggle reads On right after Ctrl+B");
    assert!(
        e.handle.format_version().get() != version,
        "a toolbar watching the format is told"
    );
    assert_eq!(e.text(), "one  two", "nothing is formatted or typed yet");

    e.type_keys("bold");
    assert_eq!(e.text(), "one bold two");
    assert!((4..8).all(|at| e.bold_at(at)), "the typed word is bold");
    assert!(
        !e.bold_at(3) && !e.bold_at(8),
        "the spaces around it are not"
    );
    assert!(
        e.handle.is_bold(),
        "after the commit, the next character is bold still"
    );

    e.undo();
    assert_eq!(e.text(), "one  two", "one undo takes the bold word out");
    assert!(!e.bold_at(4));
}

/// The toolbar path: the handle's toggles on a collapsed caret, each of the
/// four, read back On before anything is typed.
#[test]
fn every_toggle_on_a_collapsed_caret_formats_what_is_typed_next() {
    let mut e = Editor::new("ab", 1);
    e.handle.toggle_bold();
    e.handle.toggle_italic();
    e.handle.toggle_underline();
    e.handle.toggle_strikethrough();
    assert!(e.handle.is_bold());
    assert!(e.handle.is_italic());
    assert!(e.handle.is_underline());
    assert!(e.handle.is_strikethrough());

    e.type_keys("X");
    assert_eq!(e.text(), "aXb");
    let x = e.format_at(1);
    assert_eq!(
        (
            x.font_bold,
            x.font_italic,
            x.font_underline,
            x.font_strikeout
        ),
        (Some(true), Some(true), Some(true), Some(true)),
        "the typed character takes all four"
    );
    let a = e.format_at(0);
    assert_ne!(a.font_bold, Some(true), "the text before it does not");
}

/// Toggled twice, nothing is kept: the typed text is plain.
#[test]
fn toggling_twice_types_plain() {
    let mut e = Editor::new("ab", 1);
    e.key(Key::B, Modifiers::COMMAND);
    e.key(Key::B, Modifiers::COMMAND);
    assert!(!e.handle.is_bold());
    e.type_keys("X");
    assert!(!e.bold_at(1));
}

/// Moving the caret drops the toggle, even back to where it was.
#[test]
fn moving_the_caret_drops_the_toggle() {
    let mut e = Editor::new("abc", 1);
    e.key(Key::B, Modifiers::COMMAND);
    assert!(e.handle.is_bold());
    e.key(Key::ArrowRight, Modifiers::NONE);
    assert!(
        !e.handle.is_bold(),
        "the toggle reads Off once the caret moves"
    );
    e.key(Key::ArrowLeft, Modifiers::NONE);
    assert!(!e.handle.is_bold(), "and stays Off back where it was set");
    e.type_keys("X");
    assert_eq!(e.text(), "aXbc");
    assert!(!e.bold_at(1));
}

/// At the end of a bold word, typing continues bold, which the toggle shows;
/// Ctrl+B there turns it off for the text typed next.
#[test]
fn ctrl_b_after_bold_text_types_plain() {
    let mut e = Editor::new("bold text", 4);
    e.handle.select_range(0, 4);
    e.settle();
    e.key(Key::B, Modifiers::COMMAND);
    assert!(e.bold_at(0));
    e.handle.select_range(4, 4);
    e.settle();
    assert!(
        e.handle.is_bold(),
        "after a bold word, the next character is bold"
    );

    e.key(Key::B, Modifiers::COMMAND);
    assert!(!e.handle.is_bold(), "Ctrl+B turns it off");
    e.type_keys("X");
    assert_eq!(e.text(), "boldX text");
    assert!(!e.bold_at(4), "the typed character is plain");
}

/// An input method's commit takes the toggle, and one undo takes it out.
#[test]
fn an_ime_commit_takes_the_toggle() {
    let mut e = Editor::new("ab", 1);
    e.key(Key::B, Modifiers::COMMAND);
    for state in ["n", "ni"] {
        e.compose(state);
        e.settle();
        assert_eq!(e.text(), format!("a{state}b"));
        assert!(
            (1..1 + state.len()).all(|at| e.bold_at(at)),
            "the composition {state:?} is shown bold"
        );
        assert!(!e.bold_at(0), "the text before it is not");
    }
    assert!(e.handle.is_bold(), "the toggle reads On while composing");
    e.commit("\u{4F60}");
    assert_eq!(e.text(), "a\u{4F60}b");
    assert!(e.bold_at(1), "the committed text is bold");
    assert!(!e.bold_at(0) && !e.bold_at(2));

    e.undo();
    assert_eq!(e.text(), "ab", "one undo takes the committed text out");
}

/// A commit with no composition before it (a dead key, the emoji panel)
/// takes the toggle as typing does.
#[test]
fn a_commit_without_a_composition_takes_the_toggle() {
    let mut e = Editor::new("ab", 1);
    e.key(Key::B, Modifiers::COMMAND);
    e.tree
        .dispatch_event_with_ops(WidgetEvent::ime_commit("\u{E9}"), &mut e.ops);
    e.settle();
    assert_eq!(e.text(), "a\u{E9}b");
    assert!(e.bold_at(1));
}

/// Text typed in the same frame before Ctrl+B goes in without it.
#[test]
fn typing_before_the_toggle_in_the_same_frame_stays_plain() {
    let mut e = Editor::new("", 0);
    let editor = e.tree.focused().expect("focused");
    e.tree.type_text(editor, "ab");
    e.tree.press_key(Key::B, Modifiers::COMMAND);
    e.tree.type_text(editor, "cd");
    e.settle();
    assert_eq!(e.text(), "abcd");
    assert!(!e.bold_at(0) && !e.bold_at(1));
    assert!(e.bold_at(2) && e.bold_at(3));
}

/// At the start of a bold paragraph the next typed character is bold, as in a
/// word processor, and the toggle says so; Ctrl+B there types plain.
#[test]
fn at_the_start_of_a_bold_paragraph_typing_is_bold() {
    let mut e = Editor::new("plain\nBold", 6);
    e.handle.select_range(6, 10);
    e.settle();
    e.key(Key::B, Modifiers::COMMAND);
    e.handle.select_range(6, 6);
    e.settle();
    assert!(
        e.handle.is_bold(),
        "the paragraph's first character is bold"
    );
    e.type_keys("X");
    assert_eq!(e.text(), "plain\nXBold");
    assert!(e.bold_at(6), "typed in its format");

    e.handle.select_range(6, 6);
    e.settle();
    e.key(Key::B, Modifiers::COMMAND);
    assert!(!e.handle.is_bold());
    e.type_keys("Y");
    assert_eq!(e.text(), "plain\nYXBold");
    assert!(!e.bold_at(6), "Ctrl+B there types plain");
    assert!(e.bold_at(7));
}

/// In an empty paragraph the next typed character is plain: nothing to take
/// a format from.
#[test]
fn in_an_empty_paragraph_typing_is_plain() {
    let mut e = Editor::new("Bold\n\nafter", 5);
    e.handle.select_range(0, 4);
    e.settle();
    e.key(Key::B, Modifiers::COMMAND);
    e.handle.select_range(5, 5);
    e.settle();
    assert!(!e.handle.is_bold());
    e.type_keys("X");
    assert!(!e.bold_at(5));
}

// ── Across a paragraph break ─────────────────────────────────────────────────

impl Editor {
    /// Make `[start, end)` bold, then put the caret at `caret`.
    fn bold_range(&mut self, start: usize, end: usize, caret: usize) {
        self.handle.select_range(start, end);
        self.settle();
        self.key(Key::B, Modifiers::COMMAND);
        self.handle.select_range(caret, caret);
        self.settle();
    }

    fn block_count(&self) -> usize {
        self.text().split('\n').count()
    }
}

/// Bold typing, Enter, more typing: it goes on bold, as in Word and TextEdit.
#[test]
fn bold_typing_goes_on_bold_after_enter() {
    let mut e = Editor::new("", 0);
    e.key(Key::B, Modifiers::COMMAND);
    e.type_keys("ab");
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.block_count(), 2);
    assert!(
        e.handle.is_bold(),
        "the toggle reads On in the new paragraph"
    );
    e.type_keys("cd");
    assert_eq!(e.text(), "ab\ncd");
    assert!((3..5).all(|at| e.bold_at(at)), "the new paragraph is bold");
}

/// Enter at the end of bold text carries its format, toggled or not.
#[test]
fn enter_after_bold_text_types_bold() {
    let mut e = Editor::new("ab", 2);
    e.bold_range(0, 2, 2);
    e.key(Key::Enter, Modifiers::NONE);
    e.type_keys("X");
    assert_eq!(e.text(), "ab\nX");
    assert!(e.bold_at(3));

    // An Italic toggled after the break adds to what was carried.
    e.key(Key::Enter, Modifiers::NONE);
    e.key(Key::I, Modifiers::COMMAND);
    e.type_keys("Y");
    let y = e.format_at(5);
    assert_eq!((y.font_bold, y.font_italic), (Some(true), Some(true)));
}

/// Ctrl+Enter, the forced paragraph, carries it too.
#[test]
fn the_forced_paragraph_carries_the_format() {
    let mut e = Editor::new("ab", 2);
    e.bold_range(0, 2, 2);
    e.key(Key::Enter, Modifiers::COMMAND);
    assert_eq!(e.block_count(), 2);
    e.type_keys("X");
    assert!(e.bold_at(3));
}

/// Any other move of the caret drops what was carried.
#[test]
fn moving_after_enter_drops_the_carried_format() {
    let mut e = Editor::new("ab", 2);
    e.bold_range(0, 2, 2);
    e.key(Key::Enter, Modifiers::NONE);
    e.key(Key::ArrowLeft, Modifiers::NONE);
    e.key(Key::ArrowRight, Modifiers::NONE);
    assert!(!e.handle.is_bold(), "an empty paragraph reads plain again");
    e.type_keys("X");
    assert!(!e.bold_at(3));
}

/// Enter in the middle of a bold run: the new paragraph starts with bold
/// text, whose format the text typed before it takes; nothing is carried.
#[test]
fn enter_in_a_bold_run_types_in_the_runs_format() {
    let mut e = Editor::new("abcd", 2);
    e.bold_range(0, 4, 2);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.text(), "ab\ncd");
    assert!(e.state.borrow().typing_format.is_none(), "nothing carried");
    assert!(e.handle.is_bold());
    e.type_keys("X");
    assert_eq!(e.text(), "ab\nXcd");
    assert!(e.bold_at(3));
}

/// Enter at the end of a plain paragraph carries nothing.
#[test]
fn enter_after_plain_text_types_plain() {
    let mut e = Editor::new("ab", 2);
    e.key(Key::Enter, Modifiers::NONE);
    assert!(e.state.borrow().typing_format.is_none());
    e.type_keys("X");
    assert!(!e.bold_at(3));
}

/// A link is not carried: the next paragraph is not part of it.
#[test]
fn a_link_is_not_carried() {
    let mut e = Editor::new("ab", 2);
    e.handle.select_range(0, 2);
    e.settle();
    e.handle.set_link("https://example.org");
    e.key(Key::B, Modifiers::COMMAND);
    e.handle.select_range(2, 2);
    e.settle();
    e.key(Key::Enter, Modifiers::NONE);
    e.type_keys("X");
    let x = e.format_at(3);
    assert_eq!(x.font_bold, Some(true), "the bold is carried");
    assert_eq!(x.anchor_href, None, "the link is not");
}

/// Enter at the end of a bold heading: the new paragraph is a body paragraph
/// (LibreOffice Writer's "Text Body"), and the bold goes on.
#[test]
fn enter_after_a_bold_heading_keeps_the_bold() {
    let mut e = Editor::new("Title", 5);
    e.handle.set_heading_level(1);
    e.bold_range(0, 5, 5);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.handle.get_heading_level(), 0, "a body paragraph");
    e.type_keys("X");
    assert_eq!(e.text(), "Title\nX");
    assert!(e.bold_at(6), "the bold goes on");
    assert_eq!(e.heading_at(0), 1, "the heading stays one");
}

/// In a list, Enter makes the next item, which goes on bold.
#[test]
fn a_new_list_item_goes_on_bold() {
    let mut e = Editor::new("item", 4);
    e.handle.toggle_list(false);
    e.bold_range(0, 4, 4);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.block_count(), 2);
    e.type_keys("X");
    assert!(e.bold_at(5));
}

// ── Enter in a heading, as LibreOffice Writer does it ─────────────────────────

impl Editor {
    /// The heading level of the paragraph holding `at`.
    fn heading_at(&self, at: usize) -> u8 {
        self.doc
            .cursor_at(at)
            .block_format()
            .unwrap_or_default()
            .heading_level
            .unwrap_or(0)
    }

    /// An editor holding `text` as a level-2 heading, centred, caret at `caret`.
    fn heading(text: &str, caret: usize) -> Self {
        let mut e = Self::new(text, caret);
        e.handle.set_heading_level(2);
        e.handle
            .set_alignment(teksilo_text::text_document::Alignment::Center);
        e.doc.clear_undo_redo();
        e.settle();
        e
    }
}

/// At the end of a heading, Enter starts a body paragraph; its other
/// paragraph formats (the alignment here) go on, as in Writer. One undo takes
/// the break and the level change back together.
#[test]
fn enter_at_the_end_of_a_heading_starts_a_body_paragraph() {
    let mut e = Editor::heading("Title", 5);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.text(), "Title\n");
    assert_eq!(e.heading_at(0), 2, "the heading stays a heading");
    assert_eq!(e.heading_at(6), 0, "the new paragraph is body text");
    assert_eq!(
        e.doc.cursor_at(6).block_format().unwrap().alignment,
        Some(teksilo_text::text_document::Alignment::Center),
        "its alignment goes on"
    );
    e.type_keys("body");
    assert_eq!(e.heading_at(6), 0);

    e.undo();
    e.undo();
    assert_eq!(
        e.text(),
        "Title",
        "the typing, then the break, each one undo"
    );
    assert_eq!(e.heading_at(0), 2);
    assert!(
        !e.doc.can_undo(),
        "the break and the level change were one step"
    );
}

/// Enter on an empty heading is at its end too.
#[test]
fn enter_on_an_empty_heading_starts_a_body_paragraph() {
    let mut e = Editor::heading("", 0);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.heading_at(0), 2);
    assert_eq!(e.heading_at(1), 0);
}

/// In the middle of a heading, Enter splits it into two headings.
#[test]
fn enter_in_the_middle_of_a_heading_splits_it_into_two() {
    let mut e = Editor::heading("Title", 2);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.text(), "Ti\ntle");
    assert_eq!((e.heading_at(0), e.heading_at(3)), (2, 2));
    e.undo();
    assert_eq!(e.text(), "Title");
    assert!(!e.doc.can_undo());
}

/// At the start of a non-empty heading, Enter puts an empty heading above it,
/// and the text stays a heading.
#[test]
fn enter_at_the_start_of_a_heading_puts_an_empty_heading_above() {
    let mut e = Editor::heading("Title", 0);
    e.key(Key::Enter, Modifiers::NONE);
    assert_eq!(e.text(), "\nTitle");
    assert_eq!((e.heading_at(0), e.heading_at(1)), (2, 2));
    assert_eq!(
        e.state.borrow().cursor.position(),
        1,
        "the caret stays on the text"
    );
    e.undo();
    assert_eq!(e.text(), "Title");
    assert_eq!(e.heading_at(0), 2);
}

/// Ctrl+Enter, the forced paragraph, at the end of a heading: body text too.
#[test]
fn the_forced_paragraph_after_a_heading_is_body_text() {
    let mut e = Editor::heading("Title", 5);
    e.key(Key::Enter, Modifiers::COMMAND);
    assert_eq!(e.heading_at(6), 0);
}

/// A table at the very start of the document puts a cell's caret at position
/// 0 without a block start there: the format probe must not step before it.
/// Ctrl+B, Enter and typing there all ask for the caret's format.
#[test]
fn a_caret_at_the_start_of_a_leading_table_has_a_format() {
    let mut ed = Editor::new("after", 0);
    ed.doc.cursor_at(0).insert_table(1, 1).unwrap();
    ed.settle();
    ed.handle.select_range(0, 0);
    ed.settle();

    ed.key(Key::B, Modifiers::COMMAND);
    ed.type_keys("x");
    ed.key(Key::Enter, Modifiers::NONE);
    assert!(
        ed.text().contains('x'),
        "typing at the table's start landed"
    );
}
