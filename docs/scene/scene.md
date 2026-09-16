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
use teksilo_scene::{Scene, ItemChange, SceneLayer};
use teksilo_scene::{RectItem, ItemId};
use teksilo_canvas::{Point, Rect};
use teksilo_tokens::Color;

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

`with_index`, `add_widget`, `add_item`, `add_item_dynamic`, `refresh_dynamic_bounds`, `item_change_signal`, `a11y_change_signal`, `cascade_budget`, `set_cascade_budget`, `mutation_version`, `structural_version`, `local_pos`, `set_local_pos`, `local_bounds`, `set_local_bounds`, `transform`, `set_transform`, `scene_transform`, `scene_pos`, `scene_rect`, `map_to_scene`, `map_from_scene`, `flags`, `set_flags`, `set_flag`, `set_visible`, `is_effectively_visible`, `opacity`, `set_opacity`, `set_item_fill`, `clear_item_fill`, `set_item_stroke`, `clear_item_stroke`, `add_boxed_item`, `set_item_handlers`, `handlers_mut`, `handlers`, `effective_opacity`, `set_scene_rect`, `scene_rect_extent`, `pan_axes`, `current_pan_axes`, `zoomable`, `is_zoomable`, `set_pan_bounds`, `current_pan_bounds`, `set_zoom_range`, `current_zoom_range`, `pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`, `zoomable_signal`, `constraints`, `set_z`, `bring_to_front`, `send_to_back`, `z`, `set_layer`, `layer`, `set_item_parent`, `parent_of`, `is_descendant_of`, `collect_descendants`, `item`, `paint_key`, `remove`, `orphan`, `items_in_rect`, `item_thumbnails`, `item_shape`, `item_contains`, `item_region`, `items_in_region`, `item_at`, `item_at_scaled`, `items_at`, `items_at_scaled`, `item_at_in_view`, `is_hit_testable`, `colliding_items`, `colliding_items_with`, `items_along_path`, `items_along_path_with`, `len`, `is_empty`, `ids`, `index`, `add_magnet`, `remove_magnet`, `clear_magnets`, `set_magnet_local_pos`, `set_magnet_enabled`, `magnet_ids_of`, `magnet_owner`, `magnet_enabled`, `magnet_scene_pos`, `magnet`, `compute_item_snap`, `compute_port_snap`, `nearest_magnet`, `add_a11y_group`, `remove_a11y_group`, `a11y_group`, `set_a11y_parent`, `a11y_parent_of`, `add_a11y_relation`, `a11y_relations`, `set_a11y_live`, `set_a11y_landmark`, `set_a11y_categories`, `a11y_categories_of`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub enum ItemChange`

A change to an item's state, fired through
`Scene::item_change_signal` for every mutation. Apps observe to wire
validation, persistence, telemetry, mirroring to a data layer, and other
side effects. The model is "fire after the change has been applied" — by
the time the observer sees the event, the Scene already reflects it, and
(when the mutation came through a `SceneModel`) the
observer may freely read *and* write the scene back.

`#[non_exhaustive]`: this is the crate's outbound event vocabulary, matched
by every observer, and it grows whenever the scene learns to report
something new — `HandlersChanged` is the most recent. Without the
attribute each such addition would stop a downstream `match` from
compiling; with it, a consumer's wildcard arm keeps meaning "a change I do
not act on".

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
- **`AppearanceChanged`** — `set_item_fill` / `set_item_stroke` / `clear_item_*`: a lightweight item's paint-only appearance (fill / stroke colour or style) changed. Never moves geometry, so the observing `SceneView` evicts the item's cached frame and repaints **without** relayout or rebuild.  It *can* move the item's hit **shape**, though: a stroked `PathItem` derives its hit band from the stroke it draws, so a view caching hit geometry must re-read this item's shape even while skipping relayout.
- **`HandlersChanged`** — `set_item_handlers` / `handlers_mut`: the item's handler set was replaced or handed out for mutation.  `handlers_mut` fires it on the way *in*, before the set it returns has been written, because a `&mut` borrow cannot report what the caller will do with it. So this variant means "this item's handlers are no longer what you last read", which is exactly what a consumer caching them needs, and nothing finer.  Without it, the two handler mutators were the only doors in the model that changed observable state silently, and anything caching a handler set — the `SceneView`'s dispatch snapshot — would serve the old one indefinitely.

### Methods

#### `pub fn id(&self) -> ItemId`

The item this change is about.

Every variant names exactly one item, so an observer that only needs
*which* item moved need not match the whole enum. The fan-out's runaway
detector reads it too: it charges each delivery to the subject it is
about, so it needs that subject without caring what kind of change it
is. (Those per-subject counts name the culprit in the panic; the bound
that trips is a flat total — see `CascadeBudget`.)

## `pub struct CascadeBudget`

The runaway-detection budget for one scene's change fan-out — how much work
**observers** may generate from one batch before the drain declares the
cascade non-terminating and panics.

# Why a budget at all

The drain runs until its queue is empty, which is what lets an observer read
the scene and write it back. An observer whose write never converges
therefore never empties it, and — unlike `Signal::try_set`, which recurses
and so blows the stack loudly on its own — this is an iterative loop whose
only other exit is an empty queue. Unchecked it is a frozen UI thread with
no diagnostic. The limit is enforced in **every** build profile for that
reason.

"Never converges" is the precise condition, and it is not the same as
"unguarded". The geometry mutators already suppress a write that changes
nothing — `Scene::set_local_pos` returns without emitting when the
position is unchanged — so an observer that keeps writing the *same*
position settles on its own. A geometry cascade that does not settle is one
computing a *different* value every time: an accumulating offset, a rounding
drift, a spring with no rest state. The `set_a11y_*` mutators do not
self-suppress; they bump on every call, so there an equality check before
the call is a real guard.

# One trip condition, and why it is flat

**Total observer-generated deliveries in a single drain**, counted across
both channels and every subject. Nothing else trips.

Two earlier shapes of this budget each fixed the previous one's false
positive and bought a worse problem, and the third is why this one is flat:

- A cap on **rounds** aborted a legitimate 300-link settling chain, because a
  round is what was queued when it began, so an N-link chain costs N rounds.
- A cap **per subject** moved the limit onto fan-in: a guarded relaxation
  over a hub passed at 4000 incident edges and aborted at 4200.
- **Scaling** the per-subject cap with the scene's entry count fixed the
  fan-in false positive and destroyed the guard, because the bound it
  resolved grew with the model.

A flat total is the only one of the three that bounds a runaway to the same
amount of work whatever the scene's size. Measured in release, changing only
this budget: an unguarded write-back aborts after 100 001 deliveries in a
10 000-entry scene and in a 50 000-entry one alike, where the budget the
scaled design resolved for those scenes (640 000 and 3 200 000) let the same
loop run 6× and 32× longer — 3.10 s and 131 s against 520 ms and 3.95 s. A
spawner allocates ~100 000 items before tripping at either size, against
640 002 and **3 200 002** under the scaled budget.

(The flat write-back's wall clock still grows with the scene, and that is the
*app's* write, not the drain: `Scene::set_local_pos` re-buckets the moved
subtree, which walks every entry. The identical runaway on the logical-AT
channel, whose mutator does not, aborts in 4.8 ms at both sizes — that is
what this loop itself costs. The budget bounds how many times an observer's
write runs, not what one costs.)

The default also clears every legitimate shape this tier runs by more than an
order of magnitude: a 4200-edge guarded aggregate is about 4 200 deliveries,
and a 2000-link chain that re-places eight port magnets and refreshes three
AT properties per link is about 24 000.

# What it still does not decide

"Guarded cascade that is genuinely enormous" and "unguarded write-back" are
not distinguishable from the queue alone. This budget does not pretend
otherwise: it draws the line where the shapes this tier actually runs stop,
reports the subject with the highest delivery count so the diagnostic points
somewhere, and
`SceneModel::set_cascade_budget` is
the supported answer for a graph that genuinely lives past it. Mechanism in
the framework, policy in the consumer.

# What is exempt

The caller's own batch — everything already queued when the drain began — is
not charged, however large. A bulk load, a ten-thousand-item teardown, or a
scene-wide a11y re-tag inside one `SceneWriteGuard`
is finite by construction, so charging it would make the budget a cap on
batch size instead — the mistake a plain delivery counter makes, whose fix is
to raise it until it catches nothing. Cascade depth and the number of
*distinct* subjects a cascade touches are not limited either.

```
use teksilo_scene::{CascadeBudget, SceneModel};

let model = SceneModel::new();
model.set_cascade_budget(CascadeBudget::new(1_000_000));
assert_eq!(model.cascade_budget().total, 1_000_000);
```

`#[non_exhaustive]`: a budget is built through
`CascadeBudget::new` or `Default`, never by struct literal,
so a second dimension (a per-subject cap, a depth limit) can be added
without breaking a caller.

```rust
pub struct CascadeBudget { /* fields */ }
```

### Methods

#### `pub fn new(total: u64) -> Self`

A budget of `total` observer-generated deliveries per drain, clamped up
to the smallest usable value.

The clamp is here rather than at the trip so that a nonsensical budget is
rejected where it is written, instead of producing a panic whose numbers
describe no cascade that happened.

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

Reactive notification stream for every Scene mutation. Apps observe via
`signal.observe(|change| …)` to wire validation, persistence,
telemetry, or mirroring into a data layer without polling the Scene
each frame. The signal fires *after* the mutation has been applied — by
the time the observer runs, the Scene already reflects the new state.

# Notification timing

Where the fan-out happens depends on which door mutated the scene, and
the difference is exactly the `RefCell` an observer would have to
re-enter:

- **Through a `SceneModel`** (the normal path,
  including `SceneWriteGuard`) the mutation
  runs inside a *write scope*: changes queue in emission order and fan
  out once the model has released its `borrow_mut()`. An observer may
  therefore read the scene, and write it back — a write made from an
  observer queues in turn and is delivered by the same drain, after the
  changes already queued ahead of it.
- **Through a bare `&mut Scene`** there is nothing holding a `RefCell`
  open to escape from, so the fan-out is synchronous, inside the
  mutator, exactly as before.

  This is safe for a `Scene` you own outright — no second handle to it
  can exist. It is **not** safe for a `&mut Scene` reborrowed out of a
  `SceneModel`: a `RefMut` taken from the shared
  cell leaves `defer_depth` at zero, so observers still run under the
  live exclusive borrow and one that re-enters the model panics. Take
  `SceneModel::write_guard` instead —
  it derefs to `&mut Scene`, so the call sites are identical, and it
  opens the write scope the observers need.

Either way the notification has been delivered by the time the mutator
call returns.

# What is not deferred

The scene's constraint signals — `pan_axes_signal`,
`zoomable_signal`,
`pan_bounds_signal`,
`zoom_range_signal` — carry *state*, not
events (`current_pan_axes` and friends read
them back), so queueing their writes would make the scene contradict
itself inside an open write scope. They still fan out synchronously
under the borrow: an observer on one of those four must not re-enter
the `SceneModel`.

#### `pub fn a11y_change_signal(&self) -> Signal<u64>`

Reactive notification for logical-AT-structure mutations
(`add_a11y_group` / `remove_a11y_group` / `set_a11y_parent` /
`add_a11y_relation` / `set_a11y_live` / `set_a11y_landmark` /
`set_a11y_categories`). A monotonic counter bumped after each such
mutation. `SceneView` observes this to re-walk the AccessKit tree —
these changes don't flow through `item_change_signal`
because they aren't item geometry, and the AT tree is separate from the
visual scene.

#### `pub fn cascade_budget(&self) -> CascadeBudget`

The runaway-detection budget for this scene's change fan-out.

#### `pub fn set_cascade_budget(&self, budget: CascadeBudget)`

Replace the runaway-detection budget for this scene's change fan-out.

The default is tuned for the graph shapes this tier runs; see
`CascadeBudget` for the rule and for when raising it is the right
answer rather than a way to silence a real cycle.

A budget of zero is clamped to the smallest usable one rather than
stored: it would forbid the reactive write-back this channel exists for,
and would trip with no cascade for the diagnostic to describe.

Takes effect on the next drain — a budget raised from inside an observer
does not enlarge the drain already running, which is why the budget is
read once at its start.

#### `pub fn mutation_version(&self) -> u64`

Monotonic counter of every model mutation applied so far — item geometry
/ visibility / structure (each `ItemChange`) **and** logical-AT
structure (groups, parents, relations, live, landmarks, categories).

Includes the per-frame churn of
`refresh_dynamic_bounds`; a consumer
gating an expensive rebuild on "did anything *meaningful* change" wants
`structural_version` instead. The counter
wraps; compare for equality, not ordering.

#### `pub fn structural_version(&self) -> u64`

`mutation_version` with the per-frame
dynamic-bounds churn subtracted out: it advances on every mutation
**except** the `LocalBoundsChanged` events
`refresh_dynamic_bounds` emits.

The version to gate an expensive rebuild on.
`SceneView` snapshots it each `build()` and only
re-walks the (separate, expensive) AccessKit tree when it has advanced
since the previous walk — so an actively-animating
`add_item_dynamic` item, which rebuilds every
frame, does not issue an AT re-walk per frame for sub-pixel bounds drift
a screen reader cannot use.

Naming the exclusion is the point. The alternative — snapshotting
`mutation_version` before `refresh_dynamic_bounds` and again after, and
treating the difference as churn — is wrong now that the refresh fans
its changes out: an observer may legally mutate the scene from that
fan-out, and its mutation lands inside the bracket where it is
indistinguishable from churn. Folded into the baseline, an AT-structural
change made there would never un-gate a re-walk, in that build or any
later one. The counter wraps; compare for equality, not ordering.

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

# The item has the last word, and says it once

An item whose box is **derived** from its own geometry — a
`PathItem` — treats this as *"fit yourself to this
rectangle"* rather than *"adopt this rectangle"*, and the box stored
here is what it settled on, read back from the item. It lands on the
request on every axis the geometry has extent on; on an axis it has
none (a perfectly horizontal stroke has no height) the box stays the
stroke's own thickness, because there is nothing to stretch.

Either way the call is **idempotent**: asking twice for the same
rectangle emits one `ItemChange::LocalBoundsChanged` and re-buckets
the index once. An app driving this per frame — a resize handle, a
layout pass — therefore goes quiet as soon as it stops moving, rather
than emitting an endless series of nearly-identical changes.

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

#### `pub fn set_item_fill(&mut self, id: ItemId, fill: impl Into<ColorProp>)`

Replace a lightweight item's fill colour live, emitting
`ItemChange::AppearanceChanged` — **always repaint-only**, never a
relayout, rebuild, or AccessKit re-walk. The colour is a `ColorProp`,
so it accepts a plain `Color`, a theme role, a
`Signal<Color>`, or a `Signal<Role>`. No-op for item kinds without a fill
(e.g. `ImageItem`).

# Reactivity contract

A colour becomes **continuously** reactive by being registered at build
time (`SceneItem::register_bindings`). So:

- **Construct** the item with a `Signal`/role colour (`.fill(my_signal)`)
  for a colour that tracks its signal forever. This is the recommended
  path and needs no mutator at all.
- **This mutator** installs a *snapshot*: it repaints immediately, which
  is all a static colour ever needs. If you pass a `Signal`/dynamic role
  here, it paints the signal's current value now and starts tracking it
  continuously from the owning view's next rebuild (whenever some other
  structural change re-runs `register_bindings`). Deliberately *not*
  forced: a colour change must never cost a rebuild + AT re-walk.

#### `pub fn clear_item_fill(&mut self, id: ItemId)`

Clear a lightweight item's fill (Rect/Path/Group become fill-less),
emitting `ItemChange::AppearanceChanged` (repaint-only). No-op for items
whose fill can't be cleared (e.g. `TextItem`, which always has a
foreground colour).

#### `pub fn set_item_stroke(&mut self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle)`

Replace a lightweight item's stroke (colour + `StrokeStyle`) live,
emitting `ItemChange::AppearanceChanged` (repaint-only). No-op for item
kinds without a stroke slot (`TextItem` / `ImageItem`). See
`set_item_fill` for the reactivity contract.

#### `pub fn clear_item_stroke(&mut self, id: ItemId)`

Clear a lightweight item's stroke, emitting
`ItemChange::AppearanceChanged` (repaint-only). No-op for item kinds
without a stroke.

#### `pub fn add_boxed_item(&mut self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId`

Insert an already-boxed lightweight item at `local_pos`, returning its
id. The boxed-`dyn` counterpart of `add_item` — used by
`SceneListAdapter` whose delegate yields
`Box<dyn SceneItem>`.

#### `pub fn set_item_handlers(&mut self, id: ItemId, handlers: Option<SceneItemHandlerSet>)`

Replace an item's handler set. Pass `None` to clear.

Fires `ItemChange::HandlersChanged`, like every other mutator on this
type: a consumer that caches handlers has no other way to learn of it.

#### `pub fn handlers_mut(&mut self, id: ItemId) -> Option<&mut SceneItemHandlerSet>`

Mutably borrow an item's handler set, lazily creating an
empty one if none exists. Returns `None` for unknown ids.
Allows fluent chains: `scene.handlers_mut(id).unwrap().on_tap(…).cursor(…);`.

Fires `ItemChange::HandlersChanged` *before* handing the set out —
`&mut` cannot report back what the caller does with it, so the
notification means "no longer what you last read". A caller that takes
the borrow and changes nothing therefore costs one spurious
invalidation, which is the right way round: the alternative is a
consumer serving stale handlers.

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

Append every direct + transitive descendant of `id` into `out`, each
parent before its own children. The id itself is **not** included.

Costs the subtree, not the scene: the walk steps through the kept
`SceneEntry::children` adjacency rather than rescanning every entry per
visited node. A cycle in the parent graph is bounded by the visited set
rather than looping forever.

#### `pub fn item(&self, id: ItemId) -> Option<&dyn SceneItem>`

Borrow a lightweight `SceneItem` by id. `None` for unknown
ids and for heavyweight widget entries.

#### `pub fn paint_key(&self, id: ItemId) -> Option<PaintKey>`

Where `id` sits in this scene's single paint order — the value every
picker in the crate compares. `None` for unknown ids.

Defined for **both tiers**: a lightweight entry's rank is its
`SceneLayer` band (`RANK_UNDER` /
`RANK_OVER`), a heavyweight widget entry's is
`RANK_WIDGET` — which is exactly where the
arena's child walk paints it, between the two lightweight bands. See
`PaintKey` for the ordering and for the equal-`z` tie-break.

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

#### `pub fn item_thumbnails(&self) -> Vec<(Rect, teksilo_tokens::Color)>`

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

#### `pub fn item_shape(&self, id: ItemId) -> Option<ItemShape>`

The shape of any entry, in its **local** coordinates.

For a lightweight item this is `SceneItem::shape`. A **heavyweight**
widget entry has no `SceneItem` at all — `Scene::item` returns `None`
for it — so its shape is defined to be
`ItemShape::bounds` of its `local_bounds`. That is not a placeholder:
a widget's silhouette is its layout box, the arena hit-tests it as one,
and the marquee has always selected heavyweight entries through the
same index query as lightweight ones. Stating it here is what keeps a
`…ItemShape` selection mode meaningful for a scene whose primary
objects are cards.

`None` for an unknown id.

#### `pub fn item_contains(&self, id: ItemId, scene_pt: Point, view_scale: f32) -> bool`

Whether `id`'s shape contains `scene_pt`.

Works for both tiers (see `Scene::item_shape`). `view_scale` is the
live view zoom, consulted only by a cosmetic stroke band; pass `1.0`
when there is no view.

#### `pub fn item_region(&self, id: ItemId) -> Option<SceneRegion>`

The item's own shape re-published as a **scene**-space region — what a
collision query asks the rest of the scene about. `None` for an unknown
id, a shape of `ItemShape::none`, or a screen-anchored
(`IGNORES_TRANSFORMATIONS`)
item, whose silhouette is not in scene space at all.

#### `pub fn items_in_region( &self, region: &SceneRegion, mode: ItemSelectionMode, view_scale: f32, ) -> Vec<ItemId>`

Items matching `region` under `mode` — **both tiers**, exactly like
`Scene::items_in_rect`, which this generalises.

Broad-phased by the spatial index on `region.bounding_rect()`, then
narrow-phased per `mode`: the `…ItemBoundingRect` modes compare the
item's **scene** AABB against the region in scene space; the
`…ItemShape` modes map the region into the item's **local** frame and
compare it against `Scene::item_shape`. For an item with a
non-identity transform those are different tests — see
`ItemSelectionMode`.

`view_scale` is the live view zoom, consulted only by a cosmetic stroke
band; pass `1.0` when there is no view. Visibility and selectability
are **not** filtered here — this is a pure geometry query, and the
caller (e.g. `SceneSelection::commit_marquee`)
applies its own flag policy.

Items flagged
`IGNORES_TRANSFORMATIONS`
**are skipped**, for the reason `Scene::item_at` skips them: they are
anchored in screen space, so their `local_bounds` is a screen rectangle
and the scene AABB the index holds for them is a fiction. Comparing a
scene-space region against that fiction is a guess, and the two query
families used to disagree about whether to make it — the point queries
declined and the marquee did not. Reaching screen-pinned chrome needs a
region that arrives in *screen* space, which is what
`Scene::item_at_in_view` does for a point; there is no region twin of
it yet, so a marquee cannot select pinned chrome at all.

#### `pub fn item_at(&self, scene_pt: Point) -> Option<ItemId>`

Topmost **lightweight** item whose shape contains `scene_pt`, at unit
view scale. See `Scene::item_at_scaled` for the zoom-aware form and
`Scene::item_at_in_view` for the one that also places screen-anchored
items.

Heavyweight widget entries are skipped: their hit-testing is the
arena's job, and a scene-space answer would contradict it.

Items flagged
`IGNORES_TRANSFORMATIONS`
are **also skipped**, deterministically. They are anchored in screen
space, so a scene-space point cannot place them at all; answering with
a coin-flip (which is what comparing them against a scene AABB amounts
to) is worse than not answering. Use `Scene::item_at_in_view`, or
`SceneView` dispatch, when the query needs them.

"Topmost" is `Scene::paint_key` order, so an
`Over`-band item beats a higher-`z`
`Under` one, and two equal-`z` items resolve to the
later-inserted one — the same answer `SceneView` dispatch gives.

Hidden and disabled entries are excluded: this is a *hit* test, and
`ItemFlags::IS_VISIBLE` and `ItemFlags::IS_ENABLED` both say so. See
`Scene::is_hit_testable`. For a pure geometry query that ignores flags,
use `Scene::items_in_region` or `Scene::item_contains`.

#### `pub fn item_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId>`

`Scene::item_at` at an explicit view zoom.

The zoom reaches exactly one thing: a **cosmetic** stroke band, whose
width is in device pixels and therefore covers fewer scene units the
further you zoom in. Passing the live scale is what makes this agree
with `SceneView`'s own dispatch, which has always had it.

#### `pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>`

All lightweight items whose shape contains `scene_pt`, topmost-first by
z. Same tier and `IGNORES_TRANSFORMATIONS` rules as `Scene::item_at`.

#### `pub fn items_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Vec<ItemId>`

`Scene::items_at` at an explicit view zoom.

#### `pub fn item_at_in_view(&self, screen_pt: Point, view_transform: Transform2D) -> Option<ItemId>`

Topmost lightweight item under a **screen** point, resolving *both*
hit spaces the way `SceneView` dispatch does.

A normal item is tested in scene space, at the view transform's zoom; a
screen-anchored
(`IGNORES_TRANSFORMATIONS`)
item is tested against its `local_bounds` rooted at its projected
anchor, at unit scale — its local coordinates *are* screen
coordinates, so it has no zoom to convert. This is the query to use
when a scene may contain screen-pinned chrome; `Scene::item_at`
deliberately declines to guess.

#### `pub fn is_hit_testable(&self, id: ItemId) -> bool`

Whether `id` takes part in pointer hit-testing — visible along its whole
ancestor chain AND enabled.

This is `crate::pick::hit_testable` resolved against the scene, and it
is the one place the two flag contracts in `ItemFlags` are honoured:
`IS_VISIBLE` ("neither painted nor hit-tested")
and `IS_ENABLED` ("pass clicks through to items
beneath"). `false` for unknown ids.

#### `pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>`

Items overlapping `id`'s **shape**, excluding `id` itself.

Apps use this for "which other items overlap this card?" — graph
editors checking node-on-node overlap, CAD canvases finding adjacent
geometry. Backed by the spatial index, so the cost is `O(visible)` not
`O(N)`.

Defaults to `ItemSelectionMode::IntersectsItemShape`, so a
stroke-only connector collides along its line rather than across its
bounding box. Pass
`IntersectsItemBoundingRect`
to `Scene::colliding_items_with` for the cheaper box test.

Screen-anchored items neither collide nor are collided with — see
`Scene::items_in_region`, which this is built on, and
`Scene::item_region`, which declines to publish one for them.

#### `pub fn colliding_items_with(&self, id: ItemId, mode: ItemSelectionMode) -> Vec<ItemId>`

`Scene::colliding_items` under an explicit `ItemSelectionMode`.

#### `pub fn items_along_path(&self, path: &Path) -> Vec<ItemId>`

Items lying along `path` — a real region query, not the AABB-of-the-path
approximation this used to be.

The path is treated as a zero-width closed region: an item is picked
when the path crosses it or encloses it. For a *connector* — a line
with a width — pass that width to `Scene::items_along_path_with`, so
the query asks about the band the user can see.

#### `pub fn items_along_path_with( &self, path: &Path, stroke_width: f32, mode: ItemSelectionMode, ) -> Vec<ItemId>`

`Scene::items_along_path` with an explicit stroke width and
`ItemSelectionMode`.

`stroke_width` greater than zero makes the region the path's **band**
rather than its interior: "what does this 4 dp connector touch?".

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
