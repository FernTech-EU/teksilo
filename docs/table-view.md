# TableView and TreeTable

Two production-grade tabular widgets for Bastyde: a flat
[`TableView<T>`](../crates/bastyde-widgets/src/table_view.rs) over any
`ListDataSource<Item = T>` and a hierarchical
[`TreeTable<T>`](../crates/bastyde-widgets/src/tree_table.rs) over a
[`SortFilterTreeModel<T>`](../crates/bastyde-data/src/sort_filter_tree_model.rs).
They share the same column model, header strip, drag/resize/reorder,
filter popover, keyboard map, and accessibility wrappers; only the body
pane differs.

This page is the reference for the public surface and the design
contracts you can rely on.

---

## At a glance

```rust
use bastyde::data::{SelectionMode, SelectionModel, SortDirection, SortFilterListModel};
use bastyde::prelude::*;
use bastyde::widgets::{
    Column, ColumnWidth, GridLines, TableAlignment as Alignment,
    TableSelectionMode, TableView, TextWidget,
};

let model      = ListModel::from_vec(rows());
let selection  = SelectionModel::new(SelectionMode::Multi);
let proxy      = SortFilterListModel::new(model)
    .with_comparator("name", |a, b| a.name.cmp(&b.name))
    .with_predicate("name", |t| {
        let needle = t.to_lowercase();
        Box::new(move |row| row.name.to_lowercase().contains(&needle))
    });

let table = TableView::from_source(proxy.clone())
    .add_column(
        Column::new("name", "Name", |row, _| {
            Box::new(TextWidget::new(lit!(row.name.clone())))
        })
        .width(ColumnWidth::Flex(2.0))
        .sortable(true)
        .filterable(true),
    )
    .row_height(28.0)
    .alternating_rows(true)
    .grid_lines(GridLines::Horizontal)
    .selection_mode(TableSelectionMode::MultiRow)
    .selection(selection.clone());

// One-shot wiring: the proxy now consumes the table's signals.
proxy.bind_sort_signal(table.sort_signal().clone());
proxy.bind_filters_signal(table.filters_signal().clone());
table.set_sort(Some("name"), SortDirection::Ascending);
```

`TreeTable` is identical in shape but takes a `SortFilterTreeModel<T>`
and adds a `tree_column(id)` plus an optional `filter_mode(...)`:

```rust
let proxy = SortFilterTreeModel::new(model)
    .filter_mode(TreeFilterMode::KeepAncestors)
    .with_predicate("name", /* … */);

let tree = TreeTable::from_projection(proxy.clone())
    .add_column(/* tree column with the twist arrow */ name_col)
    .add_column(size_col)
    .tree_column("name")
    .selection_mode(TableSelectionMode::MultiRow);
```

---

## Column model

A column is a generic descriptor over the row type:

```rust
pub struct Column<T: 'static> { /* … */ }

Column::new("id", "ID", |row, ctx| Box::new(TextWidget::new(lit!(row.id.to_string()))))
    .width(ColumnWidth::Fixed(64.0))   // Fixed | Flex(factor) | Auto
    .min_width(40.0)
    .max_width(200.0)
    .alignment(Alignment::Trailing)    // Leading | Center | Trailing
    .sortable(true)
    .filterable(true)                  // exposes the filter popover affordance
    .resizable(true)                   // default true
    .reorderable(true)                 // default true
    .pinned(PinnedSide::Leading)       // Leading | None | Trailing
    .truncation(TruncationPolicy::Ellipsis);
```

Column ids are the persistence key for sort, filter, width, and order
signals — keep them stable across releases.

`CellContext` passed to the cell delegate carries:

| Field            | Meaning                                                     |
|------------------|-------------------------------------------------------------|
| `row_index`      | visible row (post sort/filter)                              |
| `col_id`         | the column's stable id                                      |
| `col_index`      | display position (0-based, post pin + reorder)              |
| `is_selected`    | `true` when this row (or cell, in cell-mode) is selected    |
| `is_focused`     | `true` when this cell carries the keyboard focus            |
| `is_editing`     | `true` when `editing_cell_signal == Some((row, col_index))` |
| `depth`          | `Some(level)` in `TreeTable`, `None` in `TableView`         |
| `is_tree_column` | `true` on the column hosting the twist arrow                |

---

## Sort / filter / widths / order — the signal contract

Both widgets publish six reactive signals. Mutating any of them
triggers the right rebuild level (no full layout when scrolling, no
rebuild when only the focus ring moves, etc.).

| Signal                                                                  | Type                                          | Mutated by                                                | Persistence key |
|-------------------------------------------------------------------------|-----------------------------------------------|-----------------------------------------------------------|-----------------|
| [`sort_signal`](../crates/bastyde-widgets/src/table_view.rs)               | `Signal<Option<(String, SortDirection)>>`     | header click cycle, `set_sort`, `clear_sort`              | `table.sort`    |
| [`filters_signal`](../crates/bastyde-widgets/src/table_view.rs)            | `Signal<HashMap<String, String>>`             | filter popover, `set_filter`, `clear_filters`             | `table.filters` |
| [`column_widths_signal`](../crates/bastyde-widgets/src/table_view.rs)      | `Signal<HashMap<String, f32>>`                | header drag-resize, `set_column_width`                    | `table.widths`  |
| [`column_order_signal`](../crates/bastyde-widgets/src/table_view.rs)       | `Signal<Vec<String>>`                         | header drag-reorder, `set_column_order`                   | `table.order`   |
| [`column_pinning_signal`](../crates/bastyde-widgets/src/table_view.rs)     | `Signal<HashMap<String, PinnedSide>>`         | drag across pane boundary, `set_column_pinning`           | `table.pinning` |
| [`focused_cell_signal`](../crates/bastyde-widgets/src/table_view.rs)       | `Signal<Option<(usize, usize)>>`              | keyboard nav, `set_focused_cell`, `clear_focused_cell`    | (transient)     |

### Persistence

Use [`bastyde-settings`](settings.md) to round-trip the layout. A typical
shape:

```rust
const TABLE_SORT:    SettingsKey<String>             = SettingsKey::new("table.sort", String::new);
const TABLE_FILTERS: SettingsKey<HashMap<String,String>> = SettingsKey::new("table.filters", HashMap::new);
const TABLE_WIDTHS:  SettingsKey<HashMap<String,f32>>    = SettingsKey::new("table.widths", HashMap::new);
const TABLE_ORDER:   SettingsKey<Vec<String>>            = SettingsKey::new("table.order", Vec::new);

let store = ctx.settings();
store.signal_for(&TABLE_WIDTHS).observe({
    let table = table.clone();
    move |w| table.set_column_widths(w.clone())
});
table.column_widths_signal().observe({
    let store = store.clone();
    move |w| store.set(&TABLE_WIDTHS, w.clone())
});
// Repeat for sort / filters / order.
```

The signal API is the persistence boundary on purpose — the widget
emits, the application persists. There are no `on_*_changed` hooks; an
`observe` on the signal is the same thing without the typo surface.

### `SortFilterListModel<T>` vs raw signals

The minimum the widget needs is the four signals above; you can apply
sort and filter manually inside `on_sort_changed` / `on_filters_changed`
observers. **Don't.** Use the proxy:

```rust
let proxy = SortFilterListModel::new(model)
    .with_comparator("name", |a, b| a.name.cmp(&b.name))
    .with_predicate("name", |t| { /* … */ });
let table = TableView::from_source(proxy.clone());
proxy.bind_sort_signal(table.sort_signal().clone());
proxy.bind_filters_signal(table.filters_signal().clone());
```

The proxy:

- maintains a single visible-index map shared between sort and filter,
- emits `DataChange::Reset` once per upstream change (one rebuild, not two),
- forwards row-level inserts/removes to the table's `SelectionModel`
  via the `observe_changes` chain, so `MultiRow` selection survives data
  mutations.

For trees, [`SortFilterTreeModel<T>`](../crates/bastyde-data/src/sort_filter_tree_model.rs)
plays the same role, plus a `TreeFilterMode` switch:

| Mode                | Behaviour                                                                                       |
|---------------------|-------------------------------------------------------------------------------------------------|
| `HideNonMatching`   | rows that don't match are hidden, taking their entire subtree with them                         |
| `KeepAncestors`     | a match keeps every ancestor visible (file-tree convention; the **default**)                    |
| `KeepDescendants`   | a match keeps its full subtree visible (useful for "find a folder, see what's inside")          |

`TreeTable::filter_mode(...)` forwards to the proxy in place — calling
it on the builder mutates the shared `Rc<RefCell<…>>` even though the
method consumes `Self`.

---

## The filter popover

When `Column::filterable(true)`, the header cell paints a small funnel
glyph at the trailing end (just before the resize zone). Tapping it
opens a [`Popover`](../crates/bastyde-widgets/src/popover.rs) anchored to
the glyph; the popover content is a one-line text editor + a `Clear`
button that mutate the `filters_signal[col_id]` slot in place.

- The popover dismisses on Escape or click-outside (default
  `DismissBehavior::EscapeOrClickOutside`).
- Empty editor text removes the column's entry from the map; a
  non-empty string inserts/replaces it.
- The glyph tints `TextRole::Accent` when the column has an active
  filter and `TextRole::Secondary` otherwise.

Callers that already use `SortFilterListModel<T>` /
`SortFilterTreeModel<T>` get filtered output for free —
`bind_filters_signal` re-projects the visible list whenever the popover
mutates the map.

The editor inside the popover is a deliberately minimal text field:
printable characters, Backspace, Delete (clear), and ImeCommit. It does
**not** depend on the `rich-text` feature, so the filter UI is
available in any TableView/TreeTable build.

The header's pointer handler reserves a **filter zone** at the trailing
edge (resize handle + filter glyph + a small padding tolerance) so that
PointerDown over the popover trigger reaches the trigger instead of
being eaten by the sort-cycle handler. Outside that zone, a click on
the header label still cycles the sort as before.

---

## Selection

`TableSelectionMode` picks the model:

| Mode                 | Backing model                                       | Notes                                                           |
|----------------------|-----------------------------------------------------|-----------------------------------------------------------------|
| `None`               | —                                                   | clicks just move focus                                          |
| `SingleRow`          | `bastyde_data::SelectionModel`                         | replaces; modifier keys ignored                                 |
| `MultiRow` (default) | `bastyde_data::SelectionModel` with `SelectionMode::Multi` | Ctrl-click toggles, Shift-click extends, Shift+Arrow extends |
| `SingleCell`         | `CellSelectionModel`                                | Excel-style; one `(row,col)` at a time                          |
| `MultiCell`          | `CellSelectionModel`                                | rectangular extension via Shift+Arrow / Shift+Click             |

Both selection models auto-adjust on `DataChange::ItemsInserted` /
`ItemsRemoved` / `Reset`, so visual selection survives sorting,
filtering, and underlying mutation.

`TreeTable` accepts both row and cell modes; selection is keyed by the
**flat visible index** of the `TreeSlice`. Expanding/collapsing
re-numbers indices, so don't pin a selection across an `expand_all()`
without a re-mapping step.

---

## Editing

The widget is the keyboard handler; the cell delegate is the editor
swap. Wire it in three lines:

```rust
let table = TableView::from_source(proxy)
    // ...
    .edit_trigger(EditTrigger::F2OrTypeOrDoubleClick)   // default
    .on_cell_edit_request(|row, col_id, ctx| {
        // open your editor: a TextInputField bound to the row's value,
        // a date picker, a colour picker, …
    });

let column = Column::new("amount", "Amount", move |row, ctx| {
    if ctx.is_editing {
        // Swap in your editor while editing_cell_signal matches.
        Box::new(TextInputField::new(state_for(row.id)))
    } else {
        Box::new(TextWidget::new(lit!(format!("{}", row.amount))))
    }
});
```

`EditTrigger` selects which gestures begin an edit:

| Variant                  | F2 | Type | Double-click |
|--------------------------|:--:|:----:|:------------:|
| `F2`                     | ✔  |      |              |
| `F2OrType`               | ✔  | ✔    |              |
| `DoubleClick`            |    |      | ✔            |
| `F2OrTypeOrDoubleClick`  | ✔  | ✔    | ✔            |
| `None`                   |    |      |              |

`F2OrTypeOrDoubleClick` is the default (Excel-like). `editing_cell_signal`
is the source of truth for "which cell is in edit mode"; `begin_edit`
and `end_edit` give you imperative control.

Escape ends the edit (the framework's keyboard handler reads
`editing_cell_signal` and clears it before falling back to the focus
clear behaviour).

---

## Keyboard

| Key                         | Effect                                                                                |
|-----------------------------|---------------------------------------------------------------------------------------|
| Arrow keys                  | move focused cell within the visible grid                                             |
| Home / End                  | jump to first / last column of the current row                                        |
| Ctrl-Home / Ctrl-End        | jump to first / last cell                                                             |
| PgUp / PgDn                 | scroll one page; focus moves the same number of rows                                  |
| Tab / Shift+Tab             | next / previous cell in row order, wrapping rows (configurable via `tab_traversal`)   |
| Shift + Arrow               | extend selection in `MultiRow` / `MultiCell` modes                                    |
| Space                       | toggle selection at focus                                                             |
| Enter                       | invoke `on_row_activate` (or fall back to toggle-select)                              |
| Ctrl-A                      | select all rows / cells in multi modes                                                |
| F2 / typing                 | begin edit (gated by `EditTrigger`)                                                   |
| Escape                      | end edit if any, else clear focus                                                     |
| ArrowLeft on tree column    | collapse the row when expanded (TreeTable)                                            |
| ArrowRight on tree column   | expand the row when collapsed and has children (TreeTable)                            |

The same handler powers both widgets via the
[`RowNavigator`](../crates/bastyde-widgets/src/table_view/row_navigator.rs)
trait — `FlatNavigator` for `TableView`, `TreeNavigator` for
`TreeTable`.

---

## Drag & drop

### Column resize

Each header cell exposes a small hit zone on its trailing edge
(`RESIZE_HANDLE_WIDTH`, default 4 px). Cursor switches to
`CursorIcon::ColResize` on hover; PointerDown captures the pointer and
PointerMove updates `column_widths_signal`. Two policies:

```rust
table.column_resize_policy(ColumnResizePolicy::Live)        // commit on every tick (default)
table.column_resize_policy(ColumnResizePolicy::OnRelease)   // commit on PointerUp
```

The handler converts window-space pointer coordinates into cell-local
coordinates using the cell's window origin, captured in
`place_children`. Without that translation, the resize zone test would
mis-fire from anywhere in any column past the first one.

### Column reorder

Drag a header cell from outside the resize zone. The column-reorder
drag emits `ColumnReorderDragData { col_id, source_table_id }`. The
header strip is the drop target; dropping inside the leading-pinned
pane re-pins the column to `Leading`, dropping inside the
trailing-pinned pane re-pins to `Trailing`, otherwise the column joins
the unpinned middle stream. Inter-table drops are rejected by
`source_table_id` mismatch.

### Row reorder (TableView only)

Set `.reorderable_rows(true)` and the table emits
`RowReorderDragData { source_row, source_table_id }` on row drag.
Internal reorders call `ListModel::move_item(from, to)` directly when
the source is a `ListModel<T>`. Inter-table or external drops route
through the user's `on_row_drop(payload, insertion_row, ctx) -> bool`
handler.

`TreeTable` deliberately does **not** ship row drag-drop in v1: a
hierarchical insertion-vs-reparent UX with spring-loaded folders needs
its own design pass.

---

## Accessibility

- `TableView` root: `Role::Table` with `row_count` (header inclusive
  when shown) + `column_count`.
- `TreeTable` root: `Role::TreeGrid`, same counts.
- Each header cell: `Role::ColumnHeader` with `column_index` and, on
  the active sort column, `sort_direction`.
- Each body row: `Role::Row` with `row_index` (1-based; header is row
  1, first body row is row 2). On `TreeTable`, the row also carries
  `level` (1-based depth) and `expanded` for non-leaf rows.
- Each body cell: `Role::Cell` with `row_index` and `column_index`,
  plus `selected` reflecting the current selection.
- The filter popover's trigger inherits the popover's `set_expanded`
  state and is named `"Filter"` — locating it via screen-reader search
  is the same as locating any popover button.

Virtualization vs accessibility: only rendered rows materialize cell
nodes, but `set_row_count(total)` keeps screen readers aware of the
full size. `Action::ScrollIntoView` on an unmaterialized row routes
through `ensure_row_visible`, which is the same path the keyboard
PgDn handler uses.

---

## Theme tokens

| Surface                       | Role                                |
|-------------------------------|-------------------------------------|
| outer frame border            | `BorderRole::Default`               |
| header background             | `SurfaceRole::Raised`               |
| header bottom divider         | `BorderRole::DividerStrong`         |
| body even-row bg              | `SurfaceRole::Content`              |
| body odd-row bg               | `SurfaceRole::AltRow`               |
| row selected bg               | `SurfaceRole::Selected`             |
| cell focus ring               | `BorderRole::Focused`               |
| grid lines                    | `BorderRole::Divider`               |
| sort indicator (active)       | `TextRole::Accent`                  |
| filter glyph (inactive)       | `TextRole::Secondary`               |
| filter glyph (active)         | `TextRole::Accent`                  |
| TreeTable connector lines     | `BorderRole::Divider`               |

Static numbers (`ROW_HEIGHT`, `HEADER_HEIGHT`, `RESIZE_HANDLE_WIDTH`,
`GRID_LINE_THICKNESS`, `TREE_INDENT_PER_LEVEL`, …) are `pub const`s in
[`recipe_table_style`](../crates/bastyde-widgets/src/styles/recipe_table_style.rs)
They are snapshot at build time, like every other widget.

---

## What is and isn't shipped

**Shipped:** virtualized flat + hierarchical bodies, header drag-resize,
header drag-reorder (with cross-pane re-pinning), pinned columns
(`Leading` / `Trailing`), sort cycle (None → Asc → Desc → None), filter
popover with reset, `MultiRow` / `MultiCell` selection with shift +
ctrl semantics, full keyboard nav with focus ring, edit hooks via
`editing_cell_signal` + `on_cell_edit_request`, row drag-drop reorder
on `TableView`, tree expand/collapse via twist + `ArrowLeft/Right`,
tree filter modes, `Role::Table` / `TreeGrid` accessibility with row
indices and sort direction.

**Intentionally not shipped:**

- spreadsheet-style cell merging at the layout level (cells expose
  AccessKit `row_span`/`column_span` for screen readers; the layout
  doesn't merge),
- formula evaluation / computed cells,
- multi-row column-group headers,
- footer / summary rows (compose a `StatusBar` below the table),
- in-table filter chip bar,
- TreeTable row drag-drop (insertion-vs-reparent UX needs its own
  design).

**Deltas you may notice:** `Column::header_override` is stored on
the column but the default header rendering ignores it for now;
`Column::alignment` and `Column::truncation` are likewise persisted on
the descriptor but the user's cell delegate handles its own alignment
and truncation; `row_header_column`, `cell_label`, `row_label`, and
`auto_truncation_tooltip` builders are not yet wired (their
accessibility slots exist on `CellA11y` and `RowA11y`). These are gaps,
not bugs.

---

## Demos

- `cargo run -p data-grid` — 1000-row flat
  [`TableView`](../examples/data_grid/src/main.rs) with
  `SortFilterListModel`, `MultiRow` selection, alternating rows, and
  filterable name/email/role columns.
- `cargo run -p tree-table` — mock filesystem
  [`TreeTable`](../examples/tree_table/src/main.rs) with
  `KeepAncestors` filtering, twist-arrow expand/collapse, and the same
  drag-resize / drag-reorder behaviour as the flat table.
