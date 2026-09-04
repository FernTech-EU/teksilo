// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::signal::Signal;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel};
use teksilo_i18n::lit;

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
    Column::<Row>::new("id", lit!("ID"), |row, _: &CellContext| {
        Box::new(TextWidget::new(lit!(row.id.to_string())))
    })
    .width(ColumnWidth::Fixed(60.0))
}

fn name_col() -> Column<Row> {
    Column::<Row>::new("name", lit!("Name"), |row, _: &CellContext| {
        Box::new(TextWidget::new(lit!(row.name.clone())))
    })
    .width(ColumnWidth::Flex(1.0))
}

/// Simple two-column table over `n` rows. Returns the tree + the table id.
fn build_table(n: u32) -> (WidgetTree, WidgetId, ListModel<Row>) {
    let model = rows(n);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

#[test]
fn rows_materialize_during_scrollbar_thumb_drag() {
    // The reason `BodyPane` exists — see `common::thumb_drag_test`'s module
    // docs for the invariant, and for why every virtualized view asserts it.
    use crate::styles::recipe_table_style as cp;
    let (mut tree, table, _) = build_table(500);
    crate::common::thumb_drag_test::assert_body_survives_thumb_drag(
        &mut tree,
        table,
        400.0,
        200.0,
        cp::HEADER_HEIGHT,
        "TableView",
        |t| {
            let mut n = 0;
            let mut walker = vec![table];
            while let Some(id) = walker.pop() {
                if t.accessibility_node(id).role() == Role::Row {
                    let b = t.bounds(id);
                    if b.y >= 0.0 && b.y < 200.0 {
                        n += 1;
                    }
                }
                for c in t.children(id) {
                    walker.push(c);
                }
            }
            n
        },
    );
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
    assert_eq!(info.role(), Role::Grid);

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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
fn root_role_is_grid_with_row_and_col_count() {
    // `Role::Grid`, not `Role::Table`: a table the keyboard can drive is the
    // interactive ARIA pattern, and `Role::Table` is the one role
    // `accesskit_consumer` will not treat as a selection container — so a
    // multi-select table announced that way exposed no selection to UIA.
    let (tree, table, _) = build_table(50);
    let info = tree.accessibility_node(table);
    assert_eq!(info.role(), Role::Grid);
    let row_count = count_role(&tree, table, Role::Row);
    let header_count = count_role(&tree, table, Role::ColumnHeader);
    let cell_count = count_role(&tree, table, Role::Cell);
    // Header is on by default. Each body row carries 2
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

// ── Header / Sort / Resize ─────────────────────────────────────────────────

#[test]
fn header_strip_renders_one_column_header_per_column() {
    let (tree, table, _) = build_table(10);
    let header_count = count_role(&tree, table, Role::ColumnHeader);
    assert_eq!(header_count, 2, "one ColumnHeader per declared column");
}

#[test]
fn header_can_be_hidden() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

// ── Reorder + pinned-side ──────────────────────────────────────────────────

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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            // Declaration: id, name. Trailing-pinned `id` should still
            // visually trail name once we mark it Trailing.
            .add_column(
                Column::<Row>::new("id", lit!("ID"), |row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new(lit!(row.id.to_string())))
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col()) // 60
            .add_column(name_col())
            .add_column(
                Column::<Row>::new("extra", lit!("Extra"), |_row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new(lit!("…")))
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

// ── Keyboard / focused cell / cell selection ───────────────────────────────

use teksilo_core::event::{Key, Modifiers};

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
fn first_arrow_lands_on_an_end_cell_instead_of_skipping_it() {
    // "No cursor yet" is not "cursor on (0, 0)". Collapsing the two (the old
    // `focused_cell.get().unwrap_or((0, 0))`) meant the first ArrowDown stepped
    // to row 1 — skipping row 0 — the first ArrowUp was a DEAD KEY
    // (`prev_row(0)` is `None`, so nothing happened at all), and the first
    // ArrowRight skipped the leading column. Each arrow must now land ON the
    // end cell it enters from. Table is 10 rows × 2 columns.
    use teksilo_core::event::{Key, Modifiers};

    for (key, want, what) in [
        (
            Key::ArrowDown,
            (0, 0),
            "first ArrowDown enters at the first row",
        ),
        (Key::ArrowUp, (9, 0), "first ArrowUp enters at the last row"),
        (
            Key::ArrowRight,
            (0, 0),
            "first ArrowRight enters at the leading column",
        ),
        (
            Key::ArrowLeft,
            (0, 1),
            "first ArrowLeft enters at the trailing column",
        ),
    ] {
        let (mut tree, table, _) = build_table(10);
        // Focus the VIEW, but set no cell cursor.
        tree.focus(table);
        assert_eq!(
            read_focused_cell(&tree, table),
            None,
            "precondition: no cell cursor yet"
        );

        tree.press_key(key, Modifiers::NONE);
        assert_eq!(read_focused_cell(&tree, table), Some(want), "{what}");
    }
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
fn ctrl_arrow_moves_cursor_without_touching_selection() {
    // Explorer/Finder convention: Ctrl+Arrow repositions the keyboard
    // cursor without touching selection; plain Arrow keeps its existing
    // select-follow behavior.
    let model = rows(5);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    sel.select(0);
    assert_eq!(sel.selected_indices(), vec![0]);

    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((1, 0)),
        "cursor advances"
    );
    assert_eq!(
        sel.selected_indices(),
        vec![0],
        "Ctrl+Arrow must not touch selection"
    );

    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(read_focused_cell(&tree, table), Some((2, 0)));
    assert_eq!(
        sel.selected_indices(),
        vec![0],
        "still untouched after a second Ctrl+Arrow"
    );

    // Plain Arrow (no Ctrl) resumes select-follow from wherever the
    // Ctrl+Arrow walk left the cursor.
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((3, 0)));
    assert_eq!(
        sel.selected_indices(),
        vec![3],
        "plain Arrow selects the row it lands on"
    );
}

#[test]
fn ctrl_space_toggles_the_cursor_row_after_a_ctrl_arrow_move() {
    let model = rows(5);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((2, 0)),
        "cursor at row 2, nothing selected yet"
    );
    assert!(sel.selected_indices().is_empty());

    tree.press_key(Key::Space, Modifiers::CTRL);
    assert_eq!(
        sel.selected_indices(),
        vec![2],
        "Ctrl+Space toggles the focused row on"
    );

    tree.press_key(Key::Space, Modifiers::CTRL);
    assert!(
        sel.selected_indices().is_empty(),
        "Ctrl+Space toggles it back off"
    );
}

#[test]
fn row_click_moves_focus_so_arrow_nav_resumes_there() {
    // Guards that a click moves the keyboard-navigation cursor (`focused_cell`,
    // the arrow-nav origin) to the clicked row, so the next Arrow resumes from
    // there and not from the stale cursor / row 0. In TableView this is provided
    // by the per-cell pointer handler (fires on every cell click, all modes) —
    // unlike TreeTableView, whose row handler must sync it explicitly.
    use crate::styles::recipe_table_style as cp;
    use teksilo_canvas::Point;
    use teksilo_core::event::{PointerButton, WidgetEvent};

    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(table);

    // Click row 3's body: rows are 20px tall and start below the header.
    let click_y = cp::HEADER_HEIGHT + 3.0 * 20.0 + 10.0;
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(100.0, click_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(100.0, click_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    assert_eq!(sel.selected_indices(), vec![3], "click selects row 3");
    assert_eq!(
        read_focused_cell(&tree, table).map(|(r, _)| r),
        Some(3),
        "click must move the nav cursor to the clicked row"
    );

    // ArrowDown resumes from the clicked row (3 → 4), not from row 0 → 1.
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        sel.selected_indices(),
        vec![4],
        "ArrowDown after a click resumes from the clicked row (3 → 4)"
    );
}

#[test]
fn home_end_reach_the_first_and_last_row_when_rows_are_the_unit() {
    // A row-selection table has no cell cursor for "start of the row" to mean
    // anything against, so Home is the first *row* — which is what Explorer's
    // details view and every list control do. The column is carried along
    // rather than reset, since nothing here selects a column.
    let (mut tree, table, _) = build_table(5);
    focus_at(&mut tree, table, 1, 1);
    tree.press_key(Key::End, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((4, 1)));
    tree.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 1)));
}

#[test]
fn home_end_jump_within_row_when_cells_are_the_unit() {
    // With a cell cursor the row *is* a navigable unit, so Home is its start —
    // the ARIA grid rule, and Qt's `QTableView`.
    let (mut tree, table, _) = build_cell_table(5);
    focus_at(&mut tree, table, 1, 0);
    tree.press_key(Key::End, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 1)));
    tree.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((1, 0)));
}

#[test]
fn ctrl_home_end_jump_to_corners_in_a_cell_grid() {
    let (mut tree, table, _) = build_cell_table(5);
    focus_at(&mut tree, table, 2, 1);
    tree.press_key(Key::End, Modifiers::COMMAND);
    assert_eq!(read_focused_cell(&tree, table), Some((4, 1)));
    tree.press_key(Key::Home, Modifiers::COMMAND);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
}

#[test]
fn the_accelerator_moves_the_row_cursor_without_selecting() {
    use teksilo_data::{SelectionMode, SelectionModel};
    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    focus_at(&mut tree, table, 3, 0);
    sel.select(3);

    tree.press_key(Key::End, Modifiers::COMMAND);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((9, 0)),
        "cursor moved"
    );
    assert_eq!(sel.selected_indices(), vec![3], "selection did not");

    // And Shift+End extends from the anchor instead.
    tree.press_key(Key::End, Modifiers::SHIFT);
    assert_eq!(sel.selected_indices(), (3..=9).collect::<Vec<_>>());
    tree.press_key(Key::Home, Modifiers::SHIFT);
    assert_eq!(
        sel.selected_indices(),
        (0..=3).collect::<Vec<_>>(),
        "reversing shrinks the range"
    );
}

/// The same fixture as `build_table`, in Excel-style cell selection.
fn build_cell_table(n: u32) -> (WidgetTree, WidgetId, ListModel<Row>) {
    let model = rows(n);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model.clone())
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(super::CellSelectionModel::new(
                TableSelectionMode::MultiCell,
            )),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    (tree, table, model)
}

fn read_scroll(tree: &WidgetTree, table: WidgetId) -> f32 {
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    tv.scroll_y_signal().get()
}

#[test]
fn arrow_nav_scroll_follows_focused_row() {
    // 100 rows × 20 px in a 200 px viewport. Walking the focus down past
    // the visible window must scroll to keep the focused row visible
    // ("selection always visible") — the behavior ListView / TreeView
    // already have. Regression for: TableView keyboard nav left scroll_y
    // untouched, so the focused row marched off-screen.
    let model = rows(100);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    focus_at(&mut tree, table, 0, 0);
    assert_eq!(read_scroll(&tree, table), 0.0, "starts at top");

    // Arrow down to row 20 — far below the ~10-row viewport.
    for _ in 0..20 {
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.layout(proposal);
    }
    assert_eq!(read_focused_cell(&tree, table), Some((20, 0)));
    let scroll = read_scroll(&tree, table);
    assert!(
        scroll > 200.0,
        "arrow-down nav must scroll to reveal row 20, got {scroll}"
    );

    // Ctrl+Home returns focus AND scroll to the very top.
    tree.press_key(Key::Home, Modifiers::COMMAND);
    tree.layout(proposal);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
    assert_eq!(
        read_scroll(&tree, table),
        0.0,
        "Ctrl+Home must scroll back to the top"
    );

    // Ctrl+End jumps focus AND scroll to reveal the last row. The column is
    // carried rather than reset: this table selects rows, so there is no cell
    // cursor for "the last column" to be the corner of.
    tree.press_key(Key::End, Modifiers::COMMAND);
    tree.layout(proposal);
    assert_eq!(read_focused_cell(&tree, table), Some((99, 0)));
    assert!(
        read_scroll(&tree, table) > 0.0,
        "Ctrl+End must scroll to reveal the last row"
    );
}

#[test]
fn type_ahead_jumps_focus_to_matching_row() {
    let model = ListModel::from_vec(vec![
        Row {
            id: 0,
            name: "Apple".into(),
        },
        Row {
            id: 1,
            name: "Banana".into(),
        },
        Row {
            id: 2,
            name: "Cherry".into(),
        },
        Row {
            id: 3,
            name: "Cranberry".into(),
        },
    ]);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(name_col())
            .row_height(20.0)
            .type_ahead_label(|r: &Row| r.name.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);

    // 'c' → Cherry (first row after 0 starting with c).
    tree.press_key(Key::C, Modifiers::NONE);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((2, 0)),
        "'c' → Cherry"
    );
    // 'r' within the timeout → buffer "cr" → Cranberry.
    tree.press_key(Key::R, Modifiers::NONE);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((3, 0)),
        "'cr' → Cranberry"
    );
}

#[test]
fn page_down_advances_focus_and_scroll() {
    // 100 rows × 20 px in a 200 px viewport: 10 rows per page.
    let model = rows(100);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
fn keyed_selection_stores_row_key_not_index() {
    // from_source_keyed wires a KeyedSelectionModel<S::Key>: Space on the
    // focused row toggles the row's KEY, not its index (here keys differ from
    // indices, so a key mix-up would be caught).
    use std::rc::Rc;
    use teksilo_core::ObserverHandle;
    use teksilo_data::{KeyedSelectionModel, ListDataSource, SelectionMode};

    struct KeyedRowSource {
        rows: Vec<(u64, Row)>, // (stable key, row)
    }
    impl ListDataSource for KeyedRowSource {
        type Item = Row;
        type Key = u64;
        fn len(&self) -> usize {
            self.rows.len()
        }
        fn with_item<R>(&self, i: usize, f: impl FnOnce(&Row) -> R) -> Option<R> {
            self.rows.get(i).map(|(_, r)| f(r))
        }
        fn key_at(&self, i: usize) -> Option<u64> {
            self.rows.get(i).map(|(k, _)| *k)
        }
        fn index_of(&self, key: &u64) -> Option<usize> {
            self.rows.iter().position(|(k, _)| k == key)
        }
        fn observe_changes(
            &self,
            _f: impl Fn(&teksilo_data::DataChange) + 'static,
        ) -> ObserverHandle {
            ObserverHandle::new(Rc::new(()) as Rc<dyn std::any::Any>, 0, Rc::new(|_| {}))
        }
    }

    let keyed = KeyedSelectionModel::<u64>::new(SelectionMode::Multi);
    let source = KeyedRowSource {
        rows: (0..5)
            .map(|i| {
                (
                    500 + i as u64 * 100,
                    Row {
                        id: i,
                        name: format!("row {i}"),
                    },
                )
            })
            .collect(),
    };
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::from_source_keyed(source, keyed.clone())
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Focus row 2 (key 700) and toggle it.
    focus_at(&mut tree, table, 2, 0);
    tree.press_key(Key::Space, Modifiers::NONE);
    assert!(
        keyed.is_selected(&700),
        "the row KEY is selected, not the index"
    );
    assert!(!keyed.is_selected(&2), "the index is not used as a key");
}

#[test]
fn shift_arrow_extends_multi_row_selection() {
    let model = rows(10);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    tree.press_key(Key::A, Modifiers::COMMAND);
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
fn ctrl_tab_escapes_the_cell_grid() {
    use crate::primitives::VStack;
    use teksilo_core::widget_builder::WidgetBuilder;

    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0),
    );
    // A focusable sibling after the table so focus cycling has somewhere to go.
    let sink = tree.add(TextWidget::new(lit!("sink")).focusable(true));
    let _root = tree.add(VStack::new().add_child(table).add_child(sink));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    focus_at(&mut tree, table, 0, 0);
    // Plain Tab still navigates within the grid.
    tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((0, 1)),
        "plain Tab navigates cells"
    );

    // Ctrl+Tab escapes: the focused cell does NOT advance, and keyboard focus
    // leaves the table (framework focus cycling moves to the sibling).
    let before = read_focused_cell(&tree, table);
    tree.press_key(Key::Tab, Modifiers::CTRL);
    assert_eq!(
        read_focused_cell(&tree, table),
        before,
        "Ctrl+Tab must not navigate cells"
    );
    assert_eq!(
        tree.focused(),
        Some(sink),
        "Ctrl+Tab moves focus out of the table to the next focusable"
    );
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

// ── AT active_descendant follows cell focus ─────────────────────────────────

#[test]
fn focused_cell_sets_active_descendant_to_the_cell_node() {
    use teksilo_core::accessibility::widget_id_to_node_id;

    let (mut tree, table, _) = build_table(5);
    focus_at(&mut tree, table, 1, 1);
    let update = tree.sync_accessibility();
    let table_node_id = widget_id_to_node_id(table);
    let table_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == table_node_id)
        .map(|(_, n)| n)
        .expect("table node present in the AT tree");
    let active = table_node
        .active_descendant()
        .expect("a focused cell must set active_descendant");

    // It must resolve to a real node in this same update, and that node
    // must be the cell itself (Role::Cell), not the row or the table.
    let cell_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == active)
        .map(|(_, n)| n)
        .expect("active_descendant must reference a node present in the TreeUpdate");
    assert_eq!(cell_node.role(), Role::Cell);
}

#[test]
fn active_descendant_clears_after_the_focused_cell_scrolls_out_of_realization() {
    use teksilo_core::accessibility::widget_id_to_node_id;

    let (mut tree, table, _model) = build_table(1000);
    focus_at(&mut tree, table, 1, 1);
    let table_node_id = widget_id_to_node_id(table);
    let update = tree.sync_accessibility();
    let active_before = update
        .nodes
        .iter()
        .find(|(id, _)| *id == table_node_id)
        .and_then(|(_, n)| n.active_descendant());
    assert!(active_before.is_some(), "row 1 is realized initially");

    // Scroll far enough that row 1 leaves the realized+buffer window.
    // Nothing clears `focused_cell` on scroll — the keyboard-nav cursor
    // is meant to persist off-screen — so this exercises exactly the
    // "stale id" hazard: the cell's WidgetId from the pre-scroll build no
    // longer has a live AT node once the pane rebuilds without it.
    let signal = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.scroll_y_signal().clone()
    };
    signal.set(2000.0);
    tree.request_frame();
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let update = tree.sync_accessibility();
    let active_after = update
        .nodes
        .iter()
        .find(|(id, _)| *id == table_node_id)
        .and_then(|(_, n)| n.active_descendant());
    assert_eq!(
        active_after, None,
        "a focused cell that scrolled out of realization must not leave a \
         stale active_descendant pointing at a destroyed node"
    );
}

// ── Taking focus reveals the row the cursor is on ───────────────────────────

/// Taking focus scrolls the keyboard cursor's row into the realized window.
///
/// Only the rows near the viewport are realized, so a cursor placed before the
/// table is looked at (a restored session, a "jump to what is happening now")
/// usually sits outside that window. Nothing then speaks for it: no row node
/// carries `selected`, the `cell_map` lookup in `accessibility()` finds nothing
/// so no `active_descendant` is nominated, and a screen reader taking focus
/// here is told nothing at all. The first arrow press steps past the row as
/// well, because the cursor was somewhere nobody was shown.
///
/// Asserted on the accessibility tree, since that is what the failure was
/// about: the cell has to be a node a platform can name.
#[test]
fn taking_focus_reveals_the_focused_cells_row() {
    use teksilo_core::accessibility::widget_id_to_node_id;

    let (mut tree, table, _model) = build_table(1000);
    let table_node_id = widget_id_to_node_id(table);
    let viewport = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };

    // Place the cursor far below the viewport WITHOUT giving the table focus.
    // `set_focused_cell` on its own never scrolls: only the key handler does
    // (`table_view/keyboard.rs:445`), and no key was pressed.
    {
        let any = tree.widget_as_any(table).unwrap();
        any.downcast_ref::<TableView<Row>>()
            .unwrap()
            .set_focused_cell(500, 1);
    }
    tree.request_frame();
    tree.layout(viewport);

    let update = tree.sync_accessibility();
    assert_eq!(
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == table_node_id)
            .and_then(|(_, n)| n.active_descendant()),
        None,
        "row 500 starts far outside the realized window, which is the case \
         this is about"
    );

    tree.focus(table);
    tree.layout(viewport);

    let update = tree.sync_accessibility();
    let active = update
        .nodes
        .iter()
        .find(|(id, _)| *id == table_node_id)
        .and_then(|(_, n)| n.active_descendant())
        .expect(
            "taking focus has to bring the cursor's row into the realized \
             window, or nothing in the tree can be told about it",
        );
    let cell = update
        .nodes
        .iter()
        .find(|(id, _)| *id == active)
        .map(|(_, n)| n)
        .expect("active_descendant must reference a node present in the TreeUpdate");
    assert_eq!(cell.role(), Role::Cell);
    assert_eq!(
        cell.row_index(),
        Some(501),
        "1-based, and it must be the cursor's own row rather than whichever \
         row the viewport happened to be showing"
    );
}

/// With no cell navigated to yet, the selected row is the one revealed.
///
/// A table restored into a selection has no `focused_cell`, so resolving the
/// current row only from the cursor would leave that selection off-screen and
/// unrealized: no row node carrying `selected` for AT-SPI to announce either.
/// Same fallback order the context-menu key already uses.
#[test]
fn taking_focus_reveals_the_selected_row_when_no_cell_has_been_navigated_to() {
    let model = rows(1000);
    let sel = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(sel.clone()),
    );
    let viewport = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(viewport);

    sel.select(500);
    tree.request_frame();
    tree.layout(viewport);

    let realized_selected_rows = |tree: &mut WidgetTree| -> Vec<usize> {
        tree.sync_accessibility()
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == Role::Row && n.is_selected() == Some(true))
            .filter_map(|(_, n)| n.row_index())
            .collect()
    };

    assert!(
        realized_selected_rows(&mut tree).is_empty(),
        "row 500 starts far outside the realized window, which is the case \
         this is about"
    );

    tree.focus(table);
    tree.layout(viewport);

    assert_eq!(
        realized_selected_rows(&mut tree),
        vec![501],
        "taking focus has to bring the selected row into the realized window \
         (1-based row index), or nothing in the tree can be told about it"
    );
}

/// And a row already on screen does not jump.
///
/// Ensure-visible, not scroll-to: somebody who can see the table must not have
/// it lurch when they click or Tab into it.
#[test]
fn taking_focus_does_not_move_a_row_already_in_view() {
    let (mut tree, table, _model) = build_table(1000);
    let viewport = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };

    let scroll = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.set_focused_cell(2, 1);
        tv.scroll_y_signal().clone()
    };
    tree.request_frame();
    tree.layout(viewport);
    let before = scroll.get();

    tree.focus(table);
    tree.layout(viewport);

    assert_eq!(
        scroll.get(),
        before,
        "row 2 is already visible, so the table must not scroll at all"
    );
}

// ── Cell state survives a column reorder/pin ───────────────────────────────
//
// `focused_cell`, `editing_cell`, and `CellSelectionModel` all store
// `(row, display_position)`. A drag-to-reorder or a pin toggle only bumps
// the rebuild version (see the `column_order_signal` / `column_pinning_signal`
// effects in `TableView::build`) — without a remap, the stored display
// position would silently relabel onto whatever column now sits there.
// These mirror the `begin_edit_resolves_before_the_view_is_mounted` tests'
// style: pinning makes display order diverge from declaration order, so a
// shortcut that merely keeps the same index (agreeing by accident when
// nothing moved) would fail them.

#[test]
fn column_pinning_remaps_focused_cell_to_follow_its_column() {
    // Unpinned, display order is declaration order: id@0, name@1.
    let (mut tree, table, _) = build_table(3);
    focus_at(&mut tree, table, 1, 1); // focus `name`, at display position 1
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        // Pinning `name` Leading swaps it ahead of `id` — display order
        // becomes [name, id]. A stale (1, 1) would now land on `id`.
        tv.set_column_pinning("name", super::PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((1, 0)),
        "focus must follow `name` to its new display position"
    );
}

#[test]
fn column_pinning_remaps_editing_cell_to_follow_its_column() {
    let (mut tree, table, _) = build_table(3);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.begin_edit(1, "name"); // name @ display position 1
        assert_eq!(tv.editing_cell_signal().get(), Some((1, 1)));
        tv.set_column_pinning("name", super::PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(
        tv.editing_cell_signal().get(),
        Some((1, 0)),
        "the open editor must follow `name` to its new display position, not \
         relabel onto whatever column now sits at position 1"
    );
}

#[test]
fn column_pinning_remaps_cell_selection_to_follow_its_column() {
    let model = rows(3);
    let cs = super::CellSelectionModel::new(TableSelectionMode::MultiCell);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    cs.select(1, 1); // select `name` at display position 1
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.set_column_pinning("name", super::PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert!(
        cs.is_selected(1, 0),
        "selection must follow `name` to its new display position"
    );
    assert!(!cs.is_selected(1, 1));
}

#[test]
fn set_column_order_remaps_focused_cell_across_a_full_permutation() {
    // Three columns: id (decl 0), name (decl 1), extra (decl 2) — all
    // unpinned, so display order starts as declaration order.
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .add_column(
                Column::<Row>::new("extra", lit!("Extra"), |_row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new(lit!("…")))
                })
                .width(ColumnWidth::Fixed(40.0)),
            )
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 2); // focus `extra`, at display position 2
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        // New display order: [extra, id, name] — `extra` moves from
        // position 2 to position 0.
        tv.set_column_order(vec![
            "extra".to_string(),
            "id".to_string(),
            "name".to_string(),
        ]);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((0, 0)),
        "focus must follow `extra` to its new display position"
    );
}

// ── Edit hooks + filter signal + row drag-drop ────────────────────────────

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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col().editable(true))
            .row_height(20.0)
            .edit_triggers(super::EditTriggers::F2)
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            // Mark the focused column editable; the gate keeps F2 /
            // type-to-edit a no-op on non-editable columns.
            .add_column(id_col().editable(true))
            .add_column(name_col())
            .row_height(20.0)
            .edit_triggers(super::EditTriggers::F2 | super::EditTriggers::ANY_KEY)
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .edit_triggers(super::EditTriggers::F2)
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
fn begin_edit_resolves_before_the_view_is_mounted() {
    // Seeding a freshly constructed view with an edit target the caller
    // already holds is only possible on the builder. `display_indices` is
    // filled by `build()`, so before the fix a pre-mount call resolved
    // against an empty cache and silently did nothing.
    //
    // `name` is pinned Leading, so display order is [name, id] and the
    // correct answer for "id" is 1, not its declaration index 0 — which is
    // what makes this a test of `display_order()` rather than of a shortcut
    // that happens to agree when nothing is pinned.
    let view = TableView::new(rows(5))
        .add_column(id_col())
        .add_column(name_col().pinned(super::PinnedSide::Leading))
        .row_height(20.0);

    view.begin_edit(1, "id");
    assert_eq!(view.editing_cell_signal().get(), Some((1, 1)));

    view.end_edit();
    view.begin_edit(0, "no-such-column");
    assert_eq!(view.editing_cell_signal().get(), None);
    view.begin_edit(9999, "id");
    assert_eq!(view.editing_cell_signal().get(), None);

    // The seed survives mounting.
    view.begin_edit(1, "id");
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(view);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(tv.editing_cell_signal().get(), Some((1, 1)));
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
fn reorderable_on_list_model_lays_out_cleanly() {
    // `reorderable(true)` over a `ListModel` source: the move is
    // routed through the source's `accept_drop` (a `ListModel` reorders in
    // place). This smoke test documents the contract — the table compiles
    // and lays out cleanly with reorder enabled.
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .reorderable(true),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let info = tree.accessibility_node(table);
    assert_eq!(info.role(), Role::Grid);
}

// ── Empty state ────────────────────────────────────────────────────────────

#[test]
fn empty_view_renders_when_no_rows() {
    let model: ListModel<Row> = ListModel::new();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .empty_view(|| Box::new(TextWidget::new(lit!("nothing here")))),
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

#[test]
fn filterable_column_exposes_filter_trigger_in_a11y_tree() {
    // The filter glyph is wrapped in a `Popover` whose default name
    // is "Filter" — locating it via `find_by_label` confirms the
    // affordance is present for filterable columns.
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

#[test]
fn filter_popover_opens_via_trigger_click() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

#[test]
fn filter_popover_content_hosts_a_text_input() {
    use crate::table_view::filter::FilterPopoverContent;
    // Smoke test: the popover content materialises a real `TextInput`
    // (Role::TextInput) so AT can find it and the editor receives
    // focus when the popover opens. Keystroke→signal wiring is
    // exercised indirectly via the data-grid example and via
    // `filters_signal_is_writable` for the upstream side.
    let content = FilterPopoverContent::new("seed");
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

#[test]
fn filter_popover_editor_gains_focus_on_mouse_click() {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    use teksilo_canvas::Point;
    use teksilo_core::event::WidgetEvent;
    use teksilo_core::widget::CursorIcon;

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    fn id_c() -> Column<Row> {
        Column::<Row>::new("id", lit!("ID"), |r, _: &CellContext| {
            Box::new(TextWidget::new(lit!(r.id.to_string())))
        })
        .width(ColumnWidth::Fixed(60.0))
        .pinned(crate::table_view::column::PinnedSide::Leading)
        .sortable(true)
    }
    fn name_c() -> Column<Row> {
        Column::<Row>::new("name", lit!("Name"), |r, _: &CellContext| {
            Box::new(TextWidget::new(lit!(r.name.clone())))
        })
        .width(ColumnWidth::Flex(2.0))
        .sortable(true)
        .filterable(true)
    }

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

// ── Horizontal scroll ────────────────────────────────────────────────────

/// A 3-column fixture: `id` (Leading pinned, 60px), `name` (unpinned,
/// `middle_w` px), `extra` (Trailing pinned, 60px) — the shared
/// pinned-columns horizontal-scroll rig.
fn build_pinned_scroll_table(middle_w: f32, table_w: f32) -> (WidgetTree, WidgetId) {
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(
                Column::<Row>::new("id", lit!("ID"), |row, _: &CellContext| {
                    Box::new(TextWidget::new(lit!(row.id.to_string())))
                })
                .width(ColumnWidth::Fixed(60.0))
                .pinned(super::PinnedSide::Leading),
            )
            .add_column(
                Column::<Row>::new("name", lit!("Name"), |row, _: &CellContext| {
                    Box::new(TextWidget::new(lit!(row.name.clone())))
                })
                .width(ColumnWidth::Fixed(middle_w)),
            )
            .add_column(
                Column::<Row>::new("extra", lit!("Extra"), |_row, _: &CellContext| {
                    Box::new(TextWidget::new(lit!("x")))
                })
                .width(ColumnWidth::Fixed(60.0))
                .pinned(super::PinnedSide::Trailing),
            )
            .row_height(20.0)
            // Cell selection: this fixture drives the *column* cursor.
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(super::CellSelectionModel::new(
                TableSelectionMode::MultiCell,
            )),
    );
    tree.layout(SizeProposal {
        width: Some(table_w),
        height: Some(200.0),
    });
    (tree, table)
}

/// `n` unpinned Fixed columns of `col_w` px each, in a `table_w`-wide table
/// — the shared "content overflows the viewport, nothing pinned" rig.
fn build_wide_unpinned_table(col_w: f32, n: usize, table_w: f32) -> (WidgetTree, WidgetId) {
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let mut tv = TableView::new(model);
    for i in 0..n {
        let id = format!("c{i}");
        tv = tv.add_column(
            Column::<Row>::new(id.clone(), lit!(id.clone()), |row, _: &CellContext| {
                Box::new(TextWidget::new(lit!(row.name.clone())))
            })
            .width(ColumnWidth::Fixed(col_w)),
        );
    }
    // Cell selection: this fixture exists to drive the *column* cursor, and
    // a row-selection table has no column cursor for Home/End to move.
    let table = tree.add(
        tv.row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(super::CellSelectionModel::new(
                TableSelectionMode::MultiCell,
            )),
    );
    tree.layout(SizeProposal {
        width: Some(table_w),
        height: Some(200.0),
    });
    (tree, table)
}

fn scroll_x_of(tree: &WidgetTree, table: WidgetId) -> f32 {
    let any = tree.widget_as_any(table).unwrap();
    any.downcast_ref::<TableView<Row>>()
        .unwrap()
        .scroll_x_signal()
        .get()
}

fn max_scroll_x_of(tree: &WidgetTree, table: WidgetId) -> f32 {
    let any = tree.widget_as_any(table).unwrap();
    any.downcast_ref::<TableView<Row>>()
        .unwrap()
        .max_scroll_x_signal()
        .get()
}

fn set_scroll_x(tree: &WidgetTree, table: WidgetId, x: f32) {
    let any = tree.widget_as_any(table).unwrap();
    any.downcast_ref::<TableView<Row>>()
        .unwrap()
        .scroll_x_signal()
        .set(x);
}

#[test]
fn scroll_x_clamps_after_the_pane_widens() {
    // Three unpinned Fixed columns totalling 600px in a narrow 300px table
    // force horizontal scroll. Widening the table live shrinks
    // `max_scroll_x` below the current position — `scroll_x` must clamp
    // down with it rather than stranding the view past the content edge.
    let (mut tree, table) = build_wide_unpinned_table(200.0, 3, 300.0);
    let max = max_scroll_x_of(&tree, table);
    assert!(max > 0.0, "columns must overflow the narrow table");
    set_scroll_x(&tree, table, max);
    assert_eq!(scroll_x_of(&tree, table), max);

    tree.layout(SizeProposal {
        width: Some(700.0),
        height: Some(200.0),
    });
    assert_eq!(max_scroll_x_of(&tree, table), 0.0, "content now fits");
    assert_eq!(
        scroll_x_of(&tree, table),
        0.0,
        "scroll_x must clamp down with the new (smaller) max_scroll_x"
    );
}

#[test]
fn pinned_columns_keep_their_bands_under_scroll() {
    let (mut tree, table) = build_pinned_scroll_table(400.0, 200.0);

    let cells0 = body_row_cells(&tree, table);
    assert_eq!(cells0.len(), 3, "id, name, extra");
    let id_x0 = tree.bounds(cells0[0]).x;
    let name_x0 = tree.bounds(cells0[1]).x;
    let extra_x0 = tree.bounds(cells0[2]).x;

    // The Middle band (and only it) must clip — otherwise a scrolled `name`
    // cell could paint over the pinned `id`/`extra` bands.
    let raw_bands = tree.children(first_body_row_id(&tree, table));
    assert_eq!(raw_bands.len(), 3, "leading + middle + trailing bands");
    assert!(
        !tree.widget_clips_children(raw_bands[0]),
        "the Leading band never needs to clip"
    );
    assert!(
        tree.widget_clips_children(raw_bands[1]),
        "the Middle band must clip"
    );
    assert!(
        !tree.widget_clips_children(raw_bands[2]),
        "the Trailing band never needs to clip"
    );

    let max = max_scroll_x_of(&tree, table);
    assert!(max > 0.0);
    set_scroll_x(&tree, table, 50.0_f32.min(max));
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: Some(200.0),
    });

    let cells1 = body_row_cells(&tree, table);
    assert_eq!(
        tree.bounds(cells1[0]).x,
        id_x0,
        "Leading column never moves"
    );
    assert_eq!(
        tree.bounds(cells1[2]).x,
        extra_x0,
        "Trailing column never moves"
    );
    let name_x1 = tree.bounds(cells1[1]).x;
    assert!(
        (name_x1 - (name_x0 - 50.0)).abs() < 0.5,
        "the Middle column shifts left by exactly scroll_x: got {name_x1}, want ~{}",
        name_x0 - 50.0
    );
}

#[test]
fn header_and_body_x_offsets_agree_under_scroll() {
    let (mut tree, table) = build_pinned_scroll_table(400.0, 200.0);
    set_scroll_x(&tree, table, 37.0);
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: Some(200.0),
    });

    let header_cells = header_row_cells(&tree, table);
    let body_cells = body_row_cells(&tree, table);
    assert_eq!(header_cells.len(), body_cells.len());
    for (i, (&h, &b)) in header_cells.iter().zip(body_cells.iter()).enumerate() {
        let hx = tree.bounds(h).x;
        let bx = tree.bounds(b).x;
        assert!(
            (hx - bx).abs() < 0.01,
            "column {i}: header x {hx} must equal body x {bx}"
        );
    }
}

#[test]
fn shift_wheel_scrolls_horizontally() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{ScrollDelta, WidgetEvent};
    let (mut tree, table) = build_wide_unpinned_table(200.0, 4, 300.0);
    // Pointer in the table body (below the header).
    tree.pointer_move(Point::new(50.0, 60.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Lines { x: 0.0, y: 3.0 },
        modifiers: Modifiers::SHIFT,
    });
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: Some(200.0),
    });
    assert!(
        scroll_x_of(&tree, table) > 0.0,
        "Shift+wheel must remap a vertical-only wheel to horizontal scroll"
    );
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(
        tv.scroll_y_signal().get(),
        0.0,
        "Shift+wheel must not also scroll vertically"
    );
}

#[test]
fn ensure_col_visible_follows_focus_in_both_directions() {
    // 5 unpinned 150px columns (750px total) in a 300px table.
    let (mut tree, table) = build_wide_unpinned_table(150.0, 5, 300.0);
    focus_at(&mut tree, table, 0, 0);
    assert_eq!(scroll_x_of(&tree, table), 0.0);

    // End jumps to the last column (display index 4), off the right edge —
    // scroll_x must advance to bring it into view.
    tree.press_key(Key::End, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 4)));
    let scrolled_right = scroll_x_of(&tree, table);
    assert!(
        scrolled_right > 0.0,
        "ensure-column-visible must scroll right to reveal column 4"
    );

    // Home jumps back to column 0, off the left edge of the now-scrolled
    // viewport — scroll_x must retreat back to 0.
    tree.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
    assert_eq!(
        scroll_x_of(&tree, table),
        0.0,
        "ensure-column-visible must scroll left back to 0 for column 0"
    );
}

#[test]
fn ensure_col_visible_never_scrolls_for_a_pinned_column() {
    // `id` is Leading-pinned; jumping the cursor there must never move
    // scroll_x even while the Middle pane is scrolled.
    let (mut tree, table) = build_pinned_scroll_table(400.0, 200.0);
    let max = max_scroll_x_of(&tree, table);
    set_scroll_x(&tree, table, max);
    focus_at(&mut tree, table, 0, 1); // `name`, the Middle column
    tree.press_key(Key::Home, Modifiers::NONE); // -> column 0, `id` (Leading)
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
    assert_eq!(
        scroll_x_of(&tree, table),
        max,
        "a Leading-pinned column is always visible; scroll_x must not move"
    );
}

#[test]
fn resize_handle_hit_tests_correctly_under_scroll() {
    // Two unpinned 300px columns in a 200px table: at scroll_x = 150 the
    // first column's trailing (resize) edge sits at local x = 300 - 150 =
    // 150, inside the viewport — the drag must resize THAT column using
    // window-space pointer deltas unaffected by the scroll offset.
    use crate::styles::recipe_table_style as cp;
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let (mut tree, table) = build_wide_unpinned_table(300.0, 2, 200.0);
    let max = max_scroll_x_of(&tree, table);
    set_scroll_x(&tree, table, 150.0_f32.min(max));
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: Some(200.0),
    });

    let table_bounds = tree.bounds(table);
    let resize_x = table_bounds.x + 150.0 - cp::RESIZE_HANDLE_WIDTH * 0.5;
    let resize_y = table_bounds.y + cp::HEADER_HEIGHT * 0.5;
    let drop_x = resize_x + 20.0;

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(resize_x, resize_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(drop_x, resize_y),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(drop_x, resize_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    let widths = tv.column_widths_signal().get();
    assert!(
        widths.contains_key("c0"),
        "dragging the first column's resize handle under scroll must commit its width override, got {widths:?}"
    );
    let got = widths["c0"];
    assert!(
        (got - 320.0).abs() < 1.0,
        "expected c0 to grow to ~320px, got {got}"
    );
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// The first (lowest row_index) BODY `Role::Row` — distinguished from the
/// header (same `Role::Row`, but `Role::ColumnHeader` children once
/// band-flattened) by having at least one cell child. Either cell role
/// counts: a cell-selection table announces `Role::GridCell` so its selected
/// state reaches UIA, and a row-selection one stays on `Role::Cell`.
fn first_body_row_id(tree: &WidgetTree, root: WidgetId) -> WidgetId {
    let mut walker = vec![root];
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == Role::Row {
            let flat = flatten_through_bands(tree, tree.children(id));
            if flat.iter().any(|&c| {
                matches!(
                    tree.accessibility_node(c).role(),
                    Role::Cell | Role::GridCell
                )
            }) {
                return id;
            }
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    panic!("no body Role::Row found");
}

/// The header strip's `Role::Row` — the one whose (band-flattened) children
/// are all `Role::ColumnHeader`.
fn header_row_widget_id(tree: &WidgetTree, root: WidgetId) -> WidgetId {
    let mut walker = vec![root];
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == Role::Row {
            let flat = flatten_through_bands(tree, tree.children(id));
            if !flat.is_empty()
                && flat
                    .iter()
                    .all(|&c| tree.accessibility_node(c).role() == Role::ColumnHeader)
            {
                return id;
            }
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    panic!("no header Role::Row found");
}

fn body_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    flatten_through_bands(tree, tree.children(first_body_row_id(tree, root)))
}

fn header_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    flatten_through_bands(tree, tree.children(header_row_widget_id(tree, root)))
}

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
    flatten_through_bands(tree, tree.children(row))
}

/// Expand any AT-transparent id (no `accessibility()` override, so the
/// walker's `AccessNodeBuilder` default of `Role::Unknown`) in `ids` into
/// its own children, recursively — the pane-band wrapper `RowBand` inserts
/// under column pinning (see `table_view::body`'s module docs) never calls
/// `set_role`, so this recovers the flat cell/header-cell list regardless
/// of whether the row split into Leading/Middle/Trailing bands.
fn flatten_through_bands(tree: &WidgetTree, ids: Vec<WidgetId>) -> Vec<WidgetId> {
    let mut out = Vec::new();
    for id in ids {
        if matches!(
            tree.accessibility_node(id).role(),
            Role::GenericContainer | Role::Unknown
        ) {
            out.extend(flatten_through_bands(tree, tree.children(id)));
        } else {
            out.push(id);
        }
    }
    out
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
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::styles::{
        TableGridRecipe, TableHeaderCellConfig, TableRowConfig, TableStyle,
    };
    use teksilo_core::widget_id::WidgetId;

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
            _direction: teksilo_core::styles::SortDirection,
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
    let mut theme = teksilo_core::presets::intui::light();
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
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tv = TableView::new(rows(40))
        .add_column(id_col())
        .add_column(name_col())
        .row_height(20.0)
        .overscroll_behavior(inner);
    let inner_y = tv.scroll_y_signal().clone();
    let tv_id = tree.add(tv);
    let viewport = tree.add(FixedSize::new().width(220.0).height(120.0).child_id(tv_id));
    let filler = tree.add(
        FixedSize::new()
            .width(220.0)
            .height(300.0)
            .child(TextWidget::new(lit!(""))),
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
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
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
    assert!(
        inner_bottom > 0.0,
        "inner table should scroll down; got {inner_bottom}"
    );
    assert!(
        outer_y.get() < 0.01,
        "outer must not move while the inner absorbs"
    );

    tree.pointer_move(Point::new(110.0, 90.0));
    tree.dispatch_event(WidgetEvent::Scroll {
        delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        modifiers: Modifiers::NONE,
    });
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    assert!(
        (inner_y.get() - inner_bottom).abs() < 0.01,
        "inner stays clamped at bottom"
    );
    assert!(
        outer_y.get() > 0.01,
        "outer scrolled because the inner chained the boundary"
    );
}

#[test]
fn keyboard_selection_chases_outer_scroll_area() {
    // A 200px TableView (20px rows, 20 rows) whose lower rows are below a 100px
    // outer ScrollArea's fold. Keyboard row nav keeps focus on the container
    // (rows/cells aren't focusable), so the focus-driven follow can't reveal the
    // focused row — ctx.ensure_visible must.
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, VStack};
    use teksilo_canvas::Point;
    use teksilo_core::event::{Key, Modifiers};

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tv = TableView::new(rows(20))
        .add_column(id_col())
        .add_column(name_col())
        .row_height(20.0);
    let tv_id = tree.add(tv);
    let tv_box = tree.add(FixedSize::new().width(220.0).height(200.0).child_id(tv_id));
    let filler = tree.add(
        FixedSize::new()
            .width(220.0)
            .height(200.0)
            .child(TextWidget::new(lit!(""))),
    );
    let outer_content = tree.add(VStack::new().add_child(tv_box).add_child(filler));
    let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let _outer = tree.add(outer);
    let sz = SizeProposal {
        width: Some(220.0),
        height: Some(100.0),
    };
    tree.layout(sz);

    tree.focus(tv_id);
    tree.layout(sz);
    outer_y.set(0.0);
    tree.layout(sz);
    assert!(outer_y.get().abs() < 0.01, "reset outer to top");
    // Place the pointer so the table's own focus/hover state is sane.
    tree.pointer_move(Point::new(110.0, 50.0));

    for _ in 0..20 {
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
    }
    tree.layout(sz);
    assert!(
        outer_y.get() > 0.01,
        "row nav below the fold must scroll the enclosing ScrollArea (got {})",
        outer_y.get()
    );
}

#[test]
fn nested_table_contain_blocks_chaining() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
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
    assert!(
        outer_y.get() < 0.01,
        "Contain must prevent chaining: outer stays put"
    );
}

// ── RTL (right-to-left) column layout ───────────────────────────────────────

use teksilo_core::environment::LayoutDirection;

/// Re-lay a freshly built table under RTL at the same 400×200 viewport.
fn relayout_rtl(tree: &mut WidgetTree) {
    tree.set_layout_direction(LayoutDirection::RightToLeft);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
}

#[test]
fn rtl_reverses_column_x_order_with_scrollbar() {
    // 50 rows → vertical scrollbar present. Body = 400 − 12 = 388;
    // id Fixed=60, name Flex=328. Under RTL the scrollbar moves to the
    // physical left, so the band starts at x=12 and columns fill it from
    // the right: id at 400−60=340, name at 340−328=12.
    let (mut tree, table, _) = build_table(50);
    relayout_rtl(&mut tree);

    let cells = first_visible_row_cells(&tree, table);
    assert_eq!(cells.len(), 2);
    let id_cell = tree.bounds(cells[0]);
    let name_cell = tree.bounds(cells[1]);

    // Widths are direction-neutral.
    assert!((id_cell.width - 60.0).abs() < 0.5, "id w {}", id_cell.width);
    assert!(
        (name_cell.width - 328.0).abs() < 0.5,
        "name w {}",
        name_cell.width
    );
    // Display column 0 (id) is now physically rightmost.
    assert!(
        id_cell.x > name_cell.x,
        "RTL: id.x={} should be right of name.x={}",
        id_cell.x,
        name_cell.x
    );
    assert!((id_cell.x - 340.0).abs() < 0.5, "id.x {}", id_cell.x);
    // The band shifted right by SCROLLBAR_THICKNESS (scrollbar now on the left).
    assert!(
        (name_cell.x - 12.0).abs() < 0.5,
        "name.x {} (band should start at SCROLLBAR_THICKNESS)",
        name_cell.x
    );
}

#[test]
fn rtl_right_anchors_content_without_scrollbar() {
    // 5 rows → no scrollbar. Body = 400; id=60, name Flex=340. RTL fills
    // from the right edge: id at 340, name at 0 (band_left = 0).
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    relayout_rtl(&mut tree);

    let cells = first_visible_row_cells(&tree, table);
    let id_cell = tree.bounds(cells[0]);
    let name_cell = tree.bounds(cells[1]);
    assert!((id_cell.x - 340.0).abs() < 0.5, "id.x {}", id_cell.x);
    assert!(
        name_cell.x.abs() < 0.5,
        "name.x {} (content right-anchored, no scrollbar gap)",
        name_cell.x
    );
}

#[test]
fn ltr_keeps_columns_left_to_right_control() {
    // Control for the RTL tests: same table, LTR, id leftmost.
    let (tree, table, _) = build_table(50);
    let cells = first_visible_row_cells(&tree, table);
    let id_cell = tree.bounds(cells[0]);
    let name_cell = tree.bounds(cells[1]);
    assert!(
        id_cell.x < name_cell.x,
        "LTR: id.x={} should be left of name.x={}",
        id_cell.x,
        name_cell.x
    );
    assert!(id_cell.x.abs() < 0.5, "id.x {}", id_cell.x);
}

#[test]
fn rtl_swaps_arrow_key_column_navigation() {
    // Columns run right-to-left, so ArrowLeft moves to the visually-left
    // column = the higher display index, and ArrowRight moves back.
    let (mut tree, table, _) = build_table(10);
    tree.set_layout_direction(LayoutDirection::RightToLeft);
    focus_at(&mut tree, table, 0, 0);

    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((0, 1)),
        "RTL ArrowLeft should advance to the next display column"
    );
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        read_focused_cell(&tree, table),
        Some((0, 0)),
        "RTL ArrowRight should step back toward column 0"
    );
    // Home/End move rows here (this table selects rows), so they leave the
    // column alone in either direction — there is nothing for RTL to mirror.
    tree.press_key(Key::End, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((9, 0)));
    tree.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
}

#[test]
fn rtl_leaves_home_and_end_logical_in_a_cell_grid() {
    use teksilo_core::environment::LayoutDirection;
    let (mut tree, table, _) = build_cell_table(5);
    tree.set_layout_direction(LayoutDirection::RightToLeft);
    focus_at(&mut tree, table, 0, 0);
    // Leading is column 0 whichever way the columns run, so Home/End are the
    // one pair the mirroring must *not* touch.
    tree.press_key(Key::End, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 1)));
    tree.press_key(Key::Home, Modifiers::NONE);
    assert_eq!(read_focused_cell(&tree, table), Some((0, 0)));
}

#[test]
fn rtl_live_resize_tracks_without_drift() {
    // Regression guard for the RTL-specific Live-resize drift: under RTL
    // the resize handle is at a column's physical-LEFT edge, and widening
    // moves that edge left. A cell-local drag anchor would shift with it
    // mid-drag (the relayout below is the exact trigger); the window-space
    // anchor keeps the delta honest. With the old cell-local anchor this
    // test would read ~80 instead of ~100.
    use crate::styles::recipe_table_style as cp;
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col()) // Fixed(60), display 0 → physical RIGHT under RTL
            .add_column(name_col())
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    relayout_rtl(&mut tree);

    // No scrollbar: id occupies window x [340, 400]; its RTL resize handle
    // is just inside the physical-left edge at x≈340.
    let resize_handle = cp::RESIZE_HANDLE_WIDTH;
    let down_x = 340.0 + resize_handle * 0.5;
    let down_y = cp::HEADER_HEIGHT * 0.5;

    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(down_x, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    // Drag left in two steps with a relayout between — the relayout moves
    // id's physical-left edge, which is what used to corrupt the delta.
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(down_x - 20.0, down_y),
    });
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(down_x - 40.0, down_y),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(down_x - 40.0, down_y),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    let id_w = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.column_widths_signal()
            .get()
            .get("id")
            .copied()
            .unwrap_or(0.0)
    };
    assert!(
        (id_w - 100.0).abs() < 6.0,
        "RTL drag-left by 40px should widen id from 60 to ~100 (no drift); got {}",
        id_w
    );
}

// ── Variable row heights ───────────────────────────────────────────────────

/// Fixed-size leaf for height-driven cell delegates.
#[derive(Debug)]
struct FixedLeaf(f32, f32);
impl teksilo_core::widget::Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        teksilo_canvas::Size::new(self.0, self.1).into()
    }
}

/// Collect the (y, height) bounds of the materialised `Role::Row`
/// widgets, sorted by y.
fn row_spans(tree: &WidgetTree, root: WidgetId) -> Vec<(f32, f32)> {
    let mut walker = vec![root];
    let mut spans = Vec::new();
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == Role::Row {
            let b = tree.bounds(id);
            spans.push((b.y, b.height));
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    spans
}

#[test]
fn exact_row_height_fn_positions_rows() {
    let heights = [60.0_f32, 20.0, 40.0];
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .show_header(false)
            .row_height_fn(move |i| heights[i]),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    let spans = row_spans(&tree, table);
    assert_eq!(spans.len(), 3);
    assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 60.0).abs() < 0.01);
    assert!((spans[1].0 - 60.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
    assert!((spans[2].0 - 80.0).abs() < 0.01 && (spans[2].1 - 40.0).abs() < 0.01);
}

#[test]
fn auto_row_height_measures_tallest_cell() {
    // Column A cells are 30 px tall, column B cells 44 px — the row must
    // measure to the tallest cell (44), not the 20 px estimate.
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let col_a = Column::<Row>::new("a", lit!("A"), |_row, _: &CellContext| {
        Box::new(FixedLeaf(50.0, 30.0))
    })
    .width(ColumnWidth::Fixed(60.0));
    let col_b = Column::<Row>::new("b", lit!("B"), |_row, _: &CellContext| {
        Box::new(FixedLeaf(50.0, 44.0))
    })
    .width(ColumnWidth::Flex(1.0));
    let table = tree.add(
        TableView::new(model)
            .add_column(col_a)
            .add_column(col_b)
            .show_header(false)
            .auto_row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    let spans = row_spans(&tree, table);
    assert!(
        (spans[0].1 - 44.0).abs() < 0.01,
        "row height must be the tallest cell, got {}",
        spans[0].1
    );
    assert!(
        (spans[1].0 - 44.0).abs() < 0.01,
        "row 1 must sit below the measured row 0, got {}",
        spans[1].0
    );
}

#[test]
fn page_down_with_variable_heights_lands_on_offset_row() {
    use teksilo_core::event::{Key, Modifiers};
    // Heights alternate 20 / 60 px (tops 0, 20, 80, 100, 160, 180, 240…).
    // Viewport 200 from row 0 → the row containing y = 200 is row 5
    // (top 180), NOT row 10 that a fixed rows-per-page would produce.
    let model = rows(100);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .show_header(false)
            .row_height_fn(|i| if i % 2 == 0 { 20.0 } else { 60.0 }),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::PageDown, Modifiers::NONE);
    let after = read_focused_cell(&tree, table).unwrap();
    assert_eq!(
        after.0, 5,
        "PageDown must land on the row one viewport below (offset-driven)"
    );
}

#[test]
fn append_through_sort_filter_keeps_measured_prefix() {
    use teksilo_data::SortFilterListModel;
    // Auto-measure through a SortFilterListModel: the proxy emits
    // blanket `Reset`s, but its `first_changed_index` side-channel must
    // keep the measured prefix on append — row 1 stays at the measured
    // 30 px, it doesn't snap back to the 50 px estimate.
    let model = rows(4);
    let proxy = SortFilterListModel::new(model.clone());
    let col = Column::<Row>::new("a", lit!("A"), |_row, _: &CellContext| {
        Box::new(FixedLeaf(50.0, 30.0))
    })
    .width(ColumnWidth::Flex(1.0));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::from_source(proxy)
            .add_column(col)
            .show_header(false)
            .auto_row_height(50.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    model.push(Row {
        id: 99,
        name: "new".into(),
    });
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    let spans = row_spans(&tree, table);
    assert_eq!(spans.len(), 5);
    assert!(
        (spans[1].0 - 30.0).abs() < 0.01,
        "measured prefix must survive an append through SortFilterListModel, got y {}",
        spans[1].0
    );
}

#[test]
fn row_drop_insertion_with_variable_heights() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    // Heights [40, 10, 40, 40, 40]: dropping at y = 35 (lower half of
    // the tall row 0) must insert before row 1 — the naive midpoint
    // formula would skip past the short row 1.
    let heights = [40.0_f32, 10.0, 40.0, 40.0, 40.0];
    let model = rows(5);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model.clone())
            .add_column(id_col())
            .add_column(name_col())
            .show_header(false)
            .row_height_fn(move |i| heights.get(i).copied().unwrap_or(40.0))
            .reorderable(true),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // Drag row 4 (id 4, spans 130..170) up to y = 35.
    let from = Point::new(150.0, 150.0);
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(from.x + 10.0, from.y),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(from.x, 35.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(from.x, 35.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    // Insertion before row 1: ids become [0, 4, 1, 2, 3].
    let ids: Vec<u32> = (0..model.len())
        .map(|i| model.with_item(i, |r| r.id).unwrap())
        .collect();
    assert_eq!(ids, vec![0, 4, 1, 2, 3]);
}

#[test]
fn reorder_drag_routes_to_source_accept_drop_without_mutating() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::Point;
    use teksilo_core::ObserverHandle;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    use teksilo_data::{
        DataChange, DragEligibility, DragSource, DropCommit, DropPosition, DropQuery, DropResponse,
        ListDataSource,
    };

    // The redesign's controlled path: a reorderable table routes the drop to
    // the SOURCE's accept_drop. An externally-owned source CAPTURES the
    // resolved move and returns true WITHOUT mutating its own store — the
    // old `on_reorder` hook, now expressed through the source trait.
    struct CapturingSource {
        items: Vec<Row>,
        captured: Rc<RefCell<Vec<(usize, usize)>>>,
    }
    impl ListDataSource for CapturingSource {
        type Item = Row;
        type Key = usize;
        fn len(&self) -> usize {
            self.items.len()
        }
        fn with_item<R>(&self, i: usize, f: impl FnOnce(&Row) -> R) -> Option<R> {
            self.items.get(i).map(f)
        }
        fn key_at(&self, i: usize) -> Option<usize> {
            (i < self.items.len()).then_some(i)
        }
        fn observe_changes(&self, _f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
            ObserverHandle::new(Rc::new(()) as Rc<dyn std::any::Any>, 0, Rc::new(|_| {}))
        }
        fn drag(&self, _k: &usize) -> DragEligibility {
            DragEligibility::CanDrag
        }
        fn can_accept(&self, q: &DropQuery<'_, usize>) -> DropResponse {
            match &q.source {
                DragSource::SameView { .. } if q.position != DropPosition::Into => {
                    DropResponse::Accept
                }
                _ => DropResponse::Reject,
            }
        }
        fn accept_drop(&self, c: DropCommit<'_, usize>) -> bool {
            let DragSource::SameView { key: from } = c.source else {
                return false;
            };
            let target = c.target;
            let shift = if from < target { 1 } else { 0 };
            let to = match c.position {
                DropPosition::Before => target.saturating_sub(shift),
                DropPosition::After => (target + 1).saturating_sub(shift),
                DropPosition::Into => return false,
            };
            // Controlled: capture the resolved move, do NOT mutate `items`.
            self.captured.borrow_mut().push((from, to));
            true
        }
    }

    let captured: Rc<RefCell<Vec<(usize, usize)>>> = Rc::new(RefCell::new(Vec::new()));
    let items: Vec<Row> = (0..5)
        .map(|i| Row {
            id: i,
            name: format!("row {i}"),
        })
        .collect();
    let heights = [40.0_f32, 10.0, 40.0, 40.0, 40.0];
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _table = tree.add(
        TableView::from_source(CapturingSource {
            items,
            captured: captured.clone(),
        })
        .add_column(id_col())
        .add_column(name_col())
        .show_header(false)
        .row_height_fn(move |i| heights.get(i).copied().unwrap_or(40.0))
        .reorderable(true),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // Drag row 4 (spans 130..170) up to y = 35 → insert before row 1.
    let from = Point::new(150.0, 150.0);
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: from,
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(from.x + 10.0, from.y),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(from.x, 35.0),
    });
    tree.dispatch_event(WidgetEvent::PointerUp {
        position: Point::new(from.x, 35.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::NONE,
    });

    assert_eq!(
        *captured.borrow(),
        vec![(4, 1)],
        "the drop is routed to the source's accept_drop with the resolved move"
    );
}
#[test]
fn auto_row_height_totals_settle_after_measurement() {
    // Regression: the root computes `max_scroll_y` before the pane's
    // measure pass. Without the pane's total-refresh poke, rows that
    // measure TALLER than the estimate left the totals stale forever —
    // the bottom of the content was unreachable and the last visible
    // row sat cut at the viewport edge.
    let model = rows(50);
    let col = Column::<Row>::new("a", lit!("A"), |_row, _: &CellContext| {
        Box::new(FixedLeaf(50.0, 44.0))
    })
    .width(ColumnWidth::Flex(1.0));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(col)
            .show_header(false)
            .auto_row_height(30.0),
    );
    for _ in 0..5 {
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
    }
    let max_scroll = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.max_scroll_y_signal().get()
    };
    // Realized rows measured 44 px; unrealized ones still estimate 30.
    // The total must at least exceed the all-estimate figure and the
    // scroll range must reach every measured row realized so far.
    assert!(
        max_scroll > 1300.0 + 0.5,
        "max_scroll must pick up measured heights, got {max_scroll} (stale = 1300)"
    );

    // Scroll to the (corrected) bottom and let realization settle: the
    // last row must end exactly at the viewport bottom — fully
    // reachable, nothing cut beyond reach.
    let scroll = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.scroll_y_signal().clone()
    };
    // Each measure → total-refresh → root re-place cycle spans two
    // layout passes; scrolling to the (growing) bottom extends the
    // measured region step by step — loop until the range stabilizes,
    // exactly like a user holding the wheel / thumb across frames.
    // Rows jumped over stay at their estimate (virtualization never
    // measures unrealized rows), so the total converges to
    // measured-so-far + estimates, not to the fully-measured figure.
    let read_max = |tree: &WidgetTree| {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.max_scroll_y_signal().get()
    };
    let mut settled = false;
    for _ in 0..60 {
        let max_before = read_max(&tree);
        scroll.set(max_before);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        // Compare AFTER the layout — the total-refresh poke updates the
        // max during the pass.
        let max_after = read_max(&tree);
        if (max_after - max_before).abs() < 0.01 && (scroll.get() - max_after).abs() < 0.01 {
            settled = true;
            break;
        }
    }
    assert!(settled, "scroll range must converge");

    // The user-facing invariant the total-refresh poke restores: at max
    // scroll the LAST row ends exactly at the viewport bottom — fully
    // reachable. (Pre-fix, the stale total left ~700 px of measured
    // content beyond the reachable range, with the last visible row's
    // lines cut at the viewport edge forever.)
    let max_scroll = {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.max_scroll_y_signal().get()
    };
    assert!(
        (scroll.get() - max_scroll).abs() < 0.01,
        "scroll must settle at max"
    );
    let spans = row_spans(&tree, table);
    let last_bottom = spans.last().map(|(y, h)| y + h).unwrap();
    assert!(
        (last_bottom - 200.0).abs() < 1.0,
        "at max scroll the last row must end at the viewport bottom, got {last_bottom}"
    );
}

#[test]
fn root_painted_row_decorations_clip_to_widget_bounds() {
    // Regression: alt-row stripes / grid lines / selection bands are
    // painted by the TableView root itself — `clips_children` doesn't
    // cover a widget's own paint, so the partially visible bottom
    // row's full-height stripe and its grid line bled past the
    // table's bottom edge ("the colored line overflows, not the
    // text"). The root paint now wraps the body band in a
    // SetClip/ClearClip pair.
    use teksilo_canvas::DrawCommand;

    let model = rows(50);
    let sel = SelectionModel::new(SelectionMode::Multi);
    sel.select(4); // the partial bottom row → selection band too
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .show_header(false)
            .row_height_fn(|_| 44.0) // 200 / 44 → bottom row is partial
            .alternating_rows(true)
            .grid_lines(super::GridLines::Horizontal)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let frame = tree.render();
    let mut clips: Vec<[f32; 4]> = Vec::new();
    let mut saw_raw_overflow = false;
    for cmd in &frame.draw_order {
        match cmd {
            DrawCommand::SetClip(r) => clips.push([r.x, r.y, r.width, r.height]),
            DrawCommand::ClearClip => {
                clips.pop();
            }
            DrawCommand::Decoration(i) => {
                let rect = frame.decorations[*i].rect;
                let raw_bottom = rect[1] + rect[3];
                // Effective visible bottom = rect ∩ all active clips.
                let mut bottom = raw_bottom;
                for c in &clips {
                    bottom = bottom.min(c[1] + c[3]);
                }
                if raw_bottom > 200.5 {
                    saw_raw_overflow = true;
                }
                assert!(
                    bottom <= 200.5,
                    "a painted rect's visible region extends below the table \
                     (raw rect {rect:?}, effective bottom {bottom})"
                );
            }
            _ => {}
        }
    }
    // Sanity: the scenario must actually exercise the bug — at least
    // one raw rect (the partial bottom row's stripe / grid line)
    // extends past the bounds and relies on the clip.
    assert!(
        saw_raw_overflow,
        "expected a partial bottom-row decoration spanning past the bounds"
    );
}

#[test]
fn lazy_loading_rows_render_placeholder_cells_and_request_the_window() {
    // A windowed table source with nothing resident: every visible row is
    // `Loading`, so the body pane must render placeholder cells (not skip the
    // rows) and the table must nudge the source to load the realized window.
    use std::cell::RefCell;
    use std::ops::Range;
    use std::rc::Rc;
    use teksilo_core::ObserverHandle;
    use teksilo_data::{DataChange, ListDataSource, RowState};

    struct Windowed {
        total: usize,
        requested: Rc<RefCell<Vec<Range<usize>>>>,
    }
    impl ListDataSource for Windowed {
        type Item = Row;
        type Key = usize;
        fn len(&self) -> usize {
            self.total
        }
        fn with_item<R>(&self, _i: usize, _f: impl FnOnce(&Row) -> R) -> Option<R> {
            None // nothing resident yet
        }
        fn key_at(&self, i: usize) -> Option<usize> {
            (i < self.total).then_some(i)
        }
        fn row_state(&self, _i: usize) -> RowState {
            RowState::Loading
        }
        fn request_window(&self, range: Range<usize>) {
            self.requested.borrow_mut().push(range);
        }
        fn observe_changes(&self, _f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
            ObserverHandle::new(Rc::new(()) as Rc<dyn std::any::Any>, 0, Rc::new(|_| {}))
        }
    }

    let requested = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::from_source(Windowed {
            total: 1000,
            requested: requested.clone(),
        })
        .add_column(id_col())
        .add_column(name_col())
        .show_header(false)
        .row_height(30.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // The body pane is the table's first child (header suppressed). 300px /
    // 30px = 10 visible + buffer → the loading rows realize as placeholder
    // row widgets, NOT skipped.
    let body_pane = tree.children(table)[0];
    let placeholder_rows = tree.children(body_pane).len();
    assert!(
        placeholder_rows >= 10,
        "loading rows must render as placeholders, got {placeholder_rows}"
    );
    // And the source was asked to load the realized window.
    assert!(
        !requested.borrow().is_empty(),
        "request_window must be called for the visible range"
    );
}

#[test]
fn selection_band_desaturates_when_window_inactive() {
    use teksilo_canvas::DecorationKind;
    use teksilo_tokens::SurfaceRole;

    let model = rows(5);
    let sel = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    sel.select(0); // even row → no AltRow stripe to confuse the colour match
    tree.focus(table); // view_focused = true

    let colors = teksilo_core::presets::intui::light().colors;
    let active = SurfaceRole::Selected.resolve(&colors).to_array();
    let inactive = SurfaceRole::SelectedInactive.resolve(&colors).to_array();
    assert_ne!(active, inactive);

    let band_colors = |tree: &mut WidgetTree| -> Vec<[f32; 4]> {
        tree.render()
            .decorations
            .iter()
            .filter(|d| matches!(d.kind, DecorationKind::WidgetBackground))
            .map(|d| d.color)
            .collect()
    };

    // Active window + focused view: the vivid Selected band is painted.
    let bands = band_colors(&mut tree);
    assert!(
        bands.contains(&active),
        "active window: vivid Selected band expected, got {bands:?}"
    );
    assert!(
        !bands.contains(&inactive),
        "active window: no muted band expected"
    );

    // Window blur: the band desaturates to SelectedInactive even though the
    // view keeps keyboard focus.
    tree.set_window_active(false);
    let bands = band_colors(&mut tree);
    assert!(
        bands.contains(&inactive),
        "inactive window: muted SelectedInactive band expected, got {bands:?}"
    );
    assert!(
        !bands.contains(&active),
        "inactive window: no vivid band expected"
    );

    // Reactivate: vivid band returns.
    tree.set_window_active(true);
    let bands = band_colors(&mut tree);
    assert!(
        bands.contains(&active),
        "reactivated window: vivid band returns, got {bands:?}"
    );
}

// ── Column resize grip ─────────────────────────────────────────────────────
//
// The grip straddles a column divider — `RESIZE_HANDLE_WIDTH` px into the cell
// on *each* side of it — so a cell owns the resize for its own trailing
// boundary AND for the one it shares with its predecessor. These tests pin
// that contract down from both halves, in both directions, plus the clamping
// and lifecycle rules that used to let a drag decouple from the pointer.

mod resize_grip {
    use super::*;
    use crate::styles::recipe_table_style as cp;
    use crate::table_view::column::{ColumnResizePolicy, PinnedSide};
    use teksilo_canvas::Point;
    use teksilo_core::environment::LayoutDirection;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    fn down(tree: &mut WidgetTree, x: f32, y: f32) {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(x, y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }
    fn moved(tree: &mut WidgetTree, x: f32, y: f32) {
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(x, y),
        });
    }
    fn up(tree: &mut WidgetTree, x: f32, y: f32) {
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(x, y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    fn overrides(tree: &WidgetTree, table: WidgetId) -> std::collections::HashMap<String, f32> {
        let any = tree.widget_as_any(table).unwrap();
        any.downcast_ref::<TableView<Row>>()
            .unwrap()
            .column_widths_signal()
            .get()
    }

    fn relayout(tree: &mut WidgetTree) {
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
    }

    /// `id` Fixed(60) then `name` Flex(1) at a 400 px viewport with no
    /// scrollbars: `id` spans `[0, 60]`, `name` spans `[60, 400]`, and their
    /// divider sits at x = 60.
    fn two_column_table() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TableView::new(rows(5))
                .add_column(id_col())
                .add_column(name_col())
                .row_height(20.0)
                .show_internal_scrollbars(false),
        );
        relayout(&mut tree);
        (tree, table)
    }

    #[test]
    fn grip_reaches_into_the_next_column_so_a_one_pixel_overshoot_still_resizes() {
        // Regression: the grip used to live entirely inside the cell that owns
        // the divider (`local_x > cell_w - 4`). Aiming at the seam and landing
        // one pixel late put the press in the NEXT cell's label region, where
        // it cycled the sort or started a column-reorder drag — the divider
        // was only grabbable from one side.
        let (mut tree, table) = two_column_table();
        let y = cp::HEADER_HEIGHT * 0.5;
        // x = 61 is one pixel PAST the id/name divider, i.e. inside `name`.
        down(&mut tree, 61.0, y);
        moved(&mut tree, 91.0, y);
        up(&mut tree, 91.0, y);

        let w = overrides(&tree, table);
        assert!(
            (w.get("id").copied().unwrap_or(0.0) - 90.0).abs() < 0.5,
            "grabbing the divider from the trailing column must widen `id` \
             from 60 to 90; got {w:?}"
        );
        assert!(
            !w.contains_key("name"),
            "the pressed cell's own column must not move; got {w:?}"
        );
    }

    #[test]
    fn grip_still_resizes_the_owning_column_from_its_own_trailing_edge() {
        // The other half of the same divider — unchanged behaviour, asserted
        // so widening the grip can't silently swap which column moves.
        let (mut tree, table) = two_column_table();
        let y = cp::HEADER_HEIGHT * 0.5;
        down(&mut tree, 59.0, y);
        moved(&mut tree, 89.0, y);
        up(&mut tree, 89.0, y);

        let w = overrides(&tree, table);
        assert!(
            (w.get("id").copied().unwrap_or(0.0) - 90.0).abs() < 0.5,
            "got {w:?}"
        );
    }

    #[test]
    fn cursor_turns_to_col_resize_on_the_far_side_of_the_divider_too() {
        use teksilo_core::widget::CursorIcon;
        let (mut tree, _table) = two_column_table();
        let y = cp::HEADER_HEIGHT * 0.5;
        moved(&mut tree, 61.0, y);
        assert_eq!(
            tree.current_cursor(),
            CursorIcon::ColResize,
            "the grip's far half must advertise itself like the near half"
        );
    }

    #[test]
    fn drag_clamps_to_max_width_so_the_stored_override_matches_what_renders() {
        // The solver re-clamps every override to `[min_width, max_width]`, so
        // a drag that wrote the raw (unclamped) width left
        // `column_widths_signal` — a public handle apps read back and persist
        // — holding a width the table never renders.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TableView::new(rows(5))
                .add_column(id_col())
                .add_column(name_col().max_width(200.0))
                .add_column(
                    Column::<Row>::new("extra", lit!("Extra"), |row, _: &CellContext| {
                        Box::new(TextWidget::new(lit!(row.name.clone())))
                    })
                    .width(ColumnWidth::Flex(1.0)),
                )
                .row_height(20.0)
                .show_internal_scrollbars(false),
        );
        relayout(&mut tree);
        // id 60 fixed; leftover 340 split 1:1 → name 170, extra 170.
        // name spans [60, 230]; grab its trailing edge.
        let y = cp::HEADER_HEIGHT * 0.5;
        down(&mut tree, 229.0, y);
        moved(&mut tree, 429.0, y);
        relayout(&mut tree);
        let w = overrides(&tree, table);
        assert!(
            (w.get("name").copied().unwrap_or(0.0) - 200.0).abs() < 0.5,
            "a drag 200 px past the 200 px cap must store the cap, not the raw \
             overshoot; got {w:?}"
        );
        up(&mut tree, 429.0, y);
        let w = overrides(&tree, table);
        assert!(
            (w.get("name").copied().unwrap_or(0.0) - 200.0).abs() < 0.5,
            "and the committed value must agree; got {w:?}"
        );
    }

    #[test]
    fn a_column_narrower_than_the_grip_keeps_a_central_band_for_sorting() {
        // With a fixed 4 dp zone measured from the trailing edge, a 4 px-wide
        // column was 100 % resize zone: every press anywhere on its header
        // started a resize, so sort and reorder became unreachable and the
        // column could never be recovered except through the very gesture that
        // had swallowed everything.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TableView::new(rows(5))
                .add_column(
                    Column::<Row>::new("flag", lit!("Flag"), |_row, _: &CellContext| {
                        Box::new(TextWidget::new(lit!("!")))
                    })
                    .width(ColumnWidth::Fixed(4.0))
                    .min_width(2.0)
                    .sortable(true),
                )
                .add_column(name_col())
                .row_height(20.0)
                .show_internal_scrollbars(false),
        );
        relayout(&mut tree);
        let y = cp::HEADER_HEIGHT * 0.5;
        // Dead centre of the 4 px column.
        down(&mut tree, 2.0, y);
        up(&mut tree, 2.0, y);

        let sort = {
            let any = tree.widget_as_any(table).unwrap();
            any.downcast_ref::<TableView<Row>>()
                .unwrap()
                .sort_signal()
                .get()
        };
        assert_eq!(
            sort,
            Some(("flag".to_string(), SortDirection::Ascending)),
            "a click in the middle of a very narrow column must still sort it"
        );
        assert!(
            !overrides(&tree, table).contains_key("flag"),
            "and must not have been swallowed as a resize"
        );
    }

    #[test]
    fn the_grip_does_not_reach_across_a_pane_seam() {
        // The first Middle-pane column's leading edge abuts the pinned Leading
        // pane. That boundary is a pane seam, not a column divider: under a
        // nonzero `scroll_x` the column on its far side is not the one
        // visually adjacent to it, so the grip must stop there. The pinned
        // column stays resizable from its OWN edge.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TableView::new(rows(5))
                .add_column(id_col().pinned(PinnedSide::Leading))
                .add_column(name_col())
                .row_height(20.0)
                .show_internal_scrollbars(false),
        );
        relayout(&mut tree);
        let y = cp::HEADER_HEIGHT * 0.5;

        // One pixel into the Middle pane — must NOT resize the pinned column.
        down(&mut tree, 61.0, y);
        moved(&mut tree, 91.0, y);
        up(&mut tree, 91.0, y);
        assert!(
            !overrides(&tree, table).contains_key("id"),
            "a press across the pane seam must not resize the pinned column"
        );

        // Its own trailing edge still works.
        down(&mut tree, 59.0, y);
        moved(&mut tree, 89.0, y);
        up(&mut tree, 89.0, y);
        let w = overrides(&tree, table);
        assert!(
            (w.get("id").copied().unwrap_or(0.0) - 90.0).abs() < 0.5,
            "the pinned column resizes from its own edge; got {w:?}"
        );
    }

    #[test]
    fn rtl_grip_straddles_the_divider_and_drag_direction_inverts() {
        // Under RTL the display order runs right-to-left, so `id` (display
        // slot 0) sits at [340, 400] and `name` at [0, 340]; their divider is
        // at x = 340 and the far half of the grip lives at `name`'s physical
        // RIGHT edge. Dragging that divider physically LEFT widens `id`.
        let (mut tree, table) = two_column_table();
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        relayout(&mut tree);

        let y = cp::HEADER_HEIGHT * 0.5;
        down(&mut tree, 339.0, y);
        moved(&mut tree, 309.0, y);
        up(&mut tree, 309.0, y);

        let w = overrides(&tree, table);
        assert!(
            (w.get("id").copied().unwrap_or(0.0) - 90.0).abs() < 0.5,
            "RTL: dragging the divider left from the trailing column must \
             widen `id` from 60 to 90; got {w:?}"
        );
    }

    #[test]
    fn on_release_policy_shows_a_guide_line_and_commits_only_on_pointer_up() {
        // `OnRelease` moves nothing until the button comes up, so without the
        // guide line the entire drag had zero positional feedback — the user
        // could not tell whether the gesture had even been recognised.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TableView::new(rows(5))
                .add_column(id_col())
                .add_column(name_col())
                .row_height(20.0)
                .column_resize_policy(ColumnResizePolicy::OnRelease)
                .show_internal_scrollbars(false),
        );
        relayout(&mut tree);
        let y = cp::HEADER_HEIGHT * 0.5;

        down(&mut tree, 398.0, y);
        moved(&mut tree, 428.0, y);

        let preview = {
            let any = tree.widget_as_any(table).unwrap();
            any.downcast_ref::<TableView<Row>>()
                .unwrap()
                .resize_preview_x
                .get()
        };
        assert!(
            preview.is_some_and(|x| (x - 430.0).abs() < 0.5),
            "the guide must track the prospective divider (400 + 30); got {preview:?}"
        );
        assert!(
            overrides(&tree, table).is_empty(),
            "OnRelease must not commit mid-drag"
        );

        up(&mut tree, 428.0, y);
        let w = overrides(&tree, table);
        assert!(
            (w.get("name").copied().unwrap_or(0.0) - 370.0).abs() < 0.5,
            "commit on release; got {w:?}"
        );
        let preview = {
            let any = tree.widget_as_any(table).unwrap();
            any.downcast_ref::<TableView<Row>>()
                .unwrap()
                .resize_preview_x
                .get()
        };
        assert!(preview.is_none(), "the guide must clear on release");
    }

    #[test]
    fn losing_the_window_mid_drag_abandons_the_resize_instead_of_ghost_dragging() {
        // The OS delivers no PointerUp to a window that lost focus with the
        // button down. The shared drag state used to outlive the gesture, so
        // the next bare PointerMove — no button held — kept dragging the
        // column.
        let (mut tree, table) = two_column_table();
        let y = cp::HEADER_HEIGHT * 0.5;
        down(&mut tree, 398.0, y);
        moved(&mut tree, 428.0, y);
        relayout(&mut tree);
        let during = overrides(&tree, table)
            .get("name")
            .copied()
            .unwrap_or_default();
        assert!((during - 370.0).abs() < 0.5, "sanity: got {during}");

        tree.set_window_active(false);
        moved(&mut tree, 528.0, y);
        relayout(&mut tree);
        let after = overrides(&tree, table)
            .get("name")
            .copied()
            .unwrap_or_default();
        assert!(
            (after - during).abs() < 0.001,
            "a move after deactivation must not keep resizing; {during} → {after}"
        );
    }

    #[test]
    fn a_resizable_column_header_advertises_increment_and_decrement_to_at() {
        // Drag-resize is otherwise pointer-only — unreachable by a screen
        // reader, switch access, or the automation MCP, all of which act
        // through AccessKit.
        use teksilo_core::accesskit::Action;
        let (tree, table) = two_column_table();
        let mut walker = vec![table];
        let mut header = None;
        while let Some(n) = walker.pop() {
            if tree.accessibility_node(n).role() == Role::ColumnHeader {
                header = Some(n);
                break;
            }
            for c in tree.children(n) {
                walker.push(c);
            }
        }
        let header = header.expect("a column header must exist");
        let node = tree.accessibility_node(header);
        let actions = node.actions().to_vec();
        assert!(
            actions.contains(&Action::Increment) && actions.contains(&Action::Decrement),
            "a resizable column header must advertise Increment/Decrement; got {actions:?}"
        );
    }

    #[test]
    fn at_increment_widens_the_column_and_respects_its_bounds() {
        use teksilo_core::accesskit::Action;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TableView::new(rows(5))
                .add_column(id_col().max_width(70.0))
                .add_column(name_col())
                .row_height(20.0)
                .show_internal_scrollbars(false),
        );
        relayout(&mut tree);
        let header = tree.find_by_label("ID").expect("the ID header cell");
        // id is Fixed(60) capped at 70; one 8 px step lands at 68.
        assert!(
            tree.dispatch_access_action(
                teksilo_core::accessibility::widget_id_to_node_id(header),
                Action::Increment,
                None,
                &mut teksilo_core::window::ops::NoopWindowOps,
            ),
            "the header must handle the Increment it advertised"
        );
        relayout(&mut tree);
        let w = overrides(&tree, table);
        assert!(
            (w.get("id").copied().unwrap_or(0.0) - 68.0).abs() < 0.5,
            "one Increment must step the column by COLUMN_RESIZE_STEP; got {w:?}"
        );
        // A second step would reach 76 — clamped to the 70 px cap, matching
        // what a pointer drag would have stored.
        assert!(
            tree.dispatch_access_action(
                teksilo_core::accessibility::widget_id_to_node_id(header),
                Action::Increment,
                None,
                &mut teksilo_core::window::ops::NoopWindowOps,
            ),
            "the header must handle the Increment it advertised"
        );
        relayout(&mut tree);
        let w = overrides(&tree, table);
        assert!(
            (w.get("id").copied().unwrap_or(0.0) - 70.0).abs() < 0.5,
            "the AT path must clamp exactly like the drag path; got {w:?}"
        );
    }

    #[test]
    fn the_documented_settings_round_trip_terminates_instead_of_recursing() {
        // docs/table-view.md ("Persistence") wires the table's width signal
        // into a settings signal and the settings signal back into
        // `set_column_widths`. `Signal::set` carries no equality check by
        // design, so an unguarded setter closes that pair into an unbounded
        // mutual recursion — and a `ColumnResizePolicy::Live` drag writes a
        // width on *every* pointer move, so the very first tick of the very
        // first resize would blow the notify-depth guard.
        //
        // Modelled here on the real `imperative::set_column_widths` (the
        // guard `TableView`/`TreeTableView` both route through), with the
        // app-side hop left unguarded exactly as an app would write it.
        use crate::table_view::imperative;
        use std::cell::Cell;
        use std::collections::HashMap;
        use std::rc::Rc;

        let table_widths: Signal<HashMap<String, f32>> = Signal::new(HashMap::new());
        let store: Signal<HashMap<String, f32>> = Signal::new(HashMap::new());
        let notifications = Rc::new(Cell::new(0_u32));

        // table -> store (the app's `observe`, no equality check).
        let _to_store = table_widths.observe({
            let store = store.clone();
            let seen = notifications.clone();
            move |w| {
                seen.set(seen.get() + 1);
                store.set(w.clone());
            }
        });
        // store -> table (the guarded widget setter).
        let _to_table = store.observe({
            let table_widths = table_widths.clone();
            move |w| imperative::set_column_widths(&table_widths, w.clone())
        });

        // One drag tick. Without the guard this recurses until the
        // notify-depth guard panics.
        let mut w = HashMap::new();
        w.insert("name".to_string(), 370.0);
        table_widths.set(w.clone());

        assert_eq!(
            notifications.get(),
            1,
            "the round trip must settle after exactly one pass"
        );
        assert_eq!(table_widths.get(), w);
        assert_eq!(store.get(), w);

        // A no-change write is inert on both edges.
        imperative::set_column_widths(&table_widths, w);
        assert_eq!(notifications.get(), 1, "an unchanged map must not notify");
    }

    #[test]
    fn header_strip_paints_a_separator_at_every_column_boundary() {
        // The separator IS the resize affordance: it is the only thing that
        // shows where the grip is. The body's vertical grid lines are gated on
        // `GridLines` (default `None`) and are clipped to the body anyway, so
        // the header used to offer no cue at all.
        let (mut tree, _table) = two_column_table();
        let frame = tree.render();
        let found = frame.decorations.iter().any(|d| {
            let [x, y, w, h] = d.rect;
            (x - 59.0).abs() < 0.6
                && w <= 1.5
                && y.abs() < 0.6
                && (h - cp::HEADER_HEIGHT).abs() < 0.6
        });
        assert!(
            found,
            "expected a full-height header separator at the id/name divider \
             (x≈59, h={}); decorations={:?}",
            cp::HEADER_HEIGHT,
            frame.decorations.iter().map(|d| d.rect).collect::<Vec<_>>()
        );
    }
}

// ── Editor release, and the spreadsheet selection chords ───────────────────

#[test]
fn an_open_editor_keeps_the_keys_it_needs() {
    // Rows are not focusable nodes, so a key the editor ignores bubbles
    // straight to the table. A single-line editor does nothing with PageDown,
    // and the table used to page the cursor out from under the edit.
    let (mut tree, table, _) = build_table(50);
    focus_at(&mut tree, table, 5, 0);
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.editing_cell_signal().set(Some((5, 0)));
    }

    for key in [Key::PageDown, Key::Home, Key::End, Key::PageUp] {
        tree.press_key(key, Modifiers::NONE);
        assert_eq!(
            read_focused_cell(&tree, table),
            Some((5, 0)),
            "{key:?} belongs to the editor while one is open"
        );
    }

    // Escape still reaches the table — it is about the edit, not the text.
    tree.press_key(Key::Escape, Modifiers::NONE);
    let any = tree.widget_as_any(table).unwrap();
    let tv = any.downcast_ref::<TableView<Row>>().unwrap();
    assert_eq!(tv.editing_cell_signal().get(), None);
}

#[test]
fn ctrl_space_selects_the_column_and_shift_space_the_row() {
    // The ARIA grid pattern and Excel both read these two this way. Reading
    // them as the file manager's "toggle the focused item" would take a
    // spreadsheet user's two most-used selection chords.
    let model = rows(4);
    let cs = super::CellSelectionModel::new(TableSelectionMode::MultiCell);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

    tree.press_key(Key::Space, Modifiers::CTRL);
    let got: Vec<_> = cs.selection_signal().get().into_iter().collect();
    assert_eq!(got, vec![(0, 1), (1, 1), (2, 1), (3, 1)], "the column");

    tree.press_key(Key::Space, Modifiers::SHIFT);
    let got: Vec<_> = cs.selection_signal().get().into_iter().collect();
    assert_eq!(got, vec![(1, 0), (1, 1)], "the row");
}

#[test]
fn ctrl_shift_a_deselects_a_multi_row_table() {
    use teksilo_data::{SelectionMode, SelectionModel};
    let model = rows(6);
    let sel = SelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(table);

    tree.press_key(Key::A, Modifiers::COMMAND);
    assert_eq!(sel.count(), 6);
    tree.press_key(Key::A, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(sel.count(), 0);
}

#[test]
fn a_non_selectable_table_stays_pure_structure() {
    // Nothing to select means nothing to drive, so `Role::Table` is right
    // there — the static-structure ARIA role, and what a screen reader should
    // read in browse mode rather than as a navigable grid.
    let model = rows(3);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::None),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(tree.accessibility_node(table).role(), Role::Table);
}

#[test]
fn a_cell_selection_table_announces_grid_cells() {
    // `Role::Cell` gets no UIA `SelectionItem` pattern from AccessKit, so a
    // selected cell announced that way never reports `IsSelected` on Windows.
    // `Role::GridCell` does, and maps identically on macOS and AT-SPI.
    let (tree, table, _) = build_cell_table(3);
    assert!(
        count_role(&tree, table, Role::GridCell) > 0,
        "cell-selection tables announce GridCell"
    );

    let (tree, table, _) = build_table(3);
    assert_eq!(
        count_role(&tree, table, Role::GridCell),
        0,
        "a row-selection table's cells are not the selectable unit"
    );
    assert!(count_role(&tree, table, Role::Cell) > 0);
}

#[test]
fn a_row_answers_the_scroll_into_view_an_assistive_client_sends() {
    // The one scroll action all three AccessKit adapters actually consume —
    // UIA's `IScrollItemProvider`, AppKit's `accessibilityScrollToVisible`,
    // AT-SPI's `ScrollTo`. Rows are not focusable nodes, so without it a
    // screen reader has no way to bring one into view. `ListView` and
    // `TreeView` have answered it since they shipped; the tables did not.
    let (mut tree, table, _) = build_table(100);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(read_scroll(&tree, table), 0.0);

    // Scroll away, then ask a realized row to reveal itself. The id is
    // resolved *after* the relayout: the pane rebuilds its rows on scroll, so
    // one captured earlier would name a destroyed widget.
    {
        let any = tree.widget_as_any(table).unwrap();
        let tv = any.downcast_ref::<TableView<Row>>().unwrap();
        tv.scroll_y_signal().set(600.0);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let row = first_body_row_id(&tree, table);
    let mut ops = teksilo_core::window::NoopWindowOps;
    let handled = tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(row),
        teksilo_core::accesskit::Action::ScrollIntoView,
        None,
        &mut ops,
    );
    assert!(handled, "ScrollIntoView must be serviced");
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // And it actually moved the viewport, rather than reporting success and
    // doing nothing: the row the walker named sits below the visible band, so
    // revealing it scrolls down to its bottom edge.
    assert!(
        read_scroll(&tree, table) > 600.0,
        "ScrollIntoView must move the viewport toward the row"
    );
}

// ── Regressions from the data-view keyboard review ─────────────────────────

#[test]
fn a_minus_still_opens_the_editor_in_a_flat_table() {
    // `-` is unshifted on a US board, so the tree chords claimed it before
    // the type-to-edit arm ever ran — and a flat table has no subtree to
    // expand, so the key simply vanished. Starting a negative number in an
    // `ANY_KEY` column is exactly the case that broke.
    use std::cell::Cell;
    use std::rc::Rc;
    let fired = Rc::new(Cell::new(0));
    let f = fired.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(rows(5))
            .add_column(id_col().editable(true))
            .add_column(name_col())
            .row_height(20.0)
            .edit_triggers(super::EditTriggers::F2 | super::EditTriggers::ANY_KEY)
            .on_cell_edit_request(move |_, _, _| f.set(f.get() + 1)),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 0, 0);
    tree.press_key(Key::Character('-'), Modifiers::NONE);
    assert_eq!(fired.get(), 1, "the flat table's editor must still open");
}

#[test]
fn ctrl_space_stays_a_toggle_in_a_single_cell_table() {
    // "Select the column" has no reading for a mode that holds one cell, and
    // performing it broke the mode's own invariant.
    let model = rows(4);
    let cs = super::CellSelectionModel::new(TableSelectionMode::SingleCell);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::SingleCell)
            .cell_selection(cs.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    focus_at(&mut tree, table, 1, 1);

    tree.press_key(Key::Space, Modifiers::CTRL);
    let got: Vec<_> = cs.selection_signal().get().into_iter().collect();
    assert_eq!(got, vec![(1, 1)], "one cell, not the whole column");
}

#[test]
fn a_row_advertises_the_scroll_into_view_it_answers() {
    // Every adapter gates its scroll pattern on the node *supporting* the
    // action, so handling it without advertising it left the row unreachable
    // to a real screen reader even though a synthetic dispatch worked.
    use teksilo_core::accesskit::Action;
    let (tree, table, _) = build_table(50);
    let row = first_body_row_id(&tree, table);
    assert!(
        tree.accessibility_node(row)
            .actions()
            .contains(&Action::ScrollIntoView),
        "the row must advertise the action, not only handle it"
    );
}

// ── A cell's own controls ──────────────────────────────────────────────────

/// A 200-row table whose second column is a checkbox, 35 rows realized.
fn checkbox_table() -> (
    WidgetTree,
    WidgetId,
    teksilo_core::signal::Signal<bool>,
    SizeProposal,
) {
    // One shared signal across every row: the test only needs to observe that
    // Space reached *a* checkbox, and a per-row signal would mean threading a
    // map through the delegate for nothing.
    let checked = teksilo_core::signal::Signal::new(false);
    let ck = checked.clone();
    let model = rows(200);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(
                Column::<Row>::new("done", lit!("Done"), move |_row, _: &CellContext| {
                    Box::new(crate::Checkbox::new(ck.clone()))
                })
                .width(ColumnWidth::Fixed(60.0)),
            )
            .row_height(20.0),
    );
    let p = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(p);
    tree.focus(table);
    (tree, table, checked, p)
}

#[test]
fn a_table_is_one_tab_stop_however_many_cells_are_realized() {
    // A checkbox column used to put every realized cell in the Tab order — 36
    // stops here, and a different 36 after scrolling. Pressing Tab cannot
    // detect that (the table claims Tab for its cell cursor), which is why
    // this reads the traversal graph directly.
    let (tree, table, _ck, _p) = checkbox_table();
    let stops = tree.tab_stops_within(table);
    assert_eq!(
        stops.len(),
        1,
        "a grid is one Tab stop; got {} — a cell control has leaked into the \
         Tab order, where its presence tracks the scroll position",
        stops.len()
    );
}

#[test]
fn space_checks_the_focused_cell_and_leaves_the_selection_alone() {
    use teksilo_data::{SelectionMode, SelectionModel};
    let checked = teksilo_core::signal::Signal::new(false);
    let ck = checked.clone();
    let sel = SelectionModel::new(SelectionMode::Multi);
    let model = rows(20);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TableView::new(model)
            .add_column(id_col())
            .add_column(
                Column::<Row>::new("done", lit!("Done"), move |_row, _: &CellContext| {
                    Box::new(crate::Checkbox::new(ck.clone()))
                })
                .width(ColumnWidth::Fixed(60.0)),
            )
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiRow)
            .selection(sel.clone()),
    );
    let p = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(p);
    // Cursor on the checkbox column of row 2.
    focus_at(&mut tree, table, 2, 1);
    sel.select(2);

    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(checked.get(), "Space reaches the focused cell's checkbox");
    assert_eq!(sel.selected_indices(), vec![2], "selection untouched");

    // The id column publishes no toggle, so Space is the selection there.
    focus_at(&mut tree, table, 2, 0);
    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(
        checked.get(),
        "the checkbox is not touched from another column"
    );
    assert_eq!(
        sel.selected_indices(),
        Vec::<usize>::new(),
        "row toggled off"
    );
}
