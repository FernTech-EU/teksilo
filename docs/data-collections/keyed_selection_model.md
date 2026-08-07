<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# KeyedSelectionModel

`KeyedSelectionModel<K>` — identity-based selection for collection widgets.

`KeyedSelectionModel<K>` stores selection as a set of
source-defined **keys** rather than visible **indices**. This is what
`SelectionModel` cannot do: survive lazy
window-slides and external reorders, and stay consistent across two views of
the same source that scroll/sort/filter independently (selection is a set of
identities, not positions). It coexists with the index-based
`SelectionModel` — views opt into one or the other.

Shift+click range extension is index-ordered by nature, so `extend_to` takes
the current visible key order from the caller (the projection) at click
time; the anchor is stored as a *key* so it survives scrolling out of the
resident window. The selection is exposed as a reactive
`Signal<HashSet<K>>` via `selection_signal()`.

## When to use

Use `KeyedSelectionModel` when rows are identified by a stable domain key
(entity id, file path, UUID) that survives reorders, sorts, and lazy-loading
evictions. Use `SelectionModel` when rows are
identified by their current visible index (simple in-memory lists).

```rust
# use teksilo_data::KeyedSelectionModel;
# use teksilo_data::SelectionMode;
let sel: KeyedSelectionModel<u64> = KeyedSelectionModel::new(SelectionMode::Multi);
sel.select(10);
sel.toggle(20);
sel.toggle(30);
assert_eq!(sel.count(), 3);
sel.toggle(10); // deselect
assert!(!sel.is_selected(&10));
sel.clear();
assert_eq!(sel.count(), 0);
```

## Builder methods at a glance

`mode`, `selection_signal`, `is_selected`, `selected_keys`, `count`, `select`, `toggle`, `extend_to`, `select_keys`, `clear`, `prune_missing`, `debug_named`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_data/keyed_selection_model/index.html)

## `pub struct KeyedSelectionModel`

Selection state keyed by source-defined identity rather than visible index.

The selection set is exposed as a `Signal<HashSet<K>>` (via
`selection_signal`) so widgets
observe it reactively without polling. Cloning the model shares the same
selection and anchor across all handles. The Shift+click anchor is stored as
a `K` so it survives lazy-window evictions and visible-order changes.

```rust
pub struct KeyedSelectionModel<K: ItemKey> { /* fields */ }
```

### Methods

#### `pub fn new(mode: SelectionMode) -> Self`

Create a new keyed selection model with the given mode.

#### `pub fn mode(&self) -> SelectionMode`

The selection mode.

#### `pub fn selection_signal(&self) -> Signal<HashSet<K>>`

A clone of the selection signal for reactive binding.

#### `pub fn is_selected(&self, key: &K) -> bool`

Whether `key` is currently selected (O(1)).

#### `pub fn selected_keys(&self) -> Vec<K>`

The currently selected keys (unordered snapshot).

#### `pub fn count(&self) -> usize`

Number of selected items.

#### `pub fn select(&self, key: K)`

Select a single key, clearing previous selection and setting the anchor.

#### `pub fn toggle(&self, key: K)`

Toggle a key (Ctrl+click in Multi mode; acts as `select` in Single).

#### `pub fn extend_to(&self, target: K, ordered_keys: &[K])`

Extend the selection from the anchor to `target` over the current visible
key order (Shift+click). `ordered_keys` is the projection's visible order
at click time. If the anchor isn't currently visible (scrolled out /
evicted), falls back to a single-key select.

#### `pub fn select_keys(&self, keys: impl IntoIterator<Item = K>, additive: bool)`

Replace the selection with `keys` (or, when `additive`, union them in).
Used by rubber-band selection. In `Single` mode an arbitrary one wins.

#### `pub fn clear(&self)`

Clear the selection and anchor.

#### `pub fn prune_missing(&self, exists: impl Fn(&K) -> bool)`

Drop any selected key (and the anchor) for which `exists` returns false.
Call after a removal/reset to prune deleted rows — the index-based
`adjust_for_insert`/`adjust_for_remove` are unnecessary here because keys
are stable across inserts, moves, sorts and filters.

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this model with the debug inspector under `name`; no-op in
release builds (`!cfg(debug_assertions)`). Returns `self` for chaining.
