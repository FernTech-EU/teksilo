<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ListDataSource

`ListDataSource` — read-and-command interface for a flat collection behind a `ListView` /
`TableView`.

`ListDataSource` is the flat-list peer of
`TreeDataSource`: a positional read API plus the
capability protocol (identity, DnD validation, lazy loading). It is the
input every flat data view reads through. The built-in `ListModel<T>` and
`SortFilterListModel<T>` implement it; an external/huge source
(a paged database cursor, a 1M-row windowed feed) implements it directly and owns its
own paging behind `row_state`/`request_window`/`fetch_more`.

Not object-safe (associated types + generic `with_item`); `ListView`
consumes it generically via `ListView::from_source` and erases it into a
closure bundle. The DnD and lazy methods default to inert / fully-resident,
so a read-only in-memory source implements only `len` + `with_item` +
`observe_changes`.

## When to use

Prefer `ListModel<T>` when your data fits in memory and you want
automatic `DataChange` notifications with no extra work. Implement `ListDataSource`
directly when the source is external, huge, or requires lazy window-based loading —
the view calls `request_window` each build pass and `fetch_more` near the end.

```rust
# use teksilo_data::{ListModel, ListDataSource};
// ListModel<T> implements ListDataSource — pass it directly to any flat view.
let model = ListModel::from_vec(vec!["alpha", "beta", "gamma"]);
// Access via the ListDataSource interface:
let _len = model.len();
let _first = model.with_item(0, |s| *s);
assert_eq!(_len, 3);
assert_eq!(_first, Some("alpha"));
```

## Builder methods at a glance

`Item`, `Key`, `len`, `is_empty`, `with_item`, `key_at`, `index_of`, `observe_changes`, `first_changed_index`, `drag`, `can_accept`, `accept_drop`, `reorder_within`, `on_drag_out`, `row_state`, `request_window`, `can_fetch_more`, `fetch_more`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/list_data_source/index.html)

## `pub trait ListDataSource`

A data source for a flat collection viewed by `ListView`, `TableView`, and `GridView`.

The trait separates the read interface (`len`, `with_item`) from the capability
protocol: identity (`key_at`/`index_of`), drag-and-drop validation
(`drag`/`can_accept`/`accept_drop`/`on_drag_out`), and lazy loading
(`row_state`/`request_window`/`can_fetch_more`/`fetch_more`). All capability
methods have inert defaults, so a minimal implementation only needs `len`,
`with_item`, and `observe_changes`.

```rust
pub trait ListDataSource: 'static { /* associated items below */ }
```

### Associated items

#### `type Item: 'static;`

The item type exposed by this data source.

#### `type Key: ItemKey;`

The stable per-row identity. In-memory `ListModel` uses `usize` (the
index); external sources use their own domain key so keyed selection /
DnD survive reorders without a mirror model.

#### `fn len(&self) -> usize;`

Number of rows (the **total**, including not-yet-loaded ones for a
windowed source — the scrollbar needs it).

#### `fn is_empty(&self) -> bool { /* default implementation */ }`

Whether the source is empty.

#### `fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R>;`

Access the item at `index` via a callback. Returns `None` for an
out-of-bounds index OR an in-bounds index whose data is still
`Loading` (see `row_state`).

#### `fn key_at(&self, _index: usize) -> Option<Self::Key> { /* default implementation */ }`

The stable key of the row at `index`. Default `None` (no identity);
sources that support keyed selection / DnD override it.

#### `fn index_of(&self, _key: &Self::Key) -> Option<usize> { /* default implementation */ }`

The index of a key, if currently present. Default `None`.

#### `fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle;`

Register an observer that is called on every mutation; dropping the
returned `ObserverHandle` unregisters the callback automatically.

#### `fn first_changed_index(&self) -> Option<usize> { /* default implementation */ }`

First index whose content may differ after the change just delivered —
rows `0..index` are unchanged. `None` means unknown (full change).

#### `fn drag(&self, _key: &Self::Key) -> DragEligibility { /* default implementation */ }`

Whether the row may begin a drag (the transferable gate).

#### `fn can_accept(&self, _query: &DropQuery<'_, Self::Key>) -> DropResponse { /* default implementation */ }`

Whether a hovered drop is permitted (and where) — the pre-commit verdict.

#### `fn accept_drop(&self, _commit: DropCommit<'_, Self::Key>) -> bool { /* default implementation */ }`

Apply a committed drop. Returns whether it was applied.

#### `fn reorder_within( &self, sources: &[Self::Key], target: &Self::Key, position: DropPosition, ) -> bool { /* default implementation */ }`

Reorder a whole set of this source's OWN rows so they land contiguously
at a drop gap — the multi-row same-view reorder commit. `sources` are the
dragged rows' keys in the origin's visible order; `target` / `position`
name the drop gap. Returns whether anything moved.

The default moves them one at a time, re-anchoring each after the
previous so they stay contiguous and keep their relative order — correct
for a source with **stable** keys. `ListModel`, whose
key *is* the index (so a single move renumbers everything), overrides
this with a direct block move; a single-row drag needs neither and just
falls through to one `accept_drop`.

#### `fn on_drag_out(&self, _key: &Self::Key) { /* default implementation */ }`

Called on the *origin* source after one of its rows was accepted by a
different view (source-side completion). Shared/command-backed sources
no-op this; independent models use it to drop the moved row.

#### `fn row_state(&self, _index: usize) -> RowState { /* default implementation */ }`

Whether the row at `index` is loaded.

#### `fn request_window(&self, _range: Range<usize>) { /* default implementation */ }`

Nudge the source to load the given range (the view calls this each build
with its visible + buffer window).

#### `fn can_fetch_more(&self) -> bool { /* default implementation */ }`

Whether more rows can be appended (infinite scroll).

#### `fn fetch_more(&self) { /* default implementation */ }`

Fetch the next page (append-only growth).
