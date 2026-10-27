// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The text a screen reader gets from a rich-text editor, read the way the
//! platform adapters read it: through `accesskit_consumer`.
//!
//! The consumer concatenates the runs, ends a paragraph where a run ends in a
//! `\n`, and counts an offset in Unicode scalars. So each paragraph's last run
//! ends with the separator the document has after it, and for plain text a
//! reader's offset is the document's position: the end of one paragraph and
//! the start of the next are two offsets, not one (reader finding text-03).

use accesskit_consumer::{NodeRef, Tree};
use teksilo_canvas::SizeProposal;
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::accesskit::{Action, ActionData, Role, TextSelection, TreeUpdate};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{LINE_SEPARATOR, TextDocument};

use super::{EditorHandle, RichTextEditor};

struct Reader {
    doc: TextDocument,
    handle: EditorHandle,
    tree: WidgetTree,
    id: WidgetId,
}

impl Reader {
    fn plain(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::over(doc, RichTextEditor::editor)
    }

    fn markdown(markdown: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_markdown(markdown).unwrap().wait().unwrap();
        Self::over(doc, RichTextEditor::editor)
    }

    fn over(doc: TextDocument, make: fn(TextDocument) -> RichTextEditor) -> Self {
        let editor = make(doc.clone());
        Self::mounted(doc, editor)
    }

    fn mounted(doc: TextDocument, editor: RichTextEditor) -> Self {
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        tree.dispatch_event(WidgetEvent::pointer_down(
            teksilo_canvas::Point::new(20.0, 20.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        Self {
            doc,
            handle,
            tree,
            id,
        }
    }

    fn update(&mut self) -> TreeUpdate {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(200));
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = self.tree.render();
        self.tree.sync_accessibility()
    }

    /// The editor's text as a reader reviews it.
    fn text(&mut self) -> String {
        with_editor(&self.update(), |editor| editor.document_range().text())
    }

    /// The reader's offset for the caret, after putting it at `position`.
    fn offset_of(&mut self, position: usize) -> usize {
        self.handle.select_range(position, position);
        with_editor(&self.update(), |editor| {
            editor
                .text_selection_focus()
                .expect("a focused editor reports its caret")
                .to_global_usv_index()
        })
    }

    /// Put the caret where a reader's `SetTextSelection` at `offset` puts it,
    /// with the position the adapter derives from that offset.
    fn select_from_reader(&mut self, offset: usize) -> usize {
        let at = with_editor(&self.update(), |editor| {
            editor
                .text_position_from_global_usv_index(offset)
                .expect("an offset inside the text")
                .to_raw()
        });
        let mut ops = teksilo_core::window::NoopWindowOps;
        assert!(self.tree.dispatch_access_action(
            widget_id_to_node_id(self.id),
            Action::SetTextSelection,
            Some(ActionData::SetTextSelection(TextSelection {
                anchor: at,
                focus: at,
            })),
            &mut ops,
        ));
        self.handle.cursor_position()
    }

    fn document_len(&self) -> usize {
        self.doc.to_plain_text().unwrap_or_default().chars().count()
    }
}

fn with_editor<R>(update: &TreeUpdate, read: impl FnOnce(&NodeRef<'_>) -> R) -> R {
    let tree = Tree::new(update.clone(), true);
    let state = tree.state();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if matches!(node.role(), Role::MultilineTextInput | Role::Document) {
            return read(&node);
        }
        stack.extend(node.children());
    }
    panic!("no editor node in the tree");
}

#[test]
fn paragraphs_reach_a_reader_with_a_break_between_them() {
    let mut r = Reader::plain("One\nTwo\n\nFour");
    assert_eq!(r.text(), "One\nTwo\n\nFour\n");
}

/// Every caret position, a line break and an empty paragraph included, is the
/// same offset for a reader, so Right across a paragraph break moves on and the
/// caret is never reported in the other paragraph.
#[test]
fn a_readers_offset_is_the_document_position() {
    let mut r = Reader::plain("café one\n\nde");
    r.handle.select_range(4, 4);
    assert!(r.handle.insert_line_break());
    let len = r.document_len();
    assert_eq!(r.text(), format!("café{LINE_SEPARATOR} one\n\nde\n"));
    for position in 0..=len {
        assert_eq!(r.offset_of(position), position, "caret at {position}");
    }
}

#[test]
fn the_character_at_a_paragraphs_end_is_its_break() {
    let mut r = Reader::plain("One\nTwo");
    let text = r.text();
    let end_of_one = r.offset_of(3);
    assert_eq!(text.chars().nth(end_of_one), Some('\n'));
    assert_eq!(r.offset_of(4), 4, "the next paragraph starts one further");
}

/// Paragraph navigation stops at the paragraph, where it ran from the top of
/// the document to the first `\n` a code block happened to hold.
#[test]
fn a_paragraph_is_one_paragraph_for_a_reader() {
    let mut r = Reader::markdown("# Title\n\nFirst paragraph.\n\nSecond.\n");
    let text = r.text();
    assert_eq!(text, "Title\nFirst paragraph.\nSecond.\n");
    r.handle.select_range(10, 10);
    let (start, end) = with_editor(&r.update(), |editor| {
        let caret = editor.text_selection_focus().unwrap();
        (
            caret.backward_to_paragraph_start().to_global_usv_index(),
            caret.forward_to_paragraph_end().to_global_usv_index(),
        )
    });
    assert_eq!((start, end), (6, 23), "{:?}", &text[start..end]);
}

#[test]
fn enter_adds_a_character_a_reader_hears() {
    let mut r = Reader::plain("One");
    let before = r.text().chars().count();
    r.handle.select_range(3, 3);
    r.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(r.text().chars().count(), before + 1);
}

/// `SetTextSelection` lands where the reader asked: at a paragraph start, and
/// past a character longer than a byte, where the byte offset used to be added
/// to a character position.
#[test]
fn a_readers_selection_lands_on_the_document_position() {
    let mut r = Reader::plain("café\nnext");
    for position in [0, 3, 4, 5, 7, 9] {
        assert_eq!(
            r.select_from_reader(position),
            position,
            "offset {position}"
        );
    }
}

/// An image is one character of the document, heard as its description in its
/// place. It used to be a run of its own after the paragraph, which also
/// counted it twice.
#[test]
fn an_image_is_heard_in_its_place_and_counted_once() {
    let doc = TextDocument::new();
    doc.set_plain_text("Para one\nLast").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.set_caret_position(4);
    editor.insert_image("pic", "a cat", 10, 10);
    let mut r = Reader::mounted(doc, editor);
    assert_eq!(r.text(), "Paraa cat one\nLast\n");
    // The caret right after the image maps back to the position after it.
    let after_image = r.offset_of(5);
    assert_eq!(r.select_from_reader(after_image), 5);
    let last = r.offset_of(10);
    assert_eq!(r.select_from_reader(last), 10);
}

/// A table reads cell by cell, each cell's paragraph ending in its break.
///
/// A table has two positions of its own before its first cell, which the caret
/// stops on (Right from the paragraph before it takes three presses to reach
/// the first cell) and which no text holds: a reader is told the caret is at
/// the first cell there rather than nowhere. Every other position round-trips
/// through a reader's offset.
#[test]
fn a_table_reads_cell_by_cell_and_every_caret_is_reported() {
    let mut r = Reader::markdown("Before\n\n| a | b |\n|---|---|\n| c | d |\n\nAfter\n");
    assert_eq!(r.text(), "Before\na\nb\nc\nd\nAfter\n");
    let first_cell = 9;
    let mut last_position = 0;
    while {
        r.handle.select_range(last_position + 1, last_position + 1);
        r.handle.cursor_position() == last_position + 1
    } {
        last_position += 1;
    }
    let mut previous = 0;
    for position in 0..=last_position {
        let offset = r.offset_of(position);
        assert!(offset >= previous, "offsets follow positions at {position}");
        previous = offset;
        if (7..first_cell).contains(&position) {
            assert_eq!(offset, r.offset_of(first_cell), "table position {position}");
        } else {
            assert_eq!(
                r.select_from_reader(offset),
                position,
                "caret at {position}"
            );
        }
    }
}

#[test]
fn a_read_only_viewer_separates_paragraphs_too() {
    let doc = TextDocument::new();
    doc.set_plain_text("One\nTwo").unwrap();
    let mut r = Reader::over(doc, RichTextEditor::read_only);
    assert_eq!(r.text(), "One\nTwo\n");
}

/// A list item's first line starts with its marker as it is painted, a bullet
/// or a number and a space, nested items included (reader finding text-04).
/// A markdown numbered list's markers have no full stop: text-document paints
/// them as the bare number.
#[test]
fn a_list_item_is_heard_with_its_marker() {
    let mut r = Reader::markdown("Intro\n\n- one\n- two\n  1. nested\n\n1. first\n2. second\n");
    assert_eq!(
        r.text(),
        "Intro\n• one\n• two\n1 nested\n1 first\n2 second\n"
    );
    // The line a reader hears on Down onto the second item.
    let chars: Vec<char> = r.text().chars().collect();
    r.handle.select_range(10, 10);
    let (start, end) = with_editor(&r.update(), |editor| {
        let caret = editor.text_selection_focus().unwrap();
        (
            caret.backward_to_line_start().to_global_usv_index(),
            caret.forward_to_line_end().to_global_usv_index(),
        )
    });
    let line: String = chars[start..end].iter().collect();
    assert_eq!(line, "• two\n");
}

/// The marker is one character no document position is. The caret at an
/// item's start is reported after it, on the item's text; a reader's caret
/// put on it lands at the item's start; every position still round-trips.
#[test]
fn a_caret_is_never_inside_a_marker() {
    let mut r = Reader::markdown("Intro\n\n- one\n- two\n");
    let text = r.text();
    assert_eq!(text, "Intro\n• one\n• two\n");
    let chars: Vec<char> = text.chars().collect();
    for position in 0..=r.document_len() {
        let offset = r.offset_of(position);
        assert_ne!(chars.get(offset), Some(&'•'), "caret at {position}");
        assert_ne!(chars.get(offset), Some(&' '), "caret at {position}");
        assert_eq!(
            r.select_from_reader(offset),
            position,
            "caret at {position}"
        );
    }
    let one = "Intro\n".chars().count();
    assert_eq!(chars[r.offset_of(one)], 'o');
    let marker = "Intro\n".chars().count();
    assert_eq!(chars[marker], '•');
    assert_eq!(
        r.select_from_reader(marker),
        one,
        "a caret put on the marker"
    );
}

/// A selection running up to an item's start ends before the item's marker,
/// so a reader hears what was selected and not the next item's bullet; one
/// starting there starts after it.
#[test]
fn a_selection_up_to_an_item_holds_no_marker_of_it() {
    let mut r = Reader::markdown("Intro\n\n- one\n- two\n");
    let mut selected = |anchor: usize, focus: usize| {
        r.handle.select_range(anchor, focus);
        with_editor(&r.update(), |editor| {
            editor
                .text_selection()
                .map(|s| s.text())
                .unwrap_or_default()
        })
    };
    assert_eq!(selected(0, 6), "Intro\n");
    assert_eq!(selected(6, 0), "Intro\n");
    assert_eq!(selected(6, 10), "one\n", "Shift+Down from the item's start");
    assert_eq!(selected(10, 6), "one\n");
    assert_eq!(selected(0, 13), "Intro\n• one\n• two");
}
