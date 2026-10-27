// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A line break inside a block: Shift+Enter, `insert_line_break`, and what
//! the caret keys, the clipboard and accessibility make of it.
//!
//! The document holds the break as one `LINE_SEPARATOR` (U+2028) in the
//! block's text, so the paragraph stays one paragraph.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::{LINE_SEPARATOR, TextDocument};

use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};

const LS: char = LINE_SEPARATOR;

fn layout(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(200));
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

fn press(tree: &mut WidgetTree, key: Key, modifiers: Modifiers) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key,
        modifiers,
        text: None,
    });
}

fn type_text(tree: &mut WidgetTree, text: &str) {
    for ch in text.chars() {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::A,
            modifiers: Modifiers::NONE,
            text: Some(ch.to_string()),
        });
    }
    layout(tree);
}

fn focus(tree: &mut WidgetTree, id: WidgetId) {
    let _ = tree.render();
    tree.dispatch_event(WidgetEvent::pointer_down(
        teksilo_canvas::Point::new(20.0, 20.0),
        teksilo_core::event::PointerButton::Primary,
        Modifiers::NONE,
    ));
    let focused = tree.focused();
    assert!(
        focused.is_some_and(|f| f == id || tree.is_descendant_of(f, id)),
        "the click did not focus the editor"
    );
}

struct Mounted {
    doc: TextDocument,
    handle: EditorHandle,
    state: SharedState,
    tree: WidgetTree,
    id: WidgetId,
    clipboard: teksilo_platform::clipboard::ClipboardHandle,
}

impl Mounted {
    /// An editor over `text`, laid out, focused, with the caret at `caret`.
    fn new(text: &str, caret: usize) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::over(doc, RichTextEditor::editor, caret)
    }

    fn over(doc: TextDocument, make: fn(TextDocument) -> RichTextEditor, caret: usize) -> Self {
        let editor = make(doc.clone());
        Self::with_editor(doc, editor, caret)
    }

    fn with_editor(doc: TextDocument, editor: RichTextEditor, caret: usize) -> Self {
        let handle = editor.handle();
        let state = editor.state_handle();
        let mut tree = WidgetTree::new();
        let clipboard = super::tests::ctx_with_memory_clipboard(&mut tree);
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        focus(&mut tree, id);
        handle.select_range(caret, caret);
        Self {
            doc,
            handle,
            state,
            tree,
            id,
            clipboard,
        }
    }

    fn text(&self) -> String {
        self.doc.to_plain_text().unwrap_or_default()
    }

    fn caret(&self) -> usize {
        self.handle.cursor_position()
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        press(&mut self.tree, key, modifiers);
        layout(&mut self.tree);
    }

    /// The top of the caret drawn at `position`, as the caret keys leave it.
    fn caret_top(&self, position: usize) -> f32 {
        let st = self.state.borrow();
        st.engine.caret_rect(position, CursorAffinity::Downstream)[1]
    }

    /// The top of the caret where the editor draws it now.
    fn drawn_caret_top(&self) -> f32 {
        let st = self.state.borrow();
        st.engine
            .caret_rect(st.cursor.position(), st.cursor_affinity)[1]
    }
}

#[test]
fn shift_enter_breaks_the_line_inside_the_block() {
    let mut ed = Mounted::new("verse one", 9);
    ed.press(Key::Enter, Modifiers::SHIFT);
    type_text(&mut ed.tree, "two");

    assert_eq!(ed.text(), format!("verse one{LS}two"));
    assert_eq!(ed.doc.block_count(), 1, "still one paragraph");
    assert_eq!(ed.caret(), 13);
    assert!(
        ed.drawn_caret_top() > ed.caret_top(0),
        "the caret is on the second line"
    );
}

#[test]
fn shift_enter_at_the_end_of_a_paragraph_opens_an_empty_line() {
    let mut ed = Mounted::new("last line", 9);
    let first_line = ed.caret_top(0);
    ed.press(Key::Enter, Modifiers::SHIFT);

    assert_eq!(ed.text(), format!("last line{LS}"));
    assert_eq!(ed.caret(), 10);
    assert!(
        ed.drawn_caret_top() > first_line,
        "the caret waits on an empty line of its own"
    );
}

#[test]
fn enter_still_starts_a_new_paragraph() {
    let mut ed = Mounted::new("one", 3);
    ed.press(Key::Enter, Modifiers::NONE);
    assert_eq!(ed.doc.block_count(), 2);
    assert!(!ed.text().contains(LS));
}

#[test]
fn shift_enter_replaces_the_selection_in_one_undo_step() {
    let mut ed = Mounted::new("keep CUT keep", 0);
    ed.handle.select_range(5, 8);
    ed.press(Key::Enter, Modifiers::SHIFT);
    assert_eq!(ed.text(), format!("keep {LS} keep"));

    ed.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(ed.text(), "keep CUT keep", "one undo takes it all back");
}

#[test]
fn end_stops_before_the_break_and_home_starts_after_it() {
    // a b c LS d e f: the break is offset 3, the second line starts at 4.
    let mut ed = Mounted::new("abcdef", 3);
    assert!(ed.handle.insert_line_break());
    assert_eq!(ed.text(), format!("abc{LS}def"));
    layout(&mut ed.tree);

    ed.handle.select_range(1, 1);
    ed.press(Key::End, Modifiers::NONE);
    assert_eq!(ed.caret(), 3, "End stops before the break");
    assert!(
        ed.drawn_caret_top() < ed.caret_top(4),
        "and the caret stays drawn on the first line"
    );

    ed.handle.select_range(6, 6);
    ed.press(Key::Home, Modifiers::NONE);
    assert_eq!(ed.caret(), 4, "Home goes to the start of the second line");

    ed.press(Key::End, Modifiers::NONE);
    assert_eq!(ed.caret(), 7);
}

#[test]
fn left_and_right_cross_the_break_in_one_step() {
    let mut ed = Mounted::new("abcdef", 3);
    assert!(ed.handle.insert_line_break());
    layout(&mut ed.tree);
    let first_line = ed.caret_top(0);

    ed.handle.select_range(3, 3);
    ed.press(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(ed.caret(), 4);
    assert!(ed.drawn_caret_top() > first_line, "on the second line");

    ed.press(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(ed.caret(), 3);
    assert_eq!(ed.drawn_caret_top(), first_line, "back on the first line");
}

#[test]
fn up_and_down_move_between_the_lines_of_one_block() {
    let mut ed = Mounted::new("abcdef", 3);
    assert!(ed.handle.insert_line_break());
    layout(&mut ed.tree);

    let first_line = ed.caret_top(0);
    let second_line = ed.caret_top(4);
    assert!(second_line > first_line);

    ed.handle.select_range(1, 1);
    ed.press(Key::ArrowDown, Modifiers::NONE);
    assert!(
        (4..=7).contains(&ed.caret()) && ed.drawn_caret_top() == second_line,
        "Down lands on the second line, got {}",
        ed.caret()
    );

    ed.press(Key::ArrowUp, Modifiers::NONE);
    assert!(
        ed.caret() <= 3 && ed.drawn_caret_top() == first_line,
        "Up lands back on the first line, got {}",
        ed.caret()
    );
    assert_eq!(ed.caret(), 1, "and back in the column it left");
}

#[test]
fn insert_line_break_is_one_undo_step() {
    let mut ed = Mounted::new("ab", 1);
    assert!(ed.handle.insert_line_break());
    assert_eq!(ed.text(), format!("a{LS}b"));
    ed.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(ed.text(), "ab");
}

#[test]
fn shift_enter_in_a_table_cell_does_what_enter_does() {
    let doc = TextDocument::new();
    doc.set_plain_text("").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.insert_table(2, 2);
    let handle = editor.handle();
    let state = editor.state_handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    focus(&mut tree, id);
    let row = || {
        state
            .borrow()
            .cursor
            .current_table_cell()
            .map(|cell| cell.row)
    };
    assert_eq!(
        row(),
        Some(0),
        "precondition: the caret is in the first row"
    );

    assert!(
        !handle.insert_line_break(),
        "a table cell refuses the API's line break"
    );
    press(&mut tree, Key::Enter, Modifiers::SHIFT);

    assert!(!doc.to_plain_text().unwrap_or_default().contains(LS));
    assert_eq!(row(), Some(1), "Shift+Enter moved to the cell below");
}

#[test]
fn shift_enter_in_a_code_block_does_what_enter_does() {
    let doc = TextDocument::new();
    doc.set_markdown("```\nlet x = 1;\n```\n")
        .unwrap()
        .wait()
        .unwrap();
    let mut ed = Mounted::over(doc, RichTextEditor::editor, 0);
    assert_eq!(
        ed.state
            .borrow()
            .cursor
            .block_format()
            .unwrap()
            .is_code_block,
        Some(true),
        "precondition: the caret is in the code block"
    );
    let blocks = ed.doc.block_count();

    ed.handle.select_range(10, 10);
    assert!(
        !ed.handle.insert_line_break(),
        "a code block refuses the API's line break"
    );
    ed.press(Key::Enter, Modifiers::SHIFT);

    assert!(!ed.text().contains(LS));
    assert_eq!(ed.doc.block_count(), blocks + 1, "a new line of code");
}

/// At a block boundary the caret belongs to the block it ends, so the guard
/// reads the right block on both sides of a code block.
#[test]
fn the_code_block_guard_holds_at_both_ends_of_a_line() {
    let doc = TextDocument::new();
    doc.set_markdown("Before\n\n```\ncode\n```\n\nAfter\n")
        .unwrap()
        .wait()
        .unwrap();
    let text = doc.to_plain_text().unwrap_or_default();
    let code_end = text.find("code").unwrap() + "code".len();
    let before_end = "Before".len();
    let mut ed = Mounted::over(doc, RichTextEditor::editor, 0);

    // The end of the code line: still the code block, so Enter's new line.
    ed.handle.select_range(code_end, code_end);
    assert!(!ed.handle.insert_line_break(), "the end of a code line");
    // The end of the paragraph just before the code block: a line break.
    ed.handle.select_range(before_end, before_end);
    ed.press(Key::Enter, Modifiers::SHIFT);
    assert!(
        ed.text().starts_with(&format!("Before{LS}")),
        "got {:?}",
        ed.text()
    );
}

/// The same for a table: the end of the paragraph before it is not in it.
#[test]
fn the_end_of_a_paragraph_before_a_table_takes_a_line_break() {
    let doc = TextDocument::new();
    doc.set_markdown("Before\n\n| a | b |\n|---|---|\n| c | d |\n")
        .unwrap()
        .wait()
        .unwrap();
    let ed = Mounted::over(doc, RichTextEditor::editor, 0);
    ed.handle.select_range(6, 6);
    assert!(ed.handle.insert_line_break());
    assert!(
        ed.text().starts_with(&format!("Before{LS}")),
        "got {:?}",
        ed.text()
    );
}

#[test]
fn a_read_only_editor_refuses_shift_enter() {
    let doc = TextDocument::new();
    doc.set_plain_text("fixed").unwrap();
    let mut ed = Mounted::over(doc, RichTextEditor::read_only, 5);
    ed.press(Key::Enter, Modifiers::SHIFT);
    assert_eq!(ed.text(), "fixed");
}

#[test]
fn forward_only_takes_shift_enter_and_keeps_the_selected_text() {
    let mut ed = Mounted::new("never lost", 0);
    ed.handle
        .set_command_filter(super::policy::CommandFilter::ForwardOnly);
    ed.handle.select_range(0, 5);
    ed.press(Key::Enter, Modifiers::SHIFT);
    assert_eq!(
        ed.text(),
        format!("never{LS} lost"),
        "the break goes in after the selection, not over it"
    );
}

/// The menu row's way in keeps the selected text too, as the key does.
#[test]
fn forward_only_insert_line_break_keeps_the_selected_text() {
    let ed = Mounted::new("never lost", 0);
    ed.handle
        .set_command_filter(super::policy::CommandFilter::ForwardOnly);
    ed.handle.select_range(0, 5);
    assert!(ed.handle.insert_line_break());
    assert_eq!(ed.text(), format!("never{LS} lost"));
}

/// The memory clipboard carries HTML, so a paste back is the stored fragment.
#[test]
fn copy_gives_other_programs_lines_and_this_editor_its_break() {
    let doc = TextDocument::new();
    doc.set_plain_text("abcdef").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let mut tree = WidgetTree::new();
    let clipboard = super::tests::ctx_with_memory_clipboard(&mut tree);
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    focus(&mut tree, id);
    handle.select_range(3, 3);
    assert!(handle.insert_line_break());

    handle.select_range(0, 7);
    press(&mut tree, Key::C, Modifiers::COMMAND);
    assert_eq!(clipboard.get_text().unwrap_or_default(), "abc\ndef");

    handle.select_range(7, 7);
    press(&mut tree, Key::V, Modifiers::COMMAND);
    layout(&mut tree);
    assert_eq!(
        doc.to_plain_text().unwrap_or_default(),
        format!("abc{LS}defabc{LS}def")
    );
    assert_eq!(doc.block_count(), 1);
}

/// A clipboard with no HTML: the paste recognises this editor's own copy by
/// its plain text, which is the mapped one.
#[test]
fn a_plain_only_clipboard_still_round_trips_the_break() {
    use std::any::TypeId;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use teksilo_core::event_source::TreeAppContext;
    use teksilo_platform::clipboard::{ClipboardBackend, ClipboardHandle};

    struct PlainOnly(Rc<RefCell<String>>);
    impl ClipboardBackend for PlainOnly {
        fn get_text(&mut self) -> Result<String, String> {
            Ok(self.0.borrow().clone())
        }
        fn set_text(&mut self, text: &str) -> Result<(), String> {
            *self.0.borrow_mut() = text.to_string();
            Ok(())
        }
    }
    let text = Rc::new(RefCell::new(String::new()));
    let clipboard = ClipboardHandle::new(PlainOnly(text.clone()));

    let doc = TextDocument::new();
    doc.set_plain_text("abcdef").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let mut tree = WidgetTree::new();
    let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
    registry.insert(TypeId::of::<ClipboardHandle>(), Box::new(clipboard));
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(registry)));
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    focus(&mut tree, id);
    handle.select_range(3, 3);
    assert!(handle.insert_line_break());

    handle.select_range(0, 7);
    press(&mut tree, Key::C, Modifiers::COMMAND);
    assert_eq!(*text.borrow(), "abc\ndef");

    handle.select_range(7, 7);
    press(&mut tree, Key::V, Modifiers::COMMAND);
    layout(&mut tree);
    assert_eq!(
        doc.to_plain_text().unwrap_or_default(),
        format!("abc{LS}defabc{LS}def"),
        "the paste is the stored fragment, break and all, not two paragraphs"
    );
}

#[test]
fn plain_text_for_other_programs_maps_the_separator() {
    let doc = TextDocument::new();
    doc.set_plain_text("one").unwrap();
    let cursor = doc.cursor_at(3);
    cursor.insert_line_break().unwrap();
    cursor.insert_text("two").unwrap();
    let all = doc.cursor_at(0);
    all.set_position(7, teksilo_text::text_document::MoveMode::KeepAnchor);
    let fragment = all.selection();
    assert_eq!(fragment.to_plain_text(), format!("one{LS}two"));
    assert_eq!(
        super::clipboard::plain_text_for_other_programs(&fragment),
        "one\ntwo"
    );
}

/// The messenger-composer pattern: a host claims plain Enter (send) in its
/// `on_key_preview`, and Shift+Enter still reaches the editor, now as a line
/// break, the way chat composers behave.
#[test]
fn a_composer_that_claims_enter_still_gets_line_breaks() {
    use crate::primitives::VStack;
    use std::cell::Cell;
    use std::rc::Rc;

    let sent = Rc::new(Cell::new(0u32));
    let doc = TextDocument::new();
    doc.set_plain_text("hello").unwrap();
    let editor = RichTextEditor::editor(doc.clone())
        .min_lines(1)
        .max_lines(6);
    let handle = editor.handle();
    let composer = VStack::new().child(editor).on_key_preview({
        let sent = sent.clone();
        move |event, _ctx| match event {
            WidgetEvent::KeyDown {
                key: Key::Enter,
                modifiers,
                ..
            } if *modifiers == Modifiers::NONE => {
                sent.set(sent.get() + 1);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    });
    let mut tree = WidgetTree::new();
    let id = tree.add(composer);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    focus(&mut tree, id);
    handle.select_range(5, 5);

    press(&mut tree, Key::Enter, Modifiers::SHIFT);
    assert_eq!(sent.get(), 0, "Shift+Enter is not a send");
    assert_eq!(
        doc.to_plain_text().unwrap_or_default(),
        format!("hello{LS}")
    );

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(sent.get(), 1, "Enter sends");
    assert_eq!(doc.block_count(), 1, "and starts no paragraph");
}

#[test]
fn accessibility_reads_the_separator_as_a_hard_break() {
    use teksilo_core::accesskit::Role;

    let mut ed = Mounted::new("abcdef", 3);
    assert!(ed.handle.insert_line_break());
    layout(&mut ed.tree);
    let _ = ed.tree.render();

    let update = ed.tree.sync_accessibility();
    let mut runs: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::TextRun)
        .map(|(_, n)| n)
        .collect();
    runs.sort_by(|a, b| {
        let top = |n: &&teksilo_core::accesskit::Node| n.bounds().map_or(0.0, |r| r.y0);
        top(a).total_cmp(&top(b))
    });
    let values: Vec<String> = runs
        .iter()
        .map(|n| n.value().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        values,
        [format!("abc{LS}"), "def\n".to_string()],
        "two lines, the break ending the first as one character, the \
         paragraph's own ending the second"
    );
    let first = runs[0];
    assert_eq!(
        first.character_lengths().last().copied(),
        Some(LS.len_utf8() as u8),
        "the break is one character of three bytes"
    );
    assert_eq!(first.character_lengths().len(), 4);
}

// ── Plain text from outside: paste, drop, assistive technology ──────────
//
// `insert_text` keeps every character in one block, a `\n` included, which the
// typesetter shows as a line the document does not have. Text arriving from
// another program, or written by an assistive technology, has its paragraph
// breaks made blocks and its line breaks made `LINE_SEPARATOR`.

#[test]
fn a_plain_paste_makes_paragraphs_of_paragraph_breaks_and_keeps_line_breaks() {
    let mut ed = Mounted::new("", 0);
    ed.clipboard
        .set_text("a\r\nb\u{2029}c\u{b}d\u{2028}e\u{c}f")
        .unwrap();
    ed.press(Key::V, Modifiers::COMMAND);
    assert_eq!(ed.text(), format!("a\nb\nc{LS}d{LS}e\nf"));
    assert_eq!(ed.doc.block_count(), 4);
}

#[test]
fn a_text_drop_from_another_program_makes_its_lines_paragraphs() {
    use teksilo_core::ExternalDropData;
    use teksilo_core::window::NoopWindowOps;
    let mut ed = Mounted::new("", 0);
    let data = || ExternalDropData {
        text: Some("one\ntwo".to_string()),
        formats: vec!["text/plain".to_string()],
        ..Default::default()
    };
    let at = teksilo_canvas::Point::new(40.0, 20.0);
    ed.tree.begin_external_drag(at, data(), &mut NoopWindowOps);
    ed.tree.end_external_drag(at, data(), &mut NoopWindowOps);
    layout(&mut ed.tree);
    assert_eq!(ed.text(), "one\ntwo");
    assert_eq!(ed.doc.block_count(), 2);
}

fn access_action(ed: &mut Mounted, action: teksilo_core::accesskit::Action, value: &str) {
    use teksilo_core::accessibility::widget_id_to_node_id;
    use teksilo_core::accesskit::ActionData;
    let mut ops = teksilo_core::window::NoopWindowOps;
    assert!(ed.tree.dispatch_access_action(
        widget_id_to_node_id(ed.id),
        action,
        Some(ActionData::Value(value.into())),
        &mut ops,
    ));
    layout(&mut ed.tree);
}

/// An assistive technology writes the whole text back through `SetValue` on
/// every desktop platform; AccessKit reads a `\n` ending a run as a paragraph
/// end and anything else as a line end, so the two come back as such.
#[test]
fn set_value_keeps_paragraphs_and_line_breaks() {
    let mut ed = Mounted::new("first", 0);
    access_action(
        &mut ed,
        teksilo_core::accesskit::Action::SetValue,
        &format!("alpha\nbeta{LS}gamma"),
    );
    assert_eq!(ed.text(), format!("alpha\nbeta{LS}gamma"));
    assert_eq!(ed.doc.block_count(), 2);
}

#[test]
fn replace_selected_text_keeps_paragraphs() {
    let mut ed = Mounted::new("ab", 1);
    access_action(
        &mut ed,
        teksilo_core::accesskit::Action::ReplaceSelectedText,
        "X\nY",
    );
    assert_eq!(ed.text(), "aX\nYb");
    assert_eq!(ed.doc.block_count(), 2);
}

#[test]
fn a_typed_paragraph_separator_starts_a_paragraph() {
    let mut ed = Mounted::new("ab", 1);
    ed.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::NONE,
        text: Some("\u{2029}".to_string()),
    });
    layout(&mut ed.tree);
    assert_eq!(ed.text(), "a\nb");
    assert_eq!(ed.doc.block_count(), 2);
}

// ── can_insert_line_break: the query a menu row greys itself by ─────────

/// An editor over `markdown`, mounted, with the caret on `needle`'s second
/// character (or at the start when `needle` is empty).
fn mounted_markdown(markdown: &str, needle: &str) -> Mounted {
    let doc = TextDocument::new();
    doc.set_markdown(markdown).unwrap().wait().unwrap();
    let text = doc.to_plain_text().unwrap_or_default();
    let caret = text
        .find(needle)
        .map(|byte| text[..byte].chars().count() + 1)
        .unwrap_or(0);
    Mounted::over(doc, RichTextEditor::editor, caret)
}

/// The query answers exactly what `insert_line_break` then does, in each
/// kind of block: yes in a paragraph, a list item and a quotation, no in a
/// table cell and a code block.
#[test]
fn can_insert_line_break_agrees_with_insert_line_break() {
    for (markdown, needle, expected) in [
        ("A plain paragraph\n", "plain", true),
        ("- first item\n- second item\n", "second", true),
        ("> a quotation\n", "quotation", true),
        ("Before\n\n```\nlet x = 1;\n```\n", "let", false),
    ] {
        let ed = mounted_markdown(markdown, needle);
        assert_eq!(
            ed.handle.can_insert_line_break(),
            expected,
            "query in {markdown:?}"
        );
        assert_eq!(
            ed.handle.insert_line_break(),
            expected,
            "call in {markdown:?}"
        );
    }

    // A table cell, entered the way the table tests enter one.
    let doc = TextDocument::new();
    doc.set_plain_text("").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.insert_table(2, 2);
    let handle = editor.handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    focus(&mut tree, id);
    assert!(!handle.can_insert_line_break(), "query in a table cell");
    assert!(!handle.insert_line_break(), "call in a table cell");
}

/// The widget's own copy asks the same question, before it is mounted too.
#[test]
fn the_widget_answers_can_insert_line_break_as_its_handle_does() {
    let doc = TextDocument::new();
    doc.set_markdown("```\ncode\n```\n")
        .unwrap()
        .wait()
        .unwrap();
    let editor = RichTextEditor::editor(doc);
    editor.set_caret_position(2);
    assert!(!editor.can_insert_line_break());
    assert_eq!(
        editor.can_insert_line_break(),
        editor.handle().can_insert_line_break()
    );

    let prose = RichTextEditor::editor({
        let doc = TextDocument::new();
        doc.set_plain_text("prose").unwrap();
        doc
    });
    prose.set_caret_position(2);
    assert!(prose.can_insert_line_break());
    assert!(prose.insert_line_break());
}

/// The query ignores the command filter, as the call does: a read-only
/// editor still says a line break fits, and its host checks the filter.
#[test]
fn can_insert_line_break_does_not_consult_the_filter() {
    let doc = TextDocument::new();
    doc.set_plain_text("fixed").unwrap();
    let ed = Mounted::over(doc, RichTextEditor::read_only, 2);
    assert!(ed.handle.can_insert_line_break());
    assert!(
        !ed.handle
            .command_filter()
            .accepts(super::policy::EditCommandKind::InsertLineBreak),
        "the filter is the host's to check"
    );
}

/// Asked where the break would go in: over a selection reaching into a code
/// block that is the selection's start, unless a forward-only filter keeps
/// the selection and the break goes in after it, inside the code.
#[test]
fn can_insert_line_break_asks_where_the_break_would_go_in() {
    let markdown = "Before\n\n```\nlet x = 1;\n```\n";
    let into_code = |ed: &Mounted| {
        let text = ed.text();
        let code = text.find("let").unwrap() + 2;
        ed.handle.select_range(2, code);
    };

    let ed = mounted_markdown(markdown, "");
    into_code(&ed);
    assert!(
        ed.handle.can_insert_line_break(),
        "replacing it: in 'Before'"
    );
    assert!(ed.handle.insert_line_break());
    assert!(ed.text().starts_with(&format!("Be{LS}")), "{:?}", ed.text());

    // Selected backwards, the caret in "Before" and the far end in the code:
    // the caret alone would say yes, and the break would go into the code.
    let ed = mounted_markdown(markdown, "");
    ed.handle
        .set_command_filter(super::policy::CommandFilter::ForwardOnly);
    let code = ed.text().find("let").unwrap() + 2;
    ed.handle.select_range(code, 2);
    let before = ed.text();
    assert!(!ed.handle.can_insert_line_break(), "after it: in the code");
    assert!(!ed.handle.insert_line_break());
    assert_eq!(ed.text(), before, "nothing went into the code block");
}

/// A host greys its row from `on_change`, which runs while the editor is
/// busy with its own state: the query answers rather than panicking.
#[test]
fn can_insert_line_break_is_safe_from_on_change() {
    use std::cell::Cell;
    use std::rc::Rc;
    let answers = Rc::new(Cell::new(0u32));
    let all_true = Rc::new(Cell::new(true));
    let doc = TextDocument::new();
    doc.set_plain_text("ab").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let editor = editor.on_change({
        let answers = answers.clone();
        let all_true = all_true.clone();
        move || {
            all_true.set(all_true.get() && handle.can_insert_line_break());
            answers.set(answers.get() + 1);
        }
    });
    let mut ed = Mounted::with_editor(doc, editor, 2);
    type_text(&mut ed.tree, "c");
    layout(&mut ed.tree);
    assert_eq!(ed.text(), "abc");
    assert!(answers.get() > 0, "on_change ran and asked");
    assert!(
        all_true.get(),
        "while the editor is busy it answers `true`, the row's answer before"
    );
}

/// The table half of the guard holds at every caret position a table has,
/// each cell's end included, and only there.
#[test]
fn can_insert_line_break_refuses_everywhere_in_a_table() {
    let ed = mounted_markdown(
        "Before\n\n| ab | cd |\n|---|---|\n| ef | gh |\n\nAfter\n",
        "",
    );
    let mut position = 0;
    while {
        ed.handle.select_range(position, position);
        ed.handle.cursor_position() == position
    } {
        let in_table = ed.state.borrow().cursor.current_table().is_some();
        assert_eq!(
            ed.handle.can_insert_line_break(),
            !in_table,
            "caret at {position}"
        );
        position += 1;
    }
    assert!(position > 20, "the walk reached past the table");
}
