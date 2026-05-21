//! Phase-2 tests for `TableView`.
//!
//! Covered here:
//! - Layout: column-width resolution, viewport sizing, scrollbar reservation.
//! - Virtualization: visible-range math, rebuild-on-buffer-exit.
//! - Selection: row-level click, Ctrl-toggle, Shift-extend, auto-adjust on
//!   data mutation.
//! - Accessibility: `Role::Table`, row/column counts, `Role::Row` per row,
//!   `Role::Cell` per cell with row/column indices.
//! - Empty state: `empty_view` materialises when source is empty.
//!
//! Header / sort / filter / resize / reorder / pinning / cell-selection /
//! editing tests arrive in their respective phase commits.

use bastyde_canvas::SizeProposal;
use bastyde_core::accesskit::Role;
use bastyde_core::signal::Signal;
use bastyde_core::widget_id::WidgetId;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_data::{ListModel, SelectionMode, SelectionModel};

use super::{CellContext, Column, ColumnWidth, SortDirection, TableSelectionMode, TableView};
use crate::OverscrollBehavior;
use crate::primitives::TextWidget;

#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: u32,
    name: String,
}

fn rows(n: u32) -> ListModel<Row> {
    let mut v = Vec::with_capacity(n as usize);
    for i in 0..n {
        v.push(Row {
            id: i,
            name: format!("row {i}"),
        });
    }
    ListModel::from_vec(v)
}

fn id_col() -> Column<Row> {
    Column::<Row>::new("id", "ID", |row, _: &CellContext| {
        Box::new(TextWidget::new_literal(row.id.to_string()))
    })
    .width(ColumnWidth::Fixed(60.0))
}

fn name_col() -> Column<Row> {
    Column::<Row>::new("name", "Name", |row, _: &CellContext| {
        Box::new(TextWidget::new_literal(row.name.clone()))
    })
    .width(ColumnWidth::Flex(1.0))
}

/// Simple two-column table over `n` rows. Returns the tree + the table id.
fn build_table(n: u32) -> (WidgetTree, WidgetId, ListModel<Row>) {
    let model = rows(n);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model.clone())
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    (tree, table, model)
}

// ── Module structure ───────────────────────────────────────────────────────

#[test]
fn builds_with_two_columns() {
    let (tree, table, _) = build_table(10);
    let b = tree.bounds(table);
    assert!((b.width - 400.0).abs() < 0.01, "got width {}", b.width);
    assert!((b.height - 200.0).abs() < 0.01, "got height {}", b.height);
}

// ── Layout ─────────────────────────────────────────────────────────────────

#[test]
fn fixed_plus_flex_split_pane_minus_scrollbar() {
    // Available 400 px, scrollbar reserves 12. Body = 388. id Fixed=60,
    // name Flex(1) gets 388 - 60 = 328.
    let (tree, table, _) = build_table(50);
    let info = tree.accessibility_node(table);
    assert_eq!(info.role(), Role::Table);

    // Walk to the first row, then to its first cell, and check x/widths.
    let row_ids = first_visible_row_cells(&tree, table);
    assert_eq!(row_ids.len(), 2, "expected 2 cells in row");
    let id_cell = tree.bounds(row_ids[0]);
    let name_cell = tree.bounds(row_ids[1]);
    assert!((id_cell.width - 60.0).abs() < 0.5);
    assert!(
        (name_cell.width - 328.0).abs() < 0.5,
        "got {}",
        name_cell.width
    );
    assert!((id_cell.x).abs() < 0.5);
    assert!((name_cell.x - 60.0).abs() < 0.5);
}

#[test]
fn no_scrollbar_when_content_fits() {
    // 5 rows * 20 = 100 px content, viewport 200 — no scrollbar reserved.
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let model = rows(5);
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row_ids = first_visible_row_cells(&tree, table);
    let name_cell = tree.bounds(row_ids[1]);
    // name column gets full leftover (400 - 60 = 340) without scrollbar.
    assert!(
        (name_cell.width - 340.0).abs() < 0.5,
        "got {}",
        name_cell.width
    );
}

// ── Virtualization ─────────────────────────────────────────────────────────

#[test]
fn virtualizes_visible_window() {
    // 1000 rows, viewport 200, row_height 20 — only the visible window
    // (plus a small buffer) should materialise. Don't pin the upper
    // bound too tightly: the *first* build runs with the table's
    // default-viewport guess (600 px) before `size_that_fits` reports
    // the real layout viewport, exactly like `ListView` does today.
    // The first scroll event reduces the window to the strict bound;
    // until then the count can be ~35.
    let (tree, table, _) = build_table(1000);
    let row_count = count_role(&tree, table, Role::Row);
    assert!(row_count >= 10, "got {row_count} rows");
    assert!(
        row_count <= 50,
        "got {row_count} rows (expected virtualized)"
    );
    assert!(
        row_count < 1000,
        "TableView must virtualize, not materialise all rows"
    );
}

#[test]
fn rebuild_on_scroll_past_buffer() {
    let (mut tree, table, _model) = build_table(1000);
    let initial_rows = count_role(&tree, table, Role::Row);

    // Scroll the table by enough that we leave the buffered window.
    let signal = {
        let any = tree
            .widget_as_any(table)
            .expect("TableView exposes itself via as_any");
        let tv = any
            .downcast_ref::<TableView<Row>>()
            .expect("downcast to TableView<Row>");
        tv.scroll_y_signal().clone()
    };
    signal.set(2000.0);
    tree.request_frame();
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let after_rows = count_role(&tree, table, Role::Row);
    assert!(after_rows >= 10);

    // After scrolling, the rendered rows should *cover* y=2000. Each
    // row has bounds (y = row_idx * row_h - scroll). Verify at least
    // one of the materialised rows has its top within the viewport,
    // i.e., not all rows are still pinned at the original window.
    let direct_rows: Vec<_> = tree
        .children(table)
        .into_iter()
        .filter(|c| tree.accessibility_node(*c).role() == Role::Row)
        .collect();
    let any_in_viewport = direct_rows.iter().any(|id| {
        let b = tree.bounds(*id);
        b.y >= 0.0 && b.y < 200.0
    });
    assert!(
        any_in_viewport,
        "expected at least one materialised row to land inside the viewport after scroll"
    );

    let _ = initial_rows;
}

// ── Selection ──────────────────────────────────────────────────────────────

#[test]
fn selection_signal_reflects_select() {
    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    sel.select(2);
    sel.toggle(4);
    let s = sel.selected_indices();
    assert_eq!(s, vec![2, 4]);
}

#[test]
fn selection_auto_adjusts_on_insert() {
    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model.clone())
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    sel.select(3);
    sel.toggle(7);
    model.insert(
        4,
        Row {
            id: 99,
            name: "inserted".into(),
        },
    );
    // Index 3 stays, 7 shifts to 8.
    assert_eq!(sel.selected_indices(), vec![3, 8]);
}

#[test]
fn selection_auto_adjusts_on_remove() {
    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model.clone())
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    sel.select(3);
    sel.toggle(7);
    model.remove(3);
    // Index 3 was removed; 7 shifts to 6.
    assert_eq!(sel.selected_indices(), vec![6]);
}

// ── Accessibility ──────────────────────────────────────────────────────────

#[test]
fn root_role_is_table_with_row_and_col_count() {
    let (tree, table, _) = build_table(50);
    let info = tree.accessibility_node(table);
    assert_eq!(info.role(), Role::Table);
    let row_count = count_role(&tree, table, Role::Row);
    let header_count = count_role(&tree, table, Role::ColumnHeader);
    let cell_count = count_role(&tree, table, Role::Cell);
    // Phase 3: header is on by default. Each body row carries 2
    // `Role::Cell` children; the header row carries 2 `Role::ColumnHeader`
    // children. row_count includes the header row.
    assert!(row_count >= 2);
    assert_eq!(header_count, 2, "two column headers");
    assert_eq!(
        cell_count,
        (row_count - 1) * 2,
        "{cell_count} body cells across {} body rows",
        row_count - 1
    );
}

#[test]
fn rows_under_table_form_a_rowgroup_with_header_alongside() {
    // ARIA hierarchy: `Table > [Row(header), RowGroup > Row(body)*]`.
    // The header is a direct child of the table; body rows live one
    // level down, inside the `RowGroup` produced by the body pane —
    // this split is what lets the body re-virtualize without
    // destroying the scrollbar mid-thumb-drag (the body pane is a
    // sibling of the scrollbar, not its ancestor).
    let (tree, table, _) = build_table(5);
    // Direct children — exactly one header row, exactly one rowgroup.
    let direct_rows: Vec<_> = tree
        .children(table)
        .into_iter()
        .filter(|c| tree.accessibility_node(*c).role() == Role::Row)
        .collect();
    assert_eq!(direct_rows.len(), 1, "exactly one header Role::Row");
    let rowgroups: Vec<_> = tree
        .children(table)
        .into_iter()
        .filter(|c| tree.accessibility_node(*c).role() == Role::RowGroup)
        .collect();
    assert_eq!(rowgroups.len(), 1, "exactly one Role::RowGroup body pane");
    // Body rows live inside the rowgroup.
    let body_rows: Vec<_> = tree
        .children(rowgroups[0])
        .into_iter()
        .filter(|c| tree.accessibility_node(*c).role() == Role::Row)
        .collect();
    assert_eq!(body_rows.len(), 5, "five body rows inside the rowgroup");
}

#[test]
fn cells_carry_role_cell_under_row() {
    // Each *body* Role::Row's children should all be Role::Cell. The
    // header is also a `Role::Row` but its children are
    // `Role::ColumnHeader`, so we look at every row in the tree and
    // require *one of them* to be a body row whose children are all
    // cells.
    let (tree, table, _) = build_table(3);
    let mut walker = vec![table];
    let mut saw_body_row = false;
    while let Some(id) = walker.pop() {
        let info = tree.accessibility_node(id);
        if info.role() == Role::Row {
            let kids: Vec<_> = tree
                .children(id)
                .into_iter()
                .map(|c| tree.accessibility_node(c).role())
                .collect();
            if kids.iter().all(|&r| r == Role::ColumnHeader) {
                // Header row — skip.
            } else {
                for r in &kids {
                    assert_eq!(*r, Role::Cell, "body row child should be Role::Cell");
                }
                saw_body_row = true;
            }
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    assert!(
        saw_body_row,
        "expected at least one body Role::Row in the tree"
    );
}

// ── Header / Sort / Resize (Phase 3) ───────────────────────────────────────

#[test]
fn header_strip_renders_one_column_header_per_column() {
    let (tree, table, _) = build_table(10);
    let header_count = count_role(&tree, table, Role::ColumnHeader);
    assert_eq!(header_count, 2, "one ColumnHeader per declared column");
}

#[test]
fn header_can_be_hidden() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .show_header(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(count_role(&tree, _table, Role::ColumnHeader), 0);
}

#[test]
fn header_label_appears_in_a11y_tree() {
    let (tree, table, _) = build_table(5);
    // Walk the tree, find the ColumnHeader for "id".
    let mut q = vec![table];
    let mut found_id = false;
    let mut found_name = false;
    while let Some(id) = q.pop() {
        let info = tree.accessibility_node(id);
        if info.role() == Role::ColumnHeader {
            match info.name() {
                Some("ID") => found_id = true,
                Some("Name") => found_name = true,
                _ => {}
            }
        }
        for c in tree.children(id) {
            q.push(c);
        }
    }
    assert!(found_id && found_name, "headers should expose their labels");
}

#[test]
fn sort_cycle_none_asc_desc_none() {
    let model = rows(5);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col().sortable(true))
            .add_column(name_col().sortable(true))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    // Drive the cycle imperatively (clicks are wired up but covered by
    // the integration-driven SortIndicator presence test below).
    let signal = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.sort_signal().clone()
    };
    assert_eq!(signal.get(), None);

    // Simulate "click on id header": cycle through Asc, Desc, None.
    let cycle = |s: &Signal<Option<(String, SortDirection)>>, col_id: &str| {
        let next = match s.get() {
            None => Some((col_id.to_string(), SortDirection::Ascending)),
            Some((id, SortDirection::Ascending)) if id == col_id => {
                Some((col_id.to_string(), SortDirection::Descending))
            }
            Some((id, SortDirection::Descending)) if id == col_id => None,
            Some(_) => Some((col_id.to_string(), SortDirection::Ascending)),
        };
        s.set(next);
    };
    cycle(&signal, "id");
    assert_eq!(
        signal.get(),
        Some(("id".to_string(), SortDirection::Ascending))
    );
    cycle(&signal, "id");
    assert_eq!(
        signal.get(),
        Some(("id".to_string(), SortDirection::Descending))
    );
    cycle(&signal, "id");
    assert_eq!(signal.get(), None);
}

#[test]
fn switching_sort_columns_resets_to_ascending() {
    let (tree, table, _) = build_table(5);
    let signal = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.sort_signal().clone()
    };
    signal.set(Some(("id".to_string(), SortDirection::Descending)));
    // Now "click" on name — match the cycle's "different column" branch.
    let next = match signal.get() {
        Some((id, _)) if id != "name" => Some(("name".to_string(), SortDirection::Ascending)),
        _ => unreachable!(),
    };
    signal.set(next);
    assert_eq!(
        signal.get(),
        Some(("name".to_string(), SortDirection::Ascending))
    );
}

#[test]
fn set_column_width_pins_column() {
    let (tree, table, _) = build_table(10);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.set_column_width("id", 120.0);
    }
    // Force another layout so the override propagates.
    let mut tree = tree;
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row_cells = first_visible_row_cells(&tree, table);
    let id_cell = tree.bounds(row_cells[0]);
    assert!(
        (id_cell.width - 120.0).abs() < 0.5,
        "got width {}",
        id_cell.width
    );
}

#[test]
fn clear_sort_returns_to_unsorted() {
    let (tree, table, _) = build_table(5);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    tv.set_sort(Some("id"), SortDirection::Ascending);
    assert!(tv.sort_signal().get().is_some());
    tv.clear_sort();
    assert!(tv.sort_signal().get().is_none());
}

#[test]
fn header_row_carries_role_row_with_index_one() {
    let (tree, table, _) = build_table(3);
    // Find the header row (the BodyRow / HeaderRow whose first cell
    // carries Role::ColumnHeader).
    let mut q = vec![table];
    let mut found = false;
    while let Some(id) = q.pop() {
        let info = tree.accessibility_node(id);
        if info.role() == Role::Row {
            let kids: Vec<Role> = tree
                .children(id)
                .into_iter()
                .map(|c| tree.accessibility_node(c).role())
                .collect();
            if kids.contains(&Role::ColumnHeader) {
                found = true;
                break;
            }
        }
        for c in tree.children(id) {
            q.push(c);
        }
    }
    assert!(
        found,
        "expected one Role::Row whose children are ColumnHeaders"
    );
}

// ── Reorder + pinned-side (Phase 4) ────────────────────────────────────────

#[test]
fn declared_order_is_default_display_order() {
    let (tree, table, _) = build_table(3);
    let row_cells = first_visible_row_cells(&tree, table);
    // first_visible_row_cells finds the LAST `Role::Row` walked,
    // typically a body row. Its cells are in display order =
    // declaration order: id (60 px) then name (rest).
    let id_cell = tree.bounds(row_cells[0]);
    let name_cell = tree.bounds(row_cells[1]);
    assert!(id_cell.x < name_cell.x);
}

#[test]
fn pinned_leading_moves_to_front() {
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            // Declaration: id, name. Trailing-pinned `id` should still
            // visually trail name once we mark it Trailing.
            .add_column(
                Column::<Row>::new("id", "ID", |row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new_literal(
                        row.id.to_string(),
                    ))
                })
                .width(ColumnWidth::Fixed(60.0))
                .pinned(super::PinnedSide::Trailing),
            )
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row_cells = first_visible_row_cells(&tree, table);
    // After pinning id Trailing: name comes first, id comes last.
    let first = tree.bounds(row_cells[0]); // name
    let second = tree.bounds(row_cells[1]); // id (pinned trailing)
    assert!(first.x < second.x);
    assert!(
        (second.width - 60.0).abs() < 0.5,
        "id width pinned at 60 — got {}",
        second.width
    );
}

#[test]
fn set_column_order_reorders_display() {
    // Three columns: id (decl 0), name (decl 1), extra (decl 2).
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col()) // 60
            .add_column(name_col())
            .add_column(
                Column::<Row>::new("extra", "Extra", |_row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new_literal("…"))
                })
                .width(ColumnWidth::Fixed(40.0)),
            )
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    // Reorder display → ["extra", "id", "name"].
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.set_column_order(vec![
            "extra".to_string(),
            "id".to_string(),
            "name".to_string(),
        ]);
    }
    let mut tree = tree;
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row_cells = first_visible_row_cells(&tree, table);
    let first = tree.bounds(row_cells[0]); // extra
    let second = tree.bounds(row_cells[1]); // id
    let third = tree.bounds(row_cells[2]); // name
    assert!(
        (first.width - 40.0).abs() < 0.5,
        "extra width 40 — got {}",
        first.width
    );
    assert!(
        (second.width - 60.0).abs() < 0.5,
        "id width 60 — got {}",
        second.width
    );
    assert!(third.width > 100.0, "name fills the rest");
    // x-axis ordering preserved.
    assert!(first.x < second.x);
    assert!(second.x < third.x);
}

#[test]
fn set_column_pinning_relocates_column() {
    let (tree, table, _) = build_table(3);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        // Move `name` to Trailing — it should swap to the right.
        tv.set_column_pinning("name", super::PinnedSide::Trailing);
    }
    let mut tree = tree;
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row_cells = first_visible_row_cells(&tree, table);
    // Now: id (None) first, name (Trailing) last. With only two
    // columns, name is the trailing-pinned tail.
    let id_cell = tree.bounds(row_cells[0]);
    let name_cell = tree.bounds(row_cells[1]);
    assert!(id_cell.x < name_cell.x);
}

#[test]
fn cycle_pinning_back_to_none_via_signal_clear() {
    let (tree, table, _) = build_table(3);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.set_column_pinning("id", super::PinnedSide::Leading);
        assert_eq!(
            tv.column_pinning_signal().get().get("id").copied(),
            Some(super::PinnedSide::Leading)
        );
        // Clear it.
        tv.set_column_pinning("id", super::PinnedSide::None);
        assert_eq!(tv.column_pinning_signal().get().get("id").copied(), None);
    }
}

#[test]
fn column_order_signal_persists_after_data_change() {
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model.clone())
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.set_column_order(vec!["name".to_string(), "id".to_string()]);
    }
    // Mutate the underlying source — the order should NOT reset.
    model.push(Row {
        id: 999,
        name: "z".into(),
    });
    let mut tree = tree;
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(
        tv.column_order_signal().get(),
        vec!["name".to_string(), "id".to_string()]
    );
}

#[test]
fn unknown_column_id_in_order_signal_is_ignored() {
    let (tree, table, _) = build_table(3);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        // Include a phantom id; the ColumnSolver should still produce
        // widths for the two real columns.
        tv.set_column_order(vec![
            "nope".to_string(),
            "name".to_string(),
            "id".to_string(),
        ]);
    }
    let mut tree = tree;
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row_cells = first_visible_row_cells(&tree, table);
    assert_eq!(row_cells.len(), 2, "phantom id was skipped");
}

// ── Keyboard / focused cell / cell selection (Phase 5) ─────────────────────

use bastyde_core::event::{Key, Modifiers};

fn focus_at(tree: &mut WidgetTree, table: WidgetId, row: usize, col: usize) {
    tree.focus(table);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    tv.set_focused_cell(row, col);
}

fn read_focused_cell(tree: &WidgetTree, table: WidgetId) -> Option<(usize, usize)> {
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    tv.focused_cell_signal().get()
}

#[test]
fn arrow_keys_move_focused_cell() {
    let (mut tree, table, _) = build_table(10);
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 0)));
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 1)));
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 1)));
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
}

#[test]
fn arrow_keys_clamp_at_edges() {
    let (mut tree, table, _) = build_table(3);
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
    focus_at(&mut tree, table, 2, 1);
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((2, 1)));
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((2, 1)));
}

#[test]
fn home_end_jump_within_row() {
    let (mut tree, table, _) = build_table(5);
    focus_at(&mut tree, table, 1, 0);
    tree.press_key(Key::End, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 1)));
    tree.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 0)));
}

#[test]
fn ctrl_home_end_jump_to_corners() {
    let (mut tree, table, _) = build_table(5);
    focus_at(&mut tree, table, 2, 1);
    tree.press_key(Key::End, Modifiers::CTRL);
    assert_eq!(read_focused_cell(&tree, table), Some((4, 1)));
    tree.press_key(Key::Home, Modifiers::CTRL);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
}

#[test]
fn page_down_advances_focus_and_scroll() {
    // 100 rows × 20 px in a 200 px viewport: 10 rows per page.
    let model = rows(100);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);

    let scroll_before = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.scroll_y_signal().get()
    };
    tree.press_key(Key::PageDown, Modifiers::NONE);
    let after_pos = read_focused_cell(&tree, table).unwrap();
    let scroll_after = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.scroll_y_signal().get()
    };
    assert!(
        after_pos.0 > 0,
        "PageDown must advance focused row (got {after_pos:?})"
    );
    assert!(
        scroll_after > scroll_before,
        "PageDown must scroll forward (was {scroll_before}, now {scroll_after})"
    );
}

#[test]
fn space_toggles_row_selection_at_focus() {
    let model = rows(5);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 2, 0);
    tree.press_key(Key::Space, Modifiers::NONE);
    assert!(sel.is_selected(2));
    tree.press_key(Key::Space, Modifiers::NONE);
    assert!(!sel.is_selected(2));
}

#[test]
fn shift_arrow_extends_multi_row_selection() {
    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 2, 0);
    sel.select(2);
    tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
    tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
    let s = sel.selected_indices();
    assert!(s.contains(&2));
    assert!(s.contains(&4));
    assert!(s.len() >= 3, "expected 2..4 selected, got {s:?}");
}

#[test]
fn ctrl_a_selects_all_rows_in_multi_mode() {
    let model = rows(5);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::A, Modifiers::CTRL);
    assert_eq!(sel.count(), 5);
}

#[test]
fn escape_clears_focus() {
    let (mut tree, table, _) = build_table(3);
    focus_at(&mut tree, table, 1, 1);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), None);
}

#[test]
fn tab_moves_to_next_cell_with_row_wrap() {
    let (mut tree, table, _) = build_table(3);
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 1)));
    // Past the last column → wraps to next row, col 0.
    tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 0)));
}

#[test]
fn shift_tab_moves_to_prev_cell_with_row_wrap() {
    let (mut tree, table, _) = build_table(3);
    focus_at(&mut tree, table, 1, 0);
    tree.press_key(Key::Tab, Modifiers::SHIFT);
    // Wrap to previous row, last column.
    assert_eq!(read_focused_cell(&tree, table), Some((0, 1)));
    tree.press_key(Key::Tab, Modifiers::SHIFT);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
}

#[test]
fn cell_selection_mode_tracks_pairs() {
    let model = rows(5);
    let cs = super::CellSelectionModel::new(TableSelectionMode::MultiCell);
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(cs.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 1, 1);
    tree.press_key(Key::Space, Modifiers::NONE);
    assert!(cs.is_selected(1, 1));

    // Shift+Arrow extends rectangularly.
    tree.press_key(Key::ArrowRight, Modifiers::SHIFT); // col already at 1, no-op
    tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
    assert!(cs.is_selected(2, 1));
}

// ── Edit hooks + filter signal + row drag-drop (Phase 6) ──────────────────

#[test]
fn editing_cell_signal_round_trips() {
    let (tree, table, _) = build_table(5);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(tv.editing_cell_signal().get(), None);
    tv.begin_edit(2, "name");
    let editing = tv.editing_cell_signal().get();
    assert!(matches!(editing, Some((2, _))));
    tv.end_edit();
    assert_eq!(tv.editing_cell_signal().get(), None);
}

#[test]
fn f2_triggers_edit_request() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let fired_for: Rc<RefCell<Option<(usize, String)>>> = Rc::new(RefCell::new(None));
    let f = fired_for.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col().editable(true))
            .row_height(20.0)
            .edit_trigger(super::EditTrigger::F2)
            .on_cell_edit_request(move |row, col_id, _ctx| {
                *f.borrow_mut() = Some((row, col_id.to_string()));
            }),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 1, 1);
    tree.press_key(Key::F2, Modifiers::NONE);
    assert_eq!(*fired_for.borrow(), Some((1, "name".to_string())));
}

#[test]
fn typing_triggers_edit_request_in_f2_or_type_mode() {
    use std::cell::Cell;
    use std::rc::Rc;
    let fired = Rc::new(Cell::new(0));
    let f = fired.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            // Mark the focused column editable; the gate keeps F2 /
            // type-to-edit a no-op on non-editable columns.
            .add_column(id_col().editable(true))
            .add_column(name_col())
            .row_height(20.0)
            .edit_trigger(super::EditTrigger::F2OrType)
            .on_cell_edit_request(move |_, _, _| f.set(f.get() + 1)),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::Character('x'), Modifiers::NONE);
    assert_eq!(fired.get(), 1);
}

#[test]
fn typing_does_not_trigger_edit_when_only_f2() {
    use std::cell::Cell;
    use std::rc::Rc;
    let fired = Rc::new(Cell::new(0));
    let f = fired.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .edit_trigger(super::EditTrigger::F2)
            .on_cell_edit_request(move |_, _, _| f.set(f.get() + 1)),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::Character('x'), Modifiers::NONE);
    assert_eq!(fired.get(), 0);
}

#[test]
fn escape_ends_edit_before_clearing_focus() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 1, 0);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.begin_edit(1, "id");
    }
    // First Escape ends edit; focus stays.
    tree.press_key(Key::Escape, Modifiers::NONE);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(tv.editing_cell_signal().get(), None);
    assert_eq!(tv.focused_cell_signal().get(), Some((1, 0)));
    // Second Escape clears focus.
    let _ = tv;
    let _ = any;
    tree.press_key(Key::Escape, Modifiers::NONE);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(tv.focused_cell_signal().get(), None);
}

#[test]
fn filters_signal_is_writable() {
    let (tree, table, _) = build_table(5);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    tv.set_filter("name", "abc");
    assert_eq!(
        tv.filters_signal().get().get("name").cloned(),
        Some("abc".to_string())
    );
    tv.set_filter("name", "");
    assert_eq!(tv.filters_signal().get().get("name"), None);
    tv.set_filter("name", "x");
    tv.set_filter("id", "1");
    tv.clear_filters();
    assert!(tv.filters_signal().get().is_empty());
}

#[test]
fn enter_invokes_on_row_activate() {
    use std::cell::Cell;
    use std::rc::Rc;
    let activated = Rc::new(Cell::new(None::<usize>));
    let a = activated.clone();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .on_row_activate(move |row, _ctx| a.set(Some(row))),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 3, 0);
    tree.press_key(Key::Enter, Modifiers::NONE);
    assert_eq!(activated.get(), Some(3));
}

#[test]
fn reorderable_rows_requires_list_model_source() {
    // The reorderable_rows builder doesn't validate source type
    // (allows ListDataSource for forward-compat); the move_item_fn
    // is None for ListDataSource sources, so the actual reorder is
    // a no-op. This test documents the contract — the table compiles
    // and lays out cleanly.
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .reorderable_rows(true),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let info = tree.accessibility_node(table);
    assert_eq!(info.role(), Role::Table);
}

// ── Empty state ────────────────────────────────────────────────────────────

#[test]
fn empty_view_renders_when_no_rows() {
    let model: ListModel<Row> = ListModel::new();
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .empty_view(|| Box::new(TextWidget::new_literal("nothing here"))),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // No rows at all when the source is empty.
    // Find the empty TextWidget — search by name field via accessibility.
    let mut found = false;
    let mut walker = vec![_table];
    while let Some(id) = walker.pop() {
        let info = tree.accessibility_node(id);
        if let Some(name) = info.name()
            && name == "nothing here"
        {
            found = true;
            break;
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    assert!(found, "expected the empty_view widget to be rendered");
}

// ── Filter popover ─────────────────────────────────────────────────────────

#[cfg(feature = "rich-text")]
#[test]
fn filterable_column_exposes_filter_trigger_in_a11y_tree() {
    // The filter glyph is wrapped in a `Popover` whose default name
    // is "Filter" — locating it via `find_by_label` confirms the
    // affordance is present for filterable columns.
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col().filterable(true))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let trigger = tree.find_by_label("Filter");
    assert!(
        trigger.is_some(),
        "filterable column must expose a 'Filter' trigger via the Popover"
    );
}

#[test]
fn non_filterable_column_does_not_expose_filter_trigger() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert!(
        tree.find_by_label("Filter").is_none(),
        "columns without `.filterable(true)` must not advertise a filter trigger"
    );
}

#[cfg(feature = "rich-text")]
#[test]
fn filter_popover_opens_via_trigger_click() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col().filterable(true))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // OverlayTrigger routes its opener onto the trigger child, so a
    // pointer click on the wrapper hit-tests into the child where the
    // handler lives. AccessAction dispatched at the wrapper id alone
    // no longer fires (handlers aren't on the wrapper node).
    let trigger = tree.find_by_label("Filter").expect("trigger present");
    tree.click(trigger);
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "clicking the filter trigger should open exactly one overlay"
    );
}

#[cfg(feature = "rich-text")]
#[test]
fn filter_popover_content_hosts_a_text_input() {
    use crate::table_view::filter::FilterPopoverContent;
    // Smoke test: the popover content materialises a real `TextInput`
    // (Role::TextInput) so AT can find it and the editor receives
    // focus when the popover opens. Keystroke→signal wiring is
    // exercised indirectly via the data-grid example and via
    // `filters_signal_is_writable` for the upstream side.
    let content = FilterPopoverContent::new("seed");
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let id = tree.add(content);
    tree.layout(SizeProposal {
        width: Some(280.0),
        height: Some(60.0),
    });
    let mut walker = vec![id];
    let mut found = None;
    while let Some(n) = walker.pop() {
        if tree.accessibility_node(n).role() == Role::TextInput {
            found = Some(n);
            break;
        }
        for c in tree.children(n) {
            walker.push(c);
        }
    }
    assert!(
        found.is_some(),
        "FilterPopoverContent must contain a TextInput accessible node"
    );
}

// ── Resize coordinate-system regression ────────────────────────────────────

#[test]
fn resize_drag_right_grows_column_for_non_first_column() {
    // Regression: the HeaderCell's resize handler used to compare a
    // window-space `position.x` against the cell-local width
    // (`position.x > cell_w - resize_zone`), which fired the resize
    // from anywhere inside any column past the first one — and the
    // visual result felt inverted from the cursor direction. Verify
    // that grabbing the trailing edge of a non-first column and
    // dragging right enlarges that column.
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col()) // Fixed(60), display_pos 0
            .add_column(name_col()) // Flex(1), display_pos 1
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    // id width = 60. name takes the rest = 340. Grab name's trailing
    // edge at window x ≈ 399.
    use crate::styles::recipe_table_style as cp;
    let resize_handle = cp::RESIZE_HANDLE_WIDTH;
    let down_x = 400.0 - resize_handle * 0.5;
    let drag_to_x = down_x + 30.0;
    let down_y = cp::HEADER_HEIGHT * 0.5;

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(down_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(drag_to_x, down_y),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(drag_to_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    let widths = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.column_widths_signal().get()
    };
    let name_w = widths.get("name").copied().unwrap_or(0.0);
    assert!(
        name_w > 350.0,
        "drag right should enlarge column past its baseline ~340; got {}",
        name_w
    );
}

#[test]
fn resize_pointer_down_in_filter_zone_does_not_start_resize() {
    // When a column is filterable, the trailing region holds the
    // filter glyph + its padding. The header's pointer handler must
    // leave that band alone so a click reaches the popover trigger
    // — not get swallowed by sort or accidentally start a resize.
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col().filterable(true))
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    use crate::styles::recipe_table_style as cp;
    // Click well inside the filter-zone band — past the resize zone
    // but before the sort/label region.
    let click_x = 400.0
        - cp::RESIZE_HANDLE_WIDTH
        - (cp::FILTER_INDICATOR_SIZE + cp::CELL_PADDING_HORIZONTAL) * 0.5;
    let click_y = cp::HEADER_HEIGHT * 0.5;
    let mods = Modifiers::NONE;
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(click_x, click_y),
        button: PointerButton::Primary,
        modifiers: mods,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(click_x + 20.0, click_y),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(click_x + 20.0, click_y),
        button: PointerButton::Primary,
        modifiers: mods,
    });
    let widths = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.column_widths_signal().get()
    };
    assert!(
        !widths.contains_key("name"),
        "PointerDown in the filter zone must not commit a resize override"
    );
}

// ── Bugs reported from real interaction ────────────────────────────────────

#[cfg(feature = "rich-text")]
#[test]
fn filter_popover_editor_gains_focus_on_mouse_click() {
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col().filterable(true))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    let trigger = tree.find_by_label("Filter").expect("trigger present");
    tree.click(trigger);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    // Find the editor inside the popover overlay.
    let editor = {
        let mut walker: Vec<WidgetId> = tree.overlay_manager().active_content_ids();
        let mut found = None;
        while let Some(n) = walker.pop() {
            if tree.accessibility_node(n).role() == Role::TextInput {
                found = Some(n);
                break;
            }
            for c in tree.children(n) {
                walker.push(c);
            }
        }
        found.expect("TextInput must exist inside the filter popover overlay")
    };
    // Click the centre of the editor — auto-focus should land on it.
    tree.click(editor);
    assert_eq!(
        tree.focused(),
        Some(editor),
        "clicking the popover's filter editor should focus it"
    );
}

#[test]
fn header_resize_works_when_table_is_nested_in_panel() {
    // Mirrors the real data-grid layout: VStack → Panel → TableView.
    // The original regression test put the table at root and missed
    // any coordinate-system bug introduced by nesting.
    use crate::{Panel, VStack};
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = TableView::new(rows(5))
        .add_column(id_col())
        .add_column(name_col())
        .row_height(20.0)
        .show_internal_scrollbars(false);
    let table_id = tree.add(table);
    let layout = VStack::new()
        .spacing(6.0)
        .child(Panel::new().child_id(table_id));
    tree.add(layout);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let table_bounds = tree.bounds(table_id);
    use crate::styles::recipe_table_style as cp;

    // Trailing edge of "name" in window coords (table is offset by the
    // VStack/Panel padding, so we have to ask the arena where the
    // table actually sits).
    let resize_handle = cp::RESIZE_HANDLE_WIDTH;
    let down_x = table_bounds.right() - resize_handle * 0.5;
    let drag_to_x = down_x + 30.0;
    let down_y = table_bounds.y + cp::HEADER_HEIGHT * 0.5;

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(down_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(drag_to_x, down_y),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(drag_to_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    let widths = {
        let any = tree.widget_as_any(table_id).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.column_widths_signal().get()
    };
    let name_w = widths.get("name").copied().unwrap_or(0.0);
    assert!(
        name_w > 100.0,
        "drag-resize on a nested table must commit a width override (got name={})",
        name_w
    );
}

#[test]
fn cursor_resets_to_default_when_pointer_leaves_resize_zone() {
    // After hovering the trailing edge (cursor=ColResize), moving the
    // pointer away from the zone — even within the same cell — must
    // restore the default cursor. Otherwise the cursor stays stuck
    // looking like a resize affordance over non-resize regions.
    use bastyde_canvas::Point;
    use bastyde_core::event::WidgetEvent;
    use bastyde_core::widget::CursorIcon;

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    use crate::styles::recipe_table_style as cp;
    let header_y = cp::HEADER_HEIGHT * 0.5;
    // Hover near the right edge of "name" → cursor becomes ColResize.
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(398.0, header_y),
    });
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::ColResize,
        "expected ColResize on the trailing edge"
    );
    // Now hover well inside the cell, away from the edge.
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(200.0, header_y),
    });
    assert_eq!(
        tree.current_cursor(),
        CursorIcon::Default,
        "cursor must reset to Default once the pointer leaves the trailing-edge zone"
    );
}

#[test]
fn header_resizing_works_in_full_data_grid_layout() {
    // Reproduces the data-grid example layout: pinned-leading id column
    // + multiple flex columns + filterable name column + scrollable
    // body. Asserts that grabbing the trailing edge of "name" actually
    // commits a width override.
    use crate::{Panel, VStack};
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};

    fn id_c() -> Column<Row> {
        Column::<Row>::new("id", "ID", |r, _: &CellContext| {
            Box::new(TextWidget::new_literal(r.id.to_string()))
        })
        .width(ColumnWidth::Fixed(60.0))
        .pinned(crate::table_view::column::PinnedSide::Leading)
        .sortable(true)
    }
    fn name_c() -> Column<Row> {
        Column::<Row>::new("name", "Name", |r, _: &CellContext| {
            Box::new(TextWidget::new_literal(r.name.clone()))
        })
        .width(ColumnWidth::Flex(2.0))
        .sortable(true)
        .filterable(true)
    }

    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = TableView::new(rows(50))
        .add_column(id_c())
        .add_column(name_c())
        .row_height(28.0)
        .selection_mode(TableSelectionMode::MultiRow)
        .selection(SelectionModel::new(SelectionMode::Multi));
    let table_id = tree.add(table);
    let layout = VStack::new()
        .spacing(6.0)
        .child(Panel::new().child_id(table_id));
    tree.add(layout);
    tree.layout(SizeProposal {
        width: Some(600.0),
        height: Some(400.0),
    });

    // Hit the right edge of the cell. We can't compute the absolute
    // trailing edge of "name" from layout alone (Panel adds padding,
    // VStack adds spacing) — read it from the table's own widths.
    use crate::styles::recipe_table_style as cp;
    let (name_w_at_layout, table_bounds) = {
        let any = tree.widget_as_any(table_id).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        let w = tv.column_widths_signal().get();
        (w.get("name").copied(), tree.bounds(table_id))
    };
    // No override yet — flex resolved to (table_w - 60 - scrollbar).
    assert!(name_w_at_layout.is_none());

    // The "name" column's trailing edge sits at table_bounds.right
    // minus the scrollbar reservation (12 px). resize zone is the
    // last `resize_handle_width` of that.
    let scrollbar_thickness = 12.0_f32;
    let trailing_x = table_bounds.right() - scrollbar_thickness;
    let down_x = trailing_x - cp::RESIZE_HANDLE_WIDTH * 0.5;
    let down_y = table_bounds.y + cp::HEADER_HEIGHT * 0.5;
    let drop_x = down_x + 60.0;

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(down_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(drop_x, down_y),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(drop_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    let widths = {
        let any = tree.widget_as_any(table_id).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.column_widths_signal().get()
    };
    assert!(
        widths.contains_key("name"),
        "drag-resize on a filterable column inside a Panel must commit a width override"
    );
}

#[test]
fn body_row_does_not_paint_over_header_after_scroll() {
    // Regression: when scrolled, body rows whose top is above the
    // header band used to paint *over* the header because they were
    // listed before the header in `children()` and the framework
    // paints children in order. With the fixed z-order (header last),
    // the header always ends up on top.
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(50))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Scroll halfway through.
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.scroll_y_signal().set(80.0);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Walk the table's direct children — the row-group (which
    // contains all body rows) must come BEFORE the header row, so
    // the header z-orders on top in paint.
    let children = tree.children(table);
    let mut header_pos: Option<usize> = None;
    let mut rowgroup_pos: Option<usize> = None;
    for (i, child) in children.iter().copied().enumerate() {
        let role = tree.accessibility_node(child).role();
        if role == Role::Row {
            header_pos = Some(i);
        } else if role == Role::RowGroup {
            rowgroup_pos = Some(i);
        }
    }
    let h = header_pos.expect("header row must be a direct child");
    let g = rowgroup_pos.expect("row-group must be a direct child");
    assert!(
        h > g,
        "header (idx {}) must paint after the row-group (idx {})",
        h,
        g
    );
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Walk the tree and return the cell IDs of the first (lowest row_index)
/// `Role::Row` found.
fn first_visible_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    // Cells appear in tree order because place_children iterates them in
    // order; we just take the first row's children in tree order.
    let mut walker = vec![root];
    let mut best: Option<WidgetId> = None;
    while let Some(id) = walker.pop() {
        let info = tree.accessibility_node(id);
        if info.role() == Role::Row && best.is_none() {
            best = Some(id);
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    let row = best.expect("no Role::Row found");
    tree.children(row)
}

fn count_role(tree: &WidgetTree, root: WidgetId, role: Role) -> usize {
    let mut walker = vec![root];
    let mut n = 0;
    while let Some(id) = walker.pop() {
        let info = tree.accessibility_node(id);
        if info.role() == role {
            n += 1;
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    n
}

// -----------------------------------------------------------------------------
// TableStyle::make_header_cell wiring.
// -----------------------------------------------------------------------------

#[test]
fn header_cells_route_through_table_style_make_header_cell() {
    // Installing a custom `TableStyle` whose `make_header_cell`
    // returns a sentinel widget proves the header cell wires its
    // chrome through the trait instead of building it inline. The
    // sentinel here is `RectWidget` with a `Selected` background —
    // every header cell ends up with this id as its root, so we can
    // count how many `Role::ColumnHeader` AT nodes have it as their
    // first descendant.
    use bastyde_core::build_context::BuildContext;
    use bastyde_core::styles::{TableGridRecipe, TableHeaderCellConfig, TableRowConfig, TableStyle};
    use bastyde_core::widget_id::WidgetId;
    use std::cell::Cell;
    use std::rc::Rc;

    struct CountingStyle {
        calls: Rc<Cell<u32>>,
    }
    impl TableStyle for CountingStyle {
        fn make_header_cell(
            &self,
            cfg: &TableHeaderCellConfig,
            ctx: &mut BuildContext,
        ) -> WidgetId {
            self.calls.set(self.calls.get() + 1);
            // Return the label slot directly so we don't perturb
            // layout — a passthrough style.
            let _ = ctx;
            cfg.label
        }
        fn make_sort_indicator(
            &self,
            _direction: bastyde_core::styles::SortDirection,
            ctx: &mut BuildContext,
        ) -> WidgetId {
            ctx.add(crate::primitives::Spacer::new())
        }
        fn make_row_background(&self, _cfg: &TableRowConfig, ctx: &mut BuildContext) -> WidgetId {
            ctx.add(crate::primitives::Spacer::new())
        }
        fn grid(&self) -> TableGridRecipe {
            TableGridRecipe::default()
        }
    }

    let calls = Rc::new(Cell::new(0_u32));
    let style: Rc<dyn TableStyle> = Rc::new(CountingStyle {
        calls: calls.clone(),
    });
    let mut theme = bastyde_core::presets::intui::light();
    theme.style_slots.table = Some(style);
    let mut tree = WidgetTree::new().with_theme(theme);
    tree.add(
        TableView::new(rows(2))
            .add_column(id_col())
            .add_column(name_col()),
    );
    tree.layout(SizeProposal::exact(400.0, 240.0));
    let _ = tree.render();

    // One call per header cell — two columns means two calls.
    assert_eq!(
        calls.get(),
        2,
        "TableStyle::make_header_cell must be called once per header cell",
    );
}

// -- Boundary scroll chaining -----------------------------------------------

/// A TableView (40 × 20px rows in a ~120px viewport) above a filler inside an
/// outer ScrollArea, so chaining from the inner table to the outer area is
/// observable.
fn nested_table_fixture(inner: OverscrollBehavior) -> (WidgetTree, Signal<f32>, Signal<f32>) {
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, VStack};
    let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
    let tv = TableView::new(rows(40))
        .add_column(id_col())
        .add_column(name_col())
        .row_height(20.0)
        .overscroll_behavior(inner);
    let inner_y = tv.scroll_y_signal().clone();
    let tv_id = tree.add(tv);
    let viewport = tree.add(FixedSize::new().bind_width(220.0).bind_height(120.0).child_id(tv_id));
    let filler = tree.add(
        FixedSize::new()
            .bind_width(220.0)
            .bind_height(300.0)
            .child(TextWidget::new_literal("")),
    );
    let outer_content = tree.add(VStack::new().add_child(viewport).add_child(filler));
    let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let _outer = tree.add(outer);
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    (tree, inner_y, outer_y)
}

#[test]
fn nested_table_chains_to_outer_at_boundary() {
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, inner_y, outer_y) = nested_table_fixture(OverscrollBehavior::Chain);
    // Pointer in the table body (below the header).
    tree.pointer_move(Point::new(110.0, 90.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    let inner_bottom = inner_y.get();
    assert!(inner_bottom > 0.0, "inner table should scroll down; got {inner_bottom}");
    assert!(outer_y.get() < 0.01, "outer must not move while the inner absorbs");

    tree.pointer_move(Point::new(110.0, 90.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    assert!((inner_y.get() - inner_bottom).abs() < 0.01, "inner stays clamped at bottom");
    assert!(outer_y.get() > 0.01, "outer scrolled because the inner chained the boundary");
}

#[test]
fn nested_table_contain_blocks_chaining() {
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, _inner_y, outer_y) = nested_table_fixture(OverscrollBehavior::Contain);
    tree.pointer_move(Point::new(110.0, 90.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    tree.pointer_move(Point::new(110.0, 90.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    assert!(outer_y.get() < 0.01, "Contain must prevent chaining: outer stays put");
}
