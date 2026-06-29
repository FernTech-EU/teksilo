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
# use bastyde_data::{ListModel, ListDataSource};
// ListModel<T> implements ListDataSource — pass it directly to any flat view.
let model = ListModel::from_vec(vec!["alpha", "beta", "gamma"]);
// Access via the ListDataSource interface:
let _len = model.len();
let _first = model.with_item(0, |s| *s);
assert_eq!(_len, 3);
assert_eq!(_first, Some("alpha"));
```

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/list_data_source/index.html)
