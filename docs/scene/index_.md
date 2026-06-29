<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SpatialIndex

Spatial index for `Scene` items.

`GridHashIndex` is the only shipped implementation — a uniform grid
hash. The `SpatialIndex` trait is deliberately small — three mutating
operations (`insert`, `remove`, `query`) plus two read methods
(`contains`, `len`) — so an application that needs different behaviour
(e.g. an R-tree) can supply its own implementation in a one-line change
via `Scene::with_index`.

## Why grid hash first

- Cache-friendly: items in the same cell are stored contiguously.
- Insert / remove / move are amortised `O(k)` where `k` is the
  number of cells the item overlaps (typically 1–4 for items
  smaller than the cell size).
- `query(rect)` returns deduplicated candidates from the cells the
  rect overlaps; callers can narrow with a per-item AABB check.
- Pathological case (one giant item that spans hundreds of cells)
  is rare and easily worked around by raising `cell_size` for
  that scene. A custom `SpatialIndex` would handle non-uniform
  density better — an R-tree, say, for an editor with many
  overlapping items — but none ships; the trait is the place to
  add one.

Default `cell_size` is `DEFAULT_CELL_SIZE` (`256.0` logical pixels)
— large enough that typical card-sized items (~200 px) bucket into 1–4
cells and small enough that viewport queries (~800–1200 px) hit a
manageable fan-out.

## Example

```ignore
// ItemId values are obtained from Scene::add_item in real code;
// the example uses the crate-internal constructor for illustration.
use bastyde_scene::{GridHashIndex, SpatialIndex, ItemId};
use bastyde_canvas::Rect;

let mut index = GridHashIndex::default();
let id = ItemId(1); // in practice: returned by Scene::add_item
index.insert(id, Rect::new(10.0, 10.0, 80.0, 80.0));
assert!(index.contains(id));

let hits = index.query(Rect::new(0.0, 0.0, 100.0, 100.0));
assert!(hits.contains(&id));

index.remove(id);
assert!(index.is_empty());
```

## Builder methods at a glance

`cell_size`, `cell_count`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub const DEFAULT_CELL_SIZE`

Default cell size for `GridHashIndex` — 256 logical pixels.
Item-side typical 200 px cards bucket into 1–4 cells; viewport
queries (~800–1200 px) hit a small fan-out.

```rust
pub const DEFAULT_CELL_SIZE: f32 = 256.0;
```

## `pub struct GridHashIndex`

Uniform grid spatial hash. Each item is bucketed into every cell
its AABB overlaps; queries union all items from the cells the
query rect overlaps.

```rust
pub struct GridHashIndex { /* fields */ }
```

### Methods

#### `pub fn new(cell_size: f32) -> Self`

Create a grid with `cell_size` logical pixels per cell.
Clamped to a minimum of 1.0 to avoid pathological huge bucket
counts.

#### `pub fn cell_size(&self) -> f32`

The configured cell size in logical pixels.

#### `pub fn cell_count(&self) -> usize`

Number of cells currently storing at least one item. Useful
for diagnostics; not part of the public `SpatialIndex` trait.
