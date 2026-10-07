<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PathItem

`PathItem` — vector path with optional fill and stroke.

## Public functions

### `PathItem`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(path: Path)`](#pathitem-new) |
| | **Builder methods** |
| `Self` | [`fill(color: impl Into<ColorProp>)`](#pathitem-fill) |
| `Self` | [`fill_rule(rule: FillRule)`](#pathitem-fill_rule) |
| `Self` | [`stroke(color: impl Into<ColorProp>, width: f32)`](#pathitem-stroke) |
| `Self` | [`stroke_cosmetic(color: impl Into<ColorProp>, width: f32)`](#pathitem-stroke_cosmetic) |
| `Self` | [`stroke_styled(color: impl Into<ColorProp>, style: StrokeStyle)`](#pathitem-stroke_styled) |
| `Self` | [`hit_stroke_width(width: f32)`](#pathitem-hit_stroke_width) |
| `Self` | [`label(label: impl Into<LocalizedString>)`](#pathitem-label) |
| `Self` | [`draggable(draggable: bool)`](#pathitem-draggable) |
| | **Methods** |
| `&Path` | [`path()`](#pathitem-path) |

## Detailed description

`PathItem` renders an arbitrary vector path in local item coordinates.
The path can be filled, stroked, or both, and it hit-tests as exactly that
union: a stroke-only path is clickable along its drawn line and nowhere
else, so a connector whose bounding box is mostly empty does not swallow
clicks aimed past it — making this the natural workhorse for connector
lines between cards in a node graph or story corkboard.

Strokes come in two flavours: a **logical** stroke (`.stroke`) scales
with the view zoom, making thick scene-space edges; a **cosmetic** stroke
(`.stroke_cosmetic`) holds a constant device-pixel width at any zoom,
ideal for hairline connector wires that should stay crisp and thin. A
cosmetic stroke's *clickable band* follows the rendered line at any zoom,
because the width is converted from device pixels at test time.

#### Bounds are derived, not declared

A `PathItem` computes its own `local_bounds` from its geometry (plus the
stroke's half-width and the grab slack). There is no caller-supplied AABB
to get wrong, which is what makes the rectangle the spatial index buckets
on and the shape the narrow phase tests provably the same geometry.
`Scene::set_local_bounds` therefore
**moves and scales the path** to fit the rectangle you give it, and the
item re-derives its bounds from the result — landing on the rectangle you
asked for on every axis the path has extent on, and staying there if you
ask again. On an axis it has none — a perfectly horizontal stroke has no
height — the box is the band's own thickness and no request can widen it.

#### When to use

Use `PathItem` for connector lines, polygon overlays, freehand shapes,
or any vector decoration that needs exact-shape click detection along its
stroke. For solid rectangular regions, prefer the cheaper `RectItem`.

#### Example

```ignore
use teksilo_scene::{SceneModel, PathItem};
use teksilo_canvas::{Path, Point};
use teksilo_tokens::Color;

let model = SceneModel::new();

let mut path = Path::new();
path.move_to(Point::new(0.0, 0.0))
    .line_to(Point::new(200.0, 0.0))
    .line_to(Point::new(200.0, 100.0));

let item = PathItem::new(path)
    .stroke_cosmetic(Color::new(0.3, 0.3, 0.3, 1.0), 1.5)
    .hit_stroke_width(12.0);

model.add_item(item, Point::new(50.0, 50.0));
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="pathitem"></a>

## `pub struct PathItem`

An arbitrary vector path with optional fill and stroke, in local
item coordinates.

The path's commands are evaluated in local space. A logical stroke scales
with the view zoom; a `stroke_cosmetic` stroke
holds a constant device-pixel width at any zoom (crisp connectors). The
item's `local_bounds` — the rectangle the spatial index buckets on — is
**derived** from the geometry and the stroke, so it always encloses what
the item can be clicked on.

```rust
pub struct PathItem { /* fields */ }
```

### Methods

<a id="pathitem-new"></a>

#### `pub fn new(path: Path) -> Self`

A path in local coordinates — `(0, 0)` is the item's anchor.

`local_bounds` is derived from the path (and re-derived whenever the
stroke or hit band changes), so it always encloses the clickable area.

<a id="pathitem-fill"></a>

#### `pub fn fill(mut self, color: impl Into<ColorProp>) -> Self`

Fill colour. Accepts a plain `Color`, a theme role, a
`Signal<Color>`, or a `Signal<Role>` — resolved against the active
theme at paint time.

<a id="pathitem-fill_rule"></a>

#### `pub fn fill_rule(mut self, rule: FillRule) -> Self`

The `FillRule` used for **both** painting the fill and hit-testing
the interior.

`FillRule::EvenOdd` gives a ring authored as two subpaths a real
hole — one that is transparent *and* clicks through. There is
deliberately no hit-only fill rule: a shape that paints as a disc and
hit-tests as a ring is the two-sources-of-truth bug this whole type
exists to prevent.

<a id="pathitem-stroke"></a>

#### `pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Stroke colour and width in **scene-coordinate** pixels — the stroke
scales with the view zoom.

<a id="pathitem-stroke_cosmetic"></a>

#### `pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Cosmetic stroke: the connector holds a constant **device-pixel** width
at any zoom (it never thins out or thickens). The renderer keeps the
path body sharp at the current zoom, so joins/caps stay correct.

<a id="pathitem-stroke_styled"></a>

#### `pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self`

Stroke with an explicit `StrokeStyle` — dashed, dotted, or custom caps
/ joins. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
distinguishes a pending connector from a solid confirmed one. The style
is stored verbatim (dash pattern/offset, `Logical` vs `Device` space).

<a id="pathitem-hit_stroke_width"></a>

#### `pub fn hit_stroke_width(mut self, width: f32) -> Self`

Widen the clickable band without widening the drawn line (Konva's
`hitStrokeWidth`): a 1 dp wire, 12 dp grabbable. In local units, so it
is a target size rather than a rendered thickness. Also widens the
derived `local_bounds`, so the broad phase keeps up with it.

<a id="pathitem-label"></a>

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Human-readable label.

<a id="pathitem-draggable"></a>

#### `pub fn draggable(mut self, draggable: bool) -> Self`

Opt the path into drag-to-move.

<a id="pathitem-path"></a>

#### `pub fn path(&self) -> &Path`

The path's commands, in local coordinates.
