// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The keyboard cursor through [`TreeViewHandle`]: `cursor`, `set_cursor`
//! and `cursor_signal`.
use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{SelectionMode, SelectionModel};

#[derive(Debug)]
struct Leaf;
impl Widget for Leaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(100.0, 20.0).into()
    }
}

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 200.0)
}

/// `roots` root rows, the first holding three collapsed children, as a view
/// with rows 20 tall over a selection in `mode`.
fn make(roots: usize, mode: SelectionMode) -> (TreeView<usize>, Option<NodeId>, SelectionModel) {
    let model = TreeModel::new();
    let mut first = None;
    for r in 0..roots {
        let node = model.insert_root(r, r);
        first.get_or_insert(node);
    }
    if let Some(first) = first {
        for c in 0..3 {
            model.insert_child(first, c, 100 + c);
        }
    }
    let selection = SelectionModel::new(mode);
    let view = TreeView::new(model, |_item, _entry, _sel| Box::new(Leaf))
        .item_height(20.0)
        .selection(selection.clone());
    (view, first, selection)
}

/// The view mounted, laid out and focused.
fn mount(view: TreeView<usize>) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let tv = tree.add(view);
    tree.layout(viewport());
    tree.focus(tv);
    tree.layout(viewport());
    (tree, tv)
}

fn the_view(tree: &WidgetTree, tv: WidgetId) -> &TreeView<usize> {
    tree.widget_as_any(tv)
        .and_then(|any| any.downcast_ref::<TreeView<usize>>())
        .expect("a TreeView<usize>")
}

/// The flat index of the row the tree names as its active descendant, in the
/// frame a running window would deliver next: a layout pass, then
/// `sync_accessibility`, which re-walks only when something asked it to, so
/// a cursor write that dirties nothing leaves the old row named.
fn announced_row(tree: &mut WidgetTree, tv: WidgetId) -> Option<usize> {
    tree.layout(viewport());
    let update = tree.sync_accessibility();
    let active = update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == teksilo_core::accesskit::Role::Tree)
        .and_then(|(_, node)| node.active_descendant())?;
    let map = the_view(tree, tv).row_map.borrow().clone();
    map.into_iter()
        .find(|&(_, id)| teksilo_core::accessibility::widget_id_to_node_id(id) == active)
        .map(|(index, _)| index)
}

/// In a multiple selection the accelerator walks the cursor away from the
/// selection; the handle reports where it went, and the tree announces it.
/// A selection written from outside leaves the cursor where it was.
#[test]
fn the_handle_reads_the_cursor_the_accelerator_walked_away_from_the_selection() {
    let (view, _first, sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    let (mut tree, tv) = mount(view);
    assert_eq!(handle.cursor(), None, "no cursor before any key");

    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(announced_row(&mut tree, tv), Some(0));
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
    assert_eq!(handle.cursor(), Some(2));
    assert_eq!(
        announced_row(&mut tree, tv),
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
    let (view, _first, sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    let (mut tree, tv) = mount(view);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(announced_row(&mut tree, tv), Some(0));

    handle.set_cursor(Some(4));
    assert_eq!(handle.cursor(), Some(4));
    assert_eq!(announced_row(&mut tree, tv), Some(4), "row 4 is announced");
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
    assert_eq!(tree.focused(), Some(tv), "and so did keyboard focus");

    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(handle.cursor(), Some(5), "the next arrow steps from row 4");
    assert_eq!(sel.selected_indices(), vec![5]);
}

/// A cursor set from outside follows its row through an expansion above it,
/// as one the keys set does.
#[test]
fn a_cursor_set_from_outside_follows_its_row_through_an_expansion() {
    let (view, first, _sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    let (mut tree, tv) = mount(view);
    handle.set_cursor(Some(5));

    the_view(&tree, tv).expand(first.expect("a first root"));
    tree.layout(viewport());
    assert_eq!(handle.cursor(), Some(8), "root 5, under three new rows");
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(handle.cursor(), Some(9));
}

/// A cursor set before the tree is mounted is the one its first build finds.
#[test]
fn a_cursor_set_before_mount_is_applied() {
    let (view, _first, sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    handle.set_cursor(Some(3));
    let (mut tree, tv) = mount(view);
    assert_eq!(announced_row(&mut tree, tv), Some(3));
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(sel.selected_indices(), vec![4], "Down from row 3");
}

/// An index past the last visible row is clamped to it; an empty tree has no
/// cursor.
#[test]
fn set_cursor_clamps_to_the_last_visible_row() {
    let (view, _first, _sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    let (_tree, _tv) = mount(view);
    handle.set_cursor(Some(100));
    assert_eq!(handle.cursor(), Some(19), "the children are collapsed");
    handle.set_cursor(None);
    assert_eq!(handle.cursor(), None);

    let (view, _first, _sel) = make(0, SelectionMode::Multi);
    let handle = view.handle();
    let (_tree, _tv) = mount(view);
    handle.set_cursor(Some(3));
    assert_eq!(handle.cursor(), None, "no row to put it on");
}

/// The signal notifies on every write of the cursor: the handle's and the
/// keyboard's.
#[test]
fn cursor_signal_notifies_every_move() {
    let (view, _first, _sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    let (mut tree, _tv) = mount(view);
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
