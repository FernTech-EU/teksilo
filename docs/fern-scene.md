# `fern-scene` — pannable/zoomable scene viewport

A sub-toolkit for **scene-based applications** — story corkboards, mind
maps, node-graph editors, timeline views — where content is
**free-positioned at scene coordinates** instead of placed by a layout
algorithm. Two tiers of content coexist under one view transform:

- **Heavyweight tier** — any `Widget` (Button, TextInput, Panel,
  composite components) at a `scene_rect`. Fully interactive, fully
  accessible, with focus / keyboard / animations / a11y machinery
  intact.
- **Lightweight tier** — `SceneItem`s (paths, rects, images, custom
  paint) without arena overhead. For the "background furniture" of a
  scene where thousands of items render cheaply. *(Phase 4+; not yet
  shipped.)*

The crate is built on top of fern-core's existing per-node
`set_transform` scope (which composes through hit-test, paint, and
a11y) and the platform's already-plumbed pinch / scroll-with-modifiers
/ animated-`Signal<f32>` infrastructure — so OS gestures,
reduced-motion snapping, and the four-gate idle scheduler fall out for
free.

For accessibility-shaping (logical AT tree, groups, relations, custom
focus order, ComboBox-in-a-scene auto-graft), see
[`docs/fern-scene-a11y.md`](fern-scene-a11y.md). For the full design
and roadmap, see [`docs/plans/scene-plan.md`](plans/scene-plan.md).

## Status

This document tracks what's actually shipped. Anything described as
"Phase N+" is in [`docs/plans/scene-plan.md`](plans/scene-plan.md) but
not yet implemented.

| Phase | What | Status |
|-------|------|--------|
| 0 | Transform-aware hit-test in fern-core | ✅ landed |
| 1 | `Scene` + `SceneView` + free positioning | ✅ this doc |
| 2 | View transform, pan/zoom/rotate, gestures, inertial fling | not yet |
| 3 | `SpatialIndex` trait + `GridHashIndex` + viewport culling | not yet |
| 4 | `SceneItem` trait + lightweight built-ins | not yet |
| 5a / 5b | Visual-default a11y / a11y-shaping tools | not yet |
| 6 | Selection, marquee, drag-to-move | not yet |
| 7 | Polish + R-tree alternative + mini-map | not yet |

## Phase 1 surface

```rust
use fern_scene::{ItemId, Scene, SceneView};
use fern_ui::canvas::Rect;
use fern_ui::widgets::{Panel, TextWidget, VStack};

fn build_corkboard() -> SceneView {
    let mut scene = Scene::new();

    let _id: ItemId = scene.add_widget(
        Panel::new().child(TextWidget::new_literal("Act I — Opening")),
        Rect::new(32.0, 32.0, 220.0, 140.0),
    );
    scene.add_widget(
        Panel::new().child(TextWidget::new_literal("Inciting Incident")),
        Rect::new(276.0, 32.0, 220.0, 140.0),
    );
    // …more cards…

    SceneView::new(scene)
}
```

Working demo: [`examples/scene_corkboard/`](../examples/scene_corkboard/src/main.rs).
Run with `cargo run -p scene-corkboard`.

## Concepts

### Scene

A passive data model: a flat collection of items at scene coordinates.
The `Scene` itself does no rendering or layout — `SceneView` reads
from it at build / place / paint time.

Phase 1 surface:

- `Scene::new()` — empty.
- `Scene::add_widget(widget, scene_rect) -> ItemId` — place a
  heavyweight widget.
- `Scene::move_item(id, new_bounds)` / `Scene::remove(id)` — mutators.
  Phase 1 expects pre-build mutation; full runtime mutation is Phase 6.
- `Scene::items_in_rect(scene_rect) -> Vec<ItemId>` — brute-force AABB
  query. Phase 3 swaps in a `SpatialIndex`.
- `Scene::scene_rect(id) -> Option<Rect>` / `len` / `is_empty` /
  `ids` — read accessors.

### `ItemId`

Opaque newtype over a globally unique `u64`. Returned by `add_widget`,
used to address an item later. Stable for the process lifetime; safe to
hash, compare, store. The raw value is exposed via `as_u64()` for the
synthetic-NodeId hashing the AT walker will use in Phase 5.

### SceneView

The viewport widget. Wraps a `Scene`, materialises its widgets into
the arena on `build`, and places them at their `scene_rect`s in
`place_children`.

Phase 1 surface:

- `SceneView::new(scene)` — wrap a Scene.
- `SceneView::default_size(w, h)` — size used when the parent
  `SizeProposal` is unspecified on either axis. Defaults to 800×600.
- `SceneView::scene()` / `scene_mut()` — borrow the underlying Scene.
- `SceneView::widget_id_for(item_id) -> Option<WidgetId>` — resolve
  an item's materialised widget id (after the first layout pass).

The `Widget` impl:

- `build` drains `pending_widget` from each entry (first build) or
  returns the cached widget ids (subsequent rebuilds).
- `layout_response` is greedy: it accepts the parent's proposal,
  falling back to `default_size` on unspecified axes.
- `place_children` plants each child at `bounds.origin +
  scene_rect.origin` with the scene rect's size — i.e. scene-coord
  (0,0) is anchored to the SceneView's bounds origin.
- The view transform is identity in Phase 1; Phase 2 layers a
  `set_transform` scope driven by animated `pan_x`/`pan_y`/`zoom`/
  `rotation` signals on top of the same placement code.

### Free positioning

Every existing FernUI container places children via a layout
algorithm (`HStack`, `VStack`, `Grid`, …). `SceneView` is the first
container that **bypasses the layout algorithm entirely** — its
`place_children` reads each child's `scene_rect` from the underlying
Scene model and plants it at that rectangle, full stop. That's what
makes "drop a card at (320, 480) in scene space" work.

A child's `scene_rect` is independent of its parents' transforms or
sizes; it's a fixed coordinate in the scene's own coordinate system.
Pan/zoom (Phase 2) doesn't change scene rects — it changes the view
transform applied on top.

## Accessibility (Phase 1)

In Phase 1, every materialised widget participates in the AT walker as
a normal direct child of `SceneView`. Reading-order Tab cycle is the
arena's natural focusable-walk order. Screen readers see each card's
`Panel`/`TextWidget` content correctly; pan/zoom doesn't exist yet so
nothing is off-screen-via-transform.

The full a11y story — synthetic AT nodes for lightweight items, the
parallel structural layer (logical groups, parents, relations,
auto-graft for ComboBox-in-a-card cases), custom focus / directional
navigation callbacks, off-screen visibility policies — lands in
Phases 5a and 5b. See
[`docs/fern-scene-a11y.md`](fern-scene-a11y.md) once that ships.

## Idle compliance

Phase 1 has no animation surface of its own — there's no pan/zoom yet,
no inertial fling, no looping items. Once a SceneView has been built
and laid out, the framework's standard idle gates apply unchanged.
Phase 2 introduces the four animated signals (`pan_x`/`pan_y`/`zoom`/
`rotation`) and documents the per-axis epsilon and reduced-motion
behaviour here.

## See also

- [`docs/plans/scene-plan.md`](plans/scene-plan.md) — full design and
  roadmap.
- [`docs/fern-scene-a11y.md`](fern-scene-a11y.md) — the dedicated a11y
  reference *(Phase 5a+)*.
- [`docs/idle-and-animation.md`](idle-and-animation.md) — the
  framework's four-gate idle scheduler that fern-scene's pan/zoom
  will plug into in Phase 2.
- [`docs/accessibility-overrides.md`](accessibility-overrides.md) —
  the widget-level `.access_*` chain, which fern-scene mirrors on
  `SceneItem`s in Phase 5b.
