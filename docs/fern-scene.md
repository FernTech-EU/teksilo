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
| 1 | `Scene` + `SceneView` + free positioning | ✅ landed |
| 2 | View transform, pan/zoom/rotate, gestures, inertial fling | ✅ this doc |
| 3 | `SpatialIndex` trait + `GridHashIndex` + viewport culling | not yet |
| 4 | `SceneItem` trait + lightweight built-ins | not yet |
| 5a / 5b | Visual-default a11y / a11y-shaping tools | not yet |
| 6 | Selection, marquee, drag-to-move | not yet |
| 7 | Polish + R-tree alternative + mini-map | not yet |

## Phase 1 + 2 surface

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
        .min_zoom(0.25)
        .max_zoom(4.0)
}
```

Working demo: [`examples/scene_corkboard/`](../examples/scene_corkboard/src/main.rs).
Run with `cargo run -p scene-corkboard`. From Phase 2 the demo is
pannable (two-finger trackpad / mouse wheel), zoomable (pinch on
trackpad), and inertial fling on release works automatically through
the platform's momentum events.

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

Surface:

- `SceneView::new(scene)` — wrap a Scene.
- `SceneView::default_size(w, h)` — size used when the parent
  `SizeProposal` is unspecified on either axis. Defaults to 800×600.
- `SceneView::min_zoom(v)` / `max_zoom(v)` — clamp range applied to
  every programmatic and gesture-driven zoom change. Defaults: 0.1×
  to 10×.
- `SceneView::line_height(px)` — logical pixels of pan per
  `ScrollDelta::Lines` notch (mouse wheel). Defaults to 16, matching
  `ScrollArea`.
- `SceneView::scene()` / `scene_mut()` — borrow the underlying Scene.
- `SceneView::widget_id_for(item_id) -> Option<WidgetId>` — resolve
  an item's materialised widget id (after the first layout pass).
- `SceneView::pan() -> Vec2` / `zoom() -> f32` / `rotation() -> f32` —
  read the current view-transform state.
- `SceneView::view_transform() -> Transform2D` — the composed view
  transform (the same one the render walker has on its stack while
  painting this view's subtree).
- `SceneView::pan_to(target, duration)` / `zoom_to(target, duration)` /
  `rotate_to(target, duration)` — animate to a value via
  `Easing::EaseOut`.
- `SceneView::set_pan(target)` / `set_zoom(target)` — snap without
  animating.
- `SceneView::fit_to_content()` — animate pan + zoom so the
  bounding box of all scene items fits the current viewport with a
  small margin. Resets rotation. No-op for an empty scene.
- `SceneView::scene_content_bounds() -> Option<Rect>` — the
  axis-aligned bounding box of all scene items.
- `SceneView::viewport_size() -> Size` — most recent viewport size
  observed during layout.

The `Widget` impl:

- `build` drains `pending_widget` from each entry (first build) or
  returns the cached widget ids (subsequent rebuilds), then registers
  the four animated signals with the scheduler, derives a
  `Signal<Transform2D>` from them, binds it via
  `BuildContext::set_transform` on the SceneView itself, and wires
  the `on_scroll` and `on_pinch` handlers via `apply_self_handlers`.
- `layout_response` is greedy and caches the resolved viewport size.
- `place_children` plants each child at `bounds.origin +
  scene_rect.origin` — transform-free. The view transform is the
  `set_transform` scope; the render walker pushes it around the
  whole subtree, and Phase 0's transform-aware hit-test routes
  pointer events through the same scope.

### View transform model

The view is composed from four `Signal<f32>`s (kept separate so each
animates independently with the right epsilon — sub-pixel pan,
sub-perceptual log-multiplier zoom, sub-degree rotation):

- `pan_x` / `pan_y` — viewport offset in logical pixels.
- `zoom` — multiplicative scale around the scene's origin.
- `rotation` — radians, around the scene's origin.

Composition (see `transform.rs::compose_view`): scale → rotate →
translate. Concretely, `compose_view(pan, zoom, rot).apply_point(p)` =
`(R_rot ∘ S_zoom)(p) + pan`. Composition order matches the
renderer's stack semantic (`device_t.then(prev_top)`, deepest-first),
so the same single transform fed into `set_transform` produces the
expected visual.

Pinch-to-zoom-around-pointer uses
`transform.rs::anchor_pan_for_pinch`, which solves for a new pan
that keeps the scene point under the gesture center invariant when
the zoom or rotation changes.

### Gestures

Wired automatically by `SceneView::build`:

- **Trackpad two-finger pan / wheel pan** — `WidgetEvent::Scroll`
  with `ScrollDelta::Pixels { x, y }` (trackpad) animates pan via
  `Easing::EaseOut` over `~120 ms`. `ScrollDelta::Lines` (mouse
  wheel) is multiplied by `line_height` first.
- **Inertial fling** — winit forwards trackpad momentum after
  release as further `ScrollDelta::Pixels` events. The same
  `animate_to` pipeline absorbs them, producing smooth fling
  without a custom recognizer.
- **Pinch zoom** — OS trackpad pinch (`PinchPhase::Changed { center,
  scale, rotation }`) feeds `scale` into the zoom signal (clamped to
  `[min_zoom, max_zoom]`) and `rotation` into the rotation signal.
  The pan is re-anchored so the scene point under the gesture
  center stays put. Pinch is set synchronously each frame — no
  tween, since it's a continuous user-driven gesture.
- **Reduced motion** — at build time, `SceneView` captures
  `BuildContext::prefers_reduced_motion()`. When set, the scroll
  handler `set`s the pan signals directly instead of
  `animate_to`-ing them. Pinch is already instantaneous.

`Ctrl-wheel zoom` is intentionally absent in Phase 2 because
`WidgetEvent::Scroll` does not currently carry a `Modifiers` field.
Adding one is straightforward but cross-cutting; deferred to a
later phase. Pinch + the imperative `zoom_to` cover the gap for
trackpad and programmatic zoom; mouse-only users will get a
keyboard `+` / `-` zoom binding once Phase 5a's keyboard navigation
lands.

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

The four view-transform signals are animated `Signal<f32>`s registered
with the framework's scheduler, so the four idle gates apply for free:

1. **Per-widget paint-epoch visibility.** A SceneView that's been
   scrolled off-screen (e.g. inside a parent ScrollArea or in a
   minimised window) stops getting `paint()` walks; the scheduler
   pauses all looping animations whose owner widget is off-screen
   for one frame.
2. **Per-window active flag.** When the window loses focus, the
   scheduler is a no-op until focus returns. Pan/zoom in flight at
   that moment is rebased — its perceived progress doesn't jump.
3. **Pixel-stable epsilon.** Each `animate_to` skips `signal.set()`
   when the delta is below the framework's default epsilon
   (effectively sub-pixel for `f32` values typical of pan/zoom).
4. **Drop-cancel.** When the SceneView is destroyed, the scheduler
   drops its registrations.

In practice this means: at rest (no scroll, no pinch, no in-flight
`animate_to`), the loop sleeps with `ControlFlow::Wait`. Inertial
fling is finite-duration via `animate_to(...EaseOut)`, so the loop
goes quiet automatically when the tween lands.

The unit tests pin this: see
`crates/fern-scene/src/view.rs::tests::idle_drain_zero_frames_at_rest`
and `idle_drain_returns_after_pan_animation_completes`.

Per-axis epsilon tuning (Phase 2 default uses the framework's
generic epsilon; finer-grained per-signal epsilon via
`Signal::try_animate_with_options` is a Phase 7 polish item):

- pan: 0.5 logical pixels (sub-pixel — invisible).
- zoom: ~0.001 in log2 space (~0.07% multiplicative).
- rotation: ~1e-3 rad (~0.057°).

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
