<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ListDataSource

`ListDataSource` — read-and-command interface for a flat collection behind a `ListView` /
`TableView`.

## Public functions

### `ListDataSource`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `usize` | [`len()`](#listdatasource-len) |
| `bool { /* default implementation */ }` | [`is_empty()`](#listdatasource-is_empty) |
| `Option<R>` | [`with_item<R>(index: usize, f: impl FnOnce(&Self::Item) -> R)`](#listdatasource-with_item) |
| `Option<Self::Key> { /* default implementation */ }` | [`key_at(_index: usize)`](#listdatasource-key_at) |
| `Option<usize> { /* default implementation */ }` | [`index_of(_key: &Self::Key)`](#listdatasource-index_of) |
| `ObserverHandle` | [`observe_changes(f: impl Fn(&DataChange) + 'static)`](#listdatasource-observe_changes) |
| `Option<usize> { /* default implementation */ }` | [`first_changed_index()`](#listdatasource-first_changed_index) |
| `DragEligibility { /* default implementation */ }` | [`drag(_key: &Self::Key)`](#listdatasource-drag) |
| `DropResponse { /* default implementation */ }` | [`can_accept(_query: &DropQuery<'_, Self::Key>)`](#listdatasource-can_accept) |
| `bool { /* default implementation */ }` | [`accept_drop(_commit: DropCommit<'_, Self::Key>)`](#listdatasource-accept_drop) |
| `bool { /* default implementation */ }` | [`reorder_within(sources: &[Self::Key], target: &Self::Key, position: DropPosition)`](#listdatasource-reorder_within) |
|  | [`on_drag_out(_key: &Self::Key)`](#listdatasource-on_drag_out) |
| `RowState { /* default implementation */ }` | [`row_state(_index: usize)`](#listdatasource-row_state) |
|  | [`request_window(_range: Range<usize>)`](#listdatasource-request_window) |
| `bool { /* default implementation */ }` | [`can_fetch_more()`](#listdatasource-can_fetch_more) |
|  | [`fetch_more()`](#listdatasource-fetch_more) |
| | **Constants and types** |
| `type` | [`Item`](#listdatasource-item) |
| `type` | [`Key`](#listdatasource-key) |

## Detailed description

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

#### When to use

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

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/list_data_source/index.html)

<a id="listdatasource"></a>

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

<a id="listdatasource-item"></a>

#### `type Item: 'static;`

The item type exposed by this data source.

<a id="listdatasource-key"></a>

#### `type Key: ItemKey;`

The stable per-row identity. In-memory `ListModel` uses `usize` (the
index); external sources use their own domain key so keyed selection /
DnD survive reorders without a mirror model.

<a id="listdatasource-len"></a>

#### `fn len(&self) -> usize;`

Number of rows (the **total**, including not-yet-loaded ones for a
windowed source — the scrollbar needs it).

<a id="listdatasource-is_empty"></a>

#### `fn is_empty(&self) -> bool { /* default implementation */ }`

Whether the source is empty.

<a id="listdatasource-with_item"></a>

#### `fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R>;`

Access the item at `index` via a callback. Returns `None` for an
out-of-bounds index OR an in-bounds index whose data is still
`Loading` (see `row_state`).

<a id="listdatasource-key_at"></a>

#### `fn key_at(&self, _index: usize) -> Option<Self::Key> { /* default implementation */ }`

The stable key of the row at `index`. Default `None` (no identity);
sources that support keyed selection / DnD override it.

<a id="listdatasource-index_of"></a>

#### `fn index_of(&self, _key: &Self::Key) -> Option<usize> { /* default implementation */ }`

The index of a key, if currently present. Default `None`.

<a id="listdatasource-observe_changes"></a>

#### `fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle;`

Register an observer that is called on every mutation; dropping the
returned `ObserverHandle` unregisters the callback automatically.

<a id="listdatasource-first_changed_index"></a>

#### `fn first_changed_index(&self) -> Option<usize> { /* default implementation */ }`

First index whose content may differ after the change just delivered —
rows `0..index` are unchanged. `None` means unknown (full change).

<a id="listdatasource-drag"></a>

#### `fn drag(&self, _key: &Self::Key) -> DragEligibility { /* default implementation */ }`

Whether the row may begin a drag (the transferable gate).

<a id="listdatasource-can_accept"></a>

#### `fn can_accept(&self, _query: &DropQuery<'_, Self::Key>) -> DropResponse { /* default implementation */ }`

Whether a hovered drop is permitted (and where) — the pre-commit verdict.

<a id="listdatasource-accept_drop"></a>

#### `fn accept_drop(&self, _commit: DropCommit<'_, Self::Key>) -> bool { /* default implementation */ }`

Apply a committed drop. Returns whether it was applied.

<a id="listdatasource-reorder_within"></a>

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

<a id="listdatasource-on_drag_out"></a>

#### `fn on_drag_out(&self, _key: &Self::Key) { /* default implementation */ }`

Called on the *origin* source after one of its rows was accepted by a
different view (source-side completion). Shared/command-backed sources
no-op this; independent models use it to drop the moved row.

<a id="listdatasource-row_state"></a>

#### `fn row_state(&self, _index: usize) -> RowState { /* default implementation */ }`

Whether the row at `index` is loaded.

<a id="listdatasource-request_window"></a>

#### `fn request_window(&self, _range: Range<usize>) { /* default implementation */ }`

Nudge the source to load the given range (the view calls this each build
with its visible + buffer window).

<a id="listdatasource-can_fetch_more"></a>

#### `fn can_fetch_more(&self) -> bool { /* default implementation */ }`

Whether more rows can be appended (infinite scroll).

<a id="listdatasource-fetch_more"></a>

#### `fn fetch_more(&self) { /* default implementation */ }`

Fetch the next page (append-only growth).
