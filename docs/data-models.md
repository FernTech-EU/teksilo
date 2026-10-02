<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Data models

Use `teksilo-data` to share observable collections between application state and
views. Cloning a model handle shares its state; it does not copy the collection.

## Minimal example

```rust
use teksilo_data::ListModel;

fn main() {
    let items = ListModel::from_vec(vec![String::from("First")]);
    let _observer = items.observe_changes(|change| println!("{change:?}"));
    items.push(String::from("Second"));
    assert_eq!(items.len(), 2);
}
```

Keep the observer handle alive while the subscription is needed. Drop it to
unsubscribe. A view observes its model to update its displayed items.

## Choose a model

| Need | Type |
| --- | --- |
| In-memory list | `ListModel<T>` |
| Sorted or filtered list | `SortFilterListModel<T>` |
| In-memory hierarchy | `TreeModel<T>` |
| Independent expansion of a shared tree | `TreeSlice<T>` |
| Sorted or filtered tree | `SortFilterTreeModel<T>` |
| External or lazily loaded data | `ListDataSource` or `TreeDataSource` |
| External outline with stable domain keys | `TreeDataSlice<K, T>` and `TreeRowFilter<K, T>` |
| Shared row selection | `SelectionModel` or keyed selection models |
| Check state | `CheckedModel`, `TreeCheckedModel`, or `KeyedTreeCheckedModel` |
| Chart series | `ChartModel<T>` |

## Common operations

Use `push`, `insert`, `remove`, `set`, or `clear` to change a list. Read an item
through `with_item(index, callback)`; the callback scopes the internal borrow.
Use `reconcile_by_key` when replacing external data while preserving identity.
Do not mutate a model while holding a borrow into it.

Use `Repeater` for small, changing groups of widgets. Use `ListView`, `TableView`,
or `GridView` when rows or tiles need virtualization. For trees, keep expansion
state per view when two views must expand independently.

A view can share its selection model with another view. Match the selection key
type to the source: index-based identity and stable domain identity have
different behavior when items move or are filtered.

For a database, filesystem, or paged service, implement a data-source trait
rather than copying the entire dataset into a second in-memory model. See
[data sources](data-source.md) for loading, identity, and drag-and-drop contracts.

## Chart data

`ChartModel` stores series and points. `ChartWindow` and `ChartAggregate` provide
windowing and aggregation building blocks; applications compose them explicitly.
`ChartSelection` can be shared by chart widgets. See [charts](charts.md).

## Constraints

- Model handles use UI-thread ownership. Pass background results to the UI before
  mutating them; do not share the handles across threads.
- Keep subscriptions and projection handles alive while a view depends on them.
- Stable keys are required when selection must survive reordering or replacement.
- A reactive model is not persistent storage. Use [settings](settings.md) or the
  application's database for persistence.

## Reference

- [Data collections API](data-collections/index.md)
- [Data-source contract](data-source.md)
- [List model source](../crates/teksilo-data/src/list_model.rs)
- [Tree model source](../crates/teksilo-data/src/tree_model.rs)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/data-models.md)
are retained in the repository.
