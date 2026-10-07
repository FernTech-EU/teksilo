<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SortFilterTreeModel

Composable sort + filter projection over a hierarchical tree.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`TreeFilterMode`](#treefiltermode) — Filter strategy used by `SortFilterTreeModel` |
| `struct` | [`SortFilterTreeModel`](#sortfiltertreemodel) — Hierarchical projection over a `TreeModel<T>` driven by sort + filter signals |

## Public functions

### `SortFilterTreeModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(tree: TreeModel<T>)`](#sortfiltertreemodel-new) |
| | **Builder methods** |
| `Self` | [`with_comparator(col_id: impl Into<String>, cmp: impl Fn(&T, &T) -> Ordering + 'static)`](#sortfiltertreemodel-with_comparator) |
| `Self` | [`with_predicate(col_id: impl Into<String>, factory: impl Fn(&str) -> Box<dyn Fn(&T) -> bool> + 'static)`](#sortfiltertreemodel-with_predicate) |
| `Self` | [`filter_mode(mode: TreeFilterMode)`](#sortfiltertreemodel-filter_mode) |
| | **Methods** |
|  | [`sort_signal(signal: Signal<Option<(String, SortDirection)>>)`](#sortfiltertreemodel-sort_signal) |
|  | [`filters_signal(signal: Signal<HashMap<String, String>>)`](#sortfiltertreemodel-filters_signal) |
|  | [`set_sort(col_id: Option<&str>, dir: SortDirection)`](#sortfiltertreemodel-set_sort) |
|  | [`clear_sort()`](#sortfiltertreemodel-clear_sort) |
|  | [`set_filter(col_id: &str, text: &str)`](#sortfiltertreemodel-set_filter) |
|  | [`clear_filters()`](#sortfiltertreemodel-clear_filters) |
| `usize` | [`visible_count()`](#sortfiltertreemodel-visible_count) |
| `Option<R>` | [`with_entry<R>(flat_index: usize, f: impl FnOnce(&T, &FlatEntry) -> R)`](#sortfiltertreemodel-with_entry) |
| `Option<NodeId>` | [`visible_node_id(flat_index: usize)`](#sortfiltertreemodel-visible_node_id) |
| `Option<FlatEntry>` | [`entry_at(flat_index: usize)`](#sortfiltertreemodel-entry_at) |
| `Option<usize>` | [`flat_index_of(node: NodeId)`](#sortfiltertreemodel-flat_index_of) |
| `bool` | [`is_expanded(node: NodeId)`](#sortfiltertreemodel-is_expanded) |
|  | [`expand(node: NodeId)`](#sortfiltertreemodel-expand) |
|  | [`collapse(node: NodeId)`](#sortfiltertreemodel-collapse) |
|  | [`toggle(node: NodeId)`](#sortfiltertreemodel-toggle) |
|  | [`expand_all()`](#sortfiltertreemodel-expand_all) |
|  | [`collapse_all()`](#sortfiltertreemodel-collapse_all) |
| `Signal<u64>` | [`version_signal()`](#sortfiltertreemodel-version_signal) |
| `Option<usize>` | [`first_changed_index()`](#sortfiltertreemodel-first_changed_index) |
| `TreeModel<T>` | [`tree()`](#sortfiltertreemodel-tree) |

## Detailed description

`SortFilterTreeModel<T>` wraps a `TreeModel<T>` and exposes a
`TreeSlice`-shaped API whose visible nodes are determined by:

1. **Filtering** with one of three `TreeFilterMode` strategies:
   - `HideNonMatching` — strict per-node match (hides ancestors of
     matches if they don't themselves match).
   - `KeepAncestors` — file-tree convention: an ancestor stays visible
     whenever any descendant matches. **Default.**
   - `KeepDescendants` — once a node matches, its entire subtree stays
     visible (useful for "show me this branch").
2. **Sorting** applied per-parent: comparators reorder *siblings* but
   never cross levels.

The proxy owns its own expand/collapse state (independent of any
`TreeSlice` over the same `TreeModel`) and bumps `version_signal` on
every projection rebuild — `TreeTableView` binds to that to know when to
rebuild its row tree.

A single-node `TreeChange::NodeUpdated` with **no filter active** skips
the full filter/sort/flatten recompute: the node's rank among its
siblings is checked against its immediate neighbours (tree sort never
crosses levels) and, if stable, only `first_changed_index()` and
`version_signal()` advance. Any active filter falls back to the full
rebuild (a node's own match verdict can cascade to ancestors and/or
descendants depending on `TreeFilterMode`, so cheaply proving no
cascade isn't possible without re-deriving visibility). See
`try_incremental_node_update` for the full reasoning.

#### Selection semantics

Selection on a sorted/filtered tree view is tracked by **flat (visible)
index**, mirroring `SortFilterListModel`. After a projection rebuild a
downstream `SelectionModel` keeps the same
numerical indices selected even though they may now point at different
nodes. Apps that want identity-based selection should observe
`version_signal()` and rewrite the selection from `NodeId`s after each
bump.

```rust
# use teksilo_data::{TreeModel, SortFilterTreeModel, SortDirection, TreeFilterMode};
let tree: TreeModel<&'static str> = TreeModel::new();
let src  = tree.insert_root(0, "src");
let docs = tree.insert_root(1, "docs");
tree.insert_child(src, 0, "main.rs");
tree.insert_child(docs, 0, "readme.md");

let proxy = SortFilterTreeModel::new(tree)
    .filter_mode(TreeFilterMode::KeepAncestors)
    .with_comparator("name", |a: &&str, b: &&str| a.cmp(b))
    .with_predicate("name", |text| {
        let needle = text.to_string();
        Box::new(move |row: &&str| row.contains(&needle))
    });

// Only roots visible initially (collapsed).
assert_eq!(proxy.visible_count(), 2);

proxy.set_filter("name", ".rs");
// KeepAncestors: src (parent of main.rs) stays visible even though it
// doesn't match itself.
assert!(proxy.visible_count() >= 1);
proxy.clear_filters();

proxy.expand(src);
assert_eq!(proxy.visible_count(), 3); // src + main.rs + docs
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/sort_filter_tree_model/index.html)

<a id="treefiltermode"></a>

## `pub enum TreeFilterMode`

Filter strategy used by `SortFilterTreeModel`.

```rust
pub enum TreeFilterMode { /* variants */ }
```

### Variants

- **`HideNonMatching`** — Hide rows that don't match. Children of hidden parents stay hidden too.
- **`KeepAncestors`** — Keep ancestors of matching descendants visible (file-tree convention).
- **`KeepDescendants`** — Keep matching rows AND their entire subtree.

<a id="sortfiltertreemodel"></a>

## `pub struct SortFilterTreeModel`

Hierarchical projection over a `TreeModel<T>` driven by sort + filter
signals. Exposes a `TreeSlice`-shaped read API consumed by `TreeTableView`.

```rust
pub struct SortFilterTreeModel<T: 'static> { /* fields */ }
```

### Methods

<a id="sortfiltertreemodel-new"></a>

#### `pub fn new(tree: TreeModel<T>) -> Self`

Wrap a `TreeModel<T>`. The projection starts as the identity
(everything visible, no sort, all roots collapsed).

<a id="sortfiltertreemodel-with_comparator"></a>

#### `pub fn with_comparator( self, col_id: impl Into<String>, cmp: impl Fn(&T, &T) -> Ordering + 'static, ) -> Self`

Register a comparator for a column id. Chainable.

<a id="sortfiltertreemodel-with_predicate"></a>

#### `pub fn with_predicate( self, col_id: impl Into<String>, factory: impl Fn(&str) -> Box<dyn Fn(&T) -> bool> + 'static, ) -> Self`

Register a predicate factory for a column id. Chainable.

<a id="sortfiltertreemodel-filter_mode"></a>

#### `pub fn filter_mode(self, mode: TreeFilterMode) -> Self`

Set the filter mode (default `KeepAncestors`). Chainable.

<a id="sortfiltertreemodel-sort_signal"></a>

#### `pub fn sort_signal(&self, signal: Signal<Option<(String, SortDirection)>>)`

Bind a sort signal — typically `TreeTableView::sort_signal()`.

<a id="sortfiltertreemodel-filters_signal"></a>

#### `pub fn filters_signal(&self, signal: Signal<HashMap<String, String>>)`

Bind a filters signal — typically `TreeTableView::filters_signal()`.

<a id="sortfiltertreemodel-set_sort"></a>

#### `pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection)`

Set the active sort imperatively. Routes through the bound signal
when present.

<a id="sortfiltertreemodel-clear_sort"></a>

#### `pub fn clear_sort(&self)`

Clear the active sort.

<a id="sortfiltertreemodel-set_filter"></a>

#### `pub fn set_filter(&self, col_id: &str, text: &str)`

Set or clear a single column's filter.

<a id="sortfiltertreemodel-clear_filters"></a>

#### `pub fn clear_filters(&self)`

Clear every column's filter.

<a id="sortfiltertreemodel-visible_count"></a>

#### `pub fn visible_count(&self) -> usize`

Number of currently visible (non-filtered, non-collapsed) nodes in the flat list.

<a id="sortfiltertreemodel-with_entry"></a>

#### `pub fn with_entry<R>( &self, flat_index: usize, f: impl FnOnce(&T, &FlatEntry) -> R, ) -> Option<R>`

Call `f` with the item and `FlatEntry` metadata at `flat_index`, returning `f`'s result.

<a id="sortfiltertreemodel-visible_node_id"></a>

#### `pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId>`

Return the `NodeId` of the node at `flat_index`, or `None` if the index is out of range.

<a id="sortfiltertreemodel-entry_at"></a>

#### `pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry>`

Return a clone of the `FlatEntry` at `flat_index`, or `None` if out of range.

<a id="sortfiltertreemodel-flat_index_of"></a>

#### `pub fn flat_index_of(&self, node: NodeId) -> Option<usize>`

Return the flat index of `node` in the current visible list, or `None`
if it is not visible. O(1) — backed by a position map rebuilt on
every projection rebuild.

<a id="sortfiltertreemodel-is_expanded"></a>

#### `pub fn is_expanded(&self, node: NodeId) -> bool`

Whether `node` is currently expanded in this projection.

<a id="sortfiltertreemodel-expand"></a>

#### `pub fn expand(&self, node: NodeId)`

Expand `node`, revealing its children in the flat list. Rebuilds and bumps the version signal.

<a id="sortfiltertreemodel-collapse"></a>

#### `pub fn collapse(&self, node: NodeId)`

Collapse `node`, hiding its children. Rebuilds and bumps the version signal.

<a id="sortfiltertreemodel-toggle"></a>

#### `pub fn toggle(&self, node: NodeId)`

Toggle the expanded state of `node`. Always rebuilds and bumps the version signal.

<a id="sortfiltertreemodel-expand_all"></a>

#### `pub fn expand_all(&self)`

Expand every node that has children, making the full tree visible.

<a id="sortfiltertreemodel-collapse_all"></a>

#### `pub fn collapse_all(&self)`

Collapse every node, leaving only roots visible.

<a id="sortfiltertreemodel-version_signal"></a>

#### `pub fn version_signal(&self) -> Signal<u64>`

Bumps on every projection rebuild — bind in `TreeTableView::build`
at `BindingLevel::Rebuild`.

<a id="sortfiltertreemodel-first_changed_index"></a>

#### `pub fn first_changed_index(&self) -> Option<usize>`

First flat index whose content may differ from before the latest
projection rebuild — rows `0..index` are the same nodes, at the
same depths, with the same expand state as before, so per-row
derived state (e.g. a measured row height) remains valid for them.
Equal to `visible_count()` when the visible list is unchanged.

`None` means unknown (no rebuild observed yet) — treat as a full
change. The value describes the **latest** rebuild only; read it
synchronously from a `version_signal()` observer (observers fire
inline on every bump, so per-change reads cannot miss a value).

<a id="sortfiltertreemodel-tree"></a>

#### `pub fn tree(&self) -> TreeModel<T>`

Return the underlying `TreeModel` handle for direct mutation outside the projection.
