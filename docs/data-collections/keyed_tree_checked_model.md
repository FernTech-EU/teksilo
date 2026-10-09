<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# KeyedTreeCheckedModel

`KeyedTreeCheckedModel<K>` — per-node checkbox state for a tree **keyed by a
stable domain id**, with optional descendant→ancestor tristate aggregation.

## Public functions

### `KeyedTreeCheckedModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(children: impl Fn(&K) -> Vec<K> + 'static, parent: impl Fn(&K) -> Option<K> + 'static)`](#keyedtreecheckedmodel-new) |
| `Self` | [`from_source<S>(source: S)`](#keyedtreecheckedmodel-from_source) |
| | **Builder methods** |
| `Self` | [`with_mode(mode: AggregateMode)`](#keyedtreecheckedmodel-with_mode) |
| | **Methods** |
| `AggregateMode` | [`aggregate_mode()`](#keyedtreecheckedmodel-aggregate_mode) |
|  | [`set_aggregate_mode(mode: AggregateMode)`](#keyedtreecheckedmodel-set_aggregate_mode) |
| `Signal<CheckState>` | [`signal_for(key: K)`](#keyedtreecheckedmodel-signal_for) |
| `Signal<bool>` | [`bool_signal_for(key: K)`](#keyedtreecheckedmodel-bool_signal_for) |
| `CheckState` | [`check_state(key: &K)`](#keyedtreecheckedmodel-check_state) |
|  | [`check(key: K)`](#keyedtreecheckedmodel-check) |
|  | [`uncheck(key: K)`](#keyedtreecheckedmodel-uncheck) |
|  | [`toggle(key: K)`](#keyedtreecheckedmodel-toggle) |
| `Vec<K>` | [`checked_keys()`](#keyedtreecheckedmodel-checked_keys) |
|  | [`clear()`](#keyedtreecheckedmodel-clear) |
|  | [`prune_missing(exists: impl Fn(&K) -> bool)`](#keyedtreecheckedmodel-prune_missing) |
|  | [`reaggregate()`](#keyedtreecheckedmodel-reaggregate) |

## Detailed description

The keyed counterpart of `TreeCheckedModel` — the
checkbox twin of `KeyedSelectionModel`. Where
`TreeCheckedModel` is bound to a `TreeModel<T>` and keyed by `NodeId`, this
model is keyed by *your* domain key `K` (an entity id, a tagged enum) and
takes the tree *shape* as two injected closures (`children` + `parent`), so
it composes over a `TreeDataSlice` or any
`TreeDataSource` — the "select scenes to export"
tristate over an external outline, without mirroring into a `TreeModel`.

Because identity is the domain key (stable across a full re-source), a
node's check state survives the tree reloading — a checked scene stays
checked after the backend refreshes. Use `prune_missing`
after a reload to drop the state of nodes that no longer exist.

Semantics, cascade behaviour, the `Signal<CheckState>` / `Signal<bool>`
bridge, and the re-entry guard are identical to `TreeCheckedModel` — see its
`module docs` for the detail. This model is a
share-by-clone handle (`Rc<RefCell<…>>` internally).

#### Example

```
use teksilo_data::{KeyedTreeCheckedModel, CheckState, TreeDataSlice, TreeRow};

// An outline: Binder(1) → { Chapter(2) → Scene(3), Scene(4) }
let slice: TreeDataSlice<u64, &str> = TreeDataSlice::from_rows(vec![
    TreeRow::new(1, "Binder", 0),
    TreeRow::new(2, "Chapter", 1),
    TreeRow::new(3, "Scene A", 2),
    TreeRow::new(4, "Scene B", 1),
]);

let checked = KeyedTreeCheckedModel::from_source(slice.clone());
let _ = (checked.signal_for(1), checked.signal_for(2), checked.signal_for(3), checked.signal_for(4));

checked.check(3);                                         // one scene under the chapter
assert_eq!(checked.check_state(&2), CheckState::Checked); // chapter has only Scene A → Checked
assert_eq!(checked.check_state(&1), CheckState::Indeterminate); // Binder: 2 of {chapter, Scene B}
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/keyed_tree_checked_model/index.html)

<a id="keyedtreecheckedmodel"></a>

## `pub struct KeyedTreeCheckedModel`

Per-node checkbox state for a domain-keyed tree, with optional
descendant→ancestor tristate aggregation. See the `module docs`.

```rust
pub struct KeyedTreeCheckedModel<K: ItemKey> { /* fields */ }
```

### Methods

<a id="keyedtreecheckedmodel-new"></a>

#### `pub fn new( children: impl Fn(&K) -> Vec<K> + 'static, parent: impl Fn(&K) -> Option<K> + 'static, ) -> Self`

Create a model over a tree whose shape is given by two closures:
`children(key) -> Vec<K>` and `parent(key) -> Option<K>`. Uses the
default `AggregateMode::DescendantsDriveAncestors`.

<a id="keyedtreecheckedmodel-from_source"></a>

#### `pub fn from_source<S>(source: S) -> Self where S: TreeDataSource<Key = K> + Clone + 'static,`

Create a model whose tree shape is read from a cloneable
`TreeDataSource` (e.g. a `TreeDataSlice`). The
source is cloned into the shape closures, so the model reflects the live
tree — call `prune_missing` after the source
reloads to drop state for removed nodes.

<a id="keyedtreecheckedmodel-with_mode"></a>

#### `pub fn with_mode(self, mode: AggregateMode) -> Self`

Set the `AggregateMode` at construction.

<a id="keyedtreecheckedmodel-aggregate_mode"></a>

#### `pub fn aggregate_mode(&self) -> AggregateMode`

The current `AggregateMode`.

<a id="keyedtreecheckedmodel-set_aggregate_mode"></a>

#### `pub fn set_aggregate_mode(&self, mode: AggregateMode)`

Change the cascade behaviour; takes effect on the next write.

<a id="keyedtreecheckedmodel-signal_for"></a>

#### `pub fn signal_for(&self, key: K) -> Signal<CheckState>`

Writable `Signal<CheckState>` for `key` (cached). External writes trigger
the configured aggregation pass. The cascade observer is wired
**idempotently** — including for a signal first materialised by a cascade
(`write_state`) before its own `signal_for` was ever called — so binding a
lazily-realised (e.g. virtualized) row still cascades on write.

<a id="keyedtreecheckedmodel-bool_signal_for"></a>

#### `pub fn bool_signal_for(&self, key: K) -> Signal<bool>`

Two-state `Signal<bool>` projection of `signal_for`
(cached, writable). `Checked → true`; anything else → `false`. See
`crate::TreeCheckedModel::bool_signal_for`.

<a id="keyedtreecheckedmodel-check_state"></a>

#### `pub fn check_state(&self, key: &K) -> CheckState`

The current `CheckState` for `key` (`Unchecked` if never touched).

<a id="keyedtreecheckedmodel-check"></a>

#### `pub fn check(&self, key: K)`

Set `key` to `CheckState::Checked` (triggers cascade + ancestor recompute).

<a id="keyedtreecheckedmodel-uncheck"></a>

#### `pub fn uncheck(&self, key: K)`

Set `key` to `CheckState::Unchecked` (triggers cascade + ancestor recompute).

<a id="keyedtreecheckedmodel-toggle"></a>

#### `pub fn toggle(&self, key: K)`

Toggle `key`: a leaf under `DescendantsDriveAncestors` cycles two-state;
a branch or `AggregateMode::None` cycles the full tristate sequence.

<a id="keyedtreecheckedmodel-checked_keys"></a>

#### `pub fn checked_keys(&self) -> Vec<K>`

All keys whose current state is exactly `CheckState::Checked`. May
include stale keys after a tree mutation — call `prune_missing`
or filter against the current tree yourself.

<a id="keyedtreecheckedmodel-clear"></a>

#### `pub fn clear(&self)`

Reset all known nodes to `CheckState::Unchecked`.

Writes every tracked key directly via the internal `write_state`
helper (per-key cascade-suppressed) instead of `signal_for(..).set(..)`'s normal
path, which would, for every currently-checked key, cascade the
write down its whole descendant subtree and recompute every
ancestor up to the root — redundant here, since every tracked key
ends up `Unchecked` and "all children unchecked" is already the
correct parent aggregate. See `TreeCheckedModel::clear`
for the non-keyed twin of this same optimization.

<a id="keyedtreecheckedmodel-prune_missing"></a>

#### `pub fn prune_missing(&self, exists: impl Fn(&K) -> bool)`

Drop cached check state (and its signals/observers) for every key for
which `exists(&key)` returns `false`, then `reaggregate`
surviving parents against the current tree. Call after a reload so a
deleted node's state doesn't linger in `checked_keys()` **and** the
ancestors it used to affect show the correct tristate. Mirrors
`crate::KeyedSelectionModel::prune_missing`.

<a id="keyedtreecheckedmodel-reaggregate"></a>

#### `pub fn reaggregate(&self)`

Recompute every surviving parent's aggregate from the **current** tree
shape + leaf states, deepest first. Call after the backing tree's
structure changed (a reload that added/removed/moved nodes) so parent
tristates reflect the new children; `prune_missing`
does this for you. A no-op under `AggregateMode::None`.
