<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# GroupItem

`GroupItem` — labelled box / logical AT container.

## Public functions

### `GroupItem`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new(local_bounds: Rect)`](#groupitem-new) |
| | **Builder methods** |
| `Self` | [`label(label: impl Into<LocalizedString>)`](#groupitem-label) |
| `Self` | [`show_label(show: bool)`](#groupitem-show_label) |
| `Self` | [`label_inset(dx: f32, dy: f32)`](#groupitem-label_inset) |
| `Self` | [`label_color(color: impl Into<ColorProp>)`](#groupitem-label_color) |
| `Self` | [`fill(color: impl Into<ColorProp>)`](#groupitem-fill) |
| `Self` | [`stroke(color: impl Into<ColorProp>, width: f32)`](#groupitem-stroke) |
| `Self` | [`stroke_cosmetic(color: impl Into<ColorProp>, width: f32)`](#groupitem-stroke_cosmetic) |
| `Self` | [`stroke_styled(color: impl Into<ColorProp>, style: StrokeStyle)`](#groupitem-stroke_styled) |
| `Self` | [`corner_radius(radius: f32)`](#groupitem-corner_radius) |
| | **Methods** |
| `bool` | [`is_visual()`](#groupitem-is_visual) |

## Detailed description

Visually a labelled rectangle with optional fill, stroke, and
inline label. Without any chrome it's a logical-only container
that announces itself to AT but draws nothing — the lightweight
analogue of an `A11yGroup`.

#### When to use

Use `GroupItem` when you need to:
- Draw a visible boundary box around a cluster of related items
  (e.g. a lane in a Kanban board, an "Act 1" region on a corkboard).
- Provide a named AT group that screen readers announce without
  any visible chrome — call `GroupItem::label` but omit `fill`
  and `stroke`, leaving `is_visual()` false.

#### Example

```ignore
use teksilo_scene::{Scene, GroupItem};
use teksilo_canvas::{Point, Rect};
use teksilo_tokens::Color;
use teksilo_i18n::lit;

let mut scene = Scene::new();
// A visible "Act 1" box with a rounded border.
let group = GroupItem::new(Rect::new(0.0, 0.0, 400.0, 600.0))
    .label(lit!("Act 1"))
    .show_label(true)
    .stroke(Color::new(0.6, 0.6, 0.6, 1.0), 1.5)
    .corner_radius(8.0);
let _id = scene.add_item(group, Point::new(20.0, 20.0));
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="groupitem"></a>

## `pub struct GroupItem`

A group container with optional fill / stroke / inline label, in
local item coordinates.

Visually, GroupItem renders a labelled box around its members.
Logically, it's the AT-grouping primitive: with no chrome and a
label set, it announces itself to AT but draws nothing.

```rust
pub struct GroupItem { /* fields */ }
```

### Methods

<a id="groupitem-new"></a>

#### `pub fn new(local_bounds: Rect) -> Self`

A group covering `local_bounds` in local coordinates. No
chrome by default — call `fill` / `stroke` / `show_label` to
give it visible outline / background / inline label.

<a id="groupitem-label"></a>

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Human-readable label, used as the default AT group name and
(when `show_label` is enabled) rendered inline at top-leading.

<a id="groupitem-show_label"></a>

#### `pub fn show_label(mut self, show: bool) -> Self`

Render the label inline at paint time.

<a id="groupitem-label_inset"></a>

#### `pub fn label_inset(mut self, dx: f32, dy: f32) -> Self`

Override the inset of the inline label from the local origin.

<a id="groupitem-label_color"></a>

#### `pub fn label_color(mut self, color: impl Into<ColorProp>) -> Self`

Override the inline label colour. Defaults to the stroke colour if set,
else `Color::BLACK`. Accepts a plain `Color`, a theme role, or a
reactive signal.

<a id="groupitem-fill"></a>

#### `pub fn fill(mut self, color: impl Into<ColorProp>) -> Self`

Background fill colour. Accepts a plain `Color`, a theme role, a
`Signal<Color>`, or a `Signal<Role>` — resolved against the active theme
at paint time.

<a id="groupitem-stroke"></a>

#### `pub fn stroke(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Border stroke (colour + scene-coord pixel width) — scales with zoom.

<a id="groupitem-stroke_cosmetic"></a>

#### `pub fn stroke_cosmetic(mut self, color: impl Into<ColorProp>, width: f32) -> Self`

Cosmetic border stroke: holds a constant **device-pixel** width at any
zoom. With `corner_radius > 0` the rounded outline goes through the SDF
cosmetic path; otherwise `stroke_rect` emits four `CosmeticLine` edges
(one per side), which are hard-edged and crisp at any zoom.

<a id="groupitem-stroke_styled"></a>

#### `pub fn stroke_styled(mut self, color: impl Into<ColorProp>, style: StrokeStyle) -> Self`

Border stroke with an explicit `StrokeStyle` — dashed / dotted /
custom caps. E.g. `.stroke_styled(color, StrokeStyle::dashed(2.0, 6.0, 4.0))`
for a dashed lane boundary.

A dashing style costs a CPU-rasterized path: `Canvas` diverts it away
from the decoration-rect / SDF pipelines, which cannot express a dash.
Solid and cosmetic strokes keep the cheap route.

<a id="groupitem-corner_radius"></a>

#### `pub fn corner_radius(mut self, radius: f32) -> Self`

Rounded corners for fill and stroke. Default `0.0`.

<a id="groupitem-is_visual"></a>

#### `pub fn is_visual(&self) -> bool`

Whether the group has any visual chrome configured.
