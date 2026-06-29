<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneMinimap

`SceneMinimap` — a small thumbnail of a `Scene`
showing all items as dots / rects scaled down, with an overlay
highlighting the currently visible viewport rectangle.

## Use

```
use bastyde_scene::{Scene, SceneView, SceneMinimap};
use bastyde_canvas::Rect;
# use bastyde_widgets::VStack;

let mut scene = Scene::new();
/* …populate scene… */
// Build the SceneView FIRST so we can read its reactive
// viewport signal and its scene's snapshot of items.
let view = SceneView::new(scene);
let content = view
    .scene_content_bounds()
    .unwrap_or(Rect::new(0.0, 0.0, 1000.0, 1000.0));
let viewport_signal = view.viewport_in_scene_signal();
let item_thumbs = view.scene().item_thumbnails(); // Vec<(Rect, Color)>

let _w = VStack::new()
    .child(view)
    .child(
        SceneMinimap::new(content, viewport_signal)
            .items(item_thumbs)
            .size(200.0, 150.0),
    );
```

For a live "items as they move" minimap, re-call
`Scene::item_thumbnails` on
scene mutations and rebuild the widget tree (or wire a
`Signal<Vec<(Rect, Color)>>` if your app needs per-frame
reactivity).

## Design

Deliberately decoupled from `SceneView`: it doesn't reach into
the scene model. Instead it consumes a content extent (the rect
that maps to "the entire minimap area"), a static `Vec<(Rect, Color)>`
of item thumbnails (refreshed by the app whenever items move),
and a `Signal<Rect>` for the live viewport rectangle.

Apps that want a live "items as they move" minimap rebuild their
widget tree on scene mutations or wire a `Signal<Vec<...>>`. The
viewport overlay is reactive on its own — the minimap re-paints
whenever the SceneView's pan / zoom changes, with no manual
plumbing.

## Builder methods at a glance

`size`, `items`, `background`, `border`, `viewport_color`, `content_outline`, `on_click`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct SceneMinimap`

A small thumbnail rendering of a `Scene`'s
content, with the live viewport rectangle highlighted.

Paint order: background fill → optional content-bounds outline
→ item thumbnails (dots / rects) → viewport overlay rect.

```rust
pub struct SceneMinimap { /* fields */ }
```

### Methods

#### `pub fn new(content_bounds: Rect, viewport: Signal<Rect>) -> Self`

Construct a minimap covering `content_bounds` (the scene-coord
extent that maps to the full minimap area), with `viewport`
driving the live overlay rectangle.

#### `pub fn size(mut self, width: f32, height: f32) -> Self`

Override the minimap size. Default `200×150`.

#### `pub fn items(mut self, items: Vec<(Rect, Color)>) -> Self`

Static list of item thumbnails: `(scene_rect, color)`. The
minimap projects each rect onto its drawing area and fills it
with `color`. Apps refresh by rebuilding the widget tree
when items move.

#### `pub fn background(mut self, color: Color) -> Self`

Background fill color. Default semi-transparent white.

#### `pub fn border(mut self, border: Option<(Color, f32)>) -> Self`

Border around the minimap drawing area. Pass `None` for no
border. Default 1px @ 50% black.

#### `pub fn viewport_color(mut self, color: Color) -> Self`

Color of the viewport overlay rectangle. Default solid blue.

#### `pub fn content_outline(mut self, outline: Option<(Color, f32)>) -> Self`

Outline the content extent inside the minimap (gives users a
"you're somewhere inside this much scene" cue when the
minimap is taller / wider than its content). Default `None`.

#### `pub fn on_click<F>(mut self, callback: F) -> Self where F: Fn(Point, &mut EventContext) + 'static,`

Click handler: fires with the scene-coord corresponding to
the click, plus the standard `EventContext`. Apps wire this
to e.g. `SceneView::pan_to_center` for click-to-recenter.
