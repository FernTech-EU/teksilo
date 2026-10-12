// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`TreeViewHandle`]: a reveal requested from outside the widget
//! is applied at the next layout pass, against settled row offsets.
use super::*;
use teksilo_core::widget_tree::WidgetTree;

#[derive(Debug)]
struct Leaf(f32);
impl Widget for Leaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(100.0, self.0).into()
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

/// A first root with ten (collapsed) children, then `roots - 1` more roots.
fn model(roots: usize) -> (TreeModel<usize>, NodeId) {
    let model = TreeModel::new();
    let first = model.insert_root(0, 0);
    for c in 0..10 {
        model.insert_child(first, c, 1000 + c);
    }
    for r in 1..roots {
        model.insert_root(r, r);
    }
    (model, first)
}

fn uniform_tree(roots: usize) -> (TreeView<usize>, NodeId) {
    let (model, first) = model(roots);
    let view = TreeView::new(model, |_item, _entry, _sel| Box::new(Leaf(20.0))).item_height(20.0);
    (view, first)
}

/// Rows are really 40 tall; the view is told to expect 20.
fn auto_tree(roots: usize) -> (TreeView<usize>, NodeId) {
    let (model, first) = model(roots);
    let view =
        TreeView::new(model, |_item, _entry, _sel| Box::new(Leaf(40.0))).auto_item_height(20.0);
    (view, first)
}

fn expand(tree: &WidgetTree, tv: WidgetId, node: NodeId) {
    tree.widget_as_any(tv)
        .and_then(|any| any.downcast_ref::<TreeView<usize>>())
        .expect("a TreeView<usize>")
        .expand(node);
}

/// The realized row at flat index `index`, relative to the view's top, as
/// `(top, bottom)`. `None` when the row is not realized.
fn row_span(tree: &WidgetTree, tv: WidgetId, index: usize) -> Option<(f32, f32)> {
    let map = tree
        .widget_as_any(tv)?
        .downcast_ref::<TreeView<usize>>()?
        .row_map
        .borrow()
        .clone();
    let (_, id) = map.into_iter().find(|&(i, _)| i == index)?;
    let b = tree.bounds(id);
    let top = b.y - tree.bounds(tv).y;
    Some((top, top + b.height))
}

#[test]
fn a_handle_scrolls_a_mounted_tree_to_a_row() {
    let (view, _) = uniform_tree(500);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let tv = tree.add(view);
    settle(&mut tree);

    handle.scroll_to_index(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, tv, 100), Some((0.0, 20.0)));
}

#[test]
fn a_request_made_before_mount_is_applied_at_the_first_layout() {
    let (view, _) = uniform_tree(500);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    handle.scroll_to_index(100);
    let mut tree = WidgetTree::new();
    let tv = tree.add(view);

    tree.layout(viewport());
    assert_eq!(scroll.get(), 2000.0, "applied by the first layout pass");
    settle(&mut tree);
    assert_eq!(row_span(&tree, tv, 100), Some((0.0, 20.0)));
}

/// Expanding a node above the target in the same turn adds rows the scroll
/// range of the last pass does not know about yet.
#[test]
fn a_same_turn_expand_above_then_scroll_to_index_lands_on_the_row() {
    let (view, first) = uniform_tree(100);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let tv = tree.add(view);
    settle(&mut tree);

    expand(&tree, tv, first);
    handle.scroll_to_index(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, tv, 100), Some((0.0, 20.0)));
}

/// With auto-measured rows the expansion also drops every measured height
/// below the node, so the row's place is a guess until it is measured.
#[test]
fn a_same_turn_expand_above_then_ensure_index_visible_shows_the_whole_row() {
    let (view, first) = auto_tree(200);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let tv = tree.add(view);
    settle(&mut tree);

    expand(&tree, tv, first);
    handle.ensure_index_visible(120);
    settle(&mut tree);
    let (top, bottom) = row_span(&tree, tv, 120).expect("row 120 is realized");
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
fn ensure_index_visible_does_not_scroll_a_row_already_in_view() {
    let (view, _) = auto_tree(200);
    let scroll = view.scroll_y.clone();
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let tv = tree.add(view);
    settle(&mut tree);
    scroll.set(400.0);
    settle(&mut tree);
    let before = scroll.get();
    let span = row_span(&tree, tv, 12);

    handle.ensure_index_visible(12);
    settle(&mut tree);
    assert_eq!(scroll.get(), before);
    assert_eq!(row_span(&tree, tv, 12), span);
}
