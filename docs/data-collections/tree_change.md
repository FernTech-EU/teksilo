<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeChange

TreeChange — change notifications and stable node identifiers for tree collections.

`NodeId` is an opaque, stable handle for a node in a `crate::TreeModel`.
Because `TreeModel` is backed by a slotmap, `NodeId` values survive arbitrary
insertions, removals, and moves — only deleting the node itself invalidates it.
`TreeChange` describes exactly what mutated in the tree so that projections
(`SortFilterTreeModel`, `TreeSlice`) can refresh efficiently and emit
fine-grained divergence hints.

Consumers typically receive `TreeChange` values through an observer registered
via `crate::TreeModel::observe_changes`, which fires synchronously (before
the registering call returns) after each mutation. The projections listed above
subscribe internally; app code rarely needs to subscribe directly.

```ignore
// TreeModel::observe_changes returns an ObserverHandle whose drop
// unregisters the callback — keep it alive for the observer's lifetime.
use teksilo_data::{TreeModel, TreeChange};
let tree: TreeModel<String> = TreeModel::new();
let _handle = tree.observe_changes(|change| {
    println!("{change:?}");
});
tree.insert_root(0, "root".to_string());
// prints: NodeInserted { parent: None, index: 0, node: NodeId(...) }
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/tree_change/index.html)

## `pub struct NodeId`

Opaque identifier for a node in a `TreeModel`.

`NodeId` values are stable across mutations — inserting or removing other
nodes does not invalidate existing `NodeId` handles (they are SlotMap keys).

```rust
pub struct NodeId(slotmap::DefaultKey);
```

## `pub enum TreeChange`

Describes a mutation to a tree structure. Emitted by `TreeModel<T>` automatically.

```rust
pub enum TreeChange { /* variants */ }
```

### Variants

- **`NodeInserted`** — A node was inserted as a child of `parent` at the given index. `parent` is `None` for root-level insertions.
- **`NodeRemoved`** — A node (and its entire subtree) was removed. `parent` is `None` if it was a root-level node.
- **`NodeMoved`** — A node was moved to a new parent at the given index.
- **`NodeUpdated`** — A node's data was updated in place.
- **`Reset`** — The entire tree was replaced. Consumers should discard all state and rebuild.
