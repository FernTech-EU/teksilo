<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TableHeader

`TableHeader` — the column header strip of `TableView`
and `TreeTableView`, and a widget of its own for an
application that lays out its own rows under columns: a section list with
full-width group rows, an album card beside its tracks.

Both views compose this widget, so there is one header implementation: the
`HeaderCell`s (label, sort indicator, filter popover, the resize grip on
each divider, the column-reorder drag, `Role::ColumnHeader` with its sort
direction and AccessKit `Increment` / `Decrement`), the strip that lays them
out in pinned and scrolling bands and paints their separators, and the drop
target that turns a reorder drag into a new column order.

## State

The header's state is six signals — the width overrides, the sort, the
column order, the pinning overrides, the filters and the horizontal scroll
offset. Each builder (`widths`,
`sort`, `order`,
`pinning`, `filters`,
`scroll_x`) adopts an application's signal; one
that is not given is the header's own, read back through its getter. The
rules are the views' ("Column state an application owns" in the
`table_view` module docs): the header writes nothing
into these signals until the user acts on it, ignores ids it does not
declare, and keeps them when it writes. Hand the same signals to a
`TableView` and the two stay in step.

The width map holds **overrides**, not the layout: a column with no entry
takes its declared `ColumnWidth`. What the header actually laid each
column out at is `resolved_widths_signal`,
which is what rows of an application's own read to line their cells up.

## Hosted

Inside a view the header is *hosted* (`TableHeader::hosted`): the view
resolves the widths and the display order (its body needs both before the
header is placed), paints the `OnRelease` resize guide across the whole
table, rebuilds the header whenever its state changes, and holds the
resize-drag state, which a relayout keeps. A rebuild abandons an
in-flight resize, hosted or not: it destroys the cells and the pointer
capture with them (see `TableHeader::build`). A standalone header does
all of that itself, within its own bounds, which it clips to.

## Builder methods at a glance

`widths`, `sort`, `order`, `pinning`, `filters`, `scroll_x`, `resize_policy`, `stretch_last_column`, `widths_signal`, `sort_signal`, `order_signal`, `pinning_signal`, `filters_signal`, `scroll_x_signal`, `resolved_widths_signal`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/table_view/table_header/index.html)

## `pub struct TableHeader`

A table's column header strip: one `Role::ColumnHeader` cell per column,
with click-to-sort, drag-to-resize on every divider, drag-to-reorder,
pinned columns, the filter popover and a horizontal scroll offset to follow
a scrolled body.

```ignore
let widths = Signal::new(HashMap::new());
let sort = Signal::new(None);
let header = TableHeader::new(vec![
    ColumnSpec::new("title", lit!("Title")).sortable(true),
    ColumnSpec::new("length", lit!("Length"))
        .width(ColumnWidth::Fixed(80.0))
        .sortable(true),
])
.widths(widths.clone())
.sort(sort.clone());
let layout = header.resolved_widths_signal().clone(); // line rows up with this
```

A header announces itself as the `Role::Row` with row index 1, its cells as
`Role::ColumnHeader`s, so place it inside the container that announces the
rows under it as a table or a grid.

See the `module docs` for how its state is
shared and for how a view hosts it.

```rust
pub struct TableHeader { /* fields */ }
```

### Methods

#### `pub fn new(columns: Vec<ColumnSpec>) -> Self`

A header over `columns`, in declaration order. Every piece of state it
is not handed is its own.

#### `pub fn widths(mut self, widths: Signal<HashMap<String, f32>>) -> Self`

Adopt `widths` as the map of column id → width override; a column
with no entry takes its declared `ColumnWidth`. A resize drag writes
the resized column only. Each `Flex` column before it keeps its
current width in this header, but not in the map, which other views
may share at other widths.

#### `pub fn sort(mut self, sort: Signal<Option<(String, SortDirection)>>) -> Self`

Adopt `sort` as the active sort. A click on a sortable column cycles it
None → ascending → descending → None.

#### `pub fn order(mut self, order: Signal<Vec<String>>) -> Self`

Adopt `order` as the column order, a list of column ids; columns it
does not name follow in declaration order. A reorder drop writes it.

#### `pub fn pinning(mut self, pinning: Signal<HashMap<String, PinnedSide>>) -> Self`

Adopt `pinning` as the per-column pinning overrides, which win over
each `ColumnSpec::pinned`; `PinnedSide::None` unpins a column
declared pinned. A reorder drop writes it: an entry when the drop puts
the column in a pane other than the one it declares, none when it
puts it back.

#### `pub fn filters(mut self, filters: Signal<HashMap<String, String>>) -> Self`

Adopt `filters` as the per-column filter text the filter popover of a
`filterable` column edits.

#### `pub fn scroll_x(mut self, scroll_x: Signal<f32>) -> Self`

Follow `scroll_x`, the horizontal offset of the body under the header:
the unpinned columns shift left by it, the pinned ones stay put.

#### `pub fn resize_policy(mut self, policy: ColumnResizePolicy) -> Self`

Whether a resize writes its width on every pointer move (`Live`, the
default) or on release (`OnRelease`, with a guide line meanwhile).

#### `pub fn stretch_last_column(mut self, on: bool) -> Self`

Let the last column in display order take the width the others leave
— see `TableView::stretch_last_column`.
Default off.

#### `pub fn widths_signal(&self) -> &Signal<HashMap<String, f32>>`

The width overrides — the signal `widths` adopted, or
the header's own.

#### `pub fn sort_signal(&self) -> &Signal<Option<(String, SortDirection)>>`

The active sort — the signal `sort` adopted, or the
header's own.

#### `pub fn order_signal(&self) -> &Signal<Vec<String>>`

The column order — the signal `order` adopted, or the
header's own.

#### `pub fn pinning_signal(&self) -> &Signal<HashMap<String, PinnedSide>>`

The pinning overrides — the signal `pinning`
adopted, or the header's own.

#### `pub fn filters_signal(&self) -> &Signal<HashMap<String, String>>`

The filters — the signal `filters` adopted, or the
header's own.

#### `pub fn scroll_x_signal(&self) -> &Signal<f32>`

The horizontal offset the header follows — the signal
`scroll_x` adopted, or the header's own (which
stays at 0).

#### `pub fn resolved_widths_signal(&self) -> &Signal<Vec<(String, f32)>>`

`(column id, width)` for each displayed column, in display order: the
widths the header laid its cells out at, after the overrides, the
declared widths, the `Flex` share and the min / max clamps. Written
when the header is placed in a layout pass, and only when it changed;
bind a row's layout to it to line the row's cells up with the columns.
Leading-pinned columns come first and do not move with `scroll_x`, nor
do the trailing-pinned ones at the end.

**Rows follow it one layout pass late**, unless they read it in their
own `place_children` and are laid out after the header (below it in
the same stack): only the header's bounds settle the widths, and a row
that sizes itself from them in `layout_response` (a `FixedSize` bound
to a width derived from this, say) has been measured by then, so it is
relaid out on the next pass. During a live resize drag such a row
trails the header by a frame, and a headless test needs a second
`layout()` before it lines up.

Published by a standalone header only; a view hosting the header lays
its rows out from the same widths directly.
