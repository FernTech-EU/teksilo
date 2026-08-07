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

Every mutator takes `&self`, borrows the inner `RefCell<Scene>` mutably,
mutates, and the borrow drops at the end of the statement. The change
signal fires *inside* that borrow (via `Scene::emit_item_change`), but
`Signal::try_set` snapshots its observers and releases the signal's own
cell before invoking them — so the only rule is: **an observer registered
on `item_change_signal` /
`a11y_change_signal` must not re-borrow
the `SceneModel` in its callback.** A `SceneView` observer only bumps its
own per-view signals, so it is safe. Likewise a view **delegate** must not
synchronously mutate the model during a build-time call (the view drops all
model borrows before invoking it; the delegate's *handlers* may mutate
later).

## Builder methods at a glance

`with_index`, `from_scene`, `handle_count`, `add_widget`, `add_widget_item`, `set_payload`, `payload`, `add_item`, `add_item_dynamic`, `add_boxed_item`, `set_local_pos`, `set_local_bounds`, `set_transform`, `set_flags`, `set_flag`, `set_visible`, `set_opacity`, `set_item_fill`, `clear_item_fill`, `set_item_stroke`, `clear_item_stroke`, `set_z`, `bring_to_front`, `send_to_back`, `set_layer`, `set_item_parent`, `remove`, `orphan`, `set_item_handlers`, `with_handlers_mut`, `add_magnet`, `remove_magnet`, `clear_magnets`, `set_magnet_local_pos`, `set_magnet_enabled`, `magnet_ids_of`, `magnet_owner`, `magnet_scene_pos`, `magnet`, `compute_item_snap`, `compute_port_snap`, `nearest_magnet`, `set_scene_rect`, `pan_axes`, `zoomable`, `set_pan_bounds`, `set_zoom_range`, `add_a11y_group`, `remove_a11y_group`, `set_a11y_parent`, `add_a11y_relation`, `set_a11y_live`, `set_a11y_landmark`, `set_a11y_categories`, `refresh_dynamic_bounds`, `item_change_signal`, `a11y_change_signal`, `mutation_version`, `pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`, `zoomable_signal`, `len`, `is_empty`, `ids`, `local_pos`, `local_bounds`, `transform`, `scene_transform`, `scene_pos`, `scene_rect`, `flags`, `is_effectively_visible`, `opacity`, `effective_opacity`, `z`, `layer`, `parent_of`, `is_descendant_of`, `scene_rect_extent`, `current_pan_axes`, `is_zoomable`, `current_pan_bounds`, `current_zoom_range`, `items_in_rect`, `item_at`, `items_at`, `colliding_items`, `a11y_parent_of`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_scene/index.html)

## `pub struct SceneModel`

A shared, cloneable handle to a `Scene`.

```rust
pub struct SceneModel(pub(crate) Rc<RefCell<Scene>>);
```

### Methods

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

#### `pub fn item_at(&self, scene_pt: Point) -> Option<ItemId>`

The topmost item under `scene_pt` using exact-shape hit-testing; `None` if no item is hit.

#### `pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId>`

All items under `scene_pt` (exact-shape hit-test), ordered front-to-back.

#### `pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId>`

All items whose bounding rects intersect `id`'s bounding rect.

#### `pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode>`

The AT-tree parent of `child` as set by `set_a11y_parent`; `None` = visual default.
