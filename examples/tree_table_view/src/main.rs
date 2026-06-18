// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `tree_table` — hierarchical `TreeTableView` showcase.
//!
//! Run with: `cargo run -p tree-table`
//!
//! Demonstrates:
//! - Mock filesystem in a `TreeModel<FsNode>` projected through
//!   `SortFilterTreeModel<FsNode>` with `TreeFilterMode::KeepAncestors`
//!   so a filter on a deep node still shows its ancestor path.
//! - Three columns: name (Flex, sortable, filterable, **tree column**
//!   with twist + indent + multi-line description), size (Fixed,
//!   sortable, alignment Trailing), kind (Flex, sortable + filterable,
//!   alignment Center).
//! - **Variable row heights** via `auto_row_height(26.0)`: rows with a
//!   description measure taller; expand/collapse and sort keep the
//!   measured heights above the change (divergence-driven
//!   invalidation), so the view doesn't jump.
//! - `Role::TreeGrid` accessibility with per-row level + expanded.
//! - ArrowLeft / ArrowRight on the tree column collapse / expand.

use bastyde::data::{
    SelectionMode, SelectionModel, SortDirection, SortFilterTreeModel, TreeFilterMode, TreeModel,
};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, CellContext, Column, ColumnWidth, Expand, GridLines, HStack, Spacer,
    TableAlignment as Alignment, TableSelectionMode, TextWidget, Toolbar, TreeTableView, VStack,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    Toolbar::new().child(HStack::new().child(Spacer::new()).child(
        Button::new(lit!("Toggle Dark Mode")).on_activate_fn(move |ctx| {
            let next = !is_dark.get();
            is_dark.set(next);
            ctx.set_theme(if next {
                bastyde::presets::intui::dark()
            } else {
                bastyde::presets::intui::light()
            });
        }),
    ))
}

#[derive(Clone, Debug)]
struct FsNode {
    name: String,
    size: u64,
    kind: &'static str,
    /// Optional multi-line description — rows carrying one measure
    /// taller under `auto_row_height`.
    desc: &'static str,
}

impl FsNode {
    fn folder(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 0,
            kind: "folder",
            desc: "",
        }
    }
    fn file(name: impl Into<String>, size: u64, kind: &'static str) -> Self {
        Self {
            name: name.into(),
            size,
            kind,
            desc: "",
        }
    }
    fn described(mut self, desc: &'static str) -> Self {
        self.desc = desc;
        self
    }
}

fn build_tree() -> TreeModel<FsNode> {
    let t = TreeModel::new();
    let docs = t.insert_root(0, FsNode::folder("docs"));
    t.insert_child(
        docs,
        0,
        FsNode::file("README.md", 4_321, "markdown")
            .described("Project overview.\nStart here before anything else."),
    );
    t.insert_child(docs, 1, FsNode::file("guide.md", 12_876, "markdown"));
    let plans = t.insert_child(docs, 2, FsNode::folder("plans"));
    t.insert_child(
        plans,
        0,
        FsNode::file("phase-7.md", 7_654, "markdown")
            .described("Variable row heights.\nOffsets + measurement + anchoring.\nShipped."),
    );
    t.insert_child(plans, 1, FsNode::file("phase-8.md", 5_432, "markdown"));

    let src = t.insert_root(1, FsNode::folder("src"));
    t.insert_child(
        src,
        0,
        FsNode::file("main.rs", 1_024, "rust").described("Binary entry point."),
    );
    t.insert_child(src, 1, FsNode::file("lib.rs", 2_048, "rust"));
    let util = t.insert_child(src, 2, FsNode::folder("util"));
    t.insert_child(
        util,
        0,
        FsNode::file("hash.rs", 512, "rust").described("FNV-1a.\nNo external deps."),
    );
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

    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(WindowConfig::new().title("TreeTableView").size(900, 580).root(
            move |tree, _| {
                let proxy_for_table = proxy.clone();
                let table = TreeTableView::from_projection(proxy_for_table.clone())
                    .add_column(
                        Column::<FsNode>::new("name", lit!("Name"), |row, _: &CellContext| {
                            // Name plus the optional multi-line
                            // description — the row's measured height
                            // follows this cell.
                            let mut v = VStack::new()
                                .spacing(1.0)
                                .child(TextWidget::new(lit!(row.name.clone())));
                            for line in row.desc.lines() {
                                v = v.child(TextWidget::new(lit!(line)).color(TextRole::Secondary));
                            }
                            Box::new(v)
                        })
                        .width(ColumnWidth::Flex(3.0))
                        .sortable(true)
                        .filterable(true),
                    )
                    .add_column(
                        Column::<FsNode>::new("size", lit!("Size"), |row, _: &CellContext| {
                            let s = if row.size == 0 {
                                String::new()
                            } else {
                                format!("{} B", row.size)
                            };
                            Box::new(TextWidget::new(lit!(s)))
                        })
                        .width(ColumnWidth::Fixed(96.0))
                        .sortable(true)
                        .alignment(Alignment::Trailing),
                    )
                    .add_column(
                        Column::<FsNode>::new("kind", lit!("Kind"), |row, _: &CellContext| {
                            Box::new(TextWidget::new(lit!(row.kind)))
                        })
                        .width(ColumnWidth::Flex(1.0))
                        .sortable(true)
                        .filterable(true)
                        .alignment(Alignment::Center),
                    )
                    // Rows measure to their tallest cell (described
                    // files are 2–4 lines); 26 px is just the estimate.
                    .auto_row_height(26.0)
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
