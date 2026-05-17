# `fern-scene`

A pannable / zoomable scene viewport for FernUI. Use it for any
**scene-based** application — story corkboards, mind maps, node-graph
editors, timeline views, CAD canvases, simple maps — where content is
free-positioned at scene coordinates instead of placed by a layout
algorithm.

The crate sits at the same tier as `fern-widgets`: it depends on
`fern-core`, `fern-canvas`, and `fern-tokens`, but **not** on
`fern-widgets`. Apps mixing scene-based and standard-widget UI bring
both crates in.

---

## Two tiers of content

Every scene mixes two tiers under one view transform:

- **Heavyweight tier** — any [`Widget`](../crates/fern-core/src/widget.rs)
  (Button, TextInput, Panel, custom composites) placed at a scene
  position. Fully interactive, fully accessible — every framework
  affordance survives the embedding (focus, animation, AT, drag-and-
  drop, etc.).
- **Lightweight tier** — `SceneItem`s: paint-only objects with no
  arena overhead. Cheap to render thousands of them. Used for the
  background furniture of a scene (connector lines, grids,
  decorative tiles, status dots).

Apps freely mix the two: heavyweight cards arranged on a lightweight
connector-line backdrop, a lightweight grid under a heavyweight
toolbar overlay, etc.

```rust
use fern_scene::{RectItem, Scene, SceneView};
use fern_canvas::{Point, Rect};

let mut scene = Scene::new();
scene.add_widget(my_card_widget(), Rect::new(0.0, 0.0, 200.0, 120.0));
scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0))
        .fill(fern_tokens::Color::RED),
    Point::new(220.0, 0.0),
);
let view = SceneView::new(scene);
tree.add(view);
```

---

## Coordinate model

Coordinates are **parent-relative**, mirroring Qt's
`QGraphicsItem`:

- `local_pos: Point` — origin of the item's local frame, in its
  *parent*'s coordinates (or scene coords if `parent == None`).
- `local_bounds: Rect` — AABB at origin in **local** coords.
- `transform: Transform2D` — rotation / scale / shear applied around
  the local origin before translating by `local_pos`.

The Scene composes the chain (`local → parent → … → scene`) on
demand via [`Scene::scene_transform(id)`](../crates/fern-scene/src/scene.rs).
Helpers project both ways:

```rust
scene.scene_pos(id)          // Point in scene coords
scene.scene_rect(id)         // AABB in scene coords (used by the spatial index)
scene.scene_transform(id)    // local → scene affine
scene.map_to_scene(id, pt)   // local → scene
scene.map_from_scene(id, pt) // scene → local

view.map_to_scene(view_pt)             // view → scene
view.map_from_scene(scene_pt)          // scene → view
view.map_rect_to_scene(view_rect)
view.map_rect_from_scene(scene_rect)
```

Per-item rotation / scale composes through ancestors, so rotating a
parent rotates every descendant visually and updates their hit-test
shapes in lockstep.

---

## `SceneItem` trait

Custom items implement [`SceneItem`](../crates/fern-scene/src/item.rs):

```rust
pub trait SceneItem: Debug + 'static {
    fn local_bounds(&self) -> Rect;
    fn set_local_bounds(&mut self, bounds: Rect);
    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext);

    // Optional:
    fn shape_contains(&self, local_pt: Point) -> bool;       // exact-shape hit-test
    fn initial_flags(&self) -> ItemFlags;                     // set on insert
    fn label(&self) -> Option<String>;                        // debug + AT default
    fn cache_mode(&self) -> CacheMode;                        // None | ItemCoordinate
    fn access_subtree_mode(&self) -> AccessSubtreeMode;
    fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId);
    fn accessibility(&self, b: &mut AccessNodeBuilder, ctx: &SceneItemA11yContext);
}
```

`paint` runs in **local coordinates** — the canvas already has the
item's `scene_transform` (chain × view) pushed by the SceneView paint
walk, so a `RectItem` paints with `canvas.fill_rect(self.local_bounds, ...)`.

Five built-ins ship out of the box:
[`RectItem`](../crates/fern-scene/src/items/rect.rs),
[`PathItem`](../crates/fern-scene/src/items/path.rs) (with per-segment
hit-test for stroke-only paths),
[`ImageItem`](../crates/fern-scene/src/items/image.rs),
[`TextItem`](../crates/fern-scene/src/items/text.rs) (static or
signal-bound), and
[`GroupItem`](../crates/fern-scene/src/items/group.rs) (labelled box
or logical-only AT container).

---

## Item flags

Per-item behaviour is a bitset on [`ItemFlags`](../crates/fern-scene/src/flags.rs).
Default is `IS_VISIBLE | IS_ENABLED | IS_SELECTABLE`.

| Flag | Effect |
|---|---|
| `IS_VISIBLE` | When cleared, the item skips paint and hit-test. Composes through ancestors via `Scene::is_effectively_visible`. |
| `IS_ENABLED` | Disabled items don't dispatch pointer events and don't take focus. |
| `IS_DRAGGABLE` | The item participates in drag-to-move when the view is in `DragMode::RubberBand`. |
| `IS_SELECTABLE` | The item can be picked up by marquee-select. |
| `IS_FOCUSABLE` | The item can receive keyboard focus. |
| `ACCEPTS_HOVER` | Reserved — tracks hover entrance / exit. |
| `CLIPS_TO_SHAPE` / `CLIPS_CHILDREN_TO_SHAPE` | Reserved for clip-region paint. |
| `IGNORES_TRANSFORMATIONS` | Item paints / hit-tests at fixed pixel size regardless of view zoom. Anchor (parent-relative scene point) still follows pan/zoom — so the item tracks the data point underneath, but its size stays constant. Mirrors Qt's `ItemIgnoresTransformations`. |
| `HAS_NO_CONTENTS` | Logical-only entry, skipped by the paint walk. |

Read / mutate via `Scene::flags(id)` / `Scene::set_flag(id, flag, on)` /
`Scene::set_flags(id, flags)`. Convenience: `Scene::set_visible(id,
v)`.

---

## Per-item events

Mirrors `WidgetBuilder`'s attached-handler chain for the lightweight
tier. Install via [`SceneItemHandlerSet`](../crates/fern-scene/src/item_handlers.rs):

```rust
let mut handlers = SceneItemHandlerSet::new();
handlers
    .on_tap(|pt, ctx| ctx.send_intent(MyIntent::Clicked))
    .on_double_tap(|pt, ctx| ctx.send_intent(MyIntent::Open))
    .on_hover(|entered, ctx| { /* … */ })
    .on_context_menu(|pt, ctx| ctx.send_intent(MyIntent::Menu))
    .cursor(CursorIcon::Pointer)
    .tooltip(tr!("card_tooltip"));   // accepts LocalizedString
scene.set_item_handlers(item_id, handlers);
```

The view's pointer-dispatch path projects the screen-space pointer to
scene coords, broad-phases via the spatial index, narrow-phases via
`SceneItem::shape_contains`, then dispatches to the topmost-z hit
item's handlers.

---

## View transform & gestures

[`SceneView`](../crates/fern-scene/src/view.rs) owns four animated
`Signal<f32>`s:

- `pan_x`, `pan_y`
- `zoom`
- `rotation`

The composite [`view_transform`](../crates/fern-scene/src/view.rs)
projects scene → screen and is bound via
`BuildContext::set_transform` so the renderer pushes it around the
entire subtree.

OS gestures plug in directly:

- **Trackpad two-finger pan** and **mouse-wheel scroll** drive
  `pan_x` / `pan_y` (Ctrl+wheel = zoom-about-pointer).
- **Pinch** drives `zoom` and `rotation` anchored on the gesture
  center.
- **Reduced-motion** is honoured: pan / zoom snap instead of
  animating.

Programmatic API:

```rust
view.set_pan(target);     view.pan_to(target, duration);
view.set_zoom(target);    view.zoom_to(target, duration);
view.set_rotation(rad);   view.rotate_to(rad, duration);
view.ensure_visible(scene_rect, margin);   // pan-only fit
view.fit_to_content();    view.center_on(scene_pt);
```

State persistence:

```rust
let snap = view.state();    // SceneViewState — Serde-friendly
view.restore_state(snap);
```

---

## Scene policy: pan / zoom axes

The Scene declares which navigation gestures are permitted. Apps with
a fixed-extent diagram, a horizontal-only timeline, or a
"as-large-as-the-window" embedded mini-graph set:

```rust
scene.pan_axes(PanAxes::None | PanAxes::Horizontal | PanAxes::Vertical | PanAxes::Both);
scene.zoomable(false);   // disables Ctrl+wheel, pinch, +/-
```

The View reads these at gesture-handler wiring time. Pan deltas on a
restricted axis pass through to ancestor scrollables (correct event
propagation).

For inline embeddings of a scene that fills its slot exactly, the
view itself sizes to the scene:

```rust
SceneView::new(scene).adopt_scene_size(true)   // view.size = scene_rect_extent
```

In adopt mode user pan / zoom are no-ops (the entire scene is on
screen) and the view's `layout_response` returns the scene's content
extent instead of a default.

`scene_rect` clamping:

```rust
scene.set_scene_rect(Some(Rect::new(-500.0, -500.0, 2000.0, 2000.0)));
// Programmatic + animated pan now clamp to scene_rect ± viewport.
```

---

## Drag mode

```rust
SceneView::new(scene).drag_mode(DragMode::RubberBand)        // default — item drag → move; empty → marquee
SceneView::new(scene).drag_mode(DragMode::ScrollHandDrag)    // left-drag pans the view
SceneView::new(scene).drag_mode(DragMode::NoDrag)
```

Middle-click pan is unconditional. Right-click on an item with an
`on_context_menu` handler fires the handler.

---

## Reactive observers — `item_change_signal`

Every Scene mutation fires an [`ItemChange`](../crates/fern-scene/src/scene.rs)
event through `Scene::item_change_signal()`. Apps observe to wire
snap-to-grid, validation, persistence:

```rust
let _h = scene.item_change_signal().observe(|change| {
    if let ItemChange::LocalPosChanged { id, new, .. } = change {
        snap(id, *new);
    }
});
```

`ItemChange` variants: `Added`, `Removed`, `LocalPosChanged`,
`LocalBoundsChanged`, `TransformChanged`, `VisibilityChanged`,
`OpacityChanged`, `FlagsChanged`, `ZChanged`, `ParentChanged`.

---

## Collision API

```rust
scene.item_at(scene_pt) -> Option<ItemId>          // topmost-z hit
scene.items_at(scene_pt) -> Vec<ItemId>             // all hits, sorted by z
scene.items_in_rect(scene_rect) -> Vec<ItemId>
scene.colliding_items(id) -> Vec<ItemId>            // items whose AABB intersects id's
scene.items_along_path(&path) -> Vec<ItemId>        // items under a connector polyline
```

Backed by the spatial index (default `GridHashIndex`). All query cost
is O(visible × chain-depth), independent of total scene size.

---

## Background / foreground hooks

Closures injected at the SceneView level for app-supplied chrome.
Both run with the view-transform scope pushed (paint in scene coords)
and receive the visible scene region so geometry off-screen is
trivially cullable:

```rust
SceneView::new(scene)
    .background(|canvas, _ctx, region| {
        // Zoom-aware 50-unit grid, only the visible cells.
        let step = 50.0;
        let mut x = (region.x / step).floor() * step;
        while x < region.x + region.width {
            canvas.draw_line(/* … */);
            x += step;
        }
    })
    .foreground(|canvas, _ctx, region| {
        // Snap-line indicators, ruler chrome, drop-zone hints.
    })
```

Paint order: background → items → marquee → foreground → debug
overlay.

---

## Cache modes

Items override `cache_mode()` to opt into per-item paint caching:

```rust
impl SceneItem for HeavyDecoration {
    fn cache_mode(&self) -> CacheMode { CacheMode::ItemCoordinate }
    // ...
}
```

`ItemCoordinate` records the item's first paint into a sub-canvas as
a `RenderFrame` in **local** coordinates, replays that frame on
subsequent paints. The cache is invalidated automatically on
`LocalBoundsChanged` / `Removed` via an observer wired in
`SceneView::build()`. Items that mutate visual state without going
through a Scene mutator call `view.invalidate_item_cache(id)`
manually.

Don't use `ItemCoordinate` for items whose `paint` reads external
signal state (e.g. a `TextItem::with_signal_text` — its visual
depends on signal updates that don't dirty the cache). Default is
`CacheMode::None`.

---

## Dynamic bounds (signal-driven)

Most items snapshot `local_bounds` at insert time. For items whose
bounds depend on a `Signal<Rect>` read at paint time:

```rust
scene.add_item_dynamic(MyDynItem { ... }, Point::ZERO);
// SceneView::build() calls scene.refresh_dynamic_bounds() each
// rebuild — the spatial index re-buckets on change.
```

---

## Selection

```rust
SceneView::new(scene).selection_mode(SceneSelectionMode::Multi)
// Single | Multi | None
```

Click-to-select (with Ctrl/Shift modifiers for extend / toggle),
marquee box-select. The selection state is a `Signal<HashSet<ItemId>>`
exposed via [`SceneSelection`](../crates/fern-scene/src/selection.rs).

---

## Z-order

```rust
scene.set_z(id, 5.0);
scene.z(id) -> Option<f32>
```

Higher `z` paints later (on top). Equal-`z` items fall back to
insertion order. Z-order is paint-only — it does not change hit-test
priority *between tiers* (heavyweight widgets always win
heavyweight-vs-lightweight collisions).

---

## Removal

```rust
scene.remove(id);     // recursive — id + all descendants
scene.orphan(id);     // promote children to root, leave them alive
```

Recursive remove is the Qt `removeItem` convention: deleting a parent
deletes its children. Apps wanting to drop the parent without losing
the children call `orphan(id)` first (which detaches children and
re-buckets them in the spatial index), then `remove(id)`.

---

## i18n

User-visible strings on `SceneItem` builders (`label`, `tooltip`,
`access_label`, `access_description`, `A11yGroupBuilder::label`,
`SceneView::a11y_label`, `TextItem::new`) accept `impl Into<LocalizedString>`.
Pass the result of `tr!(...)` directly:

```rust
RectItem::new(rect).access_label(tr!("save_card"))
```

Each translated method has an `_literal` `#[doc(hidden)]` twin (e.g.
`access_label_literal`, `tooltip_literal`, `TextItem::new_literal`)
that takes `impl Into<String>`. Use the twin for engine-internal
debug copy or scaffolding where translation is overkill — they're a
grep marker for "intentionally untranslated."

---

## Worked example: corkboard

```rust
let mut scene = Scene::new();
scene.set_scene_rect(Some(Rect::new(0.0, 0.0, 4000.0, 3000.0)));

// Background grid as decoration — lightweight closure, no items.
let view = SceneView::new(scene)
    .selection_mode(SceneSelectionMode::Multi)
    .background(|canvas, _ctx, region| draw_grid(canvas, region, 50.0));

// Add cards as heavyweight widgets.
let card1 = view.scene_mut().add_widget(card("Idea 1"), Rect::new(0.0, 0.0, 200.0, 120.0));
let card2 = view.scene_mut().add_widget(card("Idea 2"), Rect::new(300.0, 200.0, 200.0, 120.0));

// Connector line as a lightweight item beneath the cards.
let path = Path::new()
    .move_to(Point::new(200.0, 60.0))
    .line_to(Point::new(300.0, 260.0));
view.scene_mut().add_item(
    PathItem::new(path, Rect::new(200.0, 60.0, 100.0, 200.0))
        .stroke(Color::BLACK, 2.0),
    Point::ZERO,
);
```

---

## Worked example: simple node-graph editor

```rust
// Each node is a draggable RectItem with a child TextItem label.
let mut scene = Scene::new();
let node = scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 120.0, 60.0))
        .fill(Color::WHITE).stroke(Color::BLACK, 1.0)
        .draggable(true),
    Point::new(100.0, 100.0),
);
let label = scene.add_item(
    TextItem::new(tr!("node_name"), Rect::new(8.0, 8.0, 100.0, 24.0)),
    Point::ZERO,
);
scene.set_item_parent(label, Some(node));

// React to drag-end with snap-to-grid.
let _h = scene.item_change_signal().observe(|c| {
    if let ItemChange::LocalPosChanged { id, new, .. } = c {
        snap_to_grid(*id, *new, 20.0);
    }
});

let view = SceneView::new(scene);
```

---

## Reference

- Implementation: [`crates/fern-scene/src/`](../crates/fern-scene/src/)
- Accessibility-shaping API: [`docs/fern-scene-a11y.md`](fern-scene-a11y.md)
- Showcase demo: `cargo run -p scene-showcase`
- Corkboard demo: `cargo run -p scene-corkboard`
