// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tables and selections in a mounted editor.
//!
//! From text-document 1.12.4 a selection with one end in a table's cells and
//! the other outside holds the table whole: the anchor moves to the table's
//! edge, and copy, cut, typing over and a drag take that range. These tests
//! drive the editor through the writer's own gestures (a drag of the pointer,
//! Shift and an arrow, Ctrl+A, Tab, Enter) over documents holding a table, and
//! check the text, the selection and the paint that come out.

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{SelectionKind, TextDocument};

use super::state::{DragState, SharedState};
use super::{EditorHandle, RichTextEditor};

/// A paragraph, a two by two table, and two paragraphs after it.
///
/// Positions: `Alpha beta.` from 0, the table's anchor at 12, its cells `a1`
/// at 14, `b1` at 17, `c1` at 20 and `d1` at 23 (the last cell ends at 25),
/// `Gamma delta.` from 26 and `Omega end.` from 40.
const TABLE_BETWEEN: &str =
    "Alpha beta.\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\nGamma delta.\n\nOmega end.\n";

/// [`TABLE_BETWEEN`] with the table's last cell empty.
const EMPTY_LAST_CELL: &str =
    "Alpha beta.\n\n| a1 | b1 |\n|---|---|\n| c1 |  |\n\nGamma delta.\n\nOmega end.\n";

/// A paragraph, a table four rows tall, and a paragraph after it.
const TALL_TABLE: &str = "Alpha beta.\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n| e1 | f1 |\n\
                          | g1 | h1 |\n\nGamma delta.\n";

const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 300.0;

struct Mounted {
    doc: TextDocument,
    handle: EditorHandle,
    state: SharedState,
    tree: WidgetTree,
    height: f32,
}

impl Mounted {
    fn settle(&mut self) {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(200));
        self.tree.layout(SizeProposal::exact(WIDTH, self.height));
        let _ = self.tree.render();
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
        self.settle();
    }

    /// Where the paragraph reading `text` starts, plus `offset` characters.
    /// A table takes positions of its own, so past one this differs from the
    /// offset in the plain text.
    fn at(&self, text: &str, offset: usize) -> usize {
        (0..self.doc.block_count())
            .filter_map(|n| self.doc.block_by_number(n))
            .find(|block| block.text() == text)
            .map(|block| block.position() + offset)
            .unwrap_or_else(|| panic!("no paragraph reads {text:?}"))
    }

    /// The caret at `at`, with nothing selected.
    fn caret(&mut self, at: usize) {
        self.handle.select_range(at, at);
    }

    /// `(anchor, position)`.
    fn selection(&self) -> (usize, usize) {
        self.handle.selection()
    }

    fn kind(&self) -> SelectionKind {
        self.state.borrow().cursor.selection_kind()
    }

    fn djot(&self) -> String {
        self.doc.to_djot().unwrap_or_default()
    }

    /// The `(row, column)` of the cell the caret stands in.
    fn caret_cell(&self) -> Option<(usize, usize)> {
        self.state
            .borrow()
            .cursor
            .current_table_cell()
            .map(|cell| (cell.row, cell.column))
    }
}

fn mount(markdown: &str) -> Mounted {
    mount_sized(markdown, HEIGHT)
}

/// Mount an editor over `markdown` in a tree `height` tall and give it the
/// keyboard focus. The focusing click moves the caret; a test sets the caret
/// or the selection it needs after this returns.
fn mount_sized(markdown: &str, height: f32) -> Mounted {
    let doc = TextDocument::new();
    doc.set_markdown(markdown).unwrap().wait().unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let state = editor.state_handle();
    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(WIDTH, height));
    let _ = tree.render();
    let corner = Point::new(20.0, 10.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        corner,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        corner,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    let focused = tree.focused();
    assert!(
        focused.is_some_and(|f| f == id || tree.is_descendant_of(f, id)),
        "the click must focus the editor"
    );
    let mut mounted = Mounted {
        doc,
        handle,
        state,
        tree,
        height,
    };
    mounted.settle();
    mounted
}

/// The window point in the middle of the character at `at`.
fn middle_of(handle: &EditorHandle, at: usize) -> Point {
    let rect = handle
        .range_rect(at, at + 1)
        .expect("the character is laid out");
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// The window point a caret at `at` is drawn on.
fn caret_point(handle: &EditorHandle, at: usize) -> Point {
    let rect = handle.offset_rect(at).expect("the caret is laid out");
    Point::new(rect.x + 0.5, rect.y + rect.height / 2.0)
}

/// Press on the character at `grab`, which must be in the selection, and
/// travel past the mouse's drag slop: the drag of the selection is under way.
fn pick_up_selection(m: &mut Mounted, grab: usize) {
    let press = middle_of(&m.handle, grab);
    m.tree.dispatch_event(WidgetEvent::pointer_down(
        press,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert!(
        matches!(
            m.state.borrow().drag_state,
            DragState::PendingTextDrag { .. }
        ),
        "the press on {grab} must land in the selection {:?} and arm a drag of it",
        m.selection()
    );
    m.tree.dispatch_event(WidgetEvent::pointer_move(Point::new(
        press.x + 12.0,
        press.y,
    )));
}

/// Release the drag over the caret position `drop_at`, hovering there first.
fn release_over(m: &mut Mounted, drop_at: usize) {
    let target = caret_point(&m.handle, drop_at);
    m.tree.dispatch_event(WidgetEvent::pointer_move(target));
    m.tree.dispatch_event(WidgetEvent::pointer_up(
        target,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    m.settle();
}

/// The whole pointer sequence of a writer dragging the selection from the
/// character at `grab` to the caret position `drop_at`, for a drop the editor
/// accepts. Checks the hovering drag aimed at `drop_at`, so a text landing
/// anywhere else is the drop's doing and not the aim's.
fn drag_selection(m: &mut Mounted, grab: usize, drop_at: usize) {
    pick_up_selection(m, grab);
    let target = caret_point(&m.handle, drop_at);
    m.tree.dispatch_event(WidgetEvent::pointer_move(target));
    assert!(
        m.state.borrow().drop_caret,
        "the drag must be live over the editor"
    );
    assert_eq!(
        m.state.borrow().cursor.position(),
        drop_at,
        "the hovering drag must aim at {drop_at}"
    );
    release_over(m, drop_at);
}

// ── Dragging a selection within its own editor ───────────────────────────

#[test]
fn a_word_dropped_inside_a_word_lands_between_its_letters() {
    let mut m = mount(TABLE_BETWEEN);
    let beta = m.at("Alpha beta.", 6);
    m.handle.select_range(beta, beta + 4);
    let drop_at = m.at("Gamma delta.", 8);
    drag_selection(&mut m, beta, drop_at);
    assert_eq!(
        m.djot(),
        "Alpha .\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\nGamma debetalta.\n\nOmega end."
    );
}

#[test]
fn a_word_dragged_back_over_a_table_lands_at_the_drop_point() {
    let mut m = mount(TABLE_BETWEEN);
    let delta = m.at("Gamma delta.", 6);
    m.handle.select_range(delta, delta + 5);
    let drop_at = m.at("Alpha beta.", 2);
    drag_selection(&mut m, delta, drop_at);
    assert_eq!(
        m.djot(),
        "Aldeltapha beta.\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\nGamma .\n\nOmega end."
    );
}

/// The selection runs from inside the paragraph before the table to the
/// start of the one after it, so it holds the table whole and leaves the two
/// paragraphs apart: its removal takes one position less than its extent. The
/// drop point was re-based by the extent, and the passage landed one character
/// early, splitting the word it was dropped before.
#[test]
fn a_selection_holding_a_table_dragged_forward_lands_at_the_drop_point() {
    let mut m = mount(TABLE_BETWEEN);
    let from = m.at("Alpha beta.", 6);
    let to = m.at("Gamma delta.", 0);
    m.handle.select_range(from, to);
    assert!(
        matches!(m.kind(), SelectionKind::Mixed { .. }),
        "setup: the selection must hold the table, not {:?}",
        m.kind()
    );
    let drop_at = m.at("Gamma delta.", 6);
    drag_selection(&mut m, from, drop_at);
    assert_eq!(
        m.djot(),
        "Alpha {}\n\nGamma beta.\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\ndelta.\n\nOmega end.",
        "the passage must land in front of `delta`, table and all, once"
    );
}

/// The same selection dropped at the very end of a paragraph: one character
/// early, the paragraph's full stop was left in a paragraph of its own after
/// the table.
#[test]
fn a_selection_holding_a_table_dropped_at_a_paragraph_end_keeps_its_full_stop() {
    let mut m = mount(TABLE_BETWEEN);
    let from = m.at("Alpha beta.", 6);
    let to = m.at("Gamma delta.", 0);
    m.handle.select_range(from, to);
    let drop_at = m.at("Omega end.", 10);
    drag_selection(&mut m, from, drop_at);
    assert_eq!(
        m.djot(),
        "Alpha {}\n\nGamma delta.\n\nOmega end.beta.\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |"
    );
}

/// Taken from the end of one paragraph to the start of the next, the
/// selection is the table and nothing else.
#[test]
fn a_table_alone_dragged_forward_lands_at_the_drop_point() {
    let mut m = mount(TABLE_BETWEEN);
    let from = m.at("Alpha beta.", 11);
    let to = m.at("Gamma delta.", 0);
    m.handle.select_range(from, to);
    let drop_at = m.at("Omega end.", 5);
    let grab = m.at("a1", 0);
    drag_selection(&mut m, grab, drop_at);
    assert_eq!(
        m.djot(),
        "Alpha beta.\n\nGamma delta.\n\nOmega\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\n{} end.",
        "the table must move whole in front of ` end.`"
    );
}

/// Started in the paragraph after the table and taken back into a cell, the
/// selection holds the table from its anchor. Dropped before it, nothing needs
/// re-basing, and the table must still move whole, once.
#[test]
fn a_selection_holding_a_table_dragged_backward_lands_at_the_drop_point() {
    let mut m = mount(TABLE_BETWEEN);
    let from = m.at("Gamma delta.", 5);
    let into = m.at("b1", 1);
    m.handle.select_range(from, into);
    assert_eq!(
        m.selection(),
        (from, 12),
        "setup: the moving end must stop at the table's anchor"
    );
    let drop_at = m.at("Alpha beta.", 6);
    let grab = m.at("a1", 0);
    drag_selection(&mut m, grab, drop_at);
    assert_eq!(
        m.djot(),
        "Alpha {}\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\nGammabeta.\n\n{} delta.\n\nOmega end."
    );
}

#[test]
fn text_selected_in_one_cell_moves_out_of_it() {
    let mut m = mount(TABLE_BETWEEN);
    let a1 = m.at("a1", 0);
    m.handle.select_range(a1, a1 + 2);
    let drop_at = m.at("Gamma delta.", 6);
    drag_selection(&mut m, a1, drop_at);
    assert_eq!(
        m.djot(),
        "Alpha beta.\n\n|  | b1 |\n|---|---|\n| c1 | d1 |\n\nGamma a1delta.\n\nOmega end."
    );
}

/// A move is one edit: one Undo takes the passage away from where it landed
/// and puts it back where it was.
#[test]
fn one_undo_takes_back_a_move() {
    let mut m = mount(TABLE_BETWEEN);
    let before = m.djot();
    let from = m.at("Alpha beta.", 6);
    let to = m.at("Gamma delta.", 3);
    m.handle.select_range(from, to);
    let drop_at = m.at("Omega end.", 6);
    drag_selection(&mut m, from, drop_at);
    assert_ne!(m.djot(), before, "setup: the drag must move the passage");
    m.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(
        m.djot(),
        before,
        "one undo must restore the text exactly as it was before the drag"
    );
}

// ── Dragging a cell selection ───────────────────────────────────────────

/// A selection from one cell to another is a rectangle of cells. Its copy is
/// a table of those cells, but removing its range only empties the cells, so
/// a move put a copy of the table at the drop point and left the emptied grid
/// where it was. The editor has no modifier that turns a drag into a copy, so
/// the drop does nothing, and the cells stay selected.
#[test]
fn a_cell_selection_dropped_in_its_own_editor_changes_nothing() {
    let mut m = mount(TABLE_BETWEEN);
    let before = m.djot();
    let from = m.at("a1", 1);
    let to = m.at("d1", 1);
    m.handle.select_range(from, to);
    assert!(
        matches!(m.kind(), SelectionKind::Cells(_)),
        "setup: the selection must be a cell selection, not {:?}",
        m.kind()
    );
    let drop_at = m.at("Omega end.", 5);
    pick_up_selection(&mut m, from);
    release_over(&mut m, drop_at);
    assert_eq!(m.djot(), before, "the drop must leave the text as it was");
    assert!(
        matches!(m.kind(), SelectionKind::Cells(_)),
        "the cells must stay selected, not {:?}",
        m.kind()
    );
    assert!(
        !m.state.borrow().drop_caret,
        "no caret may promise a landing place the drop refuses"
    );
}

// ── Shift and an arrow over a selection holding a table ──────────────────

/// A selection from inside `beta.` to inside `Gamma`, the table whole between.
fn select_across_the_table(m: &mut Mounted) -> (usize, usize) {
    let from = m.at("Alpha beta.", 6);
    let to = m.at("Gamma delta.", 4);
    m.handle.select_range(from, to);
    assert!(
        matches!(m.kind(), SelectionKind::Mixed { .. }),
        "setup: the selection must hold the table, not {:?}",
        m.kind()
    );
    (from, to)
}

/// Once a selection held a whole table, every Shift and arrow turned it into
/// a range of the table's cells, and the text on either side was dropped from
/// it.
#[test]
fn shift_left_and_right_move_the_end_of_a_selection_holding_a_table() {
    let mut m = mount(TABLE_BETWEEN);
    let (from, to) = select_across_the_table(&mut m);

    m.press(Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(m.selection(), (from, to - 1));
    assert!(
        matches!(m.kind(), SelectionKind::Mixed { .. }),
        "Shift+Left must keep the text and the table, not {:?}",
        m.kind()
    );

    m.press(Key::ArrowRight, Modifiers::SHIFT);
    m.press(Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(m.selection(), (from, to + 1));
    assert!(
        matches!(m.kind(), SelectionKind::Mixed { .. }),
        "Shift+Right must keep the text and the table, not {:?}",
        m.kind()
    );
}

#[test]
fn shift_up_and_down_keep_the_text_of_a_selection_holding_a_table() {
    let mut m = mount(TABLE_BETWEEN);
    let (from, _) = select_across_the_table(&mut m);
    for (key, name) in [
        (Key::ArrowDown, "Shift+Down"),
        (Key::ArrowUp, "Shift+Up"),
        (Key::ArrowUp, "a second Shift+Up"),
        (Key::ArrowUp, "a third Shift+Up"),
    ] {
        m.press(key, Modifiers::SHIFT);
        assert_eq!(
            m.selection().0,
            from,
            "{name} must leave the selection's start where it was"
        );
        assert!(
            !matches!(m.kind(), SelectionKind::Cells(_)),
            "{name} must not turn the selection into a range of cells"
        );
    }
}

/// With the table's last cell empty, the moving end there stands at the
/// start of a cell as well as at its end, which is where a cell range starts
/// from. A selection holding the table must still move as text.
#[test]
fn shift_arrows_from_an_empty_last_cell_keep_the_text_of_the_selection() {
    for (key, name) in [(Key::ArrowLeft, "Shift+Left"), (Key::ArrowUp, "Shift+Up")] {
        let mut m = mount(EMPTY_LAST_CELL);
        let from = m.at("Alpha beta.", 6);
        m.handle.select_range(from, m.at("c1", 1));
        assert!(
            matches!(m.kind(), SelectionKind::Mixed { .. }),
            "setup: the selection must hold the table, not {:?}",
            m.kind()
        );
        m.press(key, Modifiers::SHIFT);
        assert_eq!(
            m.selection().0,
            from,
            "{name} must leave the selection's start where it was"
        );
        assert!(
            !matches!(m.kind(), SelectionKind::Cells(_)),
            "{name} must not turn the selection into a range of cells"
        );
    }
}

// ── Shift steps back into a table the selection holds ────────────────────

/// Shift+Down from the paragraph before the table takes the table whole.
/// Shift+Up then has to give it back: the step lands in the table, which
/// moved it to the table's far edge again, where it already was, so the
/// selection could only grow.
#[test]
fn shift_up_gives_back_a_table_shift_down_took() {
    let mut m = mount(TABLE_BETWEEN);
    let start = m.at("Alpha beta.", 3);
    m.caret(start);
    m.press(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, m.at("d1", 2)),
        "setup: Shift+Down must take the table to the end of its last cell"
    );
    m.press(Key::ArrowUp, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, m.at("Alpha beta.", 11)),
        "Shift+Up must give the table back, the moving end before it"
    );
    assert!(matches!(m.kind(), SelectionKind::Text), "{:?}", m.kind());
}

#[test]
fn shift_down_gives_back_a_table_shift_up_took() {
    let mut m = mount(TABLE_BETWEEN);
    let start = m.at("Gamma delta.", 3);
    m.caret(start);
    m.press(Key::ArrowUp, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, 12),
        "setup: Shift+Up must take the table to its anchor"
    );
    m.press(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, m.at("Gamma delta.", 0)),
        "Shift+Down must give the table back, the moving end after it"
    );
    assert!(matches!(m.kind(), SelectionKind::Text), "{:?}", m.kind());
}

/// The same with a page step. The editor is 70 pixels tall, so a page is about
/// two of the table's four rows, and the step back from the end of its last
/// cell lands inside it.
#[test]
fn shift_page_up_gives_back_a_table_shift_page_down_took() {
    let mut m = mount_sized(TALL_TABLE, 70.0);
    let start = m.at("Alpha beta.", 3);
    m.caret(start);
    m.press(Key::PageDown, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, m.at("h1", 2)),
        "setup: Shift+PageDown must take the table to the end of its last cell"
    );
    m.press(Key::PageUp, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, m.at("Alpha beta.", 11)),
        "Shift+PageUp must give the table back, the moving end before it"
    );
}

/// Shift+Home from the end of the table's last cell goes to the start of
/// that cell's line, in the table the selection holds.
#[test]
fn shift_home_gives_back_a_table_the_selection_holds() {
    let mut m = mount(TABLE_BETWEEN);
    let start = m.at("Alpha beta.", 3);
    m.caret(start);
    m.press(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(m.selection(), (start, m.at("d1", 2)), "setup");
    m.press(Key::Home, Modifiers::SHIFT);
    assert_eq!(
        m.selection(),
        (start, m.at("Alpha beta.", 11)),
        "Shift+Home must give the table back, the moving end before it"
    );
}

// ── Painting a cell selection ────────────────────────────────────────────

/// The rectangles the last paint filled in the selection colour.
fn selection_paint(m: &mut Mounted) -> Vec<Rect> {
    let frame = m.tree.render();
    let color = m
        .state
        .borrow()
        .last_selection_color
        .expect("the editor has painted with a selection colour");
    frame
        .decorations
        .iter()
        .filter(|decoration| {
            decoration
                .color
                .iter()
                .zip(color)
                .all(|(a, b)| (a - b).abs() < 1e-4)
        })
        .map(|decoration| {
            Rect::new(
                decoration.rect[0],
                decoration.rect[1],
                decoration.rect[2],
                decoration.rect[3],
            )
        })
        .collect()
}

fn painted(paint: &[Rect], point: Point) -> bool {
    paint.iter().any(|rect| rect.contains(point))
}

/// A point inside the cell holding `text`, in the cell's padding: to the right
/// of the text and just above its line. Painted only when the whole cell is,
/// never by the highlight of the cell's text.
fn in_the_cell_beside(m: &Mounted, text: &str) -> Point {
    let start = m.at(text, 0);
    let rect = m
        .handle
        .range_rect(start, start + text.chars().count())
        .expect("the cell's text is laid out");
    Point::new(rect.x + rect.width + 30.0, rect.y - 2.0)
}

/// Ctrl+A in a cell selects the cell's paragraph, then the cell, then the
/// table. The last two are cell selections the paint never received, so they
/// looked exactly like the first.
#[test]
fn ctrl_a_in_a_cell_paints_the_cell_then_the_table() {
    let mut m = mount(TABLE_BETWEEN);
    m.caret(m.at("a1", 1));

    m.press(Key::A, Modifiers::COMMAND);
    m.press(Key::A, Modifiers::COMMAND);
    let cell = selection_paint(&mut m);
    assert!(
        painted(&cell, in_the_cell_beside(&m, "a1")),
        "the second Ctrl+A must paint the whole cell: {cell:?}"
    );
    assert!(
        !painted(&cell, middle_of(&m.handle, m.at("b1", 0))),
        "and only that cell: {cell:?}"
    );

    m.press(Key::A, Modifiers::COMMAND);
    let table = selection_paint(&mut m);
    for text in ["a1", "b1", "c1", "d1"] {
        assert!(
            painted(&table, in_the_cell_beside(&m, text)),
            "the third Ctrl+A must paint the cell holding {text}: {table:?}"
        );
    }
}

/// A selection holding a table paints the table's cells whole, as the cell
/// selection it holds, and the text on either side as text.
#[test]
fn a_selection_holding_a_table_paints_its_cells_whole() {
    let mut m = mount(TABLE_BETWEEN);
    select_across_the_table(&mut m);
    let paint = selection_paint(&mut m);
    for text in ["a1", "b1", "c1", "d1"] {
        assert!(
            painted(&paint, in_the_cell_beside(&m, text)),
            "the cell holding {text} must be painted whole: {paint:?}"
        );
    }
    let beta = m.at("Alpha beta.", 7);
    assert!(
        painted(&paint, middle_of(&m.handle, beta)),
        "the text before the table must be painted: {paint:?}"
    );
    let gamma = m.at("Gamma delta.", 1);
    assert!(
        painted(&paint, middle_of(&m.handle, gamma)),
        "the text after the table must be painted: {paint:?}"
    );
}

/// Two tables with a paragraph between them, and a paragraph on either side.
const TWO_TABLES: &str = "Alpha beta.\n\n| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\nMiddle text.\n\n\
                          | p1 | q1 |\n|---|---|\n| r1 | s1 |\n\nOmega end.\n";

/// Every table a selection holds is painted cell by cell, not only the first:
/// the second was painted a line at a time, its highlight drawn on over the
/// next column.
#[test]
fn a_selection_holding_two_tables_paints_the_cells_of_both_whole() {
    let mut m = mount(TWO_TABLES);
    let from = m.at("Alpha beta.", 6);
    let to = m.at("Omega end.", 3);
    m.handle.select_range(from, to);
    let paint = selection_paint(&mut m);
    for text in ["a1", "b1", "c1", "d1", "p1", "q1", "r1", "s1"] {
        assert!(
            painted(&paint, in_the_cell_beside(&m, text)),
            "the cell holding {text} must be painted whole: {paint:?}"
        );
    }
}

/// The same for the whole text, where Ctrl+A outside a table selects it all.
#[test]
fn ctrl_a_paints_the_cells_of_every_table_whole() {
    let mut m = mount(TWO_TABLES);
    m.caret(m.at("Middle text.", 2));
    m.press(Key::A, Modifiers::COMMAND);
    let paint = selection_paint(&mut m);
    for text in ["a1", "b1", "c1", "d1", "p1", "q1", "r1", "s1"] {
        assert!(
            painted(&paint, in_the_cell_beside(&m, text)),
            "the cell holding {text} must be painted whole: {paint:?}"
        );
    }
}

/// Started in a cell of the second table and taken back into the first, the
/// selection holds both tables whole, from one table's edge to the other's,
/// which the document reads as text rather than as a table and some text.
#[test]
fn a_selection_from_one_table_into_another_paints_the_cells_of_both_whole() {
    let mut m = mount(TWO_TABLES);
    m.handle.select_range(m.at("q1", 1), m.at("b1", 1));
    assert!(
        matches!(m.kind(), SelectionKind::Text),
        "setup: the document reads the selection as text, not {:?}",
        m.kind()
    );
    let paint = selection_paint(&mut m);
    for text in ["a1", "b1", "c1", "d1", "p1", "q1", "r1", "s1"] {
        assert!(
            painted(&paint, in_the_cell_beside(&m, text)),
            "the cell holding {text} must be painted whole: {paint:?}"
        );
    }
}

/// A selection that stops holding a table stops painting its cells, and a
/// rectangle of cells replaced by another one is painted as the new one,
/// although neither end of the cursor moved: the paint keeps what it read
/// until the selection changes, and must see that it did.
#[test]
fn the_painted_cells_follow_the_selection_as_it_changes() {
    let mut m = mount(TABLE_BETWEEN);
    let (from, _) = select_across_the_table(&mut m);
    let held = selection_paint(&mut m);
    assert!(
        painted(&held, in_the_cell_beside(&m, "a1")),
        "setup: {held:?}"
    );

    m.handle.select_range(from, m.at("Alpha beta.", 10));
    let text = selection_paint(&mut m);
    assert!(
        !painted(&text, in_the_cell_beside(&m, "a1")),
        "the table is not held any more: {text:?}"
    );

    m.caret(m.at("a1", 1));
    m.press(Key::A, Modifiers::COMMAND);
    m.press(Key::A, Modifiers::COMMAND);
    let cell = selection_paint(&mut m);
    assert!(
        painted(&cell, in_the_cell_beside(&m, "a1")),
        "the second Ctrl+A leaves both ends where the first did, and paints the cell: {cell:?}"
    );
    assert!(
        !painted(&cell, in_the_cell_beside(&m, "b1")),
        "and only that cell: {cell:?}"
    );
    m.press(Key::A, Modifiers::COMMAND);
    let table = selection_paint(&mut m);
    assert!(
        painted(&table, in_the_cell_beside(&m, "b1")),
        "the third Ctrl+A leaves both ends where the second did, and paints the table: {table:?}"
    );

    // Selected again from end to end, the rectangle is gone although the
    // cursor's two ends are the ones it had.
    let (anchor, position) = m.selection();
    m.handle.select_range(anchor, position);
    let text = selection_paint(&mut m);
    assert!(
        !painted(&text, in_the_cell_beside(&m, "b1")),
        "the rectangle of cells must not outlive the selection that made it: {text:?}"
    );
}

// ── A table in a quotation ───────────────────────────────────────────────

/// A quotation holding a paragraph and a table, then a paragraph outside it.
const QUOTED_TABLE: &str = "> Intro\n>\n> | a1 | b1 |\n> |---|---|\n> | c1 | d1 |\n\nAfter\n";

/// The same, two quotations deep.
const NESTED_QUOTED_TABLE: &str =
    ">> Intro\n>>\n>> | a1 | b1 |\n>> |---|---|\n>> | c1 | d1 |\n\nAfter\n";

/// A table in a quotation with a quoted paragraph after it.
const QUOTED_TABLE_THEN_QUOTE: &str =
    "> | a1 | b1 |\n> |---|---|\n> | c1 | d1 |\n>\n> Quoted after\n\nAfter\n";

/// Tab, Shift+Tab and Enter looked the table up in the main text's own flow
/// only, found nothing in a quotation, and did nothing.
#[test]
fn tab_and_shift_tab_move_between_the_cells_of_a_quoted_table() {
    for markdown in [QUOTED_TABLE, NESTED_QUOTED_TABLE] {
        let mut m = mount(markdown);
        m.caret(m.at("a1", 1));
        m.press(Key::Tab, Modifiers::NONE);
        assert_eq!(
            m.selection(),
            (m.at("b1", 0), m.at("b1", 0)),
            "Tab must reach the next cell in {markdown:?}"
        );
        m.press(Key::Tab, Modifiers::SHIFT);
        assert_eq!(
            m.selection(),
            (m.at("a1", 0), m.at("a1", 0)),
            "Shift+Tab must reach the previous cell in {markdown:?}"
        );
    }
}

#[test]
fn tab_in_the_last_cell_of_a_quoted_table_opens_a_row_and_enters_it() {
    for markdown in [QUOTED_TABLE, NESTED_QUOTED_TABLE] {
        let mut m = mount(markdown);
        m.caret(m.at("d1", 2));
        m.press(Key::Tab, Modifiers::NONE);
        assert_eq!(
            m.caret_cell(),
            Some((2, 0)),
            "Tab must enter the new row's first cell in {markdown:?}"
        );
    }
}

#[test]
fn enter_on_the_last_row_of_a_quoted_table_steps_out_of_it() {
    for (markdown, next) in [
        (QUOTED_TABLE, "After"),
        (NESTED_QUOTED_TABLE, "After"),
        (QUOTED_TABLE_THEN_QUOTE, "Quoted after"),
    ] {
        let mut m = mount(markdown);
        let before = m.djot();
        m.caret(m.at("c1", 1));
        m.press(Key::Enter, Modifiers::NONE);
        assert_eq!(
            m.selection(),
            (m.at(next, 0), m.at(next, 0)),
            "Enter must reach {next:?} in {markdown:?}"
        );
        assert_eq!(m.djot(), before, "Enter in a table must add no paragraph");
    }
}

#[test]
fn enter_moves_down_a_column_of_a_quoted_table() {
    for markdown in [QUOTED_TABLE, NESTED_QUOTED_TABLE] {
        let mut m = mount(markdown);
        m.caret(m.at("b1", 1));
        m.press(Key::Enter, Modifiers::NONE);
        assert_eq!(
            m.selection(),
            (m.at("d1", 0), m.at("d1", 0)),
            "Enter must reach the cell below in {markdown:?}"
        );
    }
}
