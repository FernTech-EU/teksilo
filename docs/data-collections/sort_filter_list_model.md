<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SortFilterListModel

Composable sort + filter projection over a flat list source.

`SortFilterListModel<T>` wraps a `ListModel<T>` or any
`ListDataSource<Item = T>` and exposes a `ListDataSource<Item = T>`
whose visible item order is determined by:

1. **Filtering**: each column may register a predicate factory; rows that
   fail any non-empty filter are hidden.
2. **Sorting**: at most one column may carry an active sort direction;
   rows are reordered by the column's registered comparator.

Filter is applied first, sort second. The result is a flat reactive view
that drops directly into `TableView`, `ListView`, or `Repeater` via
`from_source(...)`.

## Reactivity

Three independent change vectors trigger a rebuild of the visible-index
map:

- The upstream source emits any `DataChange`. Most changes collapse to
  a single `DataChange::Reset` for the proxy's own observers —
  translating fine-grained inserts / removes / moves through a sort
  projection is correctness-fragile (an item's sort key can move it to a
  different visible row), so `Reset` is the safe default contract. The
  one exception is [`DataChange::ItemUpdated`]: the proxy re-evaluates
  just that row's filter verdict and its position against its current
  visible neighbours (not the whole list), and if neither changed,
  forwards a scoped `ItemUpdated` at the mapped visible index instead of
  paying for a full re-filter + re-sort + `Reset` on every edit to a
  live-updating source. Any verdict change (entering/leaving the visible
  set, or needing to move past a neighbour) still falls back to the full
  rebuild.
- A bound sort signal updates: rebuild and emit `Reset`.
- A bound filters signal updates: rebuild and emit `Reset`.

## Selection semantics

Selection on a sorted/filtered view is naturally tracked by **visible
index**, not by item identity. After a projection rebuild, a downstream
`SelectionModel` keeps the same numerical
indices selected — meaning the visual selection stays in place even
though it now points at different underlying rows. Apps that want
identity-based selection should observe their model directly and rewrite
the selection from source identifiers on each rebuild.

```rust
# use bastyde_data::{ListModel, SortFilterListModel, SortDirection};
# use bastyde_data::ListDataSource; // brings `len()` into scope
#[derive(Clone, Debug)]
struct Person { name: String, age: u32 }

let model: ListModel<Person> = ListModel::new();
model.push(Person { name: "Carol".into(), age: 30 });
model.push(Person { name: "Alice".into(), age: 25 });
model.push(Person { name: "Bob".into(), age: 28 });

let proxy = SortFilterListModel::new(model)
    .with_comparator("name", |a: &Person, b| a.name.cmp(&b.name))
    .with_predicate("name", |text| {
        let t = text.to_lowercase();
        Box::new(move |p: &Person| p.name.to_lowercase().contains(&t))
    });

proxy.set_sort(Some("name"), SortDirection::Ascending);
assert_eq!(proxy.len(), 3); // Alice, Bob, Carol

proxy.set_filter("name", "a");
assert_eq!(proxy.len(), 2); // Alice, Carol
```

## Builder methods at a glance

`from_source`, `with_comparator`, `with_predicate`, `sort_signal`, `filters_signal`, `set_sort`, `clear_sort`, `set_filter`, `clear_filters`, `first_changed_index`, `source_index_of`, `visible_index_of`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/sort_filter_list_model/index.html)

## `pub enum SortDirection`

Sort direction emitted by `TableView` / `TreeTableView` headers and consumed by sort projections.

```rust
pub enum SortDirection { /* variants */ }
```

### Variants

- **`Ascending`** — Sort from smallest to largest (A → Z, 0 → 9).
- **`Descending`** — Sort from largest to smallest (Z → A, 9 → 0).

## `pub struct SortFilterListModel`

Flat list source projecting an upstream `ListModel<T>` /
`ListDataSource<Item = T>` through sort + filter.

See module-level documentation for semantics.

```rust
pub struct SortFilterListModel<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new(model: ListModel<T>) -> Self`

Wrap a `ListModel<T>`.

#### `pub fn from_source<S: ListDataSource<Item = T>>(source: S) -> Self`

Wrap any `ListDataSource<Item = T>`.

#### `pub fn with_comparator( self, col_id: impl Into<String>, cmp: impl Fn(&T, &T) -> Ordering + 'static, ) -> Self`

Register a comparator for a column id. Chainable.

#### `pub fn with_predicate( self, col_id: impl Into<String>, factory: impl Fn(&str) -> Box<dyn Fn(&T) -> bool> + 'static, ) -> Self`

Register a predicate factory for a column id. The factory receives the
current filter text (empty = no filter, never invoked) and returns a
boxed predicate evaluated against each row. Chainable.

#### `pub fn sort_signal(&self, signal: Signal<Option<(String, SortDirection)>>)`

Bind a sort signal — typically `TableView::sort_signal()`. Updates
re-project the view. The current value is read once at bind time.

#### `pub fn filters_signal(&self, signal: Signal<HashMap<String, String>>)`

Bind a filters signal — typically `TableView::filters_signal()`.
Updates re-project the view. The current value is read once at bind
time.

#### `pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection)`

Set the active sort imperatively. If a sort signal is bound this
writes through the signal; otherwise it mutates internal state and
emits `DataChange::Reset` directly.

#### `pub fn clear_sort(&self)`

Clear the active sort.

#### `pub fn set_filter(&self, col_id: &str, text: &str)`

Set or clear a single column's filter. An empty `text` removes the
entry. If a filters signal is bound this writes through the signal.

#### `pub fn clear_filters(&self)`

Clear every column's filter.

#### `pub fn first_changed_index(&self) -> Option<usize>`

First visible index whose content may differ from before the
latest projection rebuild — rows `0..index` show the same items in
the same order as before, so per-row derived state (e.g. a
measured row height) remains valid for them. Equal to `len()` when
the visible list is unchanged. Renumbering from upstream
inserts/removes/moves is accounted for (equal source-index values
above the change point are not trusted).

`None` means unknown (no rebuild observed yet) — treat as a full
change. The value describes the **latest** rebuild only; read it
synchronously from a `DataChange` observer (callbacks fire inline
on every rebuild, so per-change reads cannot miss a value). The
`DataChange::Reset` contract for observers is unchanged — this is
a side-channel for consumers that can exploit a valid prefix.

#### `pub fn source_index_of(&self, visible: usize) -> Option<usize>`

Map a visible (post sort+filter) index to its source index.

#### `pub fn visible_index_of(&self, source: usize) -> Option<usize>`

Map an underlying source index to its visible position, if shown.
Builds a reverse-index lazily; subsequent calls in the same projection
epoch are O(1).
