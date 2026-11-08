// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Ctrl+Up and Ctrl+Down step by paragraph on Windows and Linux, as in Word,
//! WordPad and the GTK and Qt text views: up to the start of the caret's
//! paragraph, or of the one before from a start; down to the start of the next
//! paragraph. NVDA reads the paragraph the caret lands in after either key, so
//! a caret that moved only a line had it read the same paragraph again.
//!
//! macOS keeps its own `⌥↑/↓` (to the edges of the caret's paragraph), so
//! these run only where the PC convention is the build's.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::{EditorHandle, RichTextEditor};

const WIDTH: f32 = 400.0;

struct Editor {
    doc: TextDocument,
    handle: EditorHandle,
    tree: WidgetTree,
}

impl Editor {
    fn plain(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::over(doc)
    }

    fn markdown(markdown: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_markdown(markdown).unwrap().wait().unwrap();
        Self::over(doc)
    }

    fn over(doc: TextDocument) -> Self {
        let editor = RichTextEditor::editor(doc.clone());
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(WIDTH, 300.0));
        let _ = tree.render();
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(20.0, 20.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        let focused = tree.focused();
        assert!(focused.is_some_and(|f| f == id || tree.is_descendant_of(f, id)));
        let mut editor = Self { doc, handle, tree };
        editor.settle();
        editor
    }

    fn settle(&mut self) {
        self.tree.layout(SizeProposal::exact(WIDTH, 300.0));
        let _ = self.tree.render();
    }

    /// Put the caret at `needle`'s first character, plus `offset`.
    fn caret_at(&mut self, needle: &str, offset: usize) {
        let at = self.position_of(needle) + offset;
        self.handle.select_range(at, at);
        self.settle();
    }

    /// The position of `needle`'s first character, found block by block so
    /// it holds in a table too, where a position is not a plain-text offset.
    fn position_of(&self, needle: &str) -> usize {
        let mut block = self.doc.block_by_number(0);
        while let Some(b) = block {
            if let Some(i) = b.text().find(needle) {
                return b.position() + b.text()[..i].chars().count();
            }
            block = b.next();
        }
        // `next` does not go into a table's cells; look there too.
        self.doc
            .flow()
            .into_iter()
            .find_map(|element| match element {
                teksilo_text::text_document::FlowElement::Table(table) => (0..table.rows())
                    .find_map(|row| {
                        (0..table.columns()).find_map(|col| {
                            table.cell(row, col)?.blocks().into_iter().find_map(|b| {
                                let i = b.text().find(needle)?;
                                Some(b.position() + b.text()[..i].chars().count())
                            })
                        })
                    }),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is in the document"))
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.settle();
    }

    fn ctrl_down(&mut self) {
        self.press(Key::ArrowDown, Modifiers::CTRL);
    }

    fn ctrl_up(&mut self) {
        self.press(Key::ArrowUp, Modifiers::CTRL);
    }

    fn caret(&self) -> usize {
        self.handle.cursor_position()
    }

    fn anchor(&self) -> usize {
        self.handle.cursor_anchor_signal().get()
    }
}

const THREE: &str = "First paragraph here.\nSecond one, a bit longer.\nThird and last.";

#[test]
fn ctrl_down_walks_paragraph_starts() {
    let mut e = Editor::plain(THREE);
    e.caret_at("paragraph", 0);
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("Second"));
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("Third"));
}

#[test]
fn ctrl_down_in_the_last_paragraph_goes_to_its_end_and_stays() {
    let mut e = Editor::plain(THREE);
    e.caret_at("Third", 2);
    let end = e.position_of("last.") + "last.".len();
    e.ctrl_down();
    assert_eq!(e.caret(), end);
    e.ctrl_down();
    assert_eq!(e.caret(), end, "nothing follows the last paragraph");
}

#[test]
fn ctrl_up_goes_to_the_paragraph_start_then_to_the_one_before() {
    let mut e = Editor::plain(THREE);
    e.caret_at("Third", 4);
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("Third"), "this paragraph's start");
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("Second"), "the one before");
    e.ctrl_up();
    assert_eq!(e.caret(), 0);
    e.ctrl_up();
    assert_eq!(e.caret(), 0, "nothing comes before the first paragraph");
}

#[test]
fn ctrl_up_from_a_paragraph_end_stays_in_that_paragraph() {
    // At the end of a paragraph a character index reads the separator after
    // it, which belongs to the next paragraph: the step must go to this
    // paragraph's start, not the next one's.
    let mut e = Editor::plain(THREE);
    let end_of_second = e.position_of("longer.") + "longer.".len();
    e.handle.select_range(end_of_second, end_of_second);
    e.settle();
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("Second"));
}

#[test]
fn ctrl_steps_reach_an_empty_paragraph() {
    let mut e = Editor::plain("One\n\n\nFour");
    // Positions: "One" 0..3, empty at 4, empty at 5, "Four" at 6.
    e.caret_at("One", 1);
    e.ctrl_down();
    assert_eq!(e.caret(), 4);
    e.ctrl_down();
    assert_eq!(e.caret(), 5);
    e.ctrl_down();
    assert_eq!(e.caret(), 6);
    e.ctrl_up();
    assert_eq!(e.caret(), 5);
    e.ctrl_up();
    assert_eq!(e.caret(), 4);
    e.ctrl_up();
    assert_eq!(e.caret(), 0);
}

#[test]
fn shift_extends_the_selection_by_paragraph() {
    let mut e = Editor::plain(THREE);
    e.caret_at("paragraph", 0);
    let anchor = e.caret();
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.anchor(), anchor, "the selection keeps its start");
    assert_eq!(e.caret(), e.position_of("Second"));
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.caret(), e.position_of("Third"));
    e.press(Key::ArrowUp, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.caret(), e.position_of("Second"));
    assert_eq!(e.anchor(), anchor);
    assert_eq!(e.handle.selected_text(), "paragraph here.\n");
}

#[test]
fn shift_ctrl_up_selects_back_to_the_paragraph_start() {
    let mut e = Editor::plain(THREE);
    e.caret_at("bit", 0);
    let anchor = e.caret();
    e.press(Key::ArrowUp, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.anchor(), anchor);
    assert_eq!(e.caret(), e.position_of("Second"));
    assert_eq!(e.handle.selected_text(), "Second one, a ");
}

#[test]
fn ctrl_steps_walk_list_items_one_by_one() {
    let mut e = Editor::markdown("Intro.\n\n- apple pie\n- banana split\n- cherry tart\n\nOutro.");
    e.caret_at("Intro", 2);
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("apple"));
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("banana"));
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("cherry"));
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("Outro"));

    e.caret_at("banana", 3);
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("banana"));
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("apple"));
    e.ctrl_up();
    assert_eq!(e.caret(), 0);
}

#[test]
fn ctrl_steps_go_into_and_out_of_a_quotation() {
    let mut e = Editor::markdown("Before.\n\n> Quoted one.\n>\n> Quoted two.\n\nAfter.");
    e.caret_at("Before", 3);
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("Quoted one"));
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("Quoted two"));
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("After"));
    e.caret_at("After", 2);
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("After"));
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("Quoted two"));
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("Quoted one"));
    e.ctrl_up();
    assert_eq!(e.caret(), 0);
}

const TABLE: &str = "Before the table.\n\n\
                     | alpha | beta |\n\
                     |-------|------|\n\
                     | gamma | delta |\n\n\
                     After the table.";

fn table_editor() -> Editor {
    let e = Editor::markdown(TABLE);
    assert!(
        e.doc
            .flow()
            .iter()
            .any(|element| matches!(element, teksilo_text::text_document::FlowElement::Table(_))),
        "precondition: the markdown made a table"
    );
    e
}

#[test]
fn ctrl_down_goes_into_a_table_and_stays_in_its_cell() {
    let mut e = table_editor();
    e.caret_at("Before", 3);
    e.ctrl_down();
    assert_eq!(e.caret(), e.position_of("alpha"), "into the first cell");
    e.ctrl_down();
    let end_of_cell = e.position_of("alpha") + "alpha".len();
    assert_eq!(
        e.caret(),
        end_of_cell,
        "to the cell's end, not the next cell"
    );
    e.ctrl_down();
    assert_eq!(e.caret(), end_of_cell, "the cell keeps the step");
}

#[test]
fn ctrl_up_stays_in_a_cell_at_its_start() {
    let mut e = table_editor();
    e.caret_at("delta", 2);
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("delta"));
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("delta"), "the cell keeps the step");
}

#[test]
fn ctrl_up_from_after_a_table_goes_into_its_last_cell() {
    let mut e = table_editor();
    e.caret_at("After", 0);
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("delta"));
}

#[test]
fn a_selection_holding_a_table_whole_steps_on_past_it() {
    // Shift+Ctrl+Down from before the table takes the table whole (a
    // selection from outside a table never holds part of one), which leaves
    // the caret at the end of its last cell. The cell must not keep the next
    // step: the selection's start is outside the table.
    let mut e = table_editor();
    e.caret_at("Before", 0);
    let anchor = e.caret();
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.anchor(), anchor);
    assert!(
        e.caret() >= e.position_of("After"),
        "the selection reaches past the table (caret {}, after the table at {})",
        e.caret(),
        e.position_of("After"),
    );
}
