<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DebugOverlay

`SceneView` — the viewport widget that hosts a `Scene` and
places its items at scene coordinates.

`SceneView` is the bridge between the model layer (`Scene` /
`SceneModel`) and the render/event pipeline. It
manages a pan/zoom/rotation camera, materialises heavyweight widgets
for delegated items, dispatches pointer events to lightweight item
handlers, and feeds synthetic AT nodes to AccessKit for every visible
lightweight item. Multiple `SceneView`s can share one `SceneModel` and
reconcile independently on every mutation.

## Composition

- **Placement.** `place_children` plants each materialised
  heavyweight widget at its scene-space rect (composed from the
  item's `local_pos`, `transform`, and parent chain).
- **Paint bands.** Three passes: `paint` draws the `Under` lightweight
  items (backdrop), the arena child-walk draws the heavyweight widgets,
  then `post_paint` draws the `Over` lightweight items + marquee /
  foreground / debug overlays. `z` orders within each tier; the
  Under/Over band (`Scene::set_layer`)
  chooses the side. See `docs/bastyde-scene.md` §"Z-order and paint bands".
- **View transform.** Pan / zoom / rotation are four animated
  `Signal<f32>`s on `SceneView`, composed into a derived
  `Signal<Transform2D>` bound via `BuildContext::set_content_transform`
  on the view itself. The render walker pushes that scope around
  the entire subtree, so every materialised widget is visually
  transformed; transform-aware hit-test routes pointer events
  through the same scope.
- **Spatial index.** `place_children` and the paint walk consult
  `Scene::items_in_rect(visible_region)` to skip off-screen items.
- **Idle gating.** Pan / zoom that's reached its terminal tick
  stops scheduling frames via the engine's per-node `paint_epoch`.

## Input wiring

- **`on_scroll`** — trackpad two-finger pan (`ScrollDelta::Pixels`)
  and mouse wheel (`ScrollDelta::Lines`) animate the pan signals
  via `Easing::EaseOut`. Trackpad momentum events from winit
  arrive as further `Pixels` deltas; the existing animation
  pipeline turns this into smooth inertial fling without a custom
  recognizer.
- **`on_pinch`** — OS trackpad pinch (`PinchPhase::Changed`) feeds
  `scale` into the zoom signal and `rotation` into the rotation
  signal, anchored around the gesture center so the scene point
  under the user's fingers stays put.
- **Reduced-motion** — at build time, captures
  `BuildContext::prefers_reduced_motion`.
  When set, scroll handlers `set` the signals directly instead of
  `animate_to`-ing them; pinch is already instantaneous.
- **Drag-to-move** for items carrying `IS_DRAGGABLE`; **marquee**
  selection on the empty viewport surface (or under
  `DragMode::ScrollHandDrag`, pan-on-drag).

## Example

```rust
# use bastyde_scene::{Scene, SceneModel, SceneView, SceneSelectionMode, RectItem};
# use bastyde_canvas::{Point, Rect};
# use bastyde_tokens::Color;
// Build a shared model and add a lightweight rect item.
let model = SceneModel::new();
let local_bounds = Rect::new(0.0, 0.0, 120.0, 80.0);
let item_id = model.add_item(
    RectItem::new(local_bounds).fill(Color::from_rgb(0.2, 0.5, 0.8)),
    Point::new(50.0, 50.0), // local_pos in scene coords
);

// Create viewports backed by that model; each has its own camera.
let _view_a = SceneView::with_model(model.clone())
    .selection_mode(SceneSelectionMode::Single)
    .default_size(800.0, 600.0)
    .initial_zoom(1.5);

let _view_b = SceneView::with_model(model.clone())
    .interactive(false); // axis-chrome / overview pane

// Both views see the item; the model remembers its local_pos.
assert!(model.local_pos(item_id).is_some());
```

## Builder methods at a glance

`ALL`, `is_active`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub struct DebugOverlay`

Visual debug overlays painted on top of normal scene rendering.

Every flag defaults to `false`. Use this to verify that culling /
hit-test / spatial-index / dragging are doing what you expect
while developing a scene-based feature; turn off before shipping.

Each flag adds a thin overlay paint with a distinct color so
multiple flags can be combined without visual confusion:

- `item_bounds`: green outline around every
  visible scene item's `bounds_in_scene`.
- `content_bounds`: blue outline around
  the scene's overall content extent (the union of all item
  bounds).
- `viewport`: red outline around the visible
  scene region (the cull rect — the inverse-projected viewport).
- `selection_bounds`: orange outline
  around every currently-selected item.

```rust
pub struct DebugOverlay { /* fields */ }
```

### Methods

#### `pub const ALL: DebugOverlay = DebugOverlay { item_bounds: true, content_bounds: true, viewport: true, selection_bounds: true, };`

All overlays enabled. Useful to catch any anomaly visually.

#### `pub fn is_active(&self) -> bool`

Whether at least one debug overlay is enabled.

## `pub enum FocusDirection`

Direction passed to a `SceneView::focus_order` callback when the
app wants to override the default Tab cycle.

`Forward` corresponds to Tab; `Backward` to Shift+Tab. The default
SceneView focus traversal is scene insertion order — apps that
need data-flow order (graph editor), story-order (corkboard with
Acts), chronological order (timeline), etc. install a callback
that receives the current focus and returns the next id.

```rust
pub enum FocusDirection { /* variants */ }
```

### Variants

- **`Forward`** — Advance to the next item — corresponds to the Tab key.
- **`Backward`** — Retreat to the previous item — corresponds to Shift+Tab.

## `pub struct SceneView`

A pannable/zoomable viewport that renders a `Scene`'s items at scene
coordinates and routes user input (scroll, pinch, drag, keyboard) back into
the camera signals.

Construct with `SceneView::new` (single-view sugar: wraps a `Scene` in a
fresh `SceneModel`) or `SceneView::with_model` (multi-view: several
viewports share one `SceneModel` and each reconcile independently on every
mutation). Install a heavyweight builder for delegated items via
`delegate_typed`. Add to a `WidgetTree`
like any other widget; gestures and camera animations are wired automatically
during `build`.

See the `module-level documentation` for the full composition model
and `docs/bastyde-scene.md` for an end-to-end guide.

```rust
pub struct SceneView { /* fields */ }
```
