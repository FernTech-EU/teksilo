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

    /// Pan (without changing zoom) so `scene_rect.expand(margin)`
    /// fits inside the current visible scene region. If the
    /// expanded target rect already fits, this is a no-op.
    ///
    /// Pairs with focus traversal: when an off-viewport item gains
    /// focus, the SceneView's default focus traversal calls this
    /// automatically. Apps wanting to scroll a specific area into
    /// view (e.g. on search-result selection) call it directly.
    ///
    /// Pan is gated by [`Scene::pan_axes`](crate::Scene::pan_axes):
    /// if a scene declares `PanAxes::None`, this is a no-op; if it
    /// declares a single axis, only that axis pans. Items can't be
    /// scrolled into view if the policy doesn't permit panning
    /// toward them.
    pub fn ensure_visible(&self, scene_rect: Rect, margin: f32) {
        let viewport = self.last_viewport.get();
        if viewport.width <= 0.0 || viewport.height <= 0.0 {
            return;
        }
        // Visible scene region under the *current* view transform.
        // We don't change zoom — the per-axis correction is purely
        // a translation in scene space, projected back through the
        // current zoom (∆pan_screen = ∆target_scene * zoom).
        let view_xform = self.view_transform();
        let bo = self.bounds_origin_signal.get();
        let viewport_screen = Rect::new(bo.x, bo.y, viewport.width, viewport.height);
        let visible = match view_xform.inverse() {
            Some(inv) => inv.apply_rect(viewport_screen),
            None => return,
        };
        let target = scene_rect.expand(margin);

        // Per-axis: shift only when the target lies outside the
        // visible region. ∆scene > 0 means "scroll the world right",
        // which translates to ∆pan_screen = -∆scene * zoom (pan is a
        // translation applied to *the scene* at paint time, so to
        // reveal a region further right we shift the scene leftward).
        let zoom = self.zoom.get();
        let mut dx = 0.0;
        let mut dy = 0.0;
        if target.x < visible.x {
            dx = target.x - visible.x;
        } else if target.x + target.width > visible.x + visible.width {
            dx = (target.x + target.width) - (visible.x + visible.width);
        }
        if target.y < visible.y {
            dy = target.y - visible.y;
        } else if target.y + target.height > visible.y + visible.height {
            dy = (target.y + target.height) - (visible.y + visible.height);
        }
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let pan = self.pan();
        let new_pan = Vec2::new(pan.x - dx * zoom, pan.y - dy * zoom);
        // Animate the scroll instead of snapping — matches
        // `pan_to`, `fit_to_rect`, and the surrounding gesture-driven
        // animations. Reduced-motion handling is a follow-up
        // (this call goes through `Signal::animate_to`, which is
        // unconditional; `prefers-reduced-motion` consultation
        // lives at the higher-level `ctx.animate()` builder).
        self.pan_to(new_pan, self.pan_anim_duration);
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
