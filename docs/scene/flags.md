<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ItemFlags

Per-item behavior flags.

`ItemFlags` is a bitset packed into a `u32`. Each flag opts an
item into a behavior — drag-to-move participation, hit-test
response, rendering visibility, transform inheritance — that
the Scene and SceneView consult at the relevant pipeline stage.

Defaults: `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`. An item
constructed via the standard built-in builders gets these
defaults; setters layer additional flags on top.

## Builder methods at a glance

`NONE`, `IS_VISIBLE`, `IS_ENABLED`, `IS_DRAGGABLE`, `IS_SELECTABLE`, `IS_FOCUSABLE`, `ACCEPTS_HOVER`, `CLIPS_TO_SHAPE`, `CLIPS_CHILDREN_TO_SHAPE`, `IGNORES_TRANSFORMATIONS`, `HAS_NO_CONTENTS`, `NEGATIVE_Z_BEHIND_PARENT`, `contains`, `intersects`, `set`, `with`, `without`, `bits`, `from_bits`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct ItemFlags`

A bitset of per-item behavior flags.

Use `ItemFlags::default` for the standard "interactive,
visible, selectable" baseline. Compose flags with `|` and toggle
them with `ItemFlags::set` / `ItemFlags::contains`.

```rust
pub struct ItemFlags(u32);
```

### Methods

#### `pub const NONE: Self = Self(0);`

Empty bitset — no flags set.

#### `pub const IS_VISIBLE: Self = Self(1 << 0);`

Item paints and is hit-tested. Default on. Clearing this is
the equivalent of Qt's `setVisible(false)` — the item is
neither painted nor hit-tested. Children of an invisible
item are also effectively invisible.

#### `pub const IS_ENABLED: Self = Self(1 << 1);`

Item dispatches pointer events. Default on. Disabled items
are still painted but pass clicks through to items beneath.

#### `pub const IS_DRAGGABLE: Self = Self(1 << 2);`

Item participates in drag-to-move. Default off.

#### `pub const IS_SELECTABLE: Self = Self(1 << 3);`

Item is included in marquee box-select results. Default on.

#### `pub const IS_FOCUSABLE: Self = Self(1 << 4);`

Item can take keyboard focus. Default off; the focus_order
callback considers only items with this flag set.

#### `pub const ACCEPTS_HOVER: Self = Self(1 << 5);`

Item dispatches hover events (Qt `setAcceptHoverEvents`).
Default off; hover handlers wired via `ItemBuilder::on_hover`
flip this on automatically.

#### `pub const CLIPS_TO_SHAPE: Self = Self(1 << 6);`

Item's paint output is clipped to its `local_bounds`.
Default off.

#### `pub const CLIPS_CHILDREN_TO_SHAPE: Self = Self(1 << 7);`

Children are clipped to this item's `local_bounds`. Default
off; mirrors Qt's `ItemClipsChildrenToShape`.

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

#### `pub const HAS_NO_CONTENTS: Self = Self(1 << 9);`

Item has nothing to paint — the paint walk skips it
entirely. Pure logical-only containers (used for AT
grouping or hit-test routing) set this. Default off.

#### `pub const NEGATIVE_Z_BEHIND_PARENT: Self = Self(1 << 10);`

Children with `z < 0` paint **behind** this item rather
than in front. Mirrors Qt's `ItemNegativeZStacksBehindParent`.
Default off.

#### `pub const fn contains(&self, other: Self) -> bool`

Whether the bitset contains every flag in `other`.

#### `pub const fn intersects(&self, other: Self) -> bool`

Whether the bitset shares any flags with `other`.

#### `pub fn set(&mut self, flag: Self, on: bool)`

Set (when `on`) or clear (when `!on`) the bits in `flag`.

#### `pub const fn with(self, flag: Self) -> Self`

Set the bits in `flag`, returning the new bitset.

#### `pub const fn without(self, flag: Self) -> Self`

Clear the bits in `flag`, returning the new bitset.

#### `pub const fn bits(self) -> u32`

Raw `u32` bits (debug / serialization).

#### `pub const fn from_bits(bits: u32) -> Self`

Construct from raw bits.
