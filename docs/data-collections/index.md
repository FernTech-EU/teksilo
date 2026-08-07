<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Data Collections

Every public type in `teksilo-data`, grouped by category. Each page links to its full rustdoc API reference.

## Models

- [ChartAggregate](chart_aggregate.md) — `ChartAggregate<T>` — a bucket/rollup projection over a `crate::ChartModel`
- [ChartChange](chart_change.md) — ChartChange — change notifications and stable series identifiers for chart collections
- [ChartModel](chart_model.md) — `ChartModel<T>` — concrete reactive multi-series chart data model
- [ChartSelection](chart_selection.md) — `ChartSelection` — point-level selection state for chart widgets
- [ChartWindow](chart_window.md) — `ChartWindow<T>` — a "last N points per series" streaming projection over
- [CheckedModel](checked_model.md) — `CheckedModel` — per-row checkbox state for flat collection widgets
- [CheckState](check_state.md) — `CheckState` — tri-state checkbox value shared by the data layer and widgets
- [DataChange](data_change.md) — `DataChange` — change notifications for flat collections
- [ItemKey](dnd_types.md) — Shared capability types for the data-source drag-and-drop + lazy protocol
- [KeyedSelectionModel](keyed_selection_model.md) — `KeyedSelectionModel<K>` — identity-based selection for collection widgets
- [KeyedTreeCheckedModel](keyed_tree_checked_model.md) — `KeyedTreeCheckedModel<K>` — per-node checkbox state for a tree **keyed by a
- [ListDataSource](list_data_source.md) — `ListDataSource` — read-and-command interface for a flat collection behind a `ListView` /
- [ListModel](list_model.md) — `ListModel<T>` — concrete reactive list backed by a `Vec<T>`
- [SelectionModel](selection_model.md) — SelectionModel — index-based selection state for collection widgets
- [SortFilterListModel](sort_filter_list_model.md) — Composable sort + filter projection over a flat list source
- [SortFilterTreeModel](sort_filter_tree_model.md) — Composable sort + filter projection over a hierarchical tree
- [TreeChange](tree_change.md) — TreeChange — change notifications and stable node identifiers for tree collections
- [TreeCheckedModel](tree_checked_model.md) — `TreeCheckedModel` — per-node checkbox state for a tree, with optional
- [TreeDataSlice](tree_data_slice.md) — `TreeDataSlice` — the reusable `TreeDataSource` engine for an **external,
- [TreeDataSource](tree_data_source.md) — `TreeDataSource` — read-and-command interface for hierarchical data behind a
- [TreeModel](tree_model.md) — `TreeModel` — concrete reactive tree with shared, cloneable handles
- [TreeRowFilter](tree_row_filter.md) — `TreeRowFilter` — sort + tree-aware filter over a `TreeRow` stream
- [TreeSlice](tree_slice.md) — `TreeSlice` — per-view flattened projection of a `TreeModel`
