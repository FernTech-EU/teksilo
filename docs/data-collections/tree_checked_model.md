<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TreeCheckedModel

`TreeCheckedModel` — per-node checkbox state for a tree, with optional
descendant→ancestor tristate aggregation.

Companion to `crate::CheckedModel` for trees. Defaults to the standard
"Outlook folder selection" semantic: a parent's state is `Checked` if all
descendants are checked, `Unchecked` if none, `Indeterminate` otherwise;
toggling a parent cascades `Checked`/`Unchecked` down to all descendants.
Set the mode to `AggregateMode::None` to give every node independent
state instead. The model is a share-by-clone handle (`Rc<RefCell<…>>`
internally) — cloning produces a second view onto the same checkbox state.

External writes (e.g. a `Checkbox` widget bound to
`signal_for(node)` setting it directly) trigger the same
cascade-and-recompute pass as the model's own
`check`/`uncheck`/`toggle` methods, via per-node observers. A
re-entry guard prevents the cascade pass from re-firing
observers it triggers itself.

## Example

```rust
# use bastyde_data::{TreeModel, TreeCheckedModel, CheckState};
let tree = TreeModel::new();
let root = tree.insert_root(0, "root");
let child_a = tree.insert_child(root, 0, "a");
let child_b = tree.insert_child(root, 1, "b");

let model = TreeCheckedModel::new(tree);
// Pre-register signal chains before mutating.
let _ = (model.signal_for(root), model.signal_for(child_a), model.signal_for(child_b));

model.check(child_a);
assert_eq!(model.check_state(root), CheckState::Indeterminate);
model.check(child_b);
assert_eq!(model.check_state(root), CheckState::Checked);
```

## Limitation: tree-mutation desync

`signal_for(node)` and `bool_signal_for(node)` cache signals keyed
by `NodeId`. The cache is never invalidated. If the underlying
`TreeModel<T>` mutates (`remove`, `move_node`, etc.) the cached
entry for a removed `NodeId` lingers indefinitely:

- `checked_nodes()` may include a stale `NodeId` whose underlying
  tree node no longer exists. Callers that consume this list
  should validate each id against the current tree state before
  acting on it.
- `bool_signal_for` / `signal_for` for a removed node still return
  their cached signal handle. Setting it has no observable effect
  on the tree (the cascade walks `tree.children(node)` which is
  empty for a freed node).

This is an acceptable trade-off because `NodeId`s are not reused
by `TreeModel` (slotmap keys are versioned), so a stale id can
never alias a fresh node. If a future use case needs strict
invalidation on removal, subscribe to `TreeModel`'s change events
and clear the relevant entries. Tracked as out-of-scope for V1.

## Builder methods at a glance

`with_mode`, `aggregate_mode`, `set_aggregate_mode`, `signal_for`, `bool_signal_for`, `check_state`, `check`, `uncheck`, `toggle`, `checked_nodes`, `clear`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/tree_checked_model/index.html)

## `pub enum AggregateMode`

How a parent's `CheckState` relates to its descendants.

```rust
pub enum AggregateMode { /* variants */ }
```

### Variants

- **`None`** — Each node owns its state independently; parent states do not reflect their descendants and cascades do not occur.
- **`DescendantsDriveAncestors`** — All-checked → `Checked`; all-unchecked → `Unchecked`; mixed → `Indeterminate`. Toggling a parent cascades `Checked`/`Unchecked` to all descendants and recomputes every ancestor. This is the default and corresponds to the "Outlook folder selection" tristate pattern.

## `pub struct TreeCheckedModel`

Per-node checkbox state for a `TreeModel<T>`, with optional
descendant→ancestor tristate aggregation.

See the `module documentation` for the full semantics and limitations.
Clone to share the same checkbox state between multiple call sites.

```rust
pub struct TreeCheckedModel<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn new(tree: TreeModel<T>) -> Self`

Create a new model wrapping `tree` with the default
`AggregateMode::DescendantsDriveAncestors` cascade behaviour.

#### `pub fn with_mode(tree: TreeModel<T>, mode: AggregateMode) -> Self`

Create a new model wrapping `tree` with an explicit `AggregateMode`.

#### `pub fn aggregate_mode(&self) -> AggregateMode`

Returns the current `AggregateMode` controlling cascade behaviour.

#### `pub fn set_aggregate_mode(&self, mode: AggregateMode)`

Change the cascade behaviour; takes effect on the next write to any node's signal.

#### `pub fn signal_for(&self, node: NodeId) -> Signal<CheckState>`

Writable `Signal<CheckState>` for `node`. Cached: repeat calls
return the same root. External writes (e.g. from a `Checkbox`)
trigger the configured aggregation pass. The cascade observer is
wired **idempotently** — including for a signal first materialised by
a cascade (`write_state`) before its own `signal_for` was ever called
(a lazily/virtualized-realised row) — so a later external write to it
still cascades.

#### `pub fn bool_signal_for(&self, node: NodeId) -> Signal<bool>`

Two-state projection of `signal_for` for callers that want
to bind a leaf's check state to a `Signal<bool>`-shaped widget
(e.g. a non-tristate `Checkbox`). The returned signal is
**writable**: setting it to `true` calls `check(node)` (which
runs the configured cascade), `false` calls `uncheck(node)`.
Writes from the model side propagate back into the bool signal
(`Checked → true`, anything else → `false`). Cached: repeat
calls return the same handle.

For leaves under `AggregateMode::DescendantsDriveAncestors`
this is the right pairing — a leaf's state is two-state by
nature, and the model's ancestor recompute still runs. For
branches you typically want the tristate `signal_for` so
`Indeterminate` is visible.

#### `pub fn check_state(&self, node: NodeId) -> CheckState`

Returns the current `CheckState` for `node` (defaults to `Unchecked`
if the node's signal has never been written or read).

#### `pub fn check(&self, node: NodeId)`

Set `node` to `CheckState::Checked`, triggering the configured cascade and
ancestor recompute; notifies observers of every affected node's signal.

#### `pub fn uncheck(&self, node: NodeId)`

Set `node` to `CheckState::Unchecked`, triggering the configured cascade and
ancestor recompute; notifies observers of every affected node's signal.

#### `pub fn toggle(&self, node: NodeId)`

Toggle `node`'s check state: under `DescendantsDriveAncestors` a leaf
cycles two-state (`Unchecked` ↔ `Checked`); a branch or `AggregateMode::None`
cycles the full tristate sequence via `CheckState::next_tristate`.

#### `pub fn checked_nodes(&self) -> Vec<NodeId>`

Returns all `NodeId`s whose current state is exactly `CheckState::Checked`.

Note: may include stale ids if the underlying tree has been mutated since
the signals were first registered — see the module-level limitation note.

#### `pub fn clear(&self)`

Reset all known nodes to `CheckState::Unchecked` and notify observers.

Writes every tracked node directly via the internal `write_state`
helper (per-node cascade-suppressed, like the recompute pass) instead of going
through `check`/`uncheck`'s normal `signal_for(..).set(..)` path —
the latter would, for every currently-checked node, cascade the
write down its entire descendant subtree and recompute every
ancestor up to the root, all *before* the outer loop even reaches
those same nodes. Since every tracked node ends up `Unchecked` here,
there is nothing left to aggregate: "all children unchecked" is
already the correct parent state, so skipping the cascade and
ancestor recompute entirely still leaves every node's state
consistent — one direct write per tracked node instead of a
cascade+recompute pass per *checked* one.
