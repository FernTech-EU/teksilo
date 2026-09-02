<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeDataSource

`TreeDataSource` — read-and-command interface for hierarchical data behind a
`TreeView` / `TreeTableView`.

`TreeDataSource` is to trees what `ListDataSource`
is to flat lists: a projected, per-view, flattened read API plus the
capability protocol for identity, DnD validation, and lazy loading.
The built-in `TreeSlice` and
`SortFilterTreeModel` implement it over an
in-memory `TreeModel`; an external source of truth
(e.g. a Qleany entity store) implements it directly with its own `Key` type
and so never needs to mirror itself into a `TreeModel`.

## When to use

Implement `TreeDataSource` directly when your data already lives outside an
in-memory tree (a database, a virtual filesystem, a remote store) and you
do not want to mirror it into a `TreeModel`. Use `TreeSlice`
when you have a `TreeModel<T>` and want per-view expand state.

## Example

```ignore
use teksilo_data::{TreeDataSource, FlatEntry, NodeId};
use teksilo_data::dnd_types::{DragEligibility, DropQuery, DropResponse, DropCommit, RowState};
use teksilo_core::signal::Signal;

struct MySource { version: Signal<u64> }

impl TreeDataSource for MySource {
    type Item = String;
    type Key = NodeId;

    fn visible_count(&self) -> usize { 0 }
    fn with_entry<R>(&self, _i: usize, _f: impl FnOnce(&String, &FlatEntry<NodeId>) -> R) -> Option<R> { None }
    fn key_at(&self, _i: usize) -> Option<NodeId> { None }
    fn flat_index_of(&self, _k: &NodeId) -> Option<usize> { None }
    fn parent(&self, _k: &NodeId) -> Option<NodeId> { None }
    fn child_keys(&self, _k: &NodeId) -> Vec<NodeId> { vec![] }
    fn version_signal(&self) -> Signal<u64> { self.version.clone() }
    fn is_expanded(&self, _k: &NodeId) -> bool { false }
    fn set_expanded(&self, _k: &NodeId, _expanded: bool) {}
}
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/tree_data_source/index.html)

## `pub struct FlatEntry`

A single entry in a tree's flattened, currently-visible row list.

Generic over the key type so external sources carry their own identity
(`K = NodeId` for `TreeModel`-backed sources, `K = i64` for an entity-id
store, …). The default `K = NodeId` keeps every in-tree `FlatEntry` mention
and `entry.node_id` read compiling unchanged.

```rust
pub struct FlatEntry<K: ItemKey = NodeId> { /* fields */ }
```

## `pub fn tree_is_desc_or_self(...)`

Whether `node` is `ancestor` or one of its descendants — the move cycle
guard (you cannot drop a node into its own subtree).

```rust
pub fn tree_is_desc_or_self<T: 'static>(
    tree: &TreeModel<T>,
    node: NodeId,
    ancestor: NodeId,
) -> bool;
```

## `pub fn tree_apply_reorder(...)`

Apply a tree reorder by `NodeId`, with the cycle guard and the
remove-then-insert index adjustment `TreeModel::move_node` requires. Shared
by the `TreeSlice` / `SortFilterTreeModel` `accept_drop` impls. Returns
whether the move was applied (false = rejected, e.g. cycle or self-drop).

```rust
pub fn tree_apply_reorder<T: 'static>(
    tree: &TreeModel<T>,
    source: NodeId,
    target: NodeId,
    position: DropPosition,
) -> bool;
```
