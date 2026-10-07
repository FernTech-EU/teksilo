<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeDataSource

`TreeDataSource` — read-and-command interface for hierarchical data behind a
`TreeView` / `TreeTableView`.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`FlatEntry`](#flatentry) — A single entry in a tree's flattened, currently-visible row list |
| `trait` | [`TreeDataSource`](#treedatasource) — A per-view flattened, projectable view over hierarchical data |
| `fn` | [`tree_is_desc_or_self`](#tree_is_desc_or_self) — Whether `node` is `ancestor` or one of its descendants — the move cycle guard (you cannot drop a node into its own subtree) |
| `fn` | [`tree_apply_reorder`](#tree_apply_reorder) — Apply a tree reorder by `NodeId`, with the cycle guard and the remove-then-insert index adjustment `TreeModel::move_node` requires |

## Public functions

### `TreeDataSource`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `usize` | [`visible_count()`](#treedatasource-visible_count) |
| `Option<R>` | [`with_entry<R>(flat_index: usize, f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R)`](#treedatasource-with_entry) |
| `Option<Self::Key>` | [`key_at(flat_index: usize)`](#treedatasource-key_at) |
| `Option<usize>` | [`flat_index_of(key: &Self::Key)`](#treedatasource-flat_index_of) |
| `Option<Self::Key>` | [`parent(key: &Self::Key)`](#treedatasource-parent) |
| `Vec<Self::Key>` | [`child_keys(key: &Self::Key)`](#treedatasource-child_keys) |
| `Signal<u64>` | [`version_signal()`](#treedatasource-version_signal) |
| `bool` | [`is_expanded(key: &Self::Key)`](#treedatasource-is_expanded) |
|  | [`set_expanded(key: &Self::Key, expanded: bool)`](#treedatasource-set_expanded) |
| `Option<usize> { /* default implementation */ }` | [`first_changed_index()`](#treedatasource-first_changed_index) |
| `bool { /* default implementation */ }` | [`contains_key(key: &Self::Key)`](#treedatasource-contains_key) |
| `DragEligibility { /* default implementation */ }` | [`drag(_key: &Self::Key)`](#treedatasource-drag) |
| `DropResponse { /* default implementation */ }` | [`can_accept(_query: &DropQuery<'_, Self::Key>)`](#treedatasource-can_accept) |
| `bool { /* default implementation */ }` | [`accept_drop(_commit: DropCommit<'_, Self::Key>)`](#treedatasource-accept_drop) |
| `bool { /* default implementation */ }` | [`reorder_within(sources: &[Self::Key], target: &Self::Key, position: DropPosition)`](#treedatasource-reorder_within) |
|  | [`on_drag_out(_key: &Self::Key)`](#treedatasource-on_drag_out) |
| `RowState { /* default implementation */ }` | [`row_state(_flat_index: usize)`](#treedatasource-row_state) |
|  | [`request_window(_range: std::ops::Range<usize>)`](#treedatasource-request_window) |
| `bool { /* default implementation */ }` | [`can_fetch_more()`](#treedatasource-can_fetch_more) |
|  | [`fetch_more()`](#treedatasource-fetch_more) |
| | **Constants and types** |
| `type` | [`Item`](#treedatasource-item) |
| `type` | [`Key`](#treedatasource-key) |

## Detailed description

`TreeDataSource` is to trees what `ListDataSource`
is to flat lists: a projected, per-view, flattened read API plus the
capability protocol for identity, DnD validation, and lazy loading.
The built-in `TreeSlice` and
`SortFilterTreeModel` implement it over an
in-memory `TreeModel`; an external source of truth
(e.g. an entity store) implements it directly with its own `Key` type
and so never needs to mirror itself into a `TreeModel`.

#### When to use

Implement `TreeDataSource` directly when your data already lives outside an
in-memory tree (a database, a virtual filesystem, a remote store) and you
do not want to mirror it into a `TreeModel`. Use `TreeSlice`
when you have a `TreeModel<T>` and want per-view expand state.

#### Example

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

<a id="flatentry"></a>

## `pub struct FlatEntry`

A single entry in a tree's flattened, currently-visible row list.

Generic over the key type so external sources carry their own identity
(`K = NodeId` for `TreeModel`-backed sources, `K = i64` for an entity-id
store, …). The default `K = NodeId` keeps every in-tree `FlatEntry` mention
and `entry.node_id` read compiling unchanged.

```rust
pub struct FlatEntry<K: ItemKey = NodeId> { /* fields */ }
```

<a id="treedatasource"></a>

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

<a id="treedatasource-item"></a>

#### `type Item: 'static;`

The item type stored at each node.

<a id="treedatasource-key"></a>

#### `type Key: ItemKey;`

The stable per-node identity (`NodeId` for in-memory trees, an entity id
for an external store).

<a id="treedatasource-visible_count"></a>

#### `fn visible_count(&self) -> usize;`

Number of currently-visible (flattened) rows.

<a id="treedatasource-with_entry"></a>

#### `fn with_entry<R>( &self, flat_index: usize, f: impl FnOnce(&Self::Item, &FlatEntry<Self::Key>) -> R, ) -> Option<R>;`

Access the item + flat metadata at a visible index via callback.

<a id="treedatasource-key_at"></a>

#### `fn key_at(&self, flat_index: usize) -> Option<Self::Key>;`

The key of the row at a visible index.

<a id="treedatasource-flat_index_of"></a>

#### `fn flat_index_of(&self, key: &Self::Key) -> Option<usize>;`

The visible index of a key, if currently visible.

<a id="treedatasource-parent"></a>

#### `fn parent(&self, key: &Self::Key) -> Option<Self::Key>;`

The parent of a node (`None` for a root) — drives sibling nav + the
drop cycle-guard.

<a id="treedatasource-child_keys"></a>

#### `fn child_keys(&self, key: &Self::Key) -> Vec<Self::Key>;`

The children of a node, in order.

<a id="treedatasource-version_signal"></a>

#### `fn version_signal(&self) -> Signal<u64>;`

A version signal that bumps on every structural/projection change — the
view binds it at `BindingLevel::Rebuild`.

<a id="treedatasource-is_expanded"></a>

#### `fn is_expanded(&self, key: &Self::Key) -> bool;`

Whether the node is expanded.

<a id="treedatasource-set_expanded"></a>

#### `fn set_expanded(&self, key: &Self::Key, expanded: bool);`

Expand (`true`) or collapse (`false`) the node.

<a id="treedatasource-first_changed_index"></a>

#### `fn first_changed_index(&self) -> Option<usize> { /* default implementation */ }`

First visible index whose content may differ after the latest change —
rows `0..index` are unchanged, so per-row derived state (e.g. a measured
height) remains valid. `None` means unknown (treat as a full change).

<a id="treedatasource-contains_key"></a>

#### `fn contains_key(&self, key: &Self::Key) -> bool { /* default implementation */ }`

Whether `key` still exists in the source, **independent of visibility** —
a node hidden under a collapsed ancestor (or scrolled out of a lazy
window) still exists. Drives keyed-selection pruning, so that a
collapsed-but-present node keeps its selection and only a *deleted* node
is dropped. Default: visible-only (`flat_index_of(key).is_some()`);
sources whose nodes persist while collapsed/scrolled out should override
this to consult their full store.

<a id="treedatasource-drag"></a>

#### `fn drag(&self, _key: &Self::Key) -> DragEligibility { /* default implementation */ }`

Whether the node may begin a drag (the transferable gate).

<a id="treedatasource-can_accept"></a>

#### `fn can_accept(&self, _query: &DropQuery<'_, Self::Key>) -> DropResponse { /* default implementation */ }`

Whether a hovered drop is permitted (and where) — the pre-commit verdict.

<a id="treedatasource-accept_drop"></a>

#### `fn accept_drop(&self, _commit: DropCommit<'_, Self::Key>) -> bool { /* default implementation */ }`

Apply a committed drop. Returns whether it was applied.

<a id="treedatasource-reorder_within"></a>

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

<a id="treedatasource-on_drag_out"></a>

#### `fn on_drag_out(&self, _key: &Self::Key) { /* default implementation */ }`

Called on the *origin* source after one of its rows was accepted by a
different view (source-side completion). Sources backed by a shared /
command model no-op this; independent models use it to drop the moved
row.

<a id="treedatasource-row_state"></a>

#### `fn row_state(&self, _flat_index: usize) -> RowState { /* default implementation */ }`

Whether the row at a visible index is loaded.

<a id="treedatasource-request_window"></a>

#### `fn request_window(&self, _range: std::ops::Range<usize>) { /* default implementation */ }`

Nudge the source to load the given visible range (the view calls this
each build with its visible + buffer window).

<a id="treedatasource-can_fetch_more"></a>

#### `fn can_fetch_more(&self) -> bool { /* default implementation */ }`

Whether more rows can be appended (infinite scroll).

<a id="treedatasource-fetch_more"></a>

#### `fn fetch_more(&self) { /* default implementation */ }`

Fetch the next page (append-only growth).

<a id="tree_is_desc_or_self"></a>

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

<a id="tree_apply_reorder"></a>

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
