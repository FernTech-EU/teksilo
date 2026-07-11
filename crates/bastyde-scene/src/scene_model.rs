// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneModel`] — a shared, cloneable handle to a [`Scene`].
//!
//! Mirrors the `ListModel = Rc<RefCell<ListModelInner>>` pattern from
//! `bastyde-data`: cloning a `SceneModel` produces a **second handle to the
//! same scene**, so multiple [`SceneView`](crate::SceneView)s can render one
//! scene (overview + detail panes, same-document multi-window, headless model
//! reuse). Mutate the model once and every attached view reconciles.
//!
//! ## Heavyweight content across views
//!
//! A heavyweight `Widget` instance can live in only one arena, so a shared
//! model cannot hand the *same* `Box<dyn Widget>` to two views. Two paths:
//!
//! - **Single-view** — [`add_widget`](SceneModel::add_widget) stores the
//!   widget in a one-shot slot drained by the first view that builds. A
//!   second view sharing the model produces no child for it.
//! - **Multi-view** — [`add_widget_item`](SceneModel::add_widget_item) stores
//!   a type-erased `payload`; each view's delegate
//!   ([`SceneView::delegate_typed`](crate::SceneView::delegate_typed)) builds
//!   its **own** instance from the payload. [`set_payload`](SceneModel::set_payload)
//!   replaces the data and every view rebuilds that item.
//!
//! ## Borrow / observer contract
//!
//! Every mutator takes `&self`, borrows the inner `RefCell<Scene>` mutably,
//! mutates, and the borrow drops at the end of the statement. The change
//! signal fires *inside* that borrow (via `Scene::emit_item_change`), but
//! `Signal::try_set` snapshots its observers and releases the signal's own
//! cell before invoking them — so the only rule is: **an observer registered
//! on [`item_change_signal`](SceneModel::item_change_signal) /
//! [`a11y_change_signal`](SceneModel::a11y_change_signal) must not re-borrow
//! the `SceneModel` in its callback.** A `SceneView` observer only bumps its
//! own per-view signals, so it is safe. Likewise a view **delegate** must not
//! synchronously mutate the model during a build-time call (the view drops all
//! model borrows before invoking it; the delegate's *handlers* may mutate
//! later).

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, StrokeStyle, Transform2D};
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::widget::Widget;

use crate::a11y::{A11yCategory, A11yGroupBuilder, A11yGroupId, A11yNode, A11yRelation};
use crate::flags::ItemFlags;
use crate::index::SpatialIndex;
use crate::item::{ItemId, SceneItem};
use crate::item_handlers::SceneItemHandlerSet;
use crate::magnet::{Magnet, MagnetId, MagnetRef, MagnetSnap, MagnetVerdict};
use crate::scene::{ItemChange, PanAxes, Scene, SceneLayer};
use bastyde_canvas::Vec2;

/// A shared, cloneable handle to a [`Scene`].
pub struct SceneModel(pub(crate) Rc<RefCell<Scene>>);

impl Clone for SceneModel {
    /// Produce a second handle to the **same** scene (cheap `Rc` clone).
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Default for SceneModel {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SceneModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_borrow() {
            Ok(scene) => f
                .debug_struct("SceneModel")
                .field("handles", &Rc::strong_count(&self.0))
                .field("len", &scene.len())
                .finish(),
            Err(_) => f
                .debug_struct("SceneModel")
                .field("handles", &Rc::strong_count(&self.0))
                .field("len", &"<borrowed>")
                .finish(),
        }
    }
}

impl SceneModel {
    // -----------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------

    /// A handle to a fresh empty scene with the default spatial index.
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Scene::new())))
    }

    /// A handle to a fresh scene with a custom [`SpatialIndex`].
    pub fn with_index(index: Box<dyn SpatialIndex>) -> Self {
        Self(Rc::new(RefCell::new(Scene::with_index(index))))
    }

    /// Wrap an existing [`Scene`] in a handle. Used by
    /// [`SceneView::new`](crate::SceneView::new) for the single-view path.
    pub fn from_scene(scene: Scene) -> Self {
        Self(Rc::new(RefCell::new(scene)))
    }

    /// Number of distinct handles to this scene (1 = unshared).
    pub fn handle_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    // -----------------------------------------------------------------
    // Heavyweight insertion
    // -----------------------------------------------------------------

    /// Single-view heavyweight widget (the one-shot `Once` path). The first
    /// view to build drains it; a second view sharing this model produces no
    /// child for it. For multi-view, use [`add_widget_item`](Self::add_widget_item).
    pub fn add_widget<W: Widget + 'static>(&self, widget: W, rect: Rect) -> ItemId {
        self.0.borrow_mut().add_widget(widget, rect)
    }

    /// Multi-view heavyweight item: store a typed `payload`; each view builds
    /// its own widget instance from it via its delegate. Returns the [`ItemId`].
    pub fn add_widget_item<P: 'static>(&self, payload: P, rect: Rect) -> ItemId {
        self.0
            .borrow_mut()
            .add_widget_delegated(Rc::new(payload), rect)
    }

    /// Replace the payload of a `Delegated` heavyweight item; every view
    /// rebuilds that item's widget on the next pass.
    ///
    /// # Panics
    ///
    /// Panics if `id` is unknown, refers to a single-view `add_widget` (Once)
    /// entry, or refers to a lightweight item.
    pub fn set_payload<P: 'static>(&self, id: ItemId, payload: P) {
        self.0.borrow_mut().set_payload(id, Rc::new(payload));
    }

    /// The current type-erased payload of a `Delegated` item, if any.
    pub fn payload(&self, id: ItemId) -> Option<Rc<dyn std::any::Any>> {
        self.0.borrow().payload(id)
    }

    // -----------------------------------------------------------------
    // Lightweight insertion
    // -----------------------------------------------------------------

    /// Add a lightweight [`SceneItem`] at `local_pos`.
    pub fn add_item<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId {
        self.0.borrow_mut().add_item(item, local_pos)
    }

    /// Add a lightweight item with signal-driven (dynamic) bounds.
    pub fn add_item_dynamic<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId {
        self.0.borrow_mut().add_item_dynamic(item, local_pos)
    }

    /// Add an already-boxed lightweight item at `local_pos`. The boxed-`dyn`
    /// counterpart of [`add_item`](Self::add_item), used by
    /// [`SceneListAdapter`](crate::SceneListAdapter).
    pub fn add_boxed_item(&self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId {
        self.0.borrow_mut().add_boxed_item(item, local_pos)
    }

    // -----------------------------------------------------------------
    // Geometry mutation
    // -----------------------------------------------------------------

    /// Move `id` to `local_pos` in its parent's coordinate space; notifies all views.
    pub fn set_local_pos(&self, id: ItemId, local_pos: Point) {
        self.0.borrow_mut().set_local_pos(id, local_pos);
    }
    /// Replace the local bounding rect of `id`; notifies all views.
    pub fn set_local_bounds(&self, id: ItemId, local_bounds: Rect) {
        self.0.borrow_mut().set_local_bounds(id, local_bounds);
    }
    /// Set an additional local-to-parent transform (rotation, scale) on `id`; notifies all views.
    pub fn set_transform(&self, id: ItemId, transform: Transform2D) {
        self.0.borrow_mut().set_transform(id, transform);
    }

    // -----------------------------------------------------------------
    // Flags / visibility / opacity mutation
    // -----------------------------------------------------------------

    /// Replace the complete [`ItemFlags`] bitset for `id`; notifies all views.
    pub fn set_flags(&self, id: ItemId, flags: ItemFlags) {
        self.0.borrow_mut().set_flags(id, flags);
    }
    /// Set or clear a single [`ItemFlags`] bit on `id`; notifies all views.
    pub fn set_flag(&self, id: ItemId, flag: ItemFlags, on: bool) {
        self.0.borrow_mut().set_flag(id, flag, on);
    }
    /// Show or hide `id` (also hides its descendants); notifies all views.
    pub fn set_visible(&self, id: ItemId, visible: bool) {
        self.0.borrow_mut().set_visible(id, visible);
    }
    /// Set the paint opacity of `id` (0.0 = transparent, 1.0 = opaque); notifies all views.
    pub fn set_opacity(&self, id: ItemId, opacity: f32) {
        self.0.borrow_mut().set_opacity(id, opacity);
    }

    // -----------------------------------------------------------------
    // Appearance mutation (paint-only, repaint without relayout)
    // -----------------------------------------------------------------

    /// Replace a lightweight item's fill colour live; every view repaints
    /// (no relayout/rebuild). Accepts a plain [`Color`](bastyde_tokens::Color),
    /// a theme role, a `Signal<Color>`, or a `Signal<Role>`. See
    /// [`Scene::set_item_fill`] for the reactive-colour contract.
    pub fn set_item_fill(&self, id: ItemId, fill: impl Into<ColorProp>) {
        self.0.borrow_mut().set_item_fill(id, fill);
    }
    /// Clear a lightweight item's fill; every view repaints.
    pub fn clear_item_fill(&self, id: ItemId) {
        self.0.borrow_mut().clear_item_fill(id);
    }
    /// Replace a lightweight item's stroke (colour + [`StrokeStyle`]) live;
    /// every view repaints (no relayout/rebuild).
    pub fn set_item_stroke(&self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle) {
        self.0.borrow_mut().set_item_stroke(id, color, style);
    }
    /// Clear a lightweight item's stroke; every view repaints.
    pub fn clear_item_stroke(&self, id: ItemId) {
        self.0.borrow_mut().clear_item_stroke(id);
    }

    // -----------------------------------------------------------------
    // Z-order / layer / parenting mutation
    // -----------------------------------------------------------------

    /// Set the z-order of `id` within its layer; higher values paint on top.
    pub fn set_z(&self, id: ItemId, z: f32) {
        self.0.borrow_mut().set_z(id, z);
    }
    /// Give `id` the highest z-value in its layer so it paints on top of all siblings.
    pub fn bring_to_front(&self, id: ItemId) {
        self.0.borrow_mut().bring_to_front(id);
    }
    /// Give `id` the lowest z-value in its layer so it paints beneath all siblings.
    pub fn send_to_back(&self, id: ItemId) {
        self.0.borrow_mut().send_to_back(id);
    }
    /// Move `id` to a different [`SceneLayer`] (background, default, foreground); notifies all views.
    pub fn set_layer(&self, id: ItemId, layer: SceneLayer) {
        self.0.borrow_mut().set_layer(id, layer);
    }
    /// Re-parent `child` under `parent` (or under the scene root when `None`); notifies all views.
    pub fn set_item_parent(&self, child: ItemId, parent: Option<ItemId>) {
        self.0.borrow_mut().set_item_parent(child, parent);
    }

    // -----------------------------------------------------------------
    // Removal
    // -----------------------------------------------------------------

    /// Remove an item and its descendants. Drops any `Delegated` payload `Rc`
    /// and cleans the item's a11y mappings; alive logical children re-root.
    pub fn remove(&self, id: ItemId) {
        self.0.borrow_mut().remove(id);
    }
    /// Promote an item's children to the scene root.
    pub fn orphan(&self, id: ItemId) {
        self.0.borrow_mut().orphan(id);
    }

    // -----------------------------------------------------------------
    // Handlers
    // -----------------------------------------------------------------

    /// Replace the [`SceneItemHandlerSet`] of `id`, or clear it with `None`.
    pub fn set_item_handlers(&self, id: ItemId, handlers: Option<SceneItemHandlerSet>) {
        self.0.borrow_mut().set_item_handlers(id, handlers);
    }
    /// Mutate an item's handler set through a closure (avoids returning a
    /// borrow guard tied to the `RefMut`).
    pub fn with_handlers_mut<R>(
        &self,
        id: ItemId,
        f: impl FnOnce(&mut SceneItemHandlerSet) -> R,
    ) -> Option<R> {
        self.0.borrow_mut().handlers_mut(id).map(f)
    }

    // -----------------------------------------------------------------
    // Magnetism
    // -----------------------------------------------------------------

    /// Attach a [`Magnet`] to `item`; see [`Scene::add_magnet`].
    pub fn add_magnet(&self, item: ItemId, magnet: Magnet) -> MagnetId {
        self.0.borrow_mut().add_magnet(item, magnet)
    }
    /// Remove a magnet by id; see [`Scene::remove_magnet`].
    pub fn remove_magnet(&self, magnet: MagnetId) {
        self.0.borrow_mut().remove_magnet(magnet);
    }
    /// Remove every magnet on `item`; see [`Scene::clear_magnets`].
    pub fn clear_magnets(&self, item: ItemId) {
        self.0.borrow_mut().clear_magnets(item);
    }
    /// Move a magnet in its item's local frame; see [`Scene::set_magnet_local_pos`].
    pub fn set_magnet_local_pos(&self, magnet: MagnetId, local_pos: Point) {
        self.0.borrow_mut().set_magnet_local_pos(magnet, local_pos);
    }
    /// Enable or disable a magnet; see [`Scene::set_magnet_enabled`].
    pub fn set_magnet_enabled(&self, magnet: MagnetId, enabled: bool) {
        self.0.borrow_mut().set_magnet_enabled(magnet, enabled);
    }
    /// Ids of every magnet on `item`; see [`Scene::magnet_ids_of`].
    pub fn magnet_ids_of(&self, item: ItemId) -> Vec<MagnetId> {
        self.0.borrow().magnet_ids_of(item)
    }
    /// The owning item of a magnet; see [`Scene::magnet_owner`].
    pub fn magnet_owner(&self, magnet: MagnetId) -> Option<ItemId> {
        self.0.borrow().magnet_owner(magnet)
    }
    /// A magnet's scene position; see [`Scene::magnet_scene_pos`].
    pub fn magnet_scene_pos(&self, magnet: MagnetId) -> Option<Point> {
        self.0.borrow().magnet_scene_pos(magnet)
    }
    /// Resolve a magnet to a [`MagnetRef`] snapshot; see [`Scene::magnet`].
    pub fn magnet(&self, magnet: MagnetId) -> Option<MagnetRef> {
        self.0.borrow().magnet(magnet)
    }
    /// Best item-drag snap; see [`Scene::compute_item_snap`]. A shared
    /// (read-only) borrow is held while the `predicate` runs over owned
    /// candidate snapshots, so the predicate may read but must not mutate
    /// the model.
    pub fn compute_item_snap(
        &self,
        dragged: ItemId,
        drag_delta: Vec2,
        capture_radius: f32,
        predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict,
    ) -> Option<MagnetSnap> {
        self.0
            .borrow()
            .compute_item_snap(dragged, drag_delta, capture_radius, predicate)
    }
    /// Best port-drag snap; see [`Scene::compute_port_snap`].
    pub fn compute_port_snap(
        &self,
        source: MagnetId,
        cursor_scene: Point,
        capture_radius: f32,
        predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict,
    ) -> Option<(MagnetRef, Option<std::rc::Rc<dyn std::any::Any>>)> {
        self.0
            .borrow()
            .compute_port_snap(source, cursor_scene, capture_radius, predicate)
    }
    /// Nearest enabled magnet within `radius`; see [`Scene::nearest_magnet`].
    pub fn nearest_magnet(&self, scene_pt: Point, radius: f32) -> Option<MagnetId> {
        self.0.borrow().nearest_magnet(scene_pt, radius)
    }

    // -----------------------------------------------------------------
    // Scene extent / interaction constraints
    // -----------------------------------------------------------------

    /// Set the logical extent of the scene (used for scroll-bar sizing); `None` = unbounded.
    pub fn set_scene_rect(&self, rect: Option<Rect>) {
        self.0.borrow_mut().set_scene_rect(rect);
    }
    /// Restrict panning to horizontal, vertical, or both axes; updates [`pan_axes_signal`](Self::pan_axes_signal).
    pub fn pan_axes(&self, axes: PanAxes) {
        self.0.borrow_mut().pan_axes(axes);
    }
    /// Enable or disable pinch/scroll zoom; updates [`zoomable_signal`](Self::zoomable_signal).
    pub fn zoomable(&self, on: bool) {
        self.0.borrow_mut().zoomable(on);
    }
    /// Clamp the camera pan to `bounds` (scene coordinates); `None` = no limit; updates [`pan_bounds_signal`](Self::pan_bounds_signal).
    pub fn set_pan_bounds(&self, bounds: Option<Rect>) {
        self.0.borrow_mut().set_pan_bounds(bounds);
    }
    /// Restrict the zoom factor to `range`; `None` = no limit; updates [`zoom_range_signal`](Self::zoom_range_signal).
    pub fn set_zoom_range(&self, range: Option<std::ops::RangeInclusive<f32>>) {
        self.0.borrow_mut().set_zoom_range(range);
    }

    // -----------------------------------------------------------------
    // Accessibility structure mutation
    // -----------------------------------------------------------------

    /// Register a logical AT group (landmark / rotor category container); returns its stable [`A11yGroupId`].
    pub fn add_a11y_group(&self, builder: A11yGroupBuilder) -> A11yGroupId {
        self.0.borrow_mut().add_a11y_group(builder)
    }
    /// Remove a previously registered AT group; triggers an `a11y_change_signal` bump.
    pub fn remove_a11y_group(&self, id: A11yGroupId) {
        self.0.borrow_mut().remove_a11y_group(id);
    }
    /// Re-parent `child` in the AT tree, overriding the default visual parent; `None` re-attaches under the scene root.
    pub fn set_a11y_parent(&self, child: A11yNode, parent: Option<A11yNode>) {
        self.0.borrow_mut().set_a11y_parent(child, parent);
    }
    /// Declare a cross-node AT relationship (controls, describes, labels) from `from` to `to`.
    pub fn add_a11y_relation(&self, from: A11yNode, kind: A11yRelation, to: A11yNode) {
        self.0.borrow_mut().add_a11y_relation(from, kind, to);
    }
    /// Mark `node` as a live region (`Polite` or `Assertive`) so assistive tech announces changes to it.
    pub fn set_a11y_live(&self, node: A11yNode, live: accesskit::Live) {
        self.0.borrow_mut().set_a11y_live(node, live);
    }
    /// Assign a landmark `role` to `node` (e.g. `Role::Region`, `Role::Main`) for rotor navigation.
    pub fn set_a11y_landmark(&self, node: A11yNode, role: accesskit::Role) {
        self.0.borrow_mut().set_a11y_landmark(node, role);
    }
    /// Register `node` under the given rotor [`A11yCategory`] slices so it appears in category-filtered navigation.
    pub fn set_a11y_categories(&self, node: A11yNode, categories: &[A11yCategory]) {
        self.0.borrow_mut().set_a11y_categories(node, categories);
    }

    // -----------------------------------------------------------------
    // Dynamic-bounds refresh (called by SceneView::build)
    // -----------------------------------------------------------------

    /// Re-read signal-driven bounds for `add_item_dynamic` entries; returns
    /// `true` if any changed.
    pub fn refresh_dynamic_bounds(&self) -> bool {
        self.0.borrow_mut().refresh_dynamic_bounds()
    }

    // -----------------------------------------------------------------
    // Reactive signals + version
    // -----------------------------------------------------------------

    /// Reactive signal fired on every structural scene change; all views observe this to reconcile.
    pub fn item_change_signal(&self) -> Signal<ItemChange> {
        self.0.borrow().item_change_signal()
    }
    /// Reactive monotonic counter bumped on every AT-structure change; views re-walk accessibility on any increment.
    pub fn a11y_change_signal(&self) -> Signal<u64> {
        self.0.borrow().a11y_change_signal()
    }
    /// Monotonic counter incremented on every mutation; useful for cache invalidation without observing a signal.
    pub fn mutation_version(&self) -> u64 {
        self.0.borrow().mutation_version()
    }
    /// Reactive current [`PanAxes`] restriction; updated by [`pan_axes`](Self::pan_axes).
    pub fn pan_axes_signal(&self) -> Signal<PanAxes> {
        self.0.borrow().pan_axes_signal()
    }
    /// Reactive camera-pan clamp bounds; updated by [`set_pan_bounds`](Self::set_pan_bounds).
    pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>> {
        self.0.borrow().pan_bounds_signal()
    }
    /// Reactive zoom-factor clamp range; updated by [`set_zoom_range`](Self::set_zoom_range).
    pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>> {
        self.0.borrow().zoom_range_signal()
    }
    /// Reactive zoom-enabled flag; updated by [`zoomable`](Self::zoomable).
    pub fn zoomable_signal(&self) -> Signal<bool> {
        self.0.borrow().zoomable_signal()
    }

    // -----------------------------------------------------------------
    // Value queries
    // -----------------------------------------------------------------

    /// Total number of items in the scene (lightweight + heavyweight).
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }
    /// Returns `true` when the scene contains no items.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
    /// All [`ItemId`]s currently in the scene, in insertion order.
    pub fn ids(&self) -> Vec<ItemId> {
        self.0.borrow().ids()
    }
    /// The local position of `id` in its parent's coordinate space; `None` if `id` is unknown.
    pub fn local_pos(&self, id: ItemId) -> Option<Point> {
        self.0.borrow().local_pos(id)
    }
    /// The local bounding rect of `id`; `None` if `id` is unknown.
    pub fn local_bounds(&self, id: ItemId) -> Option<Rect> {
        self.0.borrow().local_bounds(id)
    }
    /// The additional local-to-parent transform of `id` (beyond position); `None` if none is set.
    pub fn transform(&self, id: ItemId) -> Option<Transform2D> {
        self.0.borrow().transform(id)
    }
    /// The full local-to-scene transform for `id` (parent chain composed); identity if `id` is unknown.
    pub fn scene_transform(&self, id: ItemId) -> Transform2D {
        self.0.borrow().scene_transform(id)
    }
    /// The origin of `id` mapped into scene coordinates; `None` if `id` is unknown.
    pub fn scene_pos(&self, id: ItemId) -> Option<Point> {
        self.0.borrow().scene_pos(id)
    }
    /// The bounding rect of `id` in scene coordinates (local bounds transformed by the parent chain); `None` if unknown.
    pub fn scene_rect(&self, id: ItemId) -> Option<Rect> {
        self.0.borrow().scene_rect(id)
    }
    /// The [`ItemFlags`] bitset of `id`; `None` if `id` is unknown.
    pub fn flags(&self, id: ItemId) -> Option<ItemFlags> {
        self.0.borrow().flags(id)
    }
    /// Returns `true` if `id` and all of its ancestors are visible.
    pub fn is_effectively_visible(&self, id: ItemId) -> bool {
        self.0.borrow().is_effectively_visible(id)
    }
    /// The own opacity of `id` (ignoring ancestors); `None` if `id` is unknown.
    pub fn opacity(&self, id: ItemId) -> Option<f32> {
        self.0.borrow().opacity(id)
    }
    /// Accumulated opacity for `id` (own × each ancestor's opacity).
    pub fn effective_opacity(&self, id: ItemId) -> f32 {
        self.0.borrow().effective_opacity(id)
    }
    /// The z-order value of `id` within its layer; `None` if `id` is unknown.
    pub fn z(&self, id: ItemId) -> Option<f32> {
        self.0.borrow().z(id)
    }
    /// The [`SceneLayer`] of `id`; `None` if `id` is unknown.
    pub fn layer(&self, id: ItemId) -> Option<SceneLayer> {
        self.0.borrow().layer(id)
    }
    /// The direct parent of `id`, or `None` if it is a root item (or unknown).
    pub fn parent_of(&self, id: ItemId) -> Option<ItemId> {
        self.0.borrow().parent_of(id)
    }
    /// Returns `true` if `id` is anywhere in `ancestor`'s subtree.
    pub fn is_descendant_of(&self, id: ItemId, ancestor: ItemId) -> bool {
        self.0.borrow().is_descendant_of(id, ancestor)
    }
    /// The logical extent set via [`set_scene_rect`](Self::set_scene_rect); `None` = unbounded.
    pub fn scene_rect_extent(&self) -> Option<Rect> {
        self.0.borrow().scene_rect_extent()
    }
    /// The current pan-axis restriction without subscribing to its signal.
    pub fn current_pan_axes(&self) -> PanAxes {
        self.0.borrow().current_pan_axes()
    }
    /// Returns `true` if zoom is currently enabled (snapshot; use [`zoomable_signal`](Self::zoomable_signal) for reactivity).
    pub fn is_zoomable(&self) -> bool {
        self.0.borrow().is_zoomable()
    }
    /// Current pan-clamp bounds without subscribing to its signal.
    pub fn current_pan_bounds(&self) -> Option<Rect> {
        self.0.borrow().current_pan_bounds()
    }
    /// Current zoom-factor clamp range without subscribing to its signal.
    pub fn current_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>> {
        self.0.borrow().current_zoom_range()
    }
    /// All items whose bounding rects overlap `scene_rect` (spatial-index query).
    pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId> {
        self.0.borrow().items_in_rect(scene_rect)
    }
    /// The topmost item under `scene_pt` using exact-shape hit-testing; `None` if no item is hit.
    pub fn item_at(&self, scene_pt: Point) -> Option<ItemId> {
        self.0.borrow().item_at(scene_pt)
    }
    /// All items under `scene_pt` (exact-shape hit-test), ordered front-to-back.
    pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId> {
        self.0.borrow().items_at(scene_pt)
    }
    /// All items whose bounding rects intersect `id`'s bounding rect.
    pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId> {
        self.0.borrow().colliding_items(id)
    }
    /// The AT-tree parent of `child` as set by [`set_a11y_parent`](Self::set_a11y_parent); `None` = visual default.
    pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode> {
        self.0.borrow().a11y_parent_of(child)
    }

    // -----------------------------------------------------------------
    // Build-support (consumed by SceneView::build)
    // -----------------------------------------------------------------

    /// Drain every still-pending single-view (`Once`) widget, in entry order.
    pub(crate) fn drain_all_once(&self) -> Vec<(ItemId, Box<dyn Widget>)> {
        self.0.borrow_mut().drain_all_once()
    }
    /// `(id, payload)` for every multi-view (`Delegated`) item, in entry order.
    pub(crate) fn delegated_payloads(&self) -> Vec<(ItemId, Rc<dyn std::any::Any>)> {
        self.0.borrow().delegated_payloads()
    }
    /// Ids of every heavyweight widget entry, in entry order.
    pub(crate) fn heavyweight_ids(&self) -> Vec<ItemId> {
        self.0.borrow().heavyweight_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::RectItem;
    use bastyde_canvas::Point;

    fn rect() -> Rect {
        Rect::new(0.0, 0.0, 10.0, 10.0)
    }

    #[test]
    fn clone_shares_data() {
        let m1 = SceneModel::new();
        let m2 = m1.clone();
        let id = m1.add_item(RectItem::new(rect()), Point::ZERO);
        assert_eq!(m2.len(), 1);
        assert_eq!(m2.local_pos(id), Some(Point::ZERO));
        m1.set_local_pos(id, Point::new(10.0, 0.0));
        assert_eq!(m2.local_pos(id), Some(Point::new(10.0, 0.0)));
        assert_eq!(m1.handle_count(), 2);
    }

    #[test]
    fn payload_round_trip_and_signal_fires() {
        let m1 = SceneModel::new();
        let m2 = m1.clone();
        let fired = Rc::new(std::cell::Cell::new(false));
        let f = fired.clone();
        let _h = m1.item_change_signal().observe(move |c| {
            if matches!(c, ItemChange::PayloadChanged { .. }) {
                f.set(true);
            }
        });
        let id = m1.add_widget_item(42u32, rect());
        assert_eq!(
            m1.payload(id)
                .and_then(|p| p.downcast_ref::<u32>().copied()),
            Some(42)
        );
        assert_eq!(
            m2.payload(id)
                .and_then(|p| p.downcast_ref::<u32>().copied()),
            Some(42)
        );
        m1.set_payload(id, 99u32);
        assert!(fired.get());
        assert_eq!(
            m2.payload(id)
                .and_then(|p| p.downcast_ref::<u32>().copied()),
            Some(99)
        );
    }

    #[test]
    fn remove_drops_payload_rc() {
        let m = SceneModel::new();
        let id = m.add_widget_item(42u32, rect());
        let weak = Rc::downgrade(&m.payload(id).unwrap());
        assert!(weak.upgrade().is_some());
        m.remove(id);
        assert!(weak.upgrade().is_none(), "payload Rc leaked after remove");
    }

    #[test]
    fn delegated_storage_and_build_helpers() {
        // The `Once` drain-with-a-real-widget path is covered by the view
        // multi-view tests (which have an arena); here we exercise the model
        // bookkeeping for `Delegated` entries without needing a `Widget`.
        let m = SceneModel::new();
        let a = m.add_widget_item(7u8, rect());
        let b = m.add_widget_item(8u8, rect());
        assert!(m.payload(a).is_some());
        assert!(m.payload(b).is_some());
        assert_eq!(m.heavyweight_ids(), vec![a, b]);
        assert_eq!(m.delegated_payloads().len(), 2);
        assert!(m.drain_all_once().is_empty(), "no Once entries to drain");
    }

    #[test]
    fn mutation_version_advances() {
        let m = SceneModel::new();
        let v0 = m.mutation_version();
        let id = m.add_widget_item(0u32, rect());
        let v1 = m.mutation_version();
        assert_ne!(v1, v0);
        m.set_payload(id, 1u32);
        let v2 = m.mutation_version();
        assert_ne!(v2, v1);
        m.remove(id);
        assert_ne!(m.mutation_version(), v2);
    }

    #[test]
    fn item_change_signal_shared_across_handles() {
        let m1 = SceneModel::new();
        let m2 = m1.clone();
        assert!(Signal::same(
            &m1.item_change_signal(),
            &m2.item_change_signal()
        ));
    }
}
