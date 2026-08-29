<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TableView

`TableView<T>` — generic, virtualized, accessible tabular widget.

Built atop the `ListModel<T>` /
`ListDataSource` data layer in
`teksilo-data` and the `teksilo-tokens` `TableStyle`. Mirrors Qt's
`QTableView`, SwiftUI's `Table`, and JavaFX's `TableView`.
The core skeleton: single body pane, row-virtualized with alternating
backgrounds, grid lines, `Role::Table > Role::Row > Role::Cell`
accessibility, multi-row selection, and an empty-state slot. Headers,
sort, filter, resize, reorder, pinning, cell selection, and editing are
also included. Row heights come in three modes: uniform (`row_height`,
the default fast path), exact per-row callback (`row_height_fn`), and
auto-measured (`auto_row_height` — rows grow to their tallest cell,
height-for-width). See docs/table-view.md "Row heights".

```ignore
use teksilo_data::ListModel;
use teksilo_widgets::table_view::{Column, ColumnWidth, TableView};
use teksilo_i18n::lit;

struct Person { name: String, age: u32 }

let model: ListModel<Person> = ListModel::new();
let _table = TableView::new(model)
    .add_column(Column::new("name", ColumnWidth::Flex(1.0))
        .label(lit!("Name"))
        .cell(|p: &Person, _cx| Box::new(
            teksilo_widgets::primitives::TextWidget::new(
                teksilo_i18n::lit!(p.name.clone())
            )
        )))
    .add_column(Column::new("age", ColumnWidth::Fixed(60.0))
        .label(lit!("Age"))
        .cell(|p: &Person, _cx| Box::new(
            teksilo_widgets::primitives::TextWidget::new(
                teksilo_i18n::lit!(p.age.to_string())
            )
        )))
    .alternating_rows(true)
    .row_height(32.0);
```

## Builder methods at a glance

`from_source`, `from_source_keyed`, `enabled`, `overscroll_behavior`, `smooth_scrolling`, `type_ahead_label`, `type_ahead_timeout`, `smooth_scroll_duration`, `scroll_bar_style`, `add_column`, `columns`, `row_height`, `row_height_fn`, `auto_row_height`, `header_height`, `show_header`, `column_resize_policy`, `tab_traversal`, `edit_trigger`, `on_cell_edit_request`, `on_row_activate`, `reorderable`, `reorderable_rows`, `exportable`, `export_external`, `on_rows_transferred_out`, `accept_foreign_rows`, `on_rows_received`, `activate_on`, `selection_mode`, `selection`, `cell_selection`, `alternating_rows`, `grid_lines`, `a11y_label`, `show_internal_scrollbars`, `empty_view`, `scroll_y_signal`, `max_scroll_y_signal`, `viewport_ratio_y_signal`, `scroll_x_signal`, `max_scroll_x_signal`, `viewport_ratio_x_signal`, `sort_signal`, `column_widths_signal`, `column_order_signal`, `column_pinning_signal`, `focused_cell_signal`, `set_focused_cell`, `clear_focused_cell`, `editing_cell_signal`, `begin_edit`, `end_edit`, `filters_signal`, `set_filter`, `clear_filters`, `scroll_to_row`, `set_sort`, `clear_sort`, `set_column_width`, `set_column_widths`, `set_column_order`, `set_column_pinning`, `ensure_row_visible`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/table_view/index.html)

## `pub struct TableView`

Generic, virtualized, accessible table with sortable / filterable / resizable columns.

Construct with `TableView::new` (from a `ListModel<T>`)
or `TableView::from_source` (any `ListDataSource`), then chain builder methods
to configure columns, row heights, selection, and so on. See module docs for the full
feature list and row-height modes.

```rust
pub struct TableView<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new(model: ListModel<T>) -> Self`

Wrap a `ListModel<T>`.

#### `pub fn from_source<S: ListDataSource<Item = T>>(source: S) -> Self`

Wrap any `ListDataSource<Item = T>` (e.g. a
`SortFilterListModel<T>`).

The source owns DnD validation (`can_accept` / `accept_drop`) and
lazy windowing (`row_state` / `request_window` / `fetch_more`); a
read-only source leaves the defaults inert.

#### `pub fn from_source_keyed<S: ListDataSource<Item = T>>( source: S, keyed: KeyedSelectionModel<S::Key>, ) -> Self where S::Key: ItemKey,`

Wrap any `ListDataSource<Item = T>` with **keyed** row selection. The
`KeyedSelectionModel<S::Key>` tracks selection by source identity, so it
survives reorders / filters / lazy window-slides and stays consistent
across two views of the same source. The view stays `TableView<T>` — the
index↔key mapping is captured from the concrete source here. Equivalent
to `from_source(..)` plus an identity-based replacement for
`selection`.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable the whole view. A disabled view greys out and stops
accepting focus / selection / keyboard input (arena-gated).

#### `pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self`

Set the scroll-chaining behavior at the boundary (default
`OverscrollBehavior::Chain`; `Contain`
disables chaining to an ancestor scrollable).

#### `pub fn smooth_scrolling(mut self, enabled: bool) -> Self`

Enable or disable animated wheel scrolling (enabled by default).
When disabled, wheel events snap immediately to the new offset.

#### `pub fn type_ahead_label(mut self, label: impl Fn(&T) -> String + 'static) -> Self`

Enable **type-ahead** ("type to jump"): typing a printable character
while the table has keyboard focus jumps the focused row to the next
row whose label starts with the accumulated search term, wrapping
around (Qt `keyboardSearch` / macOS & Windows type-select).
`label(&item)` yields the searchable text for a row; matching is
ASCII-case-insensitive. A pause longer than the
`type_ahead_timeout` starts a fresh term.

On an editable column whose `EditTrigger` is type-to-edit, typing
starts an edit instead — type-ahead applies on non-editable columns
(or when no type-to-edit trigger is configured).

#### `pub fn type_ahead_timeout(mut self, timeout: Duration) -> Self`

Reset window between keystrokes before the type-ahead search term
clears (default 500 ms). A zero duration disables type-ahead.

#### `pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self`

Duration of the smooth scroll animation (default 150 ms).

#### `pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self`

How the scroll bar is displayed (default `Permanent`). `Overlay`
and `Thin` float the bar over the content instead of reserving a
layout column for it, mirroring `ScrollArea::scroll_bar_style`.

#### `pub fn add_column(mut self, col: Column<T>) -> Self`

Append a single `Column<T>` definition to the table.

#### `pub fn columns(mut self, cols: impl IntoIterator<Item = Column<T>>) -> Self`

Append multiple `Column<T>` definitions from an iterator.

#### `pub fn row_height(mut self, height: f32) -> Self`

Fixed row height (default: the table style's 28 px) — the
uniform fast path. Mutually exclusive with
`row_height_fn` and
`auto_row_height`; the last mode setter
wins.

#### `pub fn row_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self`

Per-row heights from a callback over the visible row index. The
callback must be pure (same index + same data → same height); it
is re-swept from the first changed index on every model change
(a `SortFilterListModel` source reports that index through
`first_changed_index`, so sort/filter/append keep the valid
prefix). No measurement pass runs.

#### `pub fn auto_row_height(mut self, estimated: f32) -> Self`

Auto-measured row heights: each realized row reports the height
of its tallest cell measured at the cell's column width
(height-for-width), unrealized rows assume `estimated`. Scroll
anchoring keeps content above the viewport stationary as
estimates are corrected; the scrollbar settles one frame after a
measurement change.

#### `pub fn header_height(mut self, height: f32) -> Self`

Override the column header row height in logical pixels. Default: the table style's `HEADER_HEIGHT`.

#### `pub fn show_header(mut self, visible: bool) -> Self`

Show or hide the column header row. Default: visible.

#### `pub fn column_resize_policy(mut self, policy: ColumnResizePolicy) -> Self`

Set how column widths are redistributed when columns are
added, resized, or the table's own width changes. See
`ColumnResizePolicy`.

#### `pub fn tab_traversal(mut self, mode: TabTraversal) -> Self`

Control how Tab / Shift+Tab navigate between cells. See
`TabTraversal`.

#### `pub fn edit_trigger(mut self, trigger: EditTrigger) -> Self`

Set which user action opens a cell editor. See `EditTrigger`.

#### `pub fn on_cell_edit_request( mut self, f: impl Fn(usize, &str, &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Hook fired by the keyboard handler when an edit trigger fires
on the focused cell. Receives `(row_index, col_id, ctx)`.

#### `pub fn on_row_activate( mut self, f: impl Fn(usize, &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Hook fired when the user presses Enter on the focused row.

#### `pub fn reorderable(mut self, enabled: bool) -> Self`

Enable drag-to-reorder of **rows** (pointer drag + keyboard
Alt+ArrowUp/Down). Distinct from
`Column::reorderable`, which reorders
*columns* and defaults to `true`; this defaults to `false`.

The move is routed through the backing source's `accept_drop`: a
`ListModel` reorders in place, an external source routes the move to
its store. Per-hover the source's `can_accept` decides whether the
drop is allowed — a forbidden position shows no insertion line and
the drop is refused. A row may also be forbidden from dragging at
all (the source's `drag` gate). Cross-table / external drops arrive
at `accept_drop` as `DragSource::Foreign`; a bare `ListModel`
rejects them, an external source decides.

#### `pub fn reorderable_rows(self, enabled: bool) -> Self`

Renamed to `reorderable`, matching `ListView`,
`GridView`, `TreeView` and `TreeTableView` — this was the only view in
the family spelling it differently.

#### `pub fn exportable(mut self, mode: DragTransferMode) -> Self where T: Clone,`

Make rows **droppable outside this view** — on a
`DropTarget`, another data view, or the OS.

A dragged row (or the whole selection, when the pressed row is part of a
multi-selection) carries clones of its items in a public
`RowDragData<T>`, so a foreign receiver can pull
them out with `payload.get_typed::<RowDragData<T>>()` /
`DropTarget::on_drop_typed::<RowDragData<T>>()` — no serialization. This
also makes rows a drag source even without `reorderable`.

`mode` chooses what happens to the origin rows once a *foreign* target
accepts them: `DragTransferMode::Move` removes them (via the source's
`on_drag_out`, or `on_rows_transferred_out`),
`DragTransferMode::Copy` leaves them. A same-view reorder is never a
transfer, so `mode` never affects it. Requires `T: Clone`.

#### `pub fn export_external(mut self, f: impl Fn(&[T]) -> Vec<(String, Vec<u8>)> + 'static) -> Self where T: Clone,`

Additionally advertise the dragged rows as MIME data so they can be
dropped on a `DropZone` or exported to another
application / window via the OS. `f` maps the dragged items to
`(mime_type, bytes)` pairs (e.g. `text/plain`, `text/uri-list`, an
app-specific `application/x-…`). Implies `exportable`
(defaulting to `DragTransferMode::Move` if not already set). Requires
`T: Clone`.

#### `pub fn on_rows_transferred_out( mut self, f: impl Fn(&[usize], &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Override how rows moved out to a foreign target are removed from this
view. Receives the dragged rows' indices (descending-safe) and the live
context. Without this, an `exportable`
`Move` drag removes them through the source's
`on_drag_out` (works out of the box for a `ListModel`).

#### `pub fn accept_foreign_rows(mut self, accept: bool) -> Self`

Accept exported rows dropped from a **different** view or source without
writing a custom `ListDataSource`. Pair with
`on_rows_received`, which is handed the dropped
items and the insertion index. (Same-view reorder is
`reorderable`; a custom `ListDataSource` can still
accept foreign drops through its `can_accept`/`accept_drop` instead.)

#### `pub fn on_rows_received( mut self, f: impl Fn(Vec<T>, usize, &mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Handler for rows accepted via `accept_foreign_rows`:
`(items, insertion_index, ctx)`. Insert them into your model at the
index.

#### `pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self`

Choose single- vs double-click activation for `on_row_activate` (default
`ActivateOn::DoubleClick`). Enter/Space activates in
either mode.

#### `pub fn selection_mode(mut self, mode: TableSelectionMode) -> Self`

Choose the row-selection granularity (None / Single / Multi).
See `TableSelectionMode`.

#### `pub fn selection(mut self, sel: SelectionModel) -> Self`

Set the index-based row selection model (positions). For identity-based
selection that survives reorder / filter / window-slide, build the view
with `from_source_keyed` instead.

#### `pub fn cell_selection(mut self, sel: CellSelectionModel) -> Self`

Install an independent cell-selection model on top of row selection.
See `CellSelectionModel`.

#### `pub fn alternating_rows(mut self, enabled: bool) -> Self`

Paint every other row with a tinted background. Default: off.

#### `pub fn grid_lines(mut self, kind: GridLines) -> Self`

Draw horizontal and/or vertical grid lines between cells.
See `GridLines`.

#### `pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self`

Provide an accessible label for the table (`aria-label`). Required
when the page hosts more than one table so screen readers can
distinguish them.

#### `pub fn show_internal_scrollbars(mut self, show: bool) -> Self`

Show or hide the built-in vertical scroll bar. Default: visible. Set to
`false` when an external scroll bar is wired to `scroll_y_signal`.

#### `pub fn empty_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self`

Widget shown when the source is empty.

#### `pub fn scroll_y_signal(&self) -> &Signal<f32>`

Current vertical scroll offset in logical pixels.

#### `pub fn max_scroll_y_signal(&self) -> &Signal<f32>`

Maximum vertical scroll offset — `total_content_height − viewport_height`.

#### `pub fn viewport_ratio_y_signal(&self) -> &Signal<f32>`

Viewport-to-content height ratio, used by external scroll bar thumbs.

#### `pub fn scroll_x_signal(&self) -> &Signal<f32>`

Current horizontal scroll offset of the Middle (unpinned) pane, in
logical pixels. Leading/Trailing-pinned columns are unaffected —
see `Column::pinned`.

#### `pub fn max_scroll_x_signal(&self) -> &Signal<f32>`

Maximum horizontal scroll offset — `middle_content_width −
middle_viewport_width`.

#### `pub fn viewport_ratio_x_signal(&self) -> &Signal<f32>`

Middle-pane viewport-to-content width ratio, used by external
horizontal scroll bar thumbs.

#### `pub fn sort_signal(&self) -> &Signal<Option<(String, SortDirection)>>`

Active sort: `Some((col_id, dir))` or `None` when unsorted.
Mutated by header clicks (cycle: None → Asc → Desc → None) and by
`set_sort` / `clear_sort`.
Bind a `SortFilterListModel` to
drive a re-sort of the underlying data:

```ignore
let proxy = SortFilterListModel::new(model)
    .with_comparator("name", |a, b| a.name.cmp(&b.name));
proxy.sort_signal(table.sort_signal().clone());
```

#### `pub fn column_widths_signal(&self) -> &Signal<HashMap<String, f32>>`

Map of column id → user-overridden width. A column id appears in
this map only after the user resizes that column; missing keys
mean "use the declared width policy".

#### `pub fn column_order_signal(&self) -> &Signal<Vec<String>>`

Column ids in display order. Updated when the user drags a
header to reorder, or imperatively via
`set_column_order`. When empty, the
declared order applies. Pinned-side groups (Leading / None /
Trailing) are *always* honored — the entries inside this signal
only re-sort within each group.

#### `pub fn column_pinning_signal(&self) -> &Signal<HashMap<String, PinnedSide>>`

Per-id pinning override map. A key here pins the column to that
side; missing keys fall back to the declared `Column::pinned`.
Updated when the user drags a column across a pane boundary.

#### `pub fn focused_cell_signal(&self) -> &Signal<Option<(usize, usize)>>`

Currently keyboard-focused cell, as `(row_index, display_col)`,
or `None` when no cell is focused. Mutated by the keyboard
handler (Arrow keys / Tab / Home / End / PgUp / PgDn /
Ctrl-Home / Ctrl-End / Escape) and by direct
`set_focused_cell` /
`clear_focused_cell` calls.

#### `pub fn set_focused_cell(&self, row: usize, col: usize)`

Move the focused cell. Out-of-range values are silently clamped
when the next layout runs.

#### `pub fn clear_focused_cell(&self)`

Remove keyboard focus from any cell (equivalent to pressing Escape).

#### `pub fn editing_cell_signal(&self) -> &Signal<Option<(usize, usize)>>`

Cell currently in edit mode, or `None` when no editor is open.
Cell delegates inspect this via `CellContext::is_editing` and
swap in an editor widget when matched.

#### `pub fn begin_edit(&self, row: usize, col_id: &str)`

Begin editing the cell `(row, col_id)`. Silently no-ops if `col_id`
isn't a currently-displayed column, or if `row` is outside the visible
range — an out-of-range target would otherwise strand `editing_cell` on
a row nothing can match.

Callable **before the view is mounted**, which is the only point at
which a consumer can seed a freshly constructed view with an edit
target it already holds. `display_indices` is a cache `build()` fills,
so a pre-mount call finds it empty; the order is recomputed on demand
in that case rather than resolving against nothing and no-opping for a
third, undocumented reason.

#### `pub fn end_edit(&self)`

Close the active cell editor without committing (the field's `on_blur` still fires).

#### `pub fn filters_signal(&self) -> &Signal<HashMap<String, String>>`

Per-column filter text. Updated by filter affordances in
header cells and by
`set_filter` / `clear_filters`.
Bind a `SortFilterListModel<T>` to drive the upstream data:

```ignore
let proxy = SortFilterListModel::new(model)
    .with_predicate("name", |t| {
        let needle = t.to_string();
        Box::new(move |r: &Row| r.name.contains(&needle))
    });
proxy.filters_signal(table.filters_signal().clone());
```

#### `pub fn set_filter(&self, col_id: &str, text: &str)`

Set or clear the filter text for a single column. An empty `text` removes
the entry for `col_id` (same as clearing the filter for that column).

#### `pub fn clear_filters(&self)`

Remove all active column filters.

#### `pub fn scroll_to_row(&self, row: usize)`

Scroll so that `row` is aligned to the top of the viewport. A no-op
before the first layout pass.

#### `pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection)`

Set the active sort imperatively. Equivalent to writing to
`sort_signal` directly, except that an unchanged
value neither writes nor notifies — see
`set_column_widths`.

#### `pub fn clear_sort(&self)`

Clear the active sort.

#### `pub fn set_column_width(&self, col_id: &str, width: f32)`

Set or remove a single column's user-resized width override.
A non-positive `width` removes the entry (the column reverts to
its declared width policy).

#### `pub fn set_column_widths(&self, widths: HashMap<String, f32>)`

Replace the full width-override map (typically used to restore
a persisted layout).

A no-op when the map is unchanged, so the documented
settings-round-trip wiring (see docs/table-view.md, "Persistence")
terminates instead of recursing: `Signal::set` has no equality check of
its own, and a live resize writes a width on every pointer move.

#### `pub fn set_column_order(&self, order: Vec<String>)`

Replace the column-order list. Ids not declared on this table
are silently dropped on the next layout pass.

#### `pub fn set_column_pinning(&self, col_id: &str, side: PinnedSide)`

Pin or unpin a single column.

#### `pub fn ensure_row_visible(&self, row: usize)`

Scroll the minimum distance needed to make `row` visible. A no-op
before the first layout pass, when the viewport height is not yet known.

## `pub enum ColumnWidth`

How a column's width is determined during layout.

```rust
pub enum ColumnWidth { /* variants */ }
```

### Variants

- **`Fixed`** — Exact pixel width. Clamped by `min_width` / `max_width`.
- **`Flex`** — Share of the leftover space proportional to the flex factor — behaves like CSS `flex-grow`. The factor must be `> 0.0`.
- **`Auto`** — Intrinsic content width (currently approximated by the table's `min_column_width_default` token; refined to probe the header label and visible cells).

## `pub enum PinnedSide`

Whether a column is pinned to one side of the table.

```rust
pub enum PinnedSide { /* variants */ }
```

### Variants

- **`Leading`** — Pinned against the leading edge — stays visible during horizontal scroll.
- **`None`** — Not pinned — scrolls horizontally with the body.
- **`Trailing`** — Pinned against the trailing edge.

## `pub enum Alignment`

Horizontal alignment of a cell's content within its column.

```rust
pub enum Alignment { /* variants */ }
```

### Variants

- **`Leading`**
- **`Center`**
- **`Trailing`**

## `pub enum TruncationPolicy`

Strategy when a cell's text overflows its column.

```rust
pub enum TruncationPolicy { /* variants */ }
```

### Variants

- **`Ellipsis`** — `…`-elide the trailing portion. **Default.**
- **`None`** — Don't truncate; let the cell content draw beyond the column edge (the body pane's clip will hide it).
- **`Fade`** — Fade the trailing portion — gradient mask.

## `pub enum GridLines`

Whether the table draws grid lines between rows / columns.

```rust
pub enum GridLines { /* variants */ }
```

### Variants

- **`None`**
- **`Horizontal`**
- **`Vertical`**
- **`Both`**

## `pub enum ColumnResizePolicy`

Whether column resize commits the new width on every drag tick (`Live`)
or only on `Ended` (`OnRelease`).

```rust
pub enum ColumnResizePolicy { /* variants */ }
```

### Variants

- **`Live`**
- **`OnRelease`**

## `pub enum EditTrigger`

Triggers that cause the table to fire `on_cell_edit_request` on the
focused cell.

```rust
pub enum EditTrigger { /* variants */ }
```

### Variants

- **`F2OrTypeOrDoubleClick`** — All three triggers active. **Default.**
- **`F2`**
- **`F2OrType`**
- **`DoubleClick`**
- **`None`** — Editing disabled — the table does not fire `on_cell_edit_request`.

## `pub enum TabTraversal`

Tab / Shift-Tab traversal policy across cells of a row.

Regardless of the policy, **Ctrl+Tab / Ctrl+Shift+Tab always move focus
out of the table** to the next / previous focusable widget — the reliable
escape from `CellsThenRows`, so keyboard focus is never trapped.

```rust
pub enum TabTraversal { /* variants */ }
```

### Variants

- **`CellsThenRows`** — Tab moves to the next cell within the row, then wraps to the first cell of the next row. **Default.** (Ctrl+Tab leaves the table.)
- **`OutOfTable`** — Tab leaves the table once the focused cell is reached at the row boundary; the focus owner is whatever follows the table in tab order.

## `pub struct CellContext`

Per-cell context handed to a column's cell delegate during build.

```rust
pub struct CellContext { /* fields */ }
```

## `pub struct ColumnContext`

Per-column-header context handed to a column's header delegate.

```rust
pub struct ColumnContext { /* fields */ }
```

## `pub struct Column`

Single column declaration. Column ids must be **stable, unique strings**
— they're the persistence key for sort, filter, width, and ordering.

```rust
pub struct Column<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new( id: impl Into<String>, header: impl Into<LocalizedString>, cell: impl Fn(&T, &CellContext) -> Box<dyn Widget> + 'static, ) -> Self`

Create a column with a stable id, a localized header label, and a
cell builder that takes `&T` plus a `CellContext` and returns a
boxed widget.

#### `pub fn width(mut self, w: ColumnWidth) -> Self`

#### `pub fn min_width(mut self, px: f32) -> Self`

#### `pub fn max_width(mut self, px: f32) -> Self`

#### `pub fn alignment(mut self, a: Alignment) -> Self`

#### `pub fn resizable(mut self, b: bool) -> Self`

#### `pub fn reorderable(mut self, b: bool) -> Self`

#### `pub fn sortable(mut self, b: bool) -> Self`

#### `pub fn filterable(mut self, b: bool) -> Self`

#### `pub fn editable(mut self, b: bool) -> Self`

Mark the column as editable. Default `false`. F2 / type-to-edit
only enter edit mode on cells of editable columns; the
`on_cell_edit_request` hook also fires only for these. Cells of
non-editable columns continue to render their static delegate
regardless of `editing_cell`.

#### `pub fn pinned(mut self, side: PinnedSide) -> Self`

#### `pub fn truncation(mut self, p: TruncationPolicy) -> Self`

#### `pub fn header_override( mut self, f: impl Fn(&ColumnContext) -> Box<dyn Widget> + 'static, ) -> Self`

Override the default header rendering (label + sort/filter
indicators). The closure receives a `ColumnContext` reflecting
the current sort/filter state.

#### `pub fn id(&self) -> &str`

Stable column id (the persistence key for sort, filter, width,
and ordering signals).

## `pub enum TableSelectionMode`

Selection mode for a `TableView` or `TreeTableView`.

```rust
pub enum TableSelectionMode { /* variants */ }
```

### Variants

- **`None`** — No selection allowed.
- **`SingleRow`** — At most one row selected at a time.
- **`MultiRow`** — Multiple rows selectable; Ctrl-click toggles, Shift-click extends. **Default.**
- **`SingleCell`** — Excel-style: at most one cell selected at a time.
- **`MultiCell`** — Excel-style: rectangular cell selection.

### Methods

#### `pub fn is_cell_mode(self) -> bool`

Whether the mode operates on cells rather than entire rows.

#### `pub fn is_multi(self) -> bool`

Whether the mode allows more than one entry to be selected.

## `pub struct CellSelectionModel`

Cell-level selection state for `TableSelectionMode::SingleCell` /
`MultiCell`. Tracks `(row, col)` pairs in visible-index space.

Mirrors `teksilo_data::SelectionModel`'s API surface (signal-backed,
auto-adjustable on data mutations) but keyed by `(row, col)` instead of
`row` alone.

```rust
pub struct CellSelectionModel { /* fields */ }
```

### Methods

#### `pub fn new(mode: TableSelectionMode) -> Self`

Construct a model. **Panics** if `mode` is not a cell mode —
callers in row mode should use `teksilo_data::SelectionModel`.

#### `pub fn mode(&self) -> TableSelectionMode`

#### `pub fn selection_signal(&self) -> Signal<BTreeSet<(usize, usize)>>`

#### `pub fn is_selected(&self, row: usize, col: usize) -> bool`

#### `pub fn count(&self) -> usize`

#### `pub fn select(&self, row: usize, col: usize)`

Replace the selection with the single cell `(row, col)` and set
the anchor.

#### `pub fn toggle(&self, row: usize, col: usize)`

Toggle the cell `(row, col)` (Ctrl-click). In `SingleCell` mode
this behaves like `select`.

#### `pub fn extend_to(&self, row: usize, col: usize)`

Extend the selection to include the rectangular range from the
anchor to `(row, col)`. In `SingleCell` mode this falls back to
`select`.

#### `pub fn select_all(&self, row_count: usize, col_count: usize)`

Select every cell in `0..row_count × 0..col_count`.

#### `pub fn clear(&self)`

#### `pub fn adjust_for_row_insert(&self, at_row: usize, count: usize)`

Adjust selection after `count` rows are inserted starting at
`at_row`. Existing selections at indices `>= at_row` shift up.

#### `pub fn adjust_for_row_remove(&self, at_row: usize, count: usize)`

Adjust selection after `count` rows starting at `at_row` are
removed. Selections within the removed range are dropped; later
rows shift down.

#### `pub fn adjust_for_row_move(&self, from: usize, to: usize, count: usize)`

Adjust selection after a block of `count` rows moved from `from` to
`to` (a post-removal index, matching `ListModel::move_item`). Selected
cells follow their rows; columns are untouched.

#### `pub fn adjust_for_column_insert(&self, at_col: usize, count: usize)`

Adjust selection after `count` columns are inserted at `at_col`.

Reserved for future dynamic-column support. `TableView`/`TreeTableView`
columns are declared once via `.add_column()`/`.columns()` and are
static for the widget's lifetime — there is no runtime insert/remove
API today, so nothing calls this. A column *reorder* or pin-toggle
permutes positions instead (see `remap_columns`),
which is what the current views actually use. Kept (not removed) as
public API in case a future dynamic-column feature needs the
offset-shift semantics this and `adjust_for_column_remove`
already implement and test.

#### `pub fn adjust_for_column_remove(&self, at_col: usize, count: usize)`

Adjust selection after `count` columns starting at `at_col` are
removed.

Reserved for future dynamic-column support — see the doc comment on
`adjust_for_column_insert`; nothing
calls this today for the same reason.
