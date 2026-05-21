//! `data_grid` — flat `TableView` showcase.
//!
//! Run with: `cargo run -p data-grid`
//!
//! Demonstrates:
//! - 1000-row `ListModel<Employee>`.
//! - Six columns: id (Fixed, sortable, pinned Leading), name + email
//!   (Flex, sortable + filterable), role (Flex, sortable, alignment
//!   Center), salary (Fixed, sortable, alignment Trailing), active
//!   (Fixed, alignment Center, custom badge cell).
//! - `MultiRow` selection wired through `SelectionModel`.
//! - `SortFilterListModel<Employee>` proxy bound to `table.sort_signal()`
//!   and `table.filters_signal()` so sort + filter compose.
//! - Reset-filters button + status-bar row count.
//! - **Editable name column**: `EditTrigger::F2OrType` plus an
//!   `is_editing`-aware cell delegate that swaps a `TextInput` in for
//!   the focused cell. (This example does not write the edited value
//!   back into the model — see the comment on `name_column`.)
//!
//! Resize columns by dragging their trailing edge. Reorder columns by
//! dragging a header. Click a header to cycle sort. Use arrow keys /
//! Home / End / PgUp / PgDn / Tab on the focused cell to navigate.
//! Press F2 (or just start typing) on a focused name cell to edit it.

use bastyde::core::signal::Signal;
use bastyde::data::{
    ListDataSource, ListModel, SelectionMode, SelectionModel, SortDirection, SortFilterListModel,
};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, CellContext, Column, ColumnWidth, EditTrigger, Expand, GridLines, HStack, Padding,
    Panel, Spacer, TableAlignment as Alignment, TableSelectionMode, TableView, TextInput,
    TextWidget, Toolbar, VStack,
};

#[derive(Clone, Debug)]
struct Employee {
    id: u32,
    name: String,
    email: String,
    role: &'static str,
    salary: u32,
    active: bool,
}

fn make_data(n: u32) -> Vec<Employee> {
    let roles = ["Admin", "Editor", "Viewer", "Owner"];
    let names = [
        "Avery", "Blake", "Casey", "Drew", "Elliot", "Finn", "Gray", "Harper", "Indigo", "Jordan",
        "Kai", "Logan", "Morgan", "Nico",
    ];
    (0..n)
        .map(|i| Employee {
            id: i + 1,
            name: format!(
                "{} {}",
                names[(i as usize) % names.len()],
                (i as usize) % 100
            ),
            email: format!("user{i}@example.com"),
            role: roles[(i as usize) % roles.len()],
            salary: 35_000 + (i * 137) % 100_000,
            active: i % 3 != 0,
        })
        .collect()
}

fn id_column() -> Column<Employee> {
    Column::<Employee>::new("id", "ID", |row, _: &CellContext| {
        Box::new(TextWidget::new(lit!(row.id.to_string())))
    })
    .width(ColumnWidth::Fixed(64.0))
    .sortable(true)
    .pinned(bastyde::widgets::PinnedSide::Leading)
}

fn name_column() -> Column<Employee> {
    // Editable column: when the focused cell enters edit mode (F2 or
    // typing a printable character), the delegate swaps a `TextInput`
    // in for the static label. The `Signal<String>` is initialised
    // from the row's name; this example deliberately does not write
    // the edited value back into the underlying `ListModel<Employee>`
    // — the focus here is on the cell-delegate edit surface, not the
    // persistence path. A real app would forward `on_submit` to a
    // `model.set(row, ...)` call.
    Column::<Employee>::new("name", "Name", |row, ctx: &CellContext| {
        if ctx.is_editing {
            let buffer = Signal::new(row.name.clone());
            Box::new(TextInput::new(buffer).placeholder("Name"))
        } else {
            Box::new(TextWidget::new(lit!(row.name.clone())))
        }
    })
    .width(ColumnWidth::Flex(2.0))
    .sortable(true)
    .filterable(true)
    .editable(true)
}

fn email_column() -> Column<Employee> {
    Column::<Employee>::new("email", "Email", |row, _: &CellContext| {
        Box::new(TextWidget::new(lit!(row.email.clone())))
    })
    .width(ColumnWidth::Flex(2.0))
    .sortable(true)
    .filterable(true)
}

fn role_column() -> Column<Employee> {
    Column::<Employee>::new("role", "Role", |row, _: &CellContext| {
        Box::new(TextWidget::new(lit!(row.role)))
    })
    .width(ColumnWidth::Flex(1.0))
    .sortable(true)
    .filterable(true)
    .alignment(Alignment::Center)
}

fn salary_column() -> Column<Employee> {
    Column::<Employee>::new("salary", "Salary", |row, _: &CellContext| {
        Box::new(TextWidget::new(lit!(format!("${}", row.salary))))
    })
    .width(ColumnWidth::Fixed(110.0))
    .sortable(true)
    .alignment(Alignment::Trailing)
}

fn active_column() -> Column<Employee> {
    Column::<Employee>::new("active", "Active", |row, _: &CellContext| {
        let label = if row.active { "● Yes" } else { "○ No" };
        Box::new(TextWidget::new(lit!(label)))
    })
    .width(ColumnWidth::Fixed(80.0))
    .alignment(Alignment::Center)
}

fn main() {
    let model = ListModel::from_vec(make_data(1000));

    // SortFilterListModel proxies the source through sort + filter,
    // bound to the TableView's signals.
    let proxy = SortFilterListModel::new(model.clone())
        .with_comparator("id", |a: &Employee, b: &Employee| a.id.cmp(&b.id))
        .with_comparator("name", |a, b| a.name.cmp(&b.name))
        .with_comparator("email", |a, b| a.email.cmp(&b.email))
        .with_comparator("role", |a, b| a.role.cmp(b.role))
        .with_comparator("salary", |a, b| a.salary.cmp(&b.salary))
        .with_predicate("name", |t| {
            let needle = t.to_lowercase();
            Box::new(move |row: &Employee| row.name.to_lowercase().contains(&needle))
        })
        .with_predicate("email", |t| {
            let needle = t.to_lowercase();
            Box::new(move |row: &Employee| row.email.to_lowercase().contains(&needle))
        })
        .with_predicate("role", |t| {
            let needle = t.to_lowercase();
            Box::new(move |row: &Employee| row.role.to_lowercase().contains(&needle))
        });

    let selection = SelectionModel::new(SelectionMode::Multi);

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

    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(WindowConfig::new().title("Data Grid").size(1100, 640).root(
            move |tree, _| {
                let table = TableView::from_source(proxy.clone())
                    .add_column(id_column())
                    .add_column(name_column())
                    .add_column(email_column())
                    .add_column(role_column())
                    .add_column(salary_column())
                    .add_column(active_column())
                    .row_height(28.0)
                    .alternating_rows(true)
                    .grid_lines(GridLines::Horizontal)
                    .selection_mode(TableSelectionMode::MultiRow)
                    .selection(selection.clone())
                    // Press F2 (or start typing a letter) on the
                    // focused name cell to swap in a TextInput.
                    .edit_trigger(EditTrigger::F2OrType);

                // Wire the proxy's sort + filter from the table's signals.
                proxy.bind_sort_signal(table.sort_signal().clone());
                proxy.bind_filters_signal(table.filters_signal().clone());

                // Default sort: ascending by id.
                table.set_sort(Some("id"), SortDirection::Ascending);

                let table_id = tree.add(table);

                // Status bar showing row count + selection count.
                let status = TextWidget::new(lit!(format!(
                    "{} rows  ·  selection: {}",
                    proxy.len(),
                    selection.count()
                )));
                let toolbar = HStack::new()
                    .spacing(8.0)
                    .child(Button::new(lit!("Reset filters")))
                    .child(Button::new(lit!("Reset sort")))
                    .child(Spacer::new())
                    .child(status);

                let layout = VStack::new()
                    .spacing(6.0)
                    .child(Padding::symmetric(6.0_f32, 12.0_f32).child(toolbar))
                    .child(Panel::new().child_id(table_id));
                tree.add(
                    VStack::new()
                        .child(dark_mode_toolbar())
                        .child(Expand::new().child(layout)),
                )
            },
        ))
        .run();
}
