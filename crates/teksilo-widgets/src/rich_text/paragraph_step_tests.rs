// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Ctrl+Up and Ctrl+Down step by paragraph on Windows and Linux, as in Word,
//! WordPad and the GTK and Qt text views: up to the start of the caret's
//! paragraph, or of the one before from a start; down to the start of the next
//! paragraph. NVDA reads the paragraph the caret lands in after either key, so
//! a caret that moved only a line had it read the same paragraph again. A line
//! break inside a block (Shift+Enter) ends a paragraph for the step as it does
//! for NVDA.
//!
//! macOS keeps its own `⌥↑/↓` (to the edges of the caret's paragraph), so
//! these run only where the PC convention is the build's.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, SelectionKind, TextDocument};

use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};

const WIDTH: f32 = 400.0;

struct Editor {
    doc: TextDocument,
    handle: EditorHandle,
    state: SharedState,
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
        let state = editor.state_handle();
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
        let mut editor = Self {
            doc,
            handle,
            state,
            tree,
        };
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

    fn kind(&self) -> SelectionKind {
        self.state.borrow().cursor.selection_kind()
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

/// The caret positions of a table's own, ahead of its first cell, that a
/// caret can stand on (an assistive technology or a program can put it
/// there): each reads as in the first cell's first paragraph, whose start
/// lies after it.
fn anchor_positions(e: &Editor, first_cell: &str) -> Vec<usize> {
    let cell = e.position_of(first_cell);
    let anchors: Vec<usize> = (0..cell)
        .filter(|&at| {
            e.doc
                .block_at_caret(at)
                .is_ok_and(|block| block.start == cell)
        })
        .collect();
    assert!(
        !anchors.is_empty(),
        "precondition: the table has a position of its own before {first_cell:?}"
    );
    anchors
}

#[test]
fn ctrl_down_from_a_tables_anchor_goes_to_its_first_cells_start() {
    let mut e = table_editor();
    for at in anchor_positions(&e, "alpha") {
        e.handle.select_range(at, at);
        e.settle();
        e.ctrl_down();
        assert_eq!(e.caret(), e.position_of("alpha"), "from {at}");
    }
}

#[test]
fn ctrl_up_from_a_tables_anchor_goes_to_the_paragraph_before() {
    let mut e = table_editor();
    for at in anchor_positions(&e, "alpha") {
        e.handle.select_range(at, at);
        e.settle();
        e.ctrl_up();
        assert_eq!(e.caret(), e.position_of("Before"), "from {at}");
    }
}

#[test]
fn shift_ctrl_from_a_tables_anchor_selects_as_the_step_goes() {
    let mut e = cell_paragraphs_editor();
    for at in anchor_positions(&e, "alpha") {
        e.handle.select_range(at, at);
        e.settle();
        e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(e.caret(), e.position_of("alpha"), "down from {at}");
        assert_eq!(e.anchor(), at, "the selection keeps its start");

        e.handle.select_range(at, at);
        e.settle();
        e.press(Key::ArrowUp, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(e.caret(), e.position_of("Before"), "up from {at}");
        assert_eq!(e.anchor(), at, "the selection keeps its start");
    }
}

/// [`TABLE`] with a second paragraph in three cells: `aleph` after `alpha`
/// (row 0, column 0), `beth` after `beta` (row 0, column 1) and `gimel` after
/// `gamma` (row 1, column 0). Markdown has no way to write a cell of two
/// paragraphs, so they are added to the document.
fn cell_paragraphs_editor() -> Editor {
    const PAIRS: [(&str, &str); 3] = [("alpha", "aleph"), ("beta", "beth"), ("gamma", "gimel")];
    let doc = TextDocument::new();
    doc.set_markdown(TABLE).unwrap().wait().unwrap();
    for (first, second) in PAIRS {
        add_paragraph_after(&doc, first, second);
    }
    let e = Editor::over(doc);
    for (first, second) in PAIRS {
        let cell_of = |needle: &str| {
            let block = e.doc.block_at_caret(e.position_of(needle)).unwrap();
            let cell = e.doc.block_by_id(block.block_id)?.table_cell()?;
            Some((block.block_id, cell.row, cell.column))
        };
        let (a, b) = (cell_of(first).unwrap(), cell_of(second).unwrap());
        assert_ne!(a.0, b.0, "precondition: two paragraphs");
        assert_eq!(
            (a.1, a.2),
            (b.1, b.2),
            "precondition: {second:?} is in {first:?}'s cell"
        );
    }
    e
}

/// Add a paragraph holding `text` after the cell paragraph that reads
/// `cell_text`, in the same cell.
fn add_paragraph_after(doc: &TextDocument, cell_text: &str, text: &str) {
    let end = doc
        .snapshot_flow()
        .elements
        .iter()
        .find_map(|element| match element {
            teksilo_text::text_document::FlowElementSnapshot::Table(t) => {
                t.cells.iter().find_map(|cell| {
                    let block = cell.blocks.iter().find(|b| b.text == cell_text)?;
                    Some(block.position + block.text.chars().count())
                })
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("a cell holds {cell_text:?}"));
    let cursor = doc.cursor_at(0);
    cursor.set_position(end, MoveMode::MoveAnchor);
    cursor.insert_block().unwrap();
    cursor.insert_text(text).unwrap();
}

#[test]
fn ctrl_down_walks_the_paragraphs_of_a_cell_and_stops_at_its_end() {
    let mut e = cell_paragraphs_editor();
    e.caret_at("alpha", 2);
    e.ctrl_down();
    assert_eq!(
        e.caret(),
        e.position_of("aleph"),
        "the cell's next paragraph"
    );
    let end_of_cell = e.position_of("aleph") + "aleph".len();
    e.ctrl_down();
    assert_eq!(e.caret(), end_of_cell, "the cell's end, not the next cell");
    e.ctrl_down();
    assert_eq!(e.caret(), end_of_cell, "the cell keeps the step");
}

#[test]
fn ctrl_up_walks_back_through_a_cell_and_stops_at_its_start() {
    let mut e = cell_paragraphs_editor();
    e.caret_at("gimel", 3);
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("gimel"));
    e.ctrl_up();
    assert_eq!(
        e.caret(),
        e.position_of("gamma"),
        "the cell's first paragraph"
    );
    e.ctrl_up();
    assert_eq!(e.caret(), e.position_of("gamma"), "the cell keeps the step");
}

#[test]
fn ctrl_down_from_a_tables_anchor_stops_at_a_two_paragraph_cells_start() {
    let mut e = cell_paragraphs_editor();
    for at in anchor_positions(&e, "alpha") {
        e.handle.select_range(at, at);
        e.settle();
        e.ctrl_down();
        assert_eq!(e.caret(), e.position_of("alpha"), "from {at}");
        e.ctrl_down();
        assert_eq!(e.caret(), e.position_of("aleph"), "then the next one");
    }
}

#[test]
fn shift_ctrl_up_inside_a_cell_grows_the_text_selection() {
    // At the start of a cell's second paragraph, with a row above: the step
    // goes to the first paragraph's start, and the selection with it. Taken
    // for the cell's edge, it selected this cell and the one above instead.
    let mut e = cell_paragraphs_editor();
    e.caret_at("gimel", 0);
    let anchor = e.caret();
    e.press(Key::ArrowUp, Modifiers::CTRL | Modifiers::SHIFT);
    assert!(
        !matches!(e.kind(), SelectionKind::Cells(_)),
        "a text selection, not a cell range"
    );
    assert_eq!(e.anchor(), anchor);
    assert_eq!(e.caret(), e.position_of("gamma"));
}

#[test]
fn shift_ctrl_down_inside_a_cell_grows_the_text_selection() {
    // At the end of a cell's first paragraph, with a row below.
    let mut e = cell_paragraphs_editor();
    let end_of_alpha = e.position_of("alpha") + "alpha".len();
    e.handle.select_range(end_of_alpha, end_of_alpha);
    e.settle();
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert!(
        !matches!(e.kind(), SelectionKind::Cells(_)),
        "a text selection, not a cell range"
    );
    assert_eq!(e.anchor(), end_of_alpha);
    assert_eq!(e.caret(), e.position_of("aleph"));
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.caret(), e.position_of("aleph") + "aleph".len());
    assert!(!matches!(e.kind(), SelectionKind::Cells(_)));
}

#[test]
fn shift_ctrl_at_a_cells_edge_still_takes_the_next_cell() {
    // From the end of the cell's last paragraph the step has nowhere to go
    // in the cell, and Shift with an arrow there selects cells, as it did.
    let mut e = cell_paragraphs_editor();
    let end_of_cell = e.position_of("aleph") + "aleph".len();
    e.handle.select_range(end_of_cell, end_of_cell);
    e.settle();
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert!(matches!(e.kind(), SelectionKind::Cells(_)));
}

#[test]
fn ctrl_up_in_a_document_that_starts_with_a_table_stays_in_its_first_cell() {
    let mut e = Editor::markdown("| alpha | beta |\n|---|---|\n| gamma | delta |\n\nAfter.");
    let first = e.position_of("alpha");
    e.caret_at("alpha", 3);
    e.ctrl_up();
    assert_eq!(e.caret(), first);
    e.ctrl_up();
    assert_eq!(e.caret(), first, "nothing comes before the table");
    for at in 0..first {
        e.handle.select_range(at, at);
        e.settle();
        let from = e.caret();
        e.ctrl_up();
        assert_eq!(e.caret(), from, "nothing comes before the table's anchor");
    }
}

// A line break inside a block (Shift+Enter, a LINE SEPARATOR in the text) is
// reported to a screen reader as a line feed, so NVDA ends its paragraph there
// and reads, after a step, only up to the break. The step therefore stops just
// after each break, or the text after it is never read by Ctrl+Up/Down.

const LS: char = teksilo_text::text_document::LINE_SEPARATOR;

/// An editor over `text`, checked to have `blocks` blocks (its line breaks
/// stay inside their block).
fn with_breaks(text: &str, blocks: usize) -> Editor {
    let e = Editor::plain(text);
    assert_eq!(
        e.doc.block_count(),
        blocks,
        "precondition: a line break does not split its block"
    );
    e
}

#[test]
fn ctrl_down_stops_after_a_line_break() {
    // "Verse one" 0..9, the break at 9, "verse two" 10..19, "Next." at 20.
    let text = format!("Verse one{LS}verse two\nNext.");
    for from in [0, 4, 9] {
        let mut e = with_breaks(&text, 2);
        e.handle.select_range(from, from);
        e.settle();
        e.ctrl_down();
        assert_eq!(e.caret(), 10, "from {from}, to the line after the break");
        e.ctrl_down();
        assert_eq!(e.caret(), 20, "then the next paragraph");
    }
}

#[test]
fn ctrl_up_stops_after_a_line_break() {
    let text = format!("Verse one{LS}verse two\nNext.");
    let mut e = with_breaks(&text, 2);
    e.handle.select_range(15, 15);
    e.settle();
    e.ctrl_up();
    assert_eq!(e.caret(), 10, "the start of the line after the break");
    e.ctrl_up();
    assert_eq!(e.caret(), 0, "then the start of the text before it");

    // From the next paragraph's start, up goes to the last line before it,
    // not past it to the block's start.
    e.handle.select_range(20, 20);
    e.settle();
    e.ctrl_up();
    assert_eq!(e.caret(), 10);
    e.ctrl_up();
    assert_eq!(e.caret(), 0);
}

#[test]
fn ctrl_steps_stop_after_each_of_two_line_breaks() {
    // "one" 0..3, a break at 3, "two" 4..7, a break at 7, "three" 8..13,
    // "After." at 14.
    let mut e = with_breaks(&format!("one{LS}two{LS}three\nAfter."), 2);
    e.handle.select_range(1, 1);
    e.settle();
    for stop in [4, 8, 14] {
        e.ctrl_down();
        assert_eq!(e.caret(), stop);
    }
    for stop in [8, 4, 0] {
        e.ctrl_up();
        assert_eq!(e.caret(), stop);
    }
}

#[test]
fn ctrl_steps_stop_on_the_empty_line_between_two_line_breaks() {
    // NVDA reads the empty line between two breaks as a blank paragraph, so
    // the step stops there as at an empty paragraph. "one" 0..3, breaks at 3
    // and 4, "three" 5..10, "After." at 11.
    let mut e = with_breaks(&format!("one{LS}{LS}three\nAfter."), 2);
    e.handle.select_range(0, 0);
    e.settle();
    for stop in [4, 5, 11] {
        e.ctrl_down();
        assert_eq!(e.caret(), stop);
    }
    for stop in [5, 4, 0] {
        e.ctrl_up();
        assert_eq!(e.caret(), stop);
    }
}

#[test]
fn ctrl_steps_stop_after_a_line_break_that_ends_its_block() {
    // "one" 0..3, a break at 3 ending the block at 4, "After." at 5: the
    // empty last line of the block is a stop, read as blank.
    let mut e = with_breaks(&format!("one{LS}\nAfter."), 2);
    e.handle.select_range(1, 1);
    e.settle();
    e.ctrl_down();
    assert_eq!(e.caret(), 4);
    e.ctrl_down();
    assert_eq!(e.caret(), 5);
    e.ctrl_up();
    assert_eq!(e.caret(), 4);
    e.ctrl_up();
    assert_eq!(e.caret(), 0);
}

#[test]
fn ctrl_down_past_the_last_line_break_of_the_document_goes_to_its_end() {
    let mut e = with_breaks(&format!("one{LS}two"), 1);
    e.handle.select_range(0, 0);
    e.settle();
    e.ctrl_down();
    assert_eq!(e.caret(), 4);
    e.ctrl_down();
    assert_eq!(e.caret(), 7, "the end of the last paragraph");
    e.ctrl_down();
    assert_eq!(e.caret(), 7, "nothing follows it");
}

#[test]
fn shift_ctrl_steps_select_to_a_line_break() {
    let text = format!("Verse one{LS}verse two\nNext.");
    let mut e = with_breaks(&text, 2);
    e.handle.select_range(0, 0);
    e.settle();
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.anchor(), 0, "the selection keeps its start");
    assert_eq!(e.caret(), 10, "and grows to the line after the break");
    e.press(Key::ArrowDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.caret(), 20);
    e.press(Key::ArrowUp, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.caret(), 10, "back to the line after the break");
    assert_eq!(e.anchor(), 0);

    e.handle.select_range(15, 15);
    e.settle();
    e.press(Key::ArrowUp, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(e.anchor(), 15);
    assert_eq!(e.caret(), 10, "up to the start of the line after the break");
    assert_eq!(e.handle.selected_text(), "verse");
}

#[test]
fn a_line_break_in_a_table_cell_is_a_stop_and_the_cell_still_keeps_the_step() {
    let doc = TextDocument::new();
    doc.set_markdown(TABLE).unwrap().wait().unwrap();
    // "alpha", a break, then "omega", in the first cell.
    let mut e = Editor::over(doc);
    let end_of_alpha = e.position_of("alpha") + "alpha".len();
    let cursor = e.doc.cursor_at(end_of_alpha);
    cursor.insert_line_break().unwrap();
    cursor.insert_text("omega").unwrap();
    e.settle();
    let alpha = e.position_of("alpha");
    let omega = e.position_of("omega");
    assert_eq!(
        omega,
        end_of_alpha + 1,
        "precondition: one break before omega"
    );
    assert_eq!(
        e.doc.block_at_caret(omega).unwrap().start,
        alpha,
        "precondition: the break stays in the cell's paragraph"
    );

    e.caret_at("alpha", 2);
    e.ctrl_down();
    assert_eq!(e.caret(), omega, "the line after the break");
    let end_of_cell = omega + "omega".len();
    e.ctrl_down();
    assert_eq!(e.caret(), end_of_cell, "the cell's end, not the next cell");
    e.ctrl_down();
    assert_eq!(e.caret(), end_of_cell, "the cell keeps the step");

    e.caret_at("omega", 3);
    e.ctrl_up();
    assert_eq!(e.caret(), omega);
    e.ctrl_up();
    assert_eq!(e.caret(), alpha);
    e.ctrl_up();
    assert_eq!(e.caret(), alpha, "the cell keeps the step");
}
