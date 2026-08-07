<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeModel

`TreeModel` — concrete reactive tree with shared, cloneable handles.

`TreeModel<T>` owns a hierarchy of `T` items in a flat SlotMap arena with
parent-child links. Every structural mutation (`insert_root`, `insert_child`,
`remove`, `move_node`, `update`) emits a `TreeChange` to all registered
observers before returning. Node identity is a stable, versioned `NodeId`
(a SlotMap key) that is never reused after removal.

Cloning produces a second handle to the **same** data — all handles see the
same hierarchy and receive the same change notifications. Register observers
via `observe_changes`; the returned
`ObserverHandle` is RAII — dropping it
unregisters the callback.

For per-view expand/collapse state wrap the model in a
`TreeSlice`. For sort/filter projections use
`SortFilterTreeModel`.

## Example

```rust
# use teksilo_data::{TreeModel, TreeChange};
let tree = TreeModel::new();
let root = tree.insert_root(0, "root");
let child = tree.insert_child(root, 0, "child");

assert_eq!(tree.root_count(), 1);
assert_eq!(tree.child_count(root), 1);
assert_eq!(tree.parent(child), Some(root));

let clone = tree.clone();
clone.insert_root(1, "root2");
assert_eq!(tree.root_count(), 2); // both handles share the same data
```

## Builder methods at a glance

`root_count`, `root`, `child_count`, `child`, `parent`, `depth`, `has_children`, `children`, `with_item`, `find_by`, `insert_root`, `insert_child`, `remove`, `move_node`, `move_to_root`, `update`, `observe_changes`, `debug_named`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_data/tree_model/index.html)

## `pub struct TreeModel`

A concrete reactive tree that stores a hierarchy of `T` items in a flat arena.

`TreeModel<T>` is `Clone` — cloning produces a second handle to the same
underlying data. All handles see the same hierarchy and receive the same
`TreeChange` notifications from `observe_changes`.
Nodes are identified by opaque `NodeId` handles that are stable and
non-reusable across mutations (versioned SlotMap keys).

```rust
pub struct TreeModel<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty tree model with no roots and no observers.

#### `pub fn root_count(&self) -> usize`

Number of root-level nodes.

#### `pub fn root(&self, index: usize) -> NodeId`

Get the `NodeId` of a root-level node by index.

# Panics
Panics if `index >= root_count()`.

#### `pub fn child_count(&self, parent: NodeId) -> usize`

Number of children of the given node.

#### `pub fn child(&self, parent: NodeId, index: usize) -> NodeId`

Get the `NodeId` of a child by parent and index.

# Panics
Panics if the parent or index is invalid.

#### `pub fn parent(&self, node: NodeId) -> Option<NodeId>`

Get the parent of a node, or `None` if it is a root.

#### `pub fn depth(&self, node: NodeId) -> usize`

Compute the depth of a node (0 for roots).

#### `pub fn has_children(&self, node: NodeId) -> bool`

Whether the given node has any children.

#### `pub fn children(&self, node: NodeId) -> Vec<NodeId>`

Get the children of a node as a vector of `NodeId`.

#### `pub fn with_item<R>(&self, node: NodeId, f: impl FnOnce(&T) -> R) -> Option<R>`

Access a node's data via a callback. Returns `None` if the node doesn't exist.

#### `pub fn find_by(&self, predicate: impl Fn(&T) -> bool) -> Option<NodeId>`

Find the first node matching a predicate (depth-first from roots).

#### `pub fn insert_root(&self, index: usize, item: T) -> NodeId`

Insert a new root-level node at the given index.

# Panics
Panics if `index > root_count()`.

#### `pub fn insert_child(&self, parent: NodeId, index: usize, item: T) -> NodeId`

Insert a new child node under the given parent at the given index.

# Panics
Panics if the parent is invalid or `index > child_count(parent)`.

#### `pub fn remove(&self, node: NodeId)`

Remove a node and its entire subtree.

# Panics
Panics if the node is invalid.

#### `pub fn move_node(&self, node: NodeId, new_parent: NodeId, new_index: usize)`

Move a node (and its subtree) to a new parent at the given index.

# Panics
Panics if any of the nodes are invalid, or if the target is a
descendant of the source (would create a cycle).

#### `pub fn move_to_root(&self, node: NodeId, new_index: usize)`

Move a node to the root level at the given index.

#### `pub fn update(&self, node: NodeId, item: T)`

Update a node's data in place.

# Panics
Panics if the node is invalid.

#### `pub fn observe_changes(&self, f: impl Fn(&TreeChange) + 'static) -> ObserverHandle`

Register an observer for tree change notifications.
Returns an `ObserverHandle` — dropping it removes the callback.

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this tree with the debug inspector under `name`. In
release builds (`!cfg(debug_assertions)`) this is a no-op
pass-through so call sites stay free of `#[cfg]` lines.

Idempotent on repeated calls — the latest registration wins.
The registration drops automatically when the last `TreeModel`
handle is freed (the adapter the registry holds is `Weak`).
