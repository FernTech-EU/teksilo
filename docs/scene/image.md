<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ImageItem

`ImageItem` — a raster image at a local-coord rectangle.

`ImageItem` renders a raster image registered in the Canvas image registry
at a caller-specified rectangle in local item coordinates. The image
reference is a string key into that registry, not a path — apps pre-load
images and then name them here.

## When to use

Use `ImageItem` when you need a static or swappable raster graphic in
the lightweight tier (no arena overhead). For interactive images that need
focus, drag-and-drop, or rich accessibility, embed a full `ImageWidget`
as a heavyweight scene widget instead.

## Example

```ignore
use bastyde_scene::{SceneModel, ImageItem};
use bastyde_canvas::Rect;
use bastyde_i18n::lit;

let model = SceneModel::new();
let item = ImageItem::new(Rect::new(0.0, 0.0, 64.0, 64.0), "avatar")
    .label(lit!("User avatar"))
    .draggable(true);
model.add_item(item, bastyde_canvas::Point::new(100.0, 50.0));
```

## Builder methods at a glance

`label`, `draggable`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct ImageItem`

A raster image in a local-coord rectangle.

The image is referenced by a string key into the Canvas image registry.
Place the item in the scene via `Scene::add_item`; the key must resolve
to a registered image at paint time.

```rust
pub struct ImageItem { /* fields */ }
```

### Methods

#### `pub fn new(local_bounds: Rect, name: impl Into<String>) -> Self`

An image item of the given size in local coordinates,
referencing the image registered under `name`. The `name` is
the Canvas-image-registry identifier — not a user-visible
string, so it is not localized.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Human-readable label.

#### `pub fn draggable(mut self, draggable: bool) -> Self`

Opt the image into drag-to-move.
