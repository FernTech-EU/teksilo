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

📖 [Full rustdoc API for this module](../api/bastyde_widgets/index.html)

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
`bastyde_core::drag_payload::DragPayload` and serves both audiences:

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
