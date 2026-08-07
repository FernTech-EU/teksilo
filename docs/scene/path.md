<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PathItem

`PathItem` — vector path with optional fill and stroke.

`PathItem` renders an arbitrary vector path in local item coordinates.
The path can be filled, stroked, or both. Stroke-only paths use a
per-segment distance hit-test so users can click precisely along the
stroke even when the axis-aligned bounding box is huge — making this
the natural workhorse for connector lines between cards in a node graph
or story corkboard.

Strokes come in two flavours: a **logical** stroke (`.stroke`) scales
with the view zoom, making thick scene-space edges; a **cosmetic** stroke
(`.stroke_cosmetic`) holds a constant device-pixel width at any zoom,
ideal for hairline connector wires that should stay crisp and thin.

## When to use

Use `PathItem` for connector lines, polygon overlays, freehand shapes,
or any vector decoration that needs exact-shape click detection along its
stroke. For solid rectangular regions, prefer the cheaper `RectItem`.

## Example

```ignore
use teksilo_scene::{SceneModel, PathItem};
use teksilo_canvas::{Path, Point, Rect};
use teksilo_tokens::Color;

let model = SceneModel::new();

let mut path = Path::new();
path.move_to(Point::new(0.0, 0.0))
    .line_to(Point::new(200.0, 0.0))
    .line_to(Point::new(200.0, 100.0));

let item = PathItem::new(path, Rect::new(0.0, 0.0, 200.0, 100.0))
    .stroke_cosmetic(Color::new(0.3, 0.3, 0.3, 1.0), 1.5);

model.add_item(item, Point::new(50.0, 50.0));
```

## Builder methods at a glance

`fill`, `stroke`, `stroke_cosmetic`, `stroke_styled`, `label`, `draggable`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_scene/index.html)

## `pub struct PathItem`

An arbitrary vector path with optional fill and stroke, in local
item coordinates.

The path's commands are evaluated in local space. A logical stroke scales
with the view zoom; a `stroke_cosmetic` stroke
holds a constant device-pixel width at any zoom (crisp connectors). The
caller-provided `local_bounds` AABB is what the spatial index buckets on;
it must enclose the path's strokes (including stroke half-width on each
side).

```rust
pub struct PathItem { /* fields */ }
```

### Methods

#### `pub fn new(path: Path, local_bounds: Rect) -> Self`

A path with a caller-provided AABB in local coordinates. The
path's points are interpreted as local — `(0, 0)` is the
item's anchor.

#### `pub fn fill(mut self, color: impl Into<ColorProp>) -> Self`

Fill colour. Accepts a plain `Color`, a theme role, a
`Signal<Color>`, or a `Signal<Role>` — resolved against the active
theme at paint time.

#### `pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Stroke colour and width in **scene-coordinate** pixels — the stroke
scales with the view zoom.

#### `pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Cosmetic stroke: the connector holds a constant **device-pixel** width
at any zoom (it never thins out or thickens). The renderer keeps the
path body sharp at the current zoom, so joins/caps stay correct.

#### `pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self`

Stroke with an explicit `StrokeStyle` — dashed, dotted, or custom caps
/ joins. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
distinguishes a pending connector from a solid confirmed one. The style
is stored verbatim (dash pattern/offset, `Logical` vs `Device` space).

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Human-readable label.

#### `pub fn draggable(mut self, draggable: bool) -> Self`

Opt the path into drag-to-move.
