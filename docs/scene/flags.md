<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ItemFlags

Per-item behavior flags.

## Public functions

### `ItemFlags`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`from_bits(bits: u32)`](#itemflags-from_bits) |
| | **Builder methods** |
| `Self` | [`with(flag: Self)`](#itemflags-with) |
| `Self` | [`without(flag: Self)`](#itemflags-without) |
| | **Methods** |
| `bool` | [`contains(other: Self)`](#itemflags-contains) |
| `bool` | [`intersects(other: Self)`](#itemflags-intersects) |
|  | [`set(flag: Self, on: bool)`](#itemflags-set) |
| `u32` | [`bits()`](#itemflags-bits) |
| | **Constants and types** |
| `Self` | [`NONE`](#itemflags-none) |
| `Self` | [`IS_VISIBLE`](#itemflags-is_visible) |
| `Self` | [`IS_ENABLED`](#itemflags-is_enabled) |
| `Self` | [`IS_DRAGGABLE`](#itemflags-is_draggable) |
| `Self` | [`IS_SELECTABLE`](#itemflags-is_selectable) |
| `Self` | [`IS_FOCUSABLE`](#itemflags-is_focusable) |
| `Self` | [`ACCEPTS_HOVER`](#itemflags-accepts_hover) |
| `Self` | [`CLIPS_TO_SHAPE`](#itemflags-clips_to_shape) |
| `Self` | [`CLIPS_CHILDREN_TO_SHAPE`](#itemflags-clips_children_to_shape) |
| `Self` | [`IGNORES_TRANSFORMATIONS`](#itemflags-ignores_transformations) |
| `Self` | [`HAS_NO_CONTENTS`](#itemflags-has_no_contents) |
| `Self` | [`IS_RESIZABLE`](#itemflags-is_resizable) |
| `Self` | [`IS_ROTATABLE`](#itemflags-is_rotatable) |
| `Self` | [`ASPECT_LOCKED`](#itemflags-aspect_locked) |

## Detailed description

`ItemFlags` is a bitset packed into a `u32`. Each flag opts an
item into a behavior — drag-to-move participation, hit-test
response, rendering visibility, transform inheritance — that
the Scene and SceneView consult at the relevant pipeline stage.

Defaults: `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`. An item
constructed via the standard built-in builders gets these
defaults; setters layer additional flags on top.

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="itemflags"></a>

## `pub struct ItemFlags`

A bitset of per-item behavior flags.

Use `ItemFlags::default` for the standard "interactive,
visible, selectable" baseline. Compose flags with `|` and toggle
them with `ItemFlags::set` / `ItemFlags::contains`.

```rust
pub struct ItemFlags(u32);
```

### Methods

<a id="itemflags-none"></a>

#### `pub const NONE: Self = Self(0);`

Empty bitset — no flags set.

<a id="itemflags-is_visible"></a>

#### `pub const IS_VISIBLE: Self = Self(1 << 0);`

Item paints and is hit-tested. Default on. Clearing this is
the equivalent of Qt's `setVisible(false)` — the item is
neither painted nor hit-tested. Children of an invisible
item are also effectively invisible.

<a id="itemflags-is_enabled"></a>

#### `pub const IS_ENABLED: Self = Self(1 << 1);`

Item dispatches pointer events. Default on. Disabled items
are still painted but pass clicks through to items beneath.

<a id="itemflags-is_draggable"></a>

#### `pub const IS_DRAGGABLE: Self = Self(1 << 2);`

Item participates in drag-to-move. Default off.

<a id="itemflags-is_selectable"></a>

#### `pub const IS_SELECTABLE: Self = Self(1 << 3);`

Item is included in marquee box-select results. Default on.

<a id="itemflags-is_focusable"></a>

#### `pub const IS_FOCUSABLE: Self = Self(1 << 4);`

Item can take keyboard focus. Default off. Declared only: the
built-in traversal (`SceneView::focus_in_direction`) walks every
item in insertion order, and a `focus_order` callback is handed
the whole `Scene` and filters for itself — nothing reads this bit.

<a id="itemflags-accepts_hover"></a>

#### `pub const ACCEPTS_HOVER: Self = Self(1 << 5);`

Item dispatches hover events (Qt `setAcceptHoverEvents`).
Item dispatches hover events (Qt `setAcceptHoverEvents`).
Default off. Declared only: the view dispatches hover from the
presence of a `SceneItemHandlerSet::on_hover` callback, and
nothing sets or reads this bit.

<a id="itemflags-clips_to_shape"></a>

#### `pub const CLIPS_TO_SHAPE: Self = Self(1 << 6);`

Item's paint output is clipped to its `local_bounds`.
Default off.

<a id="itemflags-clips_children_to_shape"></a>

#### `pub const CLIPS_CHILDREN_TO_SHAPE: Self = Self(1 << 7);`

Children are clipped to this item's `local_bounds`. Default
off; mirrors Qt's `ItemClipsChildrenToShape`.

<a id="itemflags-ignores_transformations"></a>

#### `pub const IGNORES_TRANSFORMATIONS: Self = Self(1 << 8);`

Item paints and hit-tests at a fixed pixel size, independent
of the view's zoom and rotation. Its anchor (the item's
parent-relative scene point) is projected through the view
transform like any other point, so the visible position
follows pan/zoom and tracks the underlying scene data —
but the item itself does not grow with zoom or rotate with
the view. Mirrors Qt's `ItemIgnoresTransformations`.
Annotation pins for graph editors, fixed-pixel-size badges
over moving content, chart axis labels. Default off.

<a id="itemflags-has_no_contents"></a>

#### `pub const HAS_NO_CONTENTS: Self = Self(1 << 9);`

Item has nothing to paint — the paint walk skips it
entirely. Pure logical-only containers (used for AT
grouping or hit-test routing) set this. Default off.

<a id="itemflags-is_resizable"></a>

#### `pub const IS_RESIZABLE: Self = Self(1 << 11);`

Item offers **resize** handles to a selection transform controller.
Default off, like `IS_DRAGGABLE`.

A resize writes the item's `local_bounds`, so the item reflows into the
new box — a heavyweight card relayouts, a `RectItem` redraws at the new
size, a `PathItem` fits its geometry to it. Nothing is scaled visually
and then baked.

<a id="itemflags-is_rotatable"></a>

#### `pub const IS_ROTATABLE: Self = Self(1 << 12);`

Item offers a **rotate** handle to a selection transform controller.
Default off.

Honoured for the lightweight tier only. A heavyweight entry carrying it
is refused, because `place_children` sizes a card from the AABB of its
transformed bounds: a rotation there inflates the layout box and rotates
nothing. See `SceneView::transform_controller`.

<a id="itemflags-aspect_locked"></a>

#### `pub const ASPECT_LOCKED: Self = Self(1 << 13);`

A resize of this item keeps its aspect ratio whatever the controller's
`keep_ratio` setting says. Default off.

<a id="itemflags-contains"></a>

#### `pub const fn contains(&self, other: Self) -> bool`

Whether the bitset contains every flag in `other`.

<a id="itemflags-intersects"></a>

#### `pub const fn intersects(&self, other: Self) -> bool`

Whether the bitset shares any flags with `other`.

<a id="itemflags-set"></a>

#### `pub fn set(&mut self, flag: Self, on: bool)`

Set (when `on`) or clear (when `!on`) the bits in `flag`.

<a id="itemflags-with"></a>

#### `pub const fn with(self, flag: Self) -> Self`

Set the bits in `flag`, returning the new bitset.

<a id="itemflags-without"></a>

#### `pub const fn without(self, flag: Self) -> Self`

Clear the bits in `flag`, returning the new bitset.

<a id="itemflags-bits"></a>

#### `pub const fn bits(self) -> u32`

Raw `u32` bits (debug / serialization).

<a id="itemflags-from_bits"></a>

#### `pub const fn from_bits(bits: u32) -> Self`

Construct from raw bits.
