<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneModel

`SceneModel` — a shared, cloneable handle to a `Scene`.

Mirrors the `ListModel = Rc<RefCell<ListModelInner>>` pattern from
`teksilo-data`: cloning a `SceneModel` produces a **second handle to the
same scene**, so multiple `SceneView`s can render one
scene (overview + detail panes, same-document multi-window, headless model
reuse). Mutate the model once and every attached view reconciles.

## Heavyweight content across views

A heavyweight `Widget` instance can live in only one arena, so a shared
model cannot hand the *same* `Box<dyn Widget>` to two views. Two paths:

- **Single-view** — `add_widget` stores the
  widget in a one-shot slot drained by the first view that builds. A
  second view sharing the model produces no child for it.
- **Multi-view** — `add_widget_item` stores
  a type-erased `payload`; each view's delegate
  (`SceneView::delegate_typed`) builds
  its **own** instance from the payload. `set_payload`
  replaces the data and every view rebuilds that item.

## Borrow / observer contract

Every mutator takes `&self` and runs through `SceneModel::write`, which
is the `teksilo-data` **mutate-then-notify** discipline adapted to a borrow
that spans a whole `&mut Scene` method. `ListModel` can scope its
`borrow_mut()` and call `notify` after it (`list_model.rs`); `Scene` cannot,
because the notification is emitted from *inside* a `&mut self` method whose
borrow Rust only releases on return. So the notification is **queued in the
`Scene`** and drained by the owner of the borrow — `write`, or
`SceneWriteGuard` — once that borrow has dropped.

The consequence is the contract app authors actually want: **an observer
registered on `item_change_signal` or
`a11y_change_signal` may read the scene
and write it back.** A write made from an observer queues in turn and is
delivered by the same drain, after whatever was already queued ahead of it —
see `SceneModel::flush_changes` for ordering, re-entrancy and termination.

Three doors are **not** covered, each for a stated reason:

- A bare `&mut Scene` fans out synchronously, because nothing is holding a
  `RefCell` open for it to escape from. For a `Scene` you own outright that
  is safe — no second handle to it can exist. For a `&mut Scene` *reborrowed
  out of a `SceneModel`* it is not: a raw `RefMut` from the shared cell
  leaves the write scope closed, so observers run under the live exclusive
  borrow and one that re-enters the model panics, exactly as every mutator
  used to. `SceneModel::write_guard` is the fix and the substitute — it
  derefs to `&mut Scene`, so the call sites are unchanged.
- The four constraint signals (`pan_axes` / `zoomable` / `pan_bounds` /
  `zoom_range`) are scene *state*, read back by `current_pan_axes` and
  friends, so their writes stay synchronous — deferring them would make the
  scene contradict itself inside a write scope. An observer on one of those
  four must not re-enter the `SceneModel`.
- `with_handlers_mut` runs the caller's
  closure *inside* the borrow (it hands out a `&mut` into the scene, which
  cannot outlive it), so that closure must not re-enter the `SceneModel`
  either. It is unwind-safe — a panic in the closure cannot strand the write
  scope — but it is not re-entrant.

A view **delegate** must also not synchronously mutate the model during a
build-time call (the view drops all model borrows before invoking it; the
delegate's *handlers* may mutate later).

## Builder methods at a glance

`write_guard`, `flush_changes`, `cascade_budget`, `set_cascade_budget`, `with_index`, `from_scene`, `handle_count`, `add_widget`, `add_widget_item`, `set_payload`, `payload`, `add_item`, `add_item_dynamic`, `add_boxed_item`, `set_local_pos`, `set_local_bounds`, `set_transform`, `set_flags`, `set_flag`, `set_visible`, `set_opacity`, `set_item_fill`, `clear_item_fill`, `set_item_stroke`, `clear_item_stroke`, `set_z`, `bring_to_front`, `send_to_back`, `set_layer`, `set_item_parent`, `remove`, `orphan`, `set_item_handlers`, `with_handlers_mut`, `add_magnet`, `remove_magnet`, `clear_magnets`, `set_magnet_local_pos`, `set_magnet_enabled`, `magnet_ids_of`, `magnet_owner`, `magnet_scene_pos`, `magnet`, `compute_item_snap`, `compute_port_snap`, `nearest_magnet`, `set_scene_rect`, `pan_axes`, `zoomable`, `set_pan_bounds`, `set_zoom_range`, `add_a11y_group`, `remove_a11y_group`, `set_a11y_parent`, `add_a11y_relation`, `set_a11y_live`, `set_a11y_landmark`, `set_a11y_categories`, `refresh_dynamic_bounds`, `item_change_signal`, `a11y_change_signal`, `mutation_version`, `structural_version`, `pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`, `zoomable_signal`, `len`, `is_empty`, `ids`, `local_pos`, `local_bounds`, `transform`, `scene_transform`, `scene_pos`, `scene_rect`, `flags`, `is_effectively_visible`, `opacity`, `effective_opacity`, `z`, `layer`, `parent_of`, `is_descendant_of`, `scene_rect_extent`, `current_pan_axes`, `is_zoomable`, `current_pan_bounds`, `current_zoom_range`, `items_in_rect`, `items_in_region`, `item_shape`, `item_region`, `item_contains`, `paint_key`, `is_hit_testable`, `item_at`, `item_at_scaled`, `items_at`, `items_at_scaled`, `item_at_in_view`, `colliding_items`, `colliding_items_with`, `items_along_path`, `items_along_path_with`, `a11y_parent_of`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub struct SceneModel`

A shared, cloneable handle to a `Scene`.

```rust
pub struct SceneModel(pub(crate) Rc<RefCell<Scene>>);
```

### Methods

#### `pub fn write_guard(&self) -> SceneWriteGuard<'_>`

Open a write scope over the scene; see `SceneWriteGuard`.

The one door for a *block* of edits that must fan out together, and the
door a `&mut Scene` escape hatch should be built on.

#### `pub fn flush_changes(&self)`

Deliver every queued notification.

Called automatically when a write scope closes, so apps rarely need it.
The exception is recovering from a caught panic: an unwinding
`SceneWriteGuard` deliberately skips its fan-out, and a panicking
observer stops the drain where it stood, so a `catch_unwind` boundary
that intends to keep using the scene calls this once.

# When it does nothing

*Where* it is called from never makes it panic — this is the door a
recovery path is told to knock on, so it cannot be one that only works
from some of them. (An observer can still panic once the drain reaches
it, and so can the runaway budgets below; both are the observer's
doing, not the call site's.) It is a no-op, and says so rather than
delivering half a batch, when:

- nothing is queued (overwhelmingly the common case — every mutator that
  early-returned on an unchanged value, and every per-frame
  `refresh_dynamic_bounds`);
- **any** borrow on the scene is outstanding. A write scope is the
  obvious one — it owns its batch and fans out when it closes, so
  flushing from inside it would deliver a half-finished mutation, and
  the old implementation's `borrow()` simply panicked there. But a
  *shared* borrow disqualifies a flush just as firmly, because an
  observer is allowed to write and a write needs the cell exclusively:
  draining under a read-only policy closure (`SceneView::focus_order`,
  `MagnetismConfig`'s predicate) would panic the first observer that
  took the invitation. So the probe is `try_borrow_mut`, the only
  question whose answer is "the cell is completely free";
- a drain is already running, i.e. this call came from inside an
  observer. The outermost drain owns the queue and picks that observer's
  changes up on its next round, which is what keeps delivery in emission
  order rather than depth-first.

# Ordering

FIFO. Observers see changes in the order they were emitted, including
across a batch — so `remove`'s documented leaves-then-root order, and
the interleaving of the item and logical-AT channels, both survive.

# Termination

The drain keeps going until the queue is empty, so an observer's own
writes are always delivered rather than silently dropped. Most cycles
terminate on their own — `Scene::set_local_pos` and friends early-return
when the value is unchanged, so the snap-to-grid shape settles in two
rounds.

An observer that writes on **every** change with no equality guard never
settles, and is stopped by a budget that panics with a diagnostic naming
the item at fault.

**What the budget counts is total observer-generated deliveries in one
drain, and nothing else.** The caller's own batch — everything queued
before the first observer ran — is free at any size, because it is finite
by construction; so are cascade depth and the number of distinct subjects
a cascade touches. A cap on **rounds** would cap how long a legitimate
chain may be (a settling 300-link chain is 300 rounds); a cap **per
subject** would cap how wide a graph may be, and scaling that cap with
the scene makes the work a runaway gets to do scale with the scene too. A
flat total is the one bound under which a runaway costs the same number
of deliveries and the same peak memory whatever the model's size, and the
shapes this tier runs clear it by more than an order of magnitude. See
`CascadeBudget` for the measurements.

Per-subject counts are still kept — they are what lets the panic name the
culprit, which a round counter never could — but they are diagnostics,
not a second trip condition.

The budget is **not** debug-only, and this is the one place the
mechanism deliberately does not copy `Signal::try_set`: `try_set`
*recurses*, so an unchecked feedback loop there exhausts the stack and
aborts loudly by itself. This drain is an iterative loop, so an unchecked
loop here is a frozen UI thread with no diagnostic and no core dump — the
one failure mode a release build must not have. Trading a hang for a
panic is not a close call.

The queue is left intact when a budget trips (dropping it would be the
silent notification loss the whole mechanism exists to prevent), so a
later flush with the same observer still installed panics again. That is
the intended reading: the observer, not the flush, is what has to change.

# Unwind

An observer that panics costs exactly its own delivery. Each
notification leaves the queue immediately before it is handed to the
signal and is never parked in a local batch, so everything not yet
delivered is still queued, in order, when the unwind passes through —
and the next call here delivers it. (A local batch would have its
untouched tail dropped by the unwind with nothing able to redeliver it:
the scene would have advanced with no notification, permanently.)

# Coalescing

None. Repeated `LocalPosChanged` for one item are delivered once each.
Two reasons: the advertised uses of this channel — persistence,
validation, audit, mirroring to a data layer — are the ones that need
every event; and collapsing two changes would require merging one's
`old` with the other's `new`, a transaction semantic this crate does not
define. A drag is not a reason to reconsider: each pointer sample is its
own `SceneModel` call, hence its own one-element batch, exactly as
before. If a transaction layer ever wants coalescing, `SceneWriteGuard`
is already its boundary.

# Lifetime

Queued notifications live in the `Scene`. Dropping the last
`SceneModel` handle drops the scene and discards anything still queued —
which can only be a batch abandoned by an unwind, and by then there is
no scene left to observe.

#### `pub fn cascade_budget(&self) -> CascadeBudget`

The runaway-detection budget for this scene's change fan-out — how much
work **observers** may generate from one batch before the drain declares
the cascade non-terminating and panics.

See `CascadeBudget` for the rule and its default.

#### `pub fn set_cascade_budget(&self, budget: CascadeBudget)`

Replace the runaway-detection budget for this scene's change fan-out.

The mechanism is in the framework; this is where the policy lives. The
default — 100 000 observer-generated deliveries per drain — is more than
an order of magnitude above the legitimate shapes this tier runs, but it
cannot tell a guarded cascade that is genuinely enormous from an
unguarded write-back: the two look identical from the queue's side. An
app whose graph genuinely settles past the default therefore has a
supported answer here rather than a crash:

```
use teksilo_scene::{CascadeBudget, SceneModel};

let model = SceneModel::new();
model.set_cascade_budget(CascadeBudget::new(1_000_000));
assert_eq!(model.cascade_budget().total, 1_000_000);
```

Raising it to silence a cycle that does **not** settle only postpones
the freeze it is there to prevent — the drain is still unbounded in
time, just later. Check the write is guarded first.

A budget of zero is clamped to the smallest usable one rather than
stored: it would forbid the reactive write-back this channel exists for.

Takes effect on the next drain: a drain already running read its budget
when it began, so an observer cannot enlarge its own.

#### `pub fn new() -> Self`

A handle to a fresh empty scene with the default spatial index.

#### `pub fn with_index(index: Box<dyn SpatialIndex>) -> Self`

A handle to a fresh scene with a custom `SpatialIndex`.

#### `pub fn from_scene(scene: Scene) -> Self`

Wrap an existing `Scene` in a handle. Used by
`SceneView::new` for the single-view path.

#### `pub fn handle_count(&self) -> usize`

Number of distinct handles to this scene (1 = unshared).

#### `pub fn add_widget<W: Widget + 'static>(&self, widget: W, rect: Rect) -> ItemId`

Single-view heavyweight widget (the one-shot `Once` path). The first
view to build drains it; a second view sharing this model produces no
child for it. For multi-view, use `add_widget_item`.

#### `pub fn add_widget_item<P: 'static>(&self, payload: P, rect: Rect) -> ItemId`

Multi-view heavyweight item: store a typed `payload`; each view builds
its own widget instance from it via its delegate. Returns the `ItemId`.

#### `pub fn set_payload<P: 'static>(&self, id: ItemId, payload: P)`

Replace the payload of a `Delegated` heavyweight item; every view
rebuilds that item's widget on the next pass.

# Panics

Panics if `id` is unknown, refers to a single-view `add_widget` (Once)
entry, or refers to a lightweight item.

#### `pub fn payload(&self, id: ItemId) -> Option<Rc<dyn std::any::Any>>`

The current type-erased payload of a `Delegated` item, if any.

#### `pub fn add_item<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId`

Add a lightweight `SceneItem` at `local_pos`.

#### `pub fn add_item_dynamic<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId`

Add a lightweight item with signal-driven (dynamic) bounds.

#### `pub fn add_boxed_item(&self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId`

Add an already-boxed lightweight item at `local_pos`. The boxed-`dyn`
counterpart of `add_item`, used by
`SceneListAdapter`.

#### `pub fn set_local_pos(&self, id: ItemId, local_pos: Point)`

Move `id` to `local_pos` in its parent's coordinate space; notifies all views.

#### `pub fn set_local_bounds(&self, id: ItemId, local_bounds: Rect)`

Replace the local bounding rect of `id`; notifies all views.

Idempotent, and an item whose box is derived from its geometry fits
itself to the rectangle rather than adopting it — see
`Scene::set_local_bounds` for both contracts, which matter here
because this is the form an app drives per frame.

#### `pub fn set_transform(&self, id: ItemId, transform: Transform2D)`

Set an additional local-to-parent transform (rotation, scale) on `id`; notifies all views.

#### `pub fn set_flags(&self, id: ItemId, flags: ItemFlags)`

Replace the complete `ItemFlags` bitset for `id`; notifies all views.

#### `pub fn set_flag(&self, id: ItemId, flag: ItemFlags, on: bool)`

Set or clear a single `ItemFlags` bit on `id`; notifies all views.

#### `pub fn set_visible(&self, id: ItemId, visible: bool)`

Show or hide `id` (also hides its descendants); notifies all views.

#### `pub fn set_opacity(&self, id: ItemId, opacity: f32)`

Set the paint opacity of `id` (0.0 = transparent, 1.0 = opaque); notifies all views.

#### `pub fn set_item_fill(&self, id: ItemId, fill: impl Into<ColorProp>)`

Replace a lightweight item's fill colour live; every view repaints
(no relayout/rebuild). Accepts a plain `Color`,
a theme role, a `Signal<Color>`, or a `Signal<Role>`. See
`Scene::set_item_fill` for the reactive-colour contract.

#### `pub fn clear_item_fill(&self, id: ItemId)`

Clear a lightweight item's fill; every view repaints.

#### `pub fn set_item_stroke(&self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle)`

Replace a lightweight item's stroke (colour + `StrokeStyle`) live;
every view repaints (no relayout/rebuild).

#### `pub fn clear_item_stroke(&self, id: ItemId)`

Clear a lightweight item's stroke; every view repaints.

#### `pub fn set_z(&self, id: ItemId, z: f32)`

Set the z-order of `id` within its layer; higher values paint on top.

#### `pub fn bring_to_front(&self, id: ItemId)`

Give `id` the highest z-value in its layer so it paints on top of all siblings.

#### `pub fn send_to_back(&self, id: ItemId)`

Give `id` the lowest z-value in its layer so it paints beneath all siblings.

#### `pub fn set_layer(&self, id: ItemId, layer: SceneLayer)`

Move `id` to a different `SceneLayer` (background, default, foreground); notifies all views.

#### `pub fn set_item_parent(&self, child: ItemId, parent: Option<ItemId>)`

Re-parent `child` under `parent` (or under the scene root when `None`); notifies all views.

#### `pub fn remove(&self, id: ItemId)`

Remove an item and its descendants. Drops any `Delegated` payload `Rc`
and cleans the item's a11y mappings; alive logical children re-root.

#### `pub fn orphan(&self, id: ItemId)`

Promote an item's children to the scene root.

#### `pub fn set_item_handlers(&self, id: ItemId, handlers: Option<SceneItemHandlerSet>)`

Replace the `SceneItemHandlerSet` of `id`, or clear it with `None`.

#### `pub fn with_handlers_mut<R>( &self, id: ItemId, f: impl FnOnce(&mut SceneItemHandlerSet) -> R, ) -> Option<R>`

Mutate an item's handler set through a closure (avoids returning a
borrow guard tied to the `RefMut`).

#### `pub fn add_magnet(&self, item: ItemId, magnet: Magnet) -> MagnetId`

Attach a `Magnet` to `item`; see `Scene::add_magnet`.

#### `pub fn remove_magnet(&self, magnet: MagnetId)`

Remove a magnet by id; see `Scene::remove_magnet`.

#### `pub fn clear_magnets(&self, item: ItemId)`

Remove every magnet on `item`; see `Scene::clear_magnets`.

#### `pub fn set_magnet_local_pos(&self, magnet: MagnetId, local_pos: Point)`

Move a magnet in its item's local frame; see `Scene::set_magnet_local_pos`.

#### `pub fn set_magnet_enabled(&self, magnet: MagnetId, enabled: bool)`

Enable or disable a magnet; see `Scene::set_magnet_enabled`.

#### `pub fn magnet_ids_of(&self, item: ItemId) -> Vec<MagnetId>`

Ids of every magnet on `item`; see `Scene::magnet_ids_of`.

#### `pub fn magnet_owner(&self, magnet: MagnetId) -> Option<ItemId>`

The owning item of a magnet; see `Scene::magnet_owner`.

#### `pub fn magnet_scene_pos(&self, magnet: MagnetId) -> Option<Point>`

A magnet's scene position; see `Scene::magnet_scene_pos`.

#### `pub fn magnet(&self, magnet: MagnetId) -> Option<MagnetRef>`

Resolve a magnet to a `MagnetRef` snapshot; see `Scene::magnet`.

#### `pub fn compute_item_snap( &self, dragged: ItemId, drag_delta: Vec2, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<MagnetSnap>`

Best item-drag snap; see `Scene::compute_item_snap`. A shared
(read-only) borrow is held while the `predicate` runs over owned
candidate snapshots, so the predicate may read but must not mutate
the model.

#### `pub fn compute_port_snap( &self, source: MagnetId, cursor_scene: Point, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<(MagnetRef, Option<std::rc::Rc<dyn std::any::Any>>)>`

Best port-drag snap; see `Scene::compute_port_snap`.

#### `pub fn nearest_magnet(&self, scene_pt: Point, radius: f32) -> Option<MagnetId>`

Nearest enabled magnet within `radius`; see `Scene::nearest_magnet`.

#### `pub fn set_scene_rect(&self, rect: Option<Rect>)`

Set the logical extent of the scene (used for scroll-bar sizing); `None` = unbounded.

#### `pub fn pan_axes(&self, axes: PanAxes)`

Restrict panning to horizontal, vertical, or both axes; updates `pan_axes_signal`.

#### `pub fn zoomable(&self, on: bool)`

Enable or disable pinch/scroll zoom; updates `zoomable_signal`.

#### `pub fn set_pan_bounds(&self, bounds: Option<Rect>)`

Clamp the camera pan to `bounds` (scene coordinates); `None` = no limit; updates `pan_bounds_signal`.

#### `pub fn set_zoom_range(&self, range: Option<std::ops::RangeInclusive<f32>>)`

Restrict the zoom factor to `range`; `None` = no limit; updates `zoom_range_signal`.

#### `pub fn add_a11y_group(&self, builder: A11yGroupBuilder) -> A11yGroupId`

Register a logical AT group (landmark / rotor category container); returns its stable `A11yGroupId`.

#### `pub fn remove_a11y_group(&self, id: A11yGroupId)`

Remove a previously registered AT group; triggers an `a11y_change_signal` bump.

#### `pub fn set_a11y_parent(&self, child: A11yNode, parent: Option<A11yNode>)`

Re-parent `child` in the AT tree, overriding the default visual parent; `None` re-attaches under the scene root.

#### `pub fn add_a11y_relation(&self, from: A11yNode, kind: A11yRelation, to: A11yNode)`

Declare a cross-node AT relationship (controls, describes, labels) from `from` to `to`.

#### `pub fn set_a11y_live(&self, node: A11yNode, live: accesskit::Live)`

Mark `node` as a live region (`Polite` or `Assertive`) so assistive tech announces changes to it.

#### `pub fn set_a11y_landmark(&self, node: A11yNode, role: accesskit::Role)`

Assign a landmark `role` to `node` (e.g. `Role::Region`, `Role::Main`) for rotor navigation.

#### `pub fn set_a11y_categories(&self, node: A11yNode, categories: &[A11yCategory])`

Register `node` under the given rotor `A11yCategory` slices so it appears in category-filtered navigation.

#### `pub fn refresh_dynamic_bounds(&self) -> bool`

Re-read signal-driven bounds for `add_item_dynamic` entries; returns
`true` if any changed.

#### `pub fn item_change_signal(&self) -> Signal<ItemChange>`

Reactive signal fired on every structural scene change; all views observe this to reconcile.

#### `pub fn a11y_change_signal(&self) -> Signal<u64>`

Reactive monotonic counter bumped on every AT-structure change; views re-walk accessibility on any increment.

#### `pub fn mutation_version(&self) -> u64`

Monotonic counter incremented on every mutation; useful for cache invalidation without observing a signal.

#### `pub fn structural_version(&self) -> u64`

`mutation_version` with the per-frame churn of
`refresh_dynamic_bounds` excluded — the
version to gate an expensive rebuild on. See
`Scene::structural_version`.

#### `pub fn pan_axes_signal(&self) -> Signal<PanAxes>`

Reactive current `PanAxes` restriction; updated by `pan_axes`.

#### `pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>>`

Reactive camera-pan clamp bounds; updated by `set_pan_bounds`.

#### `pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive zoom-factor clamp range; updated by `set_zoom_range`.

#### `pub fn zoomable_signal(&self) -> Signal<bool>`

Reactive zoom-enabled flag; updated by `zoomable`.

#### `pub fn len(&self) -> usize`

Total number of items in the scene (lightweight + heavyweight).

#### `pub fn is_empty(&self) -> bool`

Returns `true` when the scene contains no items.

#### `pub fn ids(&self) -> Vec<ItemId>`

All `ItemId`s currently in the scene, in insertion order.

#### `pub fn local_pos(&self, id: ItemId) -> Option<Point>`

The local position of `id` in its parent's coordinate space; `None` if `id` is unknown.

#### `pub fn local_bounds(&self, id: ItemId) -> Option<Rect>`

The local bounding rect of `id`; `None` if `id` is unknown.

#### `pub fn transform(&self, id: ItemId) -> Option<Transform2D>`

The additional local-to-parent transform of `id` (beyond position); `None` if none is set.

#### `pub fn scene_transform(&self, id: ItemId) -> Transform2D`

The full local-to-scene transform for `id` (parent chain composed); identity if `id` is unknown.

#### `pub fn scene_pos(&self, id: ItemId) -> Option<Point>`

The origin of `id` mapped into scene coordinates; `None` if `id` is unknown.

#### `pub fn scene_rect(&self, id: ItemId) -> Option<Rect>`

The bounding rect of `id` in scene coordinates (local bounds transformed by the parent chain); `None` if unknown.

#### `pub fn flags(&self, id: ItemId) -> Option<ItemFlags>`

The `ItemFlags` bitset of `id`; `None` if `id` is unknown.

#### `pub fn is_effectively_visible(&self, id: ItemId) -> bool`

Returns `true` if `id` and all of its ancestors are visible.

#### `pub fn opacity(&self, id: ItemId) -> Option<f32>`

The own opacity of `id` (ignoring ancestors); `None` if `id` is unknown.

#### `pub fn effective_opacity(&self, id: ItemId) -> f32`

Accumulated opacity for `id` (own × each ancestor's opacity).

#### `pub fn z(&self, id: ItemId) -> Option<f32>`

The z-order value of `id` within its layer; `None` if `id` is unknown.

#### `pub fn layer(&self, id: ItemId) -> Option<SceneLayer>`

The `SceneLayer` of `id`; `None` if `id` is unknown.

#### `pub fn parent_of(&self, id: ItemId) -> Option<ItemId>`

The direct parent of `id`, or `None` if it is a root item (or unknown).

#### `pub fn is_descendant_of(&self, id: ItemId, ancestor: ItemId) -> bool`

Returns `true` if `id` is anywhere in `ancestor`'s subtree.

#### `pub fn scene_rect_extent(&self) -> Option<Rect>`

The logical extent set via `set_scene_rect`; `None` = unbounded.

#### `pub fn current_pan_axes(&self) -> PanAxes`

The current pan-axis restriction without subscribing to its signal.

#### `pub fn is_zoomable(&self) -> bool`

Returns `true` if zoom is currently enabled (snapshot; use `zoomable_signal` for reactivity).

#### `pub fn current_pan_bounds(&self) -> Option<Rect>`

Current pan-clamp bounds without subscribing to its signal.

#### `pub fn current_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>>`

Current zoom-factor clamp range without subscribing to its signal.

#### `pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId>`

All items whose bounding rects overlap `scene_rect` (spatial-index query).

#### `pub fn items_in_region( &self, region: &SceneRegion, mode: ItemSelectionMode, view_scale: f32, ) -> Vec<ItemId>`

Items matching `region` under `mode`; see `Scene::items_in_region`.

Screen-anchored
(`IGNORES_TRANSFORMATIONS`)
items are skipped, for the same reason `SceneModel::item_at` skips
them: a scene-space region cannot place them.

#### `pub fn item_shape(&self, id: ItemId) -> Option<ItemShape>`

An entry's geometry in local coordinates; see `Scene::item_shape`.

#### `pub fn item_region(&self, id: ItemId) -> Option<SceneRegion>`

An entry's own shape as a scene-space region; see `Scene::item_region`.

#### `pub fn item_contains(&self, id: ItemId, scene_pt: Point, view_scale: f32) -> bool`

Whether `id`'s shape contains `scene_pt`; see `Scene::item_contains`.

#### `pub fn paint_key(&self, id: ItemId) -> Option<crate::pick::PaintKey>`

Where `id` sits in this scene's single paint order; see
`Scene::paint_key`.

Defined for **both tiers**, so it is the value to compare when an app
needs to ask "which of these two is on top?" without re-deriving the
band/z/insertion rule.

#### `pub fn is_hit_testable(&self, id: ItemId) -> bool`

Whether `id` takes part in pointer hit-testing — visible along its whole
ancestor chain AND enabled; see `Scene::is_hit_testable`.

The public form of the filter every picker here applies, so an app can
ask why one of its items is not answering.

#### `pub fn item_at(&self, scene_pt: Point) -> Option<ItemId>`

The topmost **lightweight** item whose shape contains `scene_pt`, at
unit view scale; see `Scene::item_at` for the full contract.

"Topmost" is `Scene::paint_key` order, so an
`Over`-band item beats a higher-`z`
`Under` one and an equal-`z` tie goes to the
later-inserted entry. Hidden and disabled entries are excluded, and
heavyweight widget entries and screen-anchored
(`IGNORES_TRANSFORMATIONS`)
items are skipped — see `SceneModel::item_at_in_view` for the query
that places the latter.

#### `pub fn item_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId>`

`Scene::item_at` at an explicit view zoom. The zoom reaches exactly
one thing: a cosmetic stroke band's width in scene units.

#### `pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>`

All lightweight items whose shape contains `scene_pt`, topmost-first in
`Scene::paint_key` order. Same tier, flag and
`IGNORES_TRANSFORMATIONS` rules as `SceneModel::item_at`.

#### `pub fn items_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Vec<ItemId>`

`Scene::items_at` at an explicit view zoom.

#### `pub fn item_at_in_view(&self, screen_pt: Point, view_transform: Transform2D) -> Option<ItemId>`

The topmost item under a **screen** point, resolving both hit spaces;
see `Scene::item_at_in_view`.

#### `pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>`

Items overlapping `id`'s shape, excluding `id`; see
`Scene::colliding_items`.

#### `pub fn colliding_items_with(&self, id: ItemId, mode: ItemSelectionMode) -> Vec<ItemId>`

`Scene::colliding_items` under an explicit mode.

#### `pub fn items_along_path(&self, path: &teksilo_canvas::Path) -> Vec<ItemId>`

Items lying along `path`; see `Scene::items_along_path`. (This was
unreachable from a `SceneModel` before — the facade never forwarded it.)

#### `pub fn items_along_path_with( &self, path: &teksilo_canvas::Path, stroke_width: f32, mode: ItemSelectionMode, ) -> Vec<ItemId>`

`Scene::items_along_path` with an explicit stroke width and mode.

#### `pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode>`

The AT-tree parent of `child` as set by `set_a11y_parent`; `None` = visual default.

## `pub struct SceneWriteGuard`

Write guard over a `SceneModel`'s [`Scene`]: `Deref`/`DerefMut` to the
scene, with the deferred-notification contract of a `SceneModel` mutator.

Holding one keeps a *write scope* open, so every notification the edits
produce queues in emission order. On drop the guard closes the scope,
releases the `RefCell` borrow, and **then** fans the whole batch out — so a
block of edits is one notification round, and an observer may read the
scene or write it back.

```
# use teksilo_scene::{RectItem, SceneModel};
# use teksilo_canvas::{Point, Rect};
let model = SceneModel::new();
let a = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
{
    let mut scene = model.write_guard();
    scene.set_local_pos(a, Point::new(10.0, 10.0));
    scene.set_visible(a, false);
} // both changes fan out here, in this order
```

If the thread is unwinding when the guard drops, the scope is closed and
the borrow released but the fan-out is **skipped**: running observers from
a `Drop` during an unwind would abort the process the moment one of them
panicked. The queued changes survive and are delivered by the next
`flush_changes` — which a `catch_unwind`
recovery path can call explicitly.

```rust
pub struct SceneWriteGuard<'a> { /* fields */ }
```
