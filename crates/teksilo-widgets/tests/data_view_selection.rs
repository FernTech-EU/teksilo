// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! When a data-view row commits the selection its press decided.
//!
//! A **precise** pointer commits on press: that is the click convention on
//! every desktop, and this file's mouse half is the no-regression statement for
//! it — every assertion here is what the suite asserted before the touch
//! programme touched `data_views::deferred_select`.
//!
//! A **direct** pointer (a finger, a pen) commits on the **release**. Its press
//! is not yet a click — the same contact is the opening sample of a scroll, and
//! a scrolling finger has to leave the selection exactly as it found it. That
//! cannot be arranged by a release-time predicate: a selection written on
//! `PointerDown` is already written by the time any release is dispatched. So
//! the write itself moves to the release, and a release the row has lost (a won
//! `PanClaim`, a press that wandered off) is refused.
//!
//! Four views share `deferred_select` — `ListView`, `TreeView`, `TableView`,
//! `TreeTableView` — and `GridView` shares it for its tiles. `TableView`'s
//! *cell* selection is a fifth commit site with its own coordinate pair and its
//! own model, so it carries the same rule separately and is tested separately.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::Modifiers;
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel};
use teksilo_i18n::lit;
use teksilo_widgets::ListView;
use teksilo_widgets::primitives::TextWidget;
use teksilo_widgets::table_view::{
    CellSelectionModel, Column, ColumnWidth, TableSelectionMode, TableView,
};

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

const VIEWPORT: f32 = 400.0;
const ROW: f32 = 40.0;
const ROWS: usize = 200;

fn tree_with(root: impl teksilo_core::widget::Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(root);
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    (tree, id)
}

fn finger() -> PointerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PointerIdAllocator::global().begin(
        BackendDeviceKey::new(0x5CB1),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

fn touch(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::NONE,
        coalesced: Vec::new(),
    }
}

fn mouse(phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample::mouse(phase, at, EventTime::from_millis(ms))
}

/// A finger sample carrying the platform accelerator — the touch form of an
/// accelerator-click, which is how a touch device with a keyboard attached adds
/// one row to a discontiguous selection.
fn touch_accel(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample {
        modifiers: Modifiers::COMMAND,
        ..touch(id, phase, at, ms)
    }
}

fn pan_slop() -> f32 {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch)
        .pan_slop
        .expect("a touch profile pans")
}

/// The centre of row `i` at scroll 0.
fn row_centre(i: usize) -> Point {
    Point::new(VIEWPORT / 2.0, i as f32 * ROW + ROW / 2.0)
}

/// A full mouse click on `at`: press and release, no movement.
fn mouse_click(tree: &mut WidgetTree, at: Point) {
    tree.dispatch_pointer(mouse(PointerPhase::Down, at, 0));
    tree.dispatch_pointer(mouse(PointerPhase::Up, at, 8));
}

/// A finger down at `at`, with no movement and no lift.
fn finger_down(tree: &mut WidgetTree, at: Point) -> PointerId {
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
    id
}

/// A finger tap on `at`: down then up in place.
fn finger_tap(tree: &mut WidgetTree, at: Point) {
    let id = finger_down(tree, at);
    tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 8));
}

/// A finger pressing at `from` and dragging `up` logical pixels upward, past
/// the pan slop first so the claim is taken, then lifting.
fn finger_pan(tree: &mut WidgetTree, from: Point, up: f32) {
    let id = finger();
    tree.dispatch_pointer(touch(id, PointerPhase::Down, from, 0));
    let armed = Point::new(from.x, from.y - pan_slop() - 1.0);
    tree.dispatch_pointer(touch(id, PointerPhase::Move, armed, 16));
    let end = Point::new(from.x, armed.y - up);
    tree.dispatch_pointer(touch(id, PointerPhase::Move, end, 32));
    tree.dispatch_pointer(touch(id, PointerPhase::Up, end, 48));
}

fn selected(sel: &SelectionModel) -> Vec<usize> {
    sel.selected_indices()
}

/// A `ListView` in `Multi` mode with an activation hook.
///
/// The hook was once believed load-bearing — the thing that gives a row its tap
/// recognizer, hence the implicit pointer capture, hence the deferred pane
/// rebuild that keeps the pressed row's own handler alive to receive the
/// release. It is not: `plain_list` below is the same fixture without it and
/// carries the same three assertions, all passing. The hook survives here
/// because a list *with* an activation hook is a configuration worth covering,
/// not because the assertions need it.
fn list(sel: &SelectionModel) -> ListView<usize> {
    ListView::new(ListModel::from_vec((0..ROWS).collect()), |i, _item, _s| {
        Box::new(TextWidget::new(lit!(i.to_string())))
    })
    .item_height(ROW)
    .on_activate(|_i, _ctx| {})
    .selection(sel.clone())
}

// ---------------------------------------------------------------------------
// the mouse half — no regression
// ---------------------------------------------------------------------------

#[test]
fn a_mouse_press_on_a_list_row_selects_it_before_the_release() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    let (mut tree, _id) = tree_with(list(&sel));
    tree.dispatch_pointer(mouse(PointerPhase::Down, row_centre(5), 0));
    assert_eq!(
        selected(&sel),
        vec![5],
        "a mouse still selects on press — the desktop click convention",
    );
}

#[test]
fn a_mouse_click_on_a_list_row_selects_it() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    let (mut tree, _id) = tree_with(list(&sel));
    mouse_click(&mut tree, row_centre(5));
    assert_eq!(selected(&sel), vec![5]);
}

/// The pre-existing deferral, untouched: a mouse press on an already-selected
/// row keeps the whole set alive so it can be dragged, and collapses to the
/// pressed row on a release without a drag.
#[test]
fn a_mouse_press_on_a_selected_list_row_still_defers_the_collapse() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![2, 5, 9], false);
    let (mut tree, _id) = tree_with(list(&sel));

    tree.dispatch_pointer(mouse(PointerPhase::Down, row_centre(5), 0));
    assert_eq!(
        selected(&sel),
        vec![2, 5, 9],
        "the press must keep the set so a drag can carry all of it",
    );
    tree.dispatch_pointer(mouse(PointerPhase::Up, row_centre(5), 8));
    assert_eq!(selected(&sel), vec![5], "the release collapses it");
}

// ---------------------------------------------------------------------------
// the direct-pointer half
// ---------------------------------------------------------------------------

#[test]
fn a_finger_press_on_a_list_row_selects_nothing_yet() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![0], false);
    let (mut tree, _id) = tree_with(list(&sel));

    finger_down(&mut tree, row_centre(5));
    assert_eq!(
        selected(&sel),
        vec![0],
        "a finger's press is not yet a click — it may still become a scroll",
    );
}

#[test]
fn a_finger_tap_on_a_list_row_selects_it() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![0], false);
    let (mut tree, _id) = tree_with(list(&sel));

    finger_tap(&mut tree, row_centre(5));
    assert_eq!(selected(&sel), vec![5], "the release is the click");
}

/// The measurement this package inherited: a finger pan starting on an
/// **unselected** row used to leave the selection on that row. It is the half
/// of the pan story `release_completes_the_press` structurally cannot reach,
/// because the write happened on `PointerDown`.
#[test]
fn a_finger_pan_from_an_unselected_list_row_leaves_the_selection_alone() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![0], false);
    let view = list(&sel);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    finger_pan(&mut tree, row_centre(5), ROW / 3.0);

    assert!(offset.get() > 0.0, "the pan scrolled the list");
    assert_eq!(
        selected(&sel),
        vec![0],
        "the pan selected the row the finger happened to start on",
    );
}

/// The nav cursor travels with the applied selection, not with the press.
///
/// A finger's decision is made at the release, and the arrow-key origin has to
/// move with it. Where a view has no cursor yet it falls back to the *first*
/// selected row, which coincides with the tapped row for a plain tap — so the
/// case that separates the two is an accelerator tap that adds a row *after* an
/// existing selection: the cursor must be the row just touched, not the lowest
/// one selected.
#[test]
fn a_finger_tap_moves_the_arrow_key_origin_to_the_row_it_selected() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![2], false);
    let (mut tree, id) = tree_with(list(&sel));
    tree.focus(id);

    let f = finger();
    tree.dispatch_pointer(touch_accel(f, PointerPhase::Down, row_centre(7), 0));
    tree.dispatch_pointer(touch_accel(f, PointerPhase::Up, row_centre(7), 8));
    assert_eq!(
        selected(&sel),
        vec![2, 7],
        "precondition: the accelerator tap added row 7 on the release",
    );

    tree.press_key(teksilo_core::event::Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        selected(&sel),
        vec![8],
        "the arrow must step from the row the finger touched, not from row 2",
    );
}

// ---------------------------------------------------------------------------
// TableView cell selection — the fifth commit site
// ---------------------------------------------------------------------------
fn cell_table(mode: TableSelectionMode, cells: &CellSelectionModel) -> TableView<usize> {
    TableView::new(ListModel::from_vec((0..ROWS).collect()))
        .cell_selection(cells.clone())
        .add_column(
            Column::new("n", lit!("N"), |i: &usize, _cx| {
                Box::new(TextWidget::new(lit!(i.to_string())))
            })
            .width(ColumnWidth::Fixed(200.0)),
        )
        .row_height(ROW)
        .selection_mode(mode)
}

#[test]
fn a_mouse_press_selects_a_table_cell_immediately() {
    let cells = CellSelectionModel::new(TableSelectionMode::SingleCell);
    let view = cell_table(TableSelectionMode::SingleCell, &cells);
    let (mut tree, _id) = tree_with(view);
    // Below the header strip, in the body.
    let at = Point::new(60.0, VIEWPORT / 2.0);
    tree.dispatch_pointer(mouse(PointerPhase::Down, at, 0));
    assert!(cells.count() > 0, "a mouse still selects a cell on press",);
}

#[test]
fn a_finger_pan_over_a_cell_selecting_table_selects_no_cell() {
    let cells = CellSelectionModel::new(TableSelectionMode::SingleCell);
    let view = cell_table(TableSelectionMode::SingleCell, &cells);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    finger_pan(&mut tree, Point::new(60.0, VIEWPORT / 2.0), ROW / 3.0);

    assert!(offset.get() > 0.0, "the pan scrolled the table");
    assert!(
        cells.count() == 0,
        "the pan selected {} cells under the finger",
        cells.count(),
    );
}

#[test]
fn a_finger_tap_selects_a_table_cell() {
    let cells = CellSelectionModel::new(TableSelectionMode::SingleCell);
    let view = cell_table(TableSelectionMode::SingleCell, &cells);
    let (mut tree, _id) = tree_with(view);
    finger_tap(&mut tree, Point::new(60.0, VIEWPORT / 2.0));
    assert!(cells.count() > 0, "the release is the click for a cell too",);
}

// ---------------------------------------------------------------------------
// the shape that ships — every view, no activation hook, no drag
// ---------------------------------------------------------------------------
//
// The `list` fixture above configures a `ListView` with an activation hook. That
// is a real configuration, but it is not the *default* one, and the default is
// what a plain selectable list, tree, table or grid is: a selection model and
// nothing else. Four of the five views had no assertion able to tell a
// press-time write from a release-time one in that shape, which is how a
// `GridView` that selected nothing under a finger — and stopped collapsing a
// multi-selection under a mouse — shipped green. Everything below is built on
// the plain shape.
//
// Each assertion names its row by **probing** for it: a mouse click at the same
// point, on a throwaway instance of the same fixture, reports the index the
// view itself resolves there. So a header strip that changes height or a tile
// gutter that changes width cannot silently move a test off its target — it
// would fail the probe's own single-row assertion instead.

/// The offset signal a view scrolls, paired with the view.
type Offset = teksilo_core::signal::Signal<f32>;

fn plain_list(sel: &SelectionModel) -> (ListView<usize>, Offset) {
    let view = ListView::new(ListModel::from_vec((0..ROWS).collect()), |i, _item, _s| {
        Box::new(TextWidget::new(lit!(i.to_string())))
    })
    .item_height(ROW)
    .selection(sel.clone());
    let offset = view.scroll_y_signal().clone();
    (view, offset)
}

fn flat_tree_model() -> teksilo_data::TreeModel<usize> {
    let model: teksilo_data::TreeModel<usize> = teksilo_data::TreeModel::new();
    for i in 0..ROWS {
        model.insert_root(i, i);
    }
    model
}

fn plain_tree(sel: &SelectionModel) -> (teksilo_widgets::TreeView<usize>, Offset) {
    // `TreeView` takes the offset rather than exposing one, and it must be
    // animated: the view smooth-scrolls by default.
    let offset = teksilo_core::signal::Signal::new_animated(0.0);
    let view = teksilo_widgets::TreeView::new(flat_tree_model(), |item, _entry, _sel| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(ROW)
    .scroll_signal(offset.clone())
    .selection(sel.clone());
    (view, offset)
}

fn plain_table(sel: &SelectionModel) -> (TableView<usize>, Offset) {
    let view = TableView::new(ListModel::from_vec((0..ROWS).collect()))
        .add_column(
            Column::new("n", lit!("N"), |i: &usize, _cx| {
                Box::new(TextWidget::new(lit!(i.to_string())))
            })
            .width(ColumnWidth::Fixed(300.0)),
        )
        .row_height(ROW)
        .selection(sel.clone());
    let offset = view.scroll_y_signal().clone();
    (view, offset)
}

fn plain_tree_table(sel: &SelectionModel) -> (teksilo_widgets::TreeTableView<usize>, Offset) {
    let view = teksilo_widgets::TreeTableView::new(flat_tree_model())
        .add_column(
            Column::new("n", lit!("N"), |i: &usize, _cx| {
                Box::new(TextWidget::new(lit!(i.to_string())))
            })
            .width(ColumnWidth::Fixed(300.0)),
        )
        .row_height(ROW)
        .selection(sel.clone());
    let offset = view.scroll_y_signal().clone();
    (view, offset)
}

const TILE_W: f32 = 160.0;

fn plain_grid(sel: &SelectionModel) -> (teksilo_widgets::GridView<usize>, Offset) {
    let view = teksilo_widgets::GridView::new(ListModel::from_vec((0..ROWS).collect()), |tc| {
        Box::new(TextWidget::new(lit!(tc.item.to_string())))
    })
    .sizing(teksilo_widgets::GridSizing::Fixed {
        width: TILE_W,
        height: ROW,
    })
    .selection(sel.clone());
    let offset = view.scroll_y_signal().clone();
    (view, offset)
}

/// A point inside a row of the plain list / tree — no header strip above them.
fn body_row() -> Point {
    row_centre(5)
}

/// A point inside a row of the plain table / tree table, clear of the 32 dp
/// header strip and of the twist-arrow column.
fn table_body_row() -> Point {
    Point::new(200.0, 32.0 + 5.0 * ROW + ROW / 2.0)
}

/// A point on a tile of the plain grid: the leading column, well down the body.
fn grid_tile() -> Point {
    Point::new(TILE_W / 2.0, 5.0 * ROW + ROW / 2.0)
}

/// The row index a **mouse** click at `at` resolves in `make`'s view.
fn probe_index<V, F>(make: &F, at: Point) -> usize
where
    V: teksilo_core::widget::Widget + 'static,
    F: Fn(&SelectionModel) -> (V, Offset),
{
    let sel = SelectionModel::new(SelectionMode::Multi);
    let (view, _offset) = make(&sel);
    let (mut tree, _id) = tree_with(view);
    mouse_click(&mut tree, at);
    let got = selected(&sel);
    assert_eq!(
        got.len(),
        1,
        "the probe click at ({}, {}) selected {got:?}, so no single row is \
         under it and nothing below is aimed where it thinks",
        at.x,
        at.y,
    );
    got[0]
}

/// A finger tap selects the row it lands on — the release *is* the click.
fn assert_a_finger_tap_selects<V, F>(make: F, at: Point, what: &str)
where
    V: teksilo_core::widget::Widget + 'static,
    F: Fn(&SelectionModel) -> (V, Offset),
{
    let target = probe_index(&make, at);
    let decoy = if target == 0 { 1 } else { 0 };
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![decoy], false);
    let (view, _offset) = make(&sel);
    let (mut tree, _id) = tree_with(view);

    finger_tap(&mut tree, at);
    assert_eq!(
        selected(&sel),
        vec![target],
        "the finger's release must select the {what} it lifted from",
    );
}

/// A finger that pans off a row leaves the selection exactly as it found it.
fn assert_a_finger_pan_leaves_the_selection_alone<V, F>(make: F, at: Point, what: &str)
where
    V: teksilo_core::widget::Widget + 'static,
    F: Fn(&SelectionModel) -> (V, Offset),
{
    let target = probe_index(&make, at);
    let decoy = if target == 0 { 1 } else { 0 };
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![decoy], false);
    let (view, offset) = make(&sel);
    let (mut tree, _id) = tree_with(view);

    finger_pan(&mut tree, at, ROW / 3.0);

    assert!(offset.get() > 0.0, "the pan scrolled the {what}'s view");
    assert_eq!(
        selected(&sel),
        vec![decoy],
        "the pan selected the {what} the finger happened to start on",
    );
}

/// A mouse press on an already-selected row keeps the whole set so a drag can
/// carry it, and the release collapses to the pressed row.
fn assert_a_mouse_press_defers_the_collapse<V, F>(make: F, at: Point, what: &str)
where
    V: teksilo_core::widget::Widget + 'static,
    F: Fn(&SelectionModel) -> (V, Offset),
{
    let target = probe_index(&make, at);
    let mut set = vec![target, target + 1, target + 3];
    set.sort_unstable();
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(set.clone(), false);
    let (view, _offset) = make(&sel);
    let (mut tree, _id) = tree_with(view);

    tree.dispatch_pointer(mouse(PointerPhase::Down, at, 0));
    assert_eq!(
        selected(&sel),
        set,
        "the press on a selected {what} must keep the set so a drag can carry \
         all of it",
    );
    tree.dispatch_pointer(mouse(PointerPhase::Up, at, 8));
    assert_eq!(
        selected(&sel),
        vec![target],
        "the release must collapse the set onto the pressed {what}",
    );
}

// -- ListView, in the default shape (no activation hook) --------------------

/// The hook on [`list`] is **not** load-bearing, and this is the measurement
/// that says so: the same assertion holds with it removed.
#[test]
fn a_finger_tap_on_a_list_row_selects_it_with_no_activation_hook() {
    assert_a_finger_tap_selects(plain_list, body_row(), "list row");
}

#[test]
fn a_finger_pan_from_an_unselected_plain_list_row_leaves_the_selection_alone() {
    assert_a_finger_pan_leaves_the_selection_alone(plain_list, body_row(), "list row");
}

#[test]
fn a_mouse_press_on_a_selected_plain_list_row_still_defers_the_collapse() {
    assert_a_mouse_press_defers_the_collapse(plain_list, body_row(), "list row");
}

// -- TreeView ---------------------------------------------------------------

#[test]
fn a_finger_tap_on_a_tree_row_selects_it() {
    assert_a_finger_tap_selects(plain_tree, body_row(), "tree row");
}

#[test]
fn a_finger_pan_from_an_unselected_tree_row_leaves_the_selection_alone() {
    assert_a_finger_pan_leaves_the_selection_alone(plain_tree, body_row(), "tree row");
}

#[test]
fn a_mouse_press_on_a_selected_tree_row_still_defers_the_collapse() {
    assert_a_mouse_press_defers_the_collapse(plain_tree, body_row(), "tree row");
}

// -- TableView, row selection ----------------------------------------------

#[test]
fn a_finger_tap_on_a_table_row_selects_it() {
    assert_a_finger_tap_selects(plain_table, table_body_row(), "table row");
}

#[test]
fn a_finger_pan_from_an_unselected_table_row_leaves_the_selection_alone() {
    assert_a_finger_pan_leaves_the_selection_alone(plain_table, table_body_row(), "table row");
}

#[test]
fn a_mouse_press_on_a_selected_table_row_still_defers_the_collapse() {
    assert_a_mouse_press_defers_the_collapse(plain_table, table_body_row(), "table row");
}

// -- TreeTableView ---------------------------------------------------------

#[test]
fn a_finger_tap_on_a_tree_table_row_selects_it() {
    assert_a_finger_tap_selects(plain_tree_table, table_body_row(), "tree table row");
}

#[test]
fn a_finger_pan_from_an_unselected_tree_table_row_leaves_the_selection_alone() {
    assert_a_finger_pan_leaves_the_selection_alone(
        plain_tree_table,
        table_body_row(),
        "tree table row",
    );
}

#[test]
fn a_mouse_press_on_a_selected_tree_table_row_still_defers_the_collapse() {
    assert_a_mouse_press_defers_the_collapse(plain_tree_table, table_body_row(), "tree table row");
}

// -- GridView --------------------------------------------------------------

#[test]
fn a_finger_tap_on_a_grid_tile_selects_it() {
    assert_a_finger_tap_selects(plain_grid, grid_tile(), "grid tile");
}

#[test]
fn a_finger_pan_from_an_unselected_grid_tile_leaves_the_selection_alone() {
    assert_a_finger_pan_leaves_the_selection_alone(plain_grid, grid_tile(), "grid tile");
}

#[test]
fn a_mouse_press_on_a_selected_grid_tile_still_defers_the_collapse() {
    assert_a_mouse_press_defers_the_collapse(plain_grid, grid_tile(), "grid tile");
}

/// The mouse half of the same defect, stated on its own so it cannot be read as
/// a touch regression: a plain click on a tile of a multi-selection collapses
/// the set onto that tile, exactly as it does on a list row.
///
/// A tile whose press is captured by an ancestor never sees the release that
/// commits the deferral, so the set survived the click and the grid was the one
/// view where a click could not narrow a selection.
#[test]
fn a_mouse_click_on_a_selected_grid_tile_collapses_the_selection() {
    let at = grid_tile();
    let target = probe_index(&plain_grid, at);
    let mut set = vec![target, target + 1, target + 3];
    set.sort_unstable();
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(set, false);
    let (view, _offset) = plain_grid(&sel);
    let (mut tree, _id) = tree_with(view);

    mouse_click(&mut tree, at);
    assert_eq!(
        selected(&sel),
        vec![target],
        "the click must leave only the tile it landed on selected",
    );
}

/// The generalisation of the `GridView` defect — a **known defect**, measured,
/// not yet fixed.
///
/// A plain row carries no gesture arena of its own: the absorber goes on only
/// when the row is a drag source. Nothing *inside* the four row views competes
/// for the press, which is why they were correct while `GridView` — whose body
/// pane holds an absorber — was not. But an application can put an arena above a
/// row by wrapping the view in anything tappable, and then the wrapper captures
/// the press, the release is dispatched to the wrapper and bubbled
/// wrapper→root, and the row never sees it.
///
/// **Measured, on this fixture:** a plain `ListView` inside a `ZStack` carrying
/// an `on_tap`, selection preset to `[0]`, finger tap on row 5 → selection stays
/// `[0]`. A mouse click on the same row still selects it, because a mouse
/// commits on the press and the press *bubble* does reach the row; it is the
/// release-time commit that is lost. No example in `examples/` is affected —
/// there is no `on_tap` in the whole directory (the one that was, on a scene
/// card, went away when `SceneCard` replaced it).
///
/// **Why it is not fixed here.** The obvious fix is the absorber applied
/// unconditionally at all four row sites, as `GridView` now applies it to tiles.
/// That was tried and measured: it does make this test pass, and it breaks
/// `a_finger_tap_selects_a_table_cell` — the row's new arena captures the press
/// that `TableView`'s **cell** selection needs to commit on release, which is
/// this same defect one level further down. So the fix is not four lines; it is
/// four lines plus a cell-level absorber plus a decision about which node owns a
/// press when an application wraps a data view in a tappable container. That is
/// a design call with downstream consumers behind it, not a one-line bug fix,
/// and it belongs in its own package.
#[ignore = "known defect: an application-supplied ancestor arena steals a plain \
row's release. Fixing it by making the row absorber unconditional breaks \
a_finger_tap_selects_a_table_cell (measured); see this test's doc comment."]
#[test]
fn a_finger_tap_on_a_list_row_under_a_tappable_ancestor_still_selects_it() {
    use teksilo_core::widget_builder::WidgetBuilder;

    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select_indices(vec![0], false);
    let (view, _offset) = plain_list(&sel);
    let wrapped = teksilo_widgets::primitives::ZStack::new()
        .child(view)
        .on_tap(|_tap, _ctx| {});
    let (mut tree, _id) = tree_with(wrapped);

    finger_tap(&mut tree, body_row());
    assert_eq!(
        selected(&sel),
        vec![5],
        "an arena above the row took the press and the row never saw the \
         release — the `GridView` defect, reachable from application code",
    );
}
