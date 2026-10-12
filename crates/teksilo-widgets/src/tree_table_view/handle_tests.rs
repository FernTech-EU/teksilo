// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`TreeTableViewHandle`]: a reveal requested from outside the
//! widget is applied at the next layout pass, against settled row offsets.
use super::*;
use crate::table_view::column::{CellContext, ColumnWidth};
use teksilo_canvas::SizeProposal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::TreeModel;
use teksilo_i18n::lit;

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

/// A first root with `children` (collapsed) children, then `roots - 1` more
/// roots.
fn model(roots: usize, children: usize) -> (TreeModel<usize>, NodeId) {
    let model = TreeModel::new();
    let first = model.insert_root(0, 0);
    for c in 0..children {
        model.insert_child(first, c, 1000 + c);
    }
    for r in 1..roots {
        model.insert_root(r, r);
    }
    (model, first)
}

/// A headerless one-column view whose cells are `cell_height` tall.
fn view(model: TreeModel<usize>, cell_height: f32) -> TreeTableView<usize> {
    let column = Column::<usize>::new("v", lit!("V"), move |_v, _: &CellContext| {
        Box::new(Leaf(cell_height))
    })
    .width(ColumnWidth::Flex(1.0));
    TreeTableView::new(model)
        .add_column(column)
        .show_header(false)
}

fn uniform_view(roots: usize) -> (TreeTableView<usize>, NodeId) {
    let (model, first) = model(roots, 10);
    (view(model, 20.0).row_height(20.0), first)
}

/// Rows are really 40 tall; the view is told to expect 20.
fn auto_view(roots: usize) -> (TreeTableView<usize>, NodeId) {
    let (model, first) = model(roots, 10);
    (view(model, 40.0).auto_row_height(20.0), first)
}

fn mount(view: TreeTableView<usize>) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(view);
    (tree, id)
}

fn expand(tree: &WidgetTree, id: WidgetId, node: NodeId) {
    tree.widget_as_any(id)
        .and_then(|any| any.downcast_ref::<TreeTableView<usize>>())
        .expect("a TreeTableView<usize>")
        .expand(node);
}

/// The realized row at flat index `row`, relative to the view's top (there
/// is no header), as `(top, bottom)`. `None` when the row is not realized.
fn row_span(tree: &WidgetTree, id: WidgetId, row: usize) -> Option<(f32, f32)> {
    let map = tree
        .widget_as_any(id)?
        .downcast_ref::<TreeTableView<usize>>()?
        .row_map
        .borrow()
        .clone();
    let (_, row_id) = map.into_iter().find(|&(i, _)| i == row)?;
    let b = tree.bounds(row_id);
    let top = b.y - tree.bounds(id).y;
    Some((top, top + b.height))
}

#[test]
fn a_handle_scrolls_a_mounted_view_to_a_row() {
    let (view, _) = uniform_view(500);
    let handle = view.handle();
    let (mut tree, id) = mount(view);
    settle(&mut tree);

    handle.scroll_to_row(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, id, 100), Some((0.0, 20.0)));
}

#[test]
fn a_request_made_before_mount_is_applied_at_the_first_layout() {
    let (view, _) = uniform_view(500);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    handle.scroll_to_row(100);
    let (mut tree, id) = mount(view);

    tree.layout(viewport());
    assert_eq!(scroll.get(), 2000.0, "applied by the first layout pass");
    settle(&mut tree);
    assert_eq!(row_span(&tree, id, 100), Some((0.0, 20.0)));
}

/// Expanding a node above the target in the same turn adds rows the scroll
/// range of the last pass does not know about yet.
#[test]
fn a_same_turn_expand_above_then_scroll_to_row_lands_on_the_row() {
    let (view, first) = uniform_view(100);
    let handle = view.handle();
    let (mut tree, id) = mount(view);
    settle(&mut tree);

    expand(&tree, id, first);
    handle.scroll_to_row(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, id, 100), Some((0.0, 20.0)));
}

/// With auto-measured rows the expansion also drops every measured height
/// below the node, so the row's place is a guess until it is measured.
#[test]
fn a_same_turn_expand_above_then_ensure_row_visible_shows_the_whole_row() {
    let (view, first) = auto_view(200);
    let handle = view.handle();
    let (mut tree, id) = mount(view);
    settle(&mut tree);

    expand(&tree, id, first);
    handle.ensure_row_visible(120);
    settle(&mut tree);
    let (top, bottom) = row_span(&tree, id, 120).expect("row 120 is realized");
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
    let (view, _) = auto_view(200);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    let (mut tree, id) = mount(view);
    settle(&mut tree);
    scroll.set(400.0);
    settle(&mut tree);
    let before = scroll.get();
    let span = row_span(&tree, id, 12);

    handle.ensure_row_visible(12);
    settle(&mut tree);
    assert_eq!(scroll.get(), before);
    assert_eq!(row_span(&tree, id, 12), span);
}

/// With pinned ancestors, the row stops below the copy of its parent pinned
/// over the top of the viewport, as `scroll_to_row` stops it.
#[test]
fn a_reveal_stops_below_the_pinned_ancestors() {
    let (model, first) = model(10, 50);
    let view = view(model, 20.0).row_height(20.0).pinned_ancestors(1);
    let handle = view.handle();
    let (mut tree, id) = mount(view);
    settle(&mut tree);
    expand(&tree, id, first);
    settle(&mut tree);

    handle.scroll_to_row(30);
    settle(&mut tree);
    assert_eq!(row_span(&tree, id, 30), Some((20.0, 40.0)));
}
