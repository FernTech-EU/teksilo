<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SelectionModel

SelectionModel — index-based selection state for collection widgets.

## Public types

| Kind | Name |
| ---: | :--- |
| `enum` | [`SelectionMode`](#selectionmode) — Selection behavior mode |
| `struct` | [`SelectionModel`](#selectionmodel) — Manages selection state for a collection widget |

## Public functions

### `SelectionModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(mode: SelectionMode)`](#selectionmodel-new) |
| | **Builder methods** |
| `Self` | [`debug_named(_name: impl Into<String>)`](#selectionmodel-debug_named) |
| | **Methods** |
| `SelectionMode` | [`mode()`](#selectionmodel-mode) |
| `Signal<BTreeSet<usize>>` | [`selection_signal()`](#selectionmodel-selection_signal) |
| `bool` | [`is_selected(index: usize)`](#selectionmodel-is_selected) |
| `Vec<usize>` | [`selected_indices()`](#selectionmodel-selected_indices) |
| `usize` | [`count()`](#selectionmodel-count) |
|  | [`select(index: usize)`](#selectionmodel-select) |
|  | [`toggle(index: usize)`](#selectionmodel-toggle) |
|  | [`extend_to(index: usize)`](#selectionmodel-extend_to) |
|  | [`extend_to_additive(index: usize)`](#selectionmodel-extend_to_additive) |
|  | [`select_indices(indices: impl IntoIterator<Item = usize>, additive: bool)`](#selectionmodel-select_indices) |
|  | [`select_all(count: usize)`](#selectionmodel-select_all) |
|  | [`clear()`](#selectionmodel-clear) |
|  | [`adjust_for_insert(start: usize, count: usize)`](#selectionmodel-adjust_for_insert) |
|  | [`adjust_for_remove(start: usize, count: usize)`](#selectionmodel-adjust_for_remove) |
|  | [`adjust_for_move(from: usize, to: usize, count: usize)`](#selectionmodel-adjust_for_move) |
|  | [`invalidate_anchor_from(first_changed: usize)`](#selectionmodel-invalidate_anchor_from) |

## Detailed description

`SelectionModel` manages which flat indices are selected in a
`ListView`, `TreeView`, `TableView`, or `GridView`. It is a
share-by-clone handle (`Rc<RefCell<…>>` internally): pass a clone to
each view that should share selection state. The current selection is
exposed as a reactive `Signal<BTreeSet<usize>>` so widgets can bind to
it without polling.

Three selection behaviours are available via `SelectionMode`: `None`
(read-only / no interaction), `Single` (at most one item), and `Multi`
(Ctrl+click toggle + Shift+click range extension via an internal anchor).
Mutators automatically notify all `Signal` observers after every change,
and the helper methods `adjust_for_insert` / `adjust_for_remove` /
`adjust_for_move` keep selected indices consistent when the underlying
source mutates.

#### When to use `SelectionModel` vs `KeyedSelectionModel`

Use `SelectionModel` (this type) for views that are backed by a plain
`ListModel<T>` or a `SortFilterListModel<T>` where *position* is the
natural identity. Use `crate::KeyedSelectionModel` when items carry a
stable app-defined key (e.g. a `NodeId` or a UUID) and selection must
survive sort/filter rebuilds or window slides that renumber visible indices.

```rust
# use teksilo_data::{SelectionModel, SelectionMode};
let sel = SelectionModel::new(SelectionMode::Multi);
sel.select(2);         // clear-and-select index 2, anchor = 2
sel.toggle(5);         // add index 5 (Ctrl+click behaviour)
sel.extend_to(8);      // extend from anchor 5 to 8 (Shift+click behaviour)
assert!(sel.is_selected(2));
assert_eq!(sel.count(), 5); // 2, 5, 6, 7, 8
sel.clear();
assert_eq!(sel.count(), 0);
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/selection_model/index.html)

<a id="selectionmode"></a>

## `pub enum SelectionMode`

Selection behavior mode.

```rust
pub enum SelectionMode { /* variants */ }
```

### Variants

- **`None`** — No selection allowed.
- **`Single`** — At most one item selected at a time.
- **`Multi`** — Multiple items can be selected (Ctrl+click toggles, Shift+click extends).

<a id="selectionmodel"></a>

## `pub struct SelectionModel`

Manages selection state for a collection widget.

The selection is exposed as a `Signal<BTreeSet<usize>>` so widgets can
observe changes reactively.

```rust
pub struct SelectionModel { /* fields */ }
```

### Methods

<a id="selectionmodel-new"></a>

#### `pub fn new(mode: SelectionMode) -> Self`

Create a new selection model with the given mode.

<a id="selectionmodel-mode"></a>

#### `pub fn mode(&self) -> SelectionMode`

The selection mode.

<a id="selectionmodel-selection_signal"></a>

#### `pub fn selection_signal(&self) -> Signal<BTreeSet<usize>>`

Get a clone of the selection signal for reactive binding.

<a id="selectionmodel-is_selected"></a>

#### `pub fn is_selected(&self, index: usize) -> bool`

Whether the given index is currently selected.

<a id="selectionmodel-selected_indices"></a>

#### `pub fn selected_indices(&self) -> Vec<usize>`

The currently selected indices, sorted.

<a id="selectionmodel-count"></a>

#### `pub fn count(&self) -> usize`

Number of selected items.

<a id="selectionmodel-select"></a>

#### `pub fn select(&self, index: usize)`

Select a single index. In Single mode, clears previous selection.
In Multi mode, clears previous and selects just this one (use `toggle`
for Ctrl+click behavior). Sets the anchor for subsequent Shift+click.

<a id="selectionmodel-toggle"></a>

#### `pub fn toggle(&self, index: usize)`

Toggle selection of a single index (for Ctrl+click in Multi mode).
In Single mode, behaves like `select()`.

<a id="selectionmodel-extend_to"></a>

#### `pub fn extend_to(&self, index: usize)`

Extend the selection from the anchor to the given index (for Shift+click
and Shift+navigation). In Single mode, behaves like `select()`.

The result is `base ∪ anchor..=index`, where `base` is the selection as
it stood at the last non-extending mutation — **not** the current
selection. So the range tracks the anchor in both directions: reversing
a Shift gesture shrinks it, and `Shift+End` followed by `Shift+Home`
leaves one row selected rather than the whole collection. The anchor
itself does not move.

<a id="selectionmodel-extend_to_additive"></a>

#### `pub fn extend_to_additive(&self, index: usize)`

Extend from the anchor to `index`, keeping whatever was selected when
this gesture began (Ctrl+Shift+navigation).

The difference from `extend_to` is only which set the
range is unioned with: a plain gesture starts from the base committed by
the last click or toggle, while this one captures the live selection on
its first keystroke, so a second disjoint range can be built without
losing the first. Subsequent keystrokes in the same gesture reuse that
capture, so the range still shrinks when reversed.

<a id="selectionmodel-select_indices"></a>

#### `pub fn select_indices(&self, indices: impl IntoIterator<Item = usize>, additive: bool)`

Replace the selection with `indices` (or, when `additive`, union them
into the current selection). Used by rubber-band / marquee selection,
where the selected set is an arbitrary subset rather than a range. In
`Single` mode the highest index wins; `None` mode is a no-op.

<a id="selectionmodel-select_all"></a>

#### `pub fn select_all(&self, count: usize)`

Select all indices from 0 to count-1.

A no-op in `None` mode, and also in `Single` mode — "select all" has
no coherent meaning for a control that holds at most one item, and
silently selecting one arbitrary row would be worse than doing
nothing. This mirrors what the gated call sites already do
(`ListView`'s Ctrl+A handler, which documents it as "Multi selection
only — a no-op for Single / None, matching every list control", and
`TableView`'s `select_all` helper, which matches only the Multi
modes). Enforcing it here too keeps an ungated caller — `GridView`'s
Ctrl+A handler is one — from breaking the `Single` invariant that
every other mutator on this type upholds.

<a id="selectionmodel-clear"></a>

#### `pub fn clear(&self)`

Clear the selection.

<a id="selectionmodel-adjust_for_insert"></a>

#### `pub fn adjust_for_insert(&self, start: usize, count: usize)`

Adjust selection indices after items are inserted.
Indices >= `start` are shifted up by `count`.

<a id="selectionmodel-adjust_for_remove"></a>

#### `pub fn adjust_for_remove(&self, start: usize, count: usize)`

Adjust selection indices after items are removed.
Indices in `start..start+count` are deselected; indices above are shifted down.

<a id="selectionmodel-adjust_for_move"></a>

#### `pub fn adjust_for_move(&self, from: usize, to: usize, count: usize)`

Adjust selection indices after a block of `count` items moved from
`from` to `to` (a post-removal index, matching `ListModel::move_item`).
Selected indices follow their items, so a dragged row stays selected.

<a id="selectionmodel-invalidate_anchor_from"></a>

#### `pub fn invalidate_anchor_from(&self, first_changed: usize)`

Drop the range anchor when a projection has renumbered the rows under
it, so the next `Shift` gesture starts from the cursor rather than from
a row that has since moved.

A sort/filter proxy signals a blanket reset rather than a per-row
delta, so `adjust_for_*` never runs and an index anchor silently comes
to mean a different row. Views that read
`first_changed_index()` from `SortFilterListModel` / `TreeSlice` /
`TreeDataSlice` / `SortFilterTreeModel` call this with it: everything
before that index still means what it meant, so an anchor there
survives. Qt hit the same bug and fixed it by making the anchor a
persistent index; `KeyedSelectionModel` avoids it by construction.

<a id="selectionmodel-debug_named"></a>

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this selection model with the debug inspector under
`name`. In release builds (`!cfg(debug_assertions)`) this is a
no-op pass-through so call sites stay free of `#[cfg]` lines.

Idempotent on repeated calls — the latest registration wins.
The registration drops automatically when the last
`SelectionModel` handle is freed (the strong adapter `Rc` lives
inside a shared holder; the registry holds only a `Weak`).
