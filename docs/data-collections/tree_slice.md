<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeSlice

`TreeSlice` — per-view flattened projection of a `TreeModel`.

`TreeSlice<T>` wraps a `TreeModel<T>` and maintains an independent
expand/collapse set so two `TreeView` widgets sharing the same model have
independent visible rows — dual-pane file managers, overview/detail splits,
and search results panels are each one `TreeSlice::new(model.clone())`. The
slice re-flattens automatically whenever the underlying model emits a
`TreeChange`, and bumps a `version_signal`
`Signal<u64>` that views bind at `BindingLevel::Rebuild`.

A lightweight `TreeSliceHandle` (created via `TreeSlice::handle`) shares
all `Rc`-based internals and is usable in closures without keeping the
tree-change observer alive.

`TreeSlice` implements `TreeDataSource` and is the
built-in source for `TreeView` / `TreeTableView`.

## Example

```rust
# use teksilo_data::{TreeModel, TreeSlice};
let tree = TreeModel::new();
let root = tree.insert_root(0, "root");
let child = tree.insert_child(root, 0, "child");

let slice1 = TreeSlice::new(tree.clone());
let slice2 = TreeSlice::new(tree.clone());

slice1.expand(root);
assert_eq!(slice1.visible_count(), 2); // root + child visible
assert_eq!(slice2.visible_count(), 1); // still collapsed in slice2

// Inserting into the model notifies both slices.
tree.insert_child(root, 1, "child2");
assert_eq!(slice1.visible_count(), 3); // child2 also visible in the expanded slice
```

## Builder methods at a glance

`visible_count`, `with_entry`, `visible_node_id`, `entry_at`, `depth_at`, `flat_index_of`, `is_expanded`, `expand`, `collapse`, `toggle`, `expand_all`, `collapse_all`, `expanded_nodes`, `set_expanded_nodes`, `version_signal`, `first_changed_index`, `tree`, `handle`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/tree_slice/index.html)

## `pub struct TreeSlice`

Per-view flattened projection of a `TreeModel<T>`.

Owns an independent expand/collapse set and re-flattens automatically on
every `TreeChange` from the underlying model. Two slices
over the same model have completely independent expand state. See the
`module documentation` for the full picture.

```rust
pub struct TreeSlice<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new(tree: TreeModel<T>) -> Self`

Create a new `TreeSlice` for the given `TreeModel`.
All nodes start collapsed (only roots are visible).

#### `pub fn visible_count(&self) -> usize`

Number of currently visible (flattened) rows.

#### `pub fn with_entry<R>( &self, flat_index: usize, f: impl FnOnce(&T, &FlatEntry) -> R, ) -> Option<R>`

Access a flat entry by index via callback.
The callback receives `(&T, &FlatEntry)`.

#### `pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId>`

Get the `NodeId` at the given flat index.

#### `pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry>`

Get the `FlatEntry` at the given flat index (cloned).

#### `pub fn depth_at(&self, flat_index: usize) -> usize`

Get the depth at the given flat index.

#### `pub fn flat_index_of(&self, node: NodeId) -> Option<usize>`

Find the flat index for a given `NodeId`, or `None` if not visible.
O(1) — backed by a position map rebuilt on every reflatten.

#### `pub fn is_expanded(&self, node: NodeId) -> bool`

Whether the given node is expanded.

#### `pub fn expand(&self, node: NodeId)`

Expand a node (make its children visible).

#### `pub fn collapse(&self, node: NodeId)`

Collapse a node (hide its children).

#### `pub fn toggle(&self, node: NodeId)`

Toggle expand/collapse state of a node.

#### `pub fn expand_all(&self)`

Expand all nodes in the tree.

#### `pub fn collapse_all(&self)`

Collapse all nodes in the tree.

#### `pub fn expanded_nodes(&self) -> Vec<NodeId>`

Get all expanded node IDs (for persistence).

#### `pub fn set_expanded_nodes(&self, nodes: &[NodeId])`

Restore expanded state (for persistence).

#### `pub fn version_signal(&self) -> Signal<u64>`

Get the version signal for binding to `BindingLevel::Rebuild`.

#### `pub fn first_changed_index(&self) -> Option<usize>`

First flat index whose content may differ from before the latest
reflatten — the rows `0..index` are the same nodes, at the same
depths, with the same expand state as before, so any per-row
derived state (e.g. a measured row height) remains valid for them.
Equal to `visible_count()` when the visible list is unchanged.

`None` means unknown (no reflatten observed yet) — treat as a full
change. The value describes the **latest** reflatten only; read it
synchronously from a `version_signal()` observer (observers fire
inline on every bump, so per-change reads cannot miss a value).

#### `pub fn tree(&self) -> &TreeModel<T>`

Access the underlying `TreeModel`.

#### `pub fn handle(&self) -> TreeSliceHandle<T>`

Create a lightweight handle for use in closures.
Shares all Rc-based internals but does not keep the observer alive.

## `pub struct TreeSliceHandle`

Lightweight handle to a `TreeSlice`'s shared state, usable in closures.

Created via `TreeSlice::handle`. Shares all `Rc`-based internals with its
parent `TreeSlice` but does **not** keep the tree-change observer alive —
the `TreeSlice` that owns the observer must outlive all handles that rely on
automatic re-flattening on model changes.

```rust
pub struct TreeSliceHandle<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn visible_count(&self) -> usize`

Number of currently-visible (flattened) rows.

#### `pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry>`

Get the `FlatEntry` at `flat_index` (cloned), or `None` if out of bounds.

#### `pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId>`

Get the `NodeId` at `flat_index`, or `None` if out of bounds.

#### `pub fn expand(&self, node: NodeId)`

Expand `node` (make its children visible) and bump the version signal.
No-op if already expanded.

#### `pub fn collapse(&self, node: NodeId)`

Collapse `node` (hide its children) and bump the version signal.
No-op if already collapsed.

#### `pub fn is_expanded(&self, node: NodeId) -> bool`

Returns `true` if `node` is currently expanded.

#### `pub fn toggle_expand(&self, node: NodeId)`

Toggle `node`'s expand/collapse state and bump the version signal.

#### `pub fn tree(&self) -> &TreeModel<T>`

Access the underlying `TreeModel`.

#### `pub fn expand_all(&self)`

Expand every node with children — see `TreeSlice::expand_all`. Useful
after a model rebuild reassigns `NodeId`s (the old expand set no longer
matches), to keep the view fully expanded.

#### `pub fn first_changed_index(&self) -> Option<usize>`

See `TreeSlice::first_changed_index`.
