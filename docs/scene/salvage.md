<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ItemA11yDecorations

The **owning salvage** door: everything `Scene::remove`
destroys, moved out instead of dropped.

# Why a second channel exists at all

`ItemChange` travels through a `Signal`, and
`Signal<T>` requires `T: Clone` (it snapshots the value before fanning out).
A `SceneItem` is a `Box<dyn SceneItem>` with no `clone_item` and no
downcast, and a heavyweight entry holds a `Box<dyn Widget>`. Neither can be
cloned, so **the removed entry can never ride the notification channel** —
not as a design preference but as a type fact. It has to be *moved*, which
means a door that hands ownership over.

That door is `Scene::take` (the caller keeps the
salvage) and the `SceneTransactionRecord`
delivered to the edit sink (the framework hands it over). Both produce
`RemovedItem`s; `Scene::restore` puts one back.

# What a removal used to destroy

`Scene::remove` drops, in this order: the entry (its item or widget box, its
handlers, its geometry, its flags), then the item's slice of the logical
accessibility tree — AT parent, AT relations, live-region status, landmark
role, rotor categories — then its magnets. Only *after* all of that does it
announce `ItemChange::Removed`. So an app that reconstructed the item from
the event alone would restore the pixels and silently lose every one of
those. On a crate whose differentiator is per-item accessibility, that is
the single strongest reason this door exists.

# The edges a naive salvage misses

The logical AT tree is a *graph*, and a removal cuts edges on **both**
sides:

* the removed item's own AT parent (`removed → parent`), and
* every **surviving** node that was AT-parented *under* the removed item
  (`survivor → removed`), which `remove` also drops — re-rooting those
  survivors at the view root.

Recording only the first would make `restore` silently fail to re-adopt the
survivors, i.e. fail at exactly the edge case the salvage is sold on. So
`ItemA11yDecorations` carries both directions, and the same applies to
relations, which `remove` retains on either endpoint.

## Builder methods at a glance

`is_empty`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub struct ItemA11yDecorations`

The per-item slice of the **logical accessibility tree**, harvested by
`Scene::take` and re-attached by
`Scene::restore`.

Both directions of every edge, because a removal cuts both — see the module
docs. A restore re-attaches only the edges whose *other* endpoint is still
alive; an edge to something that has since been removed is dropped, because
re-inserting it would name a node the AT walker cannot resolve.

```rust
pub struct ItemA11yDecorations { /* fields */ }
```

### Methods

#### `pub fn is_empty(&self) -> bool`

Whether this item carried any logical-AT decoration at all. A restore
of an undecorated item touches none of the scene's AT maps.

## `pub struct RemovedItem`

One item lifted whole out of a `Scene`: its entry, its
magnets and its logical-AT decorations, still owned and still carrying its
original `ItemId`.

Opaque on purpose. It wraps the crate-private `SceneEntry` **whole** rather
than copying its fields out, so a future field added to the entry cannot be
missed by `Scene::restore` — the same argument the
single-construction-site rule makes for insertion.

# Ordering

`Scene::take` returns a subtree deepest-first, the
order `remove` already announces in.
`Scene::restore_all` reverses it for you;
restoring by hand means roots first, or a child hits
`RestoreError::MissingParent`.

```rust
pub struct RemovedItem { /* fields */ }
```

### Methods

#### `pub fn id(&self) -> ItemId`

The id this item had, and the id
`Scene::restore` puts it back at.

#### `pub fn local_pos(&self) -> Point`

Position in the parent's coordinate frame.

#### `pub fn local_bounds(&self) -> Rect`

AABB in local coordinates.

#### `pub fn transform(&self) -> Transform2D`

Local→parent transform.

#### `pub fn z(&self) -> f32`

Paint z-order.

#### `pub fn layer(&self) -> SceneLayer`

Lightweight paint band.

#### `pub fn parent(&self) -> Option<ItemId>`

Logical parent at the moment of removal. A restore needs this item
present, or it fails with `RestoreError::MissingParent`.

#### `pub fn detach(&mut self)`

Forget the recorded parent, so this salvage restores at **root** level.

The door `RestoreError::MissingParent` points at, and the one a
move *between* scenes needs: the parent a salvage names is still in the
scene it came from, so restoring a parented item into a second scene is
refused unless its parent went first — or unless the caller says, here,
that it should arrive unparented.

Nothing else changes: geometry is untouched, so an item that was placed
in its parent's frame keeps those numbers and now reads them as scene
coordinates. Use
`Scene::reparent_keeping_scene_pos`
after the restore when the item should not visibly move.

#### `pub fn children(&self) -> &[ItemId]`

Direct children at the moment of removal — all of them removed with it,
and all of them present in the same `Scene::take`
result.

#### `pub fn flags(&self) -> ItemFlags`

Behaviour flags.

#### `pub fn opacity(&self) -> f32`

Local opacity multiplier.

#### `pub fn is_widget(&self) -> bool`

Whether this was a heavyweight `Widget` entry rather than a lightweight
`SceneItem` one.

#### `pub fn item(&self) -> Option<&dyn SceneItem>`

The lightweight item, if this was one. `None` for widget entries.

#### `pub fn into_item(self) -> Option<Box<dyn SceneItem>>`

Consume the salvage for its lightweight item box — cut-to-clipboard.
`None` for widget entries (and the salvage is dropped either way).

#### `pub fn payload(&self) -> Option<std::rc::Rc<dyn std::any::Any>>`

The type-erased payload of a multi-view (`Delegated`) heavyweight
entry, which is what lets a restored card be rebuilt by every view's
delegate. `None` for a lightweight item and for a single-view
(`Scene::add_widget`) entry.

#### `pub fn widget_instance_present(&self) -> bool`

Whether a **single-view** (`Scene::add_widget`) entry still carries its
one `Box<dyn Widget>`.

`true` for every lightweight item and for every multi-view
(`add_widget_item`) entry, because neither depends on a one-shot box.

This is a **fact about the salvage, not a verdict on restorability**,
and the distinction matters: a `Once` widget already drained by a
mounted view restores perfectly well when the take and the restore
happen inside one build cycle (the view's orphan reap keys on the
scene's live heavyweight set, which the restore has already put the id
back into, so the arena instance is never destroyed). Refusing such a
restore would refuse one that works — and would refuse the whole entry,
geometry and accessibility included, over a widget instance.

So `Scene::restore` always accepts, and this
reports what an app needs to decide for itself. When `false` **and** the
view has already reaped the instance, the restored entry is a
heavyweight slot with nothing to materialise — visible to no view. The
rule that avoids it entirely is the same one multi-view content already
follows: content that must survive an undo goes in through
`SceneModel::add_widget_item`,
whose payload every view can rebuild from.

#### `pub fn a11y(&self) -> &ItemA11yDecorations`

The logical-AT decorations this item carried — both directions of every
edge. See `ItemA11yDecorations`.

#### `pub fn magnet_count(&self) -> usize`

How many magnets were attached to this item. Their ids are preserved by
a restore, so a consumer keying connections on
`MagnetId` keeps them.

## `pub enum RestoreError`

Why `Scene::restore` could not put a salvage back.

`#[non_exhaustive]`: this crate has out-of-tree consumers and the restore
path may learn to refuse something new.

```rust
pub enum RestoreError { /* variants */ }
```

### Variants

- **`IdAlreadyLive`** — An entry with this id is already in the scene. Restoring over it would alias two items onto one identity.  **Defensive.** `Scene::take` retires the id, so a second salvage for it cannot exist while the first is held, and the restore that succeeds consumes the one that does — which makes this unreachable through the public API today. It stays because it is the only thing standing between a future door that breaks that invariant and a corrupted `entry_index`, and refusing is better than aliasing.
- **`MissingParent`** — The parent the item was removed from is not in the scene. Restore roots-first — `Scene::restore_all` orders a whole `take` result for you — or clear the recorded parent with `RemovedItem::detach` before restoring, which is what a move into a *different* scene normally wants.  The salvage is consumed by the refused call, so a caller that may hit this checks `parent()` (or calls `detach()`) **before** handing it over.

## `pub struct ReplaceRejected`

A refused `Scene::replace_item`: why, and the
item it did not take.

The item comes back rather than being dropped, because a refused write must
not eat the caller's value. A `Box<dyn SceneItem>` can be expensive to build
and cannot be cloned, so a caller recovering from a precondition violation
would otherwise have nothing left to insert instead — and the row it was
reconciling would silently vanish.

`#[non_exhaustive]`: see `RestoreError`. The crate only ever hands this
*out*, so there is nothing to construct — recover the item with
`let ReplaceRejected { item, .. } = rejected;`.

```rust
pub struct ReplaceRejected { /* fields */ }
```

## `pub enum ReplaceItemError`

Why `Scene::replace_item` could not swap an
item box. Travels inside a `ReplaceRejected`, which also carries the item.

`#[non_exhaustive]`: see `RestoreError`.

```rust
pub enum ReplaceItemError { /* variants */ }
```

### Variants

- **`UnknownItem`** — No entry with this id.
- **`NotLightweight`** — The entry is a heavyweight `Widget`, which holds no `SceneItem` box. Replace its *data* with `SceneModel::set_payload` instead.
