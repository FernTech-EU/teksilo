<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CheckedModel

`CheckedModel` — per-row checkbox state for flat collection widgets.

Tracks which rows in a list view are marked (checked), independently of
which row is selected. Selection (cursor position) and checked-ness
(persistent marks) are orthogonal axes — the Outlook / Files-app
convention where you can check many items and then act on them all.

The model issues one writable `Signal<bool>` per row index via
`CheckedModel::signal_for`; repeated calls for the same index return the
same cached handle. A `Checkbox` widget writes to that signal on click;
the model observes every per-index signal and keeps a central
`Signal<BTreeSet<usize>>` in sync so consumers can react to the complete
checked set without subscribing to each row individually.

`CheckedModel` is a share-by-clone handle (`Rc<RefCell<…>>` internally);
cloning produces a second handle to the same state. When rows are inserted,
removed, or reordered, call the corresponding `adjust_for_*` method so that
checked state follows the moved items rather than sticking to stale indices.

For hierarchical lists with descendant→ancestor tristate aggregation, see
`crate::TreeCheckedModel` instead.

```rust
# use bastyde_data::CheckedModel;
let model = CheckedModel::new();
model.check(1);
model.check(3);
assert!(model.is_checked(1));
assert_eq!(model.checked_count(), 2);
model.toggle(1);
assert!(!model.is_checked(1));
```

## Builder methods at a glance

`checked_signal`, `signal_for`, `adjust_for_insert`, `adjust_for_remove`, `adjust_for_move`, `is_checked`, `checked_indices`, `checked_count`, `check`, `uncheck`, `toggle`, `check_all`, `clear`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/checked_model/index.html)

## `pub struct CheckedModel`

Per-row checkbox state for a flat list, with a reactive aggregate checked-set.

```rust
pub struct CheckedModel { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Creates a new, empty `CheckedModel` with no rows checked.

#### `pub fn checked_signal(&self) -> Signal<BTreeSet<usize>>`

Reactive view of the full checked-set.

#### `pub fn signal_for(&self, index: usize) -> Signal<bool>`

Writable per-index signal. Repeat calls cache the same handle —
any consumer (the model itself, the Checkbox widget, an external
observer) writing through it propagates to the central
`checked_signal()`.

#### `pub fn adjust_for_insert(&self, start: usize, count: usize)`

Shift checked-state after `count` rows are inserted at `start`.
Indices `>= start` move up by `count`.

#### `pub fn adjust_for_remove(&self, start: usize, count: usize)`

Shift checked-state after `count` rows starting at `start` are removed.
Checked rows in `start..start+count` are dropped; later rows shift down.

#### `pub fn adjust_for_move(&self, from: usize, to: usize, count: usize)`

Shift checked-state after a block of `count` rows moved from `from` to
`to` (a post-removal index, matching `ListModel::move_item`). Checked
rows follow their items.

#### `pub fn is_checked(&self, index: usize) -> bool`

Returns `true` if the row at `index` is currently checked.

#### `pub fn checked_indices(&self) -> Vec<usize>`

Returns a sorted `Vec` of every currently checked row index.

#### `pub fn checked_count(&self) -> usize`

Returns the number of currently checked rows.

#### `pub fn check(&self, index: usize)`

Marks the row at `index` as checked; notifies observers if the state changed.

#### `pub fn uncheck(&self, index: usize)`

Marks the row at `index` as unchecked; notifies observers if the state changed.

#### `pub fn toggle(&self, index: usize)`

Flips the checked state of the row at `index`; notifies observers.

#### `pub fn check_all(&self, count: usize)`

Checks every row in `0..count`; notifies observers for each row that was unchecked.

#### `pub fn clear(&self)`

Unchecks every currently checked row; notifies observers for each change.
