<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneModel

`SceneModel` — a shared, cloneable handle to a `Scene`.

## Public types

| Kind | Name |
| ---: | :--- |
| `struct` | [`SceneModel`](#scenemodel) — A shared, cloneable handle to a `Scene` |
| `struct` | [`WeakSceneModel`](#weakscenemodel) — A non-owning handle to a `Scene` — `SceneModel` without the ownership |
| `struct` | [`SceneWriteGuard`](#scenewriteguard) — Write guard over a `SceneModel`'s `Scene`: `Deref`/`DerefMut` to the scene, with the deferred-notification contract of a `SceneModel` mutator |
| `struct` | [`SceneTransaction`](#scenetransaction) |

## Public functions

### `SceneModel`

| Returns | Function |
| ---: | :--- |
| | **Constructors** |
| `Self` | [`new()`](#scenemodel-new) |
| `Self` | [`with_index(index: Box<dyn SpatialIndex>)`](#scenemodel-with_index) |
| `Self` | [`from_scene(scene: Scene)`](#scenemodel-from_scene) |
| | **Methods** |
| `SceneWriteGuard<'_>` | [`write_guard()`](#scenemodel-write_guard) |
|  | [`flush_changes()`](#scenemodel-flush_changes) |
|  | [`deliver_records()`](#scenemodel-deliver_records) |
| `SceneTransaction<'_>` | [`transaction(source: ChangeSource, history: HistoryMode)`](#scenemodel-transaction) |
| `SceneTransaction<'_>` | [`user_edit()`](#scenemodel-user_edit) |
| `Option<Box<dyn FnMut(SceneTransactionRecord)>>` | [`set_edit_sink(sink: impl FnMut(SceneTransactionRecord) + 'static)`](#scenemodel-set_edit_sink) |
| `Option<Box<dyn FnMut(SceneTransactionRecord)>>` | [`clear_edit_sink()`](#scenemodel-clear_edit_sink) |
| `Signal<TxnId>` | [`transaction_signal()`](#scenemodel-transaction_signal) |
| `u32` | [`open_transaction_depth()`](#scenemodel-open_transaction_depth) |
| `CascadeBudget` | [`cascade_budget()`](#scenemodel-cascade_budget) |
|  | [`set_cascade_budget(budget: CascadeBudget)`](#scenemodel-set_cascade_budget) |
| `usize` | [`handle_count()`](#scenemodel-handle_count) |
| `WeakSceneModel` | [`downgrade()`](#scenemodel-downgrade) |
| `ItemId` | [`add_widget<W: Widget + 'static>(widget: W, rect: Rect)`](#scenemodel-add_widget) |
| `ItemId` | [`add_widget_item<P: 'static>(payload: P, rect: Rect)`](#scenemodel-add_widget_item) |
|  | [`set_payload<P: 'static>(id: ItemId, payload: P)`](#scenemodel-set_payload) |
| `Option<Rc<dyn std::any::Any>>` | [`payload(id: ItemId)`](#scenemodel-payload) |
| `ItemId` | [`add_item<I: SceneItem + 'static>(item: I, local_pos: Point)`](#scenemodel-add_item) |
| `ItemId` | [`add_item_dynamic<I: SceneItem + 'static>(item: I, local_pos: Point)`](#scenemodel-add_item_dynamic) |
| `ItemId` | [`add_boxed_item(item: Box<dyn SceneItem>, local_pos: Point)`](#scenemodel-add_boxed_item) |
|  | [`set_local_pos(id: ItemId, local_pos: Point)`](#scenemodel-set_local_pos) |
|  | [`set_local_bounds(id: ItemId, local_bounds: Rect)`](#scenemodel-set_local_bounds) |
|  | [`set_transform(id: ItemId, transform: Transform2D)`](#scenemodel-set_transform) |
| `SizePolicy` | [`size_policy(id: ItemId)`](#scenemodel-size_policy) |
| `bool` | [`set_size_policy(id: ItemId, policy: SizePolicy)`](#scenemodel-set_size_policy) |
| `bool` | [`set_measured_size(id: ItemId, size: Size)`](#scenemodel-set_measured_size) |
|  | [`set_geometry_constraint(f: impl Fn(&crate::constrain::ProposedChange<'_>) -> crate::constrain::ChangeVerdict + 'static)`](#scenemodel-set_geometry_constraint) |
|  | [`clear_geometry_constraint()`](#scenemodel-clear_geometry_constraint) |
| `bool` | [`has_geometry_constraint()`](#scenemodel-has_geometry_constraint) |
| `crate::transform_session::TransformFrame` | [`constrain_frame(items: &[ItemId], op: crate::transform_session::TransformOp, start: crate::transform_session::TransformFrame, proposed: crate::transform_session::TransformFrame, source: crate::transform_session::TransformSource)`](#scenemodel-constrain_frame) |
| `Vec2` | [`constrain_move(items: &[ItemId], start: crate::transform_session::TransformFrame, translation: Vec2, source: crate::transform_session::TransformSource)`](#scenemodel-constrain_move) |
| `usize` | [`apply_transform_delta(roots: &[ItemId], delta: &crate::transform_session::TransformDelta)`](#scenemodel-apply_transform_delta) |
| `Vec<ItemId>` | [`selection_roots(ids: &[ItemId])`](#scenemodel-selection_roots) |
| `Vec<ItemId>` | [`transformable_roots(ids: &[ItemId], op: crate::transform_session::TransformOp)`](#scenemodel-transformable_roots) |
| `Option<crate::transform_session::TransformFrame>` | [`transform_frame(roots: &[ItemId])`](#scenemodel-transform_frame) |
| `Option<f32>` | [`scene_rotation(id: ItemId)`](#scenemodel-scene_rotation) |
|  | [`set_flags(id: ItemId, flags: ItemFlags)`](#scenemodel-set_flags) |
|  | [`set_flag(id: ItemId, flag: ItemFlags, on: bool)`](#scenemodel-set_flag) |
|  | [`set_visible(id: ItemId, visible: bool)`](#scenemodel-set_visible) |
|  | [`set_opacity(id: ItemId, opacity: f32)`](#scenemodel-set_opacity) |
|  | [`set_item_fill(id: ItemId, fill: impl Into<ColorProp>)`](#scenemodel-set_item_fill) |
|  | [`clear_item_fill(id: ItemId)`](#scenemodel-clear_item_fill) |
|  | [`set_item_stroke(id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle)`](#scenemodel-set_item_stroke) |
|  | [`clear_item_stroke(id: ItemId)`](#scenemodel-clear_item_stroke) |
|  | [`set_z(id: ItemId, z: f32)`](#scenemodel-set_z) |
|  | [`bring_to_front(id: ItemId)`](#scenemodel-bring_to_front) |
|  | [`send_to_back(id: ItemId)`](#scenemodel-send_to_back) |
|  | [`set_layer(id: ItemId, layer: SceneLayer)`](#scenemodel-set_layer) |
|  | [`set_item_parent(child: ItemId, parent: Option<ItemId>)`](#scenemodel-set_item_parent) |
|  | [`remove(id: ItemId)`](#scenemodel-remove) |
| `Vec<RemovedItem>` | [`take(id: ItemId)`](#scenemodel-take) |
| `Result<ItemId, RestoreError>` | [`restore(salvage: RemovedItem)`](#scenemodel-restore) |
| `Result<Vec<ItemId>, RestoreError>` | [`restore_all(salvage: Vec<RemovedItem>)`](#scenemodel-restore_all) |
| `Result<Box<dyn SceneItem>, ReplaceRejected>` | [`replace_item(id: ItemId, item: Box<dyn SceneItem>)`](#scenemodel-replace_item) |
| `Option<Placement>` | [`placement(id: ItemId)`](#scenemodel-placement) |
|  | [`set_placement(id: ItemId, placement: Placement)`](#scenemodel-set_placement) |
|  | [`reparent_keeping_scene_pos(id: ItemId, parent: Option<ItemId>)`](#scenemodel-reparent_keeping_scene_pos) |
| `Option<f32>` | [`z_between(below: ItemId, above: ItemId)`](#scenemodel-z_between) |
|  | [`orphan(id: ItemId)`](#scenemodel-orphan) |
|  | [`set_item_handlers(id: ItemId, handlers: Option<SceneItemHandlerSet>)`](#scenemodel-set_item_handlers) |
| `Option<R>` | [`with_handlers_mut<R>(id: ItemId, f: impl FnOnce(&mut SceneItemHandlerSet) -> R)`](#scenemodel-with_handlers_mut) |
| `MagnetId` | [`add_magnet(item: ItemId, magnet: Magnet)`](#scenemodel-add_magnet) |
|  | [`remove_magnet(magnet: MagnetId)`](#scenemodel-remove_magnet) |
|  | [`clear_magnets(item: ItemId)`](#scenemodel-clear_magnets) |
|  | [`set_magnet_local_pos(magnet: MagnetId, local_pos: Point)`](#scenemodel-set_magnet_local_pos) |
|  | [`set_magnet_enabled(magnet: MagnetId, enabled: bool)`](#scenemodel-set_magnet_enabled) |
| `Vec<MagnetId>` | [`magnet_ids_of(item: ItemId)`](#scenemodel-magnet_ids_of) |
| `Option<ItemId>` | [`magnet_owner(magnet: MagnetId)`](#scenemodel-magnet_owner) |
| `Option<Point>` | [`magnet_scene_pos(magnet: MagnetId)`](#scenemodel-magnet_scene_pos) |
| `Option<MagnetRef>` | [`magnet(magnet: MagnetId)`](#scenemodel-magnet) |
| `Option<MagnetSnap>` | [`compute_item_snap(dragged: ItemId, drag_delta: Vec2, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict)`](#scenemodel-compute_item_snap) |
| `Option<(MagnetRef, Option<std::rc::Rc<dyn std::any::Any>>)>` | [`compute_port_snap(source: MagnetId, cursor_scene: Point, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict)`](#scenemodel-compute_port_snap) |
| `Option<MagnetId>` | [`nearest_magnet(scene_pt: Point, radius: f32)`](#scenemodel-nearest_magnet) |
|  | [`set_scene_rect(rect: Option<Rect>)`](#scenemodel-set_scene_rect) |
|  | [`pan_axes(axes: PanAxes)`](#scenemodel-pan_axes) |
|  | [`zoomable(on: bool)`](#scenemodel-zoomable) |
|  | [`set_pan_bounds(bounds: Option<Rect>)`](#scenemodel-set_pan_bounds) |
|  | [`set_zoom_range(range: Option<std::ops::RangeInclusive<f32>>)`](#scenemodel-set_zoom_range) |
| `A11yGroupId` | [`add_a11y_group(builder: A11yGroupBuilder)`](#scenemodel-add_a11y_group) |
|  | [`remove_a11y_group(id: A11yGroupId)`](#scenemodel-remove_a11y_group) |
|  | [`set_a11y_parent(child: A11yNode, parent: Option<A11yNode>)`](#scenemodel-set_a11y_parent) |
|  | [`add_a11y_relation(from: A11yNode, kind: A11yRelation, to: A11yNode)`](#scenemodel-add_a11y_relation) |
|  | [`set_a11y_live(node: A11yNode, live: accesskit::Live)`](#scenemodel-set_a11y_live) |
|  | [`set_a11y_landmark(node: A11yNode, role: accesskit::Role)`](#scenemodel-set_a11y_landmark) |
|  | [`set_a11y_categories(node: A11yNode, categories: &[A11yCategory])`](#scenemodel-set_a11y_categories) |
| `bool` | [`refresh_dynamic_bounds()`](#scenemodel-refresh_dynamic_bounds) |
| `Signal<SceneChange>` | [`item_change_signal()`](#scenemodel-item_change_signal) |
| `Signal<u64>` | [`a11y_change_signal()`](#scenemodel-a11y_change_signal) |
| `u64` | [`mutation_version()`](#scenemodel-mutation_version) |
| `u64` | [`structural_version()`](#scenemodel-structural_version) |
| `Signal<PanAxes>` | [`pan_axes_signal()`](#scenemodel-pan_axes_signal) |
| `Signal<Option<Rect>>` | [`pan_bounds_signal()`](#scenemodel-pan_bounds_signal) |
| `Signal<Option<std::ops::RangeInclusive<f32>>>` | [`zoom_range_signal()`](#scenemodel-zoom_range_signal) |
| `Signal<bool>` | [`zoomable_signal()`](#scenemodel-zoomable_signal) |
| `usize` | [`len()`](#scenemodel-len) |
| `bool` | [`is_empty()`](#scenemodel-is_empty) |
| `Vec<ItemId>` | [`ids()`](#scenemodel-ids) |
| `Option<Point>` | [`local_pos(id: ItemId)`](#scenemodel-local_pos) |
| `Option<Rect>` | [`local_bounds(id: ItemId)`](#scenemodel-local_bounds) |
| `Option<Transform2D>` | [`transform(id: ItemId)`](#scenemodel-transform) |
| `Transform2D` | [`scene_transform(id: ItemId)`](#scenemodel-scene_transform) |
| `Option<Point>` | [`scene_pos(id: ItemId)`](#scenemodel-scene_pos) |
| `Option<Rect>` | [`scene_rect(id: ItemId)`](#scenemodel-scene_rect) |
| `Option<ItemFlags>` | [`flags(id: ItemId)`](#scenemodel-flags) |
| `bool` | [`is_effectively_visible(id: ItemId)`](#scenemodel-is_effectively_visible) |
| `Option<f32>` | [`opacity(id: ItemId)`](#scenemodel-opacity) |
| `f32` | [`effective_opacity(id: ItemId)`](#scenemodel-effective_opacity) |
| `Option<f32>` | [`z(id: ItemId)`](#scenemodel-z) |
| `Option<SceneLayer>` | [`layer(id: ItemId)`](#scenemodel-layer) |
| `Option<ItemId>` | [`parent_of(id: ItemId)`](#scenemodel-parent_of) |
| `bool` | [`is_descendant_of(id: ItemId, ancestor: ItemId)`](#scenemodel-is_descendant_of) |
| `Option<Rect>` | [`scene_rect_extent()`](#scenemodel-scene_rect_extent) |
| `PanAxes` | [`current_pan_axes()`](#scenemodel-current_pan_axes) |
| `bool` | [`is_zoomable()`](#scenemodel-is_zoomable) |
| `Option<Rect>` | [`current_pan_bounds()`](#scenemodel-current_pan_bounds) |
| `Option<std::ops::RangeInclusive<f32>>` | [`current_zoom_range()`](#scenemodel-current_zoom_range) |
| `Vec<ItemId>` | [`items_in_rect(scene_rect: Rect)`](#scenemodel-items_in_rect) |
| `Vec<ItemId>` | [`items_in_region(region: &SceneRegion, mode: ItemSelectionMode, view_scale: f32)`](#scenemodel-items_in_region) |
| `Option<ItemShape>` | [`item_shape(id: ItemId)`](#scenemodel-item_shape) |
| `Option<SceneRegion>` | [`item_region(id: ItemId)`](#scenemodel-item_region) |
| `bool` | [`item_contains(id: ItemId, scene_pt: Point, view_scale: f32)`](#scenemodel-item_contains) |
| `Option<crate::pick::PaintKey>` | [`paint_key(id: ItemId)`](#scenemodel-paint_key) |
| `bool` | [`is_hit_testable(id: ItemId)`](#scenemodel-is_hit_testable) |
| `Option<ItemId>` | [`item_at(scene_pt: Point)`](#scenemodel-item_at) |
| `Option<ItemId>` | [`item_at_scaled(scene_pt: Point, view_scale: f32)`](#scenemodel-item_at_scaled) |
| `Vec<ItemId>` | [`items_at(scene_pt: Point)`](#scenemodel-items_at) |
| `Vec<ItemId>` | [`items_at_scaled(scene_pt: Point, view_scale: f32)`](#scenemodel-items_at_scaled) |
| `Option<ItemId>` | [`item_at_in_view(screen_pt: Point, view_transform: Transform2D)`](#scenemodel-item_at_in_view) |
| `Vec<ItemId>` | [`colliding_items(id: ItemId)`](#scenemodel-colliding_items) |
| `Vec<ItemId>` | [`colliding_items_with(id: ItemId, mode: ItemSelectionMode)`](#scenemodel-colliding_items_with) |
| `Vec<ItemId>` | [`items_along_path(path: &teksilo_canvas::Path)`](#scenemodel-items_along_path) |
| `Vec<ItemId>` | [`items_along_path_with(path: &teksilo_canvas::Path, stroke_width: f32, mode: ItemSelectionMode)`](#scenemodel-items_along_path_with) |
| `Option<A11yNode>` | [`a11y_parent_of(child: A11yNode)`](#scenemodel-a11y_parent_of) |
| `Vec<(A11yNode, A11yRelation, A11yNode)>` | [`a11y_relations()`](#scenemodel-a11y_relations) |
| `Option<accesskit::Live>` | [`a11y_live_of(node: A11yNode)`](#scenemodel-a11y_live_of) |
| `Option<accesskit::Role>` | [`a11y_landmark_of(node: A11yNode)`](#scenemodel-a11y_landmark_of) |
| `Vec<A11yCategory>` | [`a11y_categories_of(node: A11yNode)`](#scenemodel-a11y_categories_of) |

### `WeakSceneModel`

| Returns | Function |
| ---: | :--- |
| | **Methods** |
| `Option<SceneModel>` | [`upgrade()`](#weakscenemodel-upgrade) |
| `bool` | [`is_alive()`](#weakscenemodel-is_alive) |

### `SceneTransaction`

| Returns | Function |
| ---: | :--- |
| | **Builder methods** |
| `Self` | [`squash()`](#scenetransaction-squash) |
| `Self` | [`ephemeral()`](#scenetransaction-ephemeral) |
| | **Methods** |
|  | [`abandon()`](#scenetransaction-abandon) |

## Detailed description

Mirrors the `ListModel = Rc<RefCell<ListModelInner>>` pattern from
`teksilo-data`: cloning a `SceneModel` produces a **second handle to the
same scene**, so multiple `SceneView`s can render one
scene (overview + detail panes, same-document multi-window, headless model
reuse). Mutate the model once and every attached view reconciles.

#### Heavyweight content across views

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

#### Borrow / observer contract

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

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

<a id="scenemodel"></a>

## `pub struct SceneModel`

A shared, cloneable handle to a `Scene`.

```rust
pub struct SceneModel(pub(crate) Rc<RefCell<Scene>>);
```

### Methods

<a id="scenemodel-write_guard"></a>

#### `pub fn write_guard(&self) -> SceneWriteGuard<'_>`

Open a write scope over the scene; see `SceneWriteGuard`.

The one door for a *block* of edits that must fan out together, and the
door a `&mut Scene` escape hatch should be built on.

<a id="scenemodel-flush_changes"></a>

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
when the value is unchanged, so a clamp-it-back-into-bounds observer
settles in two rounds. (Snapping a *gesture* is not this channel's job at
all; see `set_geometry_constraint`.)

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

<a id="scenemodel-deliver_records"></a>

#### `pub fn deliver_records(&self)`

Hand every committed transaction to the edit sink, with the scene
unborrowed.

Called automatically wherever a transaction closes — the end of a write
scope, the drop of a `SceneTransaction`. Apps rarely need it; the
exception is the same as `flush_changes`', a
`catch_unwind` recovery path.

A no-op when nothing is queued, when the scene is borrowed at all (the
sink is allowed to write, and a write needs the cell exclusively), when
a delivery is already running, and while the **change** fan-out is
mid-drain.

That last one is the ordering this seam promises: an observer that
writes the scene commits its own transaction from inside the drain, and
delivering from there would hand the edit sink a transaction some views
had not reconciled from yet. The records wait, and the drain's own
caller delivers them when it settles.

<a id="scenemodel-transaction"></a>

#### `pub fn transaction(&self, source: ChangeSource, history: HistoryMode) -> SceneTransaction<'_>`

Group every edit until the returned guard drops into **one**
transaction, stamped with a `ChangeSource` and a `HistoryMode`.

Each `SceneChange` emitted inside it carries the same `TxnId` and
the same stamp, and — when an edit sink is installed — the whole group
arrives as one `SceneTransactionRecord` once the guard drops.

# Nesting joins

An inner `transaction` adds no boundary: it joins the open one and its
stamp is ignored, so an observer opening its own transaction inside a
framework-opened gesture cannot split that gesture into two undo steps.

# Why the guard is safe to hold

Every mutator on this type borrows the scene for the length of one call
and releases it at the semicolon. The guard is held by the *caller*,
outside any borrow, so its `Drop` runs with the `RefCell` free — which is
what lets the edit sink read the scene and write it back.

A transaction is a **synchronous scope**. Holding one across a frame
means its edits are never delivered and its salvage accumulates; a debug
assertion in `SceneView`'s build catches the common
case.

```
# use teksilo_canvas::{Point, Rect};
# use teksilo_scene::{ChangeSource, HistoryMode, RectItem, SceneModel};
let model = SceneModel::new();
let a = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
let b = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
{
    let _txn = model.transaction(ChangeSource::User, HistoryMode::Record);
    model.set_local_pos(a, Point::new(10.0, 0.0));
    model.set_local_pos(b, Point::new(20.0, 0.0));
} // one transaction, two edits
```

<a id="scenemodel-user_edit"></a>

#### `pub fn user_edit(&self) -> SceneTransaction<'_>`

`transaction``(ChangeSource::User,
HistoryMode::Record)` — one user-visible edit.

<a id="scenemodel-set_edit_sink"></a>

#### `pub fn set_edit_sink( &self, sink: impl FnMut(SceneTransactionRecord) + 'static, ) -> Option<Box<dyn FnMut(SceneTransactionRecord)>>`

Install the edit sink, returning whatever was there.

Invoked once per committed transaction that changed anything, with the
scene **unborrowed** — so the sink may read the scene and write it back.
Its own writes are ordinary transactions: they produce their own records
and are delivered to it on a later round of the same loop, which is what
keeps the history it is feeding in agreement with the scene.

**Exactly one.** A scene's edit history has one owner; a second consumer
composes inside the first. Owning is why: the sink is handed the removed
items themselves, and two owners of one `Box` is not a thing.

With no sink installed, nothing is recorded and a removal drops its
salvage — today's behaviour at today's cost, for an app that wants no
history.

# This is not where snap-to-grid goes

The sink runs *after* the write. Snapping a gesture belongs in
`set_geometry_constraint`, which runs
*before* it and is consulted by every gesture route — so the frame the
last preview drew is the frame the commit writes. Use the sink for what
happens once an edit is real: recording it, persisting it, marking the
document dirty, mirroring it to a peer.

# Panics

If called from inside the sink itself.

<a id="scenemodel-clear_edit_sink"></a>

#### `pub fn clear_edit_sink(&self) -> Option<Box<dyn FnMut(SceneTransactionRecord)>>`

Remove the edit sink, returning it. The scene stops recording.

<a id="scenemodel-transaction_signal"></a>

#### `pub fn transaction_signal(&self) -> Signal<TxnId>`

Fires once per committed transaction, **after** the edit sink, with the
scene still unborrowed.

The per-gesture counterpart of
`item_change_signal`: one notification for a
whole logical edit rather than one per intermediate write. This is where
a document-dirty flag, a debounced save or a validation pass belongs —
they want to run once when the user lets go, not six times while the
item is moving.

<a id="scenemodel-open_transaction_depth"></a>

#### `pub fn open_transaction_depth(&self) -> u32`

How many transaction scopes are currently open on this scene — zero
between mutations.

A transaction is a **synchronous** scope: holding one across a frame
means its edits never reach the sink and its salvage accumulates. There
is no way to enforce that at the type level (nothing stops a guard being
stashed in a struct field), so this is the door an assertion knocks on.
`SceneView` asserts it is zero at the top of every
build in debug, which catches the common case.

<a id="scenemodel-cascade_budget"></a>

#### `pub fn cascade_budget(&self) -> CascadeBudget`

The runaway-detection budget for this scene's change fan-out — how much
work **observers** may generate from one batch before the drain declares
the cascade non-terminating and panics.

See `CascadeBudget` for the rule and its default.

<a id="scenemodel-set_cascade_budget"></a>

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

<a id="scenemodel-new"></a>

#### `pub fn new() -> Self`

A handle to a fresh empty scene with the default spatial index.

<a id="scenemodel-with_index"></a>

#### `pub fn with_index(index: Box<dyn SpatialIndex>) -> Self`

A handle to a fresh scene with a custom `SpatialIndex`.

<a id="scenemodel-from_scene"></a>

#### `pub fn from_scene(scene: Scene) -> Self`

Wrap an existing `Scene` in a handle. Used by
`SceneView::new` for the single-view path.

<a id="scenemodel-handle_count"></a>

#### `pub fn handle_count(&self) -> usize`

Number of distinct handles to this scene (1 = unshared).

<a id="scenemodel-downgrade"></a>

#### `pub fn downgrade(&self) -> WeakSceneModel`

A **non-owning** handle to the same scene.

For a closure the scene itself owns. The geometry constraint is the one
in this crate: the scene holds it, so a closure holding a `SceneModel`
back closes a reference cycle and the whole scene — items, heavyweight
payloads, journal, spatial index — is never dropped. A constraint
normally needs no handle at all (it is handed
`ProposedChange::scene`, which is the
whole read surface); this is for the case where a policy object holds a
model for its *other* work and is also installed as the closure.

```
# use teksilo_scene::SceneModel;
let model = SceneModel::new();
let weak = model.downgrade();
assert!(weak.upgrade().is_some());
drop(model);
assert!(weak.upgrade().is_none());
```

<a id="scenemodel-add_widget"></a>

#### `pub fn add_widget<W: Widget + 'static>(&self, widget: W, rect: Rect) -> ItemId`

Single-view heavyweight widget (the one-shot `Once` path). The first
view to build drains it; a second view sharing this model produces no
child for it. For multi-view, use `add_widget_item`.

<a id="scenemodel-add_widget_item"></a>

#### `pub fn add_widget_item<P: 'static>(&self, payload: P, rect: Rect) -> ItemId`

Multi-view heavyweight item: store a typed `payload`; each view builds
its own widget instance from it via its delegate. Returns the `ItemId`.

<a id="scenemodel-set_payload"></a>

#### `pub fn set_payload<P: 'static>(&self, id: ItemId, payload: P)`

Replace the payload of a `Delegated` heavyweight item; every view
rebuilds that item's widget on the next pass.

# Panics

Panics if `id` is unknown, refers to a single-view `add_widget` (Once)
entry, or refers to a lightweight item.

<a id="scenemodel-payload"></a>

#### `pub fn payload(&self, id: ItemId) -> Option<Rc<dyn std::any::Any>>`

The current type-erased payload of a `Delegated` item, if any.

<a id="scenemodel-add_item"></a>

#### `pub fn add_item<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId`

Add a lightweight `SceneItem` at `local_pos`.

<a id="scenemodel-add_item_dynamic"></a>

#### `pub fn add_item_dynamic<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId`

Add a lightweight item with signal-driven (dynamic) bounds.

<a id="scenemodel-add_boxed_item"></a>

#### `pub fn add_boxed_item(&self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId`

Add an already-boxed lightweight item at `local_pos`. The boxed-`dyn`
counterpart of `add_item`, used by
`SceneListAdapter`.

<a id="scenemodel-set_local_pos"></a>

#### `pub fn set_local_pos(&self, id: ItemId, local_pos: Point)`

Move `id` to `local_pos` in its parent's coordinate space; notifies all views.

<a id="scenemodel-set_local_bounds"></a>

#### `pub fn set_local_bounds(&self, id: ItemId, local_bounds: Rect)`

Replace the local bounding rect of `id`; notifies all views.

Idempotent, and an item whose box is derived from its geometry fits
itself to the rectangle rather than adopting it — see
`Scene::set_local_bounds` for both contracts, which matter here
because this is the form an app drives per frame.

<a id="scenemodel-set_transform"></a>

#### `pub fn set_transform(&self, id: ItemId, transform: Transform2D)`

Set an additional local-to-parent transform (rotation, scale) on `id`; notifies all views.

<a id="scenemodel-size_policy"></a>

#### `pub fn size_policy(&self, id: ItemId) -> SizePolicy`

Which axes of `id`'s box its widget decides. See `SizePolicy`.

<a id="scenemodel-set_size_policy"></a>

#### `pub fn set_size_policy(&self, id: ItemId, policy: SizePolicy) -> bool`

Hand one or both axes of `id`'s box to its widget, on **every** view of
this model. See `SizePolicy` and `Scene::set_size_policy` — including
for the note that it is refused for a lightweight entry.

<a id="scenemodel-set_measured_size"></a>

#### `pub fn set_measured_size(&self, id: ItemId, size: Size) -> bool`

Write a size **derived** from content rather than authored by the user;
notifies all views. See `Scene::set_measured_size` for which of the two
geometry doors this is and when to reach for the other one.

<a id="scenemodel-set_geometry_constraint"></a>

#### `pub fn set_geometry_constraint( &self, f: impl Fn(&crate::constrain::ProposedChange<'_>) -> crate::constrain::ChangeVerdict + 'static, )`

Install the document's standing geometry rule — snap-to-grid, axis lock,
page-bounds clamp — consulted before every **user-driven** gesture
applies anything, on every view attached to this model.

One per scene. Programmatic mutators
(`set_local_pos` and friends) are **never**
constrained: a document load must land where it says.

The closure is handed a `ProposedChange` —
the scene read-only, the roots the gesture moves, and the gesture's
start and proposed *frames* in scene coordinates — and returns a
`ChangeVerdict`. It may read the scene and may
not write it; a write panics naming this hook.

```
use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ChangeVerdict, RectItem, SceneModel};

let model = SceneModel::new();
let card = model.add_item(
    RectItem::new(Rect::new(0.0, 0.0, 80.0, 50.0)),
    Point::new(4.0, 4.0),
);

let grid = 25.0;
let page = Rect::new(0.0, 0.0, 1000.0, 700.0);
model.set_geometry_constraint(move |c| {
    // An explicit magnet outranks the standing rule.
    if c.magnet_snapped {
        return ChangeVerdict::Accept;
    }
    let mut f = c.proposed;
    // The frame is the item's own box in scene coordinates, so this
    // snaps what the user can see — no grab offset to carry.
    f.rect.x = (f.rect.x / grid).round() * grid;
    f.rect.y = (f.rect.y / grid).round() * grid;
    f.rect.x = f.rect.x.clamp(page.x, page.right() - f.rect.width);
    f.rect.y = f.rect.y.clamp(page.y, page.bottom() - f.rect.height);
    ChangeVerdict::Adjust(f)
});

// Programmatic writes are exact — the constraint does not see them.
model.set_local_pos(card, Point::new(7.0, 9.0));
assert_eq!(model.local_pos(card), Some(Point::new(7.0, 9.0)));
```

<a id="scenemodel-clear_geometry_constraint"></a>

#### `pub fn clear_geometry_constraint(&self)`

Remove the geometry constraint. See `Scene::clear_geometry_constraint`.

<a id="scenemodel-has_geometry_constraint"></a>

#### `pub fn has_geometry_constraint(&self) -> bool`

Whether a geometry constraint is installed.

<a id="scenemodel-constrain_frame"></a>

#### `pub fn constrain_frame( &self, items: &[ItemId], op: crate::transform_session::TransformOp, start: crate::transform_session::TransformFrame, proposed: crate::transform_session::TransformFrame, source: crate::transform_session::TransformSource, ) -> crate::transform_session::TransformFrame`

Run the installed constraint over a proposed **frame** and return the
frame to apply. Returns `proposed` unchanged when none is installed.

The general door for an app that drives its own gesture — a heavyweight
card's `on_drag`, a custom resize grip, a layout command. The
`SceneView`'s own routes call exactly this, so an
app-driven gesture and a framework-driven one obey the same rule.

`start` is the frame the gesture began from
(`transform_frame` at press time), and it must
stay fixed for the gesture's life — that is what lets a constraint
measure total travel, and what keeps the preview and the commit in
agreement.

# Panics

Twice, each naming what to do instead:

* From inside a constraint that is already running on this scene — the
  constraint *is* the answer, and asking for another one would recurse
  without end.
* While a **write scope** is open on this model — a
  `SceneWriteGuard`, any mutator, or an observer running inside one.
  Asking the constraint reads the scene, and an open write scope holds
  it exclusively. Another *shared* borrow is fine: two reads coexist,
  which is what lets a constraint read the scene it is deciding about.

<a id="scenemodel-constrain_move"></a>

#### `pub fn constrain_move( &self, items: &[ItemId], start: crate::transform_session::TransformFrame, translation: Vec2, source: crate::transform_session::TransformSource, ) -> Vec2`

Run the installed constraint over a proposed **move** and return the
scene-space translation to apply. Returns `translation` unchanged when
none is installed.

The door a heavyweight card's own drag handler uses, and the reason the
constraint lives on the model rather than on a view — a card built by a
per-view delegate holds a `SceneModel` clone and never a `&SceneView`:

```
# use teksilo_canvas::{Point, Rect, Vec2};
# use teksilo_scene::{ChangeVerdict, SceneModel, TransformSource};
# let model = SceneModel::new();
# let card = model.add_widget_item((), Rect::new(0.0, 0.0, 80.0, 50.0));
# model.set_geometry_constraint(|c| {
#     let mut f = c.proposed;
#     f.rect.x = (f.rect.x / 25.0).round() * 25.0;
#     f.rect.y = (f.rect.y / 25.0).round() * 25.0;
#     ChangeVerdict::Adjust(f)
# });
// At the press: the frame the gesture starts from.
let start = model.transform_frame(&[card]).expect("card resolves");
// On each sample: the raw travel, constrained.
let travel = Vec2::new(31.0, 12.0);
let applied = model.constrain_move(&[card], start, travel, TransformSource::Pointer);
assert_eq!(applied, Vec2::new(25.0, 0.0));
```

Only the frame's **origin** is read back, because a translation is all a
move can express: a constraint that also resizes or rotates the frame
here has that part of its answer ignored. Use
`constrain_frame` plus
`apply_transform_delta` for a gesture
that changes extent or orientation.

# Panics

The same two as `constrain_frame`: from inside
a running constraint, and while a write scope is open on this model.

<a id="scenemodel-apply_transform_delta"></a>

#### `pub fn apply_transform_delta( &self, roots: &[ItemId], delta: &crate::transform_session::TransformDelta, ) -> usize`

Apply one transform delta to `roots` as a single operation; notifies all
views. See `Scene::apply_transform_delta`.

<a id="scenemodel-selection_roots"></a>

#### `pub fn selection_roots(&self, ids: &[ItemId]) -> Vec<ItemId>`

`ids` pruned to its roots. See `Scene::selection_roots`.

<a id="scenemodel-transformable_roots"></a>

#### `pub fn transformable_roots( &self, ids: &[ItemId], op: crate::transform_session::TransformOp, ) -> Vec<ItemId>`

The roots of `ids` eligible for `op`. See `Scene::transformable_roots`.

<a id="scenemodel-transform_frame"></a>

#### `pub fn transform_frame( &self, roots: &[ItemId], ) -> Option<crate::transform_session::TransformFrame>`

The selection frame enclosing `roots`. See `Scene::transform_frame`.

<a id="scenemodel-scene_rotation"></a>

#### `pub fn scene_rotation(&self, id: ItemId) -> Option<f32>`

The item's own rotation in scene space, in radians. See
`Scene::scene_rotation`.

<a id="scenemodel-set_flags"></a>

#### `pub fn set_flags(&self, id: ItemId, flags: ItemFlags)`

Replace the complete `ItemFlags` bitset for `id`; notifies all views.

<a id="scenemodel-set_flag"></a>

#### `pub fn set_flag(&self, id: ItemId, flag: ItemFlags, on: bool)`

Set or clear a single `ItemFlags` bit on `id`; notifies all views.

<a id="scenemodel-set_visible"></a>

#### `pub fn set_visible(&self, id: ItemId, visible: bool)`

Show or hide `id` (also hides its descendants); notifies all views.

<a id="scenemodel-set_opacity"></a>

#### `pub fn set_opacity(&self, id: ItemId, opacity: f32)`

Set the paint opacity of `id` (0.0 = transparent, 1.0 = opaque); notifies all views.

<a id="scenemodel-set_item_fill"></a>

#### `pub fn set_item_fill(&self, id: ItemId, fill: impl Into<ColorProp>)`

Replace a lightweight item's fill colour live; every view repaints
(no relayout/rebuild). Accepts a plain `Color`,
a theme role, a `Signal<Color>`, or a `Signal<Role>`. See
`Scene::set_item_fill` for the reactive-colour contract.

<a id="scenemodel-clear_item_fill"></a>

#### `pub fn clear_item_fill(&self, id: ItemId)`

Clear a lightweight item's fill; every view repaints.

<a id="scenemodel-set_item_stroke"></a>

#### `pub fn set_item_stroke(&self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle)`

Replace a lightweight item's stroke (colour + `StrokeStyle`) live;
every view repaints (no relayout/rebuild).

<a id="scenemodel-clear_item_stroke"></a>

#### `pub fn clear_item_stroke(&self, id: ItemId)`

Clear a lightweight item's stroke; every view repaints.

<a id="scenemodel-set_z"></a>

#### `pub fn set_z(&self, id: ItemId, z: f32)`

Set the z-order of `id` within its layer; higher values paint on top.

<a id="scenemodel-bring_to_front"></a>

#### `pub fn bring_to_front(&self, id: ItemId)`

Give `id` the highest z-value in its layer so it paints on top of all siblings.

<a id="scenemodel-send_to_back"></a>

#### `pub fn send_to_back(&self, id: ItemId)`

Give `id` the lowest z-value in its layer so it paints beneath all siblings.

<a id="scenemodel-set_layer"></a>

#### `pub fn set_layer(&self, id: ItemId, layer: SceneLayer)`

Move `id` to a different `SceneLayer` (`Under`, `Interleaved`, `Over`); notifies all views.

<a id="scenemodel-set_item_parent"></a>

#### `pub fn set_item_parent(&self, child: ItemId, parent: Option<ItemId>)`

Re-parent `child` under `parent` (or under the scene root when `None`); notifies all views.

<a id="scenemodel-remove"></a>

#### `pub fn remove(&self, id: ItemId)`

Remove an item and its descendants. Drops any `Delegated` payload `Rc`
and cleans the item's a11y mappings; alive logical children re-root.

<a id="scenemodel-take"></a>

#### `pub fn take(&self, id: ItemId) -> Vec<RemovedItem>`

<a id="scenemodel-restore"></a>

#### `pub fn restore(&self, salvage: RemovedItem) -> Result<ItemId, RestoreError>`

Put one salvaged item back at its original id. See `Scene::restore`.

<a id="scenemodel-restore_all"></a>

#### `pub fn restore_all(&self, salvage: Vec<RemovedItem>) -> Result<Vec<ItemId>, RestoreError>`

Put a whole `take` result back, roots first. See
`Scene::restore_all`.

<a id="scenemodel-replace_item"></a>

#### `pub fn replace_item( &self, id: ItemId, item: Box<dyn SceneItem>, ) -> Result<Box<dyn SceneItem>, ReplaceRejected>`

Swap the lightweight item box at `id`, keeping the entry and the id.
Returns the box that was there. See `Scene::replace_item`.

<a id="scenemodel-placement"></a>

#### `pub fn placement(&self, id: ItemId) -> Option<Placement>`

Where `id` sits, as one value: parent, z, position, transform.

<a id="scenemodel-set_placement"></a>

#### `pub fn set_placement(&self, id: ItemId, placement: Placement)`

Write parent, z, position and transform together, as one change. See
`Scene::set_placement`.

<a id="scenemodel-reparent_keeping_scene_pos"></a>

#### `pub fn reparent_keeping_scene_pos(&self, id: ItemId, parent: Option<ItemId>)`

Reparent `id` while holding it visually still. See
`Scene::reparent_keeping_scene_pos`.

<a id="scenemodel-z_between"></a>

#### `pub fn z_between(&self, below: ItemId, above: ItemId) -> Option<f32>`

A `z` strictly between two items', or `None` when there is no room left
at that locus. See `Scene::z_between`.

<a id="scenemodel-orphan"></a>

#### `pub fn orphan(&self, id: ItemId)`

Promote an item's children to the scene root.

<a id="scenemodel-set_item_handlers"></a>

#### `pub fn set_item_handlers(&self, id: ItemId, handlers: Option<SceneItemHandlerSet>)`

Replace the `SceneItemHandlerSet` of `id`, or clear it with `None`.

<a id="scenemodel-with_handlers_mut"></a>

#### `pub fn with_handlers_mut<R>( &self, id: ItemId, f: impl FnOnce(&mut SceneItemHandlerSet) -> R, ) -> Option<R>`

Mutate an item's handler set through a closure (avoids returning a
borrow guard tied to the `RefMut`).

<a id="scenemodel-add_magnet"></a>

#### `pub fn add_magnet(&self, item: ItemId, magnet: Magnet) -> MagnetId`

Attach a `Magnet` to `item`; see `Scene::add_magnet`.

<a id="scenemodel-remove_magnet"></a>

#### `pub fn remove_magnet(&self, magnet: MagnetId)`

Remove a magnet by id; see `Scene::remove_magnet`.

<a id="scenemodel-clear_magnets"></a>

#### `pub fn clear_magnets(&self, item: ItemId)`

Remove every magnet on `item`; see `Scene::clear_magnets`.

<a id="scenemodel-set_magnet_local_pos"></a>

#### `pub fn set_magnet_local_pos(&self, magnet: MagnetId, local_pos: Point)`

Move a magnet in its item's local frame; see `Scene::set_magnet_local_pos`.

<a id="scenemodel-set_magnet_enabled"></a>

#### `pub fn set_magnet_enabled(&self, magnet: MagnetId, enabled: bool)`

Enable or disable a magnet; see `Scene::set_magnet_enabled`.

<a id="scenemodel-magnet_ids_of"></a>

#### `pub fn magnet_ids_of(&self, item: ItemId) -> Vec<MagnetId>`

Ids of every magnet on `item`; see `Scene::magnet_ids_of`.

<a id="scenemodel-magnet_owner"></a>

#### `pub fn magnet_owner(&self, magnet: MagnetId) -> Option<ItemId>`

The owning item of a magnet; see `Scene::magnet_owner`.

<a id="scenemodel-magnet_scene_pos"></a>

#### `pub fn magnet_scene_pos(&self, magnet: MagnetId) -> Option<Point>`

A magnet's scene position; see `Scene::magnet_scene_pos`.

<a id="scenemodel-magnet"></a>

#### `pub fn magnet(&self, magnet: MagnetId) -> Option<MagnetRef>`

Resolve a magnet to a `MagnetRef` snapshot; see `Scene::magnet`.

<a id="scenemodel-compute_item_snap"></a>

#### `pub fn compute_item_snap( &self, dragged: ItemId, drag_delta: Vec2, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<MagnetSnap>`

Best item-drag snap; see `Scene::compute_item_snap`. A shared
(read-only) borrow is held while the `predicate` runs over owned
candidate snapshots, so the predicate may read but must not mutate
the model.

<a id="scenemodel-compute_port_snap"></a>

#### `pub fn compute_port_snap( &self, source: MagnetId, cursor_scene: Point, capture_radius: f32, predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict, ) -> Option<(MagnetRef, Option<std::rc::Rc<dyn std::any::Any>>)>`

Best port-drag snap; see `Scene::compute_port_snap`.

<a id="scenemodel-nearest_magnet"></a>

#### `pub fn nearest_magnet(&self, scene_pt: Point, radius: f32) -> Option<MagnetId>`

Nearest enabled magnet within `radius`; see `Scene::nearest_magnet`.

<a id="scenemodel-set_scene_rect"></a>

#### `pub fn set_scene_rect(&self, rect: Option<Rect>)`

Set the logical extent of the scene (used for scroll-bar sizing); `None` = unbounded.

<a id="scenemodel-pan_axes"></a>

#### `pub fn pan_axes(&self, axes: PanAxes)`

Restrict panning to horizontal, vertical, or both axes; updates `pan_axes_signal`.

<a id="scenemodel-zoomable"></a>

#### `pub fn zoomable(&self, on: bool)`

Enable or disable pinch/scroll zoom; updates `zoomable_signal`.

<a id="scenemodel-set_pan_bounds"></a>

#### `pub fn set_pan_bounds(&self, bounds: Option<Rect>)`

Clamp the camera pan to `bounds` (scene coordinates); `None` = no limit; updates `pan_bounds_signal`.

<a id="scenemodel-set_zoom_range"></a>

#### `pub fn set_zoom_range(&self, range: Option<std::ops::RangeInclusive<f32>>)`

Restrict the zoom factor to `range`; `None` = no limit; updates `zoom_range_signal`.

<a id="scenemodel-add_a11y_group"></a>

#### `pub fn add_a11y_group(&self, builder: A11yGroupBuilder) -> A11yGroupId`

Register a logical AT group (landmark / rotor category container); returns its stable `A11yGroupId`.

<a id="scenemodel-remove_a11y_group"></a>

#### `pub fn remove_a11y_group(&self, id: A11yGroupId)`

Remove a previously registered AT group; triggers an `a11y_change_signal` bump.

<a id="scenemodel-set_a11y_parent"></a>

#### `pub fn set_a11y_parent(&self, child: A11yNode, parent: Option<A11yNode>)`

Re-parent `child` in the AT tree, overriding the default visual parent; `None` re-attaches under the scene root.

<a id="scenemodel-add_a11y_relation"></a>

#### `pub fn add_a11y_relation(&self, from: A11yNode, kind: A11yRelation, to: A11yNode)`

Declare a cross-node AT relationship (controls, describes, labels) from `from` to `to`.

<a id="scenemodel-set_a11y_live"></a>

#### `pub fn set_a11y_live(&self, node: A11yNode, live: accesskit::Live)`

Mark `node` as a live region (`Polite` or `Assertive`) so assistive tech announces changes to it.

<a id="scenemodel-set_a11y_landmark"></a>

#### `pub fn set_a11y_landmark(&self, node: A11yNode, role: accesskit::Role)`

Assign a landmark `role` to `node` (e.g. `Role::Region`, `Role::Main`) for rotor navigation.

<a id="scenemodel-set_a11y_categories"></a>

#### `pub fn set_a11y_categories(&self, node: A11yNode, categories: &[A11yCategory])`

Register `node` under the given rotor `A11yCategory` slices so it appears in category-filtered navigation.

<a id="scenemodel-refresh_dynamic_bounds"></a>

#### `pub fn refresh_dynamic_bounds(&self) -> bool`

Re-read signal-driven bounds for `add_item_dynamic` entries; returns
`true` if any changed.

<a id="scenemodel-item_change_signal"></a>

#### `pub fn item_change_signal(&self) -> Signal<SceneChange>`

Reactive signal fired on every structural scene change; all views observe this to reconcile.

<a id="scenemodel-a11y_change_signal"></a>

#### `pub fn a11y_change_signal(&self) -> Signal<u64>`

Reactive monotonic counter bumped on every AT-structure change; views re-walk accessibility on any increment.

<a id="scenemodel-mutation_version"></a>

#### `pub fn mutation_version(&self) -> u64`

Monotonic counter incremented on every mutation; useful for cache invalidation without observing a signal.

<a id="scenemodel-structural_version"></a>

#### `pub fn structural_version(&self) -> u64`

`mutation_version` with the per-frame churn of
`refresh_dynamic_bounds` excluded — the
version to gate an expensive rebuild on. See
`Scene::structural_version`.

<a id="scenemodel-pan_axes_signal"></a>

#### `pub fn pan_axes_signal(&self) -> Signal<PanAxes>`

Reactive current `PanAxes` restriction; updated by `pan_axes`.

<a id="scenemodel-pan_bounds_signal"></a>

#### `pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>>`

Reactive camera-pan clamp bounds; updated by `set_pan_bounds`.

<a id="scenemodel-zoom_range_signal"></a>

#### `pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>>`

Reactive zoom-factor clamp range; updated by `set_zoom_range`.

<a id="scenemodel-zoomable_signal"></a>

#### `pub fn zoomable_signal(&self) -> Signal<bool>`

Reactive zoom-enabled flag; updated by `zoomable`.

<a id="scenemodel-len"></a>

#### `pub fn len(&self) -> usize`

Total number of items in the scene (lightweight + heavyweight).

<a id="scenemodel-is_empty"></a>

#### `pub fn is_empty(&self) -> bool`

Returns `true` when the scene contains no items.

<a id="scenemodel-ids"></a>

#### `pub fn ids(&self) -> Vec<ItemId>`

All `ItemId`s currently in the scene, in insertion order.

<a id="scenemodel-local_pos"></a>

#### `pub fn local_pos(&self, id: ItemId) -> Option<Point>`

The local position of `id` in its parent's coordinate space; `None` if `id` is unknown.

<a id="scenemodel-local_bounds"></a>

#### `pub fn local_bounds(&self, id: ItemId) -> Option<Rect>`

The local bounding rect of `id`; `None` if `id` is unknown.

<a id="scenemodel-transform"></a>

#### `pub fn transform(&self, id: ItemId) -> Option<Transform2D>`

The additional local-to-parent transform of `id` (beyond position); `None` if none is set.

<a id="scenemodel-scene_transform"></a>

#### `pub fn scene_transform(&self, id: ItemId) -> Transform2D`

The full local-to-scene transform for `id` (parent chain composed); identity if `id` is unknown.

<a id="scenemodel-scene_pos"></a>

#### `pub fn scene_pos(&self, id: ItemId) -> Option<Point>`

The origin of `id` mapped into scene coordinates; `None` if `id` is unknown.

<a id="scenemodel-scene_rect"></a>

#### `pub fn scene_rect(&self, id: ItemId) -> Option<Rect>`

The bounding rect of `id` in scene coordinates (local bounds transformed by the parent chain); `None` if unknown.

<a id="scenemodel-flags"></a>

#### `pub fn flags(&self, id: ItemId) -> Option<ItemFlags>`

The `ItemFlags` bitset of `id`; `None` if `id` is unknown.

<a id="scenemodel-is_effectively_visible"></a>

#### `pub fn is_effectively_visible(&self, id: ItemId) -> bool`

Returns `true` if `id` and all of its ancestors are visible.

<a id="scenemodel-opacity"></a>

#### `pub fn opacity(&self, id: ItemId) -> Option<f32>`

The own opacity of `id` (ignoring ancestors); `None` if `id` is unknown.

<a id="scenemodel-effective_opacity"></a>

#### `pub fn effective_opacity(&self, id: ItemId) -> f32`

Accumulated opacity for `id` (own × each ancestor's opacity).

<a id="scenemodel-z"></a>

#### `pub fn z(&self, id: ItemId) -> Option<f32>`

The z-order value of `id` within its layer; `None` if `id` is unknown.

<a id="scenemodel-layer"></a>

#### `pub fn layer(&self, id: ItemId) -> Option<SceneLayer>`

The `SceneLayer` of `id`; `None` if `id` is unknown.

<a id="scenemodel-parent_of"></a>

#### `pub fn parent_of(&self, id: ItemId) -> Option<ItemId>`

The direct parent of `id`, or `None` if it is a root item (or unknown).

<a id="scenemodel-is_descendant_of"></a>

#### `pub fn is_descendant_of(&self, id: ItemId, ancestor: ItemId) -> bool`

Returns `true` if `id` is anywhere in `ancestor`'s subtree.

<a id="scenemodel-scene_rect_extent"></a>

#### `pub fn scene_rect_extent(&self) -> Option<Rect>`

The logical extent set via `set_scene_rect`; `None` = unbounded.

<a id="scenemodel-current_pan_axes"></a>

#### `pub fn current_pan_axes(&self) -> PanAxes`

The current pan-axis restriction without subscribing to its signal.

<a id="scenemodel-is_zoomable"></a>

#### `pub fn is_zoomable(&self) -> bool`

Returns `true` if zoom is currently enabled (snapshot; use `zoomable_signal` for reactivity).

<a id="scenemodel-current_pan_bounds"></a>

#### `pub fn current_pan_bounds(&self) -> Option<Rect>`

Current pan-clamp bounds without subscribing to its signal.

<a id="scenemodel-current_zoom_range"></a>

#### `pub fn current_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>>`

Current zoom-factor clamp range without subscribing to its signal.

<a id="scenemodel-items_in_rect"></a>

#### `pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId>`

All items whose bounding rects overlap `scene_rect` (spatial-index query).

<a id="scenemodel-items_in_region"></a>

#### `pub fn items_in_region( &self, region: &SceneRegion, mode: ItemSelectionMode, view_scale: f32, ) -> Vec<ItemId>`

Items matching `region` under `mode`; see `Scene::items_in_region`.

Screen-anchored
(`IGNORES_TRANSFORMATIONS`)
items are skipped, for the same reason `SceneModel::item_at` skips
them: a scene-space region cannot place them.

<a id="scenemodel-item_shape"></a>

#### `pub fn item_shape(&self, id: ItemId) -> Option<ItemShape>`

An entry's geometry in local coordinates; see `Scene::item_shape`.

<a id="scenemodel-item_region"></a>

#### `pub fn item_region(&self, id: ItemId) -> Option<SceneRegion>`

An entry's own shape as a scene-space region; see `Scene::item_region`.

<a id="scenemodel-item_contains"></a>

#### `pub fn item_contains(&self, id: ItemId, scene_pt: Point, view_scale: f32) -> bool`

Whether `id`'s shape contains `scene_pt`; see `Scene::item_contains`.

<a id="scenemodel-paint_key"></a>

#### `pub fn paint_key(&self, id: ItemId) -> Option<crate::pick::PaintKey>`

Where `id` sits in this scene's single paint order; see
`Scene::paint_key`.

Defined for **both tiers**, so it is the value to compare when an app
needs to ask "which of these two is on top?" without re-deriving the
band/z/insertion rule.

<a id="scenemodel-is_hit_testable"></a>

#### `pub fn is_hit_testable(&self, id: ItemId) -> bool`

Whether `id` takes part in pointer hit-testing — visible along its whole
ancestor chain AND enabled; see `Scene::is_hit_testable`.

The public form of the filter every picker here applies, so an app can
ask why one of its items is not answering.

<a id="scenemodel-item_at"></a>

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

<a id="scenemodel-item_at_scaled"></a>

#### `pub fn item_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId>`

`Scene::item_at` at an explicit view zoom. The zoom reaches exactly
one thing: a cosmetic stroke band's width in scene units.

<a id="scenemodel-items_at"></a>

#### `pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>`

All lightweight items whose shape contains `scene_pt`, topmost-first in
`Scene::paint_key` order. Same tier, flag and
`IGNORES_TRANSFORMATIONS` rules as `SceneModel::item_at`.

<a id="scenemodel-items_at_scaled"></a>

#### `pub fn items_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Vec<ItemId>`

`Scene::items_at` at an explicit view zoom.

<a id="scenemodel-item_at_in_view"></a>

#### `pub fn item_at_in_view(&self, screen_pt: Point, view_transform: Transform2D) -> Option<ItemId>`

The topmost item under a **screen** point, resolving both hit spaces;
see `Scene::item_at_in_view`.

<a id="scenemodel-colliding_items"></a>

#### `pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>`

Items overlapping `id`'s shape, excluding `id`; see
`Scene::colliding_items`.

<a id="scenemodel-colliding_items_with"></a>

#### `pub fn colliding_items_with(&self, id: ItemId, mode: ItemSelectionMode) -> Vec<ItemId>`

`Scene::colliding_items` under an explicit mode.

<a id="scenemodel-items_along_path"></a>

#### `pub fn items_along_path(&self, path: &teksilo_canvas::Path) -> Vec<ItemId>`

Items lying along `path`; see `Scene::items_along_path`. (This was
unreachable from a `SceneModel` before — the facade never forwarded it.)

<a id="scenemodel-items_along_path_with"></a>

#### `pub fn items_along_path_with( &self, path: &teksilo_canvas::Path, stroke_width: f32, mode: ItemSelectionMode, ) -> Vec<ItemId>`

`Scene::items_along_path` with an explicit stroke width and mode.

<a id="scenemodel-a11y_parent_of"></a>

#### `pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode>`

The AT-tree parent of `child` as set by `set_a11y_parent`; `None` = visual default.

<a id="scenemodel-a11y_relations"></a>

#### `pub fn a11y_relations(&self) -> Vec<(A11yNode, A11yRelation, A11yNode)>`

Every declared AT relation, in declaration order.

<a id="scenemodel-a11y_live_of"></a>

#### `pub fn a11y_live_of(&self, node: A11yNode) -> Option<accesskit::Live>`

A node's declared live-region politeness; `None` when it is not one.

<a id="scenemodel-a11y_landmark_of"></a>

#### `pub fn a11y_landmark_of(&self, node: A11yNode) -> Option<accesskit::Role>`

A node's declared landmark role; `None` when it is not a landmark.

<a id="scenemodel-a11y_categories_of"></a>

#### `pub fn a11y_categories_of(&self, node: A11yNode) -> Vec<A11yCategory>`

A node's declared rotor / quick-nav categories.

<a id="weakscenemodel"></a>

## `pub struct WeakSceneModel`

A non-owning handle to a `Scene` — `SceneModel` without the ownership.

Produced by `SceneModel::downgrade` and turned back into a `SceneModel`,
for as long as one still exists, by `upgrade`. Holding one
keeps nothing alive.

Its reason to exist is the closure the scene owns: a geometry constraint
that captures a `SceneModel` closes the ring
`SceneModel → Scene → constraint → SceneModel`, and nothing in it is ever
dropped. Capturing this instead breaks the ring. See
`SceneModel::set_geometry_constraint` and
`ProposedChange` for the rule and the safe forms.

```rust
pub struct WeakSceneModel(Weak<RefCell<Scene>>);
```

### Methods

<a id="weakscenemodel-upgrade"></a>

#### `pub fn upgrade(&self) -> Option<SceneModel>`

An owning handle to the scene, if any still exists.

`None` once the last `SceneModel` has been dropped — which is the
point: a constraint outliving its scene does nothing instead of keeping
it alive.

<a id="weakscenemodel-is_alive"></a>

#### `pub fn is_alive(&self) -> bool`

Whether the scene is still alive, without building a handle.

<a id="scenewriteguard"></a>

## `pub struct SceneWriteGuard`

Write guard over a `SceneModel`'s `Scene`: `Deref`/`DerefMut` to the
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

<a id="scenetransaction"></a>

## `pub struct SceneTransaction`

```rust
pub struct SceneTransaction<'a> { /* fields */ }
```

### Methods

<a id="scenetransaction-squash"></a>

#### `pub fn squash(self) -> Self`

Coalesce this transaction's repeated writes to one continuous quantity
— position, bounds, transform, opacity, z, placement — into a single
edit keeping the **first** `old` and the **last** `new`.

**Off by default**, and deliberately: squashing throws the intermediate
path away, which a replay, a presence indicator or a collaboration relay
needs, and losing it silently by default is exactly the quiet data loss
this seam exists to prevent. The built-in item drag already commits one
`set_local_pos` per gesture, so the default costs nothing in the common
case; a caller that writes per sample and wants endpoints asks for it.

Discrete edits — adds, removals, replacements, flags, parent changes —
are never coalesced and keep their order.

<a id="scenetransaction-ephemeral"></a>

#### `pub fn ephemeral(mut self) -> Self`

Mark this transaction's changes as per-frame artefacts: they must be
**rendered** and must not be **recorded**.

The app-side half of the knob the scene's own dynamic-bounds refresh
uses. An item whose `transform` follows a rotation `Signal` writes the
model every frame; without this, an app watching the change stream
cannot tell that churn from an edit. `ephemeral` rides every
`SceneChange` and the record, and the framework does nothing else with
it — what counts as history is the consumer's call.

<a id="scenetransaction-abandon"></a>

#### `pub fn abandon(self)`

Mark the interaction cancelled, then commit.

The scene is **not** rolled back: applying an inverse is the first 80 %
of an undo stack, and undo lives on the other side of this seam. The
record arrives with `TxnOutcome::Abandoned` so the consumer can revert
from the `old` values it was handed without pushing anything a redo
could replay — tldraw's `bail`, with the stack where it belongs.

On a nested guard this abandons the transaction it joined, because that
is the transaction it is part of.
