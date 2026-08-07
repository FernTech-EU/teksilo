<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DataChange

`DataChange` — change notifications for flat collections.

Describes the mutations that `crate::ListModel` (and `crate::ListDataSource`
implementors) emit to their subscribers. Consumers such as `ListView`,
`TableView`, and `SortFilterListModel` receive a `DataChange` through their
observer and update their internal state (measured row heights, selection
indices, sort projections) incrementally rather than rebuilding from scratch.

Most variants carry index ranges so that observers can perform O(affected)
work. `Reset` is the fallback when the change cannot be expressed
incrementally; consumers must discard all cached state and re-query the source.

Also provided: `map_index_after_move`, a pure function that maps a single
index through an `ItemsMoved` operation — used by `crate::CheckedModel` and
`crate::SelectionModel` to keep index-based state in sync after reorders.

```rust
# use teksilo_data::data_change::{DataChange, map_index_after_move};
// An insertion at row 2 shifts index 5 to 6.
let change = DataChange::ItemsInserted { range: 2..3 };
// map_index_after_move: move row 0 to position 2 (post-removal index).
let new_idx = map_index_after_move(0, 0, 2, 1);
assert_eq!(new_idx, 2);
```

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_data/data_change/index.html)

## `pub enum DataChange`

Describes a mutation to a flat list. Emitted by `crate::ListModel` automatically
and by `crate::ListDataSource` implementors manually.

```rust
pub enum DataChange { /* variants */ }
```

### Variants

- **`ItemsInserted`** — Rows were inserted; `range` holds the indices of the newly inserted items.
- **`ItemsRemoved`** — Rows were removed; `range` holds the indices they occupied *before* removal.
- **`ItemsMoved`** — A contiguous block of `count` rows moved from `from` to `to` (post-removal index).
- **`ItemUpdated`** — A single row's data changed in place without any structural shift.
- **`WindowLoaded`** — A window of previously-`Loading` rows became `Ready` (lazy / windowed sources). Semantically like `ItemsInserted` for a row-height cache (divergence = `range.start`), but no rows were added — the count was already declared — so a `SelectionModel` must NOT index-shift for it.
- **`Reset`** — The entire list was replaced; consumers must discard all cached state and rebuild.

## `pub fn map_index_after_move(...)`

Map an index through a `DataChange::ItemsMoved { from, to, count }`.

Mirrors `ListModel::move_item`: the contiguous block `from..from+count` is
removed, then reinserted so its first item lands at `to` (a *post-removal*
index). Returns where `idx` ends up after the move. Used by index-based
state (selection, checked-set) to follow items across a reorder.

```rust
pub fn map_index_after_move(idx: usize, from: usize, to: usize, count: usize) -> usize;
```

## `pub fn adjust_single_index_for_change(...)`

Map a **single** index anchor (not a selection set) through a
`DataChange`, or `None` if the row the anchor pointed at no longer
exists (it was removed, or the whole list was reset).

This is the same shift semantics as `map_index_after_move` /
`SelectionModel::adjust_for_*` / `CheckedModel::adjust_for_*`, specialized
for a bare `Option<usize>` anchor that has no "membership" to prune —
e.g. `ListView`'s keyboard-focus index. Used so a single-anchor consumer
doesn't have to re-derive insert/remove/move shift logic by hand.

- `ItemsInserted`: the anchor shifts up by the inserted count if it sat
  at or after the insertion point, otherwise it's untouched.
- `ItemsRemoved`: the anchor shifts down past the removed range; if the
  anchor itself pointed *into* the removed range, it is dropped (`None`)
  — the row it followed is gone.
- `ItemsMoved`: delegates to `map_index_after_move` (the anchor follows
  its row, or shifts around the moved block like everyone else).
- `ItemUpdated` / `WindowLoaded`: no structural shift — the anchor is
  unchanged.
- `Reset`: the anchor is dropped (`None`) — nothing about the old
  indexing survives a wholesale replacement.

```rust
pub fn adjust_single_index_for_change(idx: usize, change: &DataChange) -> Option<usize>;
```
