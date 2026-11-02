// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A data view's row menu: the keyboard (the Menu key, Shift+F10) and a
//! screen reader's "show menu" open the application's own row menu, as a right
//! click does, and on a reorderable view that menu ends with a "Move" submenu.
//! With no application menu the Move rows open alone, flat, as before.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget::{EventContext, Widget};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel, TreeModel};
use teksilo_i18n::lit;
use teksilo_widgets::table_view::{Column, ColumnWidth, TableSelectionMode, TableView};
use teksilo_widgets::*;

const ROW: f32 = 30.0;

#[derive(Debug)]
struct FixedLeaf(f32, f32);

impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(self.0, self.1).into()
    }
}

/// The application's own menu, one row named `name`.
fn app_menu(name: &'static str) -> impl Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>> {
    move |_, _| Some(Box::new(MenuList::new().item(MenuItem::new(lit!(name)))) as Box<dyn Widget>)
}

/// A leaf carrying the application's menu.
fn with_menu(name: &'static str) -> Box<dyn Widget> {
    Box::new(FixedLeaf(180.0, ROW).context_menu(app_menu(name)))
}

fn labels(tree: &mut WidgetTree) -> Vec<String> {
    tree.sync_accessibility()
        .nodes
        .iter()
        .filter_map(|(_, node)| node.label().map(str::to_string))
        .collect()
}

fn node_with_label(tree: &mut WidgetTree, label: &str) -> Option<WidgetId> {
    tree.sync_accessibility()
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .and_then(|(id, _)| teksilo_core::accessibility::node_id_to_widget_id_maybe(*id))
}

fn click_row(tree: &mut WidgetTree, label: &str) {
    let id = node_with_label(tree, label).unwrap_or_else(|| panic!("no {label:?} row"));
    tree.click(id);
    tree.layout(SizeProposal::exact(400.0, 400.0));
}

fn close(tree: &mut WidgetTree, view: WidgetId) {
    tree.press_key(Key::Escape, Modifiers::NONE);
    tree.focus(view);
}

/// The three ways in that do not point: the Menu key, Shift+F10 and the
/// assistive action, each on the focused view.
fn keyboard_and_at(tree: &mut WidgetTree, view: WidgetId, way: usize) {
    tree.focus(view);
    match way {
        0 => tree.press_key(Key::ContextMenu, Modifiers::NONE),
        1 => tree.press_key(Key::F10, Modifiers::SHIFT),
        _ => tree.dispatch_event(WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::ShowContextMenu,
            target: Some(view),
            target_node: teksilo_core::accessibility::widget_id_to_node_id(view),
            data: None,
        }),
    }
    tree.layout(SizeProposal::exact(400.0, 400.0));
}

/// Open by every non-pointing way: the app's row and a Move submenu, not the
/// flat Move rows. Then move the row through the submenu.
fn assert_app_menu_with_move(tree: &mut WidgetTree, view: WidgetId, app: &str) {
    for way in 0..3 {
        keyboard_and_at(tree, view, way);
        let seen = labels(tree);
        assert!(seen.iter().any(|l| l == app), "way {way}: {seen:?}");
        assert!(seen.iter().any(|l| l == "Move"), "way {way}: {seen:?}");
        assert!(
            !seen.iter().any(|l| l == "Move Up"),
            "way {way}, flat: {seen:?}"
        );
        close(tree, view);
    }
    keyboard_and_at(tree, view, 0);
    click_row(tree, "Move");
    click_row(tree, "Move to Top");
}

fn list_order(model: &ListModel<&'static str>) -> Vec<&'static str> {
    (0..model.len())
        .map(|i| model.with_item(i, |v| *v).expect("in range"))
        .collect()
}

fn list_view(app: bool, reorderable: bool) -> (WidgetTree, WidgetId, ListModel<&'static str>) {
    let model = ListModel::from_vec(vec!["alpha", "beta", "gamma", "delta"]);
    let selection = SelectionModel::new(SelectionMode::Single);
    selection.select(2);
    let mut tree = WidgetTree::new();
    let view = tree.add(
        ListView::new(model.clone(), move |_i, _item, _sel| {
            if app {
                with_menu("Open")
            } else {
                Box::new(FixedLeaf(180.0, ROW)) as Box<dyn Widget>
            }
        })
        .item_height(ROW)
        .selection(selection)
        .reorderable(reorderable),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    (tree, view, model)
}

#[test]
fn list_view_rows_open_the_app_menu_with_a_move_submenu() {
    let (mut tree, view, model) = list_view(true, true);
    assert_app_menu_with_move(&mut tree, view, "Open");
    assert_eq!(list_order(&model), ["gamma", "alpha", "beta", "delta"]);
}

#[test]
fn list_view_rows_with_no_app_menu_open_the_move_rows_flat() {
    let (mut tree, view, _) = list_view(false, true);
    keyboard_and_at(&mut tree, view, 0);
    let seen = labels(&mut tree);
    assert!(seen.iter().any(|l| l == "Move Up"), "{seen:?}");
    assert!(!seen.iter().any(|l| l == "Move"), "{seen:?}");
}

#[test]
fn list_view_rows_of_a_fixed_view_open_the_app_menu() {
    let (mut tree, view, _) = list_view(true, false);
    for way in 0..3 {
        keyboard_and_at(&mut tree, view, way);
        let seen = labels(&mut tree);
        assert!(seen.iter().any(|l| l == "Open"), "way {way}: {seen:?}");
        assert!(!seen.iter().any(|l| l == "Move"), "way {way}: {seen:?}");
        close(&mut tree, view);
    }
}

/// A right click on a row gets the same merged menu.
#[test]
fn a_right_click_on_a_row_gets_the_move_submenu_too() {
    let (mut tree, view, _) = list_view(true, true);
    let _ = view;
    let row = node_with_label(&mut tree, "gamma");
    let at = match row {
        Some(id) => tree.bounds(id).center(),
        None => Point::new(50.0, ROW * 2.5),
    };
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let seen = labels(&mut tree);
    assert!(seen.iter().any(|l| l == "Open"), "{seen:?}");
    assert!(seen.iter().any(|l| l == "Move"), "{seen:?}");
}

/// A menu on the view itself is farther than the row: the row's Move rows
/// open, flat.
#[test]
fn a_view_level_menu_leaves_the_rows_their_move_rows() {
    let model = ListModel::from_vec(vec!["alpha", "beta", "gamma"]);
    let selection = SelectionModel::new(SelectionMode::Single);
    selection.select(1);
    let mut tree = WidgetTree::new();
    let view = tree.add(
        ListView::new(model.clone(), |_i, _item, _sel| {
            Box::new(FixedLeaf(180.0, ROW)) as Box<dyn Widget>
        })
        .item_height(ROW)
        .selection(selection)
        .reorderable(true)
        .context_menu(app_menu("New Folder")),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    for way in 0..3 {
        keyboard_and_at(&mut tree, view, way);
        let seen = labels(&mut tree);
        assert!(seen.iter().any(|l| l == "Move Up"), "way {way}: {seen:?}");
        close(&mut tree, view);
    }
}

#[test]
fn tree_view_rows_open_the_app_menu_with_a_move_submenu() {
    let model: TreeModel<&'static str> = TreeModel::new();
    for (i, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
        model.insert_root(i, name);
    }
    let selection = SelectionModel::new(SelectionMode::Single);
    selection.select(2);
    let mut tree = WidgetTree::new();
    let view = tree.add(
        TreeView::new(model.clone(), |_item: &&'static str, _entry, _sel| {
            with_menu("Rename")
        })
        .item_height(ROW)
        .selection(selection)
        .reorderable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    assert_app_menu_with_move(&mut tree, view, "Rename");
    let roots: Vec<&'static str> = (0..model.root_count())
        .map(|i| model.with_item(model.root(i), |v| *v).expect("live"))
        .collect();
    assert_eq!(roots, ["C", "A", "B", "D"]);
}

fn table(
    cols: usize,
) -> (
    WidgetTree,
    WidgetId,
    ListModel<&'static str>,
    TableView<&'static str>,
) {
    let model = ListModel::from_vec(vec!["alpha", "beta", "gamma", "delta"]);
    let selection = SelectionModel::new(SelectionMode::Single);
    selection.select(2);
    let mut view = TableView::new(model.clone());
    for (col, name) in [("first", "Edit First"), ("second", "Edit Second")]
        .into_iter()
        .take(cols)
    {
        view = view.add_column(
            Column::new(col, lit!(col), move |_v: &&'static str, _cx| {
                with_menu(name)
            })
            .width(ColumnWidth::Fixed(150.0)),
        );
    }
    let view = view
        .header_height(0.0)
        .row_height(ROW)
        .selection_mode(TableSelectionMode::SingleRow)
        .selection(selection)
        .reorderable(true);
    let tree = WidgetTree::new();
    (tree, WidgetId::default(), model, view)
}

#[test]
fn table_view_rows_open_the_cell_menu_with_a_move_submenu() {
    let (mut tree, _, model, view) = table(2);
    let view = tree.add(view);
    tree.layout(SizeProposal::exact(400.0, 400.0));
    assert_app_menu_with_move(&mut tree, view, "Edit First");
    assert_eq!(list_order(&model), ["gamma", "alpha", "beta", "delta"]);
}

#[test]
fn table_view_opens_the_menu_of_the_focused_cell() {
    let (mut tree, _, _, view) = table(2);
    let cell = view.focused_cell_signal().clone();
    let view = tree.add(view);
    tree.layout(SizeProposal::exact(400.0, 400.0));
    cell.set(Some((2, 1)));
    tree.layout(SizeProposal::exact(400.0, 400.0));
    keyboard_and_at(&mut tree, view, 0);
    let seen = labels(&mut tree);
    assert!(seen.iter().any(|l| l == "Edit Second"), "{seen:?}");
    assert!(!seen.iter().any(|l| l == "Edit First"), "{seen:?}");
}

#[test]
fn tree_table_view_rows_open_the_cell_menu_with_a_move_submenu() {
    let model: TreeModel<&'static str> = TreeModel::new();
    for (i, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
        model.insert_root(i, name);
    }
    let selection = SelectionModel::new(SelectionMode::Single);
    selection.select(2);
    let mut tree = WidgetTree::new();
    let view = tree.add(
        TreeTableView::new(model.clone())
            .add_column(
                Column::new("name", lit!("Name"), |_v: &&'static str, _cx| {
                    with_menu("Inspect")
                })
                .width(ColumnWidth::Fixed(180.0)),
            )
            .header_height(0.0)
            .row_height(ROW)
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(selection)
            .reorderable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    assert_app_menu_with_move(&mut tree, view, "Inspect");
}

#[test]
fn grid_view_tiles_keep_the_move_rows_under_the_app_menu() {
    let model = ListModel::from_vec(vec!["a", "b", "c", "d", "e", "f"]);
    let selection = SelectionModel::new(SelectionMode::Single);
    selection.select(2);
    let mut tree = WidgetTree::new();
    let view = tree.add(
        GridView::new(model.clone(), |_cx| {
            Box::new(FixedLeaf(40.0, 40.0)) as Box<dyn Widget>
        })
        .sizing(GridSizing::Fixed {
            width: 40.0,
            height: 40.0,
        })
        .selection(selection)
        .reorderable(true)
        .tile_context_menu(|_i, pos, ctx| app_menu("Preview")(pos, ctx)),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    for way in 0..3 {
        keyboard_and_at(&mut tree, view, way);
        let seen = labels(&mut tree);
        assert!(seen.iter().any(|l| l == "Preview"), "way {way}: {seen:?}");
        assert!(seen.iter().any(|l| l == "Move"), "way {way}: {seen:?}");
        close(&mut tree, view);
    }
}

/// The widgets whose accessibility node has `role`, in tree order.
fn nodes_with_role(tree: &mut WidgetTree, role: teksilo_core::accesskit::Role) -> Vec<WidgetId> {
    tree.sync_accessibility()
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == role)
        .filter_map(|(id, _)| teksilo_core::accessibility::node_id_to_widget_id_maybe(*id))
        .collect()
}

/// A screen reader's "show menu" on the node itself (VoiceOver acts on the
/// row it navigated to), not on the focused view.
fn show_menu_on(tree: &mut WidgetTree, node: WidgetId) {
    tree.dispatch_event(WidgetEvent::AccessAction {
        action: teksilo_core::accesskit::Action::ShowContextMenu,
        target: Some(node),
        target_node: teksilo_core::accessibility::widget_id_to_node_id(node),
        data: None,
    });
    tree.layout(SizeProposal::exact(400.0, 400.0));
}

fn assert_cell_menu_with_move(tree: &mut WidgetTree, app: &str, what: &str) {
    let seen = labels(tree);
    assert!(seen.iter().any(|l| l == app), "{what}: {seen:?}");
    assert!(seen.iter().any(|l| l == "Move"), "{what}: {seen:?}");
    assert!(
        !seen.iter().any(|l| l == "Move Up"),
        "{what}, flat: {seen:?}"
    );
}

#[test]
fn show_menu_on_a_table_row_or_cell_node_opens_the_cell_menu_with_move() {
    use teksilo_core::accesskit::Role;
    // The row starts at its first displayed cell; the last cell is a cell
    // of the second column, whose own menu opens.
    for (role, app) in [(Role::Row, "Edit First"), (Role::Cell, "Edit Second")] {
        let (mut tree, _, _, view) = table(2);
        let view = tree.add(view);
        tree.layout(SizeProposal::exact(400.0, 400.0));
        // A body row or cell (the header row comes first).
        let node = *nodes_with_role(&mut tree, role)
            .last()
            .expect("a body node");
        show_menu_on(&mut tree, node);
        assert_cell_menu_with_move(&mut tree, app, &format!("{role:?}"));
        close(&mut tree, view);
    }
}

#[test]
fn show_menu_on_a_tree_table_row_node_opens_the_cell_menu_with_move() {
    use teksilo_core::accesskit::Role;
    let model: TreeModel<&'static str> = TreeModel::new();
    for (i, name) in ["A", "B", "C", "D"].into_iter().enumerate() {
        model.insert_root(i, name);
    }
    let mut tree = WidgetTree::new();
    tree.add(
        TreeTableView::new(model.clone())
            .add_column(
                Column::new("name", lit!("Name"), |_v: &&'static str, _cx| {
                    with_menu("Inspect")
                })
                .width(ColumnWidth::Fixed(180.0)),
            )
            .header_height(0.0)
            .row_height(ROW)
            .selection_mode(TableSelectionMode::SingleRow)
            .reorderable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 400.0));
    let row = *nodes_with_role(&mut tree, Role::Row)
        .last()
        .expect("a body row");
    show_menu_on(&mut tree, row);
    assert_cell_menu_with_move(&mut tree, "Inspect", "tree-table row");
}
