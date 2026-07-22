<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ListModel

`ListModel<T>` — concrete reactive list backed by a `Vec<T>`.

`ListModel<T>` stores items in a heap-allocated `Vec<T>` behind
`Rc<RefCell<…>>`. Cloning a handle shares the same underlying data — there
is no deep copy. Every mutation method (`push`, `insert`, `remove`, `set`,
`move_item`, `replace_all`, `clear`) drops the internal borrow before
notifying observers, so observer callbacks may safely call read methods
(`len`, `with_item`) without a re-entrant borrow.

`ListModel<T>` implements `ListDataSource` directly, so it can be handed
to any `ListView` / `TableView` without adaption. For lists too large to
hold in memory, implement `ListDataSource` directly on your own type
(paged database cursor, windowed feed, etc.).

## When to use

Use `ListModel<T>` when the full list fits in memory and you want automatic
change notifications with no extra setup. Use a custom `ListDataSource`
when the source is external, huge, or lazy-loaded.

## Notifications

Observers registered via `ListModel::observe_changes` receive a
`DataChange` describing the minimal change: `ItemsInserted`,
`ItemsRemoved`, `ItemUpdated`, `ItemsMoved`, or `Reset`. The
`ObserverHandle` returned is RAII — dropping
it unregisters the callback immediately.

```rust
# use bastyde_data::ListModel;
let model: ListModel<&str> = ListModel::new();
model.push("alpha");
model.push("beta");
model.push("gamma");
assert_eq!(model.len(), 3);
let second = model.with_item(1, |s| *s);
assert_eq!(second, Some("beta"));
model.set(0, "ALPHA");
model.remove(2);
assert_eq!(model.len(), 2);
```

## Builder methods at a glance

`from_vec`, `len`, `is_empty`, `with_item`, `push`, `insert`, `remove`, `set`, `move_item`, `move_items`, `replace_all`, `clear`, `observe_changes`, `reconcile_by_key`, `debug_named`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/list_model/index.html)

## `pub struct ListModel`

A concrete reactive list that stores items in a `Vec<T>`.

`ListModel<T>` is `Clone` — cloning produces a second handle to the same
data. Multiple widgets can hold clones and all see the same items.

Every mutation method modifies the internal Vec, drops the mutable borrow,
then notifies observers. By the time any observer runs, the borrow is
released and shared borrows (`len()`, `with_item()`) are safe.

```rust
pub struct ListModel<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty list model.

#### `pub fn from_vec(items: Vec<T>) -> Self`

Create a list model from an existing vector.

#### `pub fn len(&self) -> usize`

Number of items in the list.

#### `pub fn is_empty(&self) -> bool`

Whether the list is empty.

#### `pub fn with_item<R>(&self, index: usize, f: impl FnOnce(&T) -> R) -> Option<R>`

Access an item by index via a callback. Returns `None` if out of bounds.

The callback pattern avoids returning a reference that would need to
outlive the `RefCell` borrow guard.

#### `pub fn push(&self, item: T)`

Append an item to the end of the list.

#### `pub fn insert(&self, index: usize, item: T)`

Insert an item at the given index.

# Panics
Panics if `index > len()`.

#### `pub fn remove(&self, index: usize) -> T`

Remove and return the item at the given index.

# Panics
Panics if `index >= len()`.

#### `pub fn set(&self, index: usize, item: T)`

Replace the item at the given index.

# Panics
Panics if `index >= len()`.

#### `pub fn move_item(&self, from: usize, to: usize)`

Move an item from one index to another.

The item at `from` is removed, then inserted at `to` (post-removal index).

# Panics
Panics if either index is out of bounds.

#### `pub fn move_items(&self, indices: &[usize], insert_gap: usize) -> bool`

Move a set of items so they land **contiguously** at a drop gap,
preserving their relative order — the multi-row same-view reorder
commit. `indices` are the items' current positions (any order;
out-of-range entries are ignored); `insert_gap` is the destination in
`0..=len` expressed in the pre-move indexing (i.e. "land before the item
currently at `insert_gap`"; `len` = at the end).

Returns whether anything moved (`false` if `indices` held no in-range
entry). A **contiguous** source block emits a single
`DataChange::ItemsMoved` — so index-based selection follows the moved
rows; a non-contiguous set emits `DataChange::Reset` (that permutation
is not expressible as one `ItemsMoved`, and selection is dropped). For a
single index prefer `move_item`.

#### `pub fn replace_all(&self, items: Vec<T>)`

Replace the entire list contents.

#### `pub fn clear(&self)`

Remove all items from the list.

#### `pub fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle`

Register an observer that is called on every mutation.
Returns an `ObserverHandle` — dropping it removes the callback.

#### `pub fn reconcile_by_key<K: Eq + Hash>(&self, new_items: Vec<T>, key_fn: impl Fn(&T) -> K)`

Reconcile the list's contents with `new_items`, matching old and new
rows **by key** (`key_fn`) instead of wholesale-replacing them, and
emitting the minimal set of granular `DataChange`s needed to reach
that state — never `DataChange::Reset`.

This is the primitive a live view needs when a peer process (or any
other out-of-band writer) reloads a backing file and the merged
result must land in a `ListModel` that a `ListView` is *currently
displaying*, without wiping the user's selection or keyboard focus
mid-interaction. `replace_all`/`clear` always emit `Reset`, and a
`Reset` unconditionally clears a positional `SelectionModel`
(`RowSelection::from_index`) — `reconcile_by_key` is how a caller
avoids that.

Emits, in this order, coalescing contiguous runs into a single event
each:
- `DataChange::ItemsRemoved` for keys present in the old list but
  absent from `new_items`;
- `DataChange::ItemsMoved` (single-row blocks) to re-order the
  surviving rows into `new_items`'s relative order — skipped
  entirely for rows already in the right place, so an append-only or
  remove-only reload emits **no** moves at all;
- `DataChange::ItemsInserted` for keys present in `new_items` but
  not in the old list;
- `DataChange::ItemUpdated` for a row whose key is unchanged but
  whose content differs (`T: PartialEq`) — the row's stored value is
  replaced with the incoming one.

If `new_items` is identical (same keys, same order, same content, by
`PartialEq`) to the current contents, **no** change is emitted and no
observer runs — reconciling with unchanged data is silent.

# Preconditions
`key_fn` must be a pure, stable function of an item's identity (not
its content) and keys must be **unique** within both the current list
and `new_items`. See `# Panics` below — violating either is a caller
bug, not a silently-tolerated edge case.

# Panics
Panics (via an internal `.expect`) if `key_fn` is not stable — it
returns a different key for the same item across the two calls this
method makes to it (once while snapshotting the current list's keys,
once while re-deriving a key during the write pass) — or if a key is
**duplicated** within the current list or within `new_items`. Both
break the same invariant the write pass relies on: "the item that
was accounted for under this key is still findable at or after the
write cursor." A duplicate key means two different items raced to
claim one key slot, so by the time the second one is processed the
slot the accounting expected is already gone. This is this crate's
usual documented-panic-on-contract-violation style (see e.g.
`TreeModel::remove`) — a caller-side bug
surfaced immediately as a panic, not silently wrong data.

# Complexity
Re-ordering is a straightforward left-to-right pass that moves each
out-of-place survivor into its target slot; it is correct and always
granular, but is not guaranteed to emit the mathematically fewest
possible `ItemsMoved` events for an adversarial permutation (an
LIS-based scheme could do slightly better there). For the common
case this primitive targets — a peer append/remove/edit merged back
in — the existing relative order of untouched rows is preserved
as-is, so no moves are emitted at all.

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this model with the debug inspector under `name`. In
release builds (`!cfg(debug_assertions)`) this is a no-op
pass-through so call sites stay free of `#[cfg]` lines.

Idempotent on repeated calls — the latest registration wins.
The registration drops automatically when the last `ListModel`
handle is freed (the adapter the registry holds is `Weak`).
