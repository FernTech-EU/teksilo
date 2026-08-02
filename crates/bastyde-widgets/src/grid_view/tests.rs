// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless tests for `GridView` (Phase 1: uniform grid).
//!
//! No GPU / display server needed — exercises virtualization, column-count
//! derivation, tile placement, selection, keyboard navigation, data-change
//! reconciliation, and accessibility roles.

use super::*;
use bastyde_canvas::SizeProposal;
use bastyde_core::widget::LayoutContext;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_data::{ListModel, SelectionMode, SelectionModel};

#[derive(Debug)]
struct FixedLeaf(f32, f32);
impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

/// A grid of `count` items, fixed 100×50 tiles, default 8px gaps.
fn make_grid(count: usize) -> (WidgetTree, WidgetId, ListModel<usize>) {
    let model = ListModel::from_vec((0..count).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0))).tile_size(100.0, 50.0),
    );
    (tree, id, model)
}

/// The body pane is the first child; its children are the tile wrappers.
fn tiles(tree: &WidgetTree, grid_id: WidgetId) -> Vec<WidgetId> {
    let children = tree.children(grid_id);
    let body = children[0];
    tree.children(body)
}

#[test]
fn tiles_materialize_during_scrollbar_thumb_drag() {
    // The reason `GridBodyPane` exists — see `common::thumb_drag_test`'s
    // module docs for the invariant, and for why every virtualized view
    // asserts it.
    let (mut tree, id, _model) = make_grid(3000);
    crate::common::thumb_drag_test::assert_body_survives_thumb_drag(
        &mut tree,
        id,
        400.0,
        300.0,
        0.0,
        "GridView",
        |t| {
            tiles(t, id)
                .into_iter()
                .filter(|tile| {
                    let b = t.bounds(*tile);
                    b.height > 1.0 && b.y > -b.height && b.y < 300.0
                })
                .count()
        },
    );
}

#[test]
fn virtualization_realizes_only_visible_tiles() {
    // 300 items, 3 columns → 100 rows. Viewport shows ~6 rows; with the
    // 5-row buffer that's ~12 rows × 3 = ~36 tiles, far fewer than 300.
    let (mut tree, id, _model) = make_grid(300);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let n = tiles(&tree, id).len();
    assert!(n < 60, "expected far fewer than 300 tiles, got {n}");
    assert!(n >= 18, "expected at least 18 tiles realized, got {n}");
}

#[test]
fn fixed_tile_size_derives_three_columns() {
    let (mut tree, id, _model) = make_grid(30);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    // tile 0 at (0,0); tile 1 at x = 100 + 8 = 108; tile 3 starts row 1.
    let b0 = tree.bounds(t[0]);
    let b1 = tree.bounds(t[1]);
    let b3 = tree.bounds(t[3]);
    assert!((b0.x - 0.0).abs() < 0.01, "tile 0 x = {}", b0.x);
    assert!((b1.x - 108.0).abs() < 0.01, "tile 1 x = {}", b1.x);
    assert!((b0.y - 0.0).abs() < 0.01, "tile 0 y = {}", b0.y);
    // Row 1 = tile_height (50) + row_gap (8) = 58.
    assert!((b3.y - 58.0).abs() < 0.01, "tile 3 y = {}", b3.y);
    assert!((b3.x - 0.0).abs() < 0.01, "tile 3 x = {}", b3.x);
}

#[test]
fn fixed_column_count_uses_exact_columns() {
    let model = ListModel::from_vec((0..40).collect());
    let mut tree = WidgetTree::new();
    let id =
        tree.add(GridView::new(model, |_tc| Box::new(FixedLeaf(10.0, 40.0))).column_count(4, 40.0));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    // 4 columns: tile 4 wraps to row 1.
    let b0 = tree.bounds(t[0]);
    let b4 = tree.bounds(t[4]);
    assert!((b0.y - 0.0).abs() < 0.01);
    assert!((b4.y - 48.0).abs() < 0.01, "tile 4 y = {} (row 1)", b4.y);
}

#[test]
fn empty_model_realizes_no_tiles() {
    let (mut tree, id, _model) = make_grid(0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Children: scrollbar + (no body, no overlay). No tile body pane.
    let children = tree.children(id);
    // With no items there's no body pane; only the scrollbar remains.
    assert!(
        children.len() <= 1,
        "empty grid should have at most a scrollbar child, got {}",
        children.len()
    );
}

#[test]
fn data_change_triggers_rebuild() {
    let (mut tree, id, model) = make_grid(6);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id).len(), 6);

    model.push(99);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id).len(), 7);

    model.remove(0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id).len(), 6);
}

#[test]
fn focused_index_follows_insert_before_it() {
    // Bug repro: `focused_index` (the keyboard-nav anchor) was never
    // adjusted on any DataChange, so after a peer/insert shifts the tiles
    // it silently pointed at the wrong one — the next ArrowRight would
    // resume from a stale position instead of the tile the user was
    // actually on. Mirrors `ListView`'s regression test of the same name.
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};

    // tile_size(100, 50) with the default 8px gaps fits 3 columns in 400px
    // (3*100 + 2*8 = 316 <= 400; a 4th would need 424).
    let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);

    // Click tile 1 (column 1 — clear of the row's trailing edge, where
    // ArrowRight is blocked without wrap-navigation) — sets both selection
    // and the keyboard-nav anchor to 1.
    let t = tiles(&tree, id);
    tree.click(t[1]);
    assert_eq!(
        selection.selected_indices(),
        vec![1],
        "precondition: click selects tile 1"
    );

    // A peer-driven reload prepends two tiles — tile 1 is now tile 3
    // (still column 0, clear of the trailing edge).
    model.insert(0, 100);
    model.insert(0, 200);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // The selection model itself already index-shifts (existing
    // behaviour) — this just re-confirms the setup, not the fix.
    assert_eq!(
        selection.selected_indices(),
        vec![3],
        "precondition: selection shifts with the inserted tiles"
    );

    // If `focused_index` had NOT shifted (the bug), it would still read 1,
    // and ArrowRight would resume from there (→ select 2). With the fix it
    // follows the insert to 3, so ArrowRight resumes from 3 (→ 4).
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert_eq!(
        selection.selected_indices(),
        vec![4],
        "ArrowRight after a leading insert resumes from the shifted tile (3 → 4), \
         not the stale pre-insert one (1 → 2)"
    );
}

#[test]
fn focused_index_dropped_when_its_tile_is_removed() {
    // The focused tile itself was removed: the anchor must be cleared, not
    // left pointing at whatever now occupies its old slot.
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};

    let model = ListModel::from_vec((0..10).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);

    let t = tiles(&tree, id);
    tree.click(t[3]);
    assert_eq!(selection.selected_indices(), vec![3], "precondition");

    model.remove(3);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        selection.selected_indices().is_empty(),
        "precondition: selection drops the removed tile"
    );

    // With the bug, `focused_index` still reads 3 (now a DIFFERENT tile —
    // the one that slid into that slot), so ArrowRight would select 4.
    // With the fix it's cleared, so "no cursor yet" semantics apply and
    // ArrowRight lands ON tile 0 instead of stepping past it.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "focused_index must be dropped when its tile is removed, not silently repointed"
    );
}

#[test]
fn click_selects_tile() {
    let model = ListModel::from_vec((0..12).collect());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    tree.click(t[2]);
    assert!(selection.is_selected(2), "tile 2 should be selected");
    assert!(!selection.is_selected(0));
}

/// A tile advertises `Action::Click`; an AT / automation click must select
/// it. AccessKit defines `Click` as "the equivalent of a single click", and
/// the Windows / macOS adapters also route AT *select-this-item* on a
/// selectable node through `Click` — a tile is `set_selected`, so this is
/// the AT selection path.
#[test]
fn access_click_selects_tile() {
    let model = ListModel::from_vec((0..12).collect());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    tree.dispatch_event(bastyde_core::event::WidgetEvent::AccessAction {
        action: bastyde_core::accesskit::Action::Click,
        target: Some(t[2]),
        target_node: bastyde_core::accessibility::root_node_id(),
        data: None,
    });
    assert!(selection.is_selected(2), "AT click should select tile 2");
    assert!(!selection.is_selected(0));
}

/// `Click` is a *single* click: it activates only under
/// `ActivateOn::SingleClick`. Under the `DoubleClick` default it selects
/// without activating — otherwise AT would fire destructive open-actions
/// that a sighted single click never triggers.
#[test]
fn access_click_activates_only_on_single_click_mode() {
    use std::cell::Cell;
    use std::rc::Rc;

    let at_click = |tree: &mut WidgetTree, id: WidgetId| {
        tree.dispatch_event(bastyde_core::event::WidgetEvent::AccessAction {
            action: bastyde_core::accesskit::Action::Click,
            target: Some(id),
            target_node: bastyde_core::accessibility::root_node_id(),
            data: None,
        });
    };

    // DoubleClick (the default): AT click selects but must NOT activate.
    let fired: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let f = fired.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(ListModel::from_vec((0..12).collect()), |_tc| {
            Box::new(FixedLeaf(100.0, 50.0))
        })
        .tile_size(100.0, 50.0)
        .selection(SelectionModel::new(SelectionMode::Single))
        .activate_on(crate::data_views::ActivateOn::DoubleClick)
        .on_tile_activate(move |_idx, _ctx| f.set(f.get() + 1)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    at_click(&mut tree, t[2]);
    assert_eq!(
        fired.get(),
        0,
        "AT click must not activate under DoubleClick mode"
    );

    // SingleClick: AT click activates, like a real single click.
    let fired: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let f = fired.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(ListModel::from_vec((0..12).collect()), |_tc| {
            Box::new(FixedLeaf(100.0, 50.0))
        })
        .tile_size(100.0, 50.0)
        .selection(SelectionModel::new(SelectionMode::Single))
        .activate_on(crate::data_views::ActivateOn::SingleClick)
        .on_tile_activate(move |idx, _ctx| {
            assert_eq!(idx, 2);
            f.set(f.get() + 1)
        }),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    at_click(&mut tree, t[2]);
    assert_eq!(
        fired.get(),
        1,
        "AT click must activate under SingleClick mode"
    );
}

#[test]
fn first_arrow_lands_on_an_end_tile_instead_of_skipping_it() {
    // "No cursor yet" is not "cursor on tile 0": a forward key must land ON the
    // first tile rather than step past it, and a backward key on the last one.
    // A preset selection acts as the cursor, so navigation continues from what
    // the user can see.
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};

    let press = |tree: &mut WidgetTree, key: Key| {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers: Modifiers::default(),
            text: None,
        });
    };

    for (key, want, what) in [
        (Key::ArrowRight, 0usize, "first ArrowRight selects tile 0"),
        (Key::ArrowDown, 0usize, "first ArrowDown selects tile 0"),
        (
            Key::ArrowLeft,
            29usize,
            "first ArrowLeft selects the last tile",
        ),
        (Key::ArrowUp, 29usize, "first ArrowUp selects the last tile"),
    ] {
        let model = ListModel::from_vec((0..30).collect());
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let id = tree.add(
            GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
                .tile_size(100.0, 50.0)
                .selection(selection.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(id);
        assert!(
            selection.selected_indices().is_empty(),
            "precondition: nothing selected, no cursor"
        );

        press(&mut tree, key);
        assert!(selection.is_selected(want), "{what}");
    }

    // With a preselected tile, the first key steps from *it*.
    let model = ListModel::from_vec((0..30).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Tile 4 sits mid-row (3 columns), so ArrowRight is not edge-blocked.
    selection.select(4);
    tree.focus(id);
    press(&mut tree, Key::ArrowRight);
    assert!(
        selection.is_selected(5),
        "ArrowRight from a preselected tile 4 continues to 5"
    );
}

#[test]
fn arrow_keys_move_focus_and_selection() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec((0..30).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);

    // No cursor yet, so the first ArrowRight lands ON tile 0 rather than
    // stepping past it.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert!(
        selection.is_selected(0),
        "the first arrow key selects index 0, it does not skip it"
    );

    // Now at 0; ArrowRight → 1 (same row).
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert!(selection.is_selected(1), "ArrowRight selects index 1");

    // ArrowDown → 1 + 3 columns = 4.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert!(
        selection.is_selected(4),
        "ArrowDown moves by one row (cols)"
    );
}

#[test]
fn keyboard_selection_chases_outer_scroll_area() {
    // A single-column grid (20 × 50px tiles → 1000px) in a 200px grid box whose
    // lower half is below a 100px outer ScrollArea's fold. Tiles are virtualized
    // and not focusable (the grid holds focus with active_descendant), so the
    // focus-driven follow can't reveal the focused tile — ctx.ensure_visible must.
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, VStack};
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};

    let model = ListModel::from_vec((0..20).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    // Box width 120 fits exactly one 100px tile → a tall single column.
    let grid = GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0)
        .selection(sel);
    let grid_id = tree.add(grid);
    let grid_box = tree.add(
        FixedSize::new()
            .width(120.0)
            .height(200.0)
            .child_id(grid_id),
    );
    let filler = tree.add(FixedLeaf(120.0, 200.0));
    let outer_content = tree.add(VStack::new().add_child(grid_box).add_child(filler));
    let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let _outer = tree.add(outer);
    tree.layout(SizeProposal::exact(120.0, 100.0));

    tree.focus(grid_id);
    tree.layout(SizeProposal::exact(120.0, 100.0));
    outer_y.set(0.0);
    tree.layout(SizeProposal::exact(120.0, 100.0));
    assert!(outer_y.get().abs() < 0.01, "reset outer to top");

    for _ in 0..20 {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::default(),
            text: None,
        });
    }
    tree.layout(SizeProposal::exact(120.0, 100.0));
    assert!(
        outer_y.get() > 0.01,
        "navigating to a tile below the fold must scroll the enclosing ScrollArea (got {})",
        outer_y.get()
    );
}

#[test]
fn ctrl_a_selects_all() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec((0..12).collect());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(selection.count(), 12, "Ctrl+A selects every item");
}

#[test]
fn ctrl_arrow_moves_cursor_without_selecting() {
    // 3 columns fit 400px (3*100 + 2*8 = 316 <= 400).
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);

    let focused_index = |tree: &WidgetTree| -> Option<usize> {
        tree.widget_as_any(id)
            .and_then(|any| any.downcast_ref::<GridView<usize>>())
            .and_then(|g| g.focused_index.get())
    };
    let press = |tree: &mut WidgetTree, key: Key, modifiers: Modifiers| {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
    };

    // Plain ArrowRight still selects (the first press lands ON tile 0).
    press(&mut tree, Key::ArrowRight, Modifiers::default());
    assert_eq!(selection.selected_indices(), vec![0]);

    // Ctrl+ArrowRight moves the cursor without touching the selection.
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+ArrowRight must leave the selection unchanged"
    );
    assert_eq!(focused_index(&tree), Some(1));

    // Ctrl+ArrowDown moves by one row (±cols) — still cursor-only.
    press(&mut tree, Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0], "still unchanged");
    assert_eq!(
        focused_index(&tree),
        Some(4),
        "Ctrl+ArrowDown moves the cursor by one row (col count 3)"
    );

    // Ctrl+Space toggles the now-focused tile (4) on, adding to — not
    // replacing — the existing selection.
    press(&mut tree, Key::Space, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0, 4]);

    // Ctrl+Space again toggles it back off.
    press(&mut tree, Key::Space, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0]);

    // Plain Arrow after a Ctrl-cursor move still replaces the selection
    // with the new cursor position (select-follow).
    press(&mut tree, Key::ArrowRight, Modifiers::default());
    assert_eq!(selection.selected_indices(), vec![5]);
}

#[test]
fn ctrl_arrow_moves_cursor_without_selecting_in_single_mode() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert_eq!(selection.selected_indices(), vec![0]);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+ArrowRight must not select in Single mode either"
    );
    let focused = tree
        .widget_as_any(id)
        .and_then(|any| any.downcast_ref::<GridView<usize>>())
        .and_then(|g| g.focused_index.get());
    assert_eq!(focused, Some(1));

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert_eq!(selection.selected_indices(), vec![2]);
}

#[test]
fn container_has_grid_role_and_counts() {
    let (mut tree, id, _model) = make_grid(30);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::Grid);
}

#[test]
fn tiles_have_gridcell_role() {
    let (mut tree, id, _model) = make_grid(12);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    let info = tree.accessibility_node(t[0]);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::GridCell);
}

#[test]
fn tile_a11y_label_names_each_gridcell() {
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .tile_a11y_label(|i| format!("Item {i}")),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    // Each cell announces the app-supplied concise name, not just its coordinates.
    assert_eq!(tree.accessibility_node(t[0]).name(), Some("Item 0"));
    assert_eq!(tree.accessibility_node(t[1]).name(), Some("Item 1"));
    assert_eq!(
        tree.accessibility_node(t[0]).role(),
        bastyde_core::accesskit::Role::GridCell
    );
}

// ── Phase 2: variable row heights + anchoring ───────────────────────────

#[test]
fn exact_item_height_positions_rows_by_height() {
    // Single column → row r == item r. Exact heights [100, 50, 50, ...].
    let model = ListModel::from_vec((0..20).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |tc| {
            let h = if tc.index == 0 { 100.0 } else { 50.0 };
            Box::new(FixedLeaf(50.0, h))
        })
        .column_count(1, 50.0)
        .item_height(|i| if i == 0 { 100.0 } else { 50.0 }),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));
    let t = tiles(&tree, id);
    // Row 0 height 100 at y=0; row 1 at y = 100 + gap(8) = 108; height 50.
    assert!((tree.bounds(t[0]).y - 0.0).abs() < 0.01);
    assert!((tree.bounds(t[0]).height - 100.0).abs() < 0.01);
    assert!(
        (tree.bounds(t[1]).y - 108.0).abs() < 0.01,
        "row 1 y = {}",
        tree.bounds(t[1]).y
    );
    assert!((tree.bounds(t[2]).y - 166.0).abs() < 0.01); // 108 + 50 + 8
}

#[test]
fn auto_measure_places_rows_at_measured_heights() {
    // Estimate 50 but every tile actually measures 30. After one layout the
    // realized rows are placed using the measured height, not the estimate.
    let model = ListModel::from_vec((0..40).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 30.0)))
            .column_count(1, 50.0)
            .variable_row_heights(50.0),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));
    // A second layout lets the anchored scroll settle; positions are stable.
    tree.layout(SizeProposal::exact(200.0, 300.0));
    let t = tiles(&tree, id);
    // Measured: row 1 at 30 + gap(8) = 38 (not the 58 an estimate would give).
    assert!(
        (tree.bounds(t[1]).y - 38.0).abs() < 0.5,
        "row 1 y = {} (expected ~38 from measured height)",
        tree.bounds(t[1]).y
    );
    assert!((tree.bounds(t[1]).height - 30.0).abs() < 0.5);
}

#[test]
fn auto_measure_under_realization_converges_without_scroll() {
    // Estimate 50, actual 20: the first build realizes far too few rows
    // for the viewport (the estimated offsets say ~6 rows fill 300 px,
    // the measured ones say 15 do). The post-measure realization
    // re-check must request rebuilds until realized tiles cover the
    // whole viewport — previously the bottom gap only healed on the
    // next scroll event.
    let model = ListModel::from_vec((0..200).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 20.0)))
            .column_count(1, 50.0)
            .row_spacing(0.0)
            .variable_row_heights(50.0),
    );
    // Let the measure → re-check → rebuild cycle settle. No scroll input.
    for _ in 0..6 {
        tree.layout(SizeProposal::exact(200.0, 300.0));
    }
    let t = tiles(&tree, id);
    let last_bottom = t
        .iter()
        .map(|id| {
            let b = tree.bounds(*id);
            b.y + b.height
        })
        .fold(0.0_f32, f32::max);
    assert!(
        last_bottom >= 300.0,
        "realized tiles must cover the viewport bottom without scrolling, got {last_bottom}"
    );
}

#[test]
fn variable_grid_virtualizes() {
    let model = ListModel::from_vec((0..500).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 40.0)))
            .column_count(2, 40.0)
            .variable_row_heights(40.0),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));
    let n = tiles(&tree, id).len();
    assert!(n < 80, "variable grid should virtualize, got {n} tiles");
    assert!(n >= 10);
}

// ── Phase 4: sections ───────────────────────────────────────────────────

struct TwoSections;
impl super::sections::SectionProvider for TwoSections {
    fn section_count(&self) -> usize {
        2
    }
    fn items_in_section(&self, _s: usize) -> usize {
        3
    }
    fn section_title(&self, s: usize) -> String {
        format!("Section {s}")
    }
}

#[test]
fn sections_offset_tiles_below_headers() {
    // 2 columns, 50px tiles, 28px headers, 8px gaps. Section 0 header at
    // y=0, its band starts at y=28. Section 1 (after section 0's 2-row band
    // = 28 + 108 + 8 gap = 144) header at y=144, band at 172.
    let model = ListModel::from_vec((0..6).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 50.0)))
            .column_count(2, 50.0)
            .section_header_height(28.0)
            .sections(TwoSections),
    );
    tree.layout(SizeProposal::exact(300.0, 600.0));
    let kids = tree.children(id);
    let body = kids[0];
    let body_kids = tree.children(body);
    // First 6 body children are tiles (flat order), then 2 headers.
    let tile0 = tree.bounds(body_kids[0]);
    let tile3 = tree.bounds(body_kids[3]);
    assert!((tile0.y - 28.0).abs() < 0.5, "tile 0 y = {}", tile0.y);
    assert!((tile3.y - 172.0).abs() < 0.5, "tile 3 y = {}", tile3.y);
    // The header wrappers carry RowHeader role.
    let header0 = tree.accessibility_node(body_kids[6]);
    assert_eq!(
        header0.role(),
        bastyde_core::accesskit::Role::RowHeader,
        "section header should be RowHeader"
    );
}

#[test]
fn sections_report_section_local_aria_row_col() {
    // Item 3 is the FIRST item of section 1 (items 0, 1, 2 belong to
    // section 0 — see `sections_offset_tiles_below_headers` above), so it
    // must announce ARIA row 1 / col 1 (1-based) — its position within ITS
    // OWN section band — not row 2 / col 2, the answer global
    // `index / cols, index % cols` math would give.
    let model = ListModel::from_vec((0..6).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 50.0)))
            .column_count(2, 50.0)
            .section_header_height(28.0)
            .sections(TwoSections),
    );
    tree.layout(SizeProposal::exact(300.0, 600.0));
    let kids = tree.children(id);
    let body = kids[0];
    let body_kids = tree.children(body);
    let item3_id = body_kids[3];

    let update = tree.sync_accessibility();
    let node_id = widget_id_to_node_id(item3_id);
    let node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == node_id)
        .map(|(_, n)| n)
        .expect("item 3's a11y node must be present in the sync");
    assert_eq!(
        node.row_index(),
        Some(1),
        "item 3 is the first row of its OWN section"
    );
    assert_eq!(
        node.column_index(),
        Some(1),
        "item 3 is the first column of its row"
    );
}

#[test]
fn pinned_header_is_not_built_for_a_zero_section_provider() {
    // A hand-rolled `SectionProvider` that reports zero sections despite a
    // non-empty model (a misconfiguration, but one the widget must survive
    // gracefully): with `pinned_section_headers(true)`, `PinnedHeader::build`
    // used to unconditionally call the header factory at `current_section`'s
    // default (0) — a provider indexing directly into its own section list
    // would panic. The fix skips building the pinned header entirely when
    // there are no sections.
    struct ZeroSections;
    impl super::sections::SectionProvider for ZeroSections {
        fn section_count(&self) -> usize {
            0
        }
        fn items_in_section(&self, _s: usize) -> usize {
            panic!("must not be called for a zero-section provider")
        }
        fn section_title(&self, _s: usize) -> String {
            panic!("must not be called for a zero-section provider")
        }
    }

    let model = ListModel::from_vec((0..3).collect());
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 50.0)))
            .column_count(2, 50.0)
            .sections(ZeroSections)
            .pinned_section_headers(true),
    );
    // Must not panic.
    tree.layout(SizeProposal::exact(300.0, 300.0));
}

// ── Phase 4: waterfall ──────────────────────────────────────────────────

#[test]
fn waterfall_places_into_shortest_column() {
    // 2 columns, exact heights [60, 100, 40]. Item 0 → col 0; item 1 → col 1;
    // item 2 → col 0 (shorter). Gaps 8.
    let heights = [60.0_f32, 100.0, 40.0, 80.0, 50.0];
    let model = ListModel::from_vec((0..heights.len()).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, move |tc| {
            Box::new(FixedLeaf(80.0, heights[tc.index]))
        })
        .column_count(2, 60.0)
        .waterfall(60.0)
        .item_height(move |i| heights[i]),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));
    let t = tiles(&tree, id);
    let b0 = tree.bounds(t[0]);
    let b1 = tree.bounds(t[1]);
    let b2 = tree.bounds(t[2]);
    // Item 0 in column 0 at y=0; item 1 in column 1 at y=0; item 2 stacks
    // under item 0 (column 0) at y = 60 + gap(8) = 68.
    assert!((b0.y - 0.0).abs() < 0.5);
    assert!((b1.y - 0.0).abs() < 0.5);
    assert!(b1.x > b0.x, "item 1 should be in a column to the right");
    assert!((b2.x - b0.x).abs() < 0.5, "item 2 shares column 0");
    assert!((b2.y - 68.0).abs() < 0.5, "item 2 y = {}", b2.y);
}

// ── Phase 3: reorder / activation / type-ahead / incremental loading ────

#[test]
fn alt_arrow_reorders_tile() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec(vec![10usize, 20, 30, 40]);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .reorderable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    // Focus starts at 0; Alt+ArrowRight moves item 0 forward by one.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(model.with_item(0, |v| *v), Some(20));
    assert_eq!(model.with_item(1, |v| *v), Some(10));
}

#[test]
fn pointer_drag_reorders_tile_through_source_accept_drop() {
    // A pointer drag-reorder now routes through the source's `accept_drop`
    // (replacing the old `move_item_fn`): drag tile 0 past the last tile and
    // drop → it lands at the end.
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
    let model = ListModel::from_vec(vec![10usize, 20, 30, 40]);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .reorderable(true),
    );
    // 4×100 + 3×8 gaps = 424 → 4 columns fit in a 440-wide viewport.
    tree.layout(SizeProposal::exact(440.0, 300.0));

    let from = Point::new(50.0, 25.0); // tile 0 center
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    // Cross the drag threshold, then move past the last tile (insertion = end).
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(72.0, 25.0),
    });
    let to = Point::new(430.0, 25.0);
    tree.dispatch_event(WidgetEvent::PointerMove { position: to });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: to,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    assert_eq!(
        model.with_item(3, |v| *v),
        Some(10),
        "tile 0 moved to the end via the source's accept_drop"
    );
    assert_eq!(model.with_item(0, |v| *v), Some(20));
    let _ = id;
}

#[test]
fn pointer_drag_drop_in_a_row_gap_does_not_append_at_the_end() {
    // Regression: a drop anywhere in an inter-tile gap (here, the row-gap
    // band between two rows) used to silently resolve to "append at the
    // end", because `index_at_point` returns None for any non-tile point
    // and the old `insertion_index` fell straight through to `len`.
    // Dragging tile 0 and dropping in the gap between row 0 and row 1 must
    // insert it near its origin, not send it to the very end.
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
    let model = ListModel::from_vec(vec![10usize, 20, 30, 40, 50, 60, 70, 80]);
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .reorderable(true),
    );
    // 2×100 + 8 gap = 208 → 2 columns fit in 220px; row_step = 50 + 8 =
    // 58, so the row-gap band spans y 50..58.
    tree.layout(SizeProposal::exact(220.0, 300.0));

    let from = Point::new(50.0, 25.0); // tile 0 center
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    // Cross the drag threshold, then move into the row-gap: y=53 sits
    // between row 0 (0..50) and row 1 (58..108); x=70 is inside column 0,
    // past its horizontal center.
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(72.0, 25.0),
    });
    let to = Point::new(70.0, 53.0);
    tree.dispatch_event(WidgetEvent::PointerMove { position: to });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: to,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    assert_ne!(
        model.with_item(7, |v| *v),
        Some(10),
        "a row-gap drop must not silently append the dragged tile at the end"
    );
}

#[test]
fn marquee_edge_auto_scroll_selects_tiles_revealed_by_scrolling() {
    // Regression for the marquee's viewport-edge auto-scroll: a rubber-band
    // drag held near the bottom edge must keep scrolling content into view
    // and grow the selection to cover it, not just the tiles that were
    // visible at press time.
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

    // Single column (100px tile + 8px gap needs 108px; a 150px viewport
    // fits exactly one), so tile x spans 0..100 and row `i` sits at
    // y = i * 58 (50 + 8 gap). 40 rows gives plenty of off-screen content
    // below the 150px-tall viewport (only rows 0..2 are visible at rest).
    let model = ListModel::from_vec((0..40).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(150.0, 150.0));

    // Press on the background at x=120 (past the tile's 0..100 span, so
    // `index_at_point` misses and this starts a marquee, not an item
    // drag) near the top of the viewport.
    let press = Point::new(120.0, 10.0);
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: press,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    // Cross the 5px drag threshold.
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(120.0, 16.0),
    });
    // Sweep into the tile column (x=50) and past the bottom edge — deep
    // enough into the edge band that the pointer never needs to move
    // again for auto-scroll to keep going.
    let hold = Point::new(50.0, 200.0);
    tree.dispatch_event(WidgetEvent::PointerMove { position: hold });

    // Pump enough frame ticks for the auto-scroll effect to run well past
    // one screenful. Each `layout()` call advances at most one tick (see
    // `WidgetTree::advance_frame_tick`), and the tick handler re-arms
    // itself every time it's still in the edge band, so a fixed loop of
    // `tree.layout()` calls is deterministic here — no real elapsed time
    // or sleeping needed.
    for _ in 0..80 {
        tree.layout(SizeProposal::exact(150.0, 150.0));
    }

    tree.dispatch_event(WidgetEvent::PointerUp {
        position: hold,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    assert!(
        selection.is_selected(0),
        "row 0, under the original press point, should stay selected"
    );
    assert!(
        selection.is_selected(15),
        "row 15 was off-screen at press time; auto-scroll must have \
         revealed it and grown the marquee to cover it"
    );
}

#[test]
fn insertion_bar_geometry_uses_target_row_at_row_boundary() {
    // Regression: the bar used to be derived from `tile_rect(ins - 1)` (the
    // PREVIOUS item), so at a row boundary — where `ins` is the first index
    // of a NEW row — it drew on the wrong (previous) row's y/height.
    // 100×50 tiles, 10px gaps, no insets → 4 columns in 430px;
    // row_step = 50 + 10 = 60.
    let g = UniformGrid::new(
        GridSizing::Fixed {
            width: 100.0,
            height: 50.0,
        },
        10.0,
        10.0,
        EdgeInsets::ZERO,
    );
    // Insertion at index 4 = the first tile of row 1 (4 cols/row).
    let (bar_x, r) = insertion_bar_geometry(&g, 4, 12, 430.0).unwrap();
    assert!(
        (r.y - 60.0).abs() < 0.01,
        "row-boundary bar must sit on the TARGET row (y=60), got y={}",
        r.y
    );
    assert!(
        (bar_x - 0.0).abs() < 0.01,
        "bar should sit at row 1's leading edge x=0, got {bar_x}"
    );
}

#[test]
fn insertion_bar_geometry_appends_at_trailing_edge_of_last_tile() {
    let g = UniformGrid::new(
        GridSizing::Fixed {
            width: 100.0,
            height: 50.0,
        },
        10.0,
        10.0,
        EdgeInsets::ZERO,
    );
    // Last tile (index 11, of 12) is row 2 / col 3: x = 3*(100+10) = 330,
    // width 100 → trailing edge at 430.
    let (bar_x, _) = insertion_bar_geometry(&g, 12, 12, 430.0).unwrap();
    assert!((bar_x - 430.0).abs() < 0.01, "append bar_x = {bar_x}");
}

#[test]
fn insertion_bar_geometry_is_none_for_an_empty_grid() {
    let g = UniformGrid::new(
        GridSizing::Fixed {
            width: 100.0,
            height: 50.0,
        },
        10.0,
        10.0,
        EdgeInsets::ZERO,
    );
    assert!(insertion_bar_geometry(&g, 0, 0, 430.0).is_none());
}

#[test]
fn enter_activates_focused_tile() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    use std::cell::Cell;
    use std::rc::Rc;
    let model = ListModel::from_vec((0..12).collect());
    let activated = Rc::new(Cell::new(usize::MAX));
    let a = activated.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .on_tile_activate(move |idx, _ctx| a.set(idx)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    // Move focus to index 1: the first ArrowRight lands on tile 0 (no cursor
    // yet), the second steps to 1. Then Enter activates it.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert_eq!(activated.get(), 1);
}

#[test]
fn type_ahead_jumps_to_match() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let names = vec![
        "apple".to_string(),
        "banana".to_string(),
        "cherry".to_string(),
        "date".to_string(),
    ];
    let model = ListModel::from_vec(names.clone());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel)
            .type_ahead_label(move |i| names[i].clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    // Typing 'c' jumps to "cherry" (index 2).
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Character('c'),
        modifiers: Modifiers::default(),
        text: Some("c".to_string()),
    });
    assert!(selection.is_selected(2), "type-ahead 'c' selects cherry");
}

#[test]
fn type_ahead_fires_on_letter_key_variant() {
    // Regression: letters arrive as the dedicated `Key::B`..`Key::Z` variants,
    // not `Key::Character`. The handler matched only `Key::Character`, so
    // pressing a real letter key never triggered type-ahead. `press_key` sends
    // the `Key::B` variant a real keyboard would.
    use bastyde_core::event::{Key, Modifiers};
    let names = vec![
        "apple".to_string(),
        "banana".to_string(),
        "cherry".to_string(),
    ];
    let model = ListModel::from_vec(names.clone());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel)
            .type_ahead_label(move |i| names[i].clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    tree.press_key(Key::B, Modifiers::NONE);
    assert!(
        selection.is_selected(1),
        "letter key 'B' must trigger type-ahead → banana"
    );
}

#[test]
fn type_ahead_skips_unloaded_rows() {
    // Regression: type-ahead searched every index's label regardless of
    // whether the row was actually resident. The public
    // `type_ahead_label(usize) -> String` closure is index-only and can't
    // itself tell whether its row is loaded, so an unloaded (lazy /
    // windowed) row could still "match" and get jumped to. The search is
    // now gated through the source's string accessor, which returns
    // `None` for an unloaded row, so it's skipped — mirrors `ListView`'s
    // `with_item_str_fn` routing.
    use bastyde_core::ObserverHandle;
    use bastyde_core::event::{Key, Modifiers};
    use bastyde_data::ListDataSource;

    struct PartiallyLoaded;
    impl ListDataSource for PartiallyLoaded {
        type Item = String;
        type Key = usize;
        fn len(&self) -> usize {
            4
        }
        fn with_item<R>(&self, index: usize, f: impl FnOnce(&String) -> R) -> Option<R> {
            // Row 1 ("banana") is never resident — a windowed placeholder.
            if index == 1 || index >= 4 {
                return None;
            }
            let names = ["apple", "banana", "cherry", "date"];
            Some(f(&names[index].to_string()))
        }
        fn key_at(&self, index: usize) -> Option<usize> {
            (index < 4).then_some(index)
        }
        fn observe_changes(
            &self,
            _f: impl Fn(&bastyde_data::DataChange) + 'static,
        ) -> ObserverHandle {
            let inner: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(());
            ObserverHandle::new(inner, 0, std::rc::Rc::new(|_| {}))
        }
    }

    let names = ["apple", "banana", "cherry", "date"];
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::from_source(PartiallyLoaded, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel)
            .type_ahead_label(move |i| names[i].to_string()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    tree.press_key(Key::B, Modifiers::NONE);
    assert!(
        selection.selected_indices().is_empty(),
        "type-ahead must skip an unloaded row ('banana') rather than jump to it"
    );
}

#[test]
fn fetch_more_fires_when_scrolled_near_the_end() {
    // Incremental loading now flows through the source's `can_fetch_more` /
    // `fetch_more` capabilities (the old `on_near_end` hook is gone): as the
    // realized window nears the end, the body pane asks the source to grow.
    use bastyde_core::ObserverHandle;
    use bastyde_data::ListDataSource;
    use std::cell::Cell;
    use std::rc::Rc;

    struct Growing {
        total: usize,
        fetched: Rc<Cell<bool>>,
    }
    impl ListDataSource for Growing {
        type Item = usize;
        type Key = usize;
        fn len(&self) -> usize {
            self.total
        }
        fn with_item<R>(&self, i: usize, f: impl FnOnce(&usize) -> R) -> Option<R> {
            (i < self.total).then(|| f(&i))
        }
        fn key_at(&self, i: usize) -> Option<usize> {
            (i < self.total).then_some(i)
        }
        fn can_fetch_more(&self) -> bool {
            true
        }
        fn fetch_more(&self) {
            self.fetched.set(true);
        }
        fn observe_changes(
            &self,
            _f: impl Fn(&bastyde_data::DataChange) + 'static,
        ) -> ObserverHandle {
            let inner: Rc<dyn std::any::Any> = Rc::new(());
            ObserverHandle::new(inner, 0, Rc::new(|_| {}))
        }
    }

    let fetched = Rc::new(Cell::new(false));
    let source = Growing {
        total: 300,
        fetched: fetched.clone(),
    };
    let mut tree = WidgetTree::new();
    let gv = GridView::from_source(source, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0);
    let scroll = gv.scroll_y_signal().clone();
    let max_sig = gv.max_scroll_y_signal().clone();
    let _id = tree.add(gv);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // At the top the window is far from the end — no fetch yet.
    assert!(
        !fetched.get(),
        "fetch_more must not fire while far from the end"
    );
    // Scroll to the bottom; the body pane re-realizes and asks the source to
    // fetch the next page as the window nears the end.
    scroll.set(max_sig.get());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        fetched.get(),
        "fetch_more should fire as the window nears the end"
    );
}

#[test]
fn custom_grid_view_style_is_accepted() {
    // A Tier-3 style override installs without breaking layout.
    struct LoudStyle;
    impl bastyde_core::styles::GridViewStyle for LoudStyle {
        fn focus_ring(&self) -> bastyde_core::styles::GridFocusRingRecipe {
            bastyde_core::styles::GridFocusRingRecipe {
                role: bastyde_tokens::BorderRole::Accent,
                thickness: 3.0,
                inset: 0.0,
            }
        }
    }
    let model = ListModel::from_vec((0..12).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .style(LoudStyle),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tiles(&tree, id).is_empty());
}

#[test]
fn on_selection_changed_fires() {
    use std::cell::Cell;
    use std::rc::Rc;
    let model = ListModel::from_vec((0..6).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let fired = Rc::new(Cell::new(0u32));
    let f = fired.clone();
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel)
            .on_selection_changed(move |_set| f.set(f.get() + 1)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    selection.select(2);
    assert!(fired.get() >= 1, "on_selection_changed should fire");
}

#[test]
fn reactive_sizing_signal_reflows_and_preserves_scroll() {
    // `.sizing(Signal<GridSizing>)` drives a live card-size change: mutating the
    // signal reflows the columns, and — because it is a Rebuild on the SAME grid
    // instance — the internal `scroll_y` field signal survives (no jump to top).
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let sizing = Signal::new(GridSizing::Fixed {
        width: 100.0,
        height: 50.0,
    });
    let mut tree = WidgetTree::new();
    let gv = GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0))).sizing(sizing.clone());
    let scroll = gv.scroll_y_signal().clone();
    let id = tree.add(gv);

    // Width 400, 100-wide tiles → 3 columns: tiles 0,1,2 share row 0.
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    let row_delta = tree.bounds(t[2]).y - tree.bounds(t[0]).y;
    assert!(
        row_delta.abs() < 0.01,
        "3 columns expected — tile 2 on row 0 with tile 0, Δy = {row_delta}"
    );

    // Grow the tiles to 190 wide → only 2 columns now: tile 2 wraps to row 1
    // (one row-stride below tile 0). Positions compared relatively so the check
    // is independent of any scroll offset.
    sizing.set(GridSizing::Fixed {
        width: 190.0,
        height: 50.0,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    let row_delta = tree.bounds(t[2]).y - tree.bounds(t[0]).y;
    assert!(
        (row_delta - 58.0).abs() < 0.01,
        "after resize to 2 columns, tile 2 should wrap to row 1 (Δy ≈ 58), got {row_delta}"
    );

    // Scroll preservation: with content far taller than the viewport, scroll
    // down, then change the sizing again. Because the reflow is a rebuild on the
    // SAME grid instance, the `scroll_y` field signal is retained (no jump).
    scroll.set(60.0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    sizing.set(GridSizing::Fixed {
        width: 100.0,
        height: 50.0,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        (scroll.get() - 60.0).abs() < 0.01,
        "scroll_y should survive the sizing change, got {}",
        scroll.get()
    );
}
