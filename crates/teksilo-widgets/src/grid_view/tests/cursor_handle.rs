// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The keyboard cursor through `GridViewHandle`: `cursor`, `set_cursor` and
//! `cursor_signal`.
//!
//! The grids here are `grid_keyboard_fixture`'s: 30 tiles of 100x50, three
//! columns at 400 dp, rows stepping 58 dp, laid out and focused.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use teksilo_core::event::{Key, Modifiers};

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

fn handle_of(tree: &WidgetTree, id: WidgetId) -> GridViewHandle {
    the_grid(tree, id).handle()
}

/// The widget the grid names as its active descendant in the update a
/// running window would deliver now.
fn announced_tile(tree: &mut WidgetTree) -> Option<teksilo_core::accesskit::NodeId> {
    let update = tree.sync_accessibility();
    update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == teksilo_core::accesskit::Role::Grid)
        .and_then(|(_, node)| node.active_descendant())
}

fn node_of(tree: &WidgetTree, grid: WidgetId, index: usize) -> teksilo_core::accesskit::NodeId {
    teksilo_core::accessibility::widget_id_to_node_id(tile_at(tree, grid, index))
}

/// In a multiple selection the cursor walks away from the selection, and the
/// handle reports where it went; a selection written from outside leaves it.
#[test]
fn the_handle_reads_the_cursor_the_accelerator_walked_away_from_the_selection() {
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    let handle = handle_of(&tree, id);
    assert_eq!(handle.cursor(), None, "no cursor before any key");

    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    press(&mut tree, Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
    assert_eq!(handle.cursor(), Some(4), "one right, one row down");

    sel.select(20);
    assert_eq!(handle.cursor(), Some(4), "a Multi cursor stays put");
}

/// `set_cursor` moves the ring and the active descendant, keeps the selection
/// and keyboard focus where they were, and the next arrow steps from it.
#[test]
fn set_cursor_moves_the_ring_and_the_active_descendant_and_nothing_else() {
    let (mut tree, id, sel) = grid_keyboard_fixture(SelectionMode::Multi);
    let handle = handle_of(&tree, id);
    // A key, not a click or a programmatic focus: the ring is keyboard-only.
    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![0]);
    assert_eq!(announced_tile(&mut tree), Some(node_of(&tree, id, 0)));
    let before = ring_top(&mut tree).expect("the ring is painted");
    assert!(before < 4.0, "on tile 0's row, at {before}");

    handle.set_cursor(Some(7));
    tree.layout(viewport());
    assert_eq!(handle.cursor(), Some(7));
    assert_eq!(
        announced_tile(&mut tree),
        Some(node_of(&tree, id, 7)),
        "tile 7 is the active descendant"
    );
    let after = ring_top(&mut tree).expect("the ring is painted");
    assert_close(after - before, 116.0, "the ring moved down to tile 7's row");
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
    assert_eq!(tree.focused(), Some(id), "and so did keyboard focus");

    press(&mut tree, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(handle.cursor(), Some(8), "the next arrow steps from tile 7");
    assert_eq!(sel.selected_indices(), vec![8]);
}

/// A cursor set before the grid is mounted is the one its first build finds,
/// and the handle keeps reaching the grid through its rebuilds.
#[test]
fn a_cursor_set_before_mount_is_applied_and_survives_rebuilds() {
    let model = ListModel::from_vec((0..30).collect::<Vec<usize>>());
    let sel = SelectionModel::new(SelectionMode::Multi);
    let grid = GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0)
        .selection(sel.clone());
    let handle = grid.handle();
    handle.set_cursor(Some(5));

    let mut tree = WidgetTree::new();
    let id = tree.add(grid);
    tree.layout(viewport());
    tree.focus(id);
    assert_eq!(grid_focus(&tree, id), Some(5), "the grid starts there");
    assert_eq!(announced_tile(&mut tree), Some(node_of(&tree, id, 5)));
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![8], "Down from tile 5");

    // A change to the data rebuilds the grid; the handle still moves it.
    model.push(30);
    tree.layout(viewport());
    handle.set_cursor(Some(2));
    tree.layout(viewport());
    assert_eq!(grid_focus(&tree, id), Some(2));
    press(&mut tree, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![5], "Down from tile 2");
}

/// An index past the last tile is clamped to it; an empty grid has no cursor.
#[test]
fn set_cursor_clamps_to_the_last_tile() {
    let (tree, id, _sel) = grid_keyboard_fixture(SelectionMode::Multi);
    let handle = handle_of(&tree, id);
    handle.set_cursor(Some(100));
    assert_eq!(handle.cursor(), Some(29));
    handle.set_cursor(None);
    assert_eq!(handle.cursor(), None);

    let (tree, id, _model) = make_grid(0);
    let handle = handle_of(&tree, id);
    handle.set_cursor(Some(3));
    assert_eq!(handle.cursor(), None, "no tile to put it on");
}

/// The signal notifies on every write of the cursor: the handle's and the
/// keyboard's.
#[test]
fn cursor_signal_notifies_every_move() {
    let (mut tree, id, _sel) = grid_keyboard_fixture(SelectionMode::Multi);
    let handle = handle_of(&tree, id);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let signal = handle.cursor_signal();
    let _observer = signal.observe({
        let seen = seen.clone();
        move |cursor| seen.borrow_mut().push(*cursor)
    });

    handle.set_cursor(Some(3));
    press(&mut tree, Key::ArrowRight, Modifiers::CTRL);
    assert_eq!(*seen.borrow(), vec![Some(3), Some(4)]);
    assert_eq!(signal.get(), Some(4));
}
