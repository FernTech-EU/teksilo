// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Camera controls for [`SceneView`]: pan, zoom, rotation, viewport queries,
//! and fit-to-content helpers.
//!
//! Every mutating method in this file operates on `&self` via `Signal::set` /
//! `Signal::animate_to`, so a handler or clone of the view can drive the
//! camera without `with_widget_mut`. The view transform is composed in
//! [`compose_view`] from four independent
//! `Signal<f32>` values (`pan_x`, `pan_y`, `zoom`, `rotation`) so each axis
//! can animate with its own easing and epsilon. Reactive viewport queries
//! ([`viewport_in_scene_signal`](SceneView::viewport_in_scene_signal)) expose
//! the visible scene region as a `Signal<Rect>` suitable for driving a
//! [`SceneMinimap`](crate::minimap::SceneMinimap) or lazy-loading logic.

use super::*;
use teksilo_core::event::{ScrollAlign, ScrollMotion};

/// The camera, as a bundle of shared handles that outlives the `&self` that
/// built it.
///
/// [`SceneView::ensure_visible`] and friends are `&self` methods, which is
/// enough for an app holding the view. A **handler** is not: a `HandlerSet`
/// closure is built once and lives in the arena, long after the `&SceneView`
/// that installed it is gone. The reveal arm on `on_scroll` has to move the
/// camera from inside such a closure, so the camera has to be something a
/// closure can own — the same shape [`TransformDriver`](super::transform::TransformDriver)
/// takes, and for the same reason.
///
/// Every field is a handle into live state, so a `Camera` cloned at build time
/// still honours a pan-axes policy or a pan-bounds clamp the app changed
/// afterwards.
#[derive(Clone)]
pub(crate) struct Camera {
    pub pan_x: Signal<f32>,
    pub pan_y: Signal<f32>,
    pub zoom: Signal<f32>,
    pub rotation: Signal<f32>,
    pub bounds_origin: Signal<Vec2>,
    pub viewport: Signal<Size>,
    pub pan_axes: Signal<crate::scene::PanAxes>,
    pub scene_pan_bounds: Signal<Option<Rect>>,
    pub view_pan_bounds: Signal<Option<Rect>>,
    pub adopt_scene_size: bool,
    pub anim_duration: Duration,
}

impl std::fmt::Debug for Camera {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Camera")
            .field("pan", &Vec2::new(self.pan_x.get(), self.pan_y.get()))
            .field("zoom", &self.zoom.get())
            .field("rotation", &self.rotation.get())
            .finish()
    }
}

impl Camera {
    /// The scene → screen transform, composed exactly as
    /// [`SceneView::view_transform`] composes it.
    fn view_transform(&self) -> Transform2D {
        let bo = self.bounds_origin.get();
        compose_view(
            Vec2::new(self.pan_x.get() + bo.x, self.pan_y.get() + bo.y),
            self.zoom.get(),
            self.rotation.get(),
        )
    }

    /// The same gate [`SceneView::gate_pan_target`] applies: pan-axes policy,
    /// then the intersection of the scene's and the view's pan bounds.
    fn gate_pan_target(&self, target: Vec2) -> Vec2 {
        let hold = Vec2::new(self.pan_x.get(), self.pan_y.get());
        if self.adopt_scene_size {
            return hold;
        }
        let after_axes = apply_pan_axes(target, hold, self.pan_axes.get());
        clamp_pan(
            after_axes,
            self.scene_pan_bounds.get(),
            self.view_pan_bounds.get(),
            self.viewport.get(),
            self.zoom.get(),
        )
    }

    /// Bring `scene_rect.expand(margin)` into view, and report the shift in
    /// **scene** coordinates.
    ///
    /// # What the returned vector is, and why that space
    ///
    /// It is the amount the target appears to move *relative to the viewport*,
    /// stated in the space the caller asked in. That is exactly what
    /// [`teksilo_core::event::WidgetEvent::ScrollIntoView`]'s
    /// `applied_scroll` back-channel is defined to carry: the reveal walk
    /// subtracts it from the rectangle before projecting that rectangle out to
    /// the next container, using the transform it read **before** dispatching.
    /// Reporting a screen-space pan delta there would be wrong by a factor of
    /// the zoom and, under rotation, by an angle.
    ///
    /// It is computed from the transform either side of the move rather than
    /// derived by hand, so it stays exact if the composition ever changes:
    /// `delta = rect.origin − T_before⁻¹(T_after(rect.origin))`, which is the
    /// unique vector satisfying "the old transform applied to the shifted rect
    /// is where the new transform puts the unshifted one".
    ///
    /// `Vec2::ZERO` when nothing moved — the target already fits, the pan-axes
    /// policy forbids it, a clamp ate it, or the viewport has no area yet.
    pub fn reveal(
        &self,
        scene_rect: Rect,
        margin: f32,
        align: ScrollAlign,
        motion: ScrollMotion,
    ) -> Vec2 {
        let viewport = self.viewport.get();
        if viewport.width <= 0.0 || viewport.height <= 0.0 {
            return Vec2::ZERO;
        }
        let before = self.view_transform();
        let Some(inv) = before.inverse() else {
            return Vec2::ZERO;
        };
        // Visible scene region under the *current* view transform. Zoom is not
        // touched: the correction is a translation in scene space.
        let bo = self.bounds_origin.get();
        let visible = inv.apply_rect(Rect::new(bo.x, bo.y, viewport.width, viewport.height));
        let target = scene_rect.expand(margin);

        // Per-axis. Horizontal is always minimal — a vertical fraction has no
        // horizontal meaning, and yanking a horizontally-scrolled canvas
        // sideways to honour one is the behaviour `ScrollAlign` documents
        // avoiding.
        let mut dx = 0.0;
        if target.x < visible.x {
            dx = target.x - visible.x;
        } else if target.x + target.width > visible.x + visible.width {
            dx = (target.x + target.width) - (visible.x + visible.width);
        }
        let dy = match align {
            // Pin: put the target's top `f` of the way down the viewport,
            // whether or not it is already visible. Unconditional by
            // definition — a typewriter caret that only moved the camera once
            // it fell off the edge would not be pinned to anything.
            ScrollAlign::Fraction(f) => {
                let f = f.clamp(0.0, 1.0);
                let wanted = visible.y + (visible.height - target.height) * f;
                target.y - wanted
            }
            ScrollAlign::Minimal => {
                if target.y < visible.y {
                    target.y - visible.y
                } else if target.y + target.height > visible.y + visible.height {
                    (target.y + target.height) - (visible.y + visible.height)
                } else {
                    0.0
                }
            }
        };
        if dx == 0.0 && dy == 0.0 {
            return Vec2::ZERO;
        }

        // ∆scene > 0 means "show more of what is further along this axis",
        // which is a *negative* screen-space pan: pan translates the scene at
        // paint time, so revealing a region further right shifts the scene
        // leftward.
        let zoom = self.zoom.get();
        let pan = Vec2::new(self.pan_x.get(), self.pan_y.get());
        let wanted = Vec2::new(pan.x - dx * zoom, pan.y - dy * zoom);
        let gated = self.gate_pan_target(wanted);
        if gated == pan {
            return Vec2::ZERO;
        }
        match motion {
            ScrollMotion::Instant => {
                self.pan_x.set(gated.x);
                self.pan_y.set(gated.y);
            }
            // `try_`, and a snap when it is refused. A reveal is the one camera
            // move an app never asked for — it arrives because a descendant
            // moved its caret — so it must not be able to bring the app down
            // over how the app chose to declare its own pan signals. Passing
            // plain `Signal::new` handles to
            // [`view_state`](SceneView::view_state) is supported and
            // documented; before this route existed the only thing that
            // animated them was an explicit `ensure_visible` call, which is the
            // app's own doing.
            ScrollMotion::Smooth => {
                let x = self
                    .pan_x
                    .try_animate_to(gated.x, self.anim_duration, Easing::EaseOut);
                let y = self
                    .pan_y
                    .try_animate_to(gated.y, self.anim_duration, Easing::EaseOut);
                if x.is_err() || y.is_err() {
                    self.pan_x.set(gated.x);
                    self.pan_y.set(gated.y);
                }
            }
        }

        // Read the applied shift back out of the two transforms rather than
        // returning the requested `(dx, dy)`: the gate above may have taken
        // some or all of it, and an outer scroll container re-targeted by a
        // delta that never happened scrolls to the wrong place.
        let after = compose_view(
            Vec2::new(gated.x + bo.x, gated.y + bo.y),
            zoom,
            self.rotation.get(),
        );
        let origin = Point::new(scene_rect.x, scene_rect.y);
        let landed = inv.apply_point(after.apply_point(origin));
        Vec2::new(origin.x - landed.x, origin.y - landed.y)
    }
}

impl SceneView {
    /// Read access to the underlying scene, as a borrow guard.
    ///
    /// Prefer the cloneable [`model`](Self::model) handle for multi-view
    /// wiring and scene mutation (its methods are `&self`); this guard is the
    /// single-view escape hatch for ad-hoc reads.
    pub fn scene(&self) -> std::cell::Ref<'_, Scene> {
        self.model.0.borrow()
    }

    /// Mutable access to the underlying scene, as a borrow guard.
    ///
    /// Single-view escape hatch. For multi-view, mutate through the shared
    /// [`SceneModel`] handle ([`model`](Self::model)) — every
    /// mutator is `&self`, so a handler holding a clone can drive the scene
    /// directly (no `with_widget_mut` needed) and **all** views reconcile:
    ///
    /// ```
    /// # use teksilo_scene::{Scene, SceneView};
    /// # use teksilo_canvas::Rect;
    /// # let view = SceneView::new(Scene::new());
    /// # let card_data = "example payload";
    /// # let rect = Rect::new(0.0, 0.0, 200.0, 120.0);
    /// let model = view.model();          // cheap handle clone
    /// model.add_widget_item(card_data, rect);   // every view rebuilds it
    /// ```
    ///
    /// The view self-reconciles on every mutation: `add_widget_item` /
    /// `add_item` materialise on the next rebuild, `remove` destroys the
    /// orphaned arena widget and cleans its maps, `set_payload` rebuilds an
    /// item's widget, and **both** the visual tree and the *separate* AccessKit
    /// tree re-walk (geometry, reparents, and pure-a11y mutations all reach
    /// assistive tech — `build()` requests an AT re-walk, since a relayout no
    /// longer does so on its own).
    pub fn scene_mut(&mut self) -> std::cell::RefMut<'_, Scene> {
        self.model.0.borrow_mut()
    }

    /// The `WidgetId` an item was materialised as, if known.
    pub fn widget_id_for(&self, id: ItemId) -> Option<WidgetId> {
        self.materialized.get(&id).copied()
    }

    /// Current pan offset (logical pixels).
    pub fn pan(&self) -> Vec2 {
        Vec2::new(self.pan_x.get(), self.pan_y.get())
    }

    /// Current zoom factor.
    pub fn zoom(&self) -> f32 {
        self.zoom.get()
    }

    /// Current rotation in radians.
    pub fn rotation(&self) -> f32 {
        self.rotation.get()
    }

    /// In-flight animation target for the X pan signal, or `None`
    /// if the signal is at rest. Useful for tests that want to
    /// observe a tween before it lands without spinning the
    /// scheduler.
    pub fn pan_x_animation_target(&self) -> Option<f32> {
        self.pan_x.animation_target()
    }

    /// In-flight animation target for the Y pan signal.
    pub fn pan_y_animation_target(&self) -> Option<f32> {
        self.pan_y.animation_target()
    }

    /// In-flight animation target for the zoom signal.
    pub fn zoom_animation_target(&self) -> Option<f32> {
        self.zoom.animation_target()
    }

    /// The composed view transform the render walker has on its
    /// stack while painting this view's subtree. Includes the
    /// `bounds.origin` offset captured during the last
    /// `place_children` call, so this is the exact transform
    /// applied to scene-coord points by the renderer.
    pub fn view_transform(&self) -> Transform2D {
        let pan = self.pan();
        let bo = self.bounds_origin_signal.get();
        compose_view(
            Vec2::new(pan.x + bo.x, pan.y + bo.y),
            self.zoom.get(),
            self.rotation.get(),
        )
    }

    /// The view's uniform zoom — the scale of the view transform's linear
    /// part, the same number pointer dispatch passes to
    /// [`ItemShape::contains`](crate::ItemShape::contains).
    ///
    /// It reaches exactly one thing: a **cosmetic** (device-pixel) stroke
    /// band, whose width in scene coordinates shrinks as the view zooms in.
    /// Computed as the hypotenuse of the transform's first column so a pure
    /// rotation reports `1.0`.
    pub fn view_scale(&self) -> f32 {
        let m = self.view_transform().m;
        m[0].hypot(m[1])
    }

    /// Project a point in **view space** (screen-pixel coords —
    /// the same frame pointer events arrive in) into scene
    /// coordinates. Inverse of [`map_from_scene`](Self::map_from_scene).
    /// Returns the scene origin when the view transform is
    /// degenerate (e.g. zoom = 0).
    pub fn map_to_scene(&self, view_pt: Point) -> Point {
        match self.view_transform().inverse() {
            Some(inv) => inv.apply_point(view_pt),
            None => Point::ZERO,
        }
    }

    /// Project a point in **scene coords** to view space (screen
    /// pixels). Inverse of [`map_to_scene`](Self::map_to_scene).
    pub fn map_from_scene(&self, scene_pt: Point) -> Point {
        self.view_transform().apply_point(scene_pt)
    }

    /// Project a rectangle in view space into scene coordinates.
    /// Returns the AABB of the four projected corners under
    /// rotation. Empty rect when the view transform is degenerate.
    pub fn map_rect_to_scene(&self, view_rect: Rect) -> Rect {
        match self.view_transform().inverse() {
            Some(inv) => inv.apply_rect(view_rect),
            None => Rect::ZERO,
        }
    }

    /// Project a rectangle in scene coords into view space.
    pub fn map_rect_from_scene(&self, scene_rect: Rect) -> Rect {
        self.view_transform().apply_rect(scene_rect)
    }

    /// Reactive signal of the **visible scene region** — the
    /// portion of scene space currently inside the SceneView's
    /// viewport. Fires whenever pan / zoom / rotation /
    /// bounds_origin / viewport-size changes.
    ///
    /// Use to drive a minimap viewport indicator, lazy-load only
    /// the visible scene region, or implement "scroll into view"
    /// guards. The value is the AABB of the viewport rectangle
    /// projected through `view_transform.inverse()`.
    pub fn viewport_in_scene_signal(&self) -> Signal<Rect> {
        let xform_sig = self.view_transform_signal.clone();
        let vp_sig = self.last_viewport.clone();
        let bo_sig = self.bounds_origin_signal.clone();
        xform_sig
            .zip(&vp_sig)
            .zip(&bo_sig)
            .map_coalesced(|((xform, vp), bo)| {
                let screen_rect = Rect::new(bo.x, bo.y, vp.width, vp.height);
                match xform.inverse() {
                    Some(inv) => inv.apply_rect(screen_rect),
                    None => Rect::ZERO,
                }
            })
    }

    /// Reactive signal of the SceneView's resolved viewport size.
    /// Fires whenever `layout_response` resolves a new size that
    /// differs from the previous.
    pub fn viewport_size_signal(&self) -> Signal<Size> {
        self.last_viewport.clone()
    }

    /// Animate pan to `target` over `duration`. Bounded by
    /// `Easing::EaseOut`. Honours `prefers-reduced-motion` only
    /// indirectly: the scheduler pauses animation on window-inactive
    /// and the test seam allows snapping. For an explicit snap, call
    /// [`SceneView::set_pan`].
    pub fn pan_to(&self, target: Vec2, duration: Duration) {
        let target = self.gate_pan_target(target);
        self.pan_x.animate_to(target.x, duration, Easing::EaseOut);
        self.pan_y.animate_to(target.y, duration, Easing::EaseOut);
    }

    /// Snap pan to `target` without animation. Gated by the scene's
    /// [`PanAxes`](crate::scene::PanAxes) policy.
    pub fn set_pan(&self, target: Vec2) {
        let target = self.gate_pan_target(target);
        self.pan_x.set(target.x);
        self.pan_y.set(target.y);
    }

    /// Animate zoom to `target` over `duration`, clamped to
    /// `[min_zoom, max_zoom]`. No-op when the scene declares
    /// [`Scene::zoomable(false)`](crate::Scene::zoomable).
    pub fn zoom_to(&self, target: f32, duration: Duration) {
        if !self.scene().is_zoomable() || self.adopt_scene_size {
            return;
        }
        let clamped = self.gate_zoom_target(target);
        self.zoom.animate_to(clamped, duration, Easing::EaseOut);
    }

    /// Snap zoom to `target` without animation, clamped. No-op when
    /// the scene declares zoom disabled.
    pub fn set_zoom(&self, target: f32) {
        if !self.scene().is_zoomable() || self.adopt_scene_size {
            return;
        }
        let clamped = self.gate_zoom_target(target);
        self.zoom.set(clamped);
    }

    /// The camera as a cloneable handle — see [`Camera`].
    pub(super) fn camera(&self) -> Camera {
        Camera {
            pan_x: self.pan_x.clone(),
            pan_y: self.pan_y.clone(),
            zoom: self.zoom.clone(),
            rotation: self.rotation.clone(),
            bounds_origin: self.bounds_origin_signal.clone(),
            viewport: self.last_viewport.clone(),
            pan_axes: self.scene().pan_axes_signal(),
            scene_pan_bounds: self.scene().pan_bounds_signal(),
            view_pan_bounds: self.pan_bounds_override.clone(),
            adopt_scene_size: self.adopt_scene_size,
            anim_duration: self.pan_anim_duration,
        }
    }

    /// Pan (without changing zoom) so `scene_rect.expand(margin)`
    /// fits inside the current visible scene region. If the
    /// expanded target rect already fits, this is a no-op.
    ///
    /// Animated, over the view's pan-animation duration. For an instant jump,
    /// or to pin the target at a fraction of the viewport rather than merely
    /// reveal it, use
    /// [`ensure_visible_aligned`](Self::ensure_visible_aligned).
    ///
    /// # Who calls this
    ///
    /// An app scrolling to a search hit or a newly created item calls it
    /// directly. The framework calls it for every
    /// [`teksilo_core::event::WidgetEvent::ScrollIntoView`]
    /// that reaches the view — which is how a caret inside an embedded editor
    /// moves the camera, since a `SceneView` clips its children and is
    /// therefore on the reveal walk. (It is **not** called by focus traversal:
    /// this doc used to claim it was, and nothing in the crate ever did.)
    ///
    /// Pan is gated by [`Scene::pan_axes`](crate::Scene::pan_axes):
    /// if a scene declares `PanAxes::None`, this is a no-op; if it
    /// declares a single axis, only that axis pans. Items can't be
    /// scrolled into view if the policy doesn't permit panning
    /// toward them.
    pub fn ensure_visible(&self, scene_rect: Rect, margin: f32) {
        self.ensure_visible_aligned(
            scene_rect,
            margin,
            ScrollAlign::Minimal,
            ScrollMotion::Smooth,
        );
    }

    /// [`ensure_visible`](Self::ensure_visible) with an explicit alignment and
    /// motion, reporting the shift it applied in **scene** coordinates.
    ///
    /// * [`ScrollAlign::Minimal`] reveals the target and does nothing when it
    ///   is already visible; [`ScrollAlign::Fraction`] *pins* its top `f` of
    ///   the way down the viewport whether or not it was visible.
    /// * [`ScrollMotion::Instant`] snaps. That is what a caret chase wants:
    ///   animating a reveal that re-fires on every keystroke is what produces
    ///   the screen-bouncing typewriter modes are complained about for.
    ///
    /// The returned vector is `Vec2::ZERO` when nothing moved, and is what the
    /// [`ScrollIntoView`](teksilo_core::event::WidgetEvent::ScrollIntoView)
    /// `applied_scroll` back-channel carries: the shift the target appears to
    /// make **relative to the viewport**, stated in the space the caller asked
    /// in. The reveal walk subtracts it from the rectangle before projecting
    /// that rectangle out to the next container, using the transform it read
    /// *before* dispatching — so a screen-space pan delta there would be wrong
    /// by a factor of the zoom, and under rotation by an angle.
    pub fn ensure_visible_aligned(
        &self,
        scene_rect: Rect,
        margin: f32,
        align: ScrollAlign,
        motion: ScrollMotion,
    ) -> Vec2 {
        self.camera().reveal(scene_rect, margin, align, motion)
    }

    /// Project `target` through the scene's pan-axes policy AND
    /// clamp to the effective pan-bounds (intersection of Scene-
    /// declared pan_bounds and view-level pan_bounds_override —
    /// tightening-only). The orthogonal axis is held at its current
    /// value when the policy excludes it; `PanAxes::None` (and
    /// `adopt_scene_size`) holds both axes at their current pan.
    fn gate_pan_target(&self, target: Vec2) -> Vec2 {
        let hold = Vec2::new(self.pan_x.get(), self.pan_y.get());
        if self.adopt_scene_size {
            return hold;
        }
        let after_axes = apply_pan_axes(target, hold, self.scene().current_pan_axes());
        clamp_pan(
            after_axes,
            self.scene().current_pan_bounds(),
            self.pan_bounds_override.get(),
            self.last_viewport.get(),
            self.zoom.get(),
        )
    }

    /// Clamp `zoom` to the effective zoom range (intersection of
    /// Scene-declared `zoom_range` and view-level `zoom_range_override`).
    /// `None` on either side means "no constraint from that side";
    /// when both are `None` this is the identity. Tightening-only —
    /// neither side can loosen what the other imposes.
    pub(super) fn gate_zoom_target(&self, zoom: f32) -> f32 {
        clamp_zoom(zoom, self.effective_zoom_range().as_ref())
    }

    /// Effective zoom range = intersect(Scene declared, view override).
    fn effective_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>> {
        intersect_zoom_range(
            self.scene().current_zoom_range().as_ref(),
            self.zoom_range_override.get().as_ref(),
        )
    }

    /// Animate rotation to `target` over `duration` (radians).
    pub fn rotate_to(&self, target_radians: f32, duration: Duration) {
        self.rotation
            .animate_to(target_radians, duration, Easing::EaseOut);
    }

    /// Snap rotation to `target` without animation.
    pub fn set_rotation(&self, target_radians: f32) {
        self.rotation.set(target_radians);
    }

    /// Snapshot the current pan / zoom / rotation as a
    /// [`SceneViewState`](crate::SceneViewState). Designed for
    /// persistence: store the snapshot in your settings layer on
    /// app exit, restore it via [`restore_state`](Self::restore_state)
    /// on next launch.
    ///
    /// The snapshot reflects the *current* signal values — if a
    /// pan/zoom animation is in flight, the captured values are
    /// the in-flight tween position, not the eventual target.
    /// Apps that want to capture the target should query
    /// [`pan_x_animation_target`](Self::pan_x_animation_target) /
    /// friends manually.
    pub fn state(&self) -> crate::SceneViewState {
        crate::SceneViewState {
            pan_x: self.pan_x.get(),
            pan_y: self.pan_y.get(),
            zoom: self.zoom.get(),
            rotation: self.rotation.get(),
        }
    }

    /// Restore a previously captured [`SceneViewState`](crate::SceneViewState).
    /// Snaps each signal to the saved value (no animation —
    /// pan/zoom/rotation jump to the persisted state immediately).
    /// Zoom is clamped to `[min_zoom, max_zoom]`.
    pub fn restore_state(&self, state: crate::SceneViewState) {
        self.pan_x.set(state.pan_x);
        self.pan_y.set(state.pan_y);
        self.zoom.set(self.gate_zoom_target(state.zoom));
        self.rotation.set(state.rotation);
    }

    /// Latest viewport size observed during layout. Useful for
    /// imperative `fit_*` calls.
    pub fn viewport_size(&self) -> Size {
        self.last_viewport.get()
    }

    /// Compute the bounding rectangle (in scene coords) that encloses
    /// every item in the scene. Returns `None` for an empty scene.
    pub fn scene_content_bounds(&self) -> Option<Rect> {
        let ids: Vec<ItemId> = self.scene().ids();
        union_rects(ids.iter().filter_map(|id| self.scene().scene_rect(*id)))
    }

    /// Animate pan + zoom so the scene's content bounding box fits
    /// the current viewport with a small margin. No-op for an empty
    /// scene. Resets rotation to 0.
    pub fn fit_to_content(&self) {
        if let Some(content) = self.scene_content_bounds() {
            self.fit_to_rect(content);
        }
    }

    /// Animate pan + zoom so the union of the given items' bounds
    /// fits the current viewport. Ids not currently in the scene
    /// are skipped silently. No-op if `ids` is empty or all ids are
    /// stale. Resets rotation to 0.
    ///
    /// Use this for "zoom to selection" / "frame this subset" UX.
    pub fn fit_to_items(&self, ids: &[ItemId]) {
        let union = union_rects(ids.iter().filter_map(|id| self.scene().scene_rect(*id)));
        if let Some(rect) = union {
            self.fit_to_rect(rect);
        }
    }

    /// Animate pan + zoom so the bounds of the currently selected
    /// items fit the viewport. No-op when nothing is selected.
    /// Convenience for the common "F to focus selection" hotkey.
    pub fn fit_to_selection(&self) {
        let ids = self.selection.selected();
        if !ids.is_empty() {
            self.fit_to_items(&ids);
        }
    }

    /// Internal: shared math for `fit_to_content` /
    /// `fit_to_items` / `fit_to_selection`. Animates pan + zoom so
    /// `rect` fits the current viewport with a margin, and resets
    /// rotation to 0.
    fn fit_to_rect(&self, rect: Rect) {
        let viewport = self.last_viewport.get();
        let margin = 24.0;
        let avail_w = (viewport.width - margin * 2.0).max(1.0);
        let avail_h = (viewport.height - margin * 2.0).max(1.0);
        let raw_scale = (avail_w / rect.width.max(1.0)).min(avail_h / rect.height.max(1.0));
        let scale = self.gate_zoom_target(raw_scale);
        let center = rect.center();
        let pan = Vec2::new(
            viewport.width * 0.5 - scale * center.x,
            viewport.height * 0.5 - scale * center.y,
        );
        self.zoom_to(scale, self.zoom_anim_duration);
        self.rotate_to(0.0, self.zoom_anim_duration);
        self.pan_to(pan, self.zoom_anim_duration);
    }
}
