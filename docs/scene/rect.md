<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# RectItem

`RectItem` — filled / stroked rectangle in local item coords.

`RectItem` is the simplest and cheapest lightweight scene item: a rectangle
in local item coordinates with an optional fill and/or stroke. It uses the
default AABB hit-test (exact for a rectangle) and has zero arena overhead.

Like all lightweight items, `RectItem` is constructed with its geometry
relative to a local origin (`Rect::new(0.0, 0.0, w, h)`) and placed in
the scene by `Scene::add_item(item, scene_pos)`, where `scene_pos` becomes
the item's anchor in scene coordinates.

## When to use

Use `RectItem` for background tiles, card backgrounds, selection highlights,
grid cells, or any rectangular decoration in the lightweight tier. For
arbitrary shapes, use `PathItem`; for interactive content needing focus
or event handlers, embed a full widget with `Scene::add_widget`.

## Example

```ignore
use bastyde_scene::{SceneModel, RectItem};
use bastyde_canvas::{Point, Rect};
use bastyde_tokens::Color;
use bastyde_i18n::lit;

let model = SceneModel::new();

let item = RectItem::new(Rect::new(0.0, 0.0, 120.0, 80.0))
    .fill(Color::new(0.9, 0.95, 1.0, 1.0))
    .stroke_cosmetic(Color::new(0.6, 0.7, 0.85, 1.0), 1.0)
    .label(lit!("Card background"))
    .draggable(true);

model.add_item(item, Point::new(40.0, 40.0));
```

## Builder methods at a glance

`fill`, `stroke`, `stroke_cosmetic`, `label`, `draggable`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct RectItem`

A rectangle with optional fill and stroke, in local item coordinates.

Construct with `RectItem::new(Rect::new(0.0, 0.0, w, h))` and place
in the scene via `Scene::add_item(rect, local_pos)`.

```rust
pub struct RectItem { /* fields */ }
```

### Methods

#### `pub fn new(local_bounds: Rect) -> Self`

A rectangle of the given size in local item coordinates. The
passed `local_bounds` is stored verbatim — typically
`Rect::new(0.0, 0.0, w, h)`. No fill, no stroke — set at least
one or the item is invisible.

#### `pub fn fill(mut self, color: Color) -> Self`

Fill color.

#### `pub fn stroke(mut self, color: Color, width: f32) -> Self`

Stroke color and width in **scene-coordinate** pixels — the border
scales with the view zoom (a 1px border becomes 2px at 2× zoom).

#### `pub fn stroke_cosmetic(mut self, color: Color, width: f32) -> Self`

Cosmetic stroke: the border holds a constant **device-pixel** width at
any zoom (a hairline that never thins out or thickens). Ideal for grid
cells and card outlines in a pannable/zoomable scene.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Human-readable label used for debug and the default AT name.
Accepts anything convertible into `LocalizedString` — most
commonly `tr!(...)`. Plain strings auto-convert.

#### `pub fn draggable(mut self, draggable: bool) -> Self`

Opt the rectangle into drag-to-move.
