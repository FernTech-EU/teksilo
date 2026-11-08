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
//! Going up, the position before a row is the end of the row above's last
//! cell, drawn on that row's first line, so Up from a row skipped the lower
//! lines of a wrapped cell above it.
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

    /// The y of the visual line `at` is on, asked away from any wrap
    /// boundary's ambiguity: `at` is a position inside a word.
    fn y_of(&self, at: usize) -> f32 {
        self.state
            .borrow()
            .engine
            .caret_rect(at, teksilo_text::CursorAffinity::Downstream)[1]
    }

    /// The y of each visual line `text` is drawn on, from the first position
    /// of `text` to its last character, top first.
    fn line_ys(&self, text: &str) -> Vec<f32> {
        let start = self.position_of(text);
        let mut ys: Vec<f32> = Vec::new();
        for at in start..start + text.chars().count() {
            let y = self.y_of(at);
            if ys.last().is_none_or(|last| (y - last).abs() > 1.0) {
                ys.push(y);
            }
        }
        ys
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

/// The table of the NVDA run: "premier paragraphe" wraps in its cell on two
/// lines, every other cell holds one.
const WRAPPED_CELL: &str = "jouer, monsieur Stuart.\n\n\
                            | premier paragraphe | deux | trois |\n\
                            |---|---|---|\n\
                            | quatre | cinq | six |\n\
                            | sept | huit | neuf |";

/// Up from a row lands on the last line of a two-line cell above it, not on
/// its first: the step's first guess is the end of the previous cell in
/// document order, the last cell of the row above, drawn on that row's first
/// line.
#[test]
fn up_from_a_row_lands_on_the_last_line_of_a_wrapped_cell_above() {
    for (spacing, typography) in spacings() {
        for column in [0, 6] {
            let mut e = Editor::new(WRAPPED_CELL, typography.clone());
            let lines = e.line_ys("premier paragraphe");
            assert_eq!(lines.len(), 2, "precondition: the cell wraps on two lines");
            let from = e.position_of("quatre") + column;
            let x = e.x_of(from);
            e.caret_at(from);
            e.press(Key::ArrowUp);
            assert_eq!(e.caret_cell(), Some((0, 0)), "{spacing}, column {column}");
            assert!(
                (e.caret_rect()[1] - lines[1]).abs() < 1.0,
                "{spacing}: Up from \"quatre\" column {column} lands on \"paragraphe\" \
                 (caret {} at y {}, lines at {lines:?})",
                e.caret(),
                e.caret_rect()[1]
            );
            assert!(
                (e.caret_rect()[0] - x).abs() < 10.0,
                "{spacing}: column {column} keeps its x (from x {x}, at x {})",
                e.caret_rect()[0]
            );

            // And on up, the cell's first line.
            e.press(Key::ArrowUp);
            assert_eq!(e.caret_cell(), Some((0, 0)), "{spacing}, column {column}");
            assert!(
                (e.caret_rect()[1] - lines[0]).abs() < 1.0,
                "{spacing}: then \"premier\" (caret {})",
                e.caret()
            );
        }
    }
}

/// A paragraph wrapped on two lines before a table: Up from the first row
/// lands on its last line, Down from its first line stays in it.
#[test]
fn up_from_the_first_row_lands_on_the_last_line_of_a_wrapped_paragraph() {
    const WRAPPED: &str = "A paragraph long enough to wrap onto a second line in this \
                           editor, which is four hundred wide, and then some.\n\n\
                           | alpha | beta |\n\
                           |-------|------|\n\
                           | gamma | delta |";
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(WRAPPED, typography.clone());
        let lines = e.line_ys("A paragraph long enough");
        let paragraph = e.line_ys(
            "A paragraph long enough to wrap onto a second line in this editor, which is four \
             hundred wide, and then some.",
        );
        assert_eq!(
            paragraph.len(),
            2,
            "precondition: the paragraph wraps on two lines"
        );
        assert_eq!(lines[0], paragraph[0]);
        for needle in ["alpha", "beta"] {
            let mut e = Editor::new(WRAPPED, typography.clone());
            e.caret_at(e.position_of(needle) + 2);
            e.press(Key::ArrowUp);
            assert_eq!(e.caret_cell(), None, "{spacing}: out of the table");
            assert!(
                (e.caret_rect()[1] - paragraph[1]).abs() < 1.0,
                "{spacing}: Up from {needle:?} lands on the paragraph's last line \
                 (caret {} at y {}, lines at {paragraph:?})",
                e.caret(),
                e.caret_rect()[1]
            );
        }
        e.caret_at(e.position_of("A paragraph") + 3);
        e.press(Key::ArrowDown);
        assert_eq!(e.caret_cell(), None, "{spacing}: still above the table");
        assert!(
            (e.caret_rect()[1] - paragraph[1]).abs() < 1.0,
            "{spacing}: Down from the first line lands on the second (caret {})",
            e.caret()
        );
    }
}

/// Cells of one, two and three lines side by side.
const UNEVEN: &str = "Above the table, a line reaching its third column.\n\n\
                      | alpha bravo charlie delta echo foxtrot golf | hotel india juliet zulu | kilo |\n\
                      |---|---|---|\n\
                      | lima | mike | november |\n\
                      | oscar papa quebec romeo sierra tango | victor | whiskey |\n\n\
                      After the table, a line reaching its third column too.";

const ROW_ONE_FIRST: &str = "alpha bravo charlie delta echo foxtrot golf";
const ROW_ONE_SECOND: &str = "hotel india juliet zulu";
const ROW_THREE_FIRST: &str = "oscar papa quebec romeo sierra tango";

fn uneven_preconditions(e: &Editor) {
    assert_eq!(
        e.line_ys(ROW_ONE_FIRST).len(),
        3,
        "precondition: the first cell holds three lines"
    );
    assert_eq!(
        e.line_ys(ROW_ONE_SECOND).len(),
        2,
        "precondition: the second cell holds two lines"
    );
    assert_eq!(
        e.line_ys(ROW_THREE_FIRST).len(),
        3,
        "precondition: the last row's first cell holds three lines"
    );
}

/// Up from a one-line cell lands on the last line of the taller cell above
/// it, whatever the line count of the row's last cell, the one the step
/// guesses from.
#[test]
fn up_from_a_one_line_cell_lands_on_the_last_line_of_the_cell_above() {
    for (spacing, typography) in spacings() {
        for (from, above, column) in [("lima", ROW_ONE_FIRST, 0), ("mike", ROW_ONE_SECOND, 1)] {
            let mut e = Editor::new(UNEVEN, typography.clone());
            uneven_preconditions(&e);
            let last = *e.line_ys(above).last().unwrap();
            e.caret_at(e.position_of(from) + 2);
            e.press(Key::ArrowUp);
            assert_eq!(
                e.caret_cell(),
                Some((0, column)),
                "{spacing}: from {from:?}"
            );
            assert!(
                (e.caret_rect()[1] - last).abs() < 1.0,
                "{spacing}: Up from {from:?} lands on the last line of {above:?} \
                 (caret {} at y {}, last line at {last})",
                e.caret(),
                e.caret_rect()[1]
            );
        }
    }
}

/// Up from the paragraph after a table lands on the last line of the tall
/// cell of its last row, not on the row's first line, where the row's last
/// cell, the one the step guesses from, is drawn.
#[test]
fn up_from_after_a_table_lands_on_the_last_line_of_a_tall_cell_of_its_last_row() {
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(UNEVEN, typography.clone());
        uneven_preconditions(&e);
        let last = *e.line_ys(ROW_THREE_FIRST).last().unwrap();
        e.caret_at(e.position_of("After") + 2);
        e.press(Key::ArrowUp);
        assert_eq!(
            e.caret_cell(),
            Some((2, 0)),
            "{spacing}: from after the table"
        );
        assert!(
            (e.caret_rect()[1] - last).abs() < 1.0,
            "{spacing}: Up from after the table lands on the last line of the cell above \
             (caret {} at y {}, last line at {last})",
            e.caret(),
            e.caret_rect()[1]
        );
    }
}

/// Down into a row lands on the first line of the cell under the caret, both
/// beside a taller cell and from one.
#[test]
fn down_into_a_row_lands_on_the_first_line_of_the_cell_below() {
    for (spacing, typography) in spacings() {
        // From the line above the table, over each column.
        for (cell, column) in [(ROW_ONE_FIRST, 0), (ROW_ONE_SECOND, 1), ("kilo", 2)] {
            let mut e = Editor::new(UNEVEN, typography.clone());
            uneven_preconditions(&e);
            let first = e.line_ys(cell)[0];
            let from = e.first_right_of(e.position_of("Above"), e.x_of(e.position_of(cell)) + 4.0);
            e.caret_at(from);
            e.press(Key::ArrowDown);
            assert_eq!(
                e.caret_cell(),
                Some((0, column)),
                "{spacing}: into {cell:?}"
            );
            assert!(
                (e.caret_rect()[1] - first).abs() < 1.0,
                "{spacing}: Down lands on the first line of {cell:?} (caret {} at y {}, \
                 first line at {first})",
                e.caret(),
                e.caret_rect()[1]
            );
        }

        // From a one-line cell of the second row into the third, whose first
        // cell is the tall one.
        for (from, below, column) in [("lima", ROW_THREE_FIRST, 0), ("mike", "victor", 1)] {
            let mut e = Editor::new(UNEVEN, typography.clone());
            let first = e.line_ys(below)[0];
            e.caret_at(e.position_of(from) + 2);
            e.press(Key::ArrowDown);
            assert_eq!(
                e.caret_cell(),
                Some((2, column)),
                "{spacing}: from {from:?}"
            );
            assert!(
                (e.caret_rect()[1] - first).abs() < 1.0,
                "{spacing}: Down from {from:?} lands on the first line of {below:?} \
                 (caret {} at y {}, first line at {first})",
                e.caret(),
                e.caret_rect()[1]
            );
        }
    }
}

/// Down then Up through the first column visit every line of every cell, the
/// same lines both ways.
#[test]
fn down_and_up_through_a_column_visit_every_line_of_its_cells() {
    for (spacing, typography) in spacings() {
        let mut e = Editor::new(UNEVEN, typography.clone());
        uneven_preconditions(&e);
        let mut expected: Vec<f32> = e.line_ys(ROW_ONE_FIRST);
        expected.extend(e.line_ys("lima"));
        expected.extend(e.line_ys(ROW_THREE_FIRST));
        e.caret_at(e.position_of("Above") + 2);
        let mut down = Vec::new();
        for _ in 0..expected.len() {
            e.press(Key::ArrowDown);
            assert_eq!(e.caret_cell().map(|(_, c)| c), Some(0), "{spacing}");
            down.push(e.caret_rect()[1]);
        }
        let mut up = vec![e.caret_rect()[1]];
        for _ in 1..expected.len() {
            e.press(Key::ArrowUp);
            assert_eq!(e.caret_cell().map(|(_, c)| c), Some(0), "{spacing}");
            up.push(e.caret_rect()[1]);
        }
        up.reverse();
        assert_eq!(down, expected, "{spacing}: Down visits every line");
        assert_eq!(up, expected, "{spacing}: Up visits every line");
    }
}
