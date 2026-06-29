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

`from_vec`, `len`, `is_empty`, `with_item`, `push`, `insert`, `remove`, `set`, `move_item`, `replace_all`, `clear`, `observe_changes`, `debug_named`

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

#### `pub fn replace_all(&self, items: Vec<T>)`

Replace the entire list contents.

#### `pub fn clear(&self)`

Remove all items from the list.

#### `pub fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle`

Register an observer that is called on every mutation.
Returns an `ObserverHandle` — dropping it removes the callback.

#### `pub fn debug_named(self, _name: impl Into<String>) -> Self`

Register this model with the debug inspector under `name`. In
release builds (`!cfg(debug_assertions)`) this is a no-op
pass-through so call sites stay free of `#[cfg]` lines.

Idempotent on repeated calls — the latest registration wins.
The registration drops automatically when the last `ListModel`
handle is freed (the adapter the registry holds is `Weak`).
