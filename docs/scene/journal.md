<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# TxnId

The **reversible-mutation seam**: what the scene tells a data layer so that
layer can reverse an edit.

# What this is not

There is no stack here, no history, no command list and no `undo()`. Undo
belongs to the data layer, and the framework's job stops at handing that
layer a record complete enough to invert. Everything below is *mechanism*;
every policy question — what counts as one undo step, whether a cancelled
gesture clears the redo stack, how long history is kept — is answered on the
other side of the seam.

"Complete enough to invert" is a claim with teeth, so it is worth stating
exactly: **every `SceneEdit::Change` in a record carries both sides of
what it replaced**, and a `SceneEdit::Removed` carries the removed entry
itself. Nothing else is in there. Two notifications the scene emits are
therefore deliberately *not* recorded, and
`ItemChange::is_edit` is the test that says
which:

| not recorded | why |
| --- | --- |
| `ItemChange::VisibilityChanged` | a convenience beside the `FlagsChanged` that describes the same mutation — recording both would make hiding a card two edits, and only through one of the two flag doors |
| `ItemChange::HandlersChanged` with no `replaced` | `Scene::handlers_mut` hands out a `&mut` and is told nothing about what the caller does with it, so it can describe the change to no one. `Scene::set_item_handlers` knows both sides and is recorded. |

Both still ride the notification channel, because a view or a cache does
need to hear about them. The distinction is between *telling* and
*describing*.

# Two channels, because the types force it

`Scene::item_change_signal` carries a
`SceneChange` — an `ItemChange` wrapped in an envelope that says *which
transaction* it belongs to, *whose* change it was, *whether it counts as
history*, and *whether it is a per-frame artefact*. That channel is cheap,
synchronous and `Clone`, and the view reconciles from it.

It cannot carry ownership. `Signal<T>` snapshots its value before fanning
out, so `T: Clone`, and a removed entry holds a `Box<dyn SceneItem>` or a
`Box<dyn Widget>` — neither clonable. So a removal's *contents* travel a
second, owning channel: the `SceneTransactionRecord` delivered to the edit
sink. See `crate::salvage` for why that matters and what it carries.

# A transaction is a scope, and the scope is the write scope

Every `SceneModel` mutator already runs inside a write scope (that is how
the change fan-out escapes the `RefCell` borrow), and
`SceneWriteGuard` opens one around a whole block.
A transaction is the same boundary: one scope, one `TxnId`. So a subtree
`remove` is one transaction with N edits without anything being wrapped by
hand, and a block of edits under a write guard is one transaction because it
is one scope.

`SceneTransaction` sits *above* that: it holds the scope open across
several mutator calls and stamps them with a chosen
`ChangeSource`/`HistoryMode`. Nesting **joins** — an inner transaction
adds no boundary and the outer stamp wins — so an observer that opens its
own transaction inside a framework-opened gesture cannot split that gesture
in two.

# Where the sink runs, and why its own writes are journaled

The sink is invoked with **no borrow on the scene**, which is what lets it
read the scene and write it back. A sink that writes is not a corner case —
validation, clamping and mirroring all do it — and if those writes were
invisible the record the app holds would say `old → new` while the scene sat
at something else, and a redo would replay the wrong value. So a sink write
opens its own transaction like any other, its record queues behind the one
being delivered, and the same delivery loop hands it over on a later round.
The sink is never re-entered while it is running; it is also never taken out
of its slot, so a panicking sink does not vanish.

## Builder methods at a glance

`as_u64`, `is_none`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub struct TxnId`

Process-unique id for one transaction.

Rides on every `SceneChange` emitted inside it, so a consumer watching the
cheap notification channel can group a fan-out that arrived as N separate
notifications — and match it to the owning `SceneTransactionRecord` when
one is delivered.
Ids count up from 1; `TxnId(0)` is the "no transaction yet" sentinel the
change signal is seeded with and no scene mutation can ever carry.

```rust
pub struct TxnId(u64);
```

### Methods

#### `pub fn as_u64(self) -> u64`

Raw value, for a consumer keying its own map on a transaction.

#### `pub fn is_none(self) -> bool`

Whether this is the sentinel the change and transaction signals hold
before anything has been committed. Never `true` for a transaction the
scene actually opened.

## `pub enum ChangeSource`

*Whose* change this is.

Ambient on the open transaction rather than a parameter on every mutator:
widening two dozen public signatures to thread it would be a worse API and
would still miss the framework's own gesture code, which is the one caller
an app cannot reach.

The framework opens `ChangeSource::User` transactions around exactly the
places it writes the model on the user's behalf — the item-drag commit, the
selection-transform commit and the `Alt`+arrow nudge. Without that, an app
could not tell a finished drag from a programmatic move: both arrive as a
bare `LocalPosChanged`.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum ChangeSource { /* variants */ }
```

### Variants

- **`User`** — A direct user interaction — a drag, a nudge, a transform handle.
- **`Programmatic`** — The app moved the scene itself: a document load, a layout pass, a data re-source, an import, an animation tick. The default, because a mutation nobody claimed is not a user's.
- **`Remote`** — Applied on behalf of a peer — collaboration, replay, a data-layer redo. A consumer re-broadcasting or recording history is expected to skip these; the framework does not interpret it.

## `pub enum HistoryMode`

What a history *above* the scene should do with this transaction.

Three-valued, matching what the editors that have solved this converged on
(tldraw's `history: 'record' | 'record-preserveRedoStack' | 'ignore'`). The
framework never interprets it. It carries it faithfully and nothing else —
which is the whole point: deciding what counts as an undo step is the data
layer's job, and the scene's job is to make the decision expressible.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum HistoryMode { /* variants */ }
```

### Variants

- **`Record`** — A document edit: push an entry, clear redo. The default.
- **`RecordPreserveRedo`** — Push an entry but leave the redo stack alone. For a change that is undoable without invalidating a redo branch.
- **`Ignore`** — Do not push. Views still reconcile; history does not move.

## `pub enum TxnOutcome`

How a transaction finished.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum TxnOutcome { /* variants */ }
```

### Variants

- **`Committed`** — The normal ending: the guard dropped, or the write scope closed.
- **`Abandoned`** — `SceneTransaction::abandon` — a cancelled interaction.  The scene is **not** rolled back. The framework owns no inverse-application routine and will not grow one, because applying an inverse is the first 80 % of an undo stack. The record is delivered so the consumer can revert from the `old` values it was handed, without pushing anything a redo could replay.

## `pub struct SceneChange`

One `ItemChange` plus the context the notification channel needs in order
to be usable as a change feed rather than just a repaint trigger.

This is what
`Scene::item_change_signal` carries.

`#[non_exhaustive]`: consumers read it, the scene constructs it.

```rust
pub struct SceneChange { /* fields */ }
```

### Methods

#### `pub fn id(&self) -> ItemId`

The item this change is about.

## `pub enum Salvage`

The ownership an `ItemChange` cannot carry, attached to the removal that
produced it.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum Salvage { /* variants */ }
```

### Variants

- **`Owned`** — The framework owns the removed entry and is handing it over. Feed it back to `Scene::restore` to undo the removal.
- **`TakenByCaller`** — The caller used `Scene::take` and already holds the salvage, so the record carries only the fact of the removal.

## `pub enum SceneEdit`

One edit inside a `SceneTransactionRecord`, in application order.

Invert a transaction by walking `edits` in **reverse** and applying each
`old`; a `Removed` inverts to
`Scene::restore`.

Only a removal needs an owning variant. Every other mutator either carries
its own `old` in the `ItemChange` or hands the replaced value straight
back to its caller —
`Scene::replace_item` returns the box it
swapped out, so the app that called it already owns what it would need to
put back.

A `Change` here always satisfies
`ItemChange::is_edit`: the scene's derived
notifications never reach a record. See the module header for the three
and why.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum SceneEdit { /* variants */ }
```

### Variants

- **`Change`** — Any non-removal change, verbatim from the notification channel.
- **`Removed`** — A removal, with what it destroyed. Descendants come before the named root, the order `remove` announces in.

## `pub struct SceneTransactionRecord`

One committed transaction, delivered to the edit sink with the scene
unborrowed.

Owning — it carries the boxes a removal would otherwise drop — and therefore
cannot travel through a `Signal`. That is the whole reason the seam has two
channels; see the module docs.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub struct SceneTransactionRecord { /* fields */ }
```

### Methods

#### `pub fn is_empty(&self) -> bool`

Whether this transaction changed nothing. An empty record is never
delivered to the sink — a mutator that early-returned on an unchanged
value has nothing to reverse.
