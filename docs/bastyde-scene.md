<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# `bastyde-scene`

A pannable / zoomable scene viewport for Bastyde. Use it for any
**scene-based** application — story corkboards, mind maps, node-graph
editors, timeline views, CAD canvases, simple maps — where content is
free-positioned at scene coordinates instead of placed by a layout
algorithm.

The crate sits at the same tier as `bastyde-widgets`: it depends on
`bastyde-core`, `bastyde-canvas`, and `bastyde-tokens`, but **not** on
`bastyde-widgets`. Apps mixing scene-based and standard-widget UI bring
both crates in.

---

## Two tiers of content

Every scene mixes two tiers under one view transform:

- **Heavyweight tier** — any [`Widget`](../crates/bastyde-core/src/widget.rs)
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
use bastyde_scene::{RectItem, Scene, SceneView};
use bastyde_canvas::{Point, Rect};

let mut scene = Scene::new();
scene.add_widget(my_card_widget(), Rect::new(0.0, 0.0, 200.0, 120.0));
scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0))
        .fill(bastyde_tokens::Color::RED),
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
demand via [`Scene::scene_transform(id)`](../crates/bastyde-scene/src/scene.rs).
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

Custom items implement [`SceneItem`](../crates/bastyde-scene/src/item.rs):

```rust
pub trait SceneItem: Debug + 'static {
    fn local_bounds(&self) -> Rect;
    fn set_local_bounds(&mut self, bounds: Rect);
    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext<'_>);

    // Optional:
    fn set_fill(&mut self, fill: Option<ColorProp>) -> bool;                     // live colour mutation
    fn set_stroke(&mut self, stroke: Option<(ColorProp, StrokeStyle)>) -> bool;
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
`set_fill` / `set_stroke` default to a no-op (`false`) and back the live
[`SceneModel::set_item_fill`](../crates/bastyde-scene/src/scene.rs) /
`set_item_stroke` mutators — see **Item colours & theming** below.

Five built-ins ship out of the box:
[`RectItem`](../crates/bastyde-scene/src/items/rect.rs) (optional
`corner_radius` and styled/dashed strokes via `stroke_styled`),
[`PathItem`](../crates/bastyde-scene/src/items/path.rs) (with per-segment
hit-test for stroke-only paths, also `stroke_styled`),
[`ImageItem`](../crates/bastyde-scene/src/items/image.rs),
[`TextItem`](../crates/bastyde-scene/src/items/text.rs) (static or
signal-bound; horizontal `align(TextAlign::{Leading,Center,Trailing})`, a
free `rotation(radians)`, and a `measure(&mut dyn TextBackend) -> Size`
helper for sizing a slot around a label), and
[`GroupItem`](../crates/bastyde-scene/src/items/group.rs) (labelled box
or logical-only AT container — also has `corner_radius` and
`stroke_styled`).

---

## Item colours & theming

`SceneItem::paint` receives a
[`SceneItemPaintContext`](../crates/bastyde-scene/src/item.rs) carrying
everything a colour-bearing item needs to resolve its chrome against the
live theme:

| Field | Meaning |
|---|---|
| `theme: &Theme` | The **fully-projected** theme for this paint pass — already swapped to the inactive-window / high-contrast variant by the render walker. Read it directly; never call `Theme::for_inactive_window` yourself. |
| `window_active: bool` | `true` iff the host window is focused and unoccluded (`true` in headless tests). For behavioural blur cues the theme swap alone can't cover. |
| `enabled: bool` | The item's effective `IS_ENABLED` flag, forwarded to `ColorProp::resolve` so a role colour picks its disabled variant. |
| `text_scale: f32` | The global accessibility text-scale factor, for `TextItem::follow_text_scale` opt-ins. |
| `view_transform` / `dirty_scene_rect` | Unchanged — pan/zoom/rotation and the current repaint region. |

Built-in items' fill / stroke / foreground fields are
[`ColorProp`](../crates/bastyde-core/src/color_prop.rs)s — accepting a plain
[`Color`](../crates/bastyde-tokens/src/color.rs), a theme role (`SurfaceRole`
/ `TextRole` / `BorderRole`), a `Signal<Color>`, or a `Signal<Role>` — and are
resolved with `prop.resolve(ctx.theme, ctx.enabled)` inside `paint`. Because
`ctx.theme` is already the inactive-window projection, a role fill
auto-desaturates when the window loses focus with zero per-item code:

```rust
RectItem::new(rect)
    .fill(SurfaceRole::Sunken)       // resolves against ctx.theme at paint
    .stroke(BorderRole::Default, 1.0)
```

### Reactive colours

A colour is continuously reactive in two ways:

- **Build-time**: construct the item with a `Signal<Color>` or a
  `Signal<Role>` (e.g. `.fill(my_signal.clone())`). Every colour-bearing
  built-in (`RectItem`, `PathItem`, `GroupItem`, `TextItem`) registers its
  bound `ColorProp`s at `BindingLevel::RepaintOnly` in `register_bindings`, so
  a signal change repaints the owning `SceneView` — no relayout, no rebuild.
- **Runtime**: mutate a *mounted* item's colour live through the shared
  [`SceneModel`] — `set_item_fill` / `clear_item_fill` / `set_item_stroke` /
  `clear_item_stroke`. Each emits
  [`ItemChange::AppearanceChanged`](../crates/bastyde-scene/src/scene.rs),
  which the view treats as **repaint-only, always** (it evicts the item's paint
  cache and repaints — never a relayout, rebuild, or AccessKit re-walk). These
  install a *snapshot*, which is all a static colour ever needs. Passing a
  `Signal`/dynamic role here paints its current value immediately and starts
  tracking it continuously from the view's next rebuild (whenever some other
  structural change re-runs `register_bindings`) — a colour change is
  deliberately never allowed to cost a rebuild. **For a colour that tracks its
  signal forever, construct the item with it** (the build-time path above).

```rust
let model = view.model();
model.set_item_fill(card_id, SurfaceRole::AccentSubtle);   // repaint only
model.set_item_stroke(card_id, Color::RED, StrokeStyle::dashed(2.0, 6.0, 4.0));
model.clear_item_fill(card_id);
```

`set_fill` / `set_stroke` are also the `SceneItem` trait hooks (default
no-op) a custom item overrides to participate in the same live-mutation
path: `RectItem` / `PathItem` / `GroupItem` accept both; `TextItem` maps
`set_fill` onto its foreground colour (a `None` clear is rejected — text
always has a colour); `ImageItem` accepts neither. Note that on a `GroupItem`,
giving a previously **logical** (chrome-less, click-through) group a fill or
stroke makes it *visual* — it starts hit-testing and will absorb clicks that
used to fall through to the items it groups.

> **Minimap caveat.** `SceneItem::thumbnail_color` (what
> [`SceneMinimap`](../crates/bastyde-scene/src/minimap.rs) renders) is
> theme-free by signature, so an item whose colour is a **theme role** has no
> theme to resolve against and falls back to a neutral grey on the minimap.
> Use a concrete `Color` or a `Signal<Color>` for items you want faithfully
> represented there.

> **Cache caveat.** A custom item that opts into `CacheMode::ItemCoordinate`
> bakes its *resolved* colours into the cached frame. The `SceneView`
> invalidates that cache on a theme swap, a window-active flip, and an
> `IS_ENABLED` change, so role colours stay correct — but the cache is still
> keyed by `(id, raster_scale)`, so a custom item whose paint depends on any
> *other* ambient state must use the default `CacheMode::None`.

---

## Item flags

Per-item behaviour is a bitset on [`ItemFlags`](../crates/bastyde-scene/src/flags.rs).
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
tier. Install via [`SceneItemHandlerSet`](../crates/bastyde-scene/src/item_handlers.rs):

```rust
let mut handlers = SceneItemHandlerSet::new();
handlers
    .on_tap(|pt, ctx| ctx.send_intent(MyIntent::Clicked))
    .on_double_tap(|pt, ctx| ctx.send_intent(MyIntent::Open))
    .on_hover(|entered, ctx| { /* … */ })
    .on_context_menu(|pt, ctx| ctx.send_intent(MyIntent::Menu))
    .cursor(CursorIcon::Pointer)
    .tooltip(tr!(card_tooltip()));   // accepts LocalizedString
scene.set_item_handlers(item_id, Some(handlers));
```

The view's pointer-dispatch path projects the screen-space pointer to
scene coords, broad-phases via the spatial index, narrow-phases via
`SceneItem::shape_contains`, then dispatches to the topmost-z hit
item's handlers.

---

## View transform & gestures

[`SceneView`](../crates/bastyde-scene/src/view.rs) owns four animated
`Signal<f32>`s:

- `pan_x`, `pan_y`
- `zoom`
- `rotation`

The composite [`view_transform`](../crates/bastyde-scene/src/view.rs)
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

**Drag-start hit-test (narrow-phase).** In `RubberBand` mode, a press
decides *item drag vs. marquee* by hitting only **draggable** lightweight
items (`IS_DRAGGABLE` — opt in via `.draggable(true)`), and it hits them with
the **exact-shape** test, not just their AABB: a per-item snapshot carries the
item's `scene_rect` (broad-phase) plus its `shape_contains` predicate
(narrow-phase) and scene transform, sorted topmost-first. A press lands on an
item only when it falls inside the *shape* (a thin diagonal `PathItem`, a
ring, a rotated rect) — a press in the AABB but off the shape, or over a
non-draggable backdrop / heavyweight card, falls through to a **marquee**. This
is why dragging from on top of a select-only (non-`IS_DRAGGABLE`) card still
rubber-bands instead of nudging the scene: the card is not in the draggable
snapshot, and the cross-widget tap/drag disambiguation (see
[events-and-gestures.md](events-and-gestures.md)) lets the view's `on_drag`
start even though the card carries an `on_tap`.

---

## Reactive observers — `item_change_signal`

Every Scene mutation fires an [`ItemChange`](../crates/bastyde-scene/src/scene.rs)
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

`GridHashIndex` buckets an item into every grid cell its AABB overlaps.
Cell count grows as `(width / cell_size) * (height / cell_size)`, and
`cell_size` clamps to a 1.0 minimum, so nothing bounds it on its own — a
single `Scene::add_item` with a full-document backdrop or canvas rect at a
small `cell_size` can ask for billions of cells. An item whose AABB would
span more than `MAX_CELLS_PER_ITEM` (1024) cells is therefore **not**
bucketed cell-by-cell at all; it is kept in a separate always-scanned
`oversized` set and checked against every query with an exact AABB
intersection test instead. At the default 256 px `cell_size` that threshold
is an ~8192 px square item; at the clamped-minimum `cell_size` of 1.0 it's
~32 px. The query rect itself gets the same treatment — `query` /
`items_in_rect` take an arbitrary caller rect, so a "select everything"
query over a huge area hits the identical hazard on the query side; when
the query rect's own span exceeds the cap, the index scans the populated
cell map directly instead of enumerating the rect's cells, bounded by how
many cells are actually occupied rather than by the rect's area. Both
paths preserve `GridHashIndex::query`'s broad-phase invariant: it may
over-report (a cell-granularity false positive) but must never
under-report — miss an item whose bounds genuinely intersect the query
rect. `Scene::items_in_rect` (and the other collision queries above)
narrow-phase every candidate through their own exact AABB check, so the
over-report never reaches the app; it matters only to a caller that
queries `GridHashIndex` directly.

---

## Magnetism

Magnetism is typed snap-and-connect between anchor points ("magnets") on
scene items. It is general node-graph / diagram machinery: drag an item
so its magnets snap to compatible magnets on other items, drag a wire
from a magnet handle, or connect two magnets from the keyboard, and on
release a connection event carries the magnet payloads to the consumer.

The governing principle is **mechanism in scene, policy in the
consumer**. Scene owns the geometry, the broad-phase, the snap math, the
feedback rendering, the predicate hook, and the connection event. Scene
does **not** own which magnet types are compatible, what a connection
means, or whether connections persist. Compatibility is decided by the
predicate the consumer supplies; the meaning of a connection is decided
by the consumer's `on_connect`.

### The magnet model

A `Magnet` is a local point on an item (in the item's frame, so it
follows the item under any move / rotate / scale), carrying a directional
`MagnetRole` and an optional type-erased payload (`'static`,
downcastable):

```rust
let out = scene.add_magnet(
    node,
    Magnet::new(Point::new(node_w, node_h * 0.5))
        .role(MagnetRole::Source)          // advisory: Source | Target | Bidirectional
        .payload(PortId { node, kind: Out }) // any 'static value
        .label(tr!(node_output())),         // AT name
);
```

`MagnetRole` is generic diagram vocabulary (every node-graph has output
and input ports). It is **advisory** — the scene uses it for default
feedback (which end is the source) and to order the keyboard cycle, but
the predicate is always the authority on whether two magnets connect.

Mutators (all `&self` on `SceneModel`, `&mut self` on `Scene`):
`add_magnet` / `remove_magnet` / `clear_magnets` / `set_magnet_local_pos`
/ `set_magnet_enabled`. Reads: `magnet_ids_of` / `magnet` (a borrow-free
`MagnetRef` snapshot) / `magnet_scene_pos` / `magnet_owner` /
`magnet_enabled`. Removing an item drops its magnets automatically.

### The predicate and the connection event

The predicate is `Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict`, where
`MagnetVerdict` is `Reject` or `Accept(Option<Rc<dyn Any>>)` — "both
payloads in, reject or accept-with-payload out". It runs over owned
magnet snapshots while a shared (read-only) scene borrow is held, so it
may read the model but must not mutate it.

`on_connect` is `Fn(&MagnetConnection, &mut EventContext)`. It fires on
mouse release or keyboard confirm, after every borrow is dropped, so it
may freely mutate the model, declare an AT relation, or fire an intent.

### Three input methods, one mechanism

Install per view via `SceneView::magnetism(MagnetismConfig)`:

```rust
let cfg = MagnetismConfig::new(|a, b| {
        // policy: accept Source -> Target on different items
        if a.item != b.item && /* roles compatible */ true {
            MagnetVerdict::accept()
        } else {
            MagnetVerdict::Reject
        }
    })
    .on_connect(|conn, _ctx| { /* add an edge, fire an intent, … */ })
    .capture_px(14.0)                 // screen-space capture + grab radius
    .markers(MarkerVisibility::DuringInteraction)
    .connect_key(Key::Character('m')); // keyboard connect-mode toggle

let view = SceneView::with_model(model).magnetism(cfg);
```

- **Item-drag-snap** (mouse): drag a lightweight item; its magnets ride
  along and snap to the closest accepting magnet within the capture
  radius; release fires the connection and lands the item snapped.
- **Port-drag wire** (mouse): press directly on a magnet handle to drag a
  transient wire that snaps to a compatible target; release connects, the
  item does not move.
- **Keyboard connect** (any item kind): focus the view, press the connect
  key to enter connect mode, arrow-keys / Home / End move a virtual focus
  through magnets (gated by the predicate once a source is activated),
  Enter activates the source then forms the connection, Esc cancels.

The capture radius is specified in **screen pixels** and divided by the
live zoom, so snapping feels constant at any zoom.

### Feedback

A built-in renderer paints magnet markers (coloured by state, constant
pixel size) plus a connector / ghost wire during an interaction, in the
post-paint pass over the content. Replace it with
`MagnetismConfig::feedback(|canvas, ctx, &MagnetFeedback| …)` for custom
chrome; `MarkerVisibility` (`Always` / `DuringInteraction` / `Never`)
controls when markers show.

### Persistent vs transient — the consumer chooses

Scene stores **no** connection state. It fires the event; the consumer
decides. A node-graph keeps connections as persistent edges (an added
`PathItem` wire, as in the `scene-magnetism` demo); a structural editor
consumes the event once as a reparent and shows containment by nesting.

### Lightweight vs heavyweight

The built-in mouse integration rides the SceneView's lightweight drag /
pointer path (the `RectItem::draggable(true)` substrate), because that is
the only tier the SceneView drags. The keyboard connect flow works for
magnets on any item (it never touches pointer routing). For heavyweight
items (which the SceneView does not drag), originate the drag inside the
item's own widget and call the reusable snap helpers directly:
`SceneModel::compute_item_snap(dragged, drag_delta, capture_radius, &predicate)`
and `compute_port_snap(source, cursor, capture_radius, &predicate)` — the
same mechanism, reachable from any drag origin.

Demo: `cargo run -p scene-magnetism`. Accessibility shaping for magnets
(synthetic nodes + `active_descendant`) is covered in
[docs/bastyde-scene-a11y.md](bastyde-scene-a11y.md).

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

Paint order, bottom to top: `background` → **Under** items → heavyweight
children → **Over** items → marquee → `foreground` → debug overlay. The
`background` hook runs in the SceneView's `paint` (a backdrop, before the
heavyweight children); the `foreground` hook runs in its `post_paint` (after the
children), so it paints over the cards. See [Z-order and paint bands](#z-order-and-paint-bands)
for the Under/Over band and the three-pass model.

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
exposed via [`SceneSelection`](../crates/bastyde-scene/src/selection.rs).

---

## Z-order and paint bands

A `SceneView` paints in three passes — the per-widget `paint → children →
post_paint` model applied at the scene level:

| Pass | What paints | Tier |
| --- | --- | --- |
| `paint` (backdrop) | lightweight **Under** items, z-sorted | lightweight |
| arena child-walk | heavyweight widgets, z-sorted | heavyweight |
| `post_paint` (foreground) | lightweight **Over** items, then the selection marquee / app foreground hook / debug overlays | lightweight |

So the stacking order, bottom to top, is **Under items → heavyweight cards →
Over items**.

### Within a tier

```rust
scene.set_z(id, 5.0);          // higher z paints later (on top)
scene.z(id) -> Option<f32>;
scene.bring_to_front(id);      // z = current max + 1
scene.send_to_back(id);        // z = current min − 1
```

`set_z` works for **both** tiers. Lightweight items re-sort within their band on
the next paint. Heavyweight widget entries restack the arena children on the next
rebuild — the SceneView reorders `node.children` by z *without recreating the
widgets*, so a dragged card keeps its focus, text-edit cursor and in-flight
animations across the restack. Equal-`z` falls back to insertion order (stable).

`bring_to_front` is the drag-to-front primitive: call it on drag-start (via
[`SceneView::scene_mut`]) so the grabbed card — and its text — render over the
others.

### Across the tiers — the Over band

```rust
scene.set_layer(id, SceneLayer::Over);   // raise a lightweight item above the cards
scene.layer(id) -> Option<SceneLayer>;   // Under (default) | Over
```

Lightweight items default to `Under` (background furniture: connector lines,
grids, decorations). `Over` raises an item into the foreground pass so it paints
*above* the heavyweight widgets — selection halos, highlighted connectors,
annotations. Within each band `z` still orders items among themselves.

This is a **binary band, not a continuous z across the tiers**, because the
render walker offers exactly two lightweight paint positions (before and after
the child subtree). The heavyweight tier is one contiguous block in between. To
place a lightweight item *between* two specific cards, promote it to a
heavyweight widget and give it a z between theirs.

### Nested P-C-AP — a node is one widget

The `paint → children → post_paint` model is per-widget and *nests*. A scene's
bands are for **furniture** (connectors, the nodes-as-units, the lasso); each
**node** is itself a P-C-AP scope — its `paint` draws the container, its children
are the text. **Keep a node whole: build it as one heavyweight widget; never
split its container into the lightweight tier and its text into the heavyweight
tier.** The render walker paints each heavyweight child's entire subtree
atomically, so a node ordered last paints its container *and* its text on top of
the node beneath — drag-to-front "with text included" is structural, not
something you arrange. Splitting a node across tiers tears it: every container
would sit in one band and every text in the band above, so a raised card's
neighbour would have its text leak on top of it.

### Hit-testing irregular nodes

Z-order is paint-only; it does not change hit-test priority between tiers
(heavyweight widgets win heavyweight-vs-lightweight collisions). For a node whose
visible shape isn't its bounding box — an ellipse, a cloud — override
`Widget::hit_shape` so a click lands on the silhouette you see, not the
rectangle. Returning `false` for an in-bounds point makes the click fall through
to whatever node is painted underneath; this mirrors the lightweight tier's
`SceneItem::shape_contains`.

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

`remove` also cleans the logical-AT maps for the removed item(s) — parents,
relations, live, landmarks, categories — and re-roots any still-alive node
that was AT-parented under a removed item, so the separate AccessKit tree
never carries a dangling reference. See *Runtime mutation* below.

---

## Shared model & multi-view

A `Scene` lives behind a cloneable [`SceneModel`] handle — `Rc<RefCell<Scene>>`,
the same share-by-handle pattern as `bastyde-data`'s `ListModel`. Clone the
handle into several `SceneView::with_model(model.clone())` panes to render **one
scene many ways**: an overview + a detail pane, the same document in two
windows, or a headless model a tool mutates with no view at all. Mutate the
model once and **every** attached view reconciles.

```rust
let model = SceneModel::new();
let id = model.add_widget_item(CardData { /* … */ }, rect);   // a typed payload

let editor = SceneView::with_model(model.clone())
    .delegate_typed::<CardData>(|card, id| build_card(card, id));
let overview = SceneView::with_model(model.clone())   // same model, own camera
    .delegate_typed::<CardData>(|card, id| build_card(card, id));

// Later, from any handler holding a clone — no `with_widget_mut`:
model.set_payload(id, CardData { /* … */ });   // both panes rebuild that card
```

### Heavyweight content: payload + per-view delegate

A heavyweight `Widget` instance lives in exactly one arena, so a shared model
can't hand the *same* `Box<dyn Widget>` to two views. Two ways to add one:

- **Single-view** — `model.add_widget(widget, rect)` (or `Scene::add_widget`)
  stores the instance in a one-shot slot, drained by the **first** view that
  builds. A second view sharing the model produces no child for it. Use it when
  the scene has exactly one view.
- **Multi-view** — `model.add_widget_item(payload, rect)` stores a type-erased
  `payload` (any `'static` type). Each view supplies a delegate —
  `.delegate_typed::<P>(|&P, ItemId| -> Box<dyn Widget>)` (downcasts;
  debug-asserts on a type mismatch) or the untyped
  `.delegate(|&dyn Any, ItemId| -> Box<dyn Widget>)` — and builds its **own**
  instance per item. `model.set_payload(id, new)` replaces the data and
  re-invokes the delegate for that item in every view (so a card with transient
  widget state — caret, focus — should bind a `Signal` for those fields rather
  than rely on the rebuild).

Lightweight `SceneItem`s (`add_item`) are shared automatically — painted
read-only from each view's paint walk, so no per-view instance is needed.

### Selection across panes

Selection is **per-view by default**. To sync panes, build a `SceneSelection`
and pass a clone to each view via `.selection_model(sel.clone())`; capture the
same `sel.selection_signal()` in your delegate so each card derives its
highlight reactively — selecting in one pane repaints the border in every pane,
with no rebuild. (`SceneSelection` is itself a cheap-clone shared handle.)

### Single-view ergonomics

`SceneView::new(scene)` still takes a `Scene` by value (it wraps a fresh
`SceneModel` internally); `view.scene()` / `view.scene_mut()` return borrow
guards for ad-hoc single-view access; `view.model()` hands out the shared handle.

---

## `SceneListAdapter` — sync items from a `ListModel`

[`SceneListAdapter<T>`](../crates/bastyde-scene/src/scene_list_adapter.rs)
keeps a run of lightweight `SceneItem`s in lock-step with a
`bastyde_data::ListModel<T>` (or any `ListDataSource<Item = T>`), so a
data-driven collection of dots / markers / cards doesn't need hand-rolled
reconciliation against `DataChange`. It is a plain non-`Widget` struct —
construct it once, hold onto it, and it does the rest via an internal
`ObserverHandle`.

```rust
use bastyde_scene::SceneListAdapter;

let model = SceneModel::new();
let adapter = SceneListAdapter::from_model(&list_model, model.clone(), |row, _index| {
    Box::new(RectItem::new(Rect::new(0.0, 0.0, 12.0, 12.0)).fill(row.color)) as Box<dyn SceneItem>
});

let id = adapter.item_id_at(0);   // scene ItemId for row 0, if materialised
```

The delegate is `Fn(&T, usize) -> Box<dyn SceneItem>`; the returned item
positions itself via its own `local_bounds` — the adapter inserts it at the
scene origin through
[`Scene::add_boxed_item`](../crates/bastyde-scene/src/scene.rs) (the
boxed-`dyn` counterpart of `add_item`, needed because a trait-object item
can't go through the generic `add_item<I: SceneItem>`). On construction the
adapter materialises every current row; afterwards it reconciles from the
model's `DataChange` stream: a structural change (insert / remove / move /
reset) rebuilds every adapter-owned item (simple and always correct), an
`ItemUpdated` rebuilds just that one row's item in place, and a lazy-loading
source's `WindowLoaded` rebuilds only the newly-loaded range. `item_id_at`,
`ids`, `len`, `is_empty` read the current index → `ItemId` mapping; `clear`
removes every adapter-owned item from the scene. Dropping the adapter stops
observing the model but does **not** remove its items — call `clear()` first
if you want them gone.

Use `SceneListAdapter::from_source` instead of `from_model` to drive the
same reconciliation off a custom `ListDataSource<Item = T>` (the escape
hatch for huge / external sources) rather than an in-memory `ListModel<T>`.

## Runtime mutation (after mount)

The cleanest way to mutate a mounted scene is through the shared [`SceneModel`]
handle: every mutator is `&self`, so a handler holding `view.model()` (a cheap
clone) drives the scene directly and **all** views reconcile — no
`with_widget_mut` needed for content:

```rust
let model = view.model();             // a clone captured in the handler
let act = model.add_a11y_group(A11yGroup::builder().label(lit!("Act IV")));
model.set_a11y_live(A11yNode::Group(act), Live::Polite);
let card = model.add_widget_item(CardData { /* … */ }, rect);
model.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act)));
```

`with_widget_mut` remains the channel for **per-view** state a handler can't
otherwise reach — e.g. animating one pane's camera:

```rust
ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Relayout, |view| {
    view.ensure_visible(rect, 40.0);
});
```

Each view **self-reconciles** on every scene mutation — visual *and*
accessibility:

- **Add** (`add_widget_item` / `add_widget` / `add_item`) materialises into the
  arena on the next rebuild; the spatial index already holds it from insertion.
- **Payload change** (`set_payload`) re-invokes the delegate for that item in
  every view, rebuilding its widget with the new data.
- **Remove** (`remove`) destroys the orphaned arena widget (no leak), drops it
  from the materialised maps, and cleans the logical-AT maps.
- **Move / transform / reparent / visibility / opacity / z / layer** — every
  `ItemChange` variant drives a reconcile pass, so paint *and* the
  screen-projected AccessKit bounds follow.
- **Pure-a11y mutations** (`add_a11y_group`, `set_a11y_parent`, relations,
  live, landmark, categories) don't change item geometry, so they ride a
  *separate* `Scene::a11y_change_signal` — the AccessKit tree still re-walks.

A relayout no longer re-walks the AccessKit tree on its own (it's gated on
`a11y_dirty`), so `SceneView::build()` calls `ctx.request_accessibility_update()`
when it reconciles — the lever that keeps assistive tech in lock-step with the
visual scene. The call is **gated on a mutation-version delta**: `build()`
re-walks AT only when [`Scene::mutation_version`] advanced since the last walk
(any add / remove / move / reparent / visibility / a11y change). A `build()`
driven *purely* by a per-frame `add_item_dynamic` animation does **not** re-walk
AT every frame — re-walking 60×/s for sub-pixel bounds drift is waste a screen
reader can't use — but when that animation **settles**, the final bounds are
walked into AT exactly once. Discrete mutations always re-walk, even interleaved
with an animation. Demo: `cargo run -p scene-corkboard` ("Add Act").

### App-owned view state

Pan / zoom / rotation default to view-owned signals. Inject app-owned ones with
`view_state(pan_x, pan_y, zoom, rotation)` so view state survives a
rebuild-from-state, a "Reset View" button can snap it home, and a toolbar can
read it. `initial_pan` / `initial_zoom` / `initial_rotation` seed starting
values without giving up ownership. These builders run pre-mount, like the
others.

---

## i18n

User-visible strings on `SceneItem` builders (`label`, `tooltip`,
`access_label`, `access_description`, `A11yGroupBuilder::label`,
`SceneView::a11y_label`, `TextItem::new`) accept `impl Into<LocalizedString>`.
Pass the result of `tr!(...)` directly:

```rust
RectItem::new(rect).access_label(tr!(save_card()))
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

These `scene_mut()` calls run **pre-mount** — the app still owns `view`. To
mutate the same scene from a handler *after* the view is added to the tree, go
through `ctx.with_widget_mut::<SceneView>(view_id, …)` (see *Runtime mutation*
above); the live `scene-corkboard` example does exactly that for its "Add Act"
button.

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
    TextItem::new(tr!(node_name()), Rect::new(8.0, 8.0, 100.0, 24.0)),
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

- Implementation: [`crates/bastyde-scene/src/`](../crates/bastyde-scene/src/)
- Accessibility-shaping API: [`docs/bastyde-scene-a11y.md`](bastyde-scene-a11y.md)
- Showcase demo: `cargo run -p scene-showcase`
- Corkboard demo: `cargo run -p scene-corkboard`
