// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The keyboard cursor through [`ListViewHandle`]: `cursor`, `set_cursor`
//! and `cursor_signal`.
use std::cell::RefCell;
use std::rc::Rc;

use super::tests::FixedLeaf;
use super::*;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{SelectionMode, SelectionModel};

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 200.0)
}

/// A list of `count` rows 20 tall over a selection in `mode`, laid out and
/// focused, with a handle on it.
fn list(
    count: usize,
    mode: SelectionMode,
) -> (WidgetTree, WidgetId, SelectionModel, ListViewHandle) {
    let model = ListModel::from_vec((0..count).collect::<Vec<usize>>());
    let selection = SelectionModel::new(mode);
    let view = ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
        .item_height(20.0)
        .selection(selection.clone());
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    tree.layout(viewport());
    tree.focus(lv);
    tree.layout(viewport());
    (tree, lv, selection, handle)
}

/// The model index of the row the list names as its active descendant, in
/// the frame a running window would deliver next: a layout pass, then
/// `sync_accessibility`, which re-walks only when something asked it to, so
/// a cursor write that dirties nothing leaves the old row named.
fn announced_row(tree: &mut WidgetTree, lv: WidgetId) -> Option<usize> {
    tree.layout(viewport());
    let update = tree.sync_accessibility();
    let active = update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == teksilo_core::accesskit::Role::ListBox)
        .and_then(|(_, node)| node.active_descendant())?;
    let map = tree
        .widget_as_any(lv)?
        .downcast_ref::<ListView<usize>>()?
        .realized_row_ids()
        .borrow()
        .clone();
    map.into_iter()
        .find(|&(_, id)| teksilo_core::accessibility::widget_id_to_node_id(id) == active)
        .map(|(index, _)| index)
}

/// In a multiple selection the accelerator walks the cursor away from the
/// selection; the handle reports where it went, and the list announces it.
/// A selection written from outside leaves the cursor where it was.
#[test]
fn the_handle_reads_the_cursor_the_accelerator_walked_away_from_the_selection() {
    let (mut tree, lv, sel, handle) = list(20, SelectionMode::Multi);
    assert_eq!(handle.cursor(), None, "no cursor before any key");

    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(announced_row(&mut tree, lv), Some(0));
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
    assert_eq!(handle.cursor(), Some(2));
    assert_eq!(
        announced_row(&mut tree, lv),
        Some(2),
        "a cursor-only move is announced"
    );

    sel.select(9);
    assert_eq!(handle.cursor(), Some(2), "a Multi cursor stays put");
}

/// `set_cursor` moves the active descendant, keeps the selection and keyboard
/// focus where they were, and the next arrow steps from it.
#[test]
fn set_cursor_moves_the_active_descendant_and_nothing_else() {
    let (mut tree, lv, sel, handle) = list(20, SelectionMode::Multi);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(announced_row(&mut tree, lv), Some(0));

    handle.set_cursor(Some(4));
    assert_eq!(handle.cursor(), Some(4));
    assert_eq!(announced_row(&mut tree, lv), Some(4), "row 4 is announced");
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
    assert_eq!(tree.focused(), Some(lv), "and so did keyboard focus");

    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(handle.cursor(), Some(5), "the next arrow steps from row 4");
    assert_eq!(sel.selected_indices(), vec![5]);
}

/// A cursor set before the list is mounted is the one its first build finds.
#[test]
fn a_cursor_set_before_mount_is_applied() {
    let model = ListModel::from_vec((0..20).collect::<Vec<usize>>());
    let sel = SelectionModel::new(SelectionMode::Multi);
    let view = ListView::new(model.clone(), |_i, _item, _sel| {
        Box::new(FixedLeaf(100.0, 20.0))
    })
    .item_height(20.0)
    .selection(sel.clone());
    let handle = view.handle();
    handle.set_cursor(Some(3));

    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    tree.layout(viewport());
    tree.focus(lv);
    tree.layout(viewport());
    assert_eq!(announced_row(&mut tree, lv), Some(3));
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![4], "Down from row 3");

    // Still the list's cursor after the data changed and the rows rebuilt.
    model.push(20);
    tree.layout(viewport());
    handle.set_cursor(Some(1));
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![2], "Down from row 1");
}

/// An index past the last row is clamped to it; an empty list has no cursor.
#[test]
fn set_cursor_clamps_to_the_last_row() {
    let (_tree, _lv, _sel, handle) = list(20, SelectionMode::Multi);
    handle.set_cursor(Some(100));
    assert_eq!(handle.cursor(), Some(19));
    handle.set_cursor(None);
    assert_eq!(handle.cursor(), None);

    let (_tree, _lv, _sel, handle) = list(0, SelectionMode::Multi);
    handle.set_cursor(Some(3));
    assert_eq!(handle.cursor(), None, "no row to put it on");
}

/// The signal notifies on every write of the cursor: the handle's and the
/// keyboard's.
#[test]
fn cursor_signal_notifies_every_move() {
    let (mut tree, _lv, _sel, handle) = list(20, SelectionMode::Multi);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let signal = handle.cursor_signal();
    let _observer = signal.observe({
        let seen = seen.clone();
        move |cursor| seen.borrow_mut().push(*cursor)
    });

    handle.set_cursor(Some(3));
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(*seen.borrow(), vec![Some(3), Some(4)]);
    assert_eq!(signal.get(), Some(4));
}

/// A row is the list's active descendant, so it must read as focusable to a
/// platform adapter (UIA `IsKeyboardFocusable`), and an assistive `Focus` on
/// it does what Ctrl+arrow does: the cursor moves to it, the selection stays,
/// and the keyboard is on the list.
#[test]
fn an_assistive_focus_on_a_row_moves_the_cursor_there() {
    use crate::common::item_focus::test_support::{assistive_focus, reads_as_focusable};
    let (mut tree, lv, sel, handle) = list(20, SelectionMode::Multi);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(announced_row(&mut tree, lv), Some(0));
    let row = tree
        .widget_as_any(lv)
        .and_then(|any| any.downcast_ref::<ListView<usize>>())
        .and_then(|view| {
            view.realized_row_ids()
                .borrow()
                .iter()
                .find(|&&(index, _)| index == 3)
                .map(|&(_, id)| id)
        })
        .expect("row 3 is realized");
    let node = teksilo_core::accessibility::widget_id_to_node_id(row);
    assert!(
        reads_as_focusable(&mut tree, node),
        "a row the list can point at reads as focusable"
    );

    tree.clear_focus();
    assert!(assistive_focus(&mut tree, node), "the list took it");
    assert_eq!(handle.cursor(), Some(3));
    assert_eq!(tree.focused(), Some(lv), "the keyboard is on the list");
    assert_eq!(announced_row(&mut tree, lv), Some(3), "row 3 is announced");
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
}

/// A presentational list (the rows of a `ComboBox` or `CommandPalette`
/// popup, whose owner publishes the options) takes no assistive `Focus` on
/// a row: its rows are no options, and the list takes no focus.
#[test]
fn a_presentational_list_takes_no_assistive_focus_on_a_row() {
    use crate::common::item_focus::test_support::assistive_focus;
    let model = ListModel::from_vec((0..20).collect::<Vec<usize>>());
    let view = ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
        .item_height(20.0)
        .presentational(true);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    tree.layout(viewport());
    let row = tree
        .widget_as_any(lv)
        .and_then(|any| any.downcast_ref::<ListView<usize>>())
        .and_then(|view| {
            view.realized_row_ids()
                .borrow()
                .iter()
                .find(|&&(index, _)| index == 3)
                .map(|&(_, id)| id)
        })
        .expect("row 3 is realized");
    let node = teksilo_core::accessibility::widget_id_to_node_id(row);
    assert!(!assistive_focus(&mut tree, node));
    assert_eq!(handle.cursor(), None, "the cursor did not move");
    assert_eq!(tree.focused(), None);
}

/// A list inside a disabled container takes no input, so an assistive
/// `Focus` on one of its rows moves nothing, puts the keyboard nowhere, and
/// is reported unhandled.
#[test]
fn a_row_of_a_list_in_a_disabled_container_takes_no_assistive_focus() {
    use crate::common::item_focus::test_support::assistive_focus;
    let model = ListModel::from_vec((0..20).collect::<Vec<usize>>());
    let view =
        ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0))).item_height(20.0);
    let handle = view.handle();
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    let container = tree.add(crate::primitives::VStack::new().child(lv));
    tree.enabled_when(container, teksilo_core::signal::Signal::new(false));
    tree.layout(viewport());
    let row = tree
        .widget_as_any(lv)
        .and_then(|any| any.downcast_ref::<ListView<usize>>())
        .and_then(|view| {
            view.realized_row_ids()
                .borrow()
                .iter()
                .find(|&&(index, _)| index == 3)
                .map(|&(_, id)| id)
        })
        .expect("row 3 is realized");
    let node = teksilo_core::accessibility::widget_id_to_node_id(row);
    assert!(!assistive_focus(&mut tree, node), "nothing took it");
    assert_eq!(handle.cursor(), None, "the cursor did not move");
    assert_eq!(tree.focused(), None, "the keyboard went nowhere");
}

/// `.access_label` on a list names the `Role::ListBox` node itself, not a
/// wrapper around it.
#[test]
fn access_label_names_the_list_box_node() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let model = ListModel::from_vec((0..3).collect::<Vec<usize>>());
    let view = ListView::new(model, |_i, _item, _sel| Box::new(FixedLeaf(100.0, 20.0)))
        .item_height(20.0)
        .access_label("Songs");
    let mut tree = WidgetTree::new();
    let lv = tree.add(view);
    tree.layout(viewport());
    let update = tree.sync_accessibility();
    let named: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.label() == Some("Songs"))
        .collect();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].1.role(), teksilo_core::accesskit::Role::ListBox);
    assert_eq!(
        named[0].0,
        teksilo_core::accessibility::widget_id_to_node_id(lv)
    );
}
