<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeDataSlice

`TreeDataSlice` — the reusable `TreeDataSource` engine for an **external,
indent-ordered** tree (a Qleany entity store, a database, a virtual
filesystem) that is NOT mirrored into a `TreeModel`.

`TreeSlice` gives per-view expand state + flattening +
divergence to a `TreeModel`. `TreeDataSlice` gives the **same machinery** to
a source whose identity is a domain key (`K = i64` entity id, a tagged enum,
…) and whose natural shape is a flat, pre-order, indent-annotated row stream
— the shape an outline is genuinely stored in (Scrivener-class binders /
chapters / scenes, OPML, Markdown headings). The app hands over
`Vec<``TreeRow``<K, T>>` (`{ key, item, depth }`, document order) on every
(re)load; the engine owns everything else:

* **tree derivation** — parent links + child index + roots + structural
  depth, derived from the indent sequence (an item's parent is the nearest
  preceding row of strictly smaller depth; depth-0 rows are roots);
* **per-view expand state** — a `K`-keyed set, so two slices over the same
  source expand independently and expand survives a full re-source;
* **collapse-aware flattening** into the visible row list;
* **divergence** (`first_changed_index`)
  — the common-prefix of the old vs new visible rows, comparing key + depth +
  has-children + expand **and item content** (hence the `T: PartialEq`
  bound), so a consumer caching per-row state (a measured row height) keeps
  its valid prefix across reloads and expand toggles;
* **DnD mechanism** — the cycle guard + `can_accept`/`accept_drop` plumbing;
  domain *policy* is injected as closures (`TreeDataSlice::set_drag_policy`,
  `TreeDataSlice::set_drop_resolver`, `TreeDataSlice::set_reorder`).

It is a cheap `Rc`-handle (clone = share, like `ListModel` / `SceneModel`):
pass one clone to `TreeView::from_source` and keep another to drive
`reload` / `set_rows` from
the app.

## Wiring an external source

```
use bastyde_data::{TreeDataSlice, TreeRow};
use bastyde_data::dnd_types::{DragEligibility, DropPosition};

// key = entity id, item = the row's display data
let slice: TreeDataSlice<u64, String> = TreeDataSlice::new();
slice.set_expand_new_nodes(true);             // new nodes appear expanded
slice.set_source(|| vec![                     // your `rows::load`
    TreeRow::new(1, "Binder".to_string(), 0),
    TreeRow::new(2, "Chapter".to_string(), 1),
    TreeRow::new(3, "Scene".to_string(), 2),
]);
slice.set_drag_policy(|key| if *key == 1 { DragEligibility::NoDrag } else { DragEligibility::CanDrag });
slice.set_reorder(|_dragged, _target, _pos: DropPosition| { /* backend move + undo */ true });
slice.reload();

assert_eq!(slice.visible_count(), 3);         // all expanded
// let view = TreeView::from_source(slice.clone(), delegate);
```

## Builder methods at a glance

`set_source`, `set_reorder`, `set_drag_policy`, `set_drop_resolver`, `set_expand_new_nodes`, `from_rows`, `reload`, `set_rows`, `visible_count`, `with_entry`, `with_key`, `key_at`, `entry_at`, `depth_at`, `flat_index_of`, `contains_key`, `parent_of`, `child_keys_of`, `is_expanded`, `expand`, `collapse`, `toggle`, `expand_all`, `collapse_all`, `expanded_keys`, `set_expanded_keys`, `version_signal`, `first_changed_index`, `set_all_expanded`, `all_expanded`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/tree_data_slice/index.html)

## `pub struct TreeRow`

One row the app hands to a `TreeDataSlice`, in **document (pre-)order**.

`depth` is the indent level (`0` = a root). The engine derives each row's
parent, children, and structural depth from the `depth` sequence: a row's
parent is the nearest preceding row with a strictly smaller `depth`. The row
stream must be well-formed pre-order (a parent precedes its subtree) — the
shape any indent-stored outline already has.

```rust
pub struct TreeRow<K, T> { /* fields */ }
```

### Methods

#### `pub fn new(key: K, item: T, depth: usize) -> Self`

Convenience constructor.

## `pub struct TreeDataSlice`

Per-view flattened projection of an external, indent-ordered tree source.
See the `module documentation`.

```rust
pub struct TreeDataSlice<K: ItemKey, T> { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty slice. Configure it (`set_source` / `set_reorder` /
policies / `set_expand_new_nodes`) then populate with
`reload` or `set_rows`.

#### `pub fn set_source(&self, f: impl Fn() -> Vec<TreeRow<K, T>> + 'static)`

Install the row source (`rows::load`). `reload` and a
committed drop call it to re-materialise the tree.

#### `pub fn set_reorder(&self, f: impl Fn(K, K, DropPosition) -> bool + 'static)`

Install the reorder command (`dragged, target, position -> applied`).
Without one, drops are refused.

#### `pub fn set_drag_policy(&self, f: impl Fn(&K) -> DragEligibility + 'static)`

Install the per-row drag gate. Without one, no row is draggable.

#### `pub fn set_drop_resolver( &self, f: impl Fn(&K, &K, &T, DropPosition) -> Option<DropPosition> + 'static, )`

Install the domain drop resolver. The engine's cycle guard (no drop into
your own subtree, no self-drop) runs first, then hands the resolver
`(dragged, target, target_item, position)`; return `Some(pos)` to accept
at `pos` (a different `pos` snaps the indicator, i.e.
`DropResponse::Redirect`) or `None` to forbid. Without one, any
non-cyclic drop is accepted at the requested position.

#### `pub fn set_expand_new_nodes(&self, expand: bool)`

Whether nodes appearing for the first time start expanded (`true`) or
collapsed (`false`, the default, matching `TreeSlice`). Set this **before**
the first populate to affect the initial rows.

#### `pub fn from_rows(rows: Vec<TreeRow<K, T>>) -> Self`

Build a slice directly from an initial row stream (no version bump / no
divergence — construction is not a change).

#### `pub fn reload(&self) where T: PartialEq,`

Re-source the rows via the `set_source` closure and
reproject. No-op if no source is installed.

#### `pub fn set_rows(&self, rows: Vec<TreeRow<K, T>>) where T: PartialEq,`

Replace the rows with a freshly-sourced stream, preserving per-view
expand state by key, computing `first_changed_index`,
and bumping the version signal.

#### `pub fn visible_count(&self) -> usize`

Number of currently-visible (flattened) rows.

#### `pub fn with_entry<R>( &self, flat_index: usize, f: impl FnOnce(&T, &FlatEntry<K>) -> R, ) -> Option<R>`

Access the item + flat metadata at a visible index via callback.

#### `pub fn with_key<R>(&self, key: &K, f: impl FnOnce(&T) -> R) -> Option<R>`

Access a node's item by key via callback, **regardless of visibility** (a
node hidden under a collapsed ancestor is still reachable). Returns `None`
if the key is absent from the source. The by-key counterpart of
`with_entry` (which is by visible index) — use it to
resolve a key to its domain payload.

#### `pub fn key_at(&self, flat_index: usize) -> Option<K>`

The key of the row at a visible index.

#### `pub fn entry_at(&self, flat_index: usize) -> Option<FlatEntry<K>>`

The `FlatEntry` at a visible index (cloned).

#### `pub fn depth_at(&self, flat_index: usize) -> usize`

Structural depth at a visible index (`0` for a root).

#### `pub fn flat_index_of(&self, key: &K) -> Option<usize>`

The visible index of a key, if currently visible.

#### `pub fn contains_key(&self, key: &K) -> bool`

Whether `key` still exists in the source, independent of visibility (a
node hidden under a collapsed ancestor still exists).

#### `pub fn parent_of(&self, key: &K) -> Option<K>`

The parent of a node (`None` for a root or an absent key).

#### `pub fn child_keys_of(&self, key: &K) -> Vec<K>`

The children of a node, in order (empty for a leaf / absent key). O(children).

#### `pub fn is_expanded(&self, key: &K) -> bool`

Whether the node is *effectively* expanded (its children shown) — `true`
for every branch while the `set_all_expanded`
reveal override is on, otherwise its per-view expand state. Use
`expanded_keys` for the persistent set.

#### `pub fn expand(&self, key: &K) where T: PartialEq,`

Expand a node (make its children visible).

#### `pub fn collapse(&self, key: &K) where T: PartialEq,`

Collapse a node (hide its children).

#### `pub fn toggle(&self, key: &K) where T: PartialEq,`

Toggle a node's expand state.

#### `pub fn expand_all(&self) where T: PartialEq,`

Expand every node that has children.

#### `pub fn collapse_all(&self) where T: PartialEq,`

Collapse every node (only roots remain visible).

#### `pub fn expanded_keys(&self) -> Vec<K>`

The currently-expanded keys (for persistence).

#### `pub fn set_expanded_keys(&self, keys: &[K]) where T: PartialEq,`

Restore expanded state (for persistence). Keys absent from the source are
ignored on the next reflatten.

#### `pub fn version_signal(&self) -> Signal<u64>`

Version signal — bind at `BindingLevel::Rebuild`. Bumps on every
`set_rows` / expand / collapse.

#### `pub fn first_changed_index(&self) -> Option<usize>`

First visible index whose content may differ after the latest change —
rows `0..index` are unchanged (same key, depth, has-children, expand, and
item content), so per-row derived state remains valid for them. Equal to
`visible_count()` when nothing visible changed; `None` before the first
change (construction is not a change).

#### `pub fn set_all_expanded(&self, on: bool) where T: PartialEq,`

Reveal override for a filtered view: when `on`, the flatten treats every
node as expanded, so all rows in the (already sort/filter-narrowed) stream
are visible — the ancestors `TreeRowFilter::KeepAncestors` keeps no longer
hide their matching descendants. The per-view expand set is **preserved**
underneath, so turning it off restores the user's real collapse state.
Flip it on with the filter and off when it clears. No-op if unchanged.

#### `pub fn all_expanded(&self) -> bool`

Whether the reveal-all override is on (see `set_all_expanded`).
