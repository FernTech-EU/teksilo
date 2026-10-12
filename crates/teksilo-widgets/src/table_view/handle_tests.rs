// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`TableViewHandle`]: a reveal requested from outside the widget
//! is applied at the next layout pass, against settled row offsets.
use teksilo_canvas::{Size, SizeProposal};
use teksilo_core::widget::{LayoutContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::ListModel;
use teksilo_i18n::lit;

use super::{CellContext, Column, ColumnWidth, TableView};

#[derive(Debug)]
struct Leaf(f32);
impl Widget for Leaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(50.0, self.0).into()
    }
}

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 200.0)
}

/// Lay the tree out until the realize → measure → anchor cycle settles.
fn settle(tree: &mut WidgetTree) {
    for _ in 0..8 {
        tree.layout(viewport());
    }
}

/// A headerless one-column table over `0..n`, its cells `cell_height` tall.
fn table(n: usize, cell_height: f32) -> (TableView<usize>, ListModel<usize>) {
    let model = ListModel::from_vec((0..n).collect());
    let column = Column::<usize>::new("v", lit!("V"), move |_v, _: &CellContext| {
        Box::new(Leaf(cell_height))
    })
    .width(ColumnWidth::Flex(1.0));
    let view = TableView::new(model.clone())
        .add_column(column)
        .show_header(false);
    (view, model)
}

fn uniform_table(n: usize) -> (TableView<usize>, ListModel<usize>) {
    let (view, model) = table(n, 20.0);
    (view.row_height(20.0), model)
}

/// Rows are really 40 tall; the table is told to expect 20.
fn auto_table(n: usize) -> (TableView<usize>, ListModel<usize>) {
    let (view, model) = table(n, 40.0);
    (view.auto_row_height(20.0), model)
}

fn mount(view: TableView<usize>) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(view);
    (tree, id)
}

/// The realized row of model `row`, relative to the table's top (there is
/// no header), as `(top, bottom)`. `None` when the row is not realized.
fn row_span(tree: &WidgetTree, table: WidgetId, row: usize) -> Option<(f32, f32)> {
    let map = tree
        .widget_as_any(table)?
        .downcast_ref::<TableView<usize>>()?
        .row_map
        .borrow()
        .clone();
    let (_, id) = map.into_iter().find(|&(i, _)| i == row)?;
    let b = tree.bounds(id);
    let top = b.y - tree.bounds(table).y;
    Some((top, top + b.height))
}

#[test]
fn a_handle_scrolls_a_mounted_table_to_a_row() {
    let (view, _model) = uniform_table(500);
    let handle = view.handle();
    let (mut tree, table) = mount(view);
    settle(&mut tree);

    handle.scroll_to_row(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, table, 100), Some((0.0, 20.0)));
}

#[test]
fn a_request_made_before_mount_is_applied_at_the_first_layout() {
    let (view, _model) = uniform_table(500);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    handle.scroll_to_row(100);
    let (mut tree, table) = mount(view);

    tree.layout(viewport());
    assert_eq!(scroll.get(), 2000.0, "applied by the first layout pass");
    settle(&mut tree);
    assert_eq!(row_span(&tree, table, 100), Some((0.0, 20.0)));
}

/// After a same-turn insert the scroll range of the last pass still
/// describes the shorter table, which would clamp a target near the end.
#[test]
fn a_same_turn_insert_above_then_scroll_to_row_lands_on_the_row() {
    let (view, model) = uniform_table(100);
    let handle = view.handle();
    let (mut tree, table) = mount(view);
    settle(&mut tree);

    for v in 0..10 {
        model.insert(0, 1000 + v);
    }
    handle.scroll_to_row(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, table, 100), Some((0.0, 20.0)));
}

/// With auto-measured rows the insert also drops every measured height below
/// it: the target and the scroll range are guesses until the rows near the
/// end are measured.
#[test]
fn a_same_turn_insert_above_then_scroll_to_row_lands_on_the_measured_row() {
    let (view, model) = auto_table(200);
    let handle = view.handle();
    let (mut tree, table) = mount(view);
    settle(&mut tree);

    for v in 0..3 {
        model.insert(0, 1000 + v);
    }
    handle.scroll_to_row(197);
    settle(&mut tree);
    assert_eq!(row_span(&tree, table, 197), Some((0.0, 40.0)));
}

#[test]
fn a_same_turn_insert_above_then_ensure_row_visible_shows_the_whole_row() {
    let (view, model) = auto_table(200);
    let handle = view.handle();
    let (mut tree, table) = mount(view);
    settle(&mut tree);

    for v in 0..3 {
        model.insert(0, 1000 + v);
    }
    handle.ensure_row_visible(120);
    settle(&mut tree);
    let (top, bottom) = row_span(&tree, table, 120).expect("row 120 is realized");
    assert!(
        top >= -0.01 && bottom <= 200.01,
        "row 120 must be fully in view, at {top}..{bottom}"
    );
    assert!(
        (bottom - 200.0).abs() < 0.01,
        "the minimum scroll leaves it at the bottom edge, got {bottom}"
    );
}

#[test]
fn ensure_row_visible_does_not_scroll_a_row_already_in_view() {
    let (view, _model) = auto_table(200);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    let (mut tree, table) = mount(view);
    settle(&mut tree);
    scroll.set(400.0);
    settle(&mut tree);
    let before = scroll.get();
    let span = row_span(&tree, table, 12);

    handle.ensure_row_visible(12);
    settle(&mut tree);
    assert_eq!(scroll.get(), before);
    assert_eq!(row_span(&tree, table, 12), span);
}

#[test]
fn a_request_on_a_table_without_rows_is_dropped() {
    let (view, model) = uniform_table(0);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    let (mut tree, _table) = mount(view);
    handle.scroll_to_row(50);
    settle(&mut tree);

    for v in 0..500 {
        model.push(v);
    }
    settle(&mut tree);
    assert_eq!(
        scroll.get(),
        0.0,
        "rows that arrive later are not scrolled to"
    );
}
