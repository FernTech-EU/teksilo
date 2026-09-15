// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneModel`] — a shared, cloneable handle to a [`Scene`].
//!
//! Mirrors the `ListModel = Rc<RefCell<ListModelInner>>` pattern from
//! `teksilo-data`: cloning a `SceneModel` produces a **second handle to the
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
//! Every mutator takes `&self` and runs through [`SceneModel::write`], which
//! is the `teksilo-data` **mutate-then-notify** discipline adapted to a borrow
//! that spans a whole `&mut Scene` method. `ListModel` can scope its
//! `borrow_mut()` and call `notify` after it (`list_model.rs`); `Scene` cannot,
//! because the notification is emitted from *inside* a `&mut self` method whose
//! borrow Rust only releases on return. So the notification is **queued in the
//! `Scene`** and drained by the owner of the borrow — `write`, or
//! [`SceneWriteGuard`] — once that borrow has dropped.
//!
//! The consequence is the contract app authors actually want: **an observer
//! registered on [`item_change_signal`](SceneModel::item_change_signal) or
//! [`a11y_change_signal`](SceneModel::a11y_change_signal) may read the scene
//! and write it back.** A write made from an observer queues in turn and is
//! delivered by the same drain, after whatever was already queued ahead of it —
//! see [`SceneModel::flush_changes`] for ordering, re-entrancy and termination.
//!
//! Three doors are **not** covered, each for a stated reason:
//!
//! - A bare `&mut Scene` fans out synchronously, because nothing is holding a
//!   `RefCell` open for it to escape from. For a `Scene` you own outright that
//!   is safe — no second handle to it can exist. For a `&mut Scene` *reborrowed
//!   out of a `SceneModel`* it is not: a raw `RefMut` from the shared cell
//!   leaves the write scope closed, so observers run under the live exclusive
//!   borrow and one that re-enters the model panics, exactly as every mutator
//!   used to. [`SceneModel::write_guard`] is the fix and the substitute — it
//!   derefs to `&mut Scene`, so the call sites are unchanged.
//! - The four constraint signals (`pan_axes` / `zoomable` / `pan_bounds` /
//!   `zoom_range`) are scene *state*, read back by `current_pan_axes` and
//!   friends, so their writes stay synchronous — deferring them would make the
//!   scene contradict itself inside a write scope. An observer on one of those
//!   four must not re-enter the `SceneModel`.
//! - [`with_handlers_mut`](SceneModel::with_handlers_mut) runs the caller's
//!   closure *inside* the borrow (it hands out a `&mut` into the scene, which
//!   cannot outlive it), so that closure must not re-enter the `SceneModel`
//!   either. It is unwind-safe — a panic in the closure cannot strand the write
//!   scope — but it is not re-entrant.
//!
//! A view **delegate** must also not synchronously mutate the model during a
//! build-time call (the view drops all model borrows before invoking it; the
//! delegate's *handlers* may mutate later).

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, StrokeStyle, Transform2D};
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;

use crate::a11y::{A11yCategory, A11yGroupBuilder, A11yGroupId, A11yNode, A11yRelation};
use crate::flags::ItemFlags;
use crate::index::SpatialIndex;
use crate::item::{ItemId, SceneItem};
use crate::item_handlers::SceneItemHandlerSet;
use crate::magnet::{Magnet, MagnetId, MagnetRef, MagnetSnap, MagnetVerdict};
use crate::scene::{CascadeBudget, ItemChange, PanAxes, Scene, SceneLayer};
use teksilo_canvas::Vec2;

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

/// Write guard over a [`SceneModel`]'s [`Scene`]: `Deref`/`DerefMut` to the
/// scene, with the deferred-notification contract of a `SceneModel` mutator.
///
/// Holding one keeps a *write scope* open, so every notification the edits
/// produce queues in emission order. On drop the guard closes the scope,
/// releases the `RefCell` borrow, and **then** fans the whole batch out — so a
/// block of edits is one notification round, and an observer may read the
/// scene or write it back.
///
/// ```
/// # use teksilo_scene::{RectItem, SceneModel};
/// # use teksilo_canvas::{Point, Rect};
/// let model = SceneModel::new();
/// let a = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
/// {
///     let mut scene = model.write_guard();
///     scene.set_local_pos(a, Point::new(10.0, 10.0));
///     scene.set_visible(a, false);
/// } // both changes fan out here, in this order
/// ```
///
/// If the thread is unwinding when the guard drops, the scope is closed and
/// the borrow released but the fan-out is **skipped**: running observers from
/// a `Drop` during an unwind would abort the process the moment one of them
/// panicked. The queued changes survive and are delivered by the next
/// [`flush_changes`](SceneModel::flush_changes) — which a `catch_unwind`
/// recovery path can call explicitly.
pub struct SceneWriteGuard<'a> {
    /// `Some` for the guard's whole life; `None` only inside `Drop`, which
    /// takes the `RefMut` out in order to release the borrow *before* the
    /// fan-out.
    scene: Option<RefMut<'a, Scene>>,
    model: &'a SceneModel,
}

impl std::fmt::Debug for SceneWriteGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneWriteGuard")
            .field("len", &self.scene.as_ref().map(|s| s.len()))
            .finish_non_exhaustive()
    }
}

impl std::ops::Deref for SceneWriteGuard<'_> {
    type Target = Scene;
    fn deref(&self) -> &Scene {
        self.scene.as_ref().expect("guard alive outside Drop")
    }
}

impl std::ops::DerefMut for SceneWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Scene {
        self.scene.as_mut().expect("guard alive outside Drop")
    }
}

impl Drop for SceneWriteGuard<'_> {
    fn drop(&mut self) {
        if let Some(scene) = self.scene.take() {
            scene.exit_write_scope();
            drop(scene); // borrow released here, before any observer runs
        }
        if std::thread::panicking() {
            return;
        }
        self.model.flush_changes();
    }
}

impl SceneModel {
    // -----------------------------------------------------------------
    // Write scope / deferred fan-out
    // -----------------------------------------------------------------

    /// Open a write scope over the scene; see [`SceneWriteGuard`].
    ///
    /// The one door for a *block* of edits that must fan out together, and the
    /// door a `&mut Scene` escape hatch should be built on.
    pub fn write_guard(&self) -> SceneWriteGuard<'_> {
        let scene = self.0.borrow_mut();
        scene.enter_write_scope();
        SceneWriteGuard {
            scene: Some(scene),
            model: self,
        }
    }

    /// Mutate the scene under a write scope, then fan out.
    ///
    /// The body every `&self` mutator on this type is. Unwind-safe by
    /// construction: the scope is closed by [`SceneWriteGuard`]'s `Drop`, so a
    /// panic inside `f` cannot strand the scene in "deferring forever, never
    /// flushing" — the one way this mechanism could fail silently.
    fn write<R>(&self, f: impl FnOnce(&mut Scene) -> R) -> R {
        let mut guard = self.write_guard();
        let out = f(&mut guard);
        drop(guard); // release the borrow, then fan out
        out
    }

    /// Deliver every queued notification.
    ///
    /// Called automatically when a write scope closes, so apps rarely need it.
    /// The exception is recovering from a caught panic: an unwinding
    /// [`SceneWriteGuard`] deliberately skips its fan-out, and a panicking
    /// observer stops the drain where it stood, so a `catch_unwind` boundary
    /// that intends to keep using the scene calls this once.
    ///
    /// # When it does nothing
    ///
    /// *Where* it is called from never makes it panic — this is the door a
    /// recovery path is told to knock on, so it cannot be one that only works
    /// from some of them. (An observer can still panic once the drain reaches
    /// it, and so can the runaway budgets below; both are the observer's
    /// doing, not the call site's.) It is a no-op, and says so rather than
    /// delivering half a batch, when:
    ///
    /// - nothing is queued (overwhelmingly the common case — every mutator that
    ///   early-returned on an unchanged value, and every per-frame
    ///   [`refresh_dynamic_bounds`](Self::refresh_dynamic_bounds));
    /// - **any** borrow on the scene is outstanding. A write scope is the
    ///   obvious one — it owns its batch and fans out when it closes, so
    ///   flushing from inside it would deliver a half-finished mutation, and
    ///   the old implementation's `borrow()` simply panicked there. But a
    ///   *shared* borrow disqualifies a flush just as firmly, because an
    ///   observer is allowed to write and a write needs the cell exclusively:
    ///   draining under a read-only policy closure (`SceneView::focus_order`,
    ///   `MagnetismConfig`'s predicate) would panic the first observer that
    ///   took the invitation. So the probe is `try_borrow_mut`, the only
    ///   question whose answer is "the cell is completely free";
    /// - a drain is already running, i.e. this call came from inside an
    ///   observer. The outermost drain owns the queue and picks that observer's
    ///   changes up on its next round, which is what keeps delivery in emission
    ///   order rather than depth-first.
    ///
    /// # Ordering
    ///
    /// FIFO. Observers see changes in the order they were emitted, including
    /// across a batch — so `remove`'s documented leaves-then-root order, and
    /// the interleaving of the item and logical-AT channels, both survive.
    ///
    /// # Termination
    ///
    /// The drain keeps going until the queue is empty, so an observer's own
    /// writes are always delivered rather than silently dropped. Most cycles
    /// terminate on their own — `Scene::set_local_pos` and friends early-return
    /// when the value is unchanged, so the snap-to-grid shape settles in two
    /// rounds.
    ///
    /// An observer that writes on **every** change with no equality guard never
    /// settles, and is stopped by a budget that panics with a diagnostic naming
    /// the item at fault.
    ///
    /// **What the budget counts is total observer-generated deliveries in one
    /// drain, and nothing else.** The caller's own batch — everything queued
    /// before the first observer ran — is free at any size, because it is finite
    /// by construction; so are cascade depth and the number of distinct subjects
    /// a cascade touches. A cap on **rounds** would cap how long a legitimate
    /// chain may be (a settling 300-link chain is 300 rounds); a cap **per
    /// subject** would cap how wide a graph may be, and scaling that cap with
    /// the scene makes the work a runaway gets to do scale with the scene too. A
    /// flat total is the one bound under which a runaway costs the same number
    /// of deliveries and the same peak memory whatever the model's size, and the
    /// shapes this tier runs clear it by more than an order of magnitude. See
    /// [`CascadeBudget`] for the measurements.
    ///
    /// Per-subject counts are still kept — they are what lets the panic name the
    /// culprit, which a round counter never could — but they are diagnostics,
    /// not a second trip condition.
    ///
    /// The budget is **not** debug-only, and this is the one place the
    /// mechanism deliberately does not copy `Signal::try_set`: `try_set`
    /// *recurses*, so an unchecked feedback loop there exhausts the stack and
    /// aborts loudly by itself. This drain is an iterative loop, so an unchecked
    /// loop here is a frozen UI thread with no diagnostic and no core dump — the
    /// one failure mode a release build must not have. Trading a hang for a
    /// panic is not a close call.
    ///
    /// The queue is left intact when a budget trips (dropping it would be the
    /// silent notification loss the whole mechanism exists to prevent), so a
    /// later flush with the same observer still installed panics again. That is
    /// the intended reading: the observer, not the flush, is what has to change.
    ///
    /// # Unwind
    ///
    /// An observer that panics costs exactly its own delivery. Each
    /// notification leaves the queue immediately before it is handed to the
    /// signal and is never parked in a local batch, so everything not yet
    /// delivered is still queued, in order, when the unwind passes through —
    /// and the next call here delivers it. (A local batch would have its
    /// untouched tail dropped by the unwind with nothing able to redeliver it:
    /// the scene would have advanced with no notification, permanently.)
    ///
    /// # Coalescing
    ///
    /// None. Repeated `LocalPosChanged` for one item are delivered once each.
    /// Two reasons: the advertised uses of this channel — persistence,
    /// validation, audit, mirroring to a data layer — are the ones that need
    /// every event; and collapsing two changes would require merging one's
    /// `old` with the other's `new`, a transaction semantic this crate does not
    /// define. A drag is not a reason to reconsider: each pointer sample is its
    /// own `SceneModel` call, hence its own one-element batch, exactly as
    /// before. If a transaction layer ever wants coalescing, [`SceneWriteGuard`]
    /// is already its boundary.
    ///
    /// # Lifetime
    ///
    /// Queued notifications live in the `Scene`. Dropping the last
    /// `SceneModel` handle drops the scene and discards anything still queued —
    /// which can only be a batch abandoned by an unwind, and by then there is
    /// no scene left to observe.
    pub fn flush_changes(&self) {
        let (queue, budget) = {
            // `try_borrow_mut` is the probe, not `try_borrow`: it is the only
            // one that answers "is this cell completely free?", and anything
            // less means an observer taking up the invitation to write would
            // panic on the borrow. Failure is a no-op by contract — see the
            // "When it does nothing" section above.
            let Ok(scene) = self.0.try_borrow_mut() else {
                return;
            };
            if !scene.has_pending_notifications() {
                return;
            }
            // Read here, under the borrow, because the drain below has none
            // — and read once, so an observer cannot raise its own ceiling
            // mid-drain.
            (scene.change_queue(), scene.cascade_budget())
        };
        // The borrow above is gone: the observers below run with the scene
        // free, and may read it or write it back.
        queue.drain(budget);
    }

    /// The runaway-detection budget for this scene's change fan-out — how much
    /// work **observers** may generate from one batch before the drain declares
    /// the cascade non-terminating and panics.
    ///
    /// See [`CascadeBudget`] for the rule and its default.
    pub fn cascade_budget(&self) -> CascadeBudget {
        self.0.borrow().cascade_budget()
    }

    /// Replace the runaway-detection budget for this scene's change fan-out.
    ///
    /// The mechanism is in the framework; this is where the policy lives. The
    /// default — 100 000 observer-generated deliveries per drain — is more than
    /// an order of magnitude above the legitimate shapes this tier runs, but it
    /// cannot tell a guarded cascade that is genuinely enormous from an
    /// unguarded write-back: the two look identical from the queue's side. An
    /// app whose graph genuinely settles past the default therefore has a
    /// supported answer here rather than a crash:
    ///
    /// ```
    /// use teksilo_scene::{CascadeBudget, SceneModel};
    ///
    /// let model = SceneModel::new();
    /// model.set_cascade_budget(CascadeBudget::new(1_000_000));
    /// assert_eq!(model.cascade_budget().total, 1_000_000);
    /// ```
    ///
    /// Raising it to silence a cycle that does **not** settle only postpones
    /// the freeze it is there to prevent — the drain is still unbounded in
    /// time, just later. Check the write is guarded first.
    ///
    /// A budget of zero is clamped to the smallest usable one rather than
    /// stored: it would forbid the reactive write-back this channel exists for.
    ///
    /// Takes effect on the next drain: a drain already running read its budget
    /// when it began, so an observer cannot enlarge its own.
    pub fn set_cascade_budget(&self, budget: CascadeBudget) {
        self.0.borrow().set_cascade_budget(budget);
    }

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
        self.write(|s| s.add_widget(widget, rect))
    }

    /// Multi-view heavyweight item: store a typed `payload`; each view builds
    /// its own widget instance from it via its delegate. Returns the [`ItemId`].
    pub fn add_widget_item<P: 'static>(&self, payload: P, rect: Rect) -> ItemId {
        self.write(|s| s.add_widget_delegated(Rc::new(payload), rect))
    }

    /// Replace the payload of a `Delegated` heavyweight item; every view
    /// rebuilds that item's widget on the next pass.
    ///
    /// # Panics
    ///
    /// Panics if `id` is unknown, refers to a single-view `add_widget` (Once)
    /// entry, or refers to a lightweight item.
    pub fn set_payload<P: 'static>(&self, id: ItemId, payload: P) {
        self.write(|s| s.set_payload(id, Rc::new(payload)));
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
        self.write(|s| s.add_item(item, local_pos))
    }

    /// Add a lightweight item with signal-driven (dynamic) bounds.
    pub fn add_item_dynamic<I: SceneItem + 'static>(&self, item: I, local_pos: Point) -> ItemId {
        self.write(|s| s.add_item_dynamic(item, local_pos))
    }

    /// Add an already-boxed lightweight item at `local_pos`. The boxed-`dyn`
    /// counterpart of [`add_item`](Self::add_item), used by
    /// [`SceneListAdapter`](crate::SceneListAdapter).
    pub fn add_boxed_item(&self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId {
        self.write(|s| s.add_boxed_item(item, local_pos))
    }

    // -----------------------------------------------------------------
    // Geometry mutation
    // -----------------------------------------------------------------

    /// Move `id` to `local_pos` in its parent's coordinate space; notifies all views.
    pub fn set_local_pos(&self, id: ItemId, local_pos: Point) {
        self.write(|s| s.set_local_pos(id, local_pos));
    }
    /// Replace the local bounding rect of `id`; notifies all views.
    pub fn set_local_bounds(&self, id: ItemId, local_bounds: Rect) {
        self.write(|s| s.set_local_bounds(id, local_bounds));
    }
    /// Set an additional local-to-parent transform (rotation, scale) on `id`; notifies all views.
    pub fn set_transform(&self, id: ItemId, transform: Transform2D) {
        self.write(|s| s.set_transform(id, transform));
    }

    // -----------------------------------------------------------------
    // Flags / visibility / opacity mutation
    // -----------------------------------------------------------------

    /// Replace the complete [`ItemFlags`] bitset for `id`; notifies all views.
    pub fn set_flags(&self, id: ItemId, flags: ItemFlags) {
        self.write(|s| s.set_flags(id, flags));
    }
    /// Set or clear a single [`ItemFlags`] bit on `id`; notifies all views.
    pub fn set_flag(&self, id: ItemId, flag: ItemFlags, on: bool) {
        self.write(|s| s.set_flag(id, flag, on));
    }
    /// Show or hide `id` (also hides its descendants); notifies all views.
    pub fn set_visible(&self, id: ItemId, visible: bool) {
        self.write(|s| s.set_visible(id, visible));
    }
    /// Set the paint opacity of `id` (0.0 = transparent, 1.0 = opaque); notifies all views.
    pub fn set_opacity(&self, id: ItemId, opacity: f32) {
        self.write(|s| s.set_opacity(id, opacity));
    }

    // -----------------------------------------------------------------
    // Appearance mutation (paint-only, repaint without relayout)
    // -----------------------------------------------------------------

    /// Replace a lightweight item's fill colour live; every view repaints
    /// (no relayout/rebuild). Accepts a plain [`Color`](teksilo_tokens::Color),
    /// a theme role, a `Signal<Color>`, or a `Signal<Role>`. See
    /// [`Scene::set_item_fill`] for the reactive-colour contract.
    pub fn set_item_fill(&self, id: ItemId, fill: impl Into<ColorProp>) {
        self.write(|s| s.set_item_fill(id, fill));
    }
    /// Clear a lightweight item's fill; every view repaints.
    pub fn clear_item_fill(&self, id: ItemId) {
        self.write(|s| s.clear_item_fill(id));
    }
    /// Replace a lightweight item's stroke (colour + [`StrokeStyle`]) live;
    /// every view repaints (no relayout/rebuild).
    pub fn set_item_stroke(&self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle) {
        self.write(|s| s.set_item_stroke(id, color, style));
    }
    /// Clear a lightweight item's stroke; every view repaints.
    pub fn clear_item_stroke(&self, id: ItemId) {
        self.write(|s| s.clear_item_stroke(id));
    }

    // -----------------------------------------------------------------
    // Z-order / layer / parenting mutation
    // -----------------------------------------------------------------

    /// Set the z-order of `id` within its layer; higher values paint on top.
    pub fn set_z(&self, id: ItemId, z: f32) {
        self.write(|s| s.set_z(id, z));
    }
    /// Give `id` the highest z-value in its layer so it paints on top of all siblings.
    pub fn bring_to_front(&self, id: ItemId) {
        self.write(|s| s.bring_to_front(id));
    }
    /// Give `id` the lowest z-value in its layer so it paints beneath all siblings.
    pub fn send_to_back(&self, id: ItemId) {
        self.write(|s| s.send_to_back(id));
    }
    /// Move `id` to a different [`SceneLayer`] (background, default, foreground); notifies all views.
    pub fn set_layer(&self, id: ItemId, layer: SceneLayer) {
        self.write(|s| s.set_layer(id, layer));
    }
    /// Re-parent `child` under `parent` (or under the scene root when `None`); notifies all views.
    pub fn set_item_parent(&self, child: ItemId, parent: Option<ItemId>) {
        self.write(|s| s.set_item_parent(child, parent));
    }

    // -----------------------------------------------------------------
    // Removal
    // -----------------------------------------------------------------

    /// Remove an item and its descendants. Drops any `Delegated` payload `Rc`
    /// and cleans the item's a11y mappings; alive logical children re-root.
    pub fn remove(&self, id: ItemId) {
        self.write(|s| s.remove(id));
    }
    /// Promote an item's children to the scene root.
    pub fn orphan(&self, id: ItemId) {
        self.write(|s| s.orphan(id));
    }

    // -----------------------------------------------------------------
    // Handlers
    // -----------------------------------------------------------------

    /// Replace the [`SceneItemHandlerSet`] of `id`, or clear it with `None`.
    pub fn set_item_handlers(&self, id: ItemId, handlers: Option<SceneItemHandlerSet>) {
        self.write(|s| s.set_item_handlers(id, handlers));
    }
    /// Mutate an item's handler set through a closure (avoids returning a
    /// borrow guard tied to the `RefMut`).
    pub fn with_handlers_mut<R>(
        &self,
        id: ItemId,
        f: impl FnOnce(&mut SceneItemHandlerSet) -> R,
    ) -> Option<R> {
        self.write(|s| s.handlers_mut(id).map(f))
    }

    // -----------------------------------------------------------------
    // Magnetism
    // -----------------------------------------------------------------

    /// Attach a [`Magnet`] to `item`; see [`Scene::add_magnet`].
    pub fn add_magnet(&self, item: ItemId, magnet: Magnet) -> MagnetId {
        self.write(|s| s.add_magnet(item, magnet))
    }
    /// Remove a magnet by id; see [`Scene::remove_magnet`].
    pub fn remove_magnet(&self, magnet: MagnetId) {
        self.write(|s| s.remove_magnet(magnet));
    }
    /// Remove every magnet on `item`; see [`Scene::clear_magnets`].
    pub fn clear_magnets(&self, item: ItemId) {
        self.write(|s| s.clear_magnets(item));
    }
    /// Move a magnet in its item's local frame; see [`Scene::set_magnet_local_pos`].
    pub fn set_magnet_local_pos(&self, magnet: MagnetId, local_pos: Point) {
        self.write(|s| s.set_magnet_local_pos(magnet, local_pos));
    }
    /// Enable or disable a magnet; see [`Scene::set_magnet_enabled`].
    pub fn set_magnet_enabled(&self, magnet: MagnetId, enabled: bool) {
        self.write(|s| s.set_magnet_enabled(magnet, enabled));
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
        self.write(|s| s.set_scene_rect(rect));
    }
    /// Restrict panning to horizontal, vertical, or both axes; updates [`pan_axes_signal`](Self::pan_axes_signal).
    pub fn pan_axes(&self, axes: PanAxes) {
        self.write(|s| s.pan_axes(axes));
    }
    /// Enable or disable pinch/scroll zoom; updates [`zoomable_signal`](Self::zoomable_signal).
    pub fn zoomable(&self, on: bool) {
        self.write(|s| s.zoomable(on));
    }
    /// Clamp the camera pan to `bounds` (scene coordinates); `None` = no limit; updates [`pan_bounds_signal`](Self::pan_bounds_signal).
    pub fn set_pan_bounds(&self, bounds: Option<Rect>) {
        self.write(|s| s.set_pan_bounds(bounds));
    }
    /// Restrict the zoom factor to `range`; `None` = no limit; updates [`zoom_range_signal`](Self::zoom_range_signal).
    pub fn set_zoom_range(&self, range: Option<std::ops::RangeInclusive<f32>>) {
        self.write(|s| s.set_zoom_range(range));
    }

    // -----------------------------------------------------------------
    // Accessibility structure mutation
    // -----------------------------------------------------------------

    /// Register a logical AT group (landmark / rotor category container); returns its stable [`A11yGroupId`].
    pub fn add_a11y_group(&self, builder: A11yGroupBuilder) -> A11yGroupId {
        self.write(|s| s.add_a11y_group(builder))
    }
    /// Remove a previously registered AT group; triggers an `a11y_change_signal` bump.
    pub fn remove_a11y_group(&self, id: A11yGroupId) {
        self.write(|s| s.remove_a11y_group(id));
    }
    /// Re-parent `child` in the AT tree, overriding the default visual parent; `None` re-attaches under the scene root.
    pub fn set_a11y_parent(&self, child: A11yNode, parent: Option<A11yNode>) {
        self.write(|s| s.set_a11y_parent(child, parent));
    }
    /// Declare a cross-node AT relationship (controls, describes, labels) from `from` to `to`.
    pub fn add_a11y_relation(&self, from: A11yNode, kind: A11yRelation, to: A11yNode) {
        self.write(|s| s.add_a11y_relation(from, kind, to));
    }
    /// Mark `node` as a live region (`Polite` or `Assertive`) so assistive tech announces changes to it.
    pub fn set_a11y_live(&self, node: A11yNode, live: accesskit::Live) {
        self.write(|s| s.set_a11y_live(node, live));
    }
    /// Assign a landmark `role` to `node` (e.g. `Role::Region`, `Role::Main`) for rotor navigation.
    pub fn set_a11y_landmark(&self, node: A11yNode, role: accesskit::Role) {
        self.write(|s| s.set_a11y_landmark(node, role));
    }
    /// Register `node` under the given rotor [`A11yCategory`] slices so it appears in category-filtered navigation.
    pub fn set_a11y_categories(&self, node: A11yNode, categories: &[A11yCategory]) {
        self.write(|s| s.set_a11y_categories(node, categories));
    }

    // -----------------------------------------------------------------
    // Dynamic-bounds refresh (called by SceneView::build)
    // -----------------------------------------------------------------

    /// Re-read signal-driven bounds for `add_item_dynamic` entries; returns
    /// `true` if any changed.
    pub fn refresh_dynamic_bounds(&self) -> bool {
        self.write(|s| s.refresh_dynamic_bounds())
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
    /// [`mutation_version`](Self::mutation_version) with the per-frame churn of
    /// [`refresh_dynamic_bounds`](Self::refresh_dynamic_bounds) excluded — the
    /// version to gate an expensive rebuild on. See
    /// [`Scene::structural_version`].
    pub fn structural_version(&self) -> u64 {
        self.0.borrow().structural_version()
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
        self.write(|s| s.drain_all_once())
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
    use teksilo_canvas::Point;

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
