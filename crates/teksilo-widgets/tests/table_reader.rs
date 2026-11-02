// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A row-selecting `TableView` as a screen reader gets it, read through
//! `accesskit_consumer` as every platform adapter reads it.
//!
//! UIA exposes a selection state on a `Role::GridCell` and never on a
//! `Role::Row` or a `Role::Cell`, and the view puts the reader on a cell (its
//! `active_descendant`). So a table whose rows are selected had to make its
//! cells grid cells, or the selection was silent on Windows: the cell the
//! reader is on now says whether its row is selected, and a table with no
//! selection keeps plain cells. The sibling `tree_table_reader.rs` checks the
//! same for `TreeTableView`.

use accesskit_consumer::{NodeRef, Tree, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{NodeId, Role};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel};
use teksilo_i18n::lit;
use teksilo_widgets::table_view::{Column, ColumnWidth, TableSelectionMode, TableView};
use teksilo_widgets::*;

const SIZE: SizeProposal = SizeProposal {
    width: Some(600.0),
    height: Some(300.0),
};

/// A two-column table of files in `mode`.
fn table(mode: TableSelectionMode) -> (WidgetTree, WidgetId, SelectionModel) {
    let model = ListModel::from_vec(vec![
        ("Cargo.toml", "toml"),
        ("README", "text"),
        ("build.rs", "rust"),
    ]);
    let selection = SelectionModel::new(SelectionMode::Multi);
    let view = TableView::new(model)
        .add_column(
            Column::new("name", lit!("Name"), |row: &(&str, &str), _| {
                Box::new(TextWidget::new(lit!(row.0)))
            })
            .width(ColumnWidth::Flex(2.0)),
        )
        .add_column(
            Column::new("kind", lit!("Kind"), |row: &(&str, &str), _| {
                Box::new(TextWidget::new(lit!(row.1)))
            })
            .width(ColumnWidth::Flex(1.0)),
        )
        .row_height(26.0)
        .selection_mode(mode)
        .selection(selection.clone());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let view = tree.add(view);
    tree.layout(SIZE);
    (tree, view, selection)
}

/// What a reader hears a cell say: its name where the platform names cells
/// from their text (Windows, Linux), else the text of the labels it holds,
/// which VoiceOver reads from the cell's children (see the core's cell
/// naming pass, off on macOS). Each shape is checked against the other.
fn heard(n: &NodeRef<'_>) -> String {
    fn texts(n: &NodeRef<'_>, out: &mut Vec<String>) {
        for child in n.filtered_children(common_filter) {
            if child.role() == Role::Label {
                if let Some(v) = child.value() {
                    out.push(v);
                }
            } else {
                texts(&child, out);
            }
        }
    }
    let mut parts = Vec::new();
    texts(n, &mut parts);
    let text = parts.join(" ");
    if cfg!(target_os = "macos") {
        assert_eq!(n.label(), None, "macOS: a cell is read from its children");
        text
    } else {
        let name = n.label().unwrap_or_default();
        assert!(
            text.is_empty() || name == text,
            "a cell is named by the text it shows: {name:?} for {text:?}"
        );
        name
    }
}

/// Each body cell the reader can reach: role, name (as every adapter
/// computes it), selected state.
fn cells(tree: &mut WidgetTree) -> Vec<(Role, String, Option<bool>)> {
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let mut out = Vec::new();
    let mut stack: Vec<NodeRef<'_>> = vec![state.root()];
    while let Some(n) = stack.pop() {
        if matches!(n.role(), Role::Cell | Role::GridCell) {
            out.push((n.role(), heard(&n), n.is_selected()));
        }
        let kids: Vec<_> = n.filtered_children(common_filter).collect();
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// The role of the view's own node.
fn view_role(tree: &mut WidgetTree, view: WidgetId) -> Role {
    let update = tree.sync_accessibility();
    let view_node = teksilo_core::accessibility::widget_id_to_node_id(view);
    update
        .nodes
        .iter()
        .find(|(id, _)| *id == view_node)
        .map(|(_, n)| n.role())
        .expect("the view's node")
}

/// The cell the view puts the reader on (its `active_descendant`), as the
/// consumer resolves it.
fn reader_cell(tree: &mut WidgetTree, view: WidgetId) -> (Role, String, Option<bool>) {
    let update = tree.sync_accessibility();
    let view_node = teksilo_core::accessibility::widget_id_to_node_id(view);
    let active: NodeId = update
        .nodes
        .iter()
        .find(|(id, _)| *id == view_node)
        .and_then(|(_, n)| n.active_descendant())
        .expect("the view names the cell the reader is on");
    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let mut stack = vec![state.root()];
    while let Some(n) = stack.pop() {
        if n.locate().0 == active {
            return (n.role(), heard(&n), n.is_selected());
        }
        stack.extend(n.children());
    }
    panic!("the active descendant is in the tree");
}

fn click_text(tree: &mut WidgetTree, text: &str) {
    let update = tree.sync_accessibility();
    let id = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::Label && n.value() == Some(text))
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("no {text:?} on screen"));
    let id = teksilo_core::accessibility::node_id_to_widget_id_maybe(id).expect("a widget");
    tree.click(id);
    tree.layout(SIZE);
}

fn key(tree: &mut WidgetTree, key: Key, modifiers: Modifiers) {
    tree.press_key(key, modifiers);
    tree.layout(SIZE);
}

#[test]
fn the_cell_the_reader_is_on_says_its_row_selection() {
    for mode in [TableSelectionMode::SingleRow, TableSelectionMode::MultiRow] {
        let (mut tree, view, selection) = table(mode);
        assert_eq!(view_role(&mut tree, view), Role::Grid, "{mode:?}");
        click_text(&mut tree, "Cargo.toml");
        assert_eq!(selection.selected_indices(), vec![0], "{mode:?}");
        // A grid cell, so UIA exposes `SelectionItem.IsSelected` on it.
        assert_eq!(
            reader_cell(&mut tree, view),
            (Role::GridCell, "Cargo.toml".to_string(), Some(true)),
            "{mode:?}"
        );
        let all = cells(&mut tree);
        assert!(
            all.iter().all(|(role, _, _)| *role == Role::GridCell),
            "{mode:?}: every body cell is a grid cell: {all:?}"
        );
        assert!(
            all.iter()
                .filter(|(_, n, _)| n == "Cargo.toml" || n == "toml")
                .all(|(_, _, sel)| *sel == Some(true)),
            "{mode:?}: every cell of the selected row says so: {all:?}"
        );
        assert!(
            all.iter()
                .filter(|(_, n, _)| n == "README" || n == "text")
                .all(|(_, _, sel)| *sel == Some(false)),
            "{mode:?}: and the others say they are not: {all:?}"
        );
    }
}

#[test]
fn the_keyboard_selects_rows_and_the_reader_hears_it() {
    let (mut tree, view, selection) = table(TableSelectionMode::MultiRow);
    click_text(&mut tree, "Cargo.toml");

    key(&mut tree, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(
        selection.selected_indices(),
        vec![0, 1],
        "Shift+Down extends"
    );
    assert_eq!(
        reader_cell(&mut tree, view),
        (Role::GridCell, "README".to_string(), Some(true))
    );

    key(&mut tree, Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![0, 1],
        "Ctrl+Down moves without selecting"
    );
    assert_eq!(
        reader_cell(&mut tree, view),
        (Role::GridCell, "build.rs".to_string(), Some(false))
    );
}

#[test]
fn a_table_with_no_selection_keeps_plain_cells() {
    let (mut tree, view, _) = table(TableSelectionMode::None);
    assert_eq!(view_role(&mut tree, view), Role::Table);
    let all = cells(&mut tree);
    assert!(!all.is_empty(), "the body cells are read");
    assert!(
        all.iter().all(|(role, _, _)| *role == Role::Cell),
        "{all:?}"
    );
}
