// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `tree-table-view` example's table as a screen reader gets it, read
//! through `accesskit_consumer` as every platform adapter reads it.
//!
//! On Windows NVDA read each row as "data item, 2 of 5" and never its text:
//! a cell's text is in its `Role::Label` children, and neither the consumer
//! nor UIA names a cell from them. And a selected row said nothing, because
//! UIA exposes a selection state on a `Role::GridCell` and never on a
//! `Role::Row` or `Role::Cell`. The keyboard selected all along (Shift+Down
//! extends, Ctrl+Space takes a selected row out, as before these fixes); it is pinned here so
//! the reading and the selecting are checked together. The cells are named
//! from their text on Windows and Linux only; on macOS VoiceOver reads them
//! from their children, and `heard` checks whichever shape the host gets.

use accesskit_consumer::{NodeRef, Tree, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{NodeId, Role};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{
    SelectionMode, SelectionModel, SortDirection, SortFilterTreeModel, TreeFilterMode, TreeModel,
};
use teksilo_i18n::lit;
use teksilo_widgets::table_view::TableSelectionMode;
use teksilo_widgets::*;

#[derive(Clone, Debug)]
struct FsNode {
    name: String,
    kind: &'static str,
    desc: &'static str,
    entries: usize,
}

impl FsNode {
    fn folder(name: &str, entries: usize) -> Self {
        Self {
            name: name.into(),
            kind: "folder",
            desc: "",
            entries,
        }
    }
    fn file(name: &str, kind: &'static str) -> Self {
        Self {
            name: name.into(),
            kind,
            desc: "",
            entries: 0,
        }
    }
    fn described(mut self, desc: &'static str) -> Self {
        self.desc = desc;
        self
    }
    fn is_folder(&self) -> bool {
        self.kind == "folder"
    }
}

/// The example's tree, cut down: a folder with a described file, a folder of
/// icons, and two root files.
fn model() -> TreeModel<FsNode> {
    let t = TreeModel::new();
    let docs = t.insert_root(0, FsNode::folder("docs", 2));
    t.insert_child(
        docs,
        0,
        FsNode::file("README.md", "markdown")
            .described("Project overview.\nStart here before anything else."),
    );
    t.insert_child(docs, 1, FsNode::file("guide.md", "markdown"));
    let assets = t.insert_root(1, FsNode::folder("assets", 3));
    for i in 0..3 {
        t.insert_child(
            assets,
            i,
            FsNode::file(&format!("icon-{i:02}.png"), "image"),
        );
    }
    t.insert_root(2, FsNode::file("Cargo.toml", "toml"));
    t.insert_root(3, FsNode::file("README", "text"));
    t
}

const SIZE: SizeProposal = SizeProposal {
    width: Some(1000.0),
    height: Some(360.0),
};

/// The example's view: a tree column whose cell holds the name and the
/// description lines, a kind column, folders as full-width bands, multi-row
/// selection.
fn example() -> (WidgetTree, WidgetId, SelectionModel) {
    let proxy = SortFilterTreeModel::new(model())
        .filter_mode(TreeFilterMode::KeepAncestors)
        .with_comparator("name", |a: &FsNode, b: &FsNode| a.name.cmp(&b.name));
    let selection = SelectionModel::new(SelectionMode::Multi);
    let table = TreeTableView::from_projection(proxy.clone())
        .add_column(
            Column::<FsNode>::new("name", lit!("Name"), |row, _: &CellContext| {
                let mut v = VStack::new()
                    .spacing(1.0)
                    .child(TextWidget::new(lit!(row.name.clone())));
                for line in row.desc.lines() {
                    v = v.child(TextWidget::new(lit!(line)));
                }
                Box::new(v)
            })
            .width(ColumnWidth::Flex(3.0))
            .sortable(true),
        )
        .add_column(
            Column::<FsNode>::new("kind", lit!("Kind"), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(row.kind)))
            })
            .width(ColumnWidth::Flex(1.0)),
        )
        .auto_row_height(26.0)
        .selection_mode(TableSelectionMode::MultiRow)
        .selection(selection.clone())
        .tree_column("name")
        .full_width_row(FsNode::is_folder)
        .full_width_row_delegate(|row, _: &CellContext| {
            Box::new(
                HStack::new()
                    .spacing(8.0)
                    .child(TextWidget::new(lit!(row.name.clone())))
                    .child(TextWidget::new(lit!(format!("{} entries", row.entries)))),
            )
        })
        .pinned_ancestors(2);
    proxy.sort_signal(table.sort_signal().clone());
    table.set_sort(Some("name"), SortDirection::Ascending);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let view = tree.add(table);
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

/// What the consumer makes of each cell the reader can reach: role, name
/// (as every adapter computes it), selected state.
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
fn every_cell_says_the_text_it_shows() {
    let (mut tree, view, _) = example();
    {
        let any = tree.widget_as_any(view).expect("the view");
        any.downcast_ref::<TreeTableView<FsNode>>()
            .expect("a tree table")
            .expand_all();
    }
    tree.layout(SIZE);
    let names: Vec<String> = cells(&mut tree).into_iter().map(|(_, n, _)| n).collect();
    for expected in [
        "Cargo.toml",
        "toml",
        "README",
        "text",
        "assets 3 entries",
        "icon-00.png",
        "image",
        "docs 2 entries",
        "README.md Project overview. Start here before anything else.",
        "markdown",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "a cell named {expected:?} in {names:?}"
        );
    }
    assert!(
        names.iter().all(|n| !n.is_empty()),
        "no unnamed cell: {names:?}"
    );
}

#[test]
fn the_cell_the_reader_is_on_says_its_text_and_its_row_selection() {
    let (mut tree, view, selection) = example();
    click_text(&mut tree, "Cargo.toml");
    assert_eq!(selection.selected_indices(), vec![0]);
    // A grid cell, so UIA exposes `SelectionItem.IsSelected` on it.
    assert_eq!(
        reader_cell(&mut tree, view),
        (Role::GridCell, "Cargo.toml".to_string(), Some(true))
    );
    let all = cells(&mut tree);
    assert!(
        all.iter()
            .filter(|(_, n, _)| n == "Cargo.toml" || n == "toml")
            .all(|(role, _, sel)| *role == Role::GridCell && *sel == Some(true)),
        "every cell of the selected row says so: {all:?}"
    );
    assert!(
        all.iter()
            .filter(|(_, n, _)| n == "README" || n == "text")
            .all(|(_, _, sel)| *sel == Some(false)),
        "and the others say they are not: {all:?}"
    );
}

#[test]
fn the_keyboard_selects_rows_and_the_reader_hears_it() {
    let (mut tree, view, selection) = example();
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

    key(&mut tree, Key::Space, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+Space takes the row back out"
    );
    assert_eq!(
        reader_cell(&mut tree, view),
        (Role::GridCell, "README".to_string(), Some(false))
    );

    key(&mut tree, Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+Down moves without selecting"
    );
    assert_eq!(
        reader_cell(&mut tree, view),
        (Role::GridCell, "assets 3 entries".to_string(), Some(false))
    );
}
