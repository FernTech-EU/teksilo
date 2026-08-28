// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;
use teksilo_core::widget_tree::WidgetTree;
// Row-level concerns moved to `body_pane`; the tests still drive them from
// here, so they import what the root no longer needs.
use teksilo_data::{DragEligibility, RowState};
use teksilo_i18n::lit;

/// The realized row wrappers. `TreeView`'s own children are the body pane and
/// the scrollbar (see `body_pane`'s module docs for why rows sit one level
/// down), so every test that used to walk `tree.children(tv_id)` for rows goes
/// through here.
fn row_ids(tree: &WidgetTree, tv: WidgetId) -> Vec<WidgetId> {
    let kids = tree.children(tv);
    match kids.first() {
        Some(&pane) => tree.children(pane),
        None => Vec::new(),
    }
}

/// The internal scrollbar — always the TreeView's last child.
fn scrollbar_of(tree: &WidgetTree, tv: WidgetId) -> WidgetId {
    *tree.children(tv).last().expect("TreeView has children")
}

#[test]
fn smooth_scroll_survives_a_body_pane_rebuild() {
    let model: TreeModel<&'static str> = TreeModel::new();
    for i in 0..500 {
        model.insert_root(i, "row");
    }
    let (mut wtree, tv_id) = make_tree_view(model.clone());
    let scroll = {
        wtree.layout(SizeProposal::exact(400.0, 200.0));
        let any = wtree.widget_as_any(tv_id).unwrap();
        // `tests` is a child module of `tree_view`, so it can read the
        // private field directly — TreeView exposes no public scroll signal.
        any.downcast_ref::<TreeView<&'static str>>()
            .unwrap()
            .scroll_y
            .clone()
    };
    crate::common::thumb_drag_test::assert_fling_survives_pane_rebuild(
        &mut wtree,
        400.0,
        200.0,
        &scroll,
        "TreeView",
        || {
            model.insert_root(0, "fresh");
        },
    );
}

#[test]
fn rows_materialize_during_scrollbar_thumb_drag() {
    // The reason `TreeViewBodyPane` exists — see
    // `common::thumb_drag_test`'s module docs for the invariant.
    let model: TreeModel<&'static str> = TreeModel::new();
    for i in 0..500 {
        model.insert_root(i, "row");
    }
    let (mut wtree, tv_id) = make_tree_view(model);
    crate::common::thumb_drag_test::assert_body_survives_thumb_drag(
        &mut wtree,
        tv_id,
        400.0,
        200.0,
        0.0,
        "TreeView",
        |t| {
            row_ids(t, tv_id)
                .into_iter()
                .filter(|id| {
                    let b = t.bounds(*id);
                    b.height > 1.0 && b.y > -b.height && b.y < 200.0
                })
                .count()
        },
    );
}

#[derive(Debug)]
struct FixedLeaf(f32, f32);
impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

/// Build a sample tree:
/// A (has children: A1, A2)
/// B (has children: B1)
/// C (leaf)
fn sample_tree() -> TreeModel<&'static str> {
    let tree = TreeModel::new();
    let a = tree.insert_root(0, "A");
    tree.insert_child(a, 0, "A1");
    tree.insert_child(a, 1, "A2");
    let b = tree.insert_root(1, "B");
    tree.insert_child(b, 0, "B1");
    tree.insert_root(2, "C");
    tree
}

fn make_tree_view(tree: TreeModel<&'static str>) -> (WidgetTree, WidgetId) {
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |_item, entry, _selected| {
            // Width encodes depth, height is fixed
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0),
    );
    (wtree, tv_id)
}

#[test]
fn initial_shows_only_roots() {
    let tree = sample_tree();
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // 3 root items, realized inside the body pane.
    assert_eq!(row_ids(&wtree, tv_id).len(), 3);
}

#[test]
fn insert_child_into_root_updates_view() {
    let tree = sample_tree();
    let a = tree.root(0);
    let (mut wtree, tv_id) = make_tree_view(tree.clone());
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(row_ids(&wtree, tv_id).len(), 3);

    // Insert a new child under A — since A is collapsed, visible count stays 3
    tree.insert_child(a, 2, "A3");
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    // Still 3 visible (A collapsed), but the tree knows about A3
    assert_eq!(row_ids(&wtree, tv_id).len(), 3);
}

#[test]
fn model_mutation_triggers_rebuild() {
    let tree = sample_tree();
    let (mut wtree, tv_id) = make_tree_view(tree.clone());
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(row_ids(&wtree, tv_id).len(), 3);

    tree.insert_root(3, "D");
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(row_ids(&wtree, tv_id).len(), 4);
}

#[test]
fn remove_triggers_rebuild() {
    let tree = sample_tree();
    let c = tree.root(2);
    let (mut wtree, tv_id) = make_tree_view(tree.clone());
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(row_ids(&wtree, tv_id).len(), 3);

    tree.remove(c);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(row_ids(&wtree, tv_id).len(), 2);
}

#[test]
fn items_positioned_vertically() {
    let tree = sample_tree();
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let children = row_ids(&wtree, tv_id);
    let y0 = wtree.bounds(children[0]).y;
    let y1 = wtree.bounds(children[1]).y;
    let y2 = wtree.bounds(children[2]).y;
    assert!((y0 - 0.0).abs() < 0.01);
    assert!((y1 - 28.0).abs() < 0.01);
    assert!((y2 - 56.0).abs() < 0.01);
}

#[test]
fn virtualization_with_large_tree() {
    // Create a tree with 500 root nodes
    let tree = TreeModel::new();
    for i in 0..500 {
        tree.insert_root(i, format!("Node {}", i).leak() as &'static str);
    }
    let (mut wtree, tv_id) = make_tree_view(tree);
    // Viewport 300px, item height 28px → ~11 visible + 2*5 buffer = ~21
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let item_count = row_ids(&wtree, tv_id).len();
    assert!(
        item_count < 30,
        "Expected fewer than 30 items, got {}",
        item_count
    );
    assert!(
        item_count >= 10,
        "Expected at least 10 items, got {}",
        item_count
    );
}

#[test]
fn scrollbar_collapses_when_not_needed() {
    let tree = sample_tree(); // 3 roots, 3*28=84 < 300
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let sb_bounds = wtree.bounds(scrollbar_of(&wtree, tv_id));
    assert!(
        sb_bounds.width < 0.01 && sb_bounds.height < 0.01,
        "Scrollbar should be collapsed"
    );
}

#[test]
fn accessibility_role_is_tree() {
    let tree = sample_tree();
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    let info = wtree.accessibility_node(tv_id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::Tree);
}

#[test]
fn empty_tree() {
    let tree: TreeModel<&str> = TreeModel::new();
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // The body pane is mounted even with no data (it is the stable sibling
    // the scrollbar needs) and realizes no rows.
    assert_eq!(wtree.children(tv_id).len(), 2, "body pane + scrollbar");
    assert!(
        row_ids(&wtree, tv_id).is_empty(),
        "no rows for an empty tree"
    );
}

#[test]
fn tree_item_has_a11y_role_and_expanded() {
    let tree = sample_tree(); // A (has children), B (has children), C (leaf)
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let children = row_ids(&wtree, tv_id);
    // First child (A) should be a TreeItemWrapper with TreeItem role
    let info_a = wtree.accessibility_node(children[0]);
    assert_eq!(info_a.role(), teksilo_core::accesskit::Role::TreeItem);
    // A has children and is collapsed → is_expanded returns false
    assert!(
        !info_a.is_expanded(),
        "Root A should report not expanded (collapsed)"
    );

    // Third child (C) is a leaf → also not expanded
    let info_c = wtree.accessibility_node(children[2]);
    assert_eq!(info_c.role(), teksilo_core::accesskit::Role::TreeItem);
    assert!(!info_c.is_expanded(), "Leaf C should not be expanded");
}

#[test]
fn keyboard_arrow_down_navigates() {
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    let tree = sample_tree(); // A, B, C (3 roots)
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel_clone = selection.clone();

    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |_item, entry, _selected| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .selection(sel_clone),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Focus the TreeView
    wtree.focus(tv_id);

    // ArrowDown should select item 0 first (from no focus), then 1
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::NONE,
        text: None,
    });

    // No cursor yet, so the first ArrowDown lands ON row 0 rather than stepping
    // past it (it must not be possible to skip the first row by arrowing down).
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "ArrowDown from initial state should select index 0 (first root)"
    );

    // Another ArrowDown should move to index 1
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(
        selection.selected_indices(),
        vec![1],
        "Second ArrowDown should select index 1 (second root)"
    );
}

#[test]
fn arrow_nav_resumes_from_the_clicked_row() {
    // Regression: a row click must move the keyboard-navigation cursor
    // (`focused_index`) to the clicked row, so the next Arrow step continues
    // from there — not from the stale keyboard cursor / index 0. Rows are
    // 20px tall; row 3's body is at y≈70, x=100 (past any chevron column).
    use teksilo_core::event::{Key, Modifiers};
    let (mut wtree, tv, selection) = flat_tree_view(10, teksilo_data::SelectionMode::Single);
    wtree.layout(SizeProposal::exact(400.0, 300.0)); // 10 rows × 20px all visible
    wtree.focus(tv);

    // Click row 3 (selects it AND should set the nav cursor to 3).
    press_at(&mut wtree, 100.0, 70.0);
    assert_eq!(
        selection.selected_indices(),
        vec![3],
        "precondition: body click selects row 3"
    );

    // ArrowDown must step to 4 (from the clicked row), not to 1 (from index 0).
    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        vec![4],
        "ArrowDown after a click resumes from the clicked row (3 → 4)"
    );

    // And ArrowUp steps back above the clicked row (4 → 3).
    wtree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        vec![3],
        "ArrowUp resumes (4 → 3)"
    );
}

/// A flat tree of `n` roots labelled "Node {i}", with a single-select model.
fn flat_tree_view(
    n: usize,
    mode: teksilo_data::SelectionMode,
) -> (WidgetTree, WidgetId, teksilo_data::SelectionModel) {
    use teksilo_data::SelectionModel;
    let tree = TreeModel::new();
    for i in 0..n {
        tree.insert_root(i, format!("Node {i}"));
    }
    let selection = SelectionModel::new(mode);
    let sel = selection.clone();
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(120.0, 20.0)))
            .item_height(20.0)
            .selection(sel)
            .type_ahead_label(|s: &String| s.clone())
            .on_activate(|_, _| {}),
    );
    (wtree, tv, selection)
}

/// **A caller-owned scroll signal survives the view.**
///
/// A tree inside a dock is torn down and rebuilt whenever the layout changes, and
/// its own scroll offset goes with it -- the writer lands back at the top of a
/// result they had scrolled halfway through. Holding the signal where the model
/// lives is what keeps the position, exactly as the expand set is already kept.
#[test]
fn a_caller_owned_scroll_signal_outlives_the_view() {
    let scroll = Signal::new_animated(0.0_f32);

    let build = |scroll: Signal<f32>| {
        let tree = TreeModel::new();
        for i in 0..100usize {
            tree.insert_root(i, format!("Node {i}"));
        }
        let mut wtree = WidgetTree::new();
        wtree.add(
            TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
                .item_height(20.0)
                .smooth_scrolling(false)
                .scroll_signal(scroll),
        );
        wtree
    };

    let mut first = build(scroll.clone());
    first.layout(SizeProposal::exact(200.0, 200.0));
    let _ = first.render();
    scroll.set(340.0);
    first.layout(SizeProposal::exact(200.0, 200.0));
    let _ = first.render();
    assert!((scroll.get() - 340.0).abs() < 0.5);

    // The view is gone; the writer's position is not.
    drop(first);
    let mut rebuilt = build(scroll.clone());
    rebuilt.layout(SizeProposal::exact(200.0, 200.0));
    let _ = rebuilt.render();
    assert!(
        (scroll.get() - 340.0).abs() < 0.5,
        "a rebuilt tree came back at {} instead of where it was left",
        scroll.get()
    );
}

#[test]
fn first_arrow_lands_on_an_end_row_instead_of_skipping_it() {
    // "No cursor yet" is not "cursor on row 0": the very first ArrowDown must
    // select the FIRST row (not step past it to row 1, leaving the top row
    // unreachable until you arrow back up), and the first ArrowUp the LAST.
    use teksilo_core::event::{Key, Modifiers};

    for (key, want, what) in [
        (
            Key::ArrowDown,
            0usize,
            "first ArrowDown selects the first row",
        ),
        (Key::ArrowUp, 9usize, "first ArrowUp selects the last row"),
    ] {
        let (mut wtree, tv, selection) = flat_tree_view(10, teksilo_data::SelectionMode::Single);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        wtree.focus(tv);
        assert!(
            selection.selected_indices().is_empty(),
            "precondition: nothing selected, no cursor"
        );

        wtree.press_key(key, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![want], "{what}");
    }
}

#[test]
fn keyboard_cursor_starts_from_a_preset_selection() {
    // A tree can be handed a selection before it is ever focused (restoring the
    // last edited item). The first arrow must continue from that visible row,
    // not from an invisible zero.
    use teksilo_core::event::{Key, Modifiers};
    let (mut wtree, tv, selection) = flat_tree_view(10, teksilo_data::SelectionMode::Single);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    selection.select(4);
    wtree.focus(tv);

    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        vec![5],
        "Down from a preselected row 4 continues to 5"
    );
}

#[test]
fn page_down_up_moves_selection_by_viewport() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut wtree, tv, selection) = flat_tree_view(100, teksilo_data::SelectionMode::Single);
    let p = SizeProposal::exact(400.0, 200.0); // ~10 rows
    wtree.layout(p);
    wtree.focus(tv);
    selection.select(0);

    wtree.press_key(Key::PageDown, Modifiers::NONE);
    wtree.layout(p);
    let after = selection.selected_indices()[0];
    assert!(after >= 8, "PageDown advances ~one viewport, got {after}");

    wtree.press_key(Key::PageUp, Modifiers::NONE);
    wtree.layout(p);
    assert!(
        selection.selected_indices()[0] < after,
        "PageUp moves selection back up"
    );
}

#[test]
fn ctrl_a_selects_all_visible_in_multi_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut wtree, tv, selection) = flat_tree_view(7, teksilo_data::SelectionMode::Multi);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);
    wtree.press_key(Key::A, Modifiers::CTRL);
    assert_eq!(selection.selected_indices().len(), 7, "Ctrl+A selects all");
}

#[test]
fn ctrl_arrow_moves_cursor_without_selecting_in_multi_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut wtree, tv, selection) = flat_tree_view(6, teksilo_data::SelectionMode::Multi);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);

    let focused_index = |wtree: &WidgetTree| -> Option<usize> {
        wtree
            .widget_as_any(tv)
            .and_then(|any| any.downcast_ref::<TreeView<String>>())
            .and_then(|v| v.focused_index.get())
    };

    // Plain Arrow still selects (the first Down lands ON row 0).
    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![0]);

    // Ctrl+ArrowDown moves the cursor without touching the selection.
    wtree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+ArrowDown must leave the selection unchanged"
    );
    assert_eq!(
        focused_index(&wtree),
        Some(1),
        "Ctrl+ArrowDown moves the cursor to row 1"
    );

    wtree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0], "still unchanged");
    assert_eq!(focused_index(&wtree), Some(2));

    // Ctrl+Space toggles the now-focused row (row 2) on, adding to — not
    // replacing — the existing selection.
    wtree.press_key(Key::Space, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0, 2]);

    // Ctrl+Space again toggles it back off.
    wtree.press_key(Key::Space, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0]);

    // Plain Arrow after a Ctrl-cursor move still replaces the selection
    // with the new cursor position (select-follow).
    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![3]);
}

#[test]
fn ctrl_arrow_moves_cursor_without_selecting_in_single_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut wtree, tv, selection) = flat_tree_view(6, teksilo_data::SelectionMode::Single);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);

    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![0]);

    wtree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+ArrowDown must not select in Single mode either"
    );
    let focused = wtree
        .widget_as_any(tv)
        .and_then(|any| any.downcast_ref::<TreeView<String>>())
        .and_then(|v| v.focused_index.get());
    assert_eq!(focused, Some(1));

    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![2]);
}

#[test]
fn space_toggles_enter_activates() {
    use std::cell::Cell;
    use teksilo_core::event::{Key, Modifiers};
    let tree = TreeModel::new();
    for i in 0..5 {
        tree.insert_root(i, format!("Node {i}"));
    }
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let sel = selection.clone();
    let activated = Rc::new(Cell::new(None));
    let act = activated.clone();
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
            .item_height(20.0)
            .selection(sel)
            .on_activate(move |i, _| act.set(Some(i))),
    );
    wtree.layout(SizeProposal::exact(400.0, 200.0));
    wtree.focus(tv);

    // The first Down lands ON row 0 (it does not skip it), so reaching row 1
    // takes two.
    wtree.press_key(Key::ArrowDown, Modifiers::NONE); // → row 0
    wtree.press_key(Key::ArrowDown, Modifiers::NONE); // → row 1
    assert_eq!(selection.selected_indices(), vec![1]);
    assert_eq!(activated.get(), None);

    wtree.press_key(Key::Space, Modifiers::NONE); // toggle row 1 OFF
    assert!(selection.selected_indices().is_empty(), "Space toggles off");
    assert_eq!(activated.get(), None, "Space must NOT activate");

    wtree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(activated.get(), Some(1), "Enter activates");
}

/// A per-row composite tooltip opens against the row under the pointer, and
/// carries that row's own content.
///
/// The app never sees the row widget — the view builds it from the delegate —
/// so the view resolves the tooltip from the item and attaches it itself.
#[test]
fn row_composite_tooltip_opens_for_the_hovered_row() {
    use crate::primitives::TextWidget;
    use std::time::Duration;
    use teksilo_i18n::lit;

    let tree = TreeModel::new();
    for i in 0..5 {
        tree.insert_root(i, format!("Node {i}"));
    }
    let mut wtree = WidgetTree::new().with_text_backend(Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let tv = wtree.add(
        TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
            .item_height(20.0)
            .row_composite_tooltip(|_i, item: &String| {
                Some(Box::new(TextWidget::new(lit!(format!("about {item}")))) as Box<dyn Widget>)
            }),
    );
    wtree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(wtree.active_overlays().is_empty());

    // Hover row 2 (rows are 20 dp tall, so its centre sits at y = 50).
    let bounds = wtree.bounds(tv);
    wtree.pointer_move(teksilo_canvas::Point::new(bounds.x + 40.0, bounds.y + 50.0));
    assert!(
        wtree.active_overlays().is_empty(),
        "a composite row tip waits out the heavy delay, like any other"
    );

    wtree.advance_time(Duration::from_millis(700) + Duration::from_millis(50));
    assert_eq!(
        wtree.active_overlays().len(),
        1,
        "the hovered row's tooltip opens once the pointer has paused"
    );
    assert!(
        wtree.find_by_label("about Node 2").is_some(),
        "and it carries the hovered row's own content, not another row's"
    );
}

/// The same thing with a **real** `StandardTreeItem` row, not a dummy leaf.
///
/// A real row reacts to hover (background, focus ring), and if that reaction
/// rebuilds the row rather than merely repainting it, every pointer move mints
/// a fresh anchor and a fresh tooltip entry whose delay starts from zero — so
/// the tip can never ripen and never appears. A dummy leaf has no hover
/// behaviour at all, which is precisely why it cannot catch that.
#[test]
fn row_composite_tooltip_opens_on_a_real_standard_tree_item() {
    use crate::primitives::TextWidget;
    use crate::standard_item::StandardTreeItem;
    use std::time::Duration;
    use teksilo_i18n::lit;

    let tree = TreeModel::new();
    for i in 0..5 {
        tree.insert_root(i, format!("Node {i}"));
    }
    let mut wtree = WidgetTree::new().with_text_backend(Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let tv = wtree.add(
        TreeView::new(tree, |item: &String, entry, selected| {
            Box::new(
                StandardTreeItem::new(lit!(item.clone()))
                    .depth(entry.depth)
                    .selected(selected),
            )
        })
        .item_height(20.0)
        .row_composite_tooltip(|_i, item: &String| {
            Some(Box::new(TextWidget::new(lit!(format!("about {item}")))) as Box<dyn Widget>)
        }),
    );
    wtree.layout(SizeProposal::exact(400.0, 200.0));

    let bounds = wtree.bounds(tv);
    let at = teksilo_canvas::Point::new(bounds.x + 40.0, bounds.y + 50.0);
    wtree.pointer_move(at);
    // Nudge inside the stationary slop, the way a real hand does.
    wtree.pointer_move(teksilo_canvas::Point::new(at.x + 1.0, at.y));
    wtree.advance_time(Duration::from_millis(750));

    assert_eq!(
        wtree.active_overlays().len(),
        1,
        "a real row's hover reaction must not keep restarting the tooltip delay"
    );
}

/// A resolver returning `None` leaves that row without a tip — the row-by-row
/// equivalent of not calling a setter at all.
#[test]
fn a_row_whose_resolver_returns_none_has_no_tooltip() {
    use std::time::Duration;

    let tree = TreeModel::new();
    for i in 0..5 {
        tree.insert_root(i, format!("Node {i}"));
    }
    let mut wtree = WidgetTree::new().with_text_backend(Rc::new(std::cell::RefCell::new(
        teksilo_canvas::MockTextBackend::new(),
    )));
    let tv = wtree.add(
        TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
            .item_height(20.0)
            .row_composite_tooltip(|_i, _item: &String| None),
    );
    wtree.layout(SizeProposal::exact(400.0, 200.0));

    let bounds = wtree.bounds(tv);
    wtree.pointer_move(teksilo_canvas::Point::new(bounds.x + 40.0, bounds.y + 50.0));
    wtree.advance_time(Duration::from_millis(900));

    assert!(
        wtree.active_overlays().is_empty(),
        "no resolver result means no tooltip on that row"
    );
}

#[test]
fn type_ahead_jumps_to_matching_visible_row() {
    use teksilo_core::event::{Key, Modifiers};
    let tree = TreeModel::new();
    tree.insert_root(0, "Apple".to_string());
    tree.insert_root(1, "Banana".to_string());
    tree.insert_root(2, "Cherry".to_string());
    tree.insert_root(3, "Date".to_string());
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
    let sel = selection.clone();
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
            .item_height(20.0)
            .selection(sel)
            .type_ahead_label(|s: &String| s.clone()),
    );
    wtree.layout(SizeProposal::exact(400.0, 200.0));
    wtree.focus(tv);
    selection.select(0);

    wtree.press_key(Key::C, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![2], "'c' → Cherry");
    wtree.press_key(Key::B, Modifiers::NONE);
    // "cb" matches nothing → selection unchanged.
    assert_eq!(
        selection.selected_indices(),
        vec![2],
        "'cb' no match, stays"
    );
}

// --- Chevron-vs-selection regression tests ---

/// A `TreeView` whose rows are real `StandardTreeItem`s (with a live chevron)
/// over `sample_tree()`, plus a single-select model. The chevron is the only
/// toggle target (`row_click_expands(false)`), mirroring app usage.
fn make_standard_tree_view() -> (WidgetTree, WidgetId, teksilo_data::SelectionModel) {
    use teksilo_data::{SelectionMode, SelectionModel};
    let tree = sample_tree();
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel_clone = selection.clone();
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new_with_context(tree, |item: &&'static str, entry, selected, ctx| {
            Box::new(
                crate::StandardTreeItem::new(lit!((*item).to_string()))
                    .from_entry(entry)
                    .selected(selected)
                    .on_toggle_rc(ctx.toggle_callback()),
            ) as Box<dyn Widget>
        })
        .item_height(28.0)
        .selection(sel_clone)
        .row_click_expands(false),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    (wtree, tv_id, selection)
}

fn press_at(w: &mut WidgetTree, x: f32, y: f32) {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    w.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(x, y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    w.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(x, y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
}

#[test]
fn chevron_press_toggles_without_selecting_the_row() {
    // Regression: pressing the expand chevron must toggle the subtree but
    // NOT select the row. The row's select-on-press handler yields to the
    // chevron's own tap via `ctx.press_claimed_by_interactive_child()`.
    let (mut wtree, tv_id, selection) = make_standard_tree_view();
    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        3,
        "precondition: 3 collapsed roots"
    );

    // Row A is depth 0 (indent 0); the chevron column is x in [0, 16].
    press_at(&mut wtree, 8.0, 14.0);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        5,
        "chevron press should expand A, revealing A1 and A2"
    );
    assert!(
        selection.selected_indices().is_empty(),
        "chevron press must not select the row (got {:?})",
        selection.selected_indices()
    );
}

#[test]
fn body_press_selects_the_row() {
    // Companion: pressing the row BODY (past the chevron column) still
    // selects, and does not expand when row_click_expands=false.
    let (mut wtree, tv_id, selection) = make_standard_tree_view();

    press_at(&mut wtree, 100.0, 14.0);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "body press should select row 0"
    );
    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        3,
        "body press must not expand when row_click_expands=false"
    );
}

/// Like [`make_standard_tree_view`] (real `StandardTreeItem` chevrons) but
/// **reorderable**, so each row also owns a drag recognizer — the exact shape
/// where a chevron tap and an ancestor row drag compete.
fn make_reorderable_standard_tree_view() -> (WidgetTree, WidgetId) {
    let tree = sample_tree();
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new_with_context(tree, |item: &&'static str, entry, selected, ctx| {
            Box::new(
                crate::StandardTreeItem::new(lit!((*item).to_string()))
                    .from_entry(entry)
                    .selected(selected)
                    .on_toggle_rc(ctx.toggle_callback()),
            ) as Box<dyn Widget>
        })
        .item_height(28.0)
        .row_click_expands(false)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    (wtree, tv_id)
}

#[test]
fn chevron_tap_with_jitter_toggles_in_a_reorderable_tree() {
    // Regression: the expand chevron sits inside a reorderable row that owns a
    // drag recognizer. Tap and drag share a 5px threshold — a tap fails only
    // once movement is *strictly* past 5px, while a drag arms at exactly 5px.
    // So a press that drifts to exactly the threshold is still a valid tap,
    // yet — unless the chevron is a gesture dead zone — that drift arms the
    // ancestor row drag, which steals the gesture: the toggle never fires and
    // a row drag starts instead (the "click chevron → new drag" bug). With the
    // dead zone, the ancestor drag is never armed, so the tap wins and toggles.
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let (mut wtree, tv_id) = make_reorderable_standard_tree_view();
    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        3,
        "precondition: 3 collapsed roots"
    );

    // Row A is depth 0; the chevron column is x in [0, 16], row y in [0, 28].
    // Down, drift to exactly 5px (arms an ancestor drag but keeps the tap
    // alive), then release back within tolerance — a valid tap that the drag
    // must not steal.
    wtree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(8.0, 10.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(8.0, 15.0), // exactly 5px from down
    });
    wtree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(8.0, 13.0), // 3px from down → within tap tolerance
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        5,
        "the chevron tap must expand A (revealing A1, A2), not start a row drag"
    );
}

// --- Drag-and-drop integration tests ---

/// Run a full drag gesture: PointerDown on source, Move to cross threshold,
/// Move to target, Up. Mirrors `list_view::tests::drag_item`.
fn drag_item(tree: &mut WidgetTree, from: Point, to: Point) {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(from.x + 10.0, from.y),
    });
    tree.dispatch_event(WidgetEvent::PointerMove { position: to });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: to,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
}

/// Build a reorderable TreeView at the tree root with three top-level
/// nodes A (collapsed, with A1/A2 children), B (collapsed, with B1), C
/// (leaf). Item height is 28px, so rows are at y=0..28, 28..56, 56..84.
fn make_reorderable_tree_view() -> (
    WidgetTree,
    WidgetId,
    TreeModel<&'static str>,
    NodeId,
    NodeId,
    NodeId,
) {
    let model = sample_tree();
    let a = model.root(0);
    let b = model.root(1);
    let c = model.root(2);
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(model.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .reorderable(true),
    );
    (wtree, tv_id, model, a, b, c)
}

#[test]
fn drag_reorders_root_before() {
    // Drag C (row 2, y=56..84) to the top third of row 0 (before A).
    let (mut wtree, _tv_id, model, a, _b, c) = make_reorderable_tree_view();
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 2.0));

    // After move: C becomes root 0, A shifts to root 1.
    assert_eq!(model.root(0), c, "C should be first root");
    assert_eq!(model.root(1), a, "A should be second root");
}

#[test]
fn drag_reorders_root_after() {
    // Drag B (row 1, y=28..56) to the bottom third of row 2 (after C).
    let (mut wtree, _tv_id, model, _a, b, c) = make_reorderable_tree_view();
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    drag_item(&mut wtree, Point::new(50.0, 42.0), Point::new(50.0, 80.0));

    // After move: order is [A, C, B]
    assert_eq!(model.root_count(), 3);
    assert_eq!(model.root(1), c, "C should shift up to root 1");
    assert_eq!(model.root(2), b, "B should land at root 2");
}

#[test]
fn drag_reparents_into_target() {
    // Drag C (row 2) into the middle third of row 0 (into A as last child —
    // drop-into appends, the standard folder convention).
    let (mut wtree, _tv_id, model, a, _b, c) = make_reorderable_tree_view();
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Middle third of a 28px row is [9.33, 18.67]. Use y=14.
    drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 14.0));

    // C should now be A's last child (A's existing children were A1, A2).
    let a_children = model.children(a);
    assert_eq!(a_children.len(), 3, "A should have three children");
    assert_eq!(a_children[2], c, "C should be A's last child");
    // C is no longer a root.
    assert_eq!(model.root_count(), 2);
}

#[test]
fn drag_into_reparents_the_node() {
    // Drag C onto the middle third of A's row → C is reparented under A
    // (the move is applied via the cycle-guarded reorder helper).
    let model = sample_tree();
    let a = model.root(0);
    let c = model.root(2);
    let mut wtree = WidgetTree::new();
    let _tv = wtree.add(
        TreeView::new(model.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Drag C (root 2, y≈70) into the middle third of row 0 ("into A").
    drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 14.0));

    assert_eq!(model.root_count(), 2, "C is no longer a root");
    assert_eq!(model.parent(c), Some(a), "C is now a child of A");
}

#[test]
fn drag_into_own_descendant_is_refused_without_panicking() {
    // The cycle guard: dragging A into its own child A1 must be refused —
    // no move, and (critically) no panic in TreeModel::move_node.
    let model = sample_tree();
    let a = model.root(0);
    let a1 = model.children(a)[0];
    let tv = TreeView::new(model.clone(), |_item, entry, _sel| {
        Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
    })
    .item_height(28.0)
    .reorderable(true);
    // Expand A so A1 is a visible row before the drag.
    tv.expand(a);
    let mut wtree = WidgetTree::new();
    let _tv = wtree.add(tv);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Rows: A(0), A1(1), A2(2), B(3), C(4). Drag A (row 0, y≈14) into the
    // middle third of A1 (row 1, y≈42).
    drag_item(&mut wtree, Point::new(50.0, 14.0), Point::new(50.0, 42.0));

    // Refused: A is still a root, A1 still A's child. No panic occurred.
    assert_eq!(model.root_count(), 3, "A unchanged (cycle refused)");
    assert_eq!(model.parent(a1), Some(a), "A1 still under A");
}

#[test]
fn drag_emits_node_moved_change() {
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_data::TreeChange;

    let (mut wtree, _tv_id, model, _a, b, _c) = make_reorderable_tree_view();
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let emitted = Rc::new(Cell::new(false));
    let e = emitted.clone();
    let moved_node = Rc::new(Cell::new(None::<NodeId>));
    let mn = moved_node.clone();
    let handle = model.observe_changes(move |change| {
        if let TreeChange::NodeMoved { node, .. } = change {
            e.set(true);
            mn.set(Some(*node));
        }
    });

    // Drag B up — before A.
    drag_item(&mut wtree, Point::new(50.0, 42.0), Point::new(50.0, 2.0));

    assert!(emitted.get(), "TreeChange::NodeMoved should be emitted");
    assert_eq!(moved_node.get(), Some(b));
    drop(handle);
}

#[test]
fn click_on_branch_with_nested_delegate_expands() {
    // Like click_on_branch_expands_and_collapses, but the delegate
    // builds a nested subtree (ZStack + Padding + HStack + Texts +
    // Spacer) so the pointer hit-target is a deep leaf, NOT the
    // TreeItemWrapper. Regression for the case where the wrapper's
    // on_pointer_event has to route through the preview/bubble path
    // to fire toggle_expand.
    use crate::RectWidget;
    use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};

    let tree = sample_tree();
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |name, entry, selected| {
            let arrow: &'static str = if entry.has_children {
                if entry.is_expanded { "v" } else { ">" }
            } else {
                " "
            };
            let bg = if selected {
                teksilo_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25)
            } else {
                teksilo_tokens::Color::TRANSPARENT
            };
            Box::new(
                ZStack::new().child(RectWidget::new().background(bg)).child(
                    Padding::symmetric(4.0, 12.0).child(
                        HStack::new()
                            .spacing(8.0)
                            .child(TextWidget::new(lit!(arrow)))
                            .child(TextWidget::new(lit!(name.to_string())))
                            .child(Spacer::new()),
                    ),
                ),
            )
        })
        .item_height(28.0),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Sanity check: 3 roots visible.
    assert_eq!(row_ids(&wtree, tv_id).len(), 3);

    // Click A (row 0). Use the wrapper's bounds center — hit_test will
    // walk down to whatever deep leaf is at that point.
    let children = row_ids(&wtree, tv_id);
    wtree.click(children[0]);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        5,
        "Click on A (branch) should expand it even with a nested delegate"
    );
}

#[test]
fn drag_with_nested_delegate_still_works() {
    // Same nested delegate as above, but exercising drag. Regression
    // for the real-app scenario where the pointer hit-target is a
    // deep leaf (TextWidget) and the wrapper holding the gesture
    // arena + on_drag is an ancestor.
    use crate::RectWidget;
    use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};

    let tree = sample_tree();
    let a = tree.root(0);
    let c = tree.root(2);
    let mut wtree = WidgetTree::new();
    let _tv_id = wtree.add(
        TreeView::new(tree.clone(), |name, _entry, _sel| {
            Box::new(
                ZStack::new()
                    .child(RectWidget::new().background(teksilo_tokens::Color::TRANSPARENT))
                    .child(
                        Padding::symmetric(4.0, 12.0).child(
                            HStack::new()
                                .spacing(8.0)
                                .child(TextWidget::new(lit!(name.to_string())))
                                .child(Spacer::new()),
                        ),
                    ),
            )
        })
        .item_height(28.0)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Drag C (row 2, y=70) to the top third of row 0 (y=2) → drop-before A.
    drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 2.0));

    assert_eq!(tree.root(0), c, "C should be first root after drag");
    assert_eq!(tree.root(1), a, "A should shift to second root");
}

#[test]
fn click_on_branch_expands_and_collapses() {
    // Click a folder-with-children and verify its subtree appears; click
    // again and verify it collapses. Regression test for the previous
    // on_pointer_event double-dispatch bug that toggled expand twice per
    // click (net no-op).
    let tree = sample_tree();
    let (mut wtree, tv_id) = make_tree_view(tree);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Initially collapsed — 3 roots visible.
    assert_eq!(row_ids(&wtree, tv_id).len(), 3);

    // Click A (row 0, center y=14).
    let children = row_ids(&wtree, tv_id);
    wtree.click(children[0]);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // A should now be expanded, showing its two children A1, A2.
    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        5,
        "After clicking A, its two children should become visible"
    );

    // Click A again — collapses.
    let children = row_ids(&wtree, tv_id);
    wtree.click(children[0]);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        3,
        "Second click should collapse A back to 3 visible roots"
    );
}

#[test]
fn row_click_expands_false_disables_auto_toggle() {
    // With `.row_click_expands(false)` set, clicking a branch
    // row's body must NOT toggle its expansion. This is the
    // contract used by `StandardTreeItem`, which provides its
    // own chevron tap target — without this opt-out, body clicks
    // would still toggle (and chevron clicks would toggle twice,
    // cancelling out).
    let tree = sample_tree();
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |_item, entry, _selected| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .row_click_expands(false),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(row_ids(&wtree, tv_id).len(), 3);

    // Click A (a branch with children). Body click should NOT
    // expand it.
    let children = row_ids(&wtree, tv_id);
    wtree.click(children[0]);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        3,
        "row body click on a branch must not auto-expand when row_click_expands=false"
    );
}

#[test]
fn spring_loaded_folder_expands_after_dwell() {
    // Drag a leaf over a collapsed folder and hold. After the dwell
    // delay (SPRING_DELAY_MS = 700 real ms), the folder should
    // auto-expand. Test drives real wall-clock time via `sleep` —
    // it's slow but accurate. Runs in ~750 ms; still headless.
    use std::thread::sleep;
    use std::time::Duration;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let tree = sample_tree(); // A (A1 A2), B (B1), C (leaf)
    let a = tree.root(0);
    let b = tree.root(1);
    let mut wtree = WidgetTree::new();
    let _tv_id = wtree.add(
        TreeView::new(tree.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Start a drag on C (y=70, row 2), then hover over B (row 1, y=42).
    wtree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(50.0, 70.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(60.0, 70.0),
    });
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(60.0, 42.0),
    });

    // Confirm B is currently collapsed.
    assert!(tree.with_item(b, |_| ()).is_some());
    assert_eq!(
        row_ids(&wtree, _tv_id).len(),
        3,
        "Precondition: 3 visible roots, nothing expanded"
    );

    // Wait past the 700 ms spring delay, then drive a layout tick
    // so on_drag_tick fires and the elapsed check passes.
    sleep(Duration::from_millis(750));
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // B should now be expanded, revealing B1 (4 visible rows).
    assert_eq!(
        row_ids(&wtree, _tv_id).len(),
        4,
        "B should have spring-opened after the dwell"
    );

    // A was never hovered — still collapsed.
    assert!(!row_ids(&wtree, _tv_id).is_empty());
    let _ = a;

    // Clean up drag.
    wtree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(60.0, 42.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
}

#[test]
fn spring_loaded_folder_expand_then_drop_moves_the_originally_dragged_node() {
    // End-to-end pin of the drag-key-stash fix, through the real widget
    // event path (`spring_loaded_folder_expands_after_dwell` above stops
    // at the expand — this completes the gesture with a drop). Mirrors
    // `tree_source.rs`'s `a_reorder_moves_the_node_dragged_not_the_slot_it_left`,
    // which pins the same scenario at the erasure level.
    //
    // Grab leaf C (flat index 2), hover collapsed folder B (index 1) long
    // enough to spring it open — inserting B1 between B and C, which
    // shifts C from flat index 2 to 3 mid-drag — then drop. The
    // ORIGINALLY dragged node (C) must move; a stale-index bug would
    // instead move whichever node the reflow slid into index 2 (B1).
    use std::thread::sleep;
    use std::time::Duration;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let tree = sample_tree(); // A (A1 A2), B (B1), C (leaf)
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Start a drag on C (y=70, row 2), then hover over B (row 1, y=42).
    wtree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(50.0, 70.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(60.0, 70.0),
    });
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(60.0, 42.0),
    });
    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        3,
        "Precondition: 3 visible roots, nothing expanded"
    );

    // Wait past the spring delay and tick — B auto-expands, reflowing the
    // flat index space out from under the drag.
    sleep(Duration::from_millis(750));
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        row_ids(&wtree, tv_id).len(),
        4,
        "B should have spring-opened after the dwell"
    );

    // Move to the top third of row 0 (A) — DropPosition::Before — and
    // release: drop the dragged node before A.
    let drop_at = Point::new(60.0, 2.0);
    wtree.dispatch_event(WidgetEvent::PointerMove { position: drop_at });
    wtree.dispatch_event(WidgetEvent::PointerUp {
        position: drop_at,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    assert_eq!(
        tree.with_item(tree.root(0), |&v| v),
        Some("C"),
        "the ORIGINALLY dragged node (C) must land at the drop target, \
         not whichever node the mid-drag spring-load reflow shifted into \
         its old flat index"
    );
}

// --- Alt+Arrow keyboard reorder test ---

#[test]
fn alt_arrow_reorders_flat_root_sibling() {
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    let model = sample_tree(); // A, B, C (3 roots)
    let _a = model.root(0);
    let _b = model.root(1);
    let _c = model.root(2);
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel_clone = selection.clone();
    let model_clone = model.clone();

    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(model_clone, move |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .selection(sel_clone)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Focus the TreeView and select the middle item (B)
    wtree.focus(tv_id);
    wtree.click(row_ids(&wtree, tv_id)[1]); // B at index 1
    assert_eq!(selection.selected_indices(), vec![1]);

    // Press Alt+ArrowUp: B should move above A
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowUp,
        modifiers: Modifiers::ALT,
        text: None,
    });

    // After move: the roots should be reordered as B, A, C
    let new_roots: Vec<NodeId> = (0..model.root_count()).map(|i| model.root(i)).collect();
    assert_eq!(
        model.with_item(new_roots[0], |&v| v),
        Some("B"),
        "B should now be first root"
    );
    assert_eq!(
        model.with_item(new_roots[1], |&v| v),
        Some("A"),
        "A should now be second root"
    );
    // Selection should follow the moved node
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Selection should now be at index 0 (B moved to top)"
    );

    // Press Alt+ArrowDown on B: B should move below A
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::ALT,
        text: None,
    });

    // After move: order should be A, B, C again
    let new_roots: Vec<NodeId> = (0..model.root_count()).map(|i| model.root(i)).collect();
    assert_eq!(
        model.with_item(new_roots[0], |&v| v),
        Some("A"),
        "A should be back at first root"
    );
    assert_eq!(
        model.with_item(new_roots[1], |&v| v),
        Some("B"),
        "B should be back at second root"
    );
    assert_eq!(
        selection.selected_indices(),
        vec![1],
        "Selection should be back at index 1"
    );
}

#[test]
fn alt_arrow_reorders_nested_sibling() {
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    // Tree: A with children A1, A2 (in that order)
    let tree = TreeModel::new();
    let a = tree.insert_root(0, "A");
    let _a1 = tree.insert_child(a, 0, "A1");
    let _a2 = tree.insert_child(a, 1, "A2");
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel_clone = selection.clone();
    let model = tree.clone();

    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(model, |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .selection(sel_clone)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Focus the TreeView so ArrowRight expands the focused node (A)
    wtree.focus(tv_id);

    // Expand A so children are visible
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::NONE,
        text: None,
    });
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Select A2 (flat index 2: A at 0, A1 at 1, A2 at 2)
    let children = row_ids(&wtree, tv_id);
    wtree.click(children[2]);
    assert_eq!(selection.selected_indices(), vec![2]);

    // Press Alt+ArrowUp: A2 should move above A1
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowUp,
        modifiers: Modifiers::ALT,
        text: None,
    });
    // After move, relayout to refresh the tree view
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Check model: A2 should now be at index 0 under A, A1 at index 1
    let children_of_a = tree.children(a);
    assert_eq!(children_of_a.len(), 2, "A should still have 2 children");
    assert_eq!(
        tree.with_item(children_of_a[0], |&v| v),
        Some("A2"),
        "A2 should now be first child of A"
    );
    assert_eq!(
        tree.with_item(children_of_a[1], |&v| v),
        Some("A1"),
        "A1 should now be second child of A"
    );

    // Selection should now be at flat index 1 (A2 moved up, now at position 1)
    assert_eq!(
        selection.selected_indices(),
        vec![1],
        "Selection should follow A2 to flat index 1"
    );
}

#[test]
fn alt_arrow_cannot_move_past_boundaries() {
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    let model = sample_tree(); // A, B, C (3 roots)
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel_clone = selection.clone();
    let model_clone = model.clone();

    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(model_clone, move |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .selection(sel_clone)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Focus and select first item (A)
    wtree.focus(tv_id);
    wtree.click(row_ids(&wtree, tv_id)[0]);

    let a = model.root(0);
    let c = model.root(2);

    // Alt+ArrowUp on first item should do nothing
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowUp,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(
        model.with_item(a, |&v| v),
        Some("A"),
        "A should still be first after Alt+Up on first item"
    );

    // Select last item (C)
    wtree.click(row_ids(&wtree, tv_id)[2]);

    // Alt+ArrowDown on last item should do nothing
    wtree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(
        model.with_item(c, |&v| v),
        Some("C"),
        "C should still be last after Alt+Down on last item"
    );
}

// -- Boundary scroll chaining -------------------------------------------

/// A TreeView of 40 flat roots (20px each → 800px) in a 100px viewport,
/// above a filler inside an outer ScrollArea. TreeView doesn't expose its
/// scroll signal, so chaining is observed via the outer: the inner
/// absorbing the first (huge) scroll leaves the outer at 0 (the
/// anti-trivial guard), and the clamped second scroll then moves the
/// outer under `Chain` but not under `Contain`.
fn nested_tree_fixture(inner: OverscrollBehavior) -> (WidgetTree, Signal<f32>) {
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, VStack};
    let model = TreeModel::new();
    for i in 0..40 {
        model.insert_root(i, i as i32);
    }
    let mut tree = WidgetTree::new();
    let tv = TreeView::new(model, |_item: &i32, _entry, _sel| {
        Box::new(FixedLeaf(180.0, 20.0))
    })
    .item_height(20.0)
    .overscroll_behavior(inner);
    let tv_id = tree.add(tv);
    let viewport = tree.add(FixedSize::new().width(200.0).height(100.0).child_id(tv_id));
    let filler = tree.add(FixedLeaf(200.0, 200.0));
    let outer_content = tree.add(VStack::new().add_child(viewport).add_child(filler));
    let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let _outer = tree.add(outer);
    tree.layout(SizeProposal::exact(200.0, 150.0));
    (tree, outer_y)
}

#[test]
fn nested_tree_chains_to_outer_at_boundary() {
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, outer_y) = nested_tree_fixture(OverscrollBehavior::Chain);
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal::exact(200.0, 150.0));
    // The inner tree absorbed the big scroll (didn't chain) → outer at 0.
    assert!(
        outer_y.get() < 0.01,
        "outer must not move while the inner absorbs"
    );

    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal::exact(200.0, 150.0));
    assert!(
        outer_y.get() > 0.01,
        "outer scrolled because the clamped tree chained"
    );
}

#[test]
fn nested_tree_contain_blocks_chaining() {
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, outer_y) = nested_tree_fixture(OverscrollBehavior::Contain);
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal::exact(200.0, 150.0));
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal::exact(200.0, 150.0));
    assert!(
        outer_y.get() < 0.01,
        "Contain must prevent chaining: outer stays put"
    );
}

#[test]
fn keyboard_selection_chases_outer_scroll_area() {
    // A 200px TreeView (20px rows) whose lower half is below a 100px outer
    // ScrollArea's fold. Arrow-key navigation keeps focus on the container
    // (rows aren't focusable), so the focus-driven follow can't reveal the
    // selected row — ctx.ensure_visible must.
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, VStack};
    use teksilo_core::event::{Key, Modifiers};

    let model = TreeModel::new();
    for i in 0..20 {
        model.insert_root(i, i as i32);
    }
    let mut tree = WidgetTree::new();
    let tv = TreeView::new(model, |_item: &i32, _entry, _sel| {
        Box::new(FixedLeaf(180.0, 20.0))
    })
    .item_height(20.0);
    let tv_id = tree.add(tv);
    let tv_box = tree.add(FixedSize::new().width(200.0).height(200.0).child_id(tv_id));
    let filler = tree.add(FixedLeaf(200.0, 200.0));
    let outer_content = tree.add(VStack::new().add_child(tv_box).add_child(filler));
    let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let _outer = tree.add(outer);
    tree.layout(SizeProposal::exact(200.0, 100.0));

    tree.focus(tv_id);
    tree.layout(SizeProposal::exact(200.0, 100.0));
    outer_y.set(0.0);
    tree.layout(SizeProposal::exact(200.0, 100.0));
    assert!(outer_y.get().abs() < 0.01, "reset outer to top");

    for _ in 0..20 {
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
    }
    tree.layout(SizeProposal::exact(200.0, 100.0));
    assert!(
        outer_y.get() > 0.01,
        "arrow-navigating below the fold must scroll the enclosing ScrollArea (got {})",
        outer_y.get()
    );
}

// --- Variable row heights ---

/// Collect the (y, height) bounds of the realized rows (the
/// rows are the body pane's children), sorted by y.
fn row_spans(tree: &WidgetTree, tv_id: WidgetId) -> Vec<(f32, f32)> {
    let children = row_ids(tree, tv_id);
    let mut spans: Vec<(f32, f32)> = children[..]
        .iter()
        .map(|c| {
            let b = tree.bounds(*c);
            (b.y, b.height)
        })
        .collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    spans
}

#[test]
fn exact_item_height_fn_positions_tree_rows() {
    let tree = sample_tree();
    let heights = [60.0_f32, 20.0, 40.0];
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 28.0)))
            .item_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0)),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let spans = row_spans(&wtree, tv_id);
    assert_eq!(spans.len(), 3);
    assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 60.0).abs() < 0.01);
    assert!((spans[1].0 - 60.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
    assert!((spans[2].0 - 80.0).abs() < 0.01 && (spans[2].1 - 40.0).abs() < 0.01);
}

#[test]
fn auto_measure_tree_rows_at_measured_heights() {
    // Delegate rows are 30 px tall; estimate says 50 → row 1 must
    // settle at y = 30.
    let tree = sample_tree();
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 30.0)))
            .auto_item_height(50.0),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let spans = row_spans(&wtree, tv_id);
    assert!(
        (spans[1].0 - 30.0).abs() < 0.01,
        "row 1 should sit at measured 30, got {}",
        spans[1].0
    );
}

#[test]
fn expand_preserves_measured_heights_above_toggle() {
    // Rows measure 30 (estimate 50). Expanding B (flat index 1) must
    // keep A's measured height — row B stays at y = 30, it doesn't
    // snap back to the estimate.
    let tree = sample_tree();
    let b = tree.root(1);
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 30.0)))
            .auto_item_height(50.0)
            .row_click_expands(false),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    wtree
        .widget_as_any(tv_id)
        .and_then(|any| any.downcast_ref::<TreeView<&'static str>>())
        .expect("TreeView exposes itself via as_any")
        .expand(b);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    let spans = row_spans(&wtree, tv_id);
    assert_eq!(spans.len(), 4); // A, B, B1, C
    assert!(
        (spans[1].0 - 30.0).abs() < 0.01,
        "A's measured height must survive the expand below it, got {}",
        spans[1].0
    );
}

#[test]
fn drop_zone_thirds_with_variable_heights() {
    // Roots A (60 px), B (20 px), C (40 px), reorderable. Dropping C
    // in the top third of the SHORT row B (y ∈ 60..~66) must insert
    // it before B — uniform math would misattribute that y band.
    let tree = TreeModel::new();
    tree.insert_root(0, "A");
    tree.insert_root(1, "B");
    tree.insert_root(2, "C");
    let heights = [60.0_f32, 20.0, 40.0];
    let mut wtree = WidgetTree::new();
    let _tv_id = wtree.add(
        TreeView::new(tree.clone(), |_item, _entry, _sel| {
            Box::new(FixedLeaf(100.0, 28.0))
        })
        .item_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0))
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // C spans 80..120; grab its center. Drop at y = 62: row B's top
    // third (60..60+20/3).
    drag_item(&mut wtree, Point::new(50.0, 100.0), Point::new(50.0, 62.0));

    let order: Vec<&str> = (0..tree.root_count())
        .map(|i| tree.with_item(tree.root(i), |v| *v).unwrap())
        .collect();
    assert_eq!(order, vec!["A", "C", "B"]);
}

#[test]
fn keyed_selection_tracks_identity_and_prunes_deleted() {
    // Keyed (identity) selection by NodeId: selecting nodes stores their
    // ids, and deleting a node prunes only that key on the next slice
    // change — the other selected node survives.
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let model = sample_tree();
    let a = model.root(0);
    let a1 = model.children(a)[0];
    let b = model.root(1);
    let b1 = model.children(b)[0];
    let keyed = KeyedSelectionModel::<NodeId>::new(SelectionMode::Multi);
    let mut wtree = WidgetTree::new();
    wtree.add(
        TreeView::new(model.clone(), |_item, _entry, _sel| {
            Box::new(FixedLeaf(100.0, 28.0))
        })
        .item_height(28.0)
        .keyed_selection(keyed.clone()),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    keyed.select(a1);
    keyed.toggle(b1);
    assert!(keyed.is_selected(&a1) && keyed.is_selected(&b1));

    // Delete A1 → the slice reflattens (version bump) and prune drops the
    // orphaned key; B1 (still present) survives.
    model.remove(a1);
    assert!(
        !keyed.is_selected(&a1),
        "deleted node is pruned from selection"
    );
    assert!(keyed.is_selected(&b1), "surviving node stays selected");
}

// ── Generic `TreeDataSource` path (Stage 8a) ─────────────────────────────
// An external source of truth keyed on `i64` (an entity id), driving a
// `TreeView<String>` with NO `TreeModel` mirror — the designer's case.

struct MockNode {
    id: i64,
    parent: Option<i64>,
    label: String,
}

/// Minimal in-memory `TreeDataSource<Item = String, Key = i64>`. Nodes are
/// stored in pre-order; visibility is derived from the expand set.
struct MockI64Source {
    nodes: RefCell<Vec<MockNode>>,
    expanded: RefCell<std::collections::HashSet<i64>>,
    version: Signal<u64>,
    accept_log: RefCell<Vec<(i64, i64, DropPosition)>>,
}

impl MockI64Source {
    fn new() -> Self {
        // root1(1) { a(2), b(3) }   root2(4)
        let nodes = vec![
            MockNode {
                id: 1,
                parent: None,
                label: "root1".into(),
            },
            MockNode {
                id: 2,
                parent: Some(1),
                label: "a".into(),
            },
            MockNode {
                id: 3,
                parent: Some(1),
                label: "b".into(),
            },
            MockNode {
                id: 4,
                parent: None,
                label: "root2".into(),
            },
        ];
        Self {
            nodes: RefCell::new(nodes),
            expanded: RefCell::new([1, 4].into_iter().collect()),
            version: Signal::new(0),
            accept_log: RefCell::new(Vec::new()),
        }
    }
    fn parent_of(&self, id: i64) -> Option<i64> {
        self.nodes
            .borrow()
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.parent)
    }
    fn exists(&self, id: i64) -> bool {
        self.nodes.borrow().iter().any(|n| n.id == id)
    }
    fn is_descendant(&self, node: i64, ancestor: i64) -> bool {
        let mut cur = Some(node);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent_of(c);
        }
        false
    }
    fn visible_ids(&self) -> Vec<i64> {
        let nodes = self.nodes.borrow();
        let expanded = self.expanded.borrow();
        nodes
            .iter()
            .filter(|n| {
                // Visible iff every ancestor is expanded.
                let mut cur = n.parent;
                while let Some(p) = cur {
                    if !expanded.contains(&p) {
                        return false;
                    }
                    cur = nodes.iter().find(|m| m.id == p).and_then(|m| m.parent);
                }
                true
            })
            .map(|n| n.id)
            .collect()
    }
    fn depth_of(&self, id: i64) -> usize {
        let mut d = 0;
        let mut cur = self.parent_of(id);
        while let Some(p) = cur {
            d += 1;
            cur = self.parent_of(p);
        }
        d
    }
    fn remove(&self, id: i64) {
        self.nodes
            .borrow_mut()
            .retain(|n| n.id != id && n.parent != Some(id));
        self.bump();
    }
    fn bump(&self) {
        let v = self.version.get() + 1;
        self.version.set(v);
    }
}

impl TreeDataSource for MockI64Source {
    type Item = String;
    type Key = i64;
    fn visible_count(&self) -> usize {
        self.visible_ids().len()
    }
    fn with_entry<R>(
        &self,
        flat_index: usize,
        f: impl FnOnce(&String, &FlatEntry<i64>) -> R,
    ) -> Option<R> {
        let id = *self.visible_ids().get(flat_index)?;
        let entry = FlatEntry {
            node_id: id,
            depth: self.depth_of(id),
            has_children: self.nodes.borrow().iter().any(|n| n.parent == Some(id)),
            is_expanded: self.expanded.borrow().contains(&id),
        };
        let nodes = self.nodes.borrow();
        let label = &nodes.iter().find(|n| n.id == id)?.label;
        Some(f(label, &entry))
    }
    fn key_at(&self, flat_index: usize) -> Option<i64> {
        self.visible_ids().get(flat_index).copied()
    }
    fn flat_index_of(&self, key: &i64) -> Option<usize> {
        self.visible_ids().iter().position(|k| k == key)
    }
    fn parent(&self, key: &i64) -> Option<i64> {
        self.parent_of(*key)
    }
    fn child_keys(&self, key: &i64) -> Vec<i64> {
        self.nodes
            .borrow()
            .iter()
            .filter(|n| n.parent == Some(*key))
            .map(|n| n.id)
            .collect()
    }
    fn version_signal(&self) -> Signal<u64> {
        self.version.clone()
    }
    fn is_expanded(&self, key: &i64) -> bool {
        self.expanded.borrow().contains(key)
    }
    fn set_expanded(&self, key: &i64, expanded: bool) {
        if expanded {
            self.expanded.borrow_mut().insert(*key);
        } else {
            self.expanded.borrow_mut().remove(key);
        }
        self.bump();
    }
    fn contains_key(&self, key: &i64) -> bool {
        // Whole-tree existence (survives collapse), not visibility.
        self.exists(*key)
    }
    fn drag(&self, _key: &i64) -> DragEligibility {
        DragEligibility::CanDrag
    }
    fn can_accept(&self, query: &teksilo_data::DropQuery<'_, i64>) -> DropResponse {
        match &query.source {
            teksilo_data::DragSource::SameView { key: src } => {
                if *src == query.target || self.is_descendant(query.target, *src) {
                    DropResponse::Reject
                } else {
                    DropResponse::Accept
                }
            }
            teksilo_data::DragSource::Foreign { .. } => DropResponse::Reject,
        }
    }
    fn accept_drop(&self, commit: teksilo_data::DropCommit<'_, i64>) -> bool {
        let teksilo_data::DragSource::SameView { key: src } = commit.source else {
            return false;
        };
        if src == commit.target || self.is_descendant(commit.target, src) {
            return false;
        }
        self.accept_log
            .borrow_mut()
            .push((src, commit.target, commit.position));
        self.bump();
        true
    }
}

#[test]
fn from_source_row_scope_is_the_treeview_not_a_higher_ancestor() {
    // Reproduces the Skribisto shell: an outer focusable container holds the
    // binder TreeView and a sibling focusable ("the editor"). A row's focus
    // scope MUST be the TreeView — so when focus moves to the editor, the
    // row's scope goes inactive (selection mutes, focus ring clears). If the
    // scope resolved to the outer shell instead, it would stay active and the
    // ring would never clear.
    use crate::primitives::ZStack;
    use teksilo_core::widget_builder::WidgetBuilder;
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let source = Rc::new(MockI64Source::new());
    let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
    let mut tree = WidgetTree::new();
    let tv = tree.add(
        TreeView::from_source_keyed(
            MockI64Wrapper(source.clone()),
            keyed.clone(),
            |_l: &String, _r: &TreeRow, _s| Box::new(FixedLeaf(120.0, 24.0)),
        )
        .item_height(24.0),
    );
    let editor = tree.add(FixedLeaf(100.0, 24.0).focusable(true));
    // Outer shell holds both, and is itself focusable (like `App`).
    let _shell = tree.add(
        ZStack::new()
            .add_child(tv)
            .add_child(editor)
            .focusable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let rows = row_ids(&tree, tv);
    let scope = tree.view_focus_active_for(rows[0]);

    tree.focus(tv);
    assert!(scope.get(), "row scope active when the TreeView is focused");
    tree.focus(editor);
    assert!(
        !scope.get(),
        "focus moved to the sibling editor → the row's TreeView scope must go \
             inactive (so selection mutes and the focus ring clears)"
    );
}

#[test]
fn view_focus_active_tracks_view_focus_for_rows() {
    // Diagnostic for focus-aware selection: a row's focus scope (its nearest
    // focusable ancestor = the TreeView) must read inactive before focus and
    // active once a click focuses the view.
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let source = Rc::new(MockI64Source::new());
    let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
    let mut tree = WidgetTree::new();
    let tv = tree.add(
        TreeView::from_source_keyed(
            MockI64Wrapper(source.clone()),
            keyed.clone(),
            |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
        )
        .item_height(24.0)
        // Mirror Skribisto's binder: reorderable rows (drag recognizer) +
        // single-click activation (tap recognizer). These install gesture
        // arenas on each row — verify they don't preempt focusing the view.
        .reorderable(true)
        .activate_on(crate::data_views::ActivateOn::SingleClick)
        .on_activate(|_, _| {}),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let rows = row_ids(&tree, tv);
    assert_eq!(tree.focused(), None, "no focus yet");
    tree.click(rows[1]);
    // Clicking a row must move keyboard focus to the TreeView (or a focusable
    // descendant of it) — that is what makes focus-aware selection render
    // active. If this regresses, selected rows render with the muted
    // `SelectedInactive` chrome and look unselected.
    let focused = tree.focused();
    assert!(focused.is_some(), "clicking a row focuses something");
    assert!(
        focused == Some(tv) || tree.is_descendant_of(focused.unwrap(), tv),
        "focus landed inside the TreeView (got {focused:?}, tv = {tv:?})"
    );
}

#[test]
fn container_focus_ring_shows_when_tab_focused_without_selection() {
    // The reported gap: Tab into the tree with nothing selected and there is
    // no visible focus indicator — no row paints a ring because no row is
    // selected. The container focus ring fills it: paint outlines the whole
    // view when it holds keyboard focus (`view_focus_active`), the modality
    // is keyboard (`focus_visible`), and the selection is empty. This guards
    // those three paint inputs (paint output itself isn't unit-observable).
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let source = Rc::new(MockI64Source::new());
    let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
    let mut tree = WidgetTree::new();
    let tv = tree.add(
        TreeView::from_source_keyed(
            MockI64Wrapper(source.clone()),
            keyed.clone(),
            |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
        )
        .item_height(24.0),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view_focused = tree.view_focus_active_for(tv);
    let focus_visible = tree.focus_visible_signal();
    assert!(
        !view_focused.get() && !focus_visible.get(),
        "before focus: no container ring (not focused, pointer modality)"
    );

    // Tab in: focus the view under keyboard modality. `Tab` is ignored by the
    // tree's key handler, so the selection stays empty (no row ring).
    tree.focus(tv);
    tree.press_key(Key::Tab, Modifiers::NONE);
    assert!(view_focused.get(), "view holds keyboard focus");
    assert!(focus_visible.get(), "keyboard input → focus-visible");
    assert_eq!(
        keyed.count(),
        0,
        "nothing selected → no row ring, container ring shows"
    );

    // A pointer press flips modality off → container ring clears (matches the
    // row ring's `:focus-visible` rule; clicking never leaves a ring).
    tree.click(tv);
    assert!(
        !focus_visible.get(),
        "pointer input clears focus-visible → ring hides"
    );
}

#[test]
fn container_focus_ring_hidden_when_a_sibling_holds_focus() {
    // Regression: the container ring must track THIS view's own keyboard
    // focus, not a global signal. The view captured its focus signal at
    // build time; a plain `view_focus_active()` there found no focusable
    // ancestor (the root's `.focusable(true)` isn't wired into the arena
    // yet) and fell back to the constant-`true` "outside any scope" signal —
    // so every data view lit its container ring whenever ANY other widget
    // took keyboard focus. `begin_view_focus` keys the signal on the root
    // id and fixes it. This observes the painted ring (not just the signal,
    // which `view_focus_active_for` resolves correctly post-build).
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_core::widget_builder::WidgetBuilder;
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let source = Rc::new(MockI64Source::new());
    let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let root = tree.add(
        crate::primitives::VStack::new()
            .child(
                TreeView::from_source_keyed(
                    MockI64Wrapper(source.clone()),
                    keyed.clone(),
                    |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
                )
                .item_height(24.0),
            )
            // A focusable sibling that paints no chrome of its own.
            .child(FixedLeaf(40.0, 24.0).focusable(true)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let children = tree.children(root);
    let (tv, sibling) = (children[0], children[1]);

    let ring = teksilo_tokens::BorderRole::Focused
        .resolve(&teksilo_core::presets::intui::light().colors)
        .to_array();

    // The sibling holds focus under keyboard modality. It is NOT inside the
    // tree view, so the tree view's container ring must stay hidden even
    // though focus-visible is true and nothing is selected.
    tree.focus(sibling);
    tree.press_key(Key::ArrowDown, Modifiers::NONE); // sibling ignores it; flips focus-visible
    assert_eq!(tree.focused(), Some(sibling), "sibling holds focus");
    assert_eq!(keyed.count(), 0, "nothing selected");
    let frame = tree.render();
    assert!(
        !frame.decorations.iter().any(|d| d.color == ring)
            && !frame.shapes.iter().any(|s| s.color == ring)
            && !frame.cosmetic_lines.iter().any(|l| l.color == ring),
        "container ring must NOT paint while a sibling holds focus",
    );

    // Move focus to the tree view (programmatic — focus-visible stays true).
    // Now the view holds keyboard focus with no selection → the ring shows.
    tree.focus(tv);
    assert_eq!(tree.focused(), Some(tv), "tree view holds focus");
    assert_eq!(keyed.count(), 0, "still nothing selected");
    let frame = tree.render();
    assert!(
        frame.decorations.iter().any(|d| d.color == ring)
            || frame.shapes.iter().any(|s| s.color == ring)
            || frame.cosmetic_lines.iter().any(|l| l.color == ring),
        "container ring paints when the view holds keyboard focus",
    );
}

#[test]
fn from_source_keyed_i64_survives_collapse_and_prunes() {
    // A `TreeView<String>` driven by a generic `TreeDataSource<Key = i64>`
    // (no `TreeModel` mirror). Selection is keyed by the entity id: a click
    // stores the row's i64, a collapse keeps it (whole-tree existence), and
    // a delete prunes it.
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let source = Rc::new(MockI64Source::new());
    let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new();
    let tv = tree.add(
        TreeView::from_source_keyed(
            MockI64Wrapper(source.clone()),
            keyed.clone(),
            |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
        )
        .item_height(24.0),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Visible order is [1, 2, 3, 4]; row 1 is node "a" (id 2). Clicking it
    // must store the KEY 2, proving index→key translation + the render path.
    let rows = row_ids(&tree, tv);
    tree.click(rows[1]);
    assert!(
        keyed.is_selected(&2),
        "click stores the row's i64 key, not its index"
    );
    assert!(!keyed.is_selected(&1));

    // Collapse root1 → node 2 leaves the visible projection (version bump
    // runs the prune). It must survive — still present in the source.
    source.set_expanded(&1, false);
    assert!(
        keyed.is_selected(&2),
        "a collapsed-but-present i64 node keeps its selection"
    );

    // Delete node 2 → prune drops the now-missing key.
    source.remove(2);
    assert!(!keyed.is_selected(&2), "a deleted i64 node is pruned");
}

/// Newtype so we can hand the same `Rc<MockI64Source>` to the view while
/// keeping a handle for assertions (the view erases the source).
struct MockI64Wrapper(Rc<MockI64Source>);
impl TreeDataSource for MockI64Wrapper {
    type Item = String;
    type Key = i64;
    fn visible_count(&self) -> usize {
        self.0.visible_count()
    }
    fn with_entry<R>(&self, i: usize, f: impl FnOnce(&String, &FlatEntry<i64>) -> R) -> Option<R> {
        self.0.with_entry(i, f)
    }
    fn key_at(&self, i: usize) -> Option<i64> {
        self.0.key_at(i)
    }
    fn flat_index_of(&self, k: &i64) -> Option<usize> {
        self.0.flat_index_of(k)
    }
    fn parent(&self, k: &i64) -> Option<i64> {
        self.0.parent(k)
    }
    fn child_keys(&self, k: &i64) -> Vec<i64> {
        self.0.child_keys(k)
    }
    fn version_signal(&self) -> Signal<u64> {
        self.0.version_signal()
    }
    fn is_expanded(&self, k: &i64) -> bool {
        self.0.is_expanded(k)
    }
    fn set_expanded(&self, k: &i64, e: bool) {
        self.0.set_expanded(k, e)
    }
    fn contains_key(&self, k: &i64) -> bool {
        self.0.contains_key(k)
    }
    fn drag(&self, k: &i64) -> DragEligibility {
        self.0.drag(k)
    }
    fn can_accept(&self, q: &teksilo_data::DropQuery<'_, i64>) -> DropResponse {
        self.0.can_accept(q)
    }
    fn accept_drop(&self, c: teksilo_data::DropCommit<'_, i64>) -> bool {
        self.0.accept_drop(c)
    }
}

#[test]
fn from_source_drop_routes_through_source_and_refuses_cycle() {
    // Reorderable generic source: a valid drop reaches `accept_drop`; a drop
    // that would create a cycle (a parent onto its own child) is refused by
    // the source — proving the view delegates DnD to `can_accept`/
    // `accept_drop` instead of mutating a model itself.

    // Valid: drag node "b" (id 3, row 2) onto "root2" (id 4, row 3).
    let valid = Rc::new(MockI64Source::new());
    let mut t1 = WidgetTree::new();
    let v1 = t1.add(
        TreeView::from_source(
            MockI64Wrapper(valid.clone()),
            |_l: &String, _r: &TreeRow, _s| Box::new(FixedLeaf(120.0, 24.0)),
        )
        .item_height(24.0)
        .reorderable(true),
    );
    t1.layout(SizeProposal::exact(400.0, 300.0));
    let rows = row_ids(&t1, v1);
    let from = t1.bounds(rows[2]).center();
    let to = t1.bounds(rows[3]).center();
    drag_item(&mut t1, from, to);
    assert_eq!(
        valid.accept_log.borrow().len(),
        1,
        "a valid drop is routed to the source's accept_drop"
    );
    assert_eq!(
        valid.accept_log.borrow()[0].0,
        3,
        "dragged key recovered from RowDragData"
    );
    assert_eq!(
        valid.accept_log.borrow()[0].1,
        4,
        "target key resolved from the hovered row"
    );

    // Cycle: drag "root1" (id 1, row 0) onto its child "a" (id 2, row 1).
    let cyclic = Rc::new(MockI64Source::new());
    let mut t2 = WidgetTree::new();
    let v2 = t2.add(
        TreeView::from_source(
            MockI64Wrapper(cyclic.clone()),
            |_l: &String, _r: &TreeRow, _s| Box::new(FixedLeaf(120.0, 24.0)),
        )
        .item_height(24.0)
        .reorderable(true),
    );
    t2.layout(SizeProposal::exact(400.0, 300.0));
    let rows2 = row_ids(&t2, v2);
    let from2 = t2.bounds(rows2[0]).center();
    let to2 = t2.bounds(rows2[1]).center();
    drag_item(&mut t2, from2, to2);
    assert!(
        cyclic.accept_log.borrow().is_empty(),
        "a cyclic drop is refused by the source — no mutation applied"
    );
}

#[test]
fn from_source_reorder_bubbles_past_a_per_row_drop_target() {
    // Mirrors the designer outline: each row is a `DropTarget` that accepts
    // only a palette payload (here `Palette`). A row-reorder `RowDragData`
    // must bubble PAST that DropTarget to the TreeView and reorder — i.e. the
    // per-row drop target and the view's drag-to-reorder coexist.
    use crate::drop_target::DropTarget;
    use teksilo_core::event::EventResponse;
    use teksilo_core::widget_builder::WidgetBuilder;
    #[derive(Clone)]
    struct Palette;
    let src = Rc::new(MockI64Source::new());
    let mut t = WidgetTree::new();
    let v = t.add(
        TreeView::from_source(
            MockI64Wrapper(src.clone()),
            |_l: &String, _r: &TreeRow, _s| {
                // Match the designer row exactly: a DropTarget wrapped by an
                // on_pointer_event (selection) + context_menu handler node.
                Box::new(
                    DropTarget::new()
                        .on_drop_typed::<Palette>(|_p, _pos, _ctx| true)
                        .child(FixedLeaf(120.0, 24.0))
                        .on_pointer_event(|_ev, _ctx| EventResponse::Ignored)
                        .context_menu(|_pos, _ctx| None),
                ) as Box<dyn Widget>
            },
        )
        .item_height(24.0)
        .reorderable(true),
    );
    t.layout(SizeProposal::exact(400.0, 300.0));
    let rows = row_ids(&t, v);
    // Drag node "b" (id 3, row 2) onto "root2" (id 4, row 3).
    let from = t.bounds(rows[2]).center();
    let to = t.bounds(rows[3]).center();
    drag_item(&mut t, from, to);
    assert_eq!(
        src.accept_log.borrow().len(),
        1,
        "row reorder must bubble past the per-row DropTarget to the TreeView"
    );
}

#[test]
fn from_source_row_with_pointer_event_selection_stays_draggable() {
    // A row that selects on press via `on_pointer_event` (raw, returns
    // `Ignored`) installs NO gesture arena, so it never captures the pointer
    // and the row stays draggable — the pattern a reorderable view uses for
    // per-row selection (selection must land on *press* and carry the
    // Ctrl/Shift modifiers, which `TapRecognizer` fires-on-release and
    // strips). The framework also disambiguates a descendant `on_tap`
    // against an ancestor drag now (the ancestor observes the pointer while
    // the tap holds capture — see `ancestor_drag_starts_through_descendant_tap_capture`),
    // but `on_pointer_event` remains the right press-time + modifier-aware
    // choice here.
    use teksilo_core::event::{EventResponse, WidgetEvent};
    use teksilo_core::widget_builder::WidgetBuilder;
    let src = Rc::new(MockI64Source::new());
    let picked = Rc::new(Cell::new(None::<i64>));
    let mut t = WidgetTree::new();
    let picked_for_rows = picked.clone();
    let v = t.add(
        TreeView::from_source(
            MockI64Wrapper(src.clone()),
            move |_l: &String, _r: &TreeRow, _s| {
                let picked = picked_for_rows.clone();
                Box::new(FixedLeaf(120.0, 24.0).on_pointer_event(move |ev, _c| {
                    if let WidgetEvent::PointerDown { .. } = ev {
                        picked.set(Some(7));
                    }
                    EventResponse::Ignored
                })) as Box<dyn Widget>
            },
        )
        .item_height(24.0)
        .reorderable(true),
    );
    t.layout(SizeProposal::exact(400.0, 300.0));
    let rows = row_ids(&t, v);
    let from = t.bounds(rows[2]).center();
    let to = t.bounds(rows[3]).center();
    drag_item(&mut t, from, to);
    assert!(
        picked.get().is_some(),
        "press still selects via on_pointer_event"
    );
    assert_eq!(
        src.accept_log.borrow().len(),
        1,
        "on_pointer_event selection must not block the row drag"
    );
}

#[test]
fn lazy_loading_tree_rows_render_placeholders_and_request_the_window() {
    // A windowed `TreeDataSource` with nothing resident: every visible row
    // is `Loading`, so the TreeView must render placeholder skeletons (not
    // skip the rows) and nudge the source to load the realized window —
    // the tree analogue of the ListView lazy path.
    use std::ops::Range;

    struct WindowedTree {
        total: usize,
        version: Signal<u64>,
        requested: Rc<RefCell<Vec<Range<usize>>>>,
    }
    impl TreeDataSource for WindowedTree {
        type Item = String;
        type Key = usize;
        fn visible_count(&self) -> usize {
            self.total
        }
        fn with_entry<R>(
            &self,
            _flat_index: usize,
            _f: impl FnOnce(&String, &FlatEntry<usize>) -> R,
        ) -> Option<R> {
            None // nothing resident yet
        }
        fn key_at(&self, i: usize) -> Option<usize> {
            (i < self.total).then_some(i)
        }
        fn flat_index_of(&self, key: &usize) -> Option<usize> {
            (*key < self.total).then_some(*key)
        }
        fn parent(&self, _key: &usize) -> Option<usize> {
            None
        }
        fn child_keys(&self, _key: &usize) -> Vec<usize> {
            Vec::new()
        }
        fn version_signal(&self) -> Signal<u64> {
            self.version.clone()
        }
        fn is_expanded(&self, _key: &usize) -> bool {
            false
        }
        fn set_expanded(&self, _key: &usize, _expanded: bool) {}
        fn row_state(&self, _flat_index: usize) -> RowState {
            RowState::Loading
        }
        fn request_window(&self, range: Range<usize>) {
            self.requested.borrow_mut().push(range);
        }
    }

    let requested = Rc::new(RefCell::new(Vec::new()));
    let source = WindowedTree {
        total: 1000,
        version: Signal::new(0),
        requested: requested.clone(),
    };
    let mut t = WidgetTree::new();
    let v = t.add(
        TreeView::from_source(source, |_l: &String, _r: &TreeRow, _s| {
            Box::new(FixedLeaf(120.0, 28.0)) as Box<dyn Widget>
        })
        .item_height(28.0),
    );
    t.layout(SizeProposal::exact(400.0, 300.0));

    // 300px / 28px ≈ 10 visible + buffer → the loading rows are realized as
    // placeholder child widgets (children minus the scrollbar), NOT skipped.
    let placeholder_rows = row_ids(&t, v).len();
    assert!(
        placeholder_rows >= 10,
        "loading tree rows must render as placeholders, got {placeholder_rows}"
    );
    assert!(
        !requested.borrow().is_empty(),
        "request_window must be called for the visible range"
    );
}

#[test]
fn treeview_exportable_row_drops_on_foreign_sink_with_items() {
    use crate::primitives::{FixedSize, VStack};
    use teksilo_core::widget_builder::WidgetBuilder as _;
    // sample_tree(): roots A, B, C (collapsed), item type = &'static str.
    let model = sample_tree();
    #[allow(clippy::type_complexity)]
    let cap: Rc<RefCell<Option<(Vec<usize>, Option<Vec<&'static str>>)>>> =
        Rc::new(RefCell::new(None));
    let cap2 = cap.clone();
    let tv = TreeView::new(model.clone(), |_item, entry, _sel| {
        Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
    })
    .item_height(28.0)
    .exportable(DragTransferMode::Copy);
    let sink = FixedLeaf(180.0, 80.0).on_drop(move |mut payload, _pos, _ctx| {
        if let Some(rd) = payload.take_typed::<RowDragData<&'static str>>() {
            *cap2.borrow_mut() = Some((rd.rows, rd.items));
            true
        } else {
            false
        }
    });
    let mut tree = WidgetTree::new();
    tree.add(
        VStack::new()
            .spacing(0.0)
            .child(FixedSize::new().height(84.0).child(tv))
            .child(sink),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));
    // Row 0 = root A at y≈14; the sink spans y=84..164 (drop at y≈120).
    drag_item(&mut tree, Point::new(50.0, 14.0), Point::new(50.0, 120.0));
    let (rows, items) = cap.borrow().clone().expect("sink received a RowDragData");
    assert_eq!(rows, vec![0]);
    assert_eq!(items, Some(vec!["A"]));
}

#[test]
fn treeview_exportable_move_removes_source_node_via_stable_key() {
    use crate::primitives::{FixedSize, VStack};
    use teksilo_core::widget_builder::WidgetBuilder as _;
    // A Move-export drops the origin node once accepted elsewhere. The
    // move-out resolves a STABLE NodeId at drag-start (not a flat index at
    // completion), so it removes exactly the dragged node.
    let model = sample_tree(); // roots A, B, C
    let a = model.root(0);
    let accepted: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let acc2 = accepted.clone();
    let tv = TreeView::new(model.clone(), |_item, entry, _sel| {
        Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
    })
    .item_height(28.0)
    .exportable(DragTransferMode::Move);
    let sink = FixedLeaf(180.0, 80.0).on_drop(move |mut payload, _pos, _ctx| {
        if payload.take_typed::<RowDragData<&'static str>>().is_some() {
            *acc2.borrow_mut() = true;
            true
        } else {
            false
        }
    });
    let mut tree = WidgetTree::new();
    tree.add(
        VStack::new()
            .spacing(0.0)
            .child(FixedSize::new().height(84.0).child(tv))
            .child(sink),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));
    drag_item(&mut tree, Point::new(50.0, 14.0), Point::new(50.0, 120.0));
    assert!(*accepted.borrow(), "sink accepted the Move drop");
    // The exact node A (stable id captured at drag-start) was removed.
    assert_eq!(
        model.with_item(a, |v| *v),
        None,
        "node A was removed on Move"
    );
}

// --- `focused_index` reconciliation regression tests (RowAnchor) ---
//
// A tree's structural changes — including expand/collapse, which a flat
// `ListView` never has — surface only as a bare source-version bump, with
// no `DataChange` delta the keyboard cursor could shift by. These pin the
// `RowAnchor`-based fix: `focused_index` follows the row it was on, not
// the flat slot it used to occupy. Mirrors `list_view`'s
// `focused_index_follows_insert_before_it` /
// `focused_index_dropped_when_its_row_is_removed`.

#[test]
fn focused_index_follows_insert_above_it() {
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    let tree = TreeModel::new();
    for i in 0..10usize {
        tree.insert_root(i, format!("Node {i}"));
    }
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new(tree.clone(), |_item, _entry, _sel| {
            Box::new(FixedLeaf(120.0, 20.0))
        })
        .item_height(20.0)
        .selection(sel),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);

    // Click row 3 — selects it AND sets the nav cursor (`focused_index`) to 3.
    press_at(&mut wtree, 100.0, 70.0);
    assert_eq!(selection.selected_indices(), vec![3], "precondition");

    // Two roots inserted above — the same node (row 3) is now row 5.
    tree.insert_root(0, "New A".to_string());
    tree.insert_root(0, "New B".to_string());
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Index-based selection has no identity to shift by on a bare version
    // bump, so it stays put — the documented limitation on
    // `TreeView::selection` (unlike the keyboard cursor below, which tracks
    // the row by identity via `RowAnchor`).
    assert_eq!(
        selection.selected_indices(),
        vec![3],
        "precondition: index selection does not follow the insert"
    );

    // ArrowDown must resume from the shifted row (5 → 6) — the cursor
    // followed the insert, not the stale pre-insert index (3 → 4).
    wtree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        vec![6],
        "ArrowDown after a leading insert resumes from the shifted row \
         (5 → 6), not the stale pre-insert one (3 → 4)"
    );
}

#[test]
fn focused_index_clears_when_its_row_is_removed() {
    // No selection model: Enter's activation index is then a direct read of
    // `focused_index` (a selection would otherwise mask the fix via its own
    // stale-but-in-range fallback — see `alt_arrow_after_a_structural_change_...`
    // below for why that fallback matters).
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::event::{Key, Modifiers};

    let tree = TreeModel::new();
    for i in 0..10usize {
        tree.insert_root(i, format!("Node {i}"));
    }
    let activated: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let act = activated.clone();
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new(tree.clone(), |_item, _entry, _sel| {
            Box::new(FixedLeaf(120.0, 20.0))
        })
        .item_height(20.0)
        .on_activate(move |i, _| act.set(Some(i))),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);

    // Click row 3, then Enter activates the cursor's row.
    press_at(&mut wtree, 100.0, 70.0);
    wtree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(activated.get(), Some(3), "precondition: cursor is on row 3");

    // Row 3 itself — the row under the cursor — is removed.
    let node3 = tree.root(3);
    tree.remove(node3);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // With the cursor cleared (its anchor resolves to `None`), Enter with
    // no cursor falls back to row 0 — NOT row 4 (the stale index's `+1`
    // reading), and NOT whoever slid into the vacated slot 3.
    wtree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        activated.get(),
        Some(0),
        "focused_index was cleared when its row was removed, so activation \
         falls back to row 0 rather than the stale index or its replacement"
    );
}

#[test]
fn collapsing_a_branch_above_keeps_the_cursor_on_the_same_logical_row() {
    // Unlike a flat list, a tree's structural changes include expand /
    // collapse — this is the shape the source-version-only signal (no
    // `DataChange`) is specifically for. A's chevron toggles it WITHOUT
    // moving the nav cursor (`chevron_press_toggles_without_selecting_the_row`
    // pins that separately), so C's focus is undisturbed by the very
    // expand/collapse this test is exercising above it.
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::event::{Key, Modifiers};

    let tree = sample_tree(); // A (A1, A2), B (B1), C — all collapsed
    let activated: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let act = activated.clone();
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new_with_context(tree, |item: &&'static str, entry, selected, ctx| {
            Box::new(
                crate::StandardTreeItem::new(lit!((*item).to_string()))
                    .from_entry(entry)
                    .selected(selected)
                    .on_toggle_rc(ctx.toggle_callback()),
            ) as Box<dyn Widget>
        })
        .item_height(28.0)
        .row_click_expands(false)
        .on_activate(move |i, _| act.set(Some(i))),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);

    // Expand A via its chevron (x in [0, 16], row 0 depth 0) — reveals A1,
    // A2: rows are now A(0), A1(1), A2(2), B(3), C(4).
    press_at(&mut wtree, 8.0, 14.0);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(row_ids(&wtree, tv).len(), 5, "precondition: A expanded");

    // Click C's BODY (past the chevron column) to focus it.
    press_at(&mut wtree, 100.0, 126.0);
    wtree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        activated.get(),
        Some(4),
        "precondition: cursor is on row C (index 4)"
    );

    // Collapse A again via its chevron — the branch ABOVE the focused row,
    // toggled without touching the cursor. C moves from flat index 4 to 2.
    press_at(&mut wtree, 8.0, 14.0);
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        row_ids(&wtree, tv).len(),
        3,
        "precondition: A collapsed again"
    );

    wtree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        activated.get(),
        Some(2),
        "the cursor follows row C to its new flat index (4 → 2) once the \
         branch above it collapses, instead of staying on the stale index \
         (now B)"
    );
}

#[test]
fn alt_arrow_after_a_structural_change_reorders_the_row_the_cursor_is_on() {
    // No selection model, deliberately: Alt+Arrow's dragged-row fallback is
    // `selected_indices().first().or(fi.get())`, and index-based selection
    // does NOT itself follow a structural change (see
    // `focused_index_follows_insert_above_it`) — with one attached, this
    // scenario would still reorder the wrong row via the selection half of
    // that fallback, masking the `focused_index` fix under test here.
    use teksilo_core::event::{Key, Modifiers};

    let tree = TreeModel::new();
    for (i, label) in ["N0", "N1", "N2", "N3", "N4"].into_iter().enumerate() {
        tree.insert_root(i, label);
    }
    let mut wtree = WidgetTree::new();
    let tv = wtree.add(
        TreeView::new(tree.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .reorderable(true),
    );
    wtree.layout(SizeProposal::exact(400.0, 300.0));
    wtree.focus(tv);

    // Click N2's body (row 2, y≈70) — sets the nav cursor to 2.
    press_at(&mut wtree, 50.0, 70.0);

    // A root inserted above shifts N2 from flat index 2 to 3.
    tree.insert_root(0, "New");
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Alt+ArrowDown must reorder the row the cursor is VISIBLY on (N2, now
    // at index 3) past its new next sibling (N3) — not the stale index 2
    // (now occupied by N0).
    wtree.press_key(Key::ArrowDown, Modifiers::ALT);

    let order: Vec<&str> = (0..tree.root_count())
        .map(|i| tree.with_item(tree.root(i), |&v| v).unwrap())
        .collect();
    assert_eq!(
        order,
        vec!["New", "N0", "N1", "N3", "N2", "N4"],
        "Alt+ArrowDown moved N2 (the row the cursor is visibly on) past N3, \
         not the stale pre-insert index 2 (now N0)"
    );
}

// -- Realization window vs. a measuring caller ---------------------------

/// `layout_response` runs for two different questions — "you get 654 px, lay
/// yourself out" and "how tall would you like to be at this width?" — and it
/// used to cache the height it resolved from either as the viewport. The
/// second one carries no height, so it resolves to the bare fallback
/// constant: 200 px, which has nothing to do with the real viewport.
///
/// `build` sizes its realization window from that cache. So after the next
/// rebuild it realized the rows for a 200 px viewport, while `place_children`
/// went on computing the range honestly against the real 654 and bumping the
/// rebuild version to fetch the rows it thought were missing. Neither side
/// ever agreed: a rebuild every frame, each generation of rows replaced
/// before layout could place them. On screen the tree was an empty hole that
/// survived scrolling, clicking and re-selection, while a core spun.
///
/// [`measure_root_intrinsic`](WidgetTree::measure_root_intrinsic) is the
/// natural-height question in its plainest form, and it documents itself as
/// safe to call right after a layout pass — true of bounds, and false of any
/// state a widget caches out of `layout_response`. The mutation afterwards is
/// the rebuild trigger, which is what selecting or opening a row does.
#[test]
fn a_natural_height_query_does_not_strand_the_realization_window() {
    let model = TreeModel::new();
    for i in 0..56 {
        model.insert_root(i, i as i32);
    }
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(
        TreeView::new(model.clone(), |_item: &i32, _entry, _sel| {
            Box::new(FixedLeaf(280.0, 28.0))
        })
        .auto_item_height(28.0),
    );
    wtree.layout(SizeProposal::exact(300.0, 654.0));
    // The first build predates any allocation, so it realizes off the
    // constructor's 600px seed: 21 rows in view plus the 5-row buffer either
    // side. The 654px allocation lands during this same pass.
    assert_eq!(row_ids(&wtree, tv_id).len(), 27, "fixture is wrong");

    // Someone asks how tall the tree would like to be.
    wtree.measure_root_intrinsic(SizeProposal::with_width(300.0));

    // Force a rebuild, the way selecting a row does.
    model.insert_root(56, 56);
    wtree.layout(SizeProposal::exact(300.0, 654.0));

    // 654px at 28px a row is 24 rows in view, plus the 5-row buffer.
    // A viewport stranded at the 200px fallback yields 13.
    let rows = row_ids(&wtree, tv_id).len();
    assert_eq!(
        rows, 29,
        "realized {rows} rows for a 654px viewport — the window was sized \
         from the measurement's fallback, not from the allocation"
    );

    // Every realized row must actually be placed. Under the loop they kept
    // their default zero rect, because the next rebuild replaced them first.
    for id in row_ids(&wtree, tv_id).iter().take(rows) {
        let b = wtree.bounds(*id);
        assert!(
            b.height > 0.0 && b.width > 0.0,
            "row {id:?} was never placed: {b:?}"
        );
    }

    // And it converges: two idle passes must not re-realize anything.
    // Identity, not count — the loop held the count steady while discarding
    // and rebuilding every row.
    let settled = row_ids(&wtree, tv_id);
    wtree.layout(SizeProposal::exact(300.0, 654.0));
    wtree.layout(SizeProposal::exact(300.0, 654.0));
    assert_eq!(
        settled,
        row_ids(&wtree, tv_id),
        "the tree rebuilt its rows on an idle layout pass — build and \
         place_children disagree about the viewport"
    );
}

// --- Drop affordance: Into must not read as a Before/After line ---

/// Build an expanded 3-level tree view whose rows accept every drop, and drive
/// a same-view drag over the row at `target_row`.
fn drag_over(target_row: usize, frac: f32) -> (WidgetTree, WidgetId, f32) {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let tree = TreeModel::new();
    let a = tree.insert_root(0, "A");
    let a1 = tree.insert_child(a, 0, "A1");
    tree.insert_child(a1, 0, "A1a");
    tree.insert_root(1, "B");

    let tv = TreeView::new(tree, |_item, entry, _selected| {
        Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0)) as Box<dyn Widget>
    })
    .item_height(28.0)
    .reorderable(true);
    // Rows: A(0) A1(1) A1a(2) B(3) — three levels, all visible.
    tv.expand(a);
    tv.expand(a1);
    let mut wtree = WidgetTree::new();
    let tv_id = wtree.add(tv);
    wtree.layout(SizeProposal::exact(400.0, 300.0));

    // Drag the last row (B) so no target is inside the dragged subtree.
    let src_y = 3.0 * 28.0 + 14.0;
    wtree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(50.0, src_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(62.0, src_y),
    });
    let y = target_row as f32 * 28.0 + 28.0 * frac;
    wtree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(62.0, y),
    });
    (wtree, tv_id, y)
}

fn viz(wtree: &WidgetTree, tv_id: WidgetId) -> Option<DropViz> {
    wtree
        .widget_as_any(tv_id)
        .unwrap()
        .downcast_ref::<TreeView<&'static str>>()
        .unwrap()
        .drop_feedback
        .get()
}

#[test]
fn insertion_line_is_indented_to_the_level_the_row_lands_at() {
    // Row 2 is "A1a" at depth 2; a sibling drop lands at depth 2, so the line
    // must start two indent steps in — that is the whole difference between
    // "after this scene, still inside the chapter" and "after the chapter".
    let (wtree, tv, _) = drag_over(2, 0.9);
    assert!(
        matches!(viz(&wtree, tv), Some(DropViz::Line { depth: 2, .. })),
        "After a depth-2 row must report depth 2, got {:?}",
        viz(&wtree, tv)
    );

    // Row 0 is "A" at depth 0 — a root-level sibling, no indent.
    let (wtree, tv, _) = drag_over(0, 0.05);
    assert!(
        matches!(viz(&wtree, tv), Some(DropViz::Line { depth: 0, .. })),
        "Before a root row must report depth 0, got {:?}",
        viz(&wtree, tv)
    );
}

#[test]
fn into_box_is_inset_so_it_cannot_be_read_as_an_insertion_line() {
    use teksilo_canvas::{DrawCommand, ShapeKind};

    // Middle third of row 1 ("A1", depth 1) — a reparent, not a sibling.
    let (mut wtree, tv, _) = drag_over(1, 0.5);
    let Some(DropViz::Rect {
        top, height, depth, ..
    }) = viz(&wtree, tv)
    else {
        panic!("middle third must be an Into, got {:?}", viz(&wtree, tv));
    };
    assert_eq!(
        depth, 1,
        "the box frames the target row, at the target's depth"
    );
    assert_eq!((top, height), (28.0, 28.0), "the box frames row 1");

    // The painted geometry is what the writer actually reads: a rounded rect
    // strictly inside the row band. If its top edge sat on `top` it would be
    // pixel-identical to the Before line, which is the bug this guards.
    let recipe = teksilo_core::styles::ListDropIntoRecipe::default();
    assert!(recipe.inset > 0.0, "a zero inset re-creates the ambiguity");
    let frame = wtree.render();
    let rounded: Vec<_> = frame
        .draw_order
        .iter()
        .filter_map(|c| match c {
            DrawCommand::Shape(i) => frame.shapes.get(*i),
            _ => None,
        })
        .filter(|s| s.shape == ShapeKind::RoundedRect && s.corner_radii[0] > 0.0)
        .collect();
    assert!(
        rounded.iter().any(|s| {
            let [x, y, w, h] = s.screen;
            (y - (top + recipe.inset)).abs() < 0.01
                && (h - (height - recipe.inset * 2.0)).abs() < 0.01
                && x > 0.0
                && w > 0.0
        }),
        "no inset rounded box for the Into hover; shapes = {:?}",
        rounded.iter().map(|s| s.screen).collect::<Vec<_>>()
    );
    // Both a fill and an outline — the outline is what survives a drag ghost
    // sitting over the row's right half.
    assert!(
        rounded.iter().any(|s| s.stroke_width > 0.0)
            && rounded.iter().any(|s| s.stroke_width == 0.0),
        "the Into box needs both a wash and an outline"
    );
}
