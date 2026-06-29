<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scene

The `Scene` data model — the owner of all items in a pannable/zoomable
scene.

`Scene` holds a flat list of entries in a parent-relative scene-graph, plus
a pluggable `SpatialIndex` for rectangular queries. Items are positioned
by `local_pos` (in their parent's coordinate frame, or scene-root if they
have none) and an optional `transform` (rotation/scale around the local
origin); the Scene composes those up the parent chain to derive each item's
`scene_transform` and axis-aligned bounding box for hit-test, paint, and
culling. Two content tiers coexist in one `Scene`: heavyweight `Widget`s
(full focus/animation/DnD/AT — placed at scene coordinates) and lightweight
`SceneItem`s (paint-only, no arena overhead, thousands
cheap). All mutations update the `SpatialIndex` in lockstep, so
`Scene::items_in_rect` and `Scene::item_at` stay `O(visible)`.

`Scene` is rarely used directly. The normal entry point is
`SceneModel`, a cloneable `Rc<RefCell<Scene>>` handle
with `&self` mutators (the `ListModel` pattern) that lets multiple handlers
and multiple `SceneView`s share one model.

## When to use

Use `Scene` (via `SceneModel`) when you need a pannable/zoomable canvas —
story corkboards, node-graph editors, mind maps, timeline views, CAD
canvases, or simple spatial maps. Prefer a plain `ListView` or `TreeView`
when the content is linear or tree-shaped without spatial relationships.

## Example

```rust
use bastyde_scene::{Scene, ItemChange, SceneLayer};
use bastyde_scene::{RectItem, ItemId};
use bastyde_canvas::{Point, Rect};
use bastyde_tokens::Color;

let mut scene = Scene::new();

// Add a lightweight rectangle item at scene coordinates (50, 50).
let id: ItemId = scene.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 80.0, 40.0)).fill(Color::BLUE),
    Point::new(50.0, 50.0),
);

// Observe every mutation — fires after the change is already applied.
let _guard = scene.item_change_signal().observe(|change| {
    if let ItemChange::LocalPosChanged { id: _, old: _, new } = change {
        let _ = new; // react to the new position
    }
});

// Move the item; the observer fires and the spatial index updates.
scene.set_local_pos(id, Point::new(100.0, 100.0));
assert_eq!(scene.scene_pos(id), Some(Point::new(100.0, 100.0)));
```

## Builder methods at a glance

`with_index`, `add_widget`, `add_item`, `add_item_dynamic`, `refresh_dynamic_bounds`, `item_change_signal`, `a11y_change_signal`, `mutation_version`, `local_pos`, `set_local_pos`, `local_bounds`, `set_local_bounds`, `transform`, `set_transform`, `scene_transform`, `scene_pos`, `scene_rect`, `map_to_scene`, `map_from_scene`, `flags`, `set_flags`, `set_flag`, `set_visible`, `is_effectively_visible`, `opacity`, `set_opacity`, `set_item_handlers`, `handlers_mut`, `handlers`, `effective_opacity`, `set_scene_rect`, `scene_rect_extent`, `pan_axes`, `current_pan_axes`, `zoomable`, `is_zoomable`, `set_pan_bounds`, `current_pan_bounds`, `set_zoom_range`, `current_zoom_range`, `pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`, `zoomable_signal`, `constraints`, `set_z`, `bring_to_front`, `send_to_back`, `z`, `set_layer`, `layer`, `set_item_parent`, `parent_of`, `is_descendant_of`, `collect_descendants`, `item`, `remove`, `orphan`, `items_in_rect`, `item_thumbnails`, `item_at`, `colliding_items`, `items_along_path`, `items_at`, `len`, `is_empty`, `ids`, `index`, `add_magnet`, `remove_magnet`, `clear_magnets`, `set_magnet_local_pos`, `set_magnet_enabled`, `magnet_ids_of`, `magnet_owner`, `magnet_enabled`, `magnet_scene_pos`, `magnet`, `compute_item_snap`, `compute_port_snap`, `nearest_magnet`, `add_a11y_group`, `remove_a11y_group`, `a11y_group`, `set_a11y_parent`, `a11y_parent_of`, `add_a11y_relation`, `a11y_relations`, `set_a11y_live`, `set_a11y_landmark`, `set_a11y_categories`, `a11y_categories_of`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_scene/index.html)

## `pub enum ItemChange`

A change to an item's state, fired through
`Scene::item_change_signal` for every mutation. Apps observe
to wire snap-to-grid, validation, side effects, etc. The model
is "fire after the change has been applied" — by the time the
observer sees the event, the Scene already reflects it.

```rust
pub enum ItemChange { /* variants */ }
```

### Variants

- **`LocalPosChanged`** — `set_local_pos`: position in parent coords moved.
- **`LocalBoundsChanged`** — `set_local_bounds`: AABB in local coords changed.
- **`TransformChanged`** — `set_transform`: local→parent transform changed.
- **`VisibilityChanged`** — `set_visible` flipped IS_VISIBLE.
- **`FlagsChanged`** — `set_flags` / `set_flag` changed the bitset.
- **`OpacityChanged`** — `set_opacity`: local opacity multiplier changed.
- **`ZChanged`** — `set_z`: paint z-order changed.
- **`LayerChanged`** — `set_layer`: the Under/Over paint band changed.
- **`ParentChanged`** — `set_item_parent`: logical parent changed.
- **`Removed`** — `remove`: item is gone.
- **`Added`** — `add_item` / `add_widget`: item was inserted.
- **`PayloadChanged`** — `set_payload`: the type-erased payload of a `Delegated` heavyweight entry was replaced. A `SceneView` rebuilds that entry's widget (re-invokes its delegate) on the next build. Routed through `emit_item_change`, so `mutation_seq` advances and the AT-walk gate notices.

## `pub enum SceneLayer`

Which paint band a lightweight `SceneItem` sits in, relative to
the heavyweight widget tier.

A `SceneView` paints in three passes: lightweight `Under` items
(its `paint`, a backdrop), then the heavyweight widget children
(the arena child-walk), then lightweight `Over` items (its
`post_paint`, a foreground). Within each band, `z` still orders
items among themselves.

This is a binary band, not a continuous z across the tiers, because
the render walker offers exactly two lightweight paint positions
(before and after the child subtree). The heavyweight tier is one
contiguous block in between — to interleave a lightweight item
*between* two specific heavyweight nodes you must promote it to a
heavyweight widget. `Under` is the default (background furniture:
connectors, grids, decorations); `Over` is for foreground overlays
that must sit above the cards (selection halos, highlighted edges).

```rust
pub enum SceneLayer { /* variants */ }
```

### Variants

- **`Under`** — Painted under the heavyweight widget children (the default).
- **`Over`** — Painted over the heavyweight widget children.

## `pub enum PanAxes`

Which axes a `SceneView` is allowed to pan
along. Set on the `Scene` (not the View) because a given scene
model often makes sense at one orientation only — a horizontal
timeline, a vertical timeline, a fixed-extent diagram. All views
of the same scene inherit the constraint.

```rust
pub enum PanAxes { /* variants */ }
```

### Variants

- **`None`** — No user-driven pan in either axis. Programmatic `SceneView::set_pan` / `pan_to` become no-ops too.
- **`Horizontal`** — Pan only along X. Vertical scroll deltas pass through to ancestor scrollables.
- **`Vertical`** — Pan only along Y. Horizontal scroll deltas pass through to ancestor scrollables.
- **`Both`** — Default: pan freely in both axes.

## `pub struct SceneConstraints`

Reactive interaction-policy bundle owned by `Scene`. Apps
configure pan/zoom behaviour by writing to these signals; gesture
closures in `SceneView` read them live, so
runtime mode switches (e.g. a toolbar toggling pan locks) take
effect on the next event without rebuilding the view.

All four signals are exposed individually via `Scene` accessors
(`pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`,
`zoomable_signal`). Per-(sub-)scene independence falls out of the
model: each nested `SceneView` carries its own `Scene` with its
own `SceneConstraints`.

View-level *tightening* overrides (`pan_bounds_override`,
`zoom_range_override`) layer on top per-`SceneView` — the
effective constraint is the intersection. Two views over the
same `Scene` can lock down independently; neither can loosen
what the `Scene` declares.

```rust
pub struct SceneConstraints { /* fields */ }
```

### Methods

#### `pub fn pan_axes_signal(&self) -> Signal<PanAxes>`

Reactive pan-axes signal. Gesture handlers read live.

#### `pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>>`

Reactive pan-bounds signal. `None` = unconstrained.

#### `pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive zoom-range signal. `None` = unconstrained from
the Scene side.

#### `pub fn zoomable_signal(&self) -> Signal<bool>`

Reactive zoomable-on/off signal. Equivalent to a zero-width
zoom_range — kept as a separate boolean for clarity and
efficient short-circuit at gesture time.

## `pub struct Scene`

The data model behind a `SceneView`: a flat list of entries in a
parent-relative scene-graph plus a `SpatialIndex` for rectangular
queries.

The Scene itself does no rendering — it's a passive container the view
reads from at build / place / paint time. Mutations (`add_widget`,
`add_item`, `set_local_pos`, `set_transform`, `set_local_bounds`, `remove`)
update the spatial index in lockstep, so `items_in_rect`, `item_at`, and
SceneView's viewport-cull path are all `O(visible)` instead of `O(N)`. When
a parent's `local_pos` or `transform` changes, every descendant's
scene-AABB shifts; the Scene re-buckets the entire subtree.

In practice most callers operate on a `SceneModel`
handle (`Rc<RefCell<Scene>>` with `&self` mutators) rather than a bare
`Scene`. Prefer `SceneModel` for any widget or handler that needs to share
the scene across multiple owners.

```rust
pub struct Scene { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

An empty scene with the default `GridHashIndex`.

#### `pub fn with_index(index: Box<dyn SpatialIndex>) -> Self`

An empty scene with a custom `SpatialIndex`.

#### `pub fn add_widget<W: Widget + 'static>(&mut self, widget: W, local_rect: Rect) -> ItemId`

Place a heavyweight `Widget` at `local_rect`'s origin, sized
`local_rect.size`. The rect is interpreted as
`(local_pos = local_rect.origin, local_bounds = (0, 0, w, h))`.
Returns the `ItemId` for later mutation. The widget is
consumed at SceneView build time and added to the arena.

#### `pub fn add_item<I: SceneItem + 'static>(&mut self, item: I, local_pos: Point) -> ItemId`

Place a lightweight `SceneItem` at `local_pos`. The item's
`local_bounds` and `initial_flags` are read once at insert
time. The item is **not** added to the arena — it's painted
directly from `SceneView::paint`.

#### `pub fn add_item_dynamic<I: SceneItem + 'static>( &mut self, item: I, local_pos: Point, ) -> ItemId`

Like `add_item` but flags the entry as
having signal-driven `local_bounds`. The Scene re-reads
`item.local_bounds()` each rebuild via
`refresh_dynamic_bounds` — the
SceneView calls that at the start of every build pass. The
spatial index gets re-bucketed when the read-back differs
from the cached value, so `items_in_rect` / hit-test stay
correct without app-side `set_local_bounds` plumbing.

Use only when the bounds genuinely depend on a `Signal<T>`
the item reads in `local_bounds`. Static items pay an
unnecessary per-rebuild bounds read otherwise; prefer
`add_item` for the common case.

#### `pub fn refresh_dynamic_bounds(&mut self) -> bool`

Re-read every dynamic item's current `local_bounds`, applying
`set_local_bounds` (and re-bucketing the spatial index) for
any entry whose value has changed. No-op for static entries.
Called by `SceneView` at the start of each
`build()` so signal-driven bounds propagate to bucketing
without explicit app-side calls.

Returns `true` if at least one dynamic entry's bounds changed this call.
`SceneView` uses the `true → false` transition (an animation settling) as
the one moment to walk the final animated bounds into the AccessKit tree,
since it otherwise suppresses per-frame AT re-walks during the animation.

#### `pub fn item_change_signal(&self) -> Signal<ItemChange>`

Reactive notification stream for every Scene mutation. Apps
observe via `signal.observe(|change| …)` to wire snap-to-grid,
clamping, validation, and side effects without having to
poll the Scene each frame. The signal fires *after* the
mutation has been applied — by the time the observer runs
the Scene already reflects the new state.

#### `pub fn a11y_change_signal(&self) -> Signal<u64>`

Reactive notification for logical-AT-structure mutations
(`add_a11y_group` / `remove_a11y_group` / `set_a11y_parent` /
`add_a11y_relation` / `set_a11y_live` / `set_a11y_landmark` /
`set_a11y_categories`). A monotonic counter bumped after each such
mutation. `SceneView` observes this to re-walk the AccessKit tree —
these changes don't flow through `item_change_signal`
because they aren't item geometry, and the AT tree is separate from the
visual scene.

#### `pub fn mutation_version(&self) -> u64`

Monotonic counter of every model mutation applied so far — item geometry
/ visibility / structure (each `ItemChange`) **and** logical-AT
structure (groups, parents, relations, live, landmarks, categories).

`SceneView` snapshots this each `build()` and only
re-walks the (separate, expensive) AccessKit tree when it has advanced
since the previous walk — so an actively-animating
`add_item_dynamic` item, which rebuilds every
frame, does not issue an AT re-walk per frame. The counter wraps; compare
for equality, not ordering.

#### `pub fn local_pos(&self, id: ItemId) -> Option<Point>`

Read an item's `local_pos` (its anchor in parent coords).

#### `pub fn set_local_pos(&mut self, id: ItemId, local_pos: Point)`

Move an item to a new `local_pos` in its parent's coordinate
frame. Re-buckets the item *and* every descendant in the
spatial index since the descendants' scene-AABBs shift along.
No-op if the id is unknown.

#### `pub fn local_bounds(&self, id: ItemId) -> Option<Rect>`

Read an item's `local_bounds` (its AABB in local coords).

#### `pub fn set_local_bounds(&mut self, id: ItemId, local_bounds: Rect)`

Update an item's `local_bounds`. For lightweight items this
also calls `SceneItem::set_local_bounds` on the item so its
next `paint` reflects the new geometry. The spatial index is
re-bucketed; only this item moves (descendants' local frames
are unchanged). No-op if the id is unknown.

#### `pub fn transform(&self, id: ItemId) -> Option<Transform2D>`

Read an item's local→parent transform (rotation/scale around
the local origin). Identity by default.

#### `pub fn set_transform(&mut self, id: ItemId, transform: Transform2D)`

Set an item's local→parent transform. Re-buckets the item's
subtree in the spatial index. No-op if the id is unknown.

#### `pub fn scene_transform(&self, id: ItemId) -> Transform2D`

The composed local→scene transform for this item, walking up
the parent chain. Identity for an item that doesn't exist.

#### `pub fn scene_pos(&self, id: ItemId) -> Option<Point>`

The item's anchor in scene coords (its local origin
transformed through the parent chain).

#### `pub fn scene_rect(&self, id: ItemId) -> Option<Rect>`

The AABB enclosing the item's `local_bounds` after composing
through the parent chain — i.e. the rectangle the spatial
index buckets on. `None` if the id is unknown.

#### `pub fn map_to_scene(&self, id: ItemId, local_pt: Point) -> Option<Point>`

Map a point in the item's local frame to scene coords.

#### `pub fn map_from_scene(&self, id: ItemId, scene_pt: Point) -> Option<Point>`

Map a point in scene coords to the item's local frame.
Returns `None` if the item is unknown or its scene transform
is degenerate (zero scale).

#### `pub fn flags(&self, id: ItemId) -> Option<ItemFlags>`

Read an item's `ItemFlags` bitset.

#### `pub fn set_flags(&mut self, id: ItemId, flags: ItemFlags)`

Replace an item's flags wholesale. No-op if unknown.

#### `pub fn set_flag(&mut self, id: ItemId, flag: ItemFlags, on: bool)`

Set or clear a single flag on an item. No-op if unknown.

#### `pub fn set_visible(&mut self, id: ItemId, visible: bool)`

Toggle the `ItemFlags::IS_VISIBLE` bit. Convenience for
the common "hide this item" operation.

#### `pub fn is_effectively_visible(&self, id: ItemId) -> bool`

Whether the item is visible AND every ancestor in its chain
is visible. Returns `true` when nothing in the chain has
`IS_VISIBLE` cleared. `false` for unknown ids.

#### `pub fn opacity(&self, id: ItemId) -> Option<f32>`

Read an item's local opacity multiplier (`1.0` by default).

#### `pub fn set_opacity(&mut self, id: ItemId, opacity: f32)`

Set an item's local opacity, clamped to `[0.0, 1.0]`.

#### `pub fn set_item_handlers(&mut self, id: ItemId, handlers: Option<SceneItemHandlerSet>)`

Replace an item's handler set. Pass `None` to clear.

#### `pub fn handlers_mut(&mut self, id: ItemId) -> Option<&mut SceneItemHandlerSet>`

Mutably borrow an item's handler set, lazily creating an
empty one if none exists. Returns `None` for unknown ids.
Allows fluent chains: `scene.handlers_mut(id).unwrap().on_tap(…).cursor(…);`.

#### `pub fn handlers(&self, id: ItemId) -> Option<&SceneItemHandlerSet>`

Read-only access to an item's handler set, if one is set.

#### `pub fn effective_opacity(&self, id: ItemId) -> f32`

Effective opacity composed up the parent chain — the product
of every ancestor's opacity and this item's. `1.0` for an
unknown id (so callers don't end up multiplying by a stale
value).

#### `pub fn set_scene_rect(&mut self, rect: Option<Rect>)`

Declare the scene's logical extent. `None` (the default)
means "auto-compute from items each query"; `Some(rect)`
fixes the extent regardless of item placement. Used by
`SceneView` for pan clamping and `fit_to_content`.

#### `pub fn scene_rect_extent(&self) -> Option<Rect>`

The resolved scene extent — user-declared via
`Scene::set_scene_rect` if set, otherwise the AABB
enclosing every item's scene rect. `None` when neither is
available (the user didn't declare and the scene is empty).

#### `pub fn pan_axes(&mut self, axes: PanAxes)`

Set the axes the view may pan along. Default
`PanAxes::Both`. Writes to the reactive signal; gesture
closures pick the change up on the next event.

#### `pub fn current_pan_axes(&self) -> PanAxes`

The currently-declared pan axes. Live read of the signal.

#### `pub fn zoomable(&mut self, on: bool)`

Set whether the view honors zoom gestures. Default `true`.
Writes to the reactive signal.

#### `pub fn is_zoomable(&self) -> bool`

Whether the scene currently allows zoom. Live read.

#### `pub fn set_pan_bounds(&mut self, bounds: Option<Rect>)`

Clamp the visible viewport to this scene-coord rect. `None`
(default) leaves pan unconstrained. When `Some(r)`, the
`SceneView`'s pan is clamped so the
visible scene region overlaps `r`. When `r` is smaller than
the visible viewport, the rect is centered.

Distinct from `set_scene_rect`:
`scene_rect` declares the scene's logical extent (used by
`adopt_scene_size`); `pan_bounds` controls what region the
user can scroll to. A doc-style app typically sets both to
the same rect.

#### `pub fn current_pan_bounds(&self) -> Option<Rect>`

The currently-declared pan-bounds rect. Live read.

#### `pub fn set_zoom_range(&mut self, range: Option<std::ops::RangeInclusive<f32>>)`

Inclusive `[min, max]` zoom-factor clamp. `None` (default)
is unconstrained from the `Scene` side — the `SceneView`
may still impose its own override.

The effective range applied by the `SceneView` is the
intersection of `Scene` + view-level override, so apps
cannot loosen a `Scene`-declared range by setting a wider
override on the view.

#### `pub fn current_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>>`

The currently-declared zoom range. Live read.

#### `pub fn pan_axes_signal(&self) -> Signal<PanAxes>`

Reactive accessors for live observation.

#### `pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>>`

Reactive pan-bounds signal.

#### `pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive zoom-range signal.

#### `pub fn zoomable_signal(&self) -> Signal<bool>`

Reactive zoomable on/off signal.

#### `pub fn constraints(&self) -> &SceneConstraints`

Read-only view of the full constraint bundle. Useful when
passing all four signals to a custom view implementation.

#### `pub fn set_z(&mut self, id: ItemId, z: f32)`

Set paint z-order for an entry. Higher z paints later (on top);
equal-z falls back to insertion order. Default 0.0.

Works for **both** tiers: lightweight items re-sort within their
band on the next paint, and heavyweight widget entries restack the
arena children on the next rebuild (the SceneView reorders
`node.children` by z without recreating the widgets, so focus /
text-edit / animation state survives the restack). No-op for
unknown ids.

#### `pub fn bring_to_front(&mut self, id: ItemId)`

Raise an entry above all current entries by giving it a z one
greater than the current maximum. The drag-to-front primitive —
call it on drag-start so the grabbed card (and its text) renders
over the others. Works for both tiers (see `set_z`).

#### `pub fn send_to_back(&mut self, id: ItemId)`

Lower an entry below all current entries by giving it a z one less
than the current minimum. Works for both tiers (see
`set_z`).

#### `pub fn z(&self, id: ItemId) -> Option<f32>`

Read an entry's z-order.

#### `pub fn set_layer(&mut self, id: ItemId, layer: SceneLayer)`

Set the Under/Over paint band for a lightweight entry. `Over`
items paint *after* the heavyweight widget children (in the
SceneView's `post_paint`), so they sit on top of the cards;
`Under` items (the default) paint before them. Within a band,
`set_z` still orders items among themselves.
No-op for unknown ids.

#### `pub fn layer(&self, id: ItemId) -> Option<SceneLayer>`

Read an entry's Under/Over paint band. `None` for unknown ids.

#### `pub fn set_item_parent(&mut self, child: ItemId, parent: Option<ItemId>)`

Declare a parent/child relationship. `child`'s `local_pos`
and `transform` are reinterpreted as relative to the new
parent's local frame — the visual position changes unless
the caller compensates. Re-buckets `child`'s subtree.

Pass `parent = None` to detach (child's local frame becomes
scene-rooted again).

**Cycle guard:** if the proposed parent is `child` itself
or a descendant of `child`, the call is a no-op (no parent
change, no rebucket, no signal fire). Without this guard
the downstream `rebucket_subtree` walk loops indefinitely.

#### `pub fn parent_of(&self, id: ItemId) -> Option<ItemId>`

Parent of `id`, if any.

#### `pub fn is_descendant_of(&self, id: ItemId, ancestor: ItemId) -> bool`

Whether `id`'s ancestor chain contains `ancestor`.

#### `pub fn collect_descendants(&self, id: ItemId, out: &mut Vec<ItemId>)`

Append every direct + transitive descendant of `id` into
`out`, breadth-first across declaration order. The id
itself is **not** included.

#### `pub fn item(&self, id: ItemId) -> Option<&dyn SceneItem>`

Borrow a lightweight `SceneItem` by id. `None` for unknown
ids and for heavyweight widget entries.

#### `pub fn remove(&mut self, id: ItemId)`

Remove an item by id, recursively dropping every descendant.

Mirrors Qt's `QGraphicsScene::removeItem` semantics: deleting
a parent deletes its children too. No-op if `id` is unknown.
Fires one `ItemChange::Removed` per id, descendants first
then the named parent — observers see a consistent
"leaves-then-root" order.

To remove `id` without deleting its children, call
`Scene::orphan` first to promote them to root-level, then
`remove(id)`.

#### `pub fn orphan(&mut self, id: ItemId)`

Promote `id`'s direct children to root-level (clear their
`parent` field). Used when an app wants to remove `id` without
dropping its children — call `orphan(id)` then `remove(id)`.
No-op when `id` is unknown or has no children.

Fires one `ItemChange::ParentChanged` per detached child and
re-buckets every detached subtree in the spatial index — the
children's `scene_transform` shifts (no longer composes
`id`'s) so their scene-space AABBs change. Without re-bucketing
the index, `items_in_rect` and
`item_at` would return stale results.

Apps wanting *visual* stability across the orphan call should
first bake `id`'s `scene_transform` into each child's
`local_pos` + `transform`; otherwise children visibly jump.

#### `pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId>`

All items whose scene-AABB intersects `scene_rect`.

Broad phase: the spatial index returns every id bucketed in
any cell touched by `scene_rect`. Narrow phase: each candidate
goes through `scene_rect`, which itself
dispatches via `entry_index` (an `HashMap<ItemId, usize>`),
so the per-candidate cost is O(parent-chain-depth) — not
O(N). Total query is O(visible × chain) instead of O(N).

#### `pub fn item_thumbnails(&self) -> Vec<(Rect, bastyde_tokens::Color)>`

Snapshot every visible item — **both tiers** — as a `(scene_rect,
color)` pair suitable for a minimap thumbnail. Filters out items with
`HAS_NO_CONTENTS` (logical-only) and items hidden by `IS_VISIBLE` / a
hidden ancestor — the visible-effective set matches what the SceneView's
paint walk renders.

Ordered by insertion (low z first). A lightweight item's color comes
from `SceneItem::thumbnail_color` (its fill / stroke / a neutral grey);
a heavyweight widget entry has no `SceneItem`, so it's shown in a neutral
tint — a minimap that omitted the heavyweight tier would misrepresent a
widget-heavy scene (cards, nodes), so both tiers are included.

#### `pub fn item_at(&self, scene_pt: Point) -> Option<ItemId>`

Topmost lightweight item whose `shape_contains` fires for
`scene_pt`. Iterates `items_in_rect` for a tiny rect around
the point, sorts by z descending, and returns the first hit.
Heavyweight widget entries are skipped (their hit-testing is
handled by the arena event dispatch).

**Limitation:** items flagged
`IGNORES_TRANSFORMATIONS`
hit-test in screen space, not scene space — so this scene-only
query may incorrectly hit them or miss them depending on the
current view transform. Apps that route pointer events through
`SceneView`'s dispatch get screen-space hit-test for IGNORES
items automatically; only use `item_at` directly for normal
items, or pair with the view transform to filter.

#### `pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>`

Items whose scene-AABB intersects the AABB of `id`. Excludes
`id` itself. Apps use this for "which other items overlap
this card?" queries — graph editors checking node-on-node
overlap, CAD canvases finding adjacent geometry. Backed by
the spatial index, so the cost is `O(visible)` not `O(N)`.

#### `pub fn items_along_path(&self, path: &Path) -> Vec<ItemId>`

Items whose scene-AABB intersects `path`'s bounding rect.
Apps use this for "which items lie under this connector?"
queries — graph editors highlighting hovered connectors,
CAD canvases doing point-in-polygon style picking. The
narrow phase is AABB-vs-AABB; per-segment-distance precision
is left to the app.

#### `pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>`

All lightweight items whose `shape_contains` fires for
`scene_pt`, sorted topmost-first by z.

#### `pub fn len(&self) -> usize`

Number of entries in the scene.

#### `pub fn is_empty(&self) -> bool`

Whether the scene is empty.

#### `pub fn ids(&self) -> Vec<ItemId>`

All ids in insertion order.

#### `pub fn index(&self) -> &dyn SpatialIndex`

Borrow the spatial index (diagnostics / tests).

#### `pub fn add_magnet(&mut self, item: ItemId, magnet: Magnet) -> MagnetId`

Attach a `Magnet` to `item` and return its `MagnetId`.

Magnets are local to their item (their `local_pos` is in the
item's frame), so they follow the item under any move / rotate /
scale via the same `scene_transform` the item uses. No-op
returning a fresh-but-unowned id if `item` is unknown — callers
add magnets to items they just created.

Bumps the AT-structure change counter (magnets are AT structure)
so a `SceneView` with magnetism enabled re-walks its synthetic
magnet nodes.

#### `pub fn remove_magnet(&mut self, magnet: MagnetId)`

Remove a magnet by id. No-op if the id is unknown.

#### `pub fn clear_magnets(&mut self, item: ItemId)`

Remove every magnet attached to `item`. No-op if none.

#### `pub fn set_magnet_local_pos(&mut self, magnet: MagnetId, local_pos: Point)`

Move a magnet to a new position in its owning item's local
frame. No-op if the id is unknown.

#### `pub fn set_magnet_enabled(&mut self, magnet: MagnetId, enabled: bool)`

Enable or disable a magnet. Disabled magnets are skipped by
broad-phase, feedback, the keyboard cycle, and AT emission.
No-op if the id is unknown.

#### `pub fn magnet_ids_of(&self, item: ItemId) -> Vec<MagnetId>`

The ids of every magnet attached to `item`, in insertion order
(enabled and disabled alike). Empty if `item` is unknown or has
no magnets.

#### `pub fn magnet_owner(&self, magnet: MagnetId) -> Option<ItemId>`

The owning item of a magnet, or `None` if the id is unknown.

#### `pub fn magnet_enabled(&self, magnet: MagnetId) -> bool`

Whether a magnet is enabled. `false` for an unknown id.

#### `pub fn magnet_scene_pos(&self, magnet: MagnetId) -> Option<Point>`

A magnet's position in scene coordinates (its local position
projected through its owning item's `scene_transform`). `None`
for an unknown id or a degenerate item transform.

#### `pub fn magnet(&self, magnet: MagnetId) -> Option<MagnetRef>`

Resolve a magnet to a borrow-free `MagnetRef` snapshot (id,
owning item, role, payload clone, current scene position).
`None` for an unknown id or a degenerate item transform.

#### `pub fn compute_item_snap( &self, dragged: ItemId, drag_delta: Vec2, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<MagnetSnap>`

Compute the best item-drag snap: the dragged item is visually
offset by `drag_delta`, and each of its enabled magnets seeks the
nearest *accepting* magnet on another item within `capture_radius`
(in scene units). Returns the globally closest accepting pair, or
`None` if nothing accepts within range.

Pure mechanism: it collects candidates under a brief read, then
runs the consumer `predicate` with no scene borrow held, so the
predicate may inspect payloads freely. `snap_vector` added to
`drag_delta` aligns the dragged magnet onto its target.

#### `pub fn compute_port_snap( &self, source: MagnetId, cursor_scene: Point, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<(MagnetRef, Option<Rc<dyn std::any::Any>>)>`

Compute the best port-drag snap: a single `source` magnet is
dragging a transient wire whose free end is at `cursor_scene`.
Finds the nearest *accepting* target magnet within
`capture_radius` (scene units), excluding the source's own
magnet. Returns the target `MagnetRef` and the accepting
verdict's payload, or `None`.

#### `pub fn nearest_magnet(&self, scene_pt: Point, radius: f32) -> Option<MagnetId>`

The nearest enabled magnet to `scene_pt` within `radius` (scene
units), or `None`. Used by the view to start a port-drag from a
grabbed magnet handle (the handle's grab area is a screen-pixel
disc, converted to scene units by the caller).

#### `pub fn add_a11y_group(&mut self, builder: A11yGroupBuilder) -> A11yGroupId`

Declare a virtual AT group. The group has no visual
counterpart — it exists so the AT walker can emit an AT node
under which items / other groups / widgets can be reparented.

#### `pub fn remove_a11y_group(&mut self, id: A11yGroupId)`

Remove a logical group; orphaned references fall back to
SceneView root. Relations / live / landmarks / categories
targeting this group are cleaned up too.

#### `pub fn a11y_group(&self, id: A11yGroupId) -> Option<&A11yGroup>`

Borrow a logical group by id.

#### `pub fn set_a11y_parent(&mut self, child: A11yNode, parent: Option<A11yNode>)`

Declare a logical-parent relationship for AT (independent of
visual placement).

#### `pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode>`

The currently-declared logical parent of a node.

#### `pub fn add_a11y_relation(&mut self, from: A11yNode, kind: A11yRelation, to: A11yNode)`

Declare an AT relationship between two nodes.

#### `pub fn a11y_relations(&self) -> &[(A11yNode, A11yRelation, A11yNode)]`

All declared AT relations.

#### `pub fn set_a11y_live(&mut self, node: A11yNode, live: accesskit::Live)`

Mark a node as a live region. Pass `Live::Off` to clear.

#### `pub fn set_a11y_landmark(&mut self, node: A11yNode, role: accesskit::Role)`

Mark a node as a landmark by overriding its role. Pass
`Role::Unknown` to clear.

#### `pub fn set_a11y_categories(&mut self, node: A11yNode, categories: &[A11yCategory])`

Tag a node with rotor / quick-nav categories.

#### `pub fn a11y_categories_of(&self, node: A11yNode) -> Option<&[A11yCategory]>`

Read declared categories for a node.
