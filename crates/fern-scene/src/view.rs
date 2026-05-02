//! [`SceneView`] — the viewport widget that hosts a [`Scene`] and
//! places its items at scene coordinates.
//!
//! ## Phase 1 + Phase 2
//!
//! - **Phase 1.** Free positioning. `place_children` plants each
//!   materialised heavyweight widget at its `scene_rect.origin` /
//!   `size` in parent-local coords. No view transform yet.
//! - **Phase 2.** Pan / zoom / rotation as four animated `Signal<f32>`s
//!   on `SceneView`, composed into a derived `Signal<Transform2D>`
//!   bound via `BuildContext::set_transform` on the view itself. The
//!   render walker pushes that scope around the entire subtree, so
//!   every materialised widget is visually transformed; per-Phase-0
//!   transform-aware hit-test then routes pointer events through the
//!   same scope, and per-node `paint_epoch` gating means the framework's
//!   four idle gates apply for free (pan/zoom that's reached its
//!   terminal tick stops scheduling frames).
//!
//! Phase 2 also wires:
//!
//! - **`on_scroll`** — trackpad two-finger pan (`ScrollDelta::Pixels`)
//!   and mouse wheel (`ScrollDelta::Lines`) both animate the pan
//!   signals via `Easing::EaseOut`. Trackpad momentum events from
//!   winit arrive as further `Pixels` deltas; the existing animation
//!   pipeline turns this into smooth inertial fling without a
//!   custom recognizer.
//! - **`on_pinch`** — OS trackpad pinch (`PinchPhase::Changed`) feeds
//!   `scale` into the zoom signal and `rotation` into the rotation
//!   signal, anchored around the gesture center so the scene point
//!   under the user's fingers stays put.
//! - **Reduced-motion** — at build time, captures
//!   `BuildContext::prefers_reduced_motion()`. When set, scroll
//!   handlers `set` the signals directly instead of `animate_to`-ing
//!   them; pinch is already instantaneous.
//!
//! Future phases:
//!
//! - Phase 3 culls items using the spatial index in `place_children`.
//! - Phase 6 adds drag-to-move on item bodies and marquee selection
//!   on the empty viewport surface.
//! - Phase 5 layers a11y on top.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use fern_canvas::{Point, Rect, Size, SizeProposal, Transform2D, Vec2};
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use fern_core::gesture::PinchPhase;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Easing;

use crate::item::ItemId;
use crate::scene::Scene;
use crate::transform::{anchor_pan_for_pinch, compose_view};

/// Logical pixels of pan applied per `ScrollDelta::Lines` notch.
/// Mirrors the convention used by `ScrollArea` (`line_height` ≈ 16 in
/// fern-widgets).
const DEFAULT_LINE_HEIGHT: f32 = 16.0;
const DEFAULT_PAN_DURATION: Duration = Duration::from_millis(120);
const DEFAULT_ZOOM_DURATION: Duration = Duration::from_millis(180);
const DEFAULT_MIN_ZOOM: f32 = 0.1;
const DEFAULT_MAX_ZOOM: f32 = 10.0;

/// A pannable/zoomable viewport hosting a [`Scene`]'s items at scene
/// coordinates.
#[derive(Debug)]
pub struct SceneView {
    scene: Scene,
    /// Materialisation map populated during `build`. Stable across
    /// rebuilds — subsequent `build` calls just return the cached
    /// widget ids.
    materialized: HashMap<ItemId, WidgetId>,
    /// Reverse lookup populated alongside `materialized` so the
    /// per-frame `place_children` cull resolves
    /// `WidgetId → ItemId` in `O(1)`. Without it, scaling the demo
    /// to 5,000 cards would burn a full frame's budget on the
    /// per-child entry scan.
    widget_to_item: HashMap<WidgetId, ItemId>,
    /// Live mirror of `bounds.origin` (the SceneView's screen-space
    /// position as decided by its parent layout). Updated in
    /// `place_children` and folded into the view-transform composition
    /// so a SceneView positioned at a non-zero parent offset still
    /// places its children correctly under pan / zoom / rotation.
    /// Without this, zoom would multiply `bounds.origin` and the
    /// content would visually drift away from the viewport.
    bounds_origin_signal: Signal<Vec2>,
    /// Fallback size when the parent's `SizeProposal` is unspecified
    /// on either axis.
    default_size: Size,
    /// Latest viewport size observed during layout. Cached so
    /// imperative methods like [`SceneView::fit_to_content`] can
    /// reason about the visible rectangle without re-running layout.
    last_viewport: Cell<Size>,

    // --- Phase 2 view transform state ---------------------------------
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    rotation: Signal<f32>,

    // --- Phase 2 configuration ----------------------------------------
    min_zoom: f32,
    max_zoom: f32,
    pan_anim_duration: Duration,
    zoom_anim_duration: Duration,
    line_height: f32,
}

impl SceneView {
    /// Wrap a [`Scene`] in a viewport. The scene is moved into the
    /// view; query / mutate it later via [`SceneView::scene_mut`].
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            materialized: HashMap::new(),
            widget_to_item: HashMap::new(),
            default_size: Size::new(800.0, 600.0),
            last_viewport: Cell::new(Size::new(800.0, 600.0)),
            pan_x: Signal::new_animated(0.0),
            pan_y: Signal::new_animated(0.0),
            zoom: Signal::new_animated(1.0),
            rotation: Signal::new_animated(0.0),
            bounds_origin_signal: Signal::new(Vec2::ZERO),
            min_zoom: DEFAULT_MIN_ZOOM,
            max_zoom: DEFAULT_MAX_ZOOM,
            pan_anim_duration: DEFAULT_PAN_DURATION,
            zoom_anim_duration: DEFAULT_ZOOM_DURATION,
            line_height: DEFAULT_LINE_HEIGHT,
        }
    }

    /// Override the size used when the parent doesn't propose one on
    /// an axis. Defaults to 800×600 logical pixels.
    pub fn default_size(mut self, w: f32, h: f32) -> Self {
        self.default_size = Size::new(w, h);
        self.last_viewport.set(self.default_size);
        self
    }

    /// Minimum zoom factor (default 0.1×). Applied as a clamp to all
    /// programmatic and gesture-driven zoom changes.
    pub fn min_zoom(mut self, v: f32) -> Self {
        self.min_zoom = v.max(0.0001);
        self
    }

    /// Maximum zoom factor (default 10×). Applied as a clamp to all
    /// programmatic and gesture-driven zoom changes.
    pub fn max_zoom(mut self, v: f32) -> Self {
        self.max_zoom = v.max(self.min_zoom);
        self
    }

    /// Logical pixels of pan applied per scroll-wheel line notch.
    /// Defaults to 16 px (matches `ScrollArea`).
    pub fn line_height(mut self, px: f32) -> Self {
        self.line_height = px.max(0.0);
        self
    }

    /// Read access to the underlying scene model.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Mutable access to the underlying scene model. Intended for
    /// pre-build configuration (Phase 1) or future runtime mutation
    /// (Phase 6); after `SceneView` has been added to the tree, fresh
    /// `add_widget` calls take effect on the next rebuild.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
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

    /// Animate pan to `target` over `duration`. Bounded by
    /// `Easing::EaseOut`. Honours `prefers-reduced-motion` only
    /// indirectly: the scheduler pauses animation on window-inactive
    /// and the test seam allows snapping. For an explicit snap, call
    /// [`SceneView::set_pan`].
    pub fn pan_to(&self, target: Vec2, duration: Duration) {
        self.pan_x.animate_to(target.x, duration, Easing::EaseOut);
        self.pan_y.animate_to(target.y, duration, Easing::EaseOut);
    }

    /// Snap pan to `target` without animation.
    pub fn set_pan(&self, target: Vec2) {
        self.pan_x.set(target.x);
        self.pan_y.set(target.y);
    }

    /// Animate zoom to `target` over `duration`, clamped to
    /// `[min_zoom, max_zoom]`.
    pub fn zoom_to(&self, target: f32, duration: Duration) {
        let clamped = target.clamp(self.min_zoom, self.max_zoom);
        self.zoom.animate_to(clamped, duration, Easing::EaseOut);
    }

    /// Snap zoom to `target` without animation, clamped.
    pub fn set_zoom(&self, target: f32) {
        let clamped = target.clamp(self.min_zoom, self.max_zoom);
        self.zoom.set(clamped);
    }

    /// Animate rotation to `target` over `duration` (radians).
    pub fn rotate_to(&self, target_radians: f32, duration: Duration) {
        self.rotation
            .animate_to(target_radians, duration, Easing::EaseOut);
    }

    /// Latest viewport size observed during layout. Useful for
    /// imperative `fit_*` calls.
    pub fn viewport_size(&self) -> Size {
        self.last_viewport.get()
    }

    /// Compute the bounding rectangle (in scene coords) that encloses
    /// every item in the scene. Returns `None` for an empty scene.
    pub fn scene_content_bounds(&self) -> Option<Rect> {
        union_rects(self.scene.entries.iter().map(|e| e.scene_rect))
    }

    /// Animate pan + zoom so the scene's content bounding box fits
    /// the current viewport with a small margin. No-op for an empty
    /// scene. Resets rotation to 0.
    pub fn fit_to_content(&self) {
        let Some(content) = self.scene_content_bounds() else {
            return;
        };
        let viewport = self.last_viewport.get();
        let margin = 24.0;
        let avail_w = (viewport.width - margin * 2.0).max(1.0);
        let avail_h = (viewport.height - margin * 2.0).max(1.0);
        let scale = (avail_w / content.width.max(1.0))
            .min(avail_h / content.height.max(1.0))
            .clamp(self.min_zoom, self.max_zoom);
        // Center the content's center on the viewport's center.
        let content_center = content.center();
        let pan = Vec2::new(
            viewport.width * 0.5 - scale * content_center.x,
            viewport.height * 0.5 - scale * content_center.y,
        );
        self.zoom_to(scale, self.zoom_anim_duration);
        self.rotate_to(0.0, self.zoom_anim_duration);
        self.pan_to(pan, self.zoom_anim_duration);
    }
}

impl Widget for SceneView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Materialise pending widgets (drained the first time, idempotent
        // afterwards). Phase 3 also keeps the reverse lookup
        // `widget_to_item` in sync so place_children's cull is `O(1)`
        // per child instead of scanning the entries vec.
        let mut child_ids = Vec::with_capacity(self.scene.entries.len());
        for entry in self.scene.entries.iter_mut() {
            match &mut entry.kind {
                crate::scene::SceneEntryKind::Widget { pending } => {
                    if let Some(widget) = pending.take() {
                        let wid = ctx.add_boxed(widget);
                        self.materialized.insert(entry.id, wid);
                        self.widget_to_item.insert(wid, entry.id);
                        child_ids.push(wid);
                    } else if let Some(wid) = self.materialized.get(&entry.id).copied() {
                        child_ids.push(wid);
                    }
                }
                crate::scene::SceneEntryKind::Item(_) => {
                    // Lightweight items don't go in the arena. They're
                    // painted from `SceneView::paint` directly.
                }
            }
        }

        // Register the four animated signals with the scheduler so
        // they participate in idle gating (paint-epoch visibility,
        // window-inactive pause, drop-cancel). Idempotent — a re-build
        // updates the owner registration in place.
        ctx.register_animated_signal(&self.pan_x);
        ctx.register_animated_signal(&self.pan_y);
        ctx.register_animated_signal(&self.zoom);
        ctx.register_animated_signal(&self.rotation);

        // Phase 3: bind the four signals at Relayout on this node so
        // `place_children` re-runs and the viewport-cull set is
        // recomputed when pan/zoom/rotation change. The Repaint
        // binding from `set_transform` below is kept in addition;
        // it's what dirties the renderer's transform stack so
        // already-laid-out children re-paint at their new visual
        // positions.  Without this Relayout binding, a `pan` or
        // `zoom` change would only repaint the *currently visible*
        // children — items the cull collapsed to zero would stay
        // collapsed even if the new view brings them into view.
        let registry = ctx.binding_registry();
        let self_id_for_relayout = ctx.self_id();
        self.pan_x
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        self.pan_y
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        self.zoom
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);
        self.rotation
            .bind_to(self_id_for_relayout, registry, BindingLevel::Relayout);

        // Derive the view transform as a single Signal<Transform2D>
        // and bind it as a `set_transform` scope on this widget. The
        // render walker pushes this around our entire subtree.
        //
        // The composition folds `bounds.origin` into the final
        // translate so a SceneView placed at a non-zero parent
        // offset still maps scene-coord (sx, sy) to screen
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        // Without folding `bounds.origin` in, zoom would multiply
        // the parent offset and the content would drift away from
        // the viewport when zoomed.
        let pan_x = self.pan_x.clone();
        let pan_y = self.pan_y.clone();
        let zoom = self.zoom.clone();
        let rotation = self.rotation.clone();
        let bounds_origin = self.bounds_origin_signal.clone();
        let view_transform = pan_x
            .zip3(&pan_y, &zoom)
            .zip(&rotation)
            .zip(&bounds_origin)
            .map(|(((px, py, z), r), bo)| {
                compose_view(Vec2::new(*px + bo.x, *py + bo.y), *z, *r)
            });
        let self_id = ctx.self_id();
        ctx.set_transform(self_id, view_transform);

        // Wire scroll + pinch handlers. Captures are by clone so they
        // outlive the build call.
        let prefers_reduced = ctx.prefers_reduced_motion();
        let line_height = self.line_height;
        let pan_dur = self.pan_anim_duration;
        let min_zoom = self.min_zoom;
        let max_zoom = self.max_zoom;

        let mut handlers = HandlerSet::new();

        {
            let pan_x = self.pan_x.clone();
            let pan_y = self.pan_y.clone();
            handlers = handlers.on_scroll(move |event, _ctx| {
                let WidgetEvent::Scroll { delta } = event else {
                    return EventResponse::Ignored;
                };
                let (dx, dy) = match delta {
                    ScrollDelta::Pixels { x, y } => (*x, *y),
                    ScrollDelta::Lines { x, y } => (*x * line_height, *y * line_height),
                };
                // Convention: positive scroll delta on the y-axis
                // means content scrolls "up" in the viewport, which
                // is equivalent to panning the *view* down — i.e. the
                // pan offset increases. This matches `ScrollArea` and
                // the natural-scroll feel of trackpads.
                let base_x = pan_x.animation_target().unwrap_or_else(|| pan_x.get());
                let base_y = pan_y.animation_target().unwrap_or_else(|| pan_y.get());
                let target_x = base_x + dx;
                let target_y = base_y + dy;
                if prefers_reduced {
                    pan_x.set(target_x);
                    pan_y.set(target_y);
                } else {
                    pan_x.animate_to(target_x, pan_dur, Easing::EaseOut);
                    pan_y.animate_to(target_y, pan_dur, Easing::EaseOut);
                }
                EventResponse::Handled
            });
        }

        {
            let pan_x = self.pan_x.clone();
            let pan_y = self.pan_y.clone();
            let zoom = self.zoom.clone();
            let rotation = self.rotation.clone();
            let bounds_origin_for_pinch = self.bounds_origin_signal.clone();
            handlers = handlers.on_pinch(move |phase, _ctx| {
                let PinchPhase::Changed {
                    center,
                    scale,
                    rotation: rotation_delta,
                } = phase
                else {
                    return;
                };
                if !scale.is_finite() || scale <= 0.0 {
                    return;
                }
                let z_old = zoom.get();
                let r_old = rotation.get();
                let z_new = (z_old * scale).clamp(min_zoom, max_zoom);
                let r_new = r_old + rotation_delta;
                let pan_old = Vec2::new(pan_x.get(), pan_y.get());
                let bo = bounds_origin_for_pinch.get();
                let new_pan = anchor_pan_for_pinch(
                    center, pan_old, z_old, r_old, z_new, r_new, bo,
                )
                .unwrap_or(pan_old);
                // Pinch is a continuous, user-driven gesture — set
                // directly so each frame's update lands without
                // queuing a tween. Idle gates still apply: at rest
                // (pinch released, no further events), no frames are
                // requested.
                zoom.set(z_new);
                rotation.set(r_new);
                pan_x.set(new_pan.x);
                pan_y.set(new_pan.y);
            });
        }

        ctx.apply_self_handlers(handlers);

        child_ids
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let size = proposal.resolve(self.default_size.width, self.default_size.height);
        // Cache for `fit_to_content` and friends. `bounds_origin` is
        // refreshed in `place_children`, which runs whenever the
        // SceneView has at least one child — i.e. always in real
        // apps, since an empty SceneView doesn't render anything to
        // interact with.
        self.last_viewport.set(size);
        LayoutResponse::rigid(size)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Mirror the parent's choice of `bounds.origin` into a signal
        // so the derived view-transform picks it up. The signal is
        // bound at `BindingLevel::RepaintOnly` via `set_transform`,
        // so changes only trigger repaint — never relayout — which
        // keeps idle behaviour intact when the SceneView is at rest.
        let new_origin = Vec2::new(bounds.x, bounds.y);
        if self.bounds_origin_signal.get() != new_origin {
            self.bounds_origin_signal.set(new_origin);
        }

        // Place each child at its **pure scene coordinate** — not
        // offset by `bounds.origin`. The renderer's transform stack
        // composes `bounds.origin` in via the view transform's final
        // translate, so a child at scene (sx, sy) lands visually at
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        // Phase 0's transform-aware hit-test routes through the same
        // scope automatically.
        //
        // Phase 3 cull: compute the visible scene-coord region by
        // inverse-transforming the SceneView's screen-space rect,
        // then collapse the size of any child whose `scene_rect`
        // doesn't intersect it. The placement's `origin` stays at
        // its canonical scene-coord position (so focus-follow /
        // scroll-into-view see consistent coordinates whether or not
        // the child is visible); only `size` flips to zero, which
        // short-circuits the recursive layout walk under that child
        // and skips its paint entirely. Heavyweight children stay
        // materialised — true demand-load is Phase 4 territory once
        // the lightweight tier is in place.
        let visible_ids = self.compute_visible_ids(bounds);
        for placement in children.iter_mut() {
            let Some(&item_id) = self.widget_to_item.get(&placement.id) else {
                continue;
            };
            let Some(rect) = self.scene.scene_rect(item_id) else {
                continue;
            };
            placement.origin = Point::new(rect.x, rect.y);
            placement.size = if visible_ids.contains(&item_id) {
                Size::new(rect.width, rect.height)
            } else {
                Size::ZERO
            };
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {
        // The SceneView's `set_transform` scope wraps both this paint
        // call and the children walk, so any `canvas.fill_*` /
        // `canvas.stroke_*` / `canvas.draw_*` call we make here lands
        // through the same view-transform projection as the heavyweight
        // children. We pass scene-coord rects directly — the renderer
        // composes pan / zoom / rotation / bounds-origin on top.
        //
        // Lightweight items paint *before* heavyweight children. The
        // render walker invokes the parent's paint first, then descends
        // into children. That's exactly what we want for the
        // background-furniture use case (connector lines, tiled grids,
        // decorations) — items render under the cards.
        let region = self.visible_scene_region(bounds);
        let view_transform = self.view_transform();
        let item_ctx = crate::item::SceneItemPaintContext {
            view_transform,
            dirty_scene_rect: Some(region),
        };
        // `items_in_rect` returns both widget and item entries that
        // intersect the visible region. We filter to the lightweight
        // tier here — heavyweights are painted by the arena walker via
        // their own widget paint methods.
        for id in self.scene.items_in_rect(region) {
            if let Some(item) = self.scene.item(id) {
                item.paint(canvas, &item_ctx);
            }
        }
    }

    fn clips_children(&self) -> bool {
        // Scene items can extend beyond the SceneView's screen bounds
        // (a connector line whose source/target are outside the
        // viewport, a tiled background grid). Without clipping, those
        // bleed past the SceneView's rectangle. Heavyweight children
        // are already culled in `place_children` via collapse-to-zero;
        // the clip is the lightweight-tier equivalent.
        true
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

impl SceneView {
    /// The scene-coord region currently inside the viewport, given
    /// the view transform's current value. Used by `place_children`
    /// to decide which items to lay out at full size and which to
    /// collapse to zero. Falls back to a degenerate-but-non-empty
    /// rect at the SceneView's screen position when the view
    /// transform is singular (zoom = 0); zero zoom collapses
    /// everything visually anyway, so the cull fallback is a
    /// safe-by-default choice.
    fn visible_scene_region(&self, bounds: Rect) -> Rect {
        // The view transform now folds in `bounds.origin`, so to find
        // the visible scene region we inverse-apply against the
        // SceneView's full screen-space rect (origin and size).
        // Works correctly for both root SceneView (`bounds.origin =
        // (0, 0)`) and nested SceneView at a non-zero parent offset.
        let viewport_screen = Rect::new(bounds.x, bounds.y, bounds.width, bounds.height);
        match self.view_transform().inverse() {
            Some(inv) => inv.apply_rect(viewport_screen),
            None => Rect::ZERO,
        }
    }

    fn compute_visible_ids(&self, bounds: Rect) -> HashSet<ItemId> {
        let region = self.visible_scene_region(bounds);
        self.scene.items_in_rect(region).into_iter().collect()
    }
}

/// Union an iterator of axis-aligned rectangles into a single
/// bounding rectangle. Returns `None` if the iterator is empty.
fn union_rects(mut rects: impl Iterator<Item = Rect>) -> Option<Rect> {
    let first = rects.next()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.right();
    let mut max_y = first.bottom();
    for r in rects {
        if r.x < min_x {
            min_x = r.x;
        }
        if r.y < min_y {
            min_y = r.y;
        }
        if r.right() > max_x {
            max_x = r.right();
        }
        if r.bottom() > max_y {
            max_y = r.bottom();
        }
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct FillWidget;

    impl FillWidget {
        fn new() -> Self {
            Self
        }
    }

    impl Widget for FillWidget {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            Size::new(0.0, 0.0).into()
        }
    }

    // -- Phase 1 placement (unchanged) -----------------------------------

    #[test]
    fn scene_view_places_widgets_at_scene_coords() {
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::new(10.0, 20.0, 100.0, 50.0));
        let b = scene.add_widget(FillWidget::new(), Rect::new(200.0, 100.0, 80.0, 80.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let kids = tree.children(view_id);
        assert_eq!(kids.len(), 2);
        assert_eq!(tree.bounds(kids[0]), Rect::new(10.0, 20.0, 100.0, 50.0));
        assert_eq!(tree.bounds(kids[1]), Rect::new(200.0, 100.0, 80.0, 80.0));

        let view = tree
            .widget_as_any(view_id)
            .and_then(|a| a.downcast_ref::<SceneView>())
            .expect("view is a SceneView");
        assert_eq!(view.widget_id_for(a), Some(kids[0]));
        assert_eq!(view.widget_id_for(b), Some(kids[1]));
    }

    #[test]
    fn scene_view_layout_takes_proposal() {
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let bounds = tree.bounds(view_id);
        assert_eq!(bounds.width, 400.0);
        assert_eq!(bounds.height, 300.0);
    }

    #[test]
    fn empty_scene_has_no_children() {
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert!(tree.children(view_id).is_empty());
    }

    #[test]
    fn scene_view_default_size_when_proposal_unspecified() {
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).default_size(640.0, 480.0));
        tree.layout(SizeProposal::unspecified());
        let bounds = tree.bounds(view_id);
        assert_eq!(bounds.width, 640.0);
        assert_eq!(bounds.height, 480.0);
    }

    // -- Phase 2 view-transform behaviour --------------------------------

    fn view_handle(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
        tree.widget_as_any(view_id)
            .and_then(|a| a.downcast_ref::<SceneView>())
            .expect("view is a SceneView")
    }

    #[test]
    fn initial_view_transform_is_identity() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);
        assert!(view.view_transform().is_identity());
        assert_eq!(view.pan(), Vec2::ZERO);
        assert_eq!(view.zoom(), 1.0);
        assert_eq!(view.rotation(), 0.0);
    }

    #[test]
    fn set_pan_and_set_zoom_update_view_transform_immediately() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let view = view_handle(&tree, view_id);
        view.set_pan(Vec2::new(100.0, 0.0));
        view.set_zoom(2.0);
        let t = view.view_transform();
        // Scene (10, 5) → scale → (20, 10) → translate → (120, 10).
        let p = t.apply_point(Point::new(10.0, 5.0));
        assert!((p.x - 120.0).abs() < 1e-5);
        assert!((p.y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn pan_to_animates_mid_flight() {
        // Animation acceptance: pan_to(target, duration) ramps from
        // start to target over `duration`. At halfway, pan_x must be
        // strictly between start and target — proving the value is
        // mid-tween rather than snapped.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        view_handle(&tree, view_id).pan_to(Vec2::new(400.0, 0.0), Duration::from_millis(200));
        // Advance halfway. The first tick processes pending requests
        // and starts the animation; the second advances the clock.
        tree.tick_animations(Duration::from_millis(100));
        let mid_x = view_handle(&tree, view_id).pan().x;
        assert!(
            mid_x > 0.0 && mid_x < 400.0,
            "pan_x should be mid-tween (got {})",
            mid_x
        );
        // Finish the animation.
        tree.tick_animations(Duration::from_millis(120));
        let end_x = view_handle(&tree, view_id).pan().x;
        assert!(
            (end_x - 400.0).abs() < 0.5,
            "pan_x should land near 400 (got {})",
            end_x
        );
    }

    #[test]
    fn idle_drain_zero_frames_at_rest() {
        // The headline non-functional test: a SceneView that's been
        // built and laid out and is not currently animating must not
        // request any further frames *of its own accord*. Note that
        // `needs_redraw()` will still be true while `needs_paint` is
        // pending — that's the framework's normal "renderer hasn't
        // painted yet" signal, cleared on the next paint pass. The
        // fern-scene-specific contract is: no animation scheduler
        // entries running, no `request_frame()` calls from us.
        let mut scene = Scene::new();
        for i in 0..5 {
            scene.add_widget(
                FillWidget::new(),
                Rect::new(i as f32 * 50.0, 0.0, 40.0, 40.0),
            );
        }
        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.tick_animations(Duration::from_millis(0));
        assert!(
            !tree.has_active_animations(),
            "no animations active at rest"
        );
        assert!(
            !tree.frame_requested(),
            "fern-scene must not call request_frame() at rest"
        );
        assert_eq!(
            tree.active_animation_count(),
            0,
            "scheduler queue must be empty at rest"
        );
    }

    #[test]
    fn idle_drain_returns_after_pan_animation_completes() {
        // Variant of the idle-drain test: trigger an animation, let
        // it finish, then assert idle.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        view_handle(&tree, view_id).pan_to(Vec2::new(200.0, 0.0), Duration::from_millis(80));
        // Push past the terminal tick (animation duration + slack).
        tree.tick_animations(Duration::from_millis(120));
        tree.tick_animations(Duration::from_millis(0));
        assert!(
            !tree.has_active_animations(),
            "animation must terminate cleanly"
        );
        // Pan should have reached its target.
        let pan = view_handle(&tree, view_id).pan();
        assert!((pan.x - 200.0).abs() < 0.5);
    }

    #[test]
    fn zoom_to_clamps_to_max_zoom() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()).max_zoom(4.0));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        view_handle(&tree, view_id).zoom_to(100.0, Duration::from_millis(50));
        tree.tick_animations(Duration::from_millis(80));
        tree.tick_animations(Duration::from_millis(0));
        assert!((view_handle(&tree, view_id).zoom() - 4.0).abs() < 0.001);
    }

    #[test]
    fn zoom_to_clamps_to_min_zoom() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()).min_zoom(0.5));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        view_handle(&tree, view_id).zoom_to(0.001, Duration::from_millis(50));
        tree.tick_animations(Duration::from_millis(80));
        tree.tick_animations(Duration::from_millis(0));
        assert!((view_handle(&tree, view_id).zoom() - 0.5).abs() < 0.001);
    }

    #[test]
    fn fit_to_content_centres_scene_in_viewport() {
        let mut scene = Scene::new();
        // Two cards at scene coords; bounding box: (0, 0, 200, 100).
        scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 100.0));
        scene.add_widget(FillWidget::new(), Rect::new(100.0, 0.0, 100.0, 100.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        view_handle(&tree, view_id).fit_to_content();
        // Drive past the terminal tick.
        tree.tick_animations(Duration::from_millis(200));
        tree.tick_animations(Duration::from_millis(0));

        let view = view_handle(&tree, view_id);
        let t = view.view_transform();
        // Content centre (100, 50) should project to viewport centre
        // (400, 300) under the resulting view transform.
        let projected = t.apply_point(Point::new(100.0, 50.0));
        assert!(
            (projected.x - 400.0).abs() < 1.0,
            "content centre x should land at viewport centre (got {})",
            projected.x
        );
        assert!(
            (projected.y - 300.0).abs() < 1.0,
            "content centre y should land at viewport centre (got {})",
            projected.y
        );
    }

    #[test]
    fn scene_content_bounds_unions_all_items() {
        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(-50.0, -20.0, 30.0, 30.0));
        scene.add_widget(FillWidget::new(), Rect::new(100.0, 50.0, 40.0, 40.0));
        let view = SceneView::new(scene);
        let b = view.scene_content_bounds().unwrap();
        assert!((b.x - -50.0).abs() < 1e-5);
        assert!((b.y - -20.0).abs() < 1e-5);
        assert!((b.right() - 140.0).abs() < 1e-5);
        assert!((b.bottom() - 90.0).abs() < 1e-5);
    }

    #[test]
    fn scene_content_bounds_empty() {
        let view = SceneView::new(Scene::new());
        assert!(view.scene_content_bounds().is_none());
    }

    // -- Phase 2 gesture wiring -----------------------------------------

    #[test]
    fn on_scroll_pixels_animates_pan() {
        // Trackpad two-finger pan delivers `ScrollDelta::Pixels`.
        // Verify the on_scroll handler routes the delta into the pan
        // signals as an `Easing::EaseOut` tween.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Move pointer into the viewport so Scroll has a target.
        tree.pointer_move(Point::new(400.0, 300.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 50.0, y: 30.0 },
        });

        // The animation has started but not finished — `animation_target`
        // should already reflect the requested delta.
        let view = view_handle(&tree, view_id);
        assert_eq!(view.pan_x.animation_target(), Some(50.0));
        assert_eq!(view.pan_y.animation_target(), Some(30.0));

        // Drive past the terminal tick.
        tree.tick_animations(Duration::from_millis(180));
        tree.tick_animations(Duration::from_millis(0));
        let view = view_handle(&tree, view_id);
        assert!((view.pan().x - 50.0).abs() < 0.5);
        assert!((view.pan().y - 30.0).abs() < 0.5);
    }

    #[test]
    fn on_scroll_lines_uses_line_height_multiplier() {
        // Mouse-wheel scrolling delivers `ScrollDelta::Lines`. Each
        // line notch translates to `line_height` logical pixels of
        // pan (default 16). With `line_height(32.0)` set, a single
        // notch should target 32 px.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()).line_height(32.0));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.pointer_move(Point::new(400.0, 300.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });

        let view = view_handle(&tree, view_id);
        assert_eq!(view.pan_y.animation_target(), Some(32.0));
    }

    #[test]
    fn on_pinch_zooms_around_gesture_center() {
        // PinchPhase::Changed scales the zoom signal and re-anchors
        // pan so the scene point under the gesture center stays put.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.pointer_move(Point::new(200.0, 100.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: fern_core::gesture::GestureEvent::PinchChanged {
                center: Point::new(200.0, 100.0),
                scale: 2.0,
                rotation: 0.0,
            },
        });

        let view = view_handle(&tree, view_id);
        // Zoom doubled — and the pinch handler `set`s synchronously
        // (no animation), so we can read the result immediately.
        assert!((view.zoom() - 2.0).abs() < 1e-3);
        // The scene point that was visible at (200, 100) before the
        // pinch should still project to (200, 100). At the start
        // pan = 0, zoom = 1, so the scene point under (200, 100) was
        // (200, 100). Under the new view (zoom 2, pan ?) it must
        // project to (200, 100) again.
        let projected = view.view_transform().apply_point(Point::new(200.0, 100.0));
        assert!(
            (projected.x - 200.0).abs() < 1e-2,
            "pinch must keep scene point under gesture center invariant (got x={})",
            projected.x
        );
        assert!((projected.y - 100.0).abs() < 1e-2);
    }

    #[test]
    fn on_pinch_clamps_zoom_to_max() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()).max_zoom(3.0));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.pointer_move(Point::new(400.0, 300.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: fern_core::gesture::GestureEvent::PinchChanged {
                center: Point::new(400.0, 300.0),
                scale: 100.0, // would zoom to 100× without clamp
                rotation: 0.0,
            },
        });
        let view = view_handle(&tree, view_id);
        assert!((view.zoom() - 3.0).abs() < 1e-3);
    }

    #[test]
    fn reduced_motion_snaps_pan_instead_of_animating() {
        // When `prefers-reduced-motion` is set on the tree before the
        // SceneView is built, on_scroll must snap pan signals
        // directly (no animation, no scheduler entry).
        let mut tree = WidgetTree::new();
        tree.set_accessibility_preferences(false, true, 1.0);
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.pointer_move(Point::new(400.0, 300.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 50.0, y: 30.0 },
        });

        let view = view_handle(&tree, view_id);
        // The signal landed at the target immediately, no tween.
        assert!((view.pan().x - 50.0).abs() < 1e-3);
        assert!((view.pan().y - 30.0).abs() < 1e-3);
        // No animation was queued.
        assert!(view.pan_x.animation_target().is_none());
        assert!(view.pan_y.animation_target().is_none());
        assert!(!tree.has_active_animations());
    }

    // -- Phase 3 viewport culling --------------------------------------

    #[test]
    fn off_screen_items_are_culled_to_zero_size() {
        // The headline Phase 3 test: a SceneView at 800×600 with one
        // item inside the viewport and one item far outside. The
        // off-screen item's bounds collapse to zero so the layout/
        // paint walks short-circuit on it.
        let mut scene = Scene::new();
        let inside = scene.add_widget(FillWidget::new(), Rect::new(50.0, 50.0, 100.0, 100.0));
        let outside = scene.add_widget(
            FillWidget::new(),
            Rect::new(5_000.0, 5_000.0, 100.0, 100.0),
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let view = view_handle(&tree, view_id);
        let inside_widget = view.widget_id_for(inside).unwrap();
        let outside_widget = view.widget_id_for(outside).unwrap();

        let inside_bounds = tree.bounds(inside_widget);
        let outside_bounds = tree.bounds(outside_widget);
        assert_eq!(inside_bounds, Rect::new(50.0, 50.0, 100.0, 100.0));
        assert_eq!(
            outside_bounds.width, 0.0,
            "off-screen item must have zero width"
        );
        assert_eq!(
            outside_bounds.height, 0.0,
            "off-screen item must have zero height"
        );
    }

    #[test]
    fn pan_brings_culled_items_back_into_view() {
        // Items outside the initial viewport collapse to zero; pan
        // the view to cover them and they should pop back to full
        // size on the next layout.
        let mut scene = Scene::new();
        let far_right =
            scene.add_widget(FillWidget::new(), Rect::new(2_000.0, 50.0, 100.0, 100.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let view = view_handle(&tree, view_id);
        let far_widget = view.widget_id_for(far_right).unwrap();
        // Before pan: far_right is outside the viewport, culled to
        // zero.
        assert_eq!(tree.bounds(far_widget).width, 0.0);

        // Pan to bring it into view: pan_x = 1900 means scene-coord
        // 2000 lands at screen 100, well within the 800-px viewport.
        // (Pan is animated; snap directly via `set_pan` so the test
        // doesn't have to drive the scheduler for this case.)
        view.set_pan(Vec2::new(-1900.0, 0.0));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let view = view_handle(&tree, view_id);
        let bounds = tree.bounds(view.widget_id_for(far_right).unwrap());
        assert_eq!(
            bounds,
            Rect::new(2_000.0, 50.0, 100.0, 100.0),
            "panned-to item should be re-inflated to its full scene_rect"
        );
    }

    #[test]
    fn cull_uses_scene_rect_origin_as_anchor_even_when_culled() {
        // Even when collapsed to zero size, the culled child's
        // origin stays at its canonical scene-rect position. This
        // means focus-follow / scroll-into-view machinery sees a
        // consistent coordinate even for off-screen items.
        let mut scene = Scene::new();
        let id = scene.add_widget(FillWidget::new(), Rect::new(10_000.0, 5_000.0, 80.0, 80.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let view = view_handle(&tree, view_id);
        let widget = view.widget_id_for(id).unwrap();
        let bounds = tree.bounds(widget);
        assert_eq!(bounds.x, 10_000.0);
        assert_eq!(bounds.y, 5_000.0);
        assert_eq!(bounds.width, 0.0);
        assert_eq!(bounds.height, 0.0);
    }

    #[test]
    fn zoom_changes_culling_set() {
        // At zoom 1, an item far from the viewport is culled.
        // Zooming way out (small zoom = wide visible region) should
        // bring it back into the visible set.
        let mut scene = Scene::new();
        let far = scene.add_widget(FillWidget::new(), Rect::new(2_000.0, 0.0, 50.0, 50.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).min_zoom(0.05));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let view = view_handle(&tree, view_id);
        let far_widget = view.widget_id_for(far).unwrap();
        assert_eq!(tree.bounds(far_widget).width, 0.0);

        // Zoom out to 0.1× — the visible scene region is 8000 px
        // wide, well past the item at x=2000.
        view.set_zoom(0.1);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);
        assert!(
            tree.bounds(view.widget_id_for(far).unwrap()).width > 0.0,
            "zooming out must un-cull off-screen items"
        );
    }

    #[test]
    fn non_root_scene_view_places_children_at_scene_coords_and_culls_correctly() {
        // SceneView nested inside a non-zero-origin parent (a Padding
        // wrapper that pushes it to (40, 40)). Verify:
        //   1. Children are placed at *pure scene_rect* in the
        //      arena (not offset by bounds.origin).
        //   2. The view transform folds in `bounds.origin` so the
        //      visual position of scene-coord (sx, sy) is
        //      (40 + zoom*sx + pan.x, 40 + zoom*sy + pan.y).
        //   3. Culling uses the screen-space SceneView rect so
        //      items in the visible scene region survive while
        //      far-off ones collapse.
        use fern_widgets::primitives::Padding;

        let mut scene = Scene::new();
        let inside = scene.add_widget(FillWidget::new(), Rect::new(10.0, 20.0, 100.0, 50.0));
        let outside =
            scene.add_widget(FillWidget::new(), Rect::new(5_000.0, 5_000.0, 50.0, 50.0));

        let mut tree = WidgetTree::new();
        let view = SceneView::new(scene);
        // Padding(40, 40, 40, 40) shifts the SceneView's bounds.origin
        // to (40, 40) within the 800×600 root layout.
        let root_id = tree.add(Padding::uniform(40.0_f32).child(view));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // The SceneView should be the only child of the Padding.
        let view_id = tree.children(root_id)[0];
        let view = tree
            .widget_as_any(view_id)
            .and_then(|a| a.downcast_ref::<SceneView>())
            .expect("nested view downcast");

        // After layout, the SceneView's bounds_origin signal mirrors
        // the parent's chosen position (40, 40).
        assert_eq!(
            view.view_transform().apply_point(Point::new(0.0, 0.0)),
            Point::new(40.0, 40.0),
            "scene origin should land at SceneView's bounds origin under identity view"
        );

        // The visible item's arena bounds = pure scene_rect (no
        // bounds.origin offset, since the renderer adds it via
        // set_transform at paint time).
        let inside_widget = view.widget_id_for(inside).unwrap();
        assert_eq!(
            tree.bounds(inside_widget),
            Rect::new(10.0, 20.0, 100.0, 50.0),
            "child placed at pure scene_rect"
        );
        // Visual position via view_transform = bounds.origin + scene_rect.origin
        // (zoom = 1, pan = 0, rotation = 0).
        let visual_origin = view
            .view_transform()
            .apply_point(Point::new(10.0, 20.0));
        assert!(
            (visual_origin.x - 50.0).abs() < 1e-3,
            "visual x = bounds.x + scene.x = 40 + 10 = 50 (got {})",
            visual_origin.x
        );
        assert!((visual_origin.y - 60.0).abs() < 1e-3);

        // Off-screen item culled to zero size despite the non-root
        // bounds.origin.
        let outside_widget = view.widget_id_for(outside).unwrap();
        let outside_bounds = tree.bounds(outside_widget);
        assert_eq!(outside_bounds.width, 0.0);
        assert_eq!(outside_bounds.height, 0.0);
    }

    #[test]
    fn non_root_pinch_keeps_scene_under_gesture_center_invariant() {
        // The bounds-origin fix to `anchor_pan_for_pinch` means that
        // even when the SceneView is positioned at a non-zero parent
        // offset, pinch-to-zoom keeps the scene point under the
        // gesture center anchored to that center after zoom.
        //
        // The scene needs at least one item so `place_children`
        // actually runs and refreshes `bounds_origin_signal` — an
        // empty SceneView has no children for the framework to
        // walk past `place_children`, so its bounds origin would
        // stay at the `Vec2::ZERO` initial (a documented edge case
        // for empty scenes that real apps never hit).
        use fern_widgets::primitives::Padding;

        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let mut tree = WidgetTree::new();
        let view = SceneView::new(scene);
        let root_id = tree.add(Padding::uniform(50.0_f32).child(view));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view_id = tree.children(root_id)[0];

        // Move the pointer onto the SceneView and dispatch a pinch.
        // Gesture center at screen (200, 150). The SceneView is at
        // bounds.origin = (50, 50), so the scene point under the
        // center is (200 - 50, 150 - 50) = (150, 100) at zoom 1.
        tree.pointer_move(Point::new(200.0, 150.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: fern_core::gesture::GestureEvent::PinchChanged {
                center: Point::new(200.0, 150.0),
                scale: 2.0,
                rotation: 0.0,
            },
        });

        let view = tree
            .widget_as_any(view_id)
            .and_then(|a| a.downcast_ref::<SceneView>())
            .expect("downcast");
        // After zoom, scene (150, 100) must still project to
        // screen (200, 150).
        let projected = view
            .view_transform()
            .apply_point(Point::new(150.0, 100.0));
        assert!(
            (projected.x - 200.0).abs() < 1e-2,
            "projected x = {}, expected 200",
            projected.x
        );
        assert!((projected.y - 150.0).abs() < 1e-2);
    }

    #[test]
    fn empty_scene_culling_is_a_no_op() {
        // Trivial — empty scene, trivial cull. Pins that the empty
        // case doesn't panic on the inverse-transform / index query
        // path.
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert!(tree.children(view_id).is_empty());
    }

    #[test]
    fn pinch_with_invalid_scale_is_no_op() {
        // Defensive: scale = 0 or NaN must not crash or zero the zoom.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.pointer_move(Point::new(400.0, 300.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: fern_core::gesture::GestureEvent::PinchChanged {
                center: Point::new(400.0, 300.0),
                scale: 0.0,
                rotation: 0.0,
            },
        });
        assert!((view_handle(&tree, view_id).zoom() - 1.0).abs() < 1e-3);

        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: fern_core::gesture::GestureEvent::PinchChanged {
                center: Point::new(400.0, 300.0),
                scale: f32::NAN,
                rotation: 0.0,
            },
        });
        assert!((view_handle(&tree, view_id).zoom() - 1.0).abs() < 1e-3);
    }

    // -- Phase 4 lightweight items ---------------------------------------

    #[test]
    fn scene_view_paints_visible_lightweight_items() {
        // Scene with a lightweight RectItem inside the viewport and
        // another well outside it. After `tree.render()`, exactly one
        // DecorationRect lands in the frame — the off-screen item is
        // culled before paint by `items_in_rect`.
        use crate::items::RectItem;
        use fern_tokens::Color;

        let mut scene = Scene::new();
        let _on_screen = scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED),
        );
        let _off_screen = scene.add_item(
            RectItem::new(Rect::new(5_000.0, 5_000.0, 20.0, 20.0)).fill(Color::BLUE),
        );

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        // Single visible filled item ⇒ exactly one decoration.
        // (FillWidget paints nothing; SceneView paints no chrome of
        // its own; the off-screen item is culled.)
        assert_eq!(
            frame.decorations.len(),
            1,
            "visible RectItem must emit exactly one DecorationRect, off-screen item must be culled"
        );
        assert_eq!(frame.decorations[0].color, Color::RED.to_array());
    }

    #[test]
    fn scene_view_culls_all_off_screen_lightweight_items() {
        // Both items off-screen → zero decorations from the
        // lightweight tier.
        use crate::items::RectItem;
        use fern_tokens::Color;

        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(5_000.0, 5_000.0, 20.0, 20.0)).fill(Color::RED),
        );
        scene.add_item(
            RectItem::new(Rect::new(-5_000.0, -5_000.0, 20.0, 20.0)).fill(Color::BLUE),
        );

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert!(frame.decorations.is_empty());
    }

    #[test]
    fn scene_view_paints_no_items_when_scene_is_widget_only() {
        // Heavyweight-only scene: SceneView::paint walks `items_in_rect`
        // but `Scene::item(id)` returns None for widgets, so no extra
        // draw commands are emitted from the lightweight tier.
        // Verifies the kind-filtering in paint and avoids a
        // double-paint of widget bounds via the scene path.
        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(10.0, 10.0, 20.0, 20.0));

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        // FillWidget paints nothing of its own; the scene contains
        // only widgets (no SceneItems); the SceneView itself doesn't
        // draw any background. Therefore the frame has no
        // decoration / shape / glyph / path entries at all.
        assert!(frame.decorations.is_empty());
        assert!(frame.paths.is_empty());
        assert!(frame.shapes.is_empty());
    }

    #[test]
    fn scene_view_clips_children_so_items_dont_leak() {
        // SceneView::clips_children() returns true. Without a clip,
        // a path-item whose stroke extends past the viewport would
        // bleed past the SceneView's screen rect. The clip is what
        // contains the lightweight tier visually.
        let scene = Scene::new();
        let view = SceneView::new(scene);
        assert!(
            Widget::clips_children(&view),
            "SceneView must clip its subtree so light items don't bleed past bounds"
        );
    }
}
