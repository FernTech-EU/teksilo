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

/// The first realized row wrapper, the node that answers a screen reader's
/// `Click` (rows sit under the body pane, the view's first child).
fn first_row(tree: &WidgetTree, tv: WidgetId) -> WidgetId {
    let pane = tree.children(tv)[0];
    tree.children(pane)[0]
}

fn click(tree: &mut WidgetTree, row: WidgetId) {
    let mut noop = teksilo_core::window::NoopWindowOps;
    tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(row),
        teksilo_core::accesskit::Action::Click,
        None,
        &mut noop,
    );
    tree.layout(viewport());
}

/// A screen reader's `Click` on a row while the view holds focus leaves focus
/// on the view.
#[test]
fn an_at_click_on_a_row_keeps_focus_on_the_view() {
    let (view, _, _) = make(3, SelectionMode::Single);
    let (mut tree, tv) = mount(view);
    let row = first_row(&tree, tv);
    click(&mut tree, row);
    assert_eq!(tree.focused(), Some(tv));
}

/// Focus on the row node itself (where a screen reader's focus can put it):
/// the `Click` selects, the selection rebuild may replace the row, and focus
/// must not be left on nothing.
#[test]
fn an_at_click_on_a_focused_row_node_does_not_leave_focus_on_nothing() {
    let (view, _, _) = make(3, SelectionMode::Single);
    let (mut tree, tv) = mount(view);
    let row = first_row(&tree, tv);
    tree.focus(row);
    click(&mut tree, row);
    let focused = tree.focused().expect("focus is somewhere");
    assert!(
        focused == tv || tree.is_active(focused),
        "focus is on a live node"
    );
    if !tree.is_active(row) {
        assert_eq!(focused, tv, "the row went: focus is on the view");
    }
}

/// A row is the tree's active descendant, so it must read as focusable to a
/// platform adapter (UIA `IsKeyboardFocusable`), and an assistive `Focus` on
/// it does what Ctrl+arrow does: the cursor moves to it, the selection stays,
/// and the keyboard is on the tree.
#[test]
fn an_assistive_focus_on_a_row_moves_the_cursor_there() {
    use crate::common::item_focus::test_support::{assistive_focus, reads_as_focusable};
    let (view, _first, sel) = make(20, SelectionMode::Multi);
    let handle = view.handle();
    let (mut tree, tv) = mount(view);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(announced_row(&mut tree, tv), Some(0));
    let row = the_view(&tree, tv)
        .row_map
        .borrow()
        .iter()
        .find(|&&(index, _)| index == 3)
        .map(|&(_, id)| id)
        .expect("row 3 is realized");
    let node = teksilo_core::accessibility::widget_id_to_node_id(row);
    assert!(
        reads_as_focusable(&mut tree, node),
        "a row the tree can point at reads as focusable"
    );

    tree.clear_focus();
    assert!(assistive_focus(&mut tree, node), "the tree took it");
    assert_eq!(handle.cursor(), Some(3));
    assert_eq!(tree.focused(), Some(tv), "the keyboard is on the tree");
    assert_eq!(announced_row(&mut tree, tv), Some(3), "row 3 is announced");
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
}

/// The id of realized flat row `index`, if it is realized.
fn realized_row(tree: &WidgetTree, tv: WidgetId, index: usize) -> Option<WidgetId> {
    the_view(tree, tv)
        .row_map
        .borrow()
        .iter()
        .find(|&&(i, _)| i == index)
        .map(|&(_, id)| id)
}

/// Whether flat row `index` is realized and lies wholly inside the tree's
/// viewport, after the layout pass the next frame would run.
fn row_in_view(tree: &mut WidgetTree, tv: WidgetId, index: usize) -> bool {
    tree.layout(viewport());
    let Some(row) = realized_row(tree, tv, index) else {
        return false;
    };
    let (view, row) = (tree.bounds(tv), tree.bounds(row));
    row.y >= view.y - 0.01 && row.y + row.height <= view.y + view.height + 0.01
}

/// An assistive `Focus` on a realized row below the viewport shows it, as
/// Ctrl+ArrowDown onto it would. Windows found a row read out and left off
/// screen.
#[test]
fn an_assistive_focus_on_a_row_below_the_viewport_scrolls_it_into_view() {
    use crate::common::item_focus::test_support::assistive_focus;
    let (view, _first, sel) = make(60, SelectionMode::Multi);
    let handle = view.handle();
    let (mut tree, tv) = mount(view);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    let below = the_view(&tree, tv)
        .row_map
        .borrow()
        .iter()
        .map(|&(index, _)| index)
        .max()
        .expect("rows are realized");
    assert!(
        !row_in_view(&mut tree, tv, below),
        "row {below} is realized below the viewport"
    );
    let node =
        teksilo_core::accessibility::widget_id_to_node_id(realized_row(&tree, tv, below).unwrap());

    assert_eq!(tree.focused(), Some(tv), "the tree already has the keys");
    assert!(assistive_focus(&mut tree, node), "the tree took it");
    assert_eq!(handle.cursor(), Some(below));
    assert!(
        row_in_view(&mut tree, tv, below),
        "row {below} was scrolled into view"
    );
    assert_eq!(sel.selected_indices(), vec![0], "the selection stayed");
}
