<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeSlice

`TreeSlice` — per-view flattened projection of a `TreeModel`.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`TreeSlice`](#treeslice) — Per-view flattened projection of a `TreeModel<T>` |
| `struct` | [`TreeSliceHandle`](#treeslicehandle) — Lightweight handle to a `TreeSlice`'s shared state, usable in closures |

## Public functions

### `TreeSlice`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(tree: TreeModel<T>)`](#treeslice-new) |
| | **Methods** |
| `usize` | [`visible_count()`](#treeslice-visible_count) |
| `Option<R>` | [`with_entry<R>(flat_index: usize, f: impl FnOnce(&T, &FlatEntry) -> R)`](#treeslice-with_entry) |
| `Option<NodeId>` | [`visible_node_id(flat_index: usize)`](#treeslice-visible_node_id) |
| `Option<FlatEntry>` | [`entry_at(flat_index: usize)`](#treeslice-entry_at) |
| `usize` | [`depth_at(flat_index: usize)`](#treeslice-depth_at) |
| `Option<usize>` | [`flat_index_of(node: NodeId)`](#treeslice-flat_index_of) |
| `bool` | [`is_expanded(node: NodeId)`](#treeslice-is_expanded) |
|  | [`expand(node: NodeId)`](#treeslice-expand) |
|  | [`collapse(node: NodeId)`](#treeslice-collapse) |
|  | [`toggle(node: NodeId)`](#treeslice-toggle) |
|  | [`expand_all()`](#treeslice-expand_all) |
|  | [`collapse_all()`](#treeslice-collapse_all) |
| `Vec<NodeId>` | [`expanded_nodes()`](#treeslice-expanded_nodes) |
|  | [`set_expanded_nodes(nodes: &[NodeId])`](#treeslice-set_expanded_nodes) |
| `Signal<u64>` | [`version_signal()`](#treeslice-version_signal) |
| `Option<usize>` | [`first_changed_index()`](#treeslice-first_changed_index) |
| `&TreeModel<T>` | [`tree()`](#treeslice-tree) |
| `TreeSliceHandle<T>` | [`handle()`](#treeslice-handle) |

### `TreeSliceHandle`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `usize` | [`visible_count()`](#treeslicehandle-visible_count) |
| `Option<FlatEntry>` | [`entry_at(flat_index: usize)`](#treeslicehandle-entry_at) |
| `Option<NodeId>` | [`visible_node_id(flat_index: usize)`](#treeslicehandle-visible_node_id) |
|  | [`expand(node: NodeId)`](#treeslicehandle-expand) |
|  | [`collapse(node: NodeId)`](#treeslicehandle-collapse) |
| `bool` | [`is_expanded(node: NodeId)`](#treeslicehandle-is_expanded) |
|  | [`toggle_expand(node: NodeId)`](#treeslicehandle-toggle_expand) |
| `&TreeModel<T>` | [`tree()`](#treeslicehandle-tree) |
|  | [`expand_all()`](#treeslicehandle-expand_all) |
| `Option<usize>` | [`first_changed_index()`](#treeslicehandle-first_changed_index) |

## Detailed description

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
built-in source for `TreeView`.

#### Example

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

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/tree_slice/index.html)

<a id="treeslice"></a>

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

<a id="treeslice-new"></a>

#### `pub fn new(tree: TreeModel<T>) -> Self`

Create a new `TreeSlice` for the given `TreeModel`.
All nodes start collapsed (only roots are visible).

<a id="treeslice-visible_count"></a>

#### `pub fn visible_count(&self) -> usize`

Number of currently visible (flattened) rows.

<a id="treeslice-with_entry"></a>

#### `pub fn with_entry<R>( &self, flat_index: usize, f: impl FnOnce(&T, &FlatEntry) -> R, ) -> Option<R>`

Access a flat entry by index via callback.
The callback receives `(&T, &FlatEntry)`.

<a id="treeslice-visible_node_id"></a>

#### `pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId>`

Get the `NodeId` at the given flat index.

<a id="treeslice-entry_at"></a>

#### `pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry>`

Get the `FlatEntry` at the given flat index (cloned).

<a id="treeslice-depth_at"></a>

#### `pub fn depth_at(&self, flat_index: usize) -> usize`

Get the depth at the given flat index.

<a id="treeslice-flat_index_of"></a>

#### `pub fn flat_index_of(&self, node: NodeId) -> Option<usize>`

Find the flat index for a given `NodeId`, or `None` if not visible.
O(1) — backed by a position map rebuilt on every reflatten.

<a id="treeslice-is_expanded"></a>

#### `pub fn is_expanded(&self, node: NodeId) -> bool`

Whether the given node is expanded.

<a id="treeslice-expand"></a>

#### `pub fn expand(&self, node: NodeId)`

Expand a node (make its children visible).

<a id="treeslice-collapse"></a>

#### `pub fn collapse(&self, node: NodeId)`

Collapse a node (hide its children).

<a id="treeslice-toggle"></a>

#### `pub fn toggle(&self, node: NodeId)`

Toggle expand/collapse state of a node.

<a id="treeslice-expand_all"></a>

#### `pub fn expand_all(&self)`

Expand all nodes in the tree.

<a id="treeslice-collapse_all"></a>

#### `pub fn collapse_all(&self)`

Collapse all nodes in the tree.

<a id="treeslice-expanded_nodes"></a>

#### `pub fn expanded_nodes(&self) -> Vec<NodeId>`

Get all expanded node IDs (for persistence).

<a id="treeslice-set_expanded_nodes"></a>

#### `pub fn set_expanded_nodes(&self, nodes: &[NodeId])`

Restore expanded state (for persistence).

<a id="treeslice-version_signal"></a>

#### `pub fn version_signal(&self) -> Signal<u64>`

Get the version signal for binding to `BindingLevel::Rebuild`.

<a id="treeslice-first_changed_index"></a>

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

<a id="treeslice-tree"></a>

#### `pub fn tree(&self) -> &TreeModel<T>`

Access the underlying `TreeModel`.

<a id="treeslice-handle"></a>

#### `pub fn handle(&self) -> TreeSliceHandle<T>`

Create a lightweight handle for use in closures.
Shares all Rc-based internals but does not keep the observer alive.

<a id="treeslicehandle"></a>

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

<a id="treeslicehandle-visible_count"></a>

#### `pub fn visible_count(&self) -> usize`

Number of currently-visible (flattened) rows.

<a id="treeslicehandle-entry_at"></a>

#### `pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry>`

Get the `FlatEntry` at `flat_index` (cloned), or `None` if out of bounds.

<a id="treeslicehandle-visible_node_id"></a>

#### `pub fn visible_node_id(&self, flat_index: usize) -> Option<NodeId>`

Get the `NodeId` at `flat_index`, or `None` if out of bounds.

<a id="treeslicehandle-expand"></a>

#### `pub fn expand(&self, node: NodeId)`

Expand `node` (make its children visible) and bump the version signal.
No-op if already expanded.

<a id="treeslicehandle-collapse"></a>

#### `pub fn collapse(&self, node: NodeId)`

Collapse `node` (hide its children) and bump the version signal.
No-op if already collapsed.

<a id="treeslicehandle-is_expanded"></a>

#### `pub fn is_expanded(&self, node: NodeId) -> bool`

Returns `true` if `node` is currently expanded.

<a id="treeslicehandle-toggle_expand"></a>

#### `pub fn toggle_expand(&self, node: NodeId)`

Toggle `node`'s expand/collapse state and bump the version signal.

<a id="treeslicehandle-tree"></a>

#### `pub fn tree(&self) -> &TreeModel<T>`

Access the underlying `TreeModel`.

<a id="treeslicehandle-expand_all"></a>

#### `pub fn expand_all(&self)`

Expand every node with children — see `TreeSlice::expand_all`. Useful
after a model rebuild reassigns `NodeId`s (the old expand set no longer
matches), to keep the view fully expanded.

<a id="treeslicehandle-first_changed_index"></a>

#### `pub fn first_changed_index(&self) -> Option<usize>`

See `TreeSlice::first_changed_index`.
