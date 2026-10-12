// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`ListViewHandle`]: a reveal requested from outside the widget
//! is applied at the next layout pass, against settled row offsets.
use super::tests::FixedLeaf;
use super::*;
use teksilo_core::widget_tree::WidgetTree;

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 200.0)
}

/// Lay the tree out until the realize → measure → anchor cycle settles.
fn settle(tree: &mut WidgetTree) {
    for _ in 0..8 {
        tree.layout(viewport());
    }
}

/// The realized row of model `index`, relative to the list's top, as
/// `(top, bottom)`. `None` when the row is not realized.
fn row_span(tree: &WidgetTree, lv: WidgetId, index: usize) -> Option<(f32, f32)> {
    let map = {
        let any = tree.widget_as_any(lv)?;
        any.downcast_ref::<ListView<usize>>()?
            .realized_row_ids()
            .borrow()
            .clone()
    };
    let (_, id) = map.into_iter().find(|&(i, _)| i == index)?;
    let b = tree.bounds(id);
    let top = b.y - tree.bounds(lv).y;
    Some((top, top + b.height))
}

fn uniform_list(count: usize) -> (ListView<usize>, ListModel<usize>) {
    let model = ListModel::from_vec((0..count).collect());
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 20.0))
    })
    .item_height(20.0);
    (view, model)
}

/// Rows are really 40 tall; the view is told to expect 20.
fn auto_list(count: usize) -> (ListView<usize>, ListModel<usize>) {
    let model = ListModel::from_vec((0..count).collect());
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 40.0))
    })
    .auto_item_height(20.0);
    (view, model)
}

#[test]
fn a_handle_scrolls_a_mounted_list_to_an_item() {
    let (view, _model) = uniform_list(500);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);

    handle.scroll_to_index(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, lv, 100), Some((0.0, 20.0)));
}

#[test]
fn a_request_made_before_mount_is_applied_at_the_first_layout() {
    let (view, _model) = uniform_list(500);
    let scroll = view.scroll_y_signal().clone();
    let handle = view.handle();
    handle.scroll_to_index(100);
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);

    tree.layout(viewport());
    assert_eq!(scroll.get(), 2000.0, "applied by the first layout pass");
    settle(&mut tree);
    assert_eq!(row_span(&tree, lv, 100), Some((0.0, 20.0)));
}

#[test]
fn a_later_request_replaces_an_earlier_pending_one() {
    let (view, _model) = uniform_list(500);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);

    handle.scroll_to_index(300);
    handle.scroll_to_index(40);
    settle(&mut tree);
    assert_eq!(row_span(&tree, lv, 40), Some((0.0, 20.0)));
}

/// After a same-turn insert the scroll range of the last pass still
/// describes the shorter list, which would clamp a target near the end.
#[test]
fn a_same_turn_insert_above_then_scroll_to_index_lands_on_the_row() {
    let (view, model) = uniform_list(100);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);

    for v in 0..10 {
        model.insert(0, 1000 + v);
    }
    handle.scroll_to_index(100);
    settle(&mut tree);
    assert_eq!(row_span(&tree, lv, 100), Some((0.0, 20.0)));
}

/// With auto-measured rows, rows inserted above in the same turn also
/// invalidate every measured height below them: the target and the scroll
/// range are both guesses until the rows near the end are measured.
#[test]
fn a_same_turn_insert_above_then_scroll_to_index_lands_on_the_measured_row() {
    let (view, model) = auto_list(200);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);

    for v in 0..3 {
        model.insert(0, 1000 + v);
    }
    handle.scroll_to_index(197);
    settle(&mut tree);
    assert_eq!(row_span(&tree, lv, 197), Some((0.0, 40.0)));
}

/// The minimum-distance form, where the row's own height decides where the
/// scroll stops.
#[test]
fn a_same_turn_insert_above_then_ensure_index_visible_shows_the_whole_row() {
    let (view, model) = auto_list(200);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);

    for v in 0..3 {
        model.insert(0, 1000 + v);
    }
    handle.ensure_index_visible(120);
    settle(&mut tree);
    let (top, bottom) = row_span(&tree, lv, 120).expect("row 120 is realized");
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
    let (view, _model) = auto_list(200);
    let scroll = view.scroll_y_signal().clone();
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);
    scroll.set(400.0);
    settle(&mut tree);
    let before = scroll.get();
    let span = row_span(&tree, lv, 12);

    handle.ensure_index_visible(12);
    settle(&mut tree);
    assert_eq!(scroll.get(), before);
    assert_eq!(row_span(&tree, lv, 12), span);
}
