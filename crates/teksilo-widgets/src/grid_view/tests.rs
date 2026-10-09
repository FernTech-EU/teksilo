// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless tests for `GridView` — the uniform grid, variable row heights and
//! anchoring, reorder / activation / type-ahead / incremental loading, sections
//! and waterfall.
//!
//! No GPU / display server needed — exercises virtualization, column-count
//! derivation, tile placement, selection, keyboard navigation, data-change
//! reconciliation, and accessibility roles.

use super::*;
use teksilo_canvas::SizeProposal;
use teksilo_core::widget::LayoutContext;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel};

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

/// A grid of `count` items, fixed 100×50 tiles, default 8px gaps.
fn make_grid(count: usize) -> (WidgetTree, WidgetId, ListModel<usize>) {
    let model = ListModel::from_vec((0..count).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0))).tile_size(100.0, 50.0),
    );
    (tree, id, model)
}

/// The body pane. The grid's first child is the `DragSurface` that hosts the
/// marquee's drag — it has to strictly enclose whatever captures the press, or
/// the drag beats the view's own `PanClaim`; see `GridView`'s module docs — and
/// the pane is its single child.
fn body_pane(tree: &WidgetTree, grid_id: WidgetId) -> WidgetId {
    let surface = tree.children(grid_id)[0];
    tree.children(surface)[0]
}

/// The tile wrappers, in body order.
fn tiles(tree: &WidgetTree, grid_id: WidgetId) -> Vec<WidgetId> {
    tree.children(body_pane(tree, grid_id))
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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

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
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(t[2]),
        target_node: teksilo_core::accessibility::root_node_id(),
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
        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(id),
            target_node: teksilo_core::accessibility::root_node_id(),
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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

    let model = ListModel::from_vec((0..20).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    // Box width 120 fits exactly one 100px tile → a tall single column.
    let grid = GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0)
        .selection(sel);
    let grid_id = tree.add(grid);
    let grid_box = tree.add(FixedSize::new().width(120.0).height(200.0).child(grid_id));
    let filler = tree.add(FixedLeaf(120.0, 200.0));
    let outer_content = tree.add(VStack::new().child(grid_box).child(filler));
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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
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
        modifiers: Modifiers::COMMAND,
        text: None,
    });
    assert_eq!(selection.count(), 12, "Ctrl+A selects every item");
}

#[test]
fn ctrl_arrow_moves_cursor_without_selecting() {
    // 3 columns fit 400px (3*100 + 2*8 = 316 <= 400).
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
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

/// In a single selection the accelerator moves the selection with the cursor:
/// there is nothing to assemble, and a cursor off the selection would be a
/// second current tile. See the `ListView` twin.
#[test]
fn ctrl_arrow_moves_the_selection_with_the_cursor_in_single_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, selection) = grid_keyboard_fixture(SelectionMode::Single);

    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![0]);

    tree.press_key(Key::ArrowRight, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![1],
        "Ctrl+ArrowRight selects in Single mode"
    );
    assert_eq!(grid_focus(&tree, id), Some(1), "and the cursor is on it");

    // The vertical pair too: one row of three columns down.
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![4]);
    assert_eq!(grid_focus(&tree, id), Some(4));
}

#[test]
fn container_has_grid_role_and_counts() {
    let (mut tree, id, _model) = make_grid(30);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::Grid);
}

#[test]
fn tiles_have_gridcell_role() {
    let (mut tree, id, _model) = make_grid(12);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    let info = tree.accessibility_node(t[0]);
    assert_eq!(info.role(), teksilo_core::accesskit::Role::GridCell);
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
        teksilo_core::accesskit::Role::GridCell
    );
}

// ── What the grid says about its selection ─────────────────────────────

/// Every live node the platform adapters walk in `update`, with the name they
/// would announce, read through `accesskit_consumer` the way all three do: in
/// the filtered tree, live itself or through an ancestor, and named (a
/// `Label` by its value, anything else by its label).
fn live_names(update: &teksilo_core::accesskit::TreeUpdate) -> Vec<String> {
    use accesskit_consumer::{FilterResult, Tree, common_filter};
    let tree = Tree::new(update.clone(), true);
    let state = tree.state();
    let mut out = Vec::new();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        let name = if node.label_comes_from_value() {
            node.value()
        } else {
            node.label()
        };
        if common_filter(&node) == FilterResult::Include
            && node.live() != teksilo_core::accesskit::Live::Off
            && let Some(name) = name
        {
            out.push(name);
        }
        stack.extend(node.children());
    }
    out
}

/// The updates a running window delivers after a change: the frame it is
/// drawn in, and the next. A change that moves the grid's current tile moves
/// focus, and a message raised with it waits for the next update, so a reader
/// hears it after the tile (`teksilo_core::announcer`).
fn next_frames(tree: &mut WidgetTree) -> teksilo_core::accesskit::TreeUpdate {
    let _ = tree.sync_accessibility();
    tree.sync_accessibility()
}

/// A French grid of twelve named tiles over `mode`, laid out and focused.
fn french_grid(mode: SelectionMode) -> (WidgetTree, WidgetId, SelectionModel) {
    let (_mgr, mut tree) = crate::common::locale_switch_test::speaking("fr-FR");
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let selection = SelectionModel::new(mode);
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection.clone())
            .tile_a11y_label(|i| format!("Photo {i}")),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    let _ = tree.sync_accessibility();
    (tree, id, selection)
}

fn press(
    tree: &mut WidgetTree,
    key: teksilo_core::event::Key,
    modifiers: teksilo_core::event::Modifiers,
) {
    tree.dispatch_event(teksilo_core::event::WidgetEvent::KeyDown {
        key,
        modifiers,
        text: None,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// The grid is not a live region, and so neither is any tile. It used to be
/// one whenever it had a selection model, and `accesskit_consumer` hands a
/// node's `live` down to every descendant that sets none, so each named tile
/// was announced by every platform as it arrived in the realized window.
#[test]
fn neither_the_grid_nor_its_tiles_are_live() {
    let (mut tree, _id, _selection) = french_grid(SelectionMode::Multi);
    let update = tree.sync_accessibility();
    assert_eq!(live_names(&update), Vec::<String>::new());
    teksilo_i18n::thread_local::clear();
}

/// What the grid used to hold as a silent live value is said when the user
/// changes it, once per change, in the user's language.
#[test]
fn a_selection_the_user_changes_is_counted_aloud() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, selection) = french_grid(SelectionMode::Multi);
    // What arrives, as the adapters announce it: the announcer's node for a
    // message stays in the tree for a while after it is said, so a live name
    // present in an update is not a message said in it.
    let mut seen = tree.announcements_since(0).last().map_or(0, |a| a.seq);
    let mut heard = |tree: &mut teksilo_core::widget_tree::WidgetTree| {
        let _ = next_frames(tree);
        let got = tree.announcements_since(seen);
        if let Some(last) = got.last() {
            seen = last.seq;
        }
        got.into_iter().map(|a| a.text).collect::<Vec<_>>()
    };

    press(&mut tree, Key::Space, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![0]);
    assert_eq!(heard(&mut tree), vec!["1 élément sélectionné"]);

    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    press(&mut tree, Key::Space, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0, 1]);
    assert_eq!(heard(&mut tree), vec!["2 éléments sélectionnés"]);
    teksilo_i18n::thread_local::clear();
}

/// In a single selection the cursor carries the selection, so an arrow changes
/// which tile is selected and not how many. The tile the reader lands on says
/// it is selected; a count on top of it would be said on every key.
#[test]
fn moving_a_single_selection_says_no_count() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, selection) = french_grid(SelectionMode::Single);
    press(&mut tree, Key::Space, Modifiers::NONE);
    let _ = tree.sync_accessibility();
    let _ = tree.sync_accessibility();
    let seen = tree.announcements_since(0).last().map_or(0, |a| a.seq);

    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![1]);
    let _ = next_frames(&mut tree);
    assert!(
        tree.announcements_since(seen).is_empty(),
        "an arrow that moves a single selection must say no count"
    );
    teksilo_i18n::thread_local::clear();
}

/// A click on a tile is a user's change like a key.
#[test]
fn a_click_that_selects_a_tile_is_counted_aloud() {
    let (mut tree, id, selection) = french_grid(SelectionMode::Multi);
    let t = tiles(&tree, id);
    tree.click(t[2]);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(selection.is_selected(2));
    let update = next_frames(&mut tree);
    assert_eq!(live_names(&update), vec!["1 élément sélectionné"]);
    teksilo_i18n::thread_local::clear();
}

/// An assistive click on a tile selects it, as a click does, and is counted
/// the same way: it is how a screen reader user selects one directly.
#[test]
fn an_assistive_click_that_selects_a_tile_is_counted_aloud() {
    let (mut tree, id, selection) = french_grid(SelectionMode::Multi);
    let t = tiles(&tree, id);
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Click,
        target: Some(t[3]),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(selection.is_selected(3));
    let update = next_frames(&mut tree);
    assert_eq!(live_names(&update), vec!["1 élément sélectionné"]);
    teksilo_i18n::thread_local::clear();
}

/// A marquee is a user's change too, said once, when the band is let go and
/// the tiles under it are selected.
#[test]
fn a_marquee_that_selects_tiles_is_counted_aloud() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let (_mgr, mut tree) = crate::common::locale_switch_test::speaking("fr-FR");
    // One 100px column in a 150px viewport, so the strip right of the tiles
    // is background, where a press starts a marquee rather than a tile drag.
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection.clone()),
    );
    tree.layout(SizeProposal::exact(150.0, 150.0));
    let _ = tree.sync_accessibility();

    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(120.0, 10.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(120.0, 16.0)));
    let end = Point::new(50.0, 120.0);
    tree.dispatch_event(WidgetEvent::pointer_move(end));
    tree.layout(SizeProposal::exact(150.0, 150.0));
    tree.dispatch_event(WidgetEvent::pointer_up(
        end,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal::exact(150.0, 150.0));
    assert_eq!(selection.selected_indices(), vec![0, 1, 2]);
    let update = next_frames(&mut tree);
    assert_eq!(live_names(&update), vec!["3 éléments sélectionnés"]);
    teksilo_i18n::thread_local::clear();
}

/// The grid's value is the count in the user's language, for a screen reader
/// that reports the value with the grid. It was English in every language.
#[test]
fn the_grids_value_is_the_count_in_the_users_language() {
    let (mut tree, id, selection) = french_grid(SelectionMode::Multi);
    selection.select_all(3);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let update = tree.sync_accessibility();
    let grid = teksilo_core::accessibility::widget_id_to_node_id(id);
    let value = update
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == grid)
        .and_then(|(_, node)| node.value());
    assert_eq!(value, Some("3 éléments sélectionnés"));
    teksilo_i18n::thread_local::clear();
}

/// Space on a tile the grid has not realized, because the view was scrolled
/// away from the cursor, selects it there and then, inside the key handler,
/// rather than after it through `row_space_activate`. Both the handler and
/// that selection are wrapped to count aloud, and the count must still be
/// said once, not once by each.
#[test]
fn space_on_a_tile_scrolled_out_of_the_window_is_counted_once() {
    use teksilo_core::event::{Key, Modifiers};
    let (_mgr, mut tree) = crate::common::locale_switch_test::speaking("fr-FR");
    let model = ListModel::from_vec((0..300).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let grid = GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0)
        .selection(selection.clone())
        .tile_a11y_label(|i| format!("Photo {i}"));
    let scroll = grid.scroll_y_signal().clone();
    let max_scroll = grid.max_scroll_y_signal().clone();
    let id = tree.add(grid);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    scroll.set(max_scroll.get());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.sync_accessibility();
    let seen = tree.announcements_since(0).last().map_or(0, |a| a.seq);

    press(&mut tree, Key::Space, Modifiers::NONE);
    assert_eq!(selection.selected_indices(), vec![0]);
    for _ in 0..6 {
        let _ = tree.sync_accessibility();
    }
    let heard: Vec<String> = tree
        .announcements_since(seen)
        .into_iter()
        .map(|a| a.text)
        .collect();
    assert_eq!(heard, vec!["1 élément sélectionné"]);
    teksilo_i18n::thread_local::clear();
}

/// With no translations installed, the count is said in the framework's own
/// English. `grid-view-selection-count` picks its words by a plural, which
/// `tr_widget!` could not put back together without a manager, so the reader
/// was handed the message id to say.
#[test]
fn the_count_is_said_in_words_with_no_translations_installed() {
    use crate::common::heard_test::{Heard, Listener};
    use teksilo_core::event::{Key, Modifiers};
    teksilo_i18n::thread_local::clear();
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection.clone())
            .tile_a11y_label(|i| format!("Photo {i}")),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    let mut listener = Listener::attach(&mut tree);
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    let _ = listener.heard(&mut tree);

    press(&mut tree, Key::Space, Modifiers::CTRL);
    assert!(selection.is_selected(0));
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("1 item selected".to_string())]
    );
    press(&mut tree, Key::A, Modifiers::COMMAND);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("12 items selected".to_string())]
    );
    press(&mut tree, Key::A, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("No item selected".to_string())]
    );
}

/// A change of selection leaves the tile the reader is on as the node it
/// was. Every realized tile used to be rebuilt, so the focused tile came back
/// as a new node: each adapter reported a focus change to it, which Orca
/// takes as a new locus of focus and so stops what it is saying, the count
/// just announced included, and a reader never heard the tile's `selected`
/// state change, since no node it knew ever changed.
#[test]
fn a_selection_change_keeps_the_readers_tile_and_changes_its_state() {
    use crate::common::heard_test::{Heard, Listener};
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, selection) = french_grid(SelectionMode::Multi);
    let mut listener = Listener::attach(&mut tree);
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("Photo 0".to_string())]
    );
    let (tile, selected) = listener.focus().expect("the reader holds a focus");
    assert_eq!(selected, Some(false));
    let realized = tiles(&tree, id);

    press(&mut tree, Key::Space, Modifiers::CTRL);
    assert!(selection.is_selected(0));
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("1 élément sélectionné".to_string())],
        "the count, and no focus change after it"
    );
    assert_eq!(
        listener.focus(),
        Some((tile, Some(true))),
        "the reader's tile is the same node, now selected"
    );

    press(&mut tree, Key::A, Modifiers::COMMAND);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Live("12 éléments sélectionnés".to_string())]
    );
    assert_eq!(listener.focus(), Some((tile, Some(true))));
    assert_eq!(
        tiles(&tree, id),
        realized,
        "no realized tile is replaced by a change of selection"
    );
    teksilo_i18n::thread_local::clear();
}

/// A tile is the grid's active descendant and takes no keys of its own. It
/// used to offer `Action::Focus`, which the dispatcher services by moving
/// keyboard focus onto the node named, focusable or not. After a UIA
/// `SetFocus` on a tile, an AT-SPI `grab_focus`, or VoiceOver's keyboard focus
/// following its cursor there, keyboard focus sat on that tile while the
/// grid's cursor stayed where it was: Enter opened the cursor's tile, not the
/// one the reader had just been told about, and the next key that rebuilt the
/// tiles dropped focus onto the window.
#[test]
fn an_assistive_focus_on_a_tile_leaves_the_keyboard_on_the_grid() {
    use crate::common::heard_test::{Heard, Listener};
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::event::{Key, Modifiers};
    let activated = Rc::new(Cell::new(None));
    let (_mgr, mut tree) = crate::common::locale_switch_test::speaking("fr-FR");
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let on_activate = activated.clone();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection)
            .tile_a11y_label(|i| format!("Photo {i}"))
            .on_tile_activate(move |i, _ctx| on_activate.set(Some(i))),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    let mut listener = Listener::attach(&mut tree);
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    assert_eq!(
        listener.heard(&mut tree),
        vec![Heard::Focus("Photo 0".to_string())]
    );

    let tile = tiles(&tree, id)[2];
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Focus,
        target: Some(tile),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tree.focused(), Some(id), "keyboard focus stays on the grid");
    assert_eq!(listener.heard(&mut tree), vec![]);

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(
        activated.get(),
        Some(0),
        "Enter opens the tile the reader is on"
    );
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(id));
    // The count is raised in the update that moves the reader to the next
    // tile, so it is held back until that move has been heard: Orca stops
    // speaking to read a new focus, and would cut it.
    assert_eq!(
        listener.heard(&mut tree),
        vec![
            Heard::Focus("Photo 1".to_string()),
            Heard::Live("1 élément sélectionné".to_string()),
        ]
    );
    assert!(
        !tree
            .accessibility_node(tile)
            .actions()
            .contains(&teksilo_core::accesskit::Action::Focus),
        "a tile offers no focus of its own"
    );
    teksilo_i18n::thread_local::clear();
}

/// A change of selection draws again the tiles whose selectedness it flipped,
/// and no other. Every tile hears every change of the selection, including a
/// selection set again to what it was, so a tile that drew itself again on
/// each would replace its delegate's widgets on every key and every click.
#[test]
fn a_selection_change_rebuilds_only_the_tiles_it_flips() {
    use teksilo_core::event::{Key, Modifiers};
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    let contents = |tree: &WidgetTree| -> Vec<Vec<WidgetId>> {
        tiles(tree, id)
            .into_iter()
            .map(|tile| tree.children(tile))
            .collect()
    };
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    let before = contents(&tree);

    press(&mut tree, Key::Space, Modifiers::CTRL);
    assert!(selection.is_selected(0));
    let after = contents(&tree);
    assert_ne!(
        after[0], before[0],
        "the tile Space selected is drawn again"
    );
    assert_eq!(after[1..], before[1..], "and no other tile is");

    selection.select(0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        contents(&tree),
        after,
        "a selection set again to what it was draws no tile again"
    );
}

/// A double click opens a tile that is already selected. Its first click sets
/// the selection the grid already had. Every realized tile used to be
/// replaced on that, so the second click landed on a tile that had never
/// seen the first, and nothing opened.
#[test]
fn a_double_click_opens_a_tile_that_is_already_selected() {
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let activated = Rc::new(Cell::new(None));
    let on_activate = activated.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(selection.clone())
            .on_tile_activate(move |i, _ctx| on_activate.set(Some(i))),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    selection.select(1);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let bounds = tree.bounds(tiles(&tree, id)[1]);
    let at = teksilo_canvas::Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    for _ in 0..2 {
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.layout(SizeProposal::exact(400.0, 300.0));
    }
    assert!(selection.is_selected(1));
    assert_eq!(activated.get(), Some(1), "the double click opens the tile");
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
    let body_kids = tiles(&tree, id);
    // First 6 body children are tiles (flat order), then 2 headers.
    let tile0 = tree.bounds(body_kids[0]);
    let tile3 = tree.bounds(body_kids[3]);
    assert!((tile0.y - 28.0).abs() < 0.5, "tile 0 y = {}", tile0.y);
    assert!((tile3.y - 172.0).abs() < 0.5, "tile 3 y = {}", tile3.y);
    // The header wrappers carry RowHeader role.
    let header0 = tree.accessibility_node(body_kids[6]);
    assert_eq!(
        header0.role(),
        teksilo_core::accesskit::Role::RowHeader,
        "section header should be RowHeader"
    );
}

#[test]
fn sections_report_section_local_aria_row_col() {
    // Item 3 is the FIRST item of section 1 (items 0, 1, 2 belong to
    // section 0 — see `sections_offset_tiles_below_headers` above), so it
    // must report its position within ITS OWN section band, not the answer
    // global `index / cols, index % cols` math would give.
    //
    // The widget passes ARIA's 1-based row 1 / column 1; AccessKit stores both
    // zero-based, so the node carries 0 / 0 and the Windows and AT-SPI adapters
    // add the 1 back before speaking it.
    let model = ListModel::from_vec((0..6).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 50.0)))
            .column_count(2, 50.0)
            .section_header_height(28.0)
            .sections(TwoSections),
    );
    tree.layout(SizeProposal::exact(300.0, 600.0));
    let body_kids = tiles(&tree, id);
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
        Some(0),
        "item 3 is the first row of its OWN section"
    );
    assert_eq!(
        node.column_index(),
        Some(0),
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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
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
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // Cross the drag threshold, then move past the last tile (insertion = end).
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(72.0, 25.0)));
    let to = Point::new(430.0, 25.0);
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::NONE,
    ));

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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
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
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // Cross the drag threshold, then move into the row-gap: y=53 sits
    // between row 0 (0..50) and row 1 (58..108); x=70 is inside column 0,
    // past its horizontal center.
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(72.0, 25.0)));
    let to = Point::new(70.0, 53.0);
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::NONE,
    ));

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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

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
    tree.dispatch_event(WidgetEvent::pointer_down(
        press,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // Cross the 5px drag threshold.
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(120.0, 16.0)));
    // Sweep into the tile column (x=50) and past the bottom edge — deep
    // enough into the edge band that the pointer never needs to move
    // again for auto-scroll to keep going.
    let hold = Point::new(50.0, 200.0);
    tree.dispatch_event(WidgetEvent::pointer_move(hold));

    // Pump enough frame ticks for the auto-scroll effect to run well past
    // one screenful. Each `layout()` call advances at most one tick (see
    // `WidgetTree::advance_frame_tick`), and the tick handler re-arms
    // itself every time it's still in the edge band, so a fixed loop of
    // `tree.layout()` calls is deterministic here — no real elapsed time
    // or sleeping needed.
    for _ in 0..80 {
        tree.layout(SizeProposal::exact(150.0, 150.0));
    }

    tree.dispatch_event(WidgetEvent::pointer_up(
        hold,
        PointerButton::Primary,
        Modifiers::NONE,
    ));

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
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
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
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
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
    use teksilo_core::event::{Key, Modifiers};
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
    use teksilo_core::ObserverHandle;
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::ListDataSource;

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
            _f: impl Fn(&teksilo_data::DataChange) + 'static,
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
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::ObserverHandle;
    use teksilo_data::ListDataSource;

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
            _f: impl Fn(&teksilo_data::DataChange) + 'static,
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
    impl teksilo_core::styles::GridViewStyle for LoudStyle {
        fn focus_ring(&self) -> teksilo_core::styles::GridFocusRingRecipe {
            teksilo_core::styles::GridFocusRingRecipe {
                role: teksilo_tokens::BorderRole::Accent,
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

// ── Edge keys, paging and the selection verbs ──────────────────────────────

/// A 30-tile grid, 3 columns to a 400 dp row, 50 dp tiles in a 300 dp
/// viewport (six rows visible). Focused and ready for keys.
fn grid_keyboard_fixture(mode: SelectionMode) -> (WidgetTree, WidgetId, SelectionModel) {
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let selection = SelectionModel::new(mode);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    (tree, id, selection)
}

fn grid_focus(tree: &WidgetTree, id: WidgetId) -> Option<usize> {
    tree.widget_as_any(id)
        .and_then(|any| any.downcast_ref::<GridView<usize>>())
        .and_then(|g| g.focused_index.get())
}

#[test]
fn home_and_end_reach_the_ends_of_the_collection_not_the_reflow_row() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Single);
    sel.select(13); // middle of row 4, so a row-scoped Home would land on 12
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.press_key(Key::Home, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        grid_focus(&tree, id),
        Some(0),
        "a wrapped grid's rows are a reflow artifact, so Home is absolute"
    );

    tree.press_key(Key::End, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(grid_focus(&tree, id), Some(29));
}

#[test]
fn the_accelerator_moves_the_grid_cursor_without_selecting() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    sel.select(13);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.press_key(Key::End, Modifiers::COMMAND);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(grid_focus(&tree, id), Some(29), "the cursor moved");
    assert_eq!(sel.selected_indices(), vec![13], "the selection did not");
}

/// The edge-and-page keys follow the same rule in a single selection.
#[test]
fn the_accelerator_moves_the_selection_with_the_grid_cursor_in_single_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Single);
    sel.select(13);
    let p = SizeProposal::exact(400.0, 300.0);
    tree.layout(p);

    for key in [Key::End, Key::Home, Key::PageDown, Key::PageUp] {
        let before = sel.selected_indices();
        tree.press_key(key, Modifiers::COMMAND);
        tree.layout(p);
        let selected = sel.selected_indices();
        assert_ne!(
            selected, before,
            "the accelerator with {key:?} moves the selection"
        );
        assert_eq!(
            selected,
            grid_focus(&tree, id).into_iter().collect::<Vec<_>>(),
            "onto the cursor's tile, after {key:?}"
        );
    }
}

/// A selection the application writes moves the grid cursor onto it, in a
/// single selection — see the `ListView` twin. A grid publishes its cursor
/// alone as the active descendant, so the cursor is moved, not cleared.
#[test]
fn a_selection_set_from_outside_moves_the_grid_cursor_in_single_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Single);
    for _ in 0..3 {
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
    }
    assert_eq!(grid_focus(&tree, id), Some(2));

    sel.select(7);
    assert_eq!(
        grid_focus(&tree, id),
        Some(7),
        "the cursor is on the selection"
    );
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![8]);

    // A multiple selection keeps its cursor where the user left it.
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    for _ in 0..3 {
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
    }
    sel.select(7);
    assert_eq!(grid_focus(&tree, id), Some(2), "a Multi cursor stays");
}

/// An insert above the cursor shifts the cursor and the selection by the same
/// one tile. The cursor is shifted before the selection, because shifting the
/// selection puts the cursor on it in a single selection — shifted after, it
/// would move twice.
#[test]
fn an_insert_above_the_cursor_shifts_it_once_in_single_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
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
    for key in [Key::ArrowRight, Key::ArrowDown, Key::ArrowRight] {
        tree.press_key(key, Modifiers::NONE);
    }
    let before = grid_focus(&tree, id).expect("a cursor");
    assert!(before > 0, "the cursor has left the first tile");
    assert_eq!(selection.selected_indices(), vec![before]);

    model.insert(0, 100);
    assert_eq!(selection.selected_indices(), vec![before + 1]);
    assert_eq!(
        grid_focus(&tree, id),
        Some(before + 1),
        "the cursor moved once"
    );
}

/// An observer registered before the grid may rewrite the selection while it
/// is being notified — a redirect, a normaliser. The notification it
/// interrupted still reaches the grid afterwards, carrying the discarded
/// value, so the grid reads the live selection: read from the notification,
/// the cursor would land on the tile nothing selects.
#[test]
fn a_redirected_selection_takes_the_grid_cursor_where_it_ended() {
    use teksilo_core::event::{Key, Modifiers};
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Single);
    let redirect_to = selection.clone();
    let _redirect = selection.selection_signal().observe(move |selected| {
        if selected.contains(&7) {
            redirect_to.select(8);
        }
    });
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    tree.press_key(Key::ArrowRight, Modifiers::NONE);

    selection.select(7);
    assert_eq!(selection.selected_indices(), vec![8]);
    assert_eq!(grid_focus(&tree, id), Some(8));
}

#[test]
fn paging_moves_about_a_viewport_of_rows_and_always_makes_progress() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Single);
    sel.select(0);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.press_key(Key::PageDown, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let after = grid_focus(&tree, id).expect("a cursor");
    assert!(
        (12..=18).contains(&after),
        "300 dp of 50 dp rows is ~6 rows of 3 columns; got {after}"
    );

    tree.press_key(Key::PageUp, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(grid_focus(&tree, id), Some(0), "and back again");
}

#[test]
fn shift_end_then_shift_home_leaves_a_range_not_the_whole_grid() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    sel.select(10);

    tree.press_key(Key::End, Modifiers::SHIFT);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.count(), 20, "10..=29");

    tree.press_key(Key::Home, Modifiers::SHIFT);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.selected_indices(), (0..=10).collect::<Vec<_>>());
}

#[test]
fn ctrl_a_does_nothing_to_a_single_selection_grid() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, sel) = grid_keyboard_fixture(SelectionMode::Single);
    sel.select(4);
    tree.press_key(Key::A, Modifiers::COMMAND);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        sel.selected_indices(),
        vec![4],
        "select-all has no reading for a control holding at most one tile"
    );
}

#[test]
fn ctrl_shift_a_deselects_a_multi_selection_grid() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    tree.press_key(Key::A, Modifiers::COMMAND);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.count(), 30);

    tree.press_key(Key::A, Modifiers::COMMAND | Modifiers::SHIFT);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.count(), 0);
}

#[test]
fn space_toggles_a_tile_in_multi_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    sel.select(7);

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.count(), 0, "Space unpicks a picked tile");

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.selected_indices(), vec![7]);
}

#[test]
fn space_selects_without_deselecting_in_single_mode() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, _id, sel) = grid_keyboard_fixture(SelectionMode::Single);
    sel.select(7);
    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sel.selected_indices(), vec![7]);
}

// ── A tile's own controls ─────────────────────────────────────────────────

#[test]
fn a_grid_is_one_tab_stop_however_many_tiles_are_realized() {
    // A checkbox in the tile delegate used to put every realized tile in the
    // Tab order — 40 stops in this fixture, and a different 40 after
    // scrolling.
    let checked = teksilo_core::signal::Signal::new(false);
    let ck = checked.clone();
    let model = ListModel::from_vec((0..200).collect::<Vec<usize>>());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        GridView::new(model, move |_tc| Box::new(crate::Checkbox::new(ck.clone())))
            .tile_size(100.0, 50.0),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let stops = tree.tab_stops_within(id);
    assert_eq!(
        stops.len(),
        1,
        "a grid is one Tab stop; got {}",
        stops.len()
    );
}

#[test]
fn space_checks_the_focused_tile_and_leaves_the_selection_alone() {
    use teksilo_core::event::{Key, Modifiers};
    let checked = teksilo_core::signal::Signal::new(false);
    let ck = checked.clone();
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        GridView::new(model, move |_tc| Box::new(crate::Checkbox::new(ck.clone())))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    let p = SizeProposal::exact(400.0, 300.0);
    tree.layout(p);
    tree.focus(id);
    selection.select(4);
    tree.layout(p);

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(checked.get(), "Space reaches the focused tile's checkbox");
    assert_eq!(
        selection.selected_indices(),
        vec![4],
        "and leaves the selection alone"
    );

    // Ctrl+Space keeps meaning selection.
    tree.press_key(Key::Space, Modifiers::CTRL);
    tree.layout(p);
    assert_eq!(selection.selected_indices(), Vec::<usize>::new());
    assert!(checked.get(), "the check is untouched");
}

// ── Keyed selection ─────────────────────────────────────────────────────────

/// Nine tiles numbered 0 to 8 behind a `SortFilterListModel`, which keys each
/// item by its position in the model underneath, so here a tile's key is its
/// number. The proxy sorts on "n" and, while the "even" filter is set, keeps
/// the even numbers only. Three columns of 100x50 tiles fit 400 dp, so the
/// nine tiles are three rows, all realized.
fn keyed_grid(
    mode: SelectionMode,
) -> (
    WidgetTree,
    WidgetId,
    teksilo_data::SortFilterListModel<usize>,
    teksilo_data::KeyedSelectionModel<usize>,
) {
    let proxy = teksilo_data::SortFilterListModel::new(ListModel::from_vec((0..9).collect()))
        .with_comparator("n", |a: &usize, b: &usize| a.cmp(b))
        .with_predicate("even", |_text| Box::new(|n: &usize| n.is_multiple_of(2)));
    let keyed = teksilo_data::KeyedSelectionModel::<usize>::new(mode);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::from_source_keyed(proxy.clone(), keyed.clone(), |_tc| {
            Box::new(FixedLeaf(100.0, 50.0))
        })
        .tile_size(100.0, 50.0),
    );
    // Twice: the first pass settles the column count, which rebuilds the
    // tiles once.
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, id, proxy, keyed)
}

/// The realized tile at flat `index`, from the map the body pane writes.
fn tile_at(tree: &WidgetTree, grid: WidgetId, index: usize) -> WidgetId {
    tree.widget_as_any(grid)
        .and_then(|any| any.downcast_ref::<GridView<usize>>())
        .and_then(|g| {
            g.tile_map
                .borrow()
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, id)| *id)
        })
        .unwrap_or_else(|| panic!("tile {index} is realized"))
}

/// A primary click on the realized tile at `index`, with `modifiers` held.
fn click_with(
    tree: &mut WidgetTree,
    grid: WidgetId,
    index: usize,
    modifiers: teksilo_core::event::Modifiers,
) {
    use teksilo_core::event::{PointerButton, WidgetEvent};
    let center = tree.bounds(tile_at(tree, grid, index)).center();
    tree.dispatch_event(WidgetEvent::pointer_down(
        center,
        PointerButton::Primary,
        modifiers,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        center,
        PointerButton::Primary,
        modifiers,
    ));
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// The flat index of every tile the accessibility tree marks selected.
/// `position_in_set` reads 0-based in a snapshot; see
/// `focus_reveal_tests::selected_positions`.
fn selected_tiles(tree: &WidgetTree) -> Vec<usize> {
    let mut out: Vec<usize> = tree
        .accessibility_tree_snapshot()
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == teksilo_core::accesskit::Role::GridCell)
        .filter(|(_, node)| node.is_selected() == Some(true))
        .filter_map(|(_, node)| node.position_in_set())
        .collect();
    out.sort_unstable();
    out
}

fn sorted_keys(keyed: &teksilo_data::KeyedSelectionModel<usize>) -> Vec<usize> {
    let mut keys = keyed.selected_keys();
    keys.sort_unstable();
    keys
}

/// Sorting the grid moves a selected tile, and the selection goes with it:
/// tile 1 is the eighth tile once the grid counts down, and it is the one
/// still selected, on screen and to a screen reader.
#[test]
fn a_keyed_selection_follows_its_tile_through_a_sort() {
    use teksilo_data::SortDirection;
    let (mut tree, id, proxy, keyed) = keyed_grid(SelectionMode::Multi);
    tree.click(tile_at(&tree, id, 1));
    assert_eq!(
        sorted_keys(&keyed),
        vec![1],
        "a click stores the tile's key"
    );

    proxy.set_sort(Some("n"), SortDirection::Descending);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sorted_keys(&keyed), vec![1]);
    assert_eq!(
        selected_tiles(&tree),
        vec![7],
        "8, 7, … 1, 0: tile 1 sorts to position 7, and is the tile marked selected"
    );
}

/// Narrowing the filter keeps the selected tiles that are still shown,
/// wherever they land, and drops the one it hides: the contract
/// `ListView::from_source_keyed` keeps.
#[test]
fn a_keyed_selection_follows_its_tiles_through_a_filter() {
    use teksilo_core::event::Modifiers;
    let (mut tree, id, proxy, keyed) = keyed_grid(SelectionMode::Multi);
    tree.click(tile_at(&tree, id, 2));
    click_with(&mut tree, id, 3, Modifiers::COMMAND);
    assert_eq!(sorted_keys(&keyed), vec![2, 3]);

    proxy.set_filter("even", "on");
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        sorted_keys(&keyed),
        vec![2],
        "3 is filtered out and leaves the selection; 2 stays"
    );
    assert_eq!(
        selected_tiles(&tree),
        vec![1],
        "0, 2, 4, 6, 8: tile 2 is now second"
    );
}

/// Clicks with ⌘/Ctrl and Shift held, and a rubber band, all write keys,
/// read off the tiles' current positions: after a sort, position and key no
/// longer agree, so an index written by mistake would select other tiles.
#[test]
fn modifier_clicks_and_the_rubber_band_write_keys() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    use teksilo_data::SortDirection;
    let (mut tree, id, proxy, keyed) = keyed_grid(SelectionMode::Multi);
    proxy.set_sort(Some("n"), SortDirection::Descending);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Positions 0..=8 now hold tiles 8..=0.
    tree.click(tile_at(&tree, id, 0));
    click_with(&mut tree, id, 2, Modifiers::COMMAND);
    assert_eq!(sorted_keys(&keyed), vec![6, 8]);
    click_with(&mut tree, id, 4, Modifiers::SHIFT);
    assert_eq!(
        sorted_keys(&keyed),
        vec![4, 5, 6, 8],
        "Shift extends from the last ⌘-click (position 2, tile 6) to position 4 (tile 4)"
    );

    // A band over the second row (positions 3..=5, tiles 5, 4, 3), pressed on
    // the background right of the third column.
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(360.0, 70.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(360.0, 76.0)));
    let end = Point::new(40.0, 90.0);
    tree.dispatch_event(WidgetEvent::pointer_move(end));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.dispatch_event(WidgetEvent::pointer_up(
        end,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sorted_keys(&keyed), vec![3, 4, 5]);
    assert_eq!(selected_tiles(&tree), vec![3, 4, 5]);
}

/// The selection is reported as positions, read when it changes. A sort moves
/// the selected tile without changing the selection, so it reports nothing.
#[test]
fn a_keyed_grid_reports_its_selection_as_positions() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_data::SortDirection;
    let proxy = teksilo_data::SortFilterListModel::new(ListModel::from_vec((0..9).collect()))
        .with_comparator("n", |a: &usize, b: &usize| a.cmp(b));
    proxy.set_sort(Some("n"), SortDirection::Descending);
    let keyed = teksilo_data::KeyedSelectionModel::<usize>::new(SelectionMode::Multi);
    let reported: Rc<RefCell<Vec<Vec<usize>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = reported.clone();
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::from_source_keyed(proxy.clone(), keyed.clone(), |_tc| {
            Box::new(FixedLeaf(100.0, 50.0))
        })
        .tile_size(100.0, 50.0)
        .on_selection_changed(move |set| sink.borrow_mut().push(set.iter().copied().collect())),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    keyed.select(6);
    assert_eq!(*reported.borrow(), vec![vec![2]], "tile 6 is at position 2");

    proxy.set_sort(Some("n"), SortDirection::Ascending);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        reported.borrow().len(),
        1,
        "a sort does not change the selection"
    );
}

/// In a single selection the grid's cursor is the selected tile, and stays
/// it through a sort. The sort is a reset, which drops the cursor; the
/// selection has not changed, so nothing that watches it puts the cursor
/// back, and the grid would name no tile as its active descendant.
#[test]
fn a_keyed_single_selection_keeps_the_cursor_on_its_tile_through_a_sort() {
    use teksilo_data::SortDirection;
    let (mut tree, id, proxy, keyed) = keyed_grid(SelectionMode::Single);
    tree.focus(id);
    tree.click(tile_at(&tree, id, 1));
    assert_eq!(grid_focus(&tree, id), Some(1));

    proxy.set_sort(Some("n"), SortDirection::Descending);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(sorted_keys(&keyed), vec![1]);
    assert_eq!(
        grid_focus(&tree, id),
        Some(7),
        "the cursor goes with tile 1"
    );

    let snapshot = tree.accessibility_tree_snapshot();
    let active = snapshot
        .nodes
        .iter()
        .find(|(_, node)| node.role() == teksilo_core::accesskit::Role::Grid)
        .and_then(|(_, node)| node.active_descendant())
        .expect("the grid names an active descendant");
    let position = snapshot
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == active)
        .and_then(|(_, node)| node.position_in_set());
    assert_eq!(position, Some(7));
}

/// A lazy source knows its keys before its items. A selected tile scrolled
/// out of the loaded window keeps its place in the selection, a Shift range
/// across tiles that are not loaded selects their keys, and the tile is
/// shown selected again when its row loads back in.
#[test]
fn a_keyed_selection_survives_a_sliding_window() {
    use std::cell::RefCell;
    use std::ops::Range;
    use std::rc::Rc;
    use teksilo_core::ObserverHandle;
    use teksilo_core::event::Modifiers;
    use teksilo_data::{KeyedSelectionModel, ListDataSource, RowState};

    /// Sixty items keyed `1000 + index`, of which only the window the grid
    /// last asked for is loaded.
    struct Windowed {
        loaded: Rc<RefCell<Range<usize>>>,
    }
    impl ListDataSource for Windowed {
        type Item = usize;
        type Key = u64;
        fn len(&self) -> usize {
            60
        }
        fn with_item<R>(&self, i: usize, f: impl FnOnce(&usize) -> R) -> Option<R> {
            self.loaded.borrow().contains(&i).then(|| f(&i))
        }
        fn key_at(&self, i: usize) -> Option<u64> {
            (i < 60).then_some(1000 + i as u64)
        }
        fn index_of(&self, key: &u64) -> Option<usize> {
            let i = key.checked_sub(1000)? as usize;
            (i < 60).then_some(i)
        }
        fn row_state(&self, i: usize) -> RowState {
            if self.loaded.borrow().contains(&i) {
                RowState::Ready
            } else {
                RowState::Loading
            }
        }
        fn request_window(&self, range: Range<usize>) {
            *self.loaded.borrow_mut() = range;
        }
        fn observe_changes(
            &self,
            _f: impl Fn(&teksilo_data::DataChange) + 'static,
        ) -> ObserverHandle {
            ObserverHandle::new(Rc::new(()) as Rc<dyn std::any::Any>, 0, Rc::new(|_| {}))
        }
    }

    let loaded = Rc::new(RefCell::new(0..0));
    let keyed = KeyedSelectionModel::<u64>::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new();
    let grid = GridView::from_source_keyed(
        Windowed {
            loaded: loaded.clone(),
        },
        keyed.clone(),
        |_tc| Box::new(FixedLeaf(100.0, 50.0)),
    )
    .tile_size(100.0, 50.0);
    let scroll = grid.scroll_y_signal().clone();
    let max_scroll = grid.max_scroll_y_signal().clone();
    let id = tree.add(grid);
    let p = SizeProposal::exact(400.0, 300.0);
    tree.layout(p);
    tree.click(tile_at(&tree, id, 2));
    assert_eq!(keyed.selected_keys(), vec![1002]);

    scroll.set(max_scroll.get());
    tree.layout(p);
    tree.layout(p);
    assert!(
        !loaded.borrow().contains(&2),
        "the window has slid past tile 2, which is the case this is about"
    );
    click_with(&mut tree, id, 59, Modifiers::SHIFT);
    assert_eq!(keyed.count(), 58, "1002 through 1059");
    assert!(
        keyed.is_selected(&1010),
        "a tile that is not loaded is selected by its key"
    );

    scroll.set(0.0);
    tree.layout(p);
    tree.layout(p);
    assert!(loaded.borrow().contains(&2));
    let shown = selected_tiles(&tree);
    assert!(
        shown.contains(&2) && !shown.contains(&1),
        "tile 2 is selected again once it is loaded, got {shown:?}"
    );
}

/// A keyed selection changes a tile's state in place, as an index selection
/// does: no tile node is replaced, and only the tile it selects draws again.
#[test]
fn a_keyed_selection_change_rebuilds_only_the_tile_it_flips() {
    let (mut tree, id, _proxy, keyed) = keyed_grid(SelectionMode::Multi);
    let contents = |tree: &WidgetTree| -> Vec<Vec<WidgetId>> {
        tiles(tree, id)
            .into_iter()
            .map(|tile| tree.children(tile))
            .collect()
    };
    let nodes = tiles(&tree, id);
    let before = contents(&tree);

    keyed.select(4);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id), nodes, "every tile keeps its node");
    let after = contents(&tree);
    assert_ne!(after[4], before[4], "tile 4 is drawn again");
    assert_eq!(after[..4], before[..4]);
    assert_eq!(after[5..], before[5..], "and no other tile is");
    assert_eq!(selected_tiles(&tree), vec![4]);
}

// ── Detail band ─────────────────────────────────────────────────────────────

/// A band's content: 80 dp tall at any width.
fn band_content() -> Option<Box<dyn Widget>> {
    Some(Box::new(FixedLeaf(10.0, 80.0)))
}

/// A band's content whose height follows its width, as wrapped text does:
/// 8000 / width, so 20 dp at 400 and 32 dp at 250.
#[derive(Debug)]
struct WrapLeaf;
impl Widget for WrapLeaf {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(100.0).max(1.0);
        Size::new(width, 8000.0 / width).into()
    }
}

/// `count` tiles of 100x50 with a detail band built by `band`, configured
/// further by `configure`, laid out at `width` x `height`. At 400 dp the
/// grid has three columns and rows step 58 dp, so row `r` spans
/// `58r .. 58r + 50`.
fn band_grid(
    count: usize,
    width: f32,
    height: f32,
    configure: impl FnOnce(GridView<usize>) -> GridView<usize>,
) -> (
    WidgetTree,
    WidgetId,
    ListModel<usize>,
    Signal<Option<usize>>,
) {
    let model = ListModel::from_vec((0..count).collect());
    let expanded = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let grid = GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0)
        .detail_row(|_tc| band_content())
        .expanded_index(expanded.clone());
    let id = tree.add(configure(grid));
    settle(&mut tree, width, height);
    (tree, id, model, expanded)
}

/// Two passes: a pass that changes the column count or the band rebuilds
/// the tiles once more.
fn settle(tree: &mut WidgetTree, width: f32, height: f32) {
    tree.layout(SizeProposal::exact(width, height));
    tree.layout(SizeProposal::exact(width, height));
}

fn band_of(tree: &WidgetTree, grid: WidgetId) -> WidgetId {
    tree.widget_as_any(grid)
        .and_then(|any| any.downcast_ref::<GridView<usize>>())
        .and_then(|g| g.band_id)
        .expect("the grid has a band")
}

fn tile_y(tree: &WidgetTree, grid: WidgetId, index: usize) -> f32 {
    tree.bounds(tile_at(tree, grid, index)).y
}

/// Two clicks on the tile at `index`, after a pause long enough that they
/// do not continue an earlier click.
fn double_click(tree: &mut WidgetTree, grid: WidgetId, index: usize) {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    tree.advance_time(std::time::Duration::from_secs(1));
    let at = tree.bounds(tile_at(tree, grid, index)).center();
    for _ in 0..2 {
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }
}

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 0.5,
        "{what}: expected {expected}, got {actual}"
    );
}

/// Activating a tile opens a full-width band under the row that holds it,
/// as tall as its content, and the rows after it move down to make room.
/// Activating it again closes it. The application's handler fires both
/// times.
#[test]
fn activating_a_tile_opens_a_band_under_its_row() {
    use std::cell::Cell;
    use std::rc::Rc;
    let activated = Rc::new(Cell::new(0));
    let count = activated.clone();
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| {
        g.on_tile_activate(move |_i, _ctx| count.set(count.get() + 1))
    });
    assert_close(tile_y(&tree, id, 6), 116.0, "row 2 before");

    double_click(&mut tree, id, 4);
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), Some(4));
    assert_eq!(activated.get(), 1, "on_tile_activate still fires");
    let band = tree.bounds(band_of(&tree, id));
    assert_close(band.x, 0.0, "band x");
    assert_close(band.width, 400.0, "band width");
    assert_close(
        band.y,
        116.0,
        "the band starts a row gap under row 1 (58 + 50 + 8)",
    );
    assert_close(band.height, 80.0, "the band is as tall as its content");
    assert_close(tile_y(&tree, id, 3), 58.0, "the open row stays");
    assert_close(tile_y(&tree, id, 5), 58.0, "all of it");
    assert_close(
        tile_y(&tree, id, 6),
        204.0,
        "row 2 moves down by the band and a row gap (116 + 80 + 8)",
    );

    double_click(&mut tree, id, 4);
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), None);
    assert_eq!(activated.get(), 2);
    assert_close(tile_y(&tree, id, 6), 116.0, "row 2 back in place");
    assert_close(tree.bounds(band_of(&tree, id)).height, 0.0, "no band");
}

/// The band follows its tile's row when a resize changes the column count,
/// and is measured again at the new width.
#[test]
fn the_band_stays_under_its_tile_when_the_columns_change() {
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| {
        g.detail_row(|_tc| Some(Box::new(WrapLeaf) as Box<dyn Widget>))
    });
    expanded.set(Some(4));
    settle(&mut tree, 400.0, 500.0);
    let band = tree.bounds(band_of(&tree, id));
    assert_close(band.y, 116.0, "three columns: tile 4 is on row 1");
    assert_close(band.height, 20.0, "8000 / 400");

    // Two columns fit 250 dp: tile 4 moves to row 2.
    settle(&mut tree, 250.0, 500.0);
    let band = tree.bounds(band_of(&tree, id));
    assert_close(band.y, 174.0, "under row 2 (116 + 50 + 8)");
    assert_close(band.height, 32.0, "8000 / 250");
    assert_close(tile_y(&tree, id, 5), 116.0, "tile 5 shares the open row");
    assert_close(
        tile_y(&tree, id, 6),
        214.0,
        "row 3 under the band (174 + 32 + 8)",
    );
}

/// The band counts in the scroll range and in the window of realized tiles:
/// the tiles it pushes down are realized where they now are.
#[test]
fn the_band_counts_in_the_scroll_range_and_the_realized_window() {
    let (mut tree, id, _model, expanded) =
        band_grid(300, 400.0, 300.0, |g| g.detail_row_height(|_i| 1000.0));
    let (scroll, max_scroll) = {
        let grid = tree
            .widget_as_any(id)
            .and_then(|any| any.downcast_ref::<GridView<usize>>())
            .expect("the grid");
        (grid.scroll_y.clone(), grid.max_scroll_y.clone())
    };
    let before = max_scroll.get();

    expanded.set(Some(0));
    settle(&mut tree, 400.0, 300.0);
    assert_close(
        max_scroll.get(),
        before + 1008.0,
        "the band and its row gap lengthen the content",
    );
    assert_close(
        tree.bounds(band_of(&tree, id)).height,
        1000.0,
        "detail_row_height sizes the band",
    );

    // Row 1 now starts at 50 + 8 + 1000 + 8 = 1066. Scrolled to put it
    // 100 dp down the viewport, it has to be realized, and there.
    scroll.set(966.0);
    settle(&mut tree, 400.0, 300.0);
    assert_close(
        tile_y(&tree, id, 3),
        100.0,
        "row 1, realized under the band",
    );
    assert_close(
        tile_y(&tree, id, 9),
        216.0,
        "row 3, a row step further down",
    );
}

/// The band stays on its tile through an insert before it, and closes when
/// the tile is removed, or when a model without keys resets.
#[test]
fn the_band_follows_its_tile_and_closes_when_it_is_removed() {
    let (mut tree, id, model, expanded) = band_grid(12, 400.0, 500.0, |g| g);
    expanded.set(Some(4));
    settle(&mut tree, 400.0, 500.0);

    model.insert(0, 99);
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), Some(5), "the tile is now fifth");
    assert_close(tree.bounds(band_of(&tree, id)).y, 116.0, "still on row 1");

    model.remove(5);
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), None, "its tile is gone");

    expanded.set(Some(2));
    settle(&mut tree, 400.0, 500.0);
    model.replace_all((0..12).collect());
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(
        expanded.get(),
        None,
        "a reset of a model with no keys says nothing of where the tile went"
    );
}

/// A source with keys keeps the band on its tile through a sort, and closes
/// it when a filter hides the tile.
#[test]
fn a_sort_keeps_the_band_on_its_tile_when_the_source_has_keys() {
    use teksilo_data::SortDirection;
    let proxy = teksilo_data::SortFilterListModel::new(ListModel::from_vec((0..9).collect()))
        .with_comparator("n", |a: &usize, b: &usize| a.cmp(b))
        .with_predicate("even", |_text| Box::new(|n: &usize| n.is_multiple_of(2)));
    let expanded = Signal::new(Some(1));
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::from_source(proxy.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .detail_row(|_tc| band_content())
            .expanded_index(expanded.clone()),
    );
    settle(&mut tree, 400.0, 500.0);
    assert_close(tree.bounds(band_of(&tree, id)).y, 58.0, "under row 0");

    proxy.set_sort(Some("n"), SortDirection::Descending);
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), Some(7), "8, 7, … 1, 0: tile 1 is at 7");
    assert_close(tree.bounds(band_of(&tree, id)).y, 174.0, "under row 2");

    proxy.set_filter("even", "on");
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), None, "tile 1 is filtered out");
}

/// Keys: Enter on a tile opens its band and Enter again closes it; Space on
/// the open tile closes it too, and leaves the selection as it was.
#[test]
fn enter_and_space_on_the_open_tile_close_its_band() {
    use teksilo_core::event::{Key, Modifiers};
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 300.0, |g| g.selection(sel));
    tree.focus(id);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(grid_focus(&tree, id), Some(4));
    assert_eq!(selection.selected_indices(), vec![4]);

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), Some(4));
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), None);

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), Some(4));
    press(&mut tree, Key::Space, Modifiers::NONE);
    assert_eq!(expanded.get(), None);
    assert_eq!(
        selection.selected_indices(),
        vec![4],
        "Space closed the band rather than toggling the selection"
    );
}

/// ↓ from the open tile moves focus onto the first control in its band, and
/// ↑ there, which the control does not use, returns to the tile. A key the
/// band lets through moves no tile. Between tiles the arrows step over the
/// band.
#[test]
fn down_from_the_open_tile_enters_its_band_and_up_returns() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 300.0, |g| {
        g.detail_row(|_tc| {
            Some(Box::new(crate::Button::new(teksilo_i18n::lit!("Play"))) as Box<dyn Widget>)
        })
    });
    tree.focus(id);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), Some(4));
    let band = band_of(&tree, id);

    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    let focused = tree.focused().expect("something has focus");
    assert!(
        focused != band && tree.is_descendant_of(focused, band),
        "focus is on the band's control"
    );
    assert_eq!(
        grid_focus(&tree, id),
        Some(4),
        "the cursor waits on the tile"
    );

    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        grid_focus(&tree, id),
        Some(4),
        "a key the band does not use moves no tile"
    );
    assert_eq!(tree.focused(), Some(focused));

    press(&mut tree, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(id), "back on the grid");
    assert_eq!(grid_focus(&tree, id), Some(4), "on the open tile");

    press(&mut tree, Key::ArrowLeft, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(grid_focus(&tree, id), Some(6), "from tile 3, over the band");
    assert_eq!(tree.focused(), Some(id));
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(grid_focus(&tree, id), Some(4), "from tile 7, over the band");
}

/// A band with nothing to take focus is stepped over by ↓ like any other.
#[test]
fn down_from_an_open_tile_with_nothing_to_focus_moves_to_the_next_row() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 300.0, |g| g);
    tree.focus(id);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), Some(4));
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(grid_focus(&tree, id), Some(7));
    assert_eq!(tree.focused(), Some(id));
}

/// To a screen reader the band is a group named after its tile; the tile is
/// a disclosure, expanded or collapsed, that controls the band while open
/// and offers the action that changes it. Opening it replaces no tile node
/// and changes no tile's place in the grid.
#[test]
fn the_band_is_a_group_named_after_its_tile_which_controls_it() {
    use teksilo_core::accesskit::{Action, Role};
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| {
        g.tile_a11y_label(|i| format!("Album {i}"))
    });
    let node = |tree: &WidgetTree, widget: WidgetId| {
        let wanted = teksilo_core::accessibility::widget_id_to_node_id(widget);
        tree.accessibility_tree_snapshot()
            .nodes
            .into_iter()
            .find(|(node_id, _)| *node_id == wanted)
            .map(|(_, node)| node)
            .expect("the widget has a node")
    };
    let nodes = tiles(&tree, id);
    let grid_before = node(&tree, id);
    let tile6_before = node(&tree, tile_at(&tree, id, 6));
    assert_eq!(
        node(&tree, tile_at(&tree, id, 4)).is_expanded(),
        Some(false)
    );

    expanded.set(Some(4));
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(tiles(&tree, id), nodes, "no tile node is replaced");
    let band = band_of(&tree, id);
    let band_node = node(&tree, band);
    assert_eq!(band_node.role(), Role::Group);
    assert_eq!(band_node.label(), Some("Album 4"));

    let open = node(&tree, tile_at(&tree, id, 4));
    assert_eq!(open.is_expanded(), Some(true));
    assert_eq!(
        open.controls(),
        &[teksilo_core::accessibility::widget_id_to_node_id(band)]
    );
    assert!(open.supports_action(Action::Collapse));
    let closed = node(&tree, tile_at(&tree, id, 7));
    assert_eq!(closed.is_expanded(), Some(false));
    assert!(closed.controls().is_empty());
    assert!(closed.supports_action(Action::Expand));

    let grid_after = node(&tree, id);
    assert_eq!(grid_after.row_count(), grid_before.row_count());
    assert_eq!(grid_after.column_count(), grid_before.column_count());
    let tile6_after = node(&tree, tile_at(&tree, id, 6));
    assert_eq!(tile6_after.row_index(), tile6_before.row_index());
    assert_eq!(
        tile6_after.position_in_set(),
        tile6_before.position_in_set()
    );

    // The actions open and close it, for a reader with no double click.
    let act = |tree: &mut WidgetTree, action: Action, index: usize| {
        let target = tile_at(tree, id, index);
        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action,
            target: Some(target),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        settle(tree, 400.0, 500.0);
    };
    act(&mut tree, Action::Expand, 7);
    assert_eq!(expanded.get(), Some(7));
    act(&mut tree, Action::Collapse, 7);
    assert_eq!(expanded.get(), None);
}

/// The rubber band selects tiles by where they are, band included: a sweep
/// over the band alone selects nothing, and one across it selects the rows on
/// either side.
#[test]
fn the_rubber_band_selects_no_band() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let (mut tree, _id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| g.selection(sel));
    expanded.set(Some(1));
    settle(&mut tree, 400.0, 500.0);
    // Row 0 is 0..50, the band 58..138, row 1 now 146..196.
    let sweep = |tree: &mut WidgetTree, from: Point, to: Point| {
        tree.dispatch_event(WidgetEvent::pointer_down(
            from,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(from.x, from.y + 6.0)));
        tree.dispatch_event(WidgetEvent::pointer_move(to));
        settle(tree, 400.0, 500.0);
        tree.dispatch_event(WidgetEvent::pointer_up(
            to,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        settle(tree, 400.0, 500.0);
    };

    sweep(&mut tree, Point::new(360.0, 70.0), Point::new(40.0, 130.0));
    assert_eq!(
        selection.selected_indices(),
        Vec::<usize>::new(),
        "where row 1 was, there is only the band"
    );

    // Pressed on the band itself, a drag down over row 1 starts no marquee:
    // the press belongs to the band, not to the grid's background.
    sweep(&mut tree, Point::new(50.0, 100.0), Point::new(250.0, 170.0));
    assert_eq!(selection.selected_indices(), Vec::<usize>::new());

    sweep(&mut tree, Point::new(360.0, 10.0), Point::new(40.0, 160.0));
    assert_eq!(selection.selected_indices(), vec![0, 1, 2, 3, 4, 5]);
}

/// With variable row heights the band opens under the open row's tallest
/// tile.
#[test]
fn the_band_opens_under_a_variable_height_row() {
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| {
        g.item_height(|i| if i == 4 { 90.0 } else { 50.0 })
    });
    expanded.set(Some(3));
    settle(&mut tree, 400.0, 500.0);
    let band = tree.bounds(band_of(&tree, id));
    assert_close(band.y, 156.0, "row 1 is 58..148, tile 4 makes it 90 tall");
    assert_close(
        tile_y(&tree, id, 6),
        244.0,
        "row 2 under the band (156 + 80 + 8)",
    );
}

/// In a sectioned grid the band opens inside its section, and the next
/// section's header moves down with the rows.
#[test]
fn the_band_opens_inside_its_section_and_moves_the_next_header_down() {
    let model = ListModel::from_vec((0..6).collect::<Vec<usize>>());
    let expanded = Signal::new(Some(0));
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 50.0)))
            .column_count(2, 50.0)
            .section_header_height(28.0)
            .sections(TwoSections)
            .detail_row(|_tc| band_content())
            .expanded_index(expanded),
    );
    settle(&mut tree, 300.0, 600.0);
    // Section 0: header 0..28, rows 28..78 and 86..136; section 1's header
    // was at 144 and its first row at 172 (`sections_offset_tiles_below_headers`).
    assert_close(
        tree.bounds(band_of(&tree, id)).y,
        86.0,
        "under row 0 of section 0",
    );
    assert_close(tile_y(&tree, id, 2), 174.0, "section 0's row 1 (86 + 88)");
    assert_close(tile_y(&tree, id, 3), 260.0, "section 1's row 0 (172 + 88)");
    let header1 = tiles(&tree, id)[7];
    assert_close(
        tree.bounds(header1).y,
        232.0,
        "section 1's header (144 + 88)",
    );
}

/// A waterfall has no rows to open a band under: the band is never built,
/// and activation leaves the disclosure alone.
#[test]
fn a_waterfall_has_no_band() {
    let heights = [60.0_f32, 100.0, 40.0, 80.0, 50.0];
    let model = ListModel::from_vec((0..heights.len()).collect());
    let expanded = Signal::new(None);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(80.0, 50.0)))
            .column_count(2, 60.0)
            .waterfall(60.0)
            .item_height(move |i| heights[i])
            .detail_row(|_tc| band_content())
            .expanded_index(expanded.clone()),
    );
    settle(&mut tree, 200.0, 400.0);
    double_click(&mut tree, id, 0);
    settle(&mut tree, 200.0, 400.0);
    assert_eq!(expanded.get(), None);
    let band = tree
        .widget_as_any(id)
        .and_then(|any| any.downcast_ref::<GridView<usize>>())
        .and_then(|g| g.band_id);
    assert_eq!(band, None);
    assert_close(
        tile_y(&tree, id, 2),
        68.0,
        "item 2 still stacks under item 0",
    );
}

/// A `ListModel`'s keys are its positions, and a reset keeps a position for
/// whatever item lands there, so `replace_all` closes the band through
/// `from_source` as it does through `new`, rather than leaving it open over
/// another item.
#[test]
fn a_list_model_reset_closes_the_band_through_from_source_too() {
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let expanded = Signal::new(Some(4));
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::from_source(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .detail_row(|_tc| band_content())
            .expanded_index(expanded.clone()),
    );
    settle(&mut tree, 400.0, 500.0);

    model.insert(0, 99);
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), Some(5), "an insert is still followed");

    model.replace_all((100..112).collect());
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(
        expanded.get(),
        None,
        "tile 5 is another item now; its position is not its identity"
    );
}

/// The same through `from_source_keyed`: the band closes on the reset. The
/// keyed selection keeps its keys, which for a `ListModel` are positions, so
/// it stays on the same positions, as `ListView::from_source_keyed`'s does.
#[test]
fn a_list_model_reset_closes_the_band_under_a_keyed_selection() {
    let model = ListModel::from_vec((0..12).collect::<Vec<usize>>());
    let keyed = teksilo_data::KeyedSelectionModel::<usize>::new(SelectionMode::Multi);
    let expanded = Signal::new(Some(4));
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::from_source_keyed(model.clone(), keyed.clone(), |_tc| {
            Box::new(FixedLeaf(100.0, 50.0))
        })
        .tile_size(100.0, 50.0)
        .detail_row(|_tc| band_content())
        .expanded_index(expanded.clone()),
    );
    settle(&mut tree, 400.0, 500.0);
    keyed.select_keys([2, 4], false);

    model.replace_all((100..112).collect());
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(expanded.get(), None);
    assert_eq!(sorted_keys(&keyed), vec![2, 4]);
}

// ── Detail band: reveal, focus, repaint, identity, order, lifetime ─────────

/// The grid at `id`, for its private state.
fn the_grid(tree: &WidgetTree, id: WidgetId) -> &GridView<usize> {
    tree.widget_as_any(id)
        .and_then(|any| any.downcast_ref::<GridView<usize>>())
        .expect("the grid is the widget at `id`")
}

/// A band holding one button, which can take focus.
fn band_with_a_button(_tc: &TileContext<'_, usize>) -> Option<Box<dyn Widget>> {
    Some(Box::new(crate::Button::new(teksilo_i18n::lit!("Play"))) as Box<dyn Widget>)
}

/// The accessibility node of `widget`.
fn a11y_node(tree: &WidgetTree, widget: WidgetId) -> teksilo_core::accesskit::Node {
    let wanted = teksilo_core::accessibility::widget_id_to_node_id(widget);
    tree.accessibility_tree_snapshot()
        .nodes
        .into_iter()
        .find(|(node_id, _)| *node_id == wanted)
        .map(|(_, node)| node)
        .expect("the widget has a node")
}

/// Whether `rect` lies inside the vertical span `top..bottom`.
fn within(rect: Rect, top: f32, bottom: f32) -> bool {
    rect.y >= top - 0.5 && rect.y + rect.height <= bottom + 0.5
}

/// ↓ from the open tile onto a band below the viewport brings the control it
/// focuses into view, and so does Tab: the framework's reveal walk asks the
/// grid, which used to ignore it.
#[test]
fn focus_entering_a_band_below_the_viewport_scrolls_it_into_view() {
    use teksilo_core::event::{Key, Modifiers};
    for key in [Key::ArrowDown, Key::Tab] {
        let (mut tree, id, _model, expanded) =
            band_grid(30, 400.0, 300.0, |g| g.detail_row(band_with_a_button));
        // Row 4 is 232..282, so its band starts at 290, under the viewport.
        expanded.set(Some(13));
        settle(&mut tree, 400.0, 300.0);
        tree.focus(id);
        the_grid(&tree, id).focused_index.set(Some(13));
        let band = band_of(&tree, id);
        assert!(
            tree.bounds(band).y > 280.0,
            "the band starts under the viewport, which is the case this is about"
        );

        press(&mut tree, key, Modifiers::NONE);
        settle(&mut tree, 400.0, 300.0);
        let focused = tree.focused().expect("something has focus");
        assert!(
            tree.is_descendant_of(focused, band),
            "{key:?} put focus on the band's control"
        );
        assert!(
            within(tree.bounds(focused), 0.0, 300.0),
            "{key:?}: the focused control is on screen, at {:?}",
            tree.bounds(focused)
        );
    }
}

/// Opening a band under the last visible row scrolls it into view, by Enter
/// as by a double click, and keeps its tile on screen.
#[test]
fn opening_a_band_under_the_last_visible_row_scrolls_it_into_view() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, _model, expanded) = band_grid(30, 400.0, 300.0, |g| g);
    tree.focus(id);
    the_grid(&tree, id).focused_index.set(Some(13));
    press(&mut tree, Key::Enter, Modifiers::NONE);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(expanded.get(), Some(13));
    assert!(
        within(tree.bounds(band_of(&tree, id)), 0.0, 300.0),
        "Enter: the band is on screen, at {:?}",
        tree.bounds(band_of(&tree, id))
    );
    assert!(tile_y(&tree, id, 13) >= 0.0, "and so is its tile");

    press(&mut tree, Key::Enter, Modifiers::NONE);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(expanded.get(), None);
    the_grid(&tree, id).scroll_y.set(0.0);
    settle(&mut tree, 400.0, 300.0);

    double_click(&mut tree, id, 13);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(expanded.get(), Some(13));
    assert!(
        within(tree.bounds(band_of(&tree, id)), 0.0, 300.0),
        "double click: the band is on screen, at {:?}",
        tree.bounds(band_of(&tree, id))
    );
}

/// Closing the band while focus is inside it hands focus back to the grid,
/// with the cursor on the tile, whatever closes it: the application (a close
/// button in the band, a shortcut), or `Collapse` from a screen reader. The
/// grid's keys work again afterwards.
#[test]
fn closing_the_band_with_focus_inside_it_returns_focus_to_its_tile() {
    use teksilo_core::accesskit::Action;
    use teksilo_core::event::{Key, Modifiers};
    for by_reader in [false, true] {
        let (mut tree, id, _model, expanded) =
            band_grid(12, 400.0, 300.0, |g| g.detail_row(band_with_a_button));
        tree.focus(id);
        the_grid(&tree, id).focused_index.set(Some(4));
        press(&mut tree, Key::Enter, Modifiers::NONE);
        press(&mut tree, Key::ArrowDown, Modifiers::NONE);
        let band = band_of(&tree, id);
        assert!(
            tree.focused()
                .is_some_and(|f| tree.is_descendant_of(f, band)),
            "focus starts on the band's control"
        );

        if by_reader {
            let tile = tile_at(&tree, id, 4);
            tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
                action: Action::Collapse,
                target: Some(tile),
                target_node: teksilo_core::accessibility::root_node_id(),
                data: None,
            });
        } else {
            expanded.set(None);
        }
        settle(&mut tree, 400.0, 300.0);
        assert_eq!(expanded.get(), None);
        assert_eq!(tree.focused(), Some(id), "focus is back on the grid");
        assert_eq!(grid_focus(&tree, id), Some(4), "on the tile");

        press(&mut tree, Key::ArrowRight, Modifiers::NONE);
        assert_eq!(grid_focus(&tree, id), Some(5), "and the grid's keys work");
    }
}

/// The top edge of the keyboard focus ring in the frame `tree` renders: the
/// highest of the thin horizontal bars the overlay paints, the only
/// decorations in a band grid of unpainted tiles.
fn ring_top(tree: &mut WidgetTree) -> Option<f32> {
    let frame = tree.render();
    frame
        .decorations
        .iter()
        .filter(|d| d.rect[3] < 4.0 && d.rect[2] > 50.0)
        .map(|d| d.rect[1])
        .reduce(f32::min)
}

/// The focus ring is painted where its tile is when a band opens or closes
/// above it, though neither the cursor nor the scroll offset changed.
#[test]
fn the_focus_ring_follows_its_tile_when_a_band_opens_above_it() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| g);
    tree.focus(id);
    for key in [
        Key::ArrowRight,
        Key::ArrowDown,
        Key::ArrowDown,
        Key::ArrowRight,
    ] {
        press(&mut tree, key, Modifiers::NONE);
    }
    settle(&mut tree, 400.0, 500.0);
    assert_eq!(grid_focus(&tree, id), Some(7));
    let before = ring_top(&mut tree).expect("the ring is painted");
    assert!((before - 116.0).abs() < 4.0, "on tile 7's row, at {before}");

    // Opened from outside: the band under row 0 moves row 2 down 88 dp.
    expanded.set(Some(1));
    settle(&mut tree, 400.0, 500.0);
    let after = ring_top(&mut tree).expect("the ring is painted");
    assert_close(after - before, 88.0, "the ring moved with its tile");

    expanded.set(None);
    settle(&mut tree, 400.0, 500.0);
    let closed = ring_top(&mut tree).expect("the ring is painted");
    assert_close(closed, before, "and back when it closes");
}

/// Space on a tile with nothing to disclose acts on the selection: Enter on
/// it opened nothing, so there is no band for Space to close.
#[test]
fn space_selects_a_tile_with_nothing_to_disclose() {
    use teksilo_core::event::{Key, Modifiers};
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 300.0, |g| {
        g.selection(sel)
            .detail_row(|tc| if tc.index == 4 { None } else { band_content() })
    });
    tree.focus(id);
    for key in [Key::ArrowRight, Key::ArrowDown, Key::ArrowRight] {
        press(&mut tree, key, Modifiers::NONE);
    }
    assert_eq!(selection.selected_indices(), vec![4]);

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), None, "tile 4 has nothing to disclose");
    press(&mut tree, Key::Space, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        Vec::<usize>::new(),
        "Space toggled the tile"
    );

    // Nor does a screen reader's Expand open anything there.
    let tile = tile_at(&tree, id, 4);
    tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::Expand,
        target: Some(tile),
        target_node: teksilo_core::accessibility::root_node_id(),
        data: None,
    });
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(expanded.get(), None);
    assert_eq!(a11y_node(&tree, tile).is_expanded(), Some(false));
}

/// Enter on a tile that opens a band selects it too: opening a tile is acting
/// on it, as a double click is. Closing it leaves the selection alone.
#[test]
fn enter_selects_the_tile_it_opens() {
    use teksilo_core::event::{Key, Modifiers};
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 300.0, |g| g.selection(sel));
    tree.focus(id);
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    assert_eq!(selection.selected_indices(), vec![0]);
    assert_eq!(grid_focus(&tree, id), Some(1));

    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), Some(1));
    assert_eq!(selection.selected_indices(), vec![1], "the tile it opened");

    selection.select_indices(vec![0, 1], false);
    press(&mut tree, Key::Enter, Modifiers::NONE);
    assert_eq!(expanded.get(), None);
    assert_eq!(
        selection.selected_indices(),
        vec![0, 1],
        "closing selects nothing"
    );
}

/// Opening, moving and closing the band replace no tile node, the tiles
/// realized by a scroll made while it was open included: they keep the ids
/// the grid names as its active descendant.
#[test]
fn opening_moving_and_closing_the_band_replace_no_tile_node() {
    let (mut tree, id, _model, expanded) = band_grid(300, 400.0, 300.0, |g| g);
    let nodes = tiles(&tree, id);
    expanded.set(Some(4));
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(tiles(&tree, id), nodes, "open: no tile node is replaced");

    // A row's scroll rebuilds the realized tiles, with the band open.
    the_grid(&tree, id).scroll_y.set(58.0);
    settle(&mut tree, 400.0, 300.0);
    let nodes = tiles(&tree, id);
    for (step, open) in [("move", Some(7)), ("close", None)] {
        expanded.set(open);
        settle(&mut tree, 400.0, 300.0);
        assert_eq!(tiles(&tree, id), nodes, "{step}: no tile node is replaced");
    }
}

/// A band opened or closed above the viewport leaves the visible tiles where
/// they were on screen.
#[test]
fn a_band_changing_above_the_viewport_leaves_the_visible_tiles_still() {
    let (mut tree, id, _model, expanded) = band_grid(300, 400.0, 300.0, |g| g);
    let scroll = the_grid(&tree, id).scroll_y.clone();
    scroll.set(580.0);
    settle(&mut tree, 400.0, 300.0);
    assert_close(tile_y(&tree, id, 30), 0.0, "row 10 at the top");

    expanded.set(Some(1));
    settle(&mut tree, 400.0, 300.0);
    assert_close(tile_y(&tree, id, 30), 0.0, "opened above: row 10 stays");
    assert_close(scroll.get(), 668.0, "the offset took the band and its gap");

    expanded.set(Some(4));
    settle(&mut tree, 400.0, 300.0);
    assert_close(tile_y(&tree, id, 30), 0.0, "moved above: row 10 stays");

    expanded.set(None);
    settle(&mut tree, 400.0, 300.0);
    assert_close(tile_y(&tree, id, 30), 0.0, "closed above: row 10 stays");
    assert_close(scroll.get(), 580.0, "the offset is back");
}

/// A band taller than the realization buffer, moved while above the
/// viewport: the tiles on screen stay the nodes they were, all realized in
/// the first frame after the move.
#[test]
fn a_tall_band_moved_above_the_viewport_replaces_no_tile() {
    let (mut tree, id, _model, expanded) =
        band_grid(300, 400.0, 300.0, |g| g.detail_row_height(|_i| 1000.0));
    expanded.set(Some(1));
    settle(&mut tree, 400.0, 300.0);
    the_grid(&tree, id).scroll_y.set(2000.0);
    settle(&mut tree, 400.0, 300.0);
    // Without the band's 1008 dp, the viewport is 992..1292: rows 17 to 22.
    assert_close(tile_y(&tree, id, 51), 986.0 + 1008.0 - 2000.0, "row 17");
    let nodes = tiles(&tree, id);

    expanded.set(Some(4));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id), nodes, "no tile node is replaced");
    let realized: Vec<usize> = the_grid(&tree, id)
        .tile_map
        .borrow()
        .iter()
        .map(|(i, _)| *i)
        .collect();
    let missing: Vec<usize> = (51..=68).filter(|i| !realized.contains(i)).collect();
    assert_eq!(
        missing,
        Vec::<usize>::new(),
        "every tile on screen is realized"
    );
    assert_close(tile_y(&tree, id, 51), -6.0, "and still where it was");
}

/// A sort that leaves the band's tile where it was realizes every tile on
/// screen in its first frame. The reset empties the layout's row table, and
/// the band asks it for its row before the pane is built; the pane is built
/// from a synced table all the same, because the reset's own scroll write
/// re-syncs it first. Exact row heights well above the 10 dp estimate are
/// what would show a stale table.
#[test]
fn a_band_kept_through_a_sort_realizes_the_whole_viewport_at_once() {
    use teksilo_data::SortDirection;
    let proxy = teksilo_data::SortFilterListModel::new(ListModel::from_vec((0..43).collect()))
        .with_comparator("n", |a: &usize, b: &usize| a.cmp(b));
    let expanded = Signal::new(Some(21));
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::from_source(proxy.clone(), |_tc| Box::new(FixedLeaf(100.0, 90.0)))
            .tile_size(100.0, 10.0)
            .item_height(|_i| 90.0)
            .detail_row(|_tc| band_content())
            .detail_row_height(|_i| 1000.0)
            .expanded_index(expanded.clone()),
    );
    settle(&mut tree, 400.0, 900.0);

    // 42, 41, … 0: item 21 stays at 21, on row 7 (686..776), on screen.
    proxy.set_sort(Some("n"), SortDirection::Descending);
    tree.layout(SizeProposal::exact(400.0, 900.0));
    assert_eq!(expanded.get(), Some(21));
    let realized: Vec<usize> = the_grid(&tree, id)
        .tile_map
        .borrow()
        .iter()
        .map(|(i, _)| *i)
        .collect();
    let missing: Vec<usize> = (0..24).filter(|i| !realized.contains(i)).collect();
    assert_eq!(missing, Vec::<usize>::new(), "rows 0 to 7 are on screen");
}

/// A screen reader meets the band right after its own row, not after every
/// tile of the grid: it is read where it is seen.
#[test]
fn the_band_is_read_after_its_own_row() {
    let (mut tree, id, _model, expanded) = band_grid(12, 400.0, 500.0, |g| g);
    expanded.set(Some(4));
    settle(&mut tree, 400.0, 500.0);
    let band = teksilo_core::accessibility::widget_id_to_node_id(band_of(&tree, id));
    let pane = a11y_node(&tree, body_pane(&tree, id));
    let order = pane.children().to_vec();
    let at = |index: usize| {
        let tile = teksilo_core::accessibility::widget_id_to_node_id(tile_at(&tree, id, index));
        order
            .iter()
            .position(|n| *n == tile)
            .expect("the tile is read")
    };
    let band_at = order
        .iter()
        .position(|n| *n == band)
        .expect("the band is read among the tiles");
    assert_eq!(band_at, at(5) + 1, "after row 1's last tile");
    assert_eq!(band_at + 1, at(6), "before row 2's first");
    assert!(
        !a11y_node(&tree, id).children().contains(&band),
        "and only there"
    );
}

/// A band whose tile has scrolled out of the realized window, with no
/// `tile_a11y_label` to name it, still has a name.
#[test]
fn a_band_whose_tile_is_not_realized_is_still_named() {
    teksilo_i18n::thread_local::clear();
    let (mut tree, id, _model, expanded) = band_grid(300, 400.0, 300.0, |g| g);
    expanded.set(Some(1));
    settle(&mut tree, 400.0, 300.0);
    let band = band_of(&tree, id);
    assert!(
        !a11y_node(&tree, band).labelled_by().is_empty(),
        "labelled by its tile while the tile is realized"
    );

    let max = the_grid(&tree, id).max_scroll_y.get();
    the_grid(&tree, id).scroll_y.set(max);
    settle(&mut tree, 400.0, 300.0);
    assert!(
        !the_grid(&tree, id)
            .tile_map
            .borrow()
            .iter()
            .any(|(i, _)| *i == 1),
        "tile 1 is far above the window, which is the case this is about"
    );
    assert_eq!(a11y_node(&tree, band).label(), Some("Details of item 2"));
}

/// Items keyed `1000 + index`, of which only those a window load delivered
/// are resident. The grid asks for windows; the test delivers them.
struct Lazy {
    loaded: Rc<RefCell<Vec<bool>>>,
    asked: Rc<RefCell<std::ops::Range<usize>>>,
    changes: Signal<Option<DataChange>>,
}

impl Lazy {
    fn new(len: usize) -> Self {
        Self {
            loaded: Rc::new(RefCell::new(vec![false; len])),
            asked: Rc::new(RefCell::new(0..0)),
            changes: Signal::new(None),
        }
    }

    fn handle(&self) -> Self {
        Self {
            loaded: self.loaded.clone(),
            asked: self.asked.clone(),
            changes: self.changes.clone(),
        }
    }

    /// Deliver `range`, and say so.
    fn load(&self, range: std::ops::Range<usize>) {
        for loaded in &mut self.loaded.borrow_mut()[range.clone()] {
            *loaded = true;
        }
        self.changes.set(Some(DataChange::WindowLoaded { range }));
    }
}

impl teksilo_data::ListDataSource for Lazy {
    type Item = usize;
    type Key = u64;
    fn len(&self) -> usize {
        self.loaded.borrow().len()
    }
    fn with_item<R>(&self, i: usize, f: impl FnOnce(&usize) -> R) -> Option<R> {
        let loaded = self.loaded.borrow().get(i).copied().unwrap_or(false);
        loaded.then(|| f(&i))
    }
    fn key_at(&self, i: usize) -> Option<u64> {
        (i < self.len()).then_some(1000 + i as u64)
    }
    fn row_state(&self, i: usize) -> teksilo_data::RowState {
        if self.loaded.borrow().get(i).copied().unwrap_or(false) {
            teksilo_data::RowState::Ready
        } else {
            teksilo_data::RowState::Loading
        }
    }
    fn request_window(&self, range: std::ops::Range<usize>) {
        *self.asked.borrow_mut() = range;
    }
    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> teksilo_core::ObserverHandle {
        self.changes.observe(move |change| {
            if let Some(change) = change {
                f(change);
            }
        })
    }
}

/// A lazy source loading a window keeps the band as it was, its content and
/// the focus inside it included, and the keyed selection on its tiles. A band
/// opened on a tile that has not loaded holds a placeholder, and is built
/// again when that tile's window arrives.
#[test]
fn a_window_load_keeps_the_band_and_the_keyed_selection() {
    use teksilo_core::event::{Key, Modifiers};
    let source = Lazy::new(90);
    let keyed = teksilo_data::KeyedSelectionModel::<u64>::new(SelectionMode::Multi);
    let expanded = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        GridView::from_source_keyed(source.handle(), keyed.clone(), |_tc| {
            Box::new(FixedLeaf(100.0, 50.0)) as Box<dyn Widget>
        })
        .tile_size(100.0, 50.0)
        .detail_row(band_with_a_button)
        .expanded_index(expanded.clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let asked = source.asked.borrow().clone();
    source.load(asked);
    settle(&mut tree, 400.0, 300.0);
    keyed.select_keys([1001, 1004], false);
    expanded.set(Some(2));
    settle(&mut tree, 400.0, 300.0);

    tree.focus(id);
    the_grid(&tree, id).focused_index.set(Some(2));
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    let band = band_of(&tree, id);
    let content = tree.children(band);
    let focused = tree.focused().expect("focus is on the band's control");
    assert!(tree.is_descendant_of(focused, band));

    source.load(60..75);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(band_of(&tree, id), band, "the band is the same node");
    assert_eq!(tree.children(band), content, "with the same content");
    assert_eq!(tree.focused(), Some(focused), "and focus where it was");
    let mut keys = keyed.selected_keys();
    keys.sort_unstable();
    assert_eq!(keys, vec![1001, 1004]);
    assert_eq!(selected_tiles(&tree), vec![1, 4]);

    // Tile 80 has not loaded: its band holds a placeholder, with nothing to
    // focus, until its window arrives.
    tree.focus(id);
    expanded.set(Some(80));
    settle(&mut tree, 400.0, 300.0);
    let detail = the_grid(&tree, id)
        .detail
        .clone()
        .expect("a band is in force");
    assert!(!detail.enterable.get(), "a placeholder");
    source.load(75..90);
    settle(&mut tree, 400.0, 300.0);
    assert!(
        detail.enterable.get(),
        "the band was built again, with its button"
    );
}
