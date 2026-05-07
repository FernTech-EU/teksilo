//! `tree_table` — hierarchical `TreeTable` showcase.
//!
//! Run with: `cargo run -p tree-table`
//!
//! Demonstrates:
//! - Mock filesystem in a `TreeModel<FsNode>` projected through
//!   `SortFilterTreeModel<FsNode>` with `TreeFilterMode::KeepAncestors`
//!   so a filter on a deep node still shows its ancestor path.
//! - Four columns: name (Flex, sortable, filterable, **tree column**
//!   with twist + indent), size (Fixed, sortable, alignment Trailing),
//!   modified (Fixed, alignment Trailing), kind (Flex, sortable +
//!   filterable, alignment Center).
//! - `Role::TreeGrid` accessibility with per-row level + expanded.
//! - ArrowLeft / ArrowRight on the tree column collapse / expand.

use fern_ui::data::{
    SelectionMode, SelectionModel, SortDirection, SortFilterTreeModel, TreeFilterMode, TreeModel,
};
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, CellContext, Column, ColumnWidth, Expand, GridLines, HStack, Spacer,
    TableAlignment as Alignment, TableSelectionMode, TextWidget, Toolbar, TreeTable, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new_literal("Toggle Dark Mode").on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                Theme::dark_default()
            } else {
                Theme::light_default()
            });
        }),
    ))
}

#[derive(Clone, Debug)]
struct FsNode {
    name: String,
    size: u64,
    kind: &'static str,
}

impl FsNode {
    fn folder(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 0,
            kind: "folder",
        }
    }
    fn file(name: impl Into<String>, size: u64, kind: &'static str) -> Self {
        Self {
            name: name.into(),
            size,
            kind,
        }
    }
}

fn build_tree() -> TreeModel<FsNode> {
    let t = TreeModel::new();
    let docs = t.insert_root(0, FsNode::folder("docs"));
    t.insert_child(docs, 0, FsNode::file("README.md", 4_321, "markdown"));
    t.insert_child(docs, 1, FsNode::file("guide.md", 12_876, "markdown"));
    let plans = t.insert_child(docs, 2, FsNode::folder("plans"));
    t.insert_child(plans, 0, FsNode::file("phase-7.md", 7_654, "markdown"));
    t.insert_child(plans, 1, FsNode::file("phase-8.md", 5_432, "markdown"));

    let src = t.insert_root(1, FsNode::folder("src"));
    t.insert_child(src, 0, FsNode::file("main.rs", 1_024, "rust"));
    t.insert_child(src, 1, FsNode::file("lib.rs", 2_048, "rust"));
    let util = t.insert_child(src, 2, FsNode::folder("util"));
    t.insert_child(util, 0, FsNode::file("hash.rs", 512, "rust"));
    t.insert_child(util, 1, FsNode::file("parse.rs", 1_536, "rust"));

    t.insert_root(2, FsNode::file("Cargo.toml", 768, "toml"));
    t.insert_root(3, FsNode::file("README", 256, "text"));
    t
}

fn main() {
    let model = build_tree();
    let proxy = SortFilterTreeModel::new(model)
        .filter_mode(TreeFilterMode::KeepAncestors)
        .with_comparator("name", |a: &FsNode, b: &FsNode| a.name.cmp(&b.name))
        .with_comparator("size", |a, b| a.size.cmp(&b.size))
        .with_comparator("kind", |a, b| a.kind.cmp(b.kind))
        .with_predicate("name", |t| {
            let needle = t.to_lowercase();
            Box::new(move |n: &FsNode| n.name.to_lowercase().contains(&needle))
        })
        .with_predicate("kind", |t| {
            let needle = t.to_lowercase();
            Box::new(move |n: &FsNode| n.kind.to_lowercase().contains(&needle))
        });

    let selection = SelectionModel::new(SelectionMode::Multi);

    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(Theme::light_default())
        .initial_window(WindowConfig::new().title("TreeTable").size(900, 580).root(
            move |tree, _| {
                let proxy_for_table = proxy.clone();
                let table = TreeTable::from_projection(proxy_for_table.clone())
                    .add_column(
                        Column::<FsNode>::new("name", "Name", |row, _: &CellContext| {
                            Box::new(TextWidget::new_literal(row.name.clone()))
                        })
                        .width(ColumnWidth::Flex(3.0))
                        .sortable(true)
                        .filterable(true),
                    )
                    .add_column(
                        Column::<FsNode>::new("size", "Size", |row, _: &CellContext| {
                            let s = if row.size == 0 {
                                String::new()
                            } else {
                                format!("{} B", row.size)
                            };
                            Box::new(TextWidget::new_literal(s))
                        })
                        .width(ColumnWidth::Fixed(96.0))
                        .sortable(true)
                        .alignment(Alignment::Trailing),
                    )
                    .add_column(
                        Column::<FsNode>::new("kind", "Kind", |row, _: &CellContext| {
                            Box::new(TextWidget::new_literal(row.kind))
                        })
                        .width(ColumnWidth::Flex(1.0))
                        .sortable(true)
                        .filterable(true)
                        .alignment(Alignment::Center),
                    )
                    .row_height(26.0)
                    .alternating_rows(true)
                    .grid_lines(GridLines::Horizontal)
                    .selection_mode(TableSelectionMode::MultiRow)
                    .selection(selection.clone())
                    .tree_column("name");

                // Wire the proxy to the table's signals.
                proxy_for_table.bind_sort_signal(table.sort_signal().clone());
                proxy_for_table.bind_filters_signal(table.filters_signal().clone());

                // Default sort by name ascending.
                table.set_sort(Some("name"), SortDirection::Ascending);

                let table_id = tree.add(table);
                tree.add(
                    VStack::new()
                        .child(dark_mode_toolbar())
                        .child(Expand::new().child_id(table_id)),
                )
            },
        ))
        .run();
}
