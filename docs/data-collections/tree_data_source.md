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
(e.g. an entity store) implements it directly with its own `Key` type
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

## Builder methods at a glance

`Item`, `Key`, `visible_count`, `with_entry`, `key_at`, `flat_index_of`, `parent`, `child_keys`, `version_signal`, `is_expanded`, `set_expanded`, `first_changed_index`, `contains_key`, `drag`, `can_accept`, `accept_drop`, `reorder_within`, `on_drag_out`, `row_state`, `request_window`, `can_fetch_more`, `fetch_more`

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

## `pub trait TreeDataSource`

A per-view flattened, projectable view over hierarchical data.

Not object-safe (associated types + `impl FnOnce`); views consume it
generically and erase it into a closure bundle, exactly as `ListView` does
with `ListDataSource`. The DnD (`drag`/`can_accept`/`accept_drop`/
`on_drag_out`) and lazy (`row_state`/`request_window`/`can_fetch_more`/
`fetch_more`) methods default to inert/fully-resident, so a read-only source
implements only the core read + nav surface.

```rust
pub trait TreeDataSource: 'static { /* associated items below */ }
```

### Associated items

#### `type Item: 'static;`

The item type stored at each node.

#### `type Key: ItemKey;`

The stable per-node identity (`NodeId` for in-memory trees, an entity id
for an external store).

#### `fn visible_count(&self) -> usize;`

Number of currently-visible (flattened) rows.

#### `fn with_entry<R>( &self, flat_index: usize, f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R, ) -> Option<R>;`

Access the item + flat metadata at a visible index via callback.

#### `fn key_at(&self, flat_index: usize) -> Option<Self::Key>;`

The key of the row at a visible index.

#### `fn flat_index_of(&self, key: &Self::Key) -> Option<usize>;`

The visible index of a key, if currently visible.

#### `fn parent(&self, key: &Self::Key) -> Option<Self::Key>;`

The parent of a node (`None` for a root) — drives sibling nav + the
drop cycle-guard.

#### `fn child_keys(&self, key: &Self::Key) -> Vec<Self::Key>;`

The children of a node, in order.

#### `fn version_signal(&self) -> Signal<u64>;`

A version signal that bumps on every structural/projection change — the
view binds it at `BindingLevel::Rebuild`.

#### `fn is_expanded(&self, key: &Self::Key) -> bool;`

Whether the node is expanded.

#### `fn set_expanded(&self, key: &Self::Key, expanded: bool);`

Expand (`true`) or collapse (`false`) the node.

#### `fn first_changed_index(&self) -> Option<usize> { /* default implementation */ }`

First visible index whose content may differ after the latest change —
rows `0..index` are unchanged, so per-row derived state (e.g. a measured
height) remains valid. `None` means unknown (treat as a full change).

#### `fn contains_key(&self, key: &Self::Key) -> bool { /* default implementation */ }`

Whether `key` still exists in the source, **independent of visibility** —
a node hidden under a collapsed ancestor (or scrolled out of a lazy
window) still exists. Drives keyed-selection pruning, so that a
collapsed-but-present node keeps its selection and only a *deleted* node
is dropped. Default: visible-only (`flat_index_of(key).is_some()`);
sources whose nodes persist while collapsed/scrolled out should override
this to consult their full store.

#### `fn drag(&self, _key: &Self::Key) -> DragEligibility { /* default implementation */ }`

Whether the node may begin a drag (the transferable gate).

#### `fn can_accept(&self, _query: &DropQuery<'_, Self::Key>) -> DropResponse { /* default implementation */ }`

Whether a hovered drop is permitted (and where) — the pre-commit verdict.

#### `fn accept_drop(&self, _commit: DropCommit<'_, Self::Key>) -> bool { /* default implementation */ }`

Apply a committed drop. Returns whether it was applied.

#### `fn reorder_within( &self, sources: &[Self::Key], target: &Self::Key, position: DropPosition, ) -> bool { /* default implementation */ }`

Reorder a whole set of this source's OWN nodes so they land contiguously
at a drop gap — the multi-row same-view reorder commit. `sources` are the
dragged nodes' keys in visible order; `target` / `position` name the drop
gap. Returns whether anything moved.

The default first drops any `sources` node that is a **descendant of
another** `sources` node (moving an ancestor already carries its
subtree), then moves the remaining top-level nodes one at a time,
re-anchoring each after the previous. Tree keys are stable, so the
re-anchoring is correct without index bookkeeping.

#### `fn on_drag_out(&self, _key: &Self::Key) { /* default implementation */ }`

Called on the *origin* source after one of its rows was accepted by a
different view (source-side completion). Sources backed by a shared /
command model no-op this; independent models use it to drop the moved
row.

#### `fn row_state(&self, _flat_index: usize) -> RowState { /* default implementation */ }`

Whether the row at a visible index is loaded.

#### `fn request_window(&self, _range: std::ops::Range<usize>) { /* default implementation */ }`

Nudge the source to load the given visible range (the view calls this
each build with its visible + buffer window).

#### `fn can_fetch_more(&self) -> bool { /* default implementation */ }`

Whether more rows can be appended (infinite scroll).

#### `fn fetch_more(&self) { /* default implementation */ }`

Fetch the next page (append-only growth).

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
