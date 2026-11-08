// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Up and Down across a table's top and bottom edges.
//!
//! A line step finds the next line through the positions beside the caret's
//! line, and around a table those are not the next line down: the table's own
//! positions sit between the paragraph before it and its first cell, drawn at
//! that paragraph's end, and the position after a cell's last line is the
//! next cell, on the same row. Down from the line above a table, or from a
//! cell of its last row, then probed one caret height on, which with prose's
//! line and paragraph spacing is still the caret's own line: the caret stayed,
//! and a screen reader read the same line again as if the text ended there.
//!
//! Every case runs with no spacing and with prose spacing, and each starts a
//! fresh editor, so no column held from an earlier step decides where the
//! caret lands.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::EditorTypographyDefaults;
use teksilo_text::text_document::{FlowElement, TextDocument};

use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};

const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 400.0;

/// A paragraph long enough to reach above the table's second column, a two
/// by two table, and a paragraph after it.
const TABLE: &str = "Before the table, reaching the second column.\n\n\
                     | alpha | beta |\n\
                     |-------|------|\n\
                     | gamma | delta |\n\n\
                     After the table, reaching the second column too.";

/// Line and paragraph spacing as a writing application sets them for prose.
fn prose() -> EditorTypographyDefaults {
    EditorTypographyDefaults {
        line_height: 1.6,
        paragraph_spacing_before: 6.0,
        paragraph_spacing_after: 12.0,
        ..Default::default()
    }
}

/// No spacing, and prose's.
fn spacings() -> [(&'static str, Option<EditorTypographyDefaults>); 2] {
    [("no spacing", None), ("prose spacing", Some(prose()))]
}

struct Editor {
    doc: TextDocument,
    handle: EditorHandle,
    state: SharedState,
    tree: WidgetTree,
}

impl Editor {
    fn new(markdown: &str, typography: Option<EditorTypographyDefaults>) -> Self {
        let doc = TextDocument::new();
        doc.set_markdown(markdown).unwrap().wait().unwrap();
        Self::over(doc, typography)
    }

    fn over(doc: TextDocument, typography: Option<EditorTypographyDefaults>) -> Self {
        assert!(
            doc.flow()
                .iter()
                .any(|element| matches!(element, FlowElement::Table(_))),
            "precondition: the markdown made a table"
        );
        let mut editor = RichTextEditor::editor(doc.clone());
        if let Some(typography) = typography {
            editor = editor.typography_defaults(typography);
        }
        let handle = editor.handle();
        let state = editor.state_handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
        let _ = tree.render();
        tree.dispatch_event(WidgetEvent::pointer_down(
            Point::new(20.0, 10.0),
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
        self.tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
        let _ = self.tree.render();
    }

    /// The position of `needle`'s first character, found block by block and
    /// in the table's cells, where a position is not a plain-text offset.
    fn position_of(&self, needle: &str) -> usize {
        let mut block = self.doc.block_by_number(0);
        while let Some(b) = block {
            if let Some(i) = b.text().find(needle) {
                return b.position() + b.text()[..i].chars().count();
            }
            block = b.next();
        }
        self.doc
            .flow()
            .into_iter()
            .find_map(|element| match element {
                FlowElement::Table(table) => (0..table.rows()).find_map(|row| {
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

    fn caret_at(&mut self, at: usize) {
        self.handle.select_range(at, at);
        self.settle();
    }

    fn press(&mut self, key: Key) {
        self.press_with(key, Modifiers::NONE);
    }

    fn press_with(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.settle();
    }

    fn caret(&self) -> usize {
        self.handle.cursor_position()
    }

    /// Where the caret is drawn, as `[x, y, width, height]`.
    fn caret_rect(&self) -> [f32; 4] {
        let st = self.state.borrow();
        st.engine.caret_rect(st.shown_caret(), st.cursor_affinity)
    }

    /// The x a caret at `at` is drawn at.
    fn x_of(&self, at: usize) -> f32 {
        let st = self.state.borrow();
        st.engine.caret_rect(at, st.cursor_affinity)[0]
    }

    /// The `(row, column)` of the cell the caret stands in.
    fn caret_cell(&self) -> Option<(usize, usize)> {
        self.state
            .borrow()
            .cursor
            .current_table_cell()
            .map(|cell| (cell.row, cell.column))
    }

    /// The first position from `from` on whose caret is drawn right of `x`.
    fn first_right_of(&self, from: usize, x: f32) -> usize {
        (from..from + 200)
            .find(|&at| self.x_of(at) > x)
            .unwrap_or_else(|| panic!("a position after {from} is drawn right of {x}"))
    }
}

/// The x a caret in the table's second column is drawn right of.
fn second_column_x(e: &Editor) -> f32 {
    e.x_of(e.position_of("beta"))
}

#[test]
fn down_from_the_line_above_a_table_goes_into_its_first_row() {
    for (spacing, typography) in spacings() {
        // From the paragraph's start: the first cell's start.
        let mut e = Editor::new(TABLE, typography.clone());
        e.caret_at(e.position_of("Before"));
        e.press(Key::ArrowDown);
        assert_eq!(
            e.caret_cell(),
            Some((0, 0)),
            "{spacing}: into the first cell"
        );
        assert_eq!(e.caret(), e.position_of("alpha"), "{spacing}");

        // From a column inside "alpha": the character under the caret.
        let mut e = Editor::new(TABLE, typography.clone());
        let from = e.position_of("Before") + 3;
        let x = e.x_of(from);
        e.caret_at(from);
        e.press(Key::ArrowDown);
        assert_eq!(
            e.caret_cell(),
            Some((0, 0)),
            "{spacing}: into the first cell"
        );
        assert!(
            (e.caret_rect()[0] - x).abs() < 10.0,
            "{spacing}: the caret stays in its column (from x {x}, at x {})",
            e.caret_rect()[0]
        );

        // From above the second column: the cell under the caret.
        let mut e = Editor::new(TABLE, typography.clone());
        let from = e.first_right_of(e.position_of("Before"), second_column_x(&e) + 4.0);
        e.caret_at(from);
        e.press(Key::ArrowDown);
        assert_eq!(
            e.caret_cell(),
            Some((0, 1)),
            "{spacing}: into the cell under the caret"
        );

        // From past the end of the first cell's text but still above its
        // column: the end of that cell's text.
        let mut e = Editor::new(TABLE, typography.clone());
        let from = e.position_of("Before") + "Before the table".chars().count();
        assert!(
            e.x_of(from) > e.x_of(e.position_of("alpha") + "alpha".len()) + 10.0
                && e.x_of(from) < second_column_x(&e) - 10.0,
            "precondition: {from} is drawn past alpha's text, above its column"
        );
        e.caret_at(from);
        e.press(Key::ArrowDown);
        assert_eq!(e.caret_cell(), Some((0, 0)), "{spacing}");
        assert_eq!(
            e.caret(),
            e.position_of("alpha") + "alpha".len(),
            "{spacing}: the end of the first cell's text"
        );
    }
}

#[test]
fn up_from_the_first_row_goes_to_the_line_above() {
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(TABLE, typography.clone());
        e.caret_at(e.position_of("alpha") + 2);
        e.press(Key::ArrowUp);
        assert_eq!(e.caret_cell(), None, "{spacing}: out of the table");
        assert!(
            e.caret() < e.position_of("alpha"),
            "{spacing}: on the line above (caret {})",
            e.caret()
        );

        let mut e = Editor::new(TABLE, typography.clone());
        e.caret_at(e.position_of("beta") + 2);
        e.press(Key::ArrowUp);
        assert_eq!(e.caret_cell(), None, "{spacing}: out of the table");
        assert!(e.caret() < e.position_of("alpha"), "{spacing}");
    }
}

#[test]
fn down_from_the_last_row_goes_to_the_paragraph_after() {
    for (spacing, typography) in spacings() {
        for needle in ["gamma", "delta"] {
            let mut e = Editor::new(TABLE, typography.clone());
            let from = e.position_of(needle) + 2;
            let x = e.x_of(from);
            e.caret_at(from);
            e.press(Key::ArrowDown);
            assert_eq!(
                e.caret_cell(),
                None,
                "{spacing}: Down from {needle:?} leaves the table (caret {})",
                e.caret()
            );
            assert!(
                e.caret() >= e.position_of("After"),
                "{spacing}: Down from {needle:?} goes to the paragraph after (caret {})",
                e.caret()
            );
            assert!(
                (e.caret_rect()[0] - x).abs() < 10.0,
                "{spacing}: Down from {needle:?} keeps its column (from x {x}, at x {})",
                e.caret_rect()[0]
            );
        }
    }
}

#[test]
fn up_from_the_paragraph_after_a_table_goes_into_its_last_row() {
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(TABLE, typography.clone());
        e.caret_at(e.position_of("After") + 2);
        e.press(Key::ArrowUp);
        assert_eq!(e.caret_cell(), Some((1, 0)), "{spacing}");

        let mut e = Editor::new(TABLE, typography.clone());
        let from = e.first_right_of(e.position_of("After"), second_column_x(&e) + 4.0);
        e.caret_at(from);
        e.press(Key::ArrowUp);
        assert_eq!(e.caret_cell(), Some((1, 1)), "{spacing}");
    }
}

#[test]
fn down_then_up_across_a_table_comes_back_to_the_line_above() {
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(TABLE, typography.clone());
        let from = e.position_of("Before") + 3;
        e.caret_at(from);
        for expected in [Some((0, 0)), Some((1, 0)), None] {
            e.press(Key::ArrowDown);
            assert_eq!(e.caret_cell(), expected, "{spacing}: going down");
        }
        assert!(e.caret() >= e.position_of("After"), "{spacing}");
        for expected in [Some((1, 0)), Some((0, 0)), None] {
            e.press(Key::ArrowUp);
            assert_eq!(e.caret_cell(), expected, "{spacing}: going up");
        }
        assert_eq!(e.caret(), from, "{spacing}: back where it started");
    }
}

#[test]
fn shift_down_from_the_line_above_a_table_extends_the_selection() {
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(TABLE, typography.clone());
        let from = e.position_of("Before") + 3;
        e.caret_at(from);
        e.press_with(Key::ArrowDown, Modifiers::SHIFT);
        let (anchor, position) = e.handle.selection();
        assert_eq!(anchor, from, "{spacing}: the selection keeps its start");
        assert!(
            position
                > e.position_of("Before") + "Before the table, reaching the second column.".len(),
            "{spacing}: the selection reaches into the table (position {position})"
        );
    }
}

/// What a writer gets from the insert-table command at the end of a chapter:
/// a table of empty cells after the last paragraph, nothing typed after it.
#[test]
fn down_and_up_walk_an_empty_table_inserted_after_the_last_paragraph() {
    for (spacing, typography) in spacings() {
        let doc = TextDocument::new();
        doc.set_plain_text("A first paragraph.\nThe last paragraph, above the table.")
            .unwrap();
        let end = doc.character_count();
        doc.cursor_at(end).insert_table(3, 3).unwrap();
        let mut e = Editor::over(doc, typography);
        let from = e.position_of("The last") + 4;
        e.caret_at(from);
        for row in 0..3 {
            e.press(Key::ArrowDown);
            assert_eq!(
                e.caret_cell(),
                Some((row, 0)),
                "{spacing}: Down to row {row} (caret {})",
                e.caret()
            );
        }
        for row in (0..2).rev() {
            e.press(Key::ArrowUp);
            assert_eq!(e.caret_cell(), Some((row, 0)), "{spacing}: Up to row {row}");
        }
        e.press(Key::ArrowUp);
        assert_eq!(e.caret_cell(), None, "{spacing}: out of the table");
        let line = e.position_of("The last");
        assert!(
            (line..line + 40).contains(&e.caret()),
            "{spacing}: back on the paragraph above (caret {})",
            e.caret()
        );
    }
}
