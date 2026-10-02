<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scene viewport

Use `teksilo-scene` for content positioned in scene coordinates: diagrams,
corkboards, maps, and node editors. A `SceneView` adds pan and zoom to a `Scene`.
Add `teksilo-scene` alongside `teksilo` at the same framework version.

## Minimal example

```rust
use teksilo::prelude::*;
use teksilo::widgets::TextWidget;
use teksilo_scene::{Scene, SceneView};

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Scene")
                .size(800, 600)
                .root(|tree, _| {
                    let mut scene = Scene::new();
                    scene.add_widget(
                        TextWidget::new(lit!("Hello scene")),
                        Rect::new(40.0, 40.0, 200.0, 80.0),
                    );
                    tree.add(SceneView::new(scene))
                }),
        )
        .run();
}
```

## Common operations

| Task | API |
| --- | --- |
| Add an interactive widget | `Scene::add_widget(widget, rect)` |
| Add lightweight geometry | `Scene::add_item(item, position)` |
| Set the scene extent | `Scene::set_scene_rect(Some(rect))` |
| Configure selection | `SceneView::selection_mode` and `SceneSelection` |
| Build movable cards with embedded controls | `SceneCard` |
| Share content between views | `SceneModel` and per-view delegates |
| Keep the camera in application state | `SceneViewState` |
| Follow a data collection | `SceneListAdapter` |
| Add an overview | `SceneMinimap` |

Use widgets for controls that need layout, focus, and keyboard input. Use
`SceneItem` implementations such as `RectItem`, `PathItem`, or `TextItem` for
lightweight content. Give meaningful items accessible names and roles.

## Coordinates and interaction

Item geometry uses scene or parent coordinates. Pointer samples from
`EventContext` use window coordinates. Convert explicitly before applying a
pointer position to scene geometry.

Pan and zoom change the view transform. They do not change the model's item
positions. Each view of a shared model can have its own camera and selection.
For selection, item flags, drag modes, and magnet connections, use the
[scene API reference](scene/index.md).

## Editing after mount

For a shared `SceneModel`, keep a cloned model handle in application state and
mutate that handle. Each view observes the shared model. Heavyweight content
uses data payloads and per-view delegates, so views create separate widgets.

For a view-owned `Scene`, access a mounted `SceneView` through
`EventContext::with_widget_mut` and edit its `scene_mut()` value. Select an
appropriate binding level for the mutation. The
[corkboard example](../examples/scene_corkboard/src/main.rs) demonstrates shared
models, delegates, selection, runtime edits, and separate cameras.

## Constraints

- Keep adapters and observer handles alive for as long as synchronization is needed.
- Do not share mounted widget instances between views. Share application data.
- Choose item visibility, hit testing, and accessibility behavior separately.
  A decorative item should not become an unexplained focus target.
- Test keyboard navigation and off-screen accessibility for the application's
  scene. See [scene accessibility](teksilo-scene-a11y.md).
- Ink tools must process batched pointer samples. See [ink](ink.md).

## Reference

- [Scene API](scene/index.md)
- [Scene source](../crates/teksilo-scene/src/lib.rs)
- [Scene accessibility](teksilo-scene-a11y.md)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/teksilo-scene.md)
are retained in the repository.
