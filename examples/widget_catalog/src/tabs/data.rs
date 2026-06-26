// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Data tab — the full data-driven family, all live:
//! Repeater, ListView, StandardListItem, StandardTreeItem, TreeView,
//! TableView, TreeTableView, GridView. The heavyweights are cannibalized
//! from the data_collections / data_grid / tree_table / grid_view
//! examples and shrunk to fit a scrolling catalog tab.

use bastyde::canvas::EdgeInsets;
use bastyde::data::{
    ListModel, SelectionMode, SelectionModel, SortDirection, SortFilterListModel,
    SortFilterTreeModel, TreeFilterMode, TreeModel,
};
use bastyde::prelude::*;
use bastyde::widgets::{
    CellContext, Center, Column, ColumnWidth, Divider, FixedSize, GridLines, GridSizing, GridView,
    ListView, RectWidget, Repeater, StandardListItem, StandardTreeItem, TableAlignment,
    TableSelectionMode, TableView, TextWidget, TreeTableView, TreeView, VStack, ZStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_data_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_data_refs())
}

// ── Simple flat models (Repeater / ListView) ──────────────────────────

fn make_repeater_model() -> ListModel<String> {
    ListModel::from_vec(vec![
        tr!(dat_fruit_apple()).resolve_now(),
        tr!(dat_fruit_banana()).resolve_now(),
        tr!(dat_fruit_cherry()).resolve_now(),
        tr!(dat_fruit_date()).resolve_now(),
    ])
}

fn make_list_model() -> ListModel<String> {
    ListModel::from_vec(
        (1..=10)
            .map(|i| format!("{} {}", tr!(dat_list_row()).resolve_now(), i))
            .collect(),
    )
}

fn repeater_widget() -> Repeater<String> {
    Repeater::new(make_repeater_model(), |_idx, item: &String| {
        Box::new(
            TextWidget::new(lit!(format!("• {item}")))
                .style(TextStyleRole::Body)
                .color(TextRole::Primary),
        )
    })
    .spacing(2.0)
}

fn list_view_widget() -> ListView<String> {
    // Wire a selection model + reflect `selected` in the row, like the
    // TreeView below — otherwise arrow-key navigation moves the focus index
    // but nothing highlights (only the container focus ring shows).
    ListView::new(make_list_model(), |_idx, item: &String, selected| {
        Box::new(StandardListItem::new(lit!(item.clone())).selected(selected))
    })
    .selection(SelectionModel::new(SelectionMode::Single))
}

// ── TreeView (data_collections) ───────────────────────────────────────

fn make_tree_model() -> TreeModel<String> {
    let t = TreeModel::new();
    let docs = t.insert_root(0, "Documents".into());
    let proj = t.insert_child(docs, 0, "Projects".into());
    t.insert_child(proj, 0, "Bastyde".into());
    t.insert_child(proj, 1, "Skribisto".into());
    t.insert_child(docs, 1, "Notes".into());
    let pics = t.insert_root(1, "Pictures".into());
    t.insert_child(pics, 0, "Vacation".into());
    t.insert_root(2, "Downloads".into());
    t
}

fn tree_view_widget() -> impl Widget + 'static {
    TreeView::new_with_context(make_tree_model(), move |item, entry, selected, ctx| {
        Box::new(
            StandardTreeItem::new(lit!(item.clone()))
                .from_entry(entry)
                .selected(selected)
                .on_toggle_rc(ctx.toggle_callback()),
        )
    })
    .item_height(28.0)
    .selection(SelectionModel::new(SelectionMode::Single))
}

// ── TableView (data_grid) ─────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Person {
    name: String,
    role: &'static str,
    salary: u32,
}

fn make_people() -> Vec<Person> {
    let roles = ["Admin", "Editor", "Viewer"];
    let names = [
        "Avery", "Blake", "Casey", "Drew", "Elliot", "Finn", "Gray", "Harper", "Indigo", "Jordan",
        "Kai", "Logan",
    ];
    names
        .iter()
        .enumerate()
        .map(|(i, n)| Person {
            name: (*n).to_string(),
            role: roles[i % roles.len()],
            salary: 45_000 + (i as u32 * 3_137) % 60_000,
        })
        .collect()
}

fn table_view_widget() -> impl Widget + 'static {
    let proxy = SortFilterListModel::new(ListModel::from_vec(make_people()))
        .with_comparator("name", |a: &Person, b: &Person| a.name.cmp(&b.name))
        .with_comparator("role", |a, b| a.role.cmp(b.role))
        .with_comparator("salary", |a, b| a.salary.cmp(&b.salary))
        .with_predicate("name", |t| {
            let needle = t.to_lowercase();
            Box::new(move |row: &Person| row.name.to_lowercase().contains(&needle))
        });
    let table = TableView::from_source(proxy.clone())
        .add_column(
            Column::<Person>::new("name", lit!("Name"), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(row.name.clone())))
            })
            .width(ColumnWidth::Flex(2.0))
            .sortable(true)
            .filterable(true),
        )
        .add_column(
            Column::<Person>::new("role", lit!("Role"), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(row.role)))
            })
            .width(ColumnWidth::Flex(1.0))
            .sortable(true)
            .alignment(TableAlignment::Center),
        )
        .add_column(
            Column::<Person>::new("salary", lit!("Salary"), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(format!("${}", row.salary))))
            })
            .width(ColumnWidth::Fixed(96.0))
            .sortable(true)
            .alignment(TableAlignment::Trailing),
        )
        .row_height(28.0)
        .alternating_rows(true)
        .grid_lines(GridLines::Horizontal)
        .selection_mode(TableSelectionMode::MultiRow)
        .selection(SelectionModel::new(SelectionMode::Multi));
    proxy.bind_sort_signal(table.sort_signal().clone());
    proxy.bind_filters_signal(table.filters_signal().clone());
    table.set_sort(Some("name"), SortDirection::Ascending);
    table
}

// ── TreeTableView (tree_table) ────────────────────────────────────────────

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

fn make_fs_tree() -> TreeModel<FsNode> {
    let t = TreeModel::new();
    let docs = t.insert_root(0, FsNode::folder("docs"));
    t.insert_child(docs, 0, FsNode::file("README.md", 4_321, "markdown"));
    let plans = t.insert_child(docs, 1, FsNode::folder("plans"));
    t.insert_child(plans, 0, FsNode::file("phase-7.md", 7_654, "markdown"));
    let src = t.insert_root(1, FsNode::folder("src"));
    t.insert_child(src, 0, FsNode::file("main.rs", 1_024, "rust"));
    t.insert_child(src, 1, FsNode::file("lib.rs", 2_048, "rust"));
    t.insert_root(2, FsNode::file("Cargo.toml", 768, "toml"));
    t
}

fn tree_table_widget() -> impl Widget + 'static {
    let proxy = SortFilterTreeModel::new(make_fs_tree())
        .filter_mode(TreeFilterMode::KeepAncestors)
        .with_comparator("name", |a: &FsNode, b: &FsNode| a.name.cmp(&b.name))
        .with_comparator("size", |a, b| a.size.cmp(&b.size));
    let table = TreeTableView::from_projection(proxy.clone())
        .add_column(
            Column::<FsNode>::new("name", lit!("Name"), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(row.name.clone())))
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
            .alignment(TableAlignment::Trailing),
        )
        .add_column(
            Column::<FsNode>::new("kind", lit!("Kind"), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(row.kind)))
            })
            .width(ColumnWidth::Flex(1.0))
            .alignment(TableAlignment::Center),
        )
        .row_height(26.0)
        .alternating_rows(true)
        .grid_lines(GridLines::Horizontal)
        .selection_mode(TableSelectionMode::MultiRow)
        .selection(SelectionModel::new(SelectionMode::Multi))
        .tree_column("name");
    proxy.bind_sort_signal(table.sort_signal().clone());
    proxy.bind_filters_signal(table.filters_signal().clone());
    table.set_sort(Some("name"), SortDirection::Ascending);
    table
}

// ── GridView (grid_view) ──────────────────────────────────────────────

fn grid_view_widget() -> impl Widget + 'static {
    let words = [
        "Sunset", "Harbor", "Trail", "Picnic", "Summit", "Garden", "Market", "Bridge", "Cabin",
        "Meadow", "Canyon", "Festival", "Skyline", "Lantern", "Orchard", "Pier",
    ];
    let model = ListModel::from_vec(words.iter().map(|w| (*w).to_string()).collect());
    GridView::new(model, move |tc| {
        let bg = if tc.is_selected {
            SurfaceRole::AccentSubtle
        } else {
            SurfaceRole::Raised
        };
        Box::new(ZStack::new().child(RectWidget::new().background(bg)).child(
            Center::new().child(TextWidget::new(lit!(tc.item.clone())).color(TextRole::Primary)),
        )) as Box<dyn Widget>
    })
    .sizing(GridSizing::Adaptive {
        min_width: 120.0,
        max_width: Some(180.0),
        height: 90.0,
    })
    .spacing(10.0)
    .content_inset(EdgeInsets::uniform(12.0))
    .selection(SelectionModel::new(SelectionMode::Multi))
    .marquee_selection(true)
    .a11y_label("Tiles")
}

// ── Sizing helper ─────────────────────────────────────────────────────

fn sized(w: f32, h: f32, body: impl Widget + 'static) -> FixedSize {
    FixedSize::new().bind_width(w).bind_height(h).child(body)
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let repeater = section(ctx, lit!("Repeater"), repeater_widget());
    let list_view = section(
        ctx,
        lit!("ListView"),
        sized(280.0, 180.0, list_view_widget()),
    );
    let standard_list_item = section(
        ctx,
        tr!(dat_standard_list_item_standalone()),
        VStack::new()
            .spacing(2.0)
            .child(StandardListItem::new(tr!(data_first_item())))
            .child(StandardListItem::new(tr!(data_second_item())))
            .child(StandardListItem::new(tr!(data_third_item()))),
    );
    let standard_tree_item = section(
        ctx,
        tr!(dat_standard_tree_item_standalone()),
        VStack::new()
            .spacing(2.0)
            .child(StandardTreeItem::new(tr!(dat_tree_root())).depth(0))
            .child(StandardTreeItem::new(tr!(data_child_a())).depth(1))
            .child(StandardTreeItem::new(tr!(data_child_b())).depth(1))
            .child(StandardTreeItem::new(tr!(dat_tree_grandchild())).depth(2)),
    );
    let tree_view = section(
        ctx,
        lit!("TreeView"),
        sized(320.0, 200.0, tree_view_widget()),
    );
    let table_view = section(
        ctx,
        lit!("TableView"),
        sized(540.0, 200.0, table_view_widget()),
    );
    let tree_table = section(
        ctx,
        lit!("TreeTableView"),
        sized(540.0, 200.0, tree_table_widget()),
    );
    let grid_view = section(
        ctx,
        lit!("GridView"),
        sized(540.0, 230.0, grid_view_widget()),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(repeater)
            .add_child(list_view)
            .add_child(standard_list_item)
            .add_child(standard_tree_item)
            .add_child(tree_view)
            .add_child(table_view)
            .add_child(tree_table)
            .add_child(grid_view),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // Every data widget takes a closure delegate (and the table family
    // also binds sort/filter signals) — bati! property syntax can't
    // express those, so pre-build each and splice via `#{ id }`.
    let repeater_id = ctx.add(repeater_widget());
    let list_view_id = ctx.add(sized(280.0, 180.0, list_view_widget()));
    let tree_view_id = ctx.add(sized(320.0, 200.0, tree_view_widget()));
    let table_view_id = ctx.add(sized(540.0, 200.0, table_view_widget()));
    let tree_table_id = ctx.add(sized(540.0, 200.0, tree_table_widget()));
    let grid_view_id = ctx.add(sized(540.0, 230.0, grid_view_widget()));

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_data_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_data_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("Repeater")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ repeater_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("ListView")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ list_view_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dat_standard_list_item_standalone())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 2.0
                    StandardListItem::new(tr!(data_first_item()))
                    StandardListItem::new(tr!(data_second_item()))
                    StandardListItem::new(tr!(data_third_item()))
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(dat_standard_tree_item_standalone())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 2.0
                    StandardTreeItem::new(tr!(dat_tree_root())) {
                        depth: 0
                    }
                    StandardTreeItem::new(tr!(data_child_a())) {
                        depth: 1
                    }
                    StandardTreeItem::new(tr!(data_child_b())) {
                        depth: 1
                    }
                    StandardTreeItem::new(tr!(dat_tree_grandchild())) {
                        depth: 2
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TreeView")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ tree_view_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TableView")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ table_view_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TreeTableView")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ tree_table_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("GridView")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ grid_view_id }
            }
        }
    )
}
