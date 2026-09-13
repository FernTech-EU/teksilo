<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ActivateOn

Shared substrate for the data views' source-owned drag-and-drop + lazy
loading.

Centralizes the vocabulary the four data views (`ListView` / `TreeView` /
`TableView` / `TreeTableView`) share, so DnD validation (`can_accept`) and
the lazy placeholder are wired one way everywhere:

- `RowDragData` — the **public, generic** intra-app drag payload a row (or
  a whole selected set) emits. The receiving source distinguishes its OWN
  reorder (matching `ViewId`) from a foreign drop, and translates the
  origin's `rows` → its own key via `key_at`, so the source's `Key` type
  never leaks into the view. When the origin opted into export it also
  carries `items` (clones of the dragged `T`), so a foreign `DropTarget`,
  a different data view, or the OS can consume the drag.
- `DropIndicator` — what `paint` renders; `allowed == false` is the
  pre-commit forbidden affordance.
- `flat_insertion_target` — maps a flat insertion index to the
  `(target, position)` pair `can_accept` / `accept_drop` expect.
- `default_placeholder` — the skeleton for a `Loading` row.

## Builder methods at a glance

`items`, `into_items`, `is_export`, `len`, `is_empty`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/index.html)

## `pub enum ActivateOn`

How a data-view row/tile is *activated* (opened/committed) by pointer —
distinct from *selection*, which also moves on arrow-key navigation. Mirrors
the platform split other toolkits expose (Qt
`SH_ItemView_ActivateItemOnSingleClick`, GTK `activate-on-single-click`).
Enter/Space always activates regardless of this mode.

Pass to `ListView::activate_on`, `TreeView::activate_on`, etc.

```rust
pub enum ActivateOn { /* variants */ }
```

### Variants

- **`SingleClick`** — One primary click activates the row (KDE / web / Scrivener convention). Selection and activation happen on the same click.
- **`DoubleClick`** — A double primary click activates the row; the first click only selects it (Finder / Explorer / Qt and GTK default). This is the `Default`.

## `pub struct ViewId`

Opaque, kind-tagged, process-unique identity of a drag-capable data-view
instance. Used to tell a view's OWN reorder (`SameView`) from a foreign drop
on the receive side. Apps only ever compare two `ViewId`s for equality (e.g.
out of a received `RowDragData`); there is no public constructor, and the
value is stable for a view instance's lifetime, so it is safe to compare
even across windows (each mint is globally unique).

```rust
pub struct ViewId(ViewKind, usize);
```

## `pub enum DragTransferMode`

What the *origin* view does to its own rows once a drag is accepted by a
**foreign** target (a different `DropTarget` / view / the OS). Purely an
origin-side cleanup choice — the receiver is unaffected. A same-view reorder
is never a transfer, so this never applies to it.

```rust
pub enum DragTransferMode { /* variants */ }
```

### Variants

- **`Copy`** — Leave the origin rows in place (the dragged data is duplicated).
- **`Move`** — Remove the dragged rows from the origin once accepted elsewhere (or exported as an OS move). This is the `Default`.

## `pub struct RowDragData`

The public, generic drag payload every data-view row (or selected set)
emits. It occupies the single typed slot of a
`teksilo_core::drag_payload::DragPayload` and serves both audiences:

- the origin view's own erased classifier reads `source` +
  `rows` to recognise a same-view reorder;
- a **foreign** consumer (another view's custom `ListDataSource`, a
  `DropTarget::accept_typed::<RowDragData<T>>()`, or `on_rows_received`)
  reads `items`.

`items` is `Some` only when the origin view opted into export via
`.exportable(..)` (which requires `T: Clone`); a plain `.reorderable(true)`
drag carries `items == None` (nothing outside the origin could use it
anyway), so a reorder-only view is never accidentally droppable elsewhere.

```rust
pub struct RowDragData<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn items(&self) -> Option<&[T]>`

The dragged items, if this is an export drag (`.exportable(..)` was set
on the origin). `None` for a reorder-only drag.

#### `pub fn into_items(self) -> Option<Vec<T>>`

Consume the payload for its items (avoids cloning on the receive side).

#### `pub fn is_export(&self) -> bool`

Whether this drag carries exportable items — i.e. the origin opted into
`.exportable(..)`. A foreign receiver should gate on this (a reorder-only
payload has the same Rust type but carries nothing usable).

#### `pub fn len(&self) -> usize`

Number of dragged rows.

#### `pub fn is_empty(&self) -> bool`

Whether no rows are carried (never true for a real drag).

## `pub struct RowAnchor`

Active drag-drop feedback a tree data view paints itself: a between-rows
insertion line (Before/After) or a highlighted row (an into-container drop).

Shared by `TreeView` and `TreeTableView` so both render the same affordance
for the same source verdict.
A stable handle to a row in a data view.

Per-row event handlers (a chevron toggle, a click, an activation) are built
once and then live as long as the row widget does, so capturing the flat
index they were built at is fragile: expanding a branch above, applying a
filter, or sorting shifts every index below, and the stale handler would act
on whatever row moved into that slot.

A `RowAnchor` closes over the row's **source-owned identity** instead and
resolves the row's *current* position on demand. The key never surfaces in
the anchor's type — it is captured inside the resolver, so views stay
key-agnostic (`TreeSource` and
`ListSource` both erase it).

Sources without identity (a bare `ListModel`, or any source that leaves
`key_at` at its `None` default) get a fixed anchor that always reports the
index it was built with — no worse than capturing the index directly.

A bare `ListModel` has no identity to offer (a `Vec` row *is* its position),
so anchors over one are fixed. `SortFilterListModel` keys rows by their
**source index**, which no sort/filter reprojection renumbers — so anchors
over a projection do track their row across a filter change, which is the
flat fragility in practice. They can still mis-resolve inside the window
between an *upstream* insert/remove and the rebuild it schedules, since that
does renumber source indices; no worse than the captured index they replace.
The tree sources all carry real identity.

**Precondition: keys must be unique.** Resolution falls back to a lookup by
key, which returns the *first* match, so a source handing out duplicate keys
would silently redirect an anchor onto a different row — the very failure
this type exists to prevent.

```rust
pub struct RowAnchor { /* fields */ }
```

### Methods

#### `pub fn index(&self) -> Option<usize>`

The row's current flat index, or `None` if it no longer exists in the
source (it was deleted, or filtered away).

#### `pub fn is_live(&self) -> bool`

Whether the row still exists.
