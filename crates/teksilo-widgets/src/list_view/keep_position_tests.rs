// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`ListView::keep_position_on_insert`]: the row at the top of the
//! viewport keeps its place on screen when the model changes above it.
use super::tests::FixedLeaf;
use super::*;
use teksilo_core::widget_tree::WidgetTree;

const VIEWPORT_HEIGHT: f32 = 300.0;

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, VIEWPORT_HEIGHT)
}

/// Lay the tree out until the realize, measure and anchor cycle settles.
fn settle(tree: &mut WidgetTree) {
    for _ in 0..8 {
        tree.layout(viewport());
    }
}

/// The realized row of model `index`, relative to the list's top, as
/// `(top, bottom)`. `None` when the row is not realized.
fn row_span(tree: &WidgetTree, lv: WidgetId, index: usize) -> Option<(f32, f32)> {
    let (_, id) = realized(tree, lv).into_iter().find(|&(i, _)| i == index)?;
    let b = tree.bounds(id);
    let top = b.y - tree.bounds(lv).y;
    Some((top, top + b.height))
}

/// The realized rows' model indices and node ids.
fn realized(tree: &WidgetTree, lv: WidgetId) -> Vec<(usize, WidgetId)> {
    let any = tree.widget_as_any(lv).expect("the list is mounted");
    let ids = if let Some(view) = any.downcast_ref::<ListView<usize>>() {
        view.realized_row_ids()
    } else {
        any.downcast_ref::<ListView<String>>()
            .expect("a list of usize or String")
            .realized_row_ids()
    };
    ids.borrow().clone()
}

fn top_of(tree: &WidgetTree, lv: WidgetId, index: usize) -> f32 {
    row_span(tree, lv, index)
        .unwrap_or_else(|| panic!("row {index} is realized"))
        .0
}

/// `count` rows numbered from 0, 30 tall, uniform.
fn uniform(count: usize, keep: bool) -> (WidgetTree, WidgetId, ListModel<usize>, Signal<f32>) {
    let model = ListModel::from_vec((0..count).collect());
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 30.0))
    })
    .item_height(30.0)
    .keep_position_on_insert(keep);
    let scroll = view.scroll_y_signal().clone();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);
    (tree, lv, model, scroll)
}

/// Insert ten rows at index 0, one `insert` each, numbered from 1000.
fn insert_ten_at_top(model: &ListModel<usize>) {
    for v in (0..10).rev() {
        model.insert(0, 1000 + v);
    }
}

#[test]
fn rows_inserted_at_the_top_leave_the_first_visible_row_in_place() {
    let (mut tree, lv, model, scroll) = uniform(100, true);
    let max_before = view_max(&tree, lv);
    scroll.set(13.0);
    settle(&mut tree);
    assert_eq!(top_of(&tree, lv, 0), -13.0);

    insert_ten_at_top(&model);
    tree.layout(viewport());

    assert_eq!(scroll.get(), 313.0);
    assert_eq!(
        top_of(&tree, lv, 10),
        -13.0,
        "the row that was first keeps its offset in the pass that shows the change"
    );
    let rows: Vec<usize> = realized(&tree, lv).into_iter().map(|(i, _)| i).collect();
    assert!(
        rows.iter().all(|&i| i >= 5),
        "no row of the inserted page beyond the realization buffer is built for a frame: {rows:?}"
    );
    assert_eq!(
        view_max(&tree, lv),
        max_before + 300.0,
        "the scroll range grows"
    );
    assert!(scroll.get() <= view_max(&tree, lv));
}

fn view_max(tree: &WidgetTree, lv: WidgetId) -> f32 {
    tree.widget_as_any(lv)
        .and_then(|any| any.downcast_ref::<ListView<usize>>())
        .expect("a list of usize")
        .max_scroll_y_signal()
        .get()
}

#[test]
fn rows_inserted_at_the_top_while_scrolled_to_the_middle() {
    let (mut tree, lv, model, scroll) = uniform(200, true);
    scroll.set(1507.0);
    settle(&mut tree);
    let before = top_of(&tree, lv, 50);

    insert_ten_at_top(&model);
    tree.layout(viewport());

    assert_eq!(scroll.get(), 1807.0);
    assert!((top_of(&tree, lv, 60) - before).abs() < 0.5);
}

#[test]
fn rows_removed_above_leave_the_first_visible_row_in_place() {
    let (mut tree, lv, model, scroll) = uniform(200, true);
    scroll.set(913.0);
    settle(&mut tree);
    let before = top_of(&tree, lv, 30);

    for _ in 0..5 {
        model.remove(2);
    }
    tree.layout(viewport());

    assert_eq!(scroll.get(), 763.0);
    assert!((top_of(&tree, lv, 25) - before).abs() < 0.5);
}

#[test]
fn removing_the_first_visible_row_puts_the_next_one_in_its_place() {
    let (mut tree, lv, model, scroll) = uniform(200, true);
    scroll.set(913.0);
    settle(&mut tree);
    let before = top_of(&tree, lv, 30);

    model.remove(30);
    tree.layout(viewport());

    assert_eq!(scroll.get(), 913.0);
    assert!((top_of(&tree, lv, 30) - before).abs() < 0.5);
}

#[test]
fn rows_inserted_below_the_first_visible_row_do_not_scroll() {
    let (mut tree, lv, model, scroll) = uniform(200, true);
    scroll.set(913.0);
    settle(&mut tree);
    let before = top_of(&tree, lv, 30);

    for v in 0..10 {
        model.insert(35, 1000 + v);
    }
    model.insert(150, 2000);
    tree.layout(viewport());

    assert_eq!(scroll.get(), 913.0);
    assert_eq!(top_of(&tree, lv, 30), before);
}

/// Without the option the offset stays the same number, so the page
/// inserted above slides into view: today's behaviour, pinned.
#[test]
fn without_the_option_rows_inserted_above_take_the_readers_place() {
    let (mut tree, lv, model, scroll) = uniform(100, false);
    scroll.set(13.0);
    settle(&mut tree);

    insert_ten_at_top(&model);
    tree.layout(viewport());

    assert_eq!(scroll.get(), 13.0);
    assert_eq!(top_of(&tree, lv, 0), -13.0);
    assert_eq!(top_of(&tree, lv, 10), 287.0);
}

/// Heights from a callback that reads the row's own item: the inserted rows
/// are 50 tall, the others 20, 30 or 40.
#[test]
fn exact_heights_shift_by_the_inserted_rows_own_heights() {
    let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
    let heights = model.clone();
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 20.0))
    })
    .item_height_fn(move |i| {
        heights
            .with_item(i, |&v| {
                if v >= 1000 {
                    50.0
                } else {
                    20.0 + (v % 3) as f32 * 10.0
                }
            })
            .unwrap_or(20.0)
    })
    .keep_position_on_insert(true);
    let scroll = view.scroll_y_signal().clone();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);
    // Row 40 starts at 40 rows of 20/30/40 cycling: 13 cycles of 90 + 20.
    scroll.set(1190.0 + 11.0);
    settle(&mut tree);
    assert_eq!(top_of(&tree, lv, 40), -11.0);

    insert_ten_at_top(&model);
    tree.layout(viewport());

    assert_eq!(scroll.get(), 1201.0 + 500.0);
    assert!((top_of(&tree, lv, 50) + 11.0).abs() < 0.5);
}

/// Auto-measured rows: the old ones measure 40, the inserted ones 60, and
/// the view expects 30.
fn auto(count: usize) -> (WidgetTree, WidgetId, ListModel<usize>, Signal<f32>) {
    let model = ListModel::from_vec((0..count).collect());
    let view = ListView::new(model.clone(), |_i, &item, _sel| {
        Box::new(FixedLeaf(100.0, if item >= 1000 { 60.0 } else { 40.0 }))
    })
    .auto_item_height(30.0)
    .keep_position_on_insert(true);
    let scroll = view.scroll_y_signal().clone();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);
    (tree, lv, model, scroll)
}

#[test]
fn measured_rows_inserted_at_the_top_settle_without_moving_the_reader() {
    let (mut tree, lv, model, scroll) = auto(100);
    scroll.set(7.0);
    settle(&mut tree);
    assert_eq!(top_of(&tree, lv, 0), -7.0);

    insert_ten_at_top(&model);
    tree.layout(viewport());
    assert!(
        (top_of(&tree, lv, 10) + 7.0).abs() < 0.5,
        "in the pass that shows the change, at {}",
        top_of(&tree, lv, 10)
    );

    settle(&mut tree);
    assert!((top_of(&tree, lv, 10) + 7.0).abs() < 0.5);
    assert_eq!(
        row_span(&tree, lv, 9).map(|(top, bottom)| bottom - top),
        Some(60.0),
        "the inserted rows next to the reader have been measured"
    );
}

#[test]
fn measured_rows_inserted_at_the_top_while_scrolled_to_the_middle() {
    let (mut tree, lv, model, scroll) = auto(200);
    // Reach the middle a viewport at a time, so every row above is measured.
    for step in 1..=10 {
        scroll.set(step as f32 * 290.0);
        settle(&mut tree);
    }
    let first = realized(&tree, lv)
        .into_iter()
        .map(|(i, _)| i)
        .find(|&i| top_of(&tree, lv, i) > -40.0)
        .expect("a row at the top");
    let before = top_of(&tree, lv, first);
    let scroll_before = scroll.get();

    insert_ten_at_top(&model);
    tree.layout(viewport());
    assert!((top_of(&tree, lv, first + 10) - before).abs() < 0.5);
    settle(&mut tree);
    assert!((top_of(&tree, lv, first + 10) - before).abs() < 0.5);
    assert_eq!(
        scroll.get(),
        scroll_before + 300.0,
        "the inserted rows count at the estimate while they are out of view, \
         and the rows already measured keep their heights at their new indices"
    );
}

/// The reproduction from the report: a chat history loading its older page
/// with `reconcile_by_key`, which reports the page as one insertion at 0.
#[test]
fn an_older_page_reconciled_by_key_leaves_the_reader_in_place() {
    let model = ListModel::from_vec((0..100).map(|i| format!("row {i}")).collect::<Vec<_>>());
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 30.0))
    })
    .auto_item_height(30.0)
    .keep_position_on_insert(true);
    let scroll = view.scroll_y_signal().clone();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    tree.layout(viewport());

    let changes = Rc::new(RefCell::new(Vec::new()));
    let _observer = model.observe_changes({
        let changes = changes.clone();
        move |change| changes.borrow_mut().push(change.clone())
    });
    let mut rows: Vec<String> = (0..10).map(|i| format!("old {i}")).collect();
    rows.extend((0..100).map(|i| format!("row {i}")));
    model.reconcile_by_key(rows, |r| r.clone());
    assert_eq!(
        *changes.borrow(),
        vec![DataChange::ItemsInserted { range: 0..10 }]
    );
    tree.layout(viewport());

    assert_eq!(scroll.get(), 300.0);
    assert_eq!(top_of(&tree, lv, 10), 0.0, "\"row 0\" is still at the top");
}

#[test]
fn a_pending_handle_reveal_wins_over_the_kept_position() {
    let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 30.0))
    })
    .item_height(30.0)
    .keep_position_on_insert(true);
    let handle = view.handle();
    let scroll = view.scroll_y_signal().clone();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);

    insert_ten_at_top(&model);
    handle.scroll_to_index(50);
    tree.layout(viewport());
    assert_eq!(
        scroll.get(),
        1500.0,
        "applied in the same pass, after the shift"
    );

    settle(&mut tree);
    assert_eq!(top_of(&tree, lv, 50), 0.0);
}

/// The selection follows the inserted rows by index, and a focused view
/// reveals its selected row when the selection moves: the row must be found
/// where the reader left it, so nothing scrolls a second time.
#[test]
fn a_selected_row_in_a_focused_view_is_shifted_once() {
    use teksilo_data::{SelectionMode, SelectionModel};

    let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 30.0))
    })
    .item_height(30.0)
    .selection(selection.clone())
    .keep_position_on_insert(true);
    let scroll = view.scroll_y_signal().clone();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    settle(&mut tree);
    scroll.set(913.0);
    settle(&mut tree);
    selection.select(35);
    tree.focus(lv);
    settle(&mut tree);
    assert_eq!(scroll.get(), 913.0, "row 35 was already in view");
    let before = top_of(&tree, lv, 35);

    insert_ten_at_top(&model);
    tree.layout(viewport());

    assert_eq!(selection.selected_indices(), vec![45]);
    assert_eq!(scroll.get(), 1213.0);
    assert_eq!(top_of(&tree, lv, 45), before);
}
