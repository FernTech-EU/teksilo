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

    // --- Phase 5a a11y configuration ----------------------------------
    a11y_off_screen_mode: crate::a11y::A11yOffScreenMode,

    // --- Phase 5b a11y configuration ----------------------------------
    /// Cooperative (default) vs StrictlyParallel.
    a11y_mode: crate::a11y::A11yMode,
    /// SceneView's own arena `WidgetId`, captured during the first
    /// `build()`. Needed by `a11y_redirect_descendant` to compute
    /// the synthetic `NodeId` of a declared logical parent group
    /// (the hash key is `(self_id, group_id, SyntheticKind::SceneGroup)`).
    /// `Cell` because the trait method is `&self`.
    self_widget_id: Cell<Option<WidgetId>>,

    // --- Interactivity ------------------------------------------------
    /// When `false`, `build()` skips registering scroll / pinch /
    /// keyboard handlers and does not mark the SceneView focusable.
    /// Programmatic `pan_to` / `zoom_to` still work — this only
    /// gates user-driven navigation. Used by chart-style nested
    /// scenes where the outer container is purely decorative
    /// (axis chrome around an inner data SceneView).
    interactive: bool,

    // --- Cached derived signals ---------------------------------------
    /// `view_transform` as a derived `Signal<Transform2D>`,
    /// constructed once in `new()` and reused across rebuilds.
    /// Exposed via [`view_transform_signal`](Self::view_transform_signal)
    /// so consumers (e.g. axis labels in a parent SceneView) can
    /// bind to it reactively without taking a snapshot every paint.
    view_transform_signal: Signal<Transform2D>,
}

impl SceneView {
    /// Wrap a [`Scene`] in a viewport. The scene is moved into the
    /// view; query / mutate it later via [`SceneView::scene_mut`].
    pub fn new(scene: Scene) -> Self {
        let pan_x = Signal::new_animated(0.0);
        let pan_y = Signal::new_animated(0.0);
        let zoom = Signal::new_animated(1.0);
        let rotation = Signal::new_animated(0.0);
        let bounds_origin_signal = Signal::new(Vec2::ZERO);
        // Derived view-transform signal — composed once in `new` so
        // it's stable across rebuilds. The same instance is used by
        // `set_transform` in `build` and exposed publicly via
        // [`view_transform_signal`](Self::view_transform_signal).
        let view_transform_signal = pan_x
            .zip3(&pan_y, &zoom)
            .zip(&rotation)
            .zip(&bounds_origin_signal)
            .map(|(((px, py, z), r), bo)| {
                compose_view(Vec2::new(*px + bo.x, *py + bo.y), *z, *r)
            });
        Self {
            scene,
            materialized: HashMap::new(),
            widget_to_item: HashMap::new(),
            default_size: Size::new(800.0, 600.0),
            last_viewport: Cell::new(Size::new(800.0, 600.0)),
            pan_x,
            pan_y,
            zoom,
            rotation,
            bounds_origin_signal,
            min_zoom: DEFAULT_MIN_ZOOM,
            max_zoom: DEFAULT_MAX_ZOOM,
            pan_anim_duration: DEFAULT_PAN_DURATION,
            zoom_anim_duration: DEFAULT_ZOOM_DURATION,
            line_height: DEFAULT_LINE_HEIGHT,
            a11y_off_screen_mode: crate::a11y::A11yOffScreenMode::default(),
            a11y_mode: crate::a11y::A11yMode::default(),
            self_widget_id: Cell::new(None),
            interactive: true,
            view_transform_signal,
        }
    }

    /// Disable user-driven navigation: scroll, pinch, and keyboard
    /// handlers are not registered, and the SceneView is not made
    /// focusable. Programmatic [`pan_to`](Self::pan_to) /
    /// [`zoom_to`](Self::zoom_to) / [`fit_to_content`](Self::fit_to_content)
    /// still work — this gates only user input.
    ///
    /// Use this for **outer** SceneViews in nested chart-style
    /// patterns: an outer locked SceneView holds axis chrome
    /// (`TextItem`s reading the inner's pan/zoom signals via
    /// [`view_transform_signal`](Self::view_transform_signal)),
    /// an inner interactive SceneView holds the data and accepts
    /// pan/zoom from the user. Default: interactive (`true`).
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Live `Signal<f32>` for the X pan offset. Use this from a
    /// parent scene (or any reactive consumer) to derive values
    /// that follow the SceneView's pan — typically axis-label
    /// text in a chart-style outer SceneView.
    pub fn pan_x_signal(&self) -> Signal<f32> {
        self.pan_x.clone()
    }

    /// Live `Signal<f32>` for the Y pan offset.
    pub fn pan_y_signal(&self) -> Signal<f32> {
        self.pan_y.clone()
    }

    /// Live `Signal<f32>` for the zoom factor.
    pub fn zoom_signal(&self) -> Signal<f32> {
        self.zoom.clone()
    }

    /// Live `Signal<f32>` for the rotation in radians.
    pub fn rotation_signal(&self) -> Signal<f32> {
        self.rotation.clone()
    }

    /// Live `Signal<Transform2D>` for the composed view transform
    /// (pan + zoom + rotation + bounds-origin). Folds in the
    /// `bounds.origin` contribution so reactive consumers see the
    /// exact transform the renderer applies. Updated whenever any
    /// of the underlying signals change. Use this when the
    /// consumer needs the full matrix (e.g. converting a screen
    /// point to scene coords from outside the SceneView).
    pub fn view_transform_signal(&self) -> Signal<Transform2D> {
        self.view_transform_signal.clone()
    }

    /// Override the [`A11yMode`](crate::a11y::A11yMode) for this
    /// SceneView. Default is `Cooperative` — the visual scene
    /// layout drives AT emission unless explicitly overridden via
    /// [`Scene::set_a11y_parent`](crate::Scene::set_a11y_parent).
    /// Switch to `StrictlyParallel` when your app's AT shape is
    /// fundamentally different from its visual layout: items
    /// without a declared logical parent are then suppressed from
    /// the AT tree, and the app declares every node it wants AT
    /// users to reach.
    pub fn a11y_mode(mut self, mode: crate::a11y::A11yMode) -> Self {
        self.a11y_mode = mode;
        self
    }

    /// Override the off-screen visibility policy for the AT walker.
    /// Default: `ViewportPlusN { n: 1 }` — items inside the
    /// viewport plus a one-screen margin appear in the AT tree.
    /// `AllItems` for small scenes where AT users want a complete
    /// table of contents; `ViewportOnly` for very large scenes where
    /// listing off-screen content would overwhelm AT clients.
    pub fn a11y_off_screen_mode(mut self, mode: crate::a11y::A11yOffScreenMode) -> Self {
        self.a11y_off_screen_mode = mode;
        self
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

        // Walk every lightweight item and let it register its own
        // reactive bindings against this SceneView. Items with
        // signal-bound state (e.g. `TextItem::bind_text`) call
        // `signal.bind_to(scene_view_id, registry, RepaintOnly)`
        // here so a signal change dirties our paint and the next
        // walk reads the current value. Items without bindings
        // default to a no-op `register_bindings`.
        let self_id_for_items = ctx.self_id();
        for entry in self.scene.entries.iter() {
            if let crate::scene::SceneEntryKind::Item(item) = &entry.kind {
                item.register_bindings(ctx, self_id_for_items);
            }
        }

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

        // The view-transform signal is constructed once in `new`
        // (so it's stable across rebuilds and exposable via
        // `view_transform_signal()`). Bind it as a `set_transform`
        // scope on this widget; the render walker pushes it around
        // our entire subtree. The composition folds `bounds.origin`
        // into the final translate so a SceneView at a non-zero
        // parent offset still maps scene-coord (sx, sy) to screen
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        let self_id = ctx.self_id();
        ctx.set_transform(self_id, self.view_transform_signal.clone());
        // Capture for the AT-redirect hook (Phase 5b auto-graft).
        // The hook is `&self`; without a stash here it has no way
        // to derive its own `WidgetId` to compute synthetic NodeIds.
        self.self_widget_id.set(Some(self_id));

        // Wire scroll + pinch handlers. Captures are by clone so they
        // outlive the build call.
        let prefers_reduced = ctx.prefers_reduced_motion();
        let line_height = self.line_height;
        let pan_dur = self.pan_anim_duration;
        let min_zoom = self.min_zoom;
        let max_zoom = self.max_zoom;

        let mut handlers = HandlerSet::new();

        // User-driven navigation (scroll, pinch, keyboard) is gated
        // by `interactive`. When false (typically the outer scene
        // in a nested chart-style layout), skip handler registration
        // entirely — events bubble through to the inner SceneView,
        // which handles them with its own handlers. Programmatic
        // pan_to / zoom_to remain callable.
        if self.interactive {

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

        // --- Phase 5a keyboard navigation -------------------------------
        //
        // Default scheme:
        // - Arrow keys: pan by ~one viewport-quarter per press. Released
        //   here for now; held-key repeat naturally chains tweens via
        //   `animate_to`. Apps that wire `focus_order(...)` (Phase 5b)
        //   can override the arrow path by handling them upstream.
        // - `+` / `=`: zoom in by 1.25× about the viewport center.
        // - `-`: zoom out by 0.8× about the viewport center.
        // - `0`: reset zoom to 1.0 about the viewport center.
        //
        // Handler is `on_key` (focused-widget surface) — it only
        // fires when the SceneView itself is the focus target, NOT
        // when a heavyweight child (like a TextInput) has focus and
        // the user is typing. This is the right default: typing
        // letters into a card shouldn't pan the scene. Apps that
        // want global pan/zoom shortcuts should register them
        // through the `Shortcut`/`Action` pipeline so they work
        // regardless of focus.
        {
            use fern_core::event::{EventResponse, Key, WidgetEvent};
            let pan_x = self.pan_x.clone();
            let pan_y = self.pan_y.clone();
            let zoom = self.zoom.clone();
            let pan_dur = self.pan_anim_duration;
            let zoom_dur = self.zoom_anim_duration;
            let min_zoom = self.min_zoom;
            let max_zoom = self.max_zoom;
            let viewport_size = self.last_viewport.clone();
            let pan_x_for_xform = self.pan_x.clone();
            let pan_y_for_xform = self.pan_y.clone();
            let zoom_for_xform = self.zoom.clone();
            let rotation_for_xform = self.rotation.clone();
            let bounds_origin_for_xform = self.bounds_origin_signal.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                // Pan step = quarter of the smaller viewport axis,
                // capped to a sensible minimum so unusually small
                // viewports still feel responsive.
                let vp = viewport_size.get();
                let pan_step = (vp.width.min(vp.height) * 0.25).max(64.0);
                let mut handled = true;
                let recenter_zoom = |z_new: f32| {
                    // Adjust pan so the viewport center stays fixed
                    // when zoom changes. Same anchor logic as pinch
                    // about viewport center, but always centered.
                    let bo = bounds_origin_for_xform.get();
                    let viewport = vp;
                    let anchor_screen = fern_canvas::Point::new(
                        bo.x + viewport.width * 0.5,
                        bo.y + viewport.height * 0.5,
                    );
                    let z_old = zoom_for_xform.get();
                    let r = rotation_for_xform.get();
                    let pan_old = Vec2::new(pan_x_for_xform.get(), pan_y_for_xform.get());
                    if let Some(new_pan) = anchor_pan_for_pinch(
                        anchor_screen, pan_old, z_old, r, z_new, r, bo,
                    ) {
                        pan_x_for_xform.animate_to(new_pan.x, pan_dur, Easing::EaseOut);
                        pan_y_for_xform.animate_to(new_pan.y, pan_dur, Easing::EaseOut);
                    }
                };
                match key {
                    Key::ArrowLeft => {
                        let target = pan_x.animation_target().unwrap_or_else(|| pan_x.get())
                            + pan_step;
                        pan_x.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    Key::ArrowRight => {
                        let target = pan_x.animation_target().unwrap_or_else(|| pan_x.get())
                            - pan_step;
                        pan_x.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    Key::ArrowUp => {
                        let target = pan_y.animation_target().unwrap_or_else(|| pan_y.get())
                            + pan_step;
                        pan_y.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    Key::ArrowDown => {
                        let target = pan_y.animation_target().unwrap_or_else(|| pan_y.get())
                            - pan_step;
                        pan_y.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    other if other.to_char() == Some('+') || other.to_char() == Some('=') => {
                        let z_new = (zoom.get() * 1.25).clamp(min_zoom, max_zoom);
                        zoom.animate_to(z_new, zoom_dur, Easing::EaseOut);
                        recenter_zoom(z_new);
                    }
                    other if other.to_char() == Some('-') => {
                        let z_new = (zoom.get() * 0.8).clamp(min_zoom, max_zoom);
                        zoom.animate_to(z_new, zoom_dur, Easing::EaseOut);
                        recenter_zoom(z_new);
                    }
                    other if other.to_char() == Some('0') => {
                        zoom.animate_to(1.0, zoom_dur, Easing::EaseOut);
                        recenter_zoom(1.0);
                    }
                    _ => handled = false,
                }
                if handled {
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
            // SceneView itself is focusable so it can receive these
            // key events. Heavyweight children grab focus first when
            // they're the click target — typing in a card stays in
            // the card.
            handlers = handlers.focusable(true);
        }

        } // end of `if self.interactive`

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

    fn wants_descendant_redirects(&self) -> bool {
        // SceneView opts into the ancestor-chain query so
        // `A11yNode::Widget(widget_id)` declarations work for
        // widgets at any arena depth — not just direct children
        // of SceneView. The walker pays the ancestor walk only
        // when at least one ancestor opts in, so the cost is
        // contained to subtrees that actually need it.
        true
    }

    fn a11y_redirect_descendant(
        &self,
        _self_id: WidgetId,
        descendant: WidgetId,
    ) -> Option<accesskit::NodeId> {
        // Phase 5b auto-graft: tell the framework walker to skip
        // its default push for any heavyweight scene entry whose
        // declared logical parent is in our own logical tree.
        // Two paths:
        //   1. The widget was added via `Scene::add_widget` (most
        //      common). Its `ItemId` lives in `widget_to_item`.
        //      Look up the declaration via
        //      `a11y_parent_of(A11yNode::Item(item_id))`.
        //   2. The widget was relocated ad-hoc via
        //      `set_a11y_parent(A11yNode::Widget(widget_id), ...)`.
        //      Look it up directly. Used for descendants of
        //      heavyweight items.
        use crate::a11y::A11yNode;
        use fern_core::accessibility::{synthetic_node_id, SyntheticKind};
        let owner = self.self_widget_id.get()?;
        let parent = self
            .widget_to_item
            .get(&descendant)
            .and_then(|item_id| self.scene.a11y_parent_of(A11yNode::Item(*item_id)))
            .or_else(|| self.scene.a11y_parent_of(A11yNode::Widget(descendant)))?;
        match parent {
            A11yNode::Item(item_id) => Some(synthetic_node_id(
                owner,
                item_id.as_u64(),
                SyntheticKind::SceneItem,
            )),
            A11yNode::Group(group_id) => Some(synthetic_node_id(
                owner,
                group_id.as_u64(),
                SyntheticKind::SceneGroup,
            )),
            A11yNode::Widget(_) => {
                // Widget→Widget reparenting: the declared parent
                // widget's NodeId isn't ours to attach to (it's
                // owned by another arena widget's accessibility()
                // emission). Fall through.
                None
            }
        }
    }

    fn accessibility(&self, builder: &mut fern_core::accessibility::AccessNodeBuilder) {
        use crate::a11y::A11yNode;
        use crate::scene::SceneEntryKind;
        use fern_core::accessibility::{synthetic_node_id, SyntheticKind};
        use std::collections::{HashMap, HashSet};

        // SceneView itself is `Role::Pane` — a generic container
        // surfacing the scene as one navigable region. Heavyweight
        // children (real widgets in the arena) are emitted by the
        // tree walker as natural descendants; we only need to add
        // the lightweight tier here.
        builder.set_role(accesskit::Role::Pane);

        // Compute screen-space viewport for the at-visible-region
        // query. `last_viewport` was set by `layout_response`;
        // `bounds_origin_signal` was set by `place_children`.
        let viewport_size = self.last_viewport.get();
        let bounds_origin = self.bounds_origin_signal.get();
        let viewport_screen = Rect::new(
            bounds_origin.x,
            bounds_origin.y,
            viewport_size.width,
            viewport_size.height,
        );
        let view_transform = self.view_transform();
        let visible_scene_region = match view_transform.inverse() {
            Some(inv) => inv.apply_rect(viewport_screen),
            None => Rect::ZERO,
        };
        let at_region = self
            .a11y_off_screen_mode
            .at_visible_region(visible_scene_region);

        // The set of items the off-screen-mode policy says are
        // AT-visible. Used to filter the second pass.
        let visible_item_ids: HashSet<ItemId> = match at_region {
            Some(r) => self.scene.items_in_rect(r).into_iter().collect(),
            None => self.scene.ids().into_iter().collect(),
        };

        // Build a `parent → ordered children` map of the logical
        // tree. Roots (no declared parent) live under the synthetic
        // key `None`. Insertion-order preserves the apps' declared
        // child order: groups in `add_a11y_group` order, items in
        // `add_item` order. The `None` bucket keeps groups before
        // items so screen readers announce structure first.
        let mut logical_children: HashMap<Option<A11yNode>, Vec<A11yNode>> = HashMap::new();

        // Phase 1: place groups. Groups always emit — they have no
        // visual default to fall back to. A group with no declared
        // parent goes to SceneView root, regardless of mode.
        for group in &self.scene.a11y_groups {
            let node = A11yNode::Group(group.id);
            let parent = self.scene.a11y_parent_of(node);
            logical_children.entry(parent).or_default().push(node);
        }

        // Phase 2: place all visible scene entries — lightweight
        // items and heavyweight widgets alike. Both kinds use
        // `A11yNode::Item(item_id)` as their logical-tree address.
        // Discrimination by entry kind happens at emit time.
        //
        // Mode dispatch (applies to lightweight items only —
        // heavyweight widgets always emit via the framework walker
        // since they own focus / interaction state; the only
        // question is whether they emit at SceneView root or under
        // a declared logical parent):
        //   - Cooperative: item without a declared parent emits
        //     as a SceneView-root child (visual default).
        //   - StrictlyParallel: lightweight item without a parent
        //     is suppressed; heavyweight without a parent stays
        //     at SceneView root via the framework walker.
        for entry in &self.scene.entries {
            if !visible_item_ids.contains(&entry.id) {
                continue;
            }
            let node = A11yNode::Item(entry.id);
            let parent = self.scene.a11y_parent_of(node);
            let is_widget = matches!(&entry.kind, SceneEntryKind::Widget { .. });
            match (parent, is_widget, self.a11y_mode) {
                (Some(p), _, _) => {
                    logical_children.entry(Some(p)).or_default().push(node);
                }
                (None, false, crate::a11y::A11yMode::Cooperative) => {
                    // Lightweight item, root, cooperative → emit at root.
                    logical_children.entry(None).or_default().push(node);
                }
                (None, false, crate::a11y::A11yMode::StrictlyParallel) => {
                    // Lightweight item, root, strict → suppressed.
                }
                (None, true, _) => {
                    // Heavyweight at root — let the framework walker
                    // handle it via natural descendant emission. No
                    // entry in our logical tree.
                }
            }
        }

        // Phase 3: ad-hoc widget relocations addressed by `WidgetId`
        // (rare — typically a descendant of a heavyweight scene
        // item that should belong elsewhere logically). Widgets
        // referenced via `A11yNode::Item(item_id)` are already
        // handled in Phase 2.
        for (child_node, parent_node) in &self.scene.a11y_parents {
            if matches!(child_node, A11yNode::Widget(_)) {
                logical_children
                    .entry(Some(*parent_node))
                    .or_default()
                    .push(*child_node);
            }
        }

        // Walk the logical tree DFS, depth-first, emitting synthetic
        // NodeIds. Cycle guard: a node visited twice (the result of
        // a malformed `set_a11y_parent(A, B); set_a11y_parent(B, A)`
        // pairing) is skipped on its second appearance.
        let mut visited: HashSet<A11yNode> = HashSet::new();
        let roots = logical_children
            .get(&None)
            .cloned()
            .unwrap_or_default();
        for root in roots {
            self.emit_logical_node(builder, root, None, &logical_children, &mut visited);
        }

        // Apply Phase 5b cross-tree decorations (relations / live
        // regions / landmarks). Items / groups must already be in
        // `children_collected` for the writes to land on the right
        // node. Heavyweight widgets are not yet routed through here
        // — the walker can't decorate widget-derived NodeIds from a
        // sibling's accessibility() impl. Apps that need to point
        // a `flow_to`/`controls` at a widget should use the
        // synthetic NodeIds (Phase 5b decorating widgets is part of
        // the deferred auto-graft work).
        let owner = builder.owner_id();
        let resolve = |node: A11yNode| -> Option<accesskit::NodeId> {
            match node {
                A11yNode::Item(id) => owner.map(|o| {
                    synthetic_node_id(o, id.as_u64(), SyntheticKind::SceneItem)
                }),
                A11yNode::Group(id) => owner.map(|o| {
                    synthetic_node_id(o, id.as_u64(), SyntheticKind::SceneGroup)
                }),
                A11yNode::Widget(id) => {
                    Some(fern_core::accessibility::widget_id_to_node_id(id))
                }
            }
        };
        for (from, kind, to) in self.scene.a11y_relations() {
            let (Some(from_id), Some(to_id)) = (resolve(*from), resolve(*to)) else {
                continue;
            };
            self.apply_relation_to_collected(builder, from_id, *kind, to_id);
        }
        for (node, live) in &self.scene.a11y_live {
            let Some(id) = resolve(*node) else { continue };
            self.set_collected_live(builder, id, *live);
        }
        for (node, role) in &self.scene.a11y_landmarks {
            let Some(id) = resolve(*node) else { continue };
            self.set_collected_role(builder, id, *role);
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
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

    // -- Phase 5b helpers used by `accessibility` -----------------------

    /// Recursive DFS step: emit one node of the logical tree under
    /// `parent_id` (`None` = SceneView's own node), then descend.
    /// Cycle-guards via `visited`; the same node visited twice is
    /// skipped on the second appearance, so a malformed parent
    /// declaration doesn't infinite-loop the walker.
    fn emit_logical_node(
        &self,
        builder: &mut fern_core::accessibility::AccessNodeBuilder,
        node: crate::a11y::A11yNode,
        parent_id: Option<accesskit::NodeId>,
        logical_children: &std::collections::HashMap<
            Option<crate::a11y::A11yNode>,
            Vec<crate::a11y::A11yNode>,
        >,
        visited: &mut std::collections::HashSet<crate::a11y::A11yNode>,
    ) {
        use crate::a11y::A11yNode;
        use fern_core::accessibility::SyntheticKind;

        if !visited.insert(node) {
            return;
        }

        let view_transform = self.view_transform();
        let synthetic_id = match node {
            A11yNode::Item(item_id) => {
                // Discriminate by entry kind: lightweight items
                // emit a synthetic AT node; heavyweight items
                // attach the framework-emitted widget node under
                // the declared parent (auto-graft).
                if let Some(item) = self.scene.item(item_id) {
                    let scene_bounds = item.bounds_in_scene();
                    let screen_bounds = view_transform.apply_rect(scene_bounds);
                    let ctx = crate::item::SceneItemA11yContext {
                        view_transform,
                        screen_bounds,
                        item_id,
                    };
                    builder.push_scene_child_under(
                        parent_id,
                        item_id.as_u64(),
                        SyntheticKind::SceneItem,
                        |child| {
                            item.accessibility(child, &ctx);
                            child.inner_mut().set_bounds(accesskit::Rect {
                                x0: screen_bounds.x as f64,
                                y0: screen_bounds.y as f64,
                                x1: (screen_bounds.x + screen_bounds.width) as f64,
                                y1: (screen_bounds.y + screen_bounds.height) as f64,
                            });
                        },
                    )
                } else if let Some(&widget_id) = self.materialized.get(&item_id) {
                    // Heavyweight scene entry — auto-graft.
                    let Some(parent) = parent_id else {
                        debug_assert!(
                            false,
                            "auto-graft requires a declared parent — root \
                             heavyweight items emit through the framework walker"
                        );
                        return;
                    };
                    let widget_node_id =
                        fern_core::accessibility::widget_id_to_node_id(widget_id);
                    builder.attach_scene_child_under(parent, widget_node_id);
                    widget_node_id
                } else {
                    // Item id not found — Scene was mutated between
                    // logical-tree population and emit. Skip.
                    return;
                }
            }
            A11yNode::Group(group_id) => {
                let Some(group) = self.scene.a11y_group(group_id) else {
                    return;
                };
                let role = group.role;
                let label = group.label.clone();
                builder.push_scene_child_under(
                    parent_id,
                    group_id.as_u64(),
                    SyntheticKind::SceneGroup,
                    |child| {
                        child.set_role(role);
                        if let Some(label) = label {
                            child.set_name(label);
                        }
                    },
                )
            }
            A11yNode::Widget(widget_id) => {
                // Auto-graft: the widget's full AT node is emitted
                // by the framework walker as part of the recursive
                // descent. Here we only need to add its NodeId to
                // the declared parent's children list. The
                // redirect hook (`a11y_redirect_descendant`) tells
                // the walker to skip its own push, so the widget
                // appears exactly once — under its declared
                // logical parent.
                //
                // Widgets at the logical-tree root (parent_id =
                // None) should never get here: the population
                // pass only adds widgets when their parent is
                // declared. Bail on that path so we don't
                // double-attach.
                let Some(parent) = parent_id else {
                    debug_assert!(
                        false,
                        "auto-graft requires a declared parent — root widgets emit \
                         through the framework walker as natural descendants"
                    );
                    return;
                };
                let widget_node_id =
                    fern_core::accessibility::widget_id_to_node_id(widget_id);
                builder.attach_scene_child_under(parent, widget_node_id);
                widget_node_id
            }
        };

        if let Some(children) = logical_children.get(&Some(node)) {
            for child in children {
                self.emit_logical_node(
                    builder,
                    *child,
                    Some(synthetic_id),
                    logical_children,
                    visited,
                );
            }
        }
    }

    /// Apply an [`A11yRelation`] to the synthetic node identified by
    /// `from_id` in the builder's collected children. No-op (with
    /// debug-assert) if `from_id` isn't found — the relation source
    /// must have been emitted into the logical tree first.
    fn apply_relation_to_collected(
        &self,
        builder: &mut fern_core::accessibility::AccessNodeBuilder,
        from_id: accesskit::NodeId,
        kind: crate::a11y::A11yRelation,
        to_id: accesskit::NodeId,
    ) {
        use crate::a11y::A11yRelation;
        builder.with_collected_node(from_id, |node| match kind {
            A11yRelation::Controls => node.push_controlled(to_id),
            A11yRelation::DescribedBy => node.push_described_by(to_id),
            A11yRelation::LabelledBy => node.push_labelled_by(to_id),
            A11yRelation::FlowTo => node.push_flow_to(to_id),
        });
    }

    fn set_collected_live(
        &self,
        builder: &mut fern_core::accessibility::AccessNodeBuilder,
        node_id: accesskit::NodeId,
        live: accesskit::Live,
    ) {
        builder.with_collected_node(node_id, |node| {
            node.set_live(live);
        });
    }

    fn set_collected_role(
        &self,
        builder: &mut fern_core::accessibility::AccessNodeBuilder,
        node_id: accesskit::NodeId,
        role: accesskit::Role,
    ) {
        builder.with_collected_node(node_id, |node| {
            node.set_role(role);
        });
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

    // -- Phase 5a a11y + keyboard navigation -----------------------------

    #[test]
    fn scene_view_emits_synthetic_at_node_per_visible_item() {
        // The AT walker should emit one synthetic AT node per
        // visible lightweight item, with screen-projected bounds.
        // Off-screen items (subject to the off-screen-mode policy)
        // should be excluded from the tree.
        use crate::items::RectItem;
        use fern_core::accessibility::{is_synthetic, synthetic_node_id, SyntheticKind};
        use fern_tokens::Color;

        let mut scene = Scene::new();
        let on_screen = scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0))
                .fill(Color::RED)
                .access_label("nearby"),
        );
        let _far_off = scene.add_item(
            RectItem::new(Rect::new(50_000.0, 50_000.0, 20.0, 20.0)).fill(Color::BLUE),
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Compute the synthetic NodeId we expect for the on-screen
        // item. The walker derives `synthetic_node_id(view_id,
        // item_id.as_u64(), SyntheticKind::SceneItem)`.
        let expected_id =
            synthetic_node_id(view_id, on_screen.as_u64(), SyntheticKind::SceneItem);
        assert!(is_synthetic(expected_id), "must have bit-63 set");

        // Build the AT tree update and verify our synthetic NodeId
        // appears (and the off-screen item's would-be id does not).
        let update = tree.sync_accessibility();
        let nodes_have_id = |needle: accesskit::NodeId| {
            update.nodes.iter().any(|(id, _)| *id == needle)
        };
        assert!(
            nodes_have_id(expected_id),
            "on-screen item must appear in the AT tree update"
        );
        let synthetic_count = update
            .nodes
            .iter()
            .filter(|(id, _)| is_synthetic(*id))
            .count();
        assert!(
            synthetic_count >= 1,
            "expected at least one synthetic SceneItem node, got {}",
            synthetic_count
        );
    }

    #[test]
    fn keyboard_arrow_keys_animate_pan() {
        use fern_core::event::{Key, Modifiers, WidgetEvent};

        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Focus the SceneView so on_key fires on it.
        tree.focus(view_id);
        let pan_before = view_handle(&tree, view_id).pan();

        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::default(),
            text: None,
        });

        // Pan target should move (negative x — content shifts to
        // bring the viewport's right side into view, equivalent to
        // panning the scene leftward in screen space). The pan
        // signal is animated; we check the *target*.
        let pan_target_x = view_handle(&tree, view_id)
            .pan_x_animation_target()
            .unwrap_or(pan_before.x);
        assert!(
            pan_target_x < pan_before.x,
            "ArrowRight should reduce pan_x target (saw {})",
            pan_target_x
        );
    }

    #[test]
    fn keyboard_plus_minus_animate_zoom() {
        use fern_core::event::{Key, Modifiers, WidgetEvent};

        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.focus(view_id);
        let zoom_before = view_handle(&tree, view_id).zoom();

        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Character('+'),
            modifiers: Modifiers::default(),
            text: Some("+".into()),
        });

        let zoom_after_target = view_handle(&tree, view_id)
            .zoom_animation_target()
            .unwrap_or(zoom_before);
        assert!(
            zoom_after_target > zoom_before,
            "Plus key should increase zoom target (saw {})",
            zoom_after_target
        );
    }

    #[test]
    fn a11y_off_screen_mode_viewport_only_excludes_grown_items() {
        // With ViewportOnly, an item just past the viewport edge
        // does NOT appear in the AT tree, even though the default
        // ViewportPlusN { n: 1 } would include it.
        use crate::items::RectItem;
        use fern_core::accessibility::is_synthetic;
        use fern_tokens::Color;

        let mut scene = Scene::new();
        // In-viewport item: definitely AT-visible.
        scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED),
        );
        // Item just past the right edge of the 400x300 viewport.
        // Default mode would include it (within 1× viewport
        // margin), but ViewportOnly should not.
        scene.add_item(
            RectItem::new(Rect::new(450.0, 100.0, 20.0, 20.0)).fill(Color::BLUE),
        );

        let mut tree = WidgetTree::new();
        let view_id =
            tree.add(SceneView::new(scene).a11y_off_screen_mode(crate::a11y::A11yOffScreenMode::ViewportOnly));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();
        let synthetic_count = update
            .nodes
            .iter()
            .filter(|(id, _)| is_synthetic(*id))
            .count();
        assert_eq!(
            synthetic_count, 1,
            "ViewportOnly mode must exclude the off-screen item"
        );
    }

    // -- Phase 5b logical AT structure -----------------------------------

    #[test]
    fn add_a11y_group_round_trip() {
        let mut scene = Scene::new();
        let id = scene.add_a11y_group(crate::a11y::A11yGroup::builder().label("Act 1"));
        assert_eq!(scene.a11y_group(id).map(|g| g.label()), Some(Some("Act 1")));
    }

    #[test]
    fn set_a11y_parent_reparents_item_under_group() {
        // Item declared with a logical parent (Group) should be
        // emitted under the group, NOT under the SceneView root.
        use crate::a11y::{A11yGroup, A11yNode};
        use crate::items::RectItem;
        use fern_core::accessibility::{
            is_synthetic, synthetic_node_id, SyntheticKind,
        };

        let mut scene = Scene::new();
        let act1 = scene.add_a11y_group(A11yGroup::builder().label("Act 1"));
        let card = scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).access_label("Scene A"),
        );
        scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(act1)));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        // Group node and item node both exist as synthetic nodes.
        let group_node_id =
            synthetic_node_id(view_id, act1.as_u64(), SyntheticKind::SceneGroup);
        let item_node_id =
            synthetic_node_id(view_id, card.as_u64(), SyntheticKind::SceneItem);

        let find_node = |needle: accesskit::NodeId| {
            update.nodes.iter().find(|(id, _)| *id == needle)
        };
        let group_node = find_node(group_node_id).expect("group node exists");
        let item_node = find_node(item_node_id).expect("item node exists");
        assert!(is_synthetic(group_node.0));
        assert!(is_synthetic(item_node.0));

        // The group's children list contains the item node; the
        // SceneView's children list does NOT contain the item node
        // directly (the reparenting moved it).
        assert!(
            group_node.1.children().contains(&item_node_id),
            "item must be a child of its declared logical parent group"
        );
        let scene_view_node_id =
            fern_core::accessibility::widget_id_to_node_id(view_id);
        let scene_view_node = find_node(scene_view_node_id).expect("scene view node");
        assert!(
            !scene_view_node.1.children().contains(&item_node_id),
            "item must NOT also appear as a direct child of SceneView when reparented"
        );
        assert!(
            scene_view_node.1.children().contains(&group_node_id),
            "group is the root-level synthetic — should be a direct child of SceneView"
        );
    }

    #[test]
    fn nested_groups_emit_in_logical_dfs_order() {
        // Group B parented under Group A → SceneView's children list
        // contains A; A's contains B; B's contains its item.
        use crate::a11y::{A11yGroup, A11yNode};
        use crate::items::RectItem;
        use fern_core::accessibility::{synthetic_node_id, SyntheticKind};

        let mut scene = Scene::new();
        let outer = scene.add_a11y_group(A11yGroup::builder().label("Outer"));
        let inner = scene.add_a11y_group(A11yGroup::builder().label("Inner"));
        let item = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)));
        scene.set_a11y_parent(A11yNode::Group(inner), Some(A11yNode::Group(outer)));
        scene.set_a11y_parent(A11yNode::Item(item), Some(A11yNode::Group(inner)));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        let outer_id =
            synthetic_node_id(view_id, outer.as_u64(), SyntheticKind::SceneGroup);
        let inner_id =
            synthetic_node_id(view_id, inner.as_u64(), SyntheticKind::SceneGroup);
        let item_id_synth =
            synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);

        let find = |needle: accesskit::NodeId| {
            update
                .nodes
                .iter()
                .find(|(id, _)| *id == needle)
                .map(|(_, n)| n)
        };
        let outer_node = find(outer_id).expect("outer group exists");
        let inner_node = find(inner_id).expect("inner group exists");
        let _item_node = find(item_id_synth).expect("item node exists");

        assert!(outer_node.children().contains(&inner_id));
        assert!(inner_node.children().contains(&item_id_synth));
        assert!(!outer_node.children().contains(&item_id_synth));
    }

    #[test]
    fn add_a11y_relation_writes_into_accesskit_arrays() {
        // Declared FlowTo from item A → item B should land as a
        // FlowTo entry on A's AccessKit Node.
        use crate::a11y::{A11yNode, A11yRelation};
        use crate::items::RectItem;
        use fern_core::accessibility::{synthetic_node_id, SyntheticKind};

        let mut scene = Scene::new();
        let a = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)));
        let b = scene.add_item(RectItem::new(Rect::new(40.0, 10.0, 20.0, 20.0)));
        scene.add_a11y_relation(A11yNode::Item(a), A11yRelation::FlowTo, A11yNode::Item(b));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        let a_id = synthetic_node_id(view_id, a.as_u64(), SyntheticKind::SceneItem);
        let b_id = synthetic_node_id(view_id, b.as_u64(), SyntheticKind::SceneItem);
        let a_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == a_id)
            .map(|(_, n)| n)
            .expect("a node exists");
        // AccessKit's `flow_to` accessor returns the slice we pushed.
        assert!(
            a_node.flow_to().contains(&b_id),
            "FlowTo relation must land on AccessKit's flow_to array"
        );
    }

    #[test]
    fn set_a11y_live_marks_node_as_live_region() {
        use crate::a11y::A11yNode;
        use crate::items::RectItem;
        use fern_core::accessibility::{synthetic_node_id, SyntheticKind};

        let mut scene = Scene::new();
        let item = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)));
        scene.set_a11y_live(A11yNode::Item(item), accesskit::Live::Polite);

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        let id = synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);
        let node = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == id)
            .map(|(_, n)| n)
            .expect("item node");
        assert_eq!(node.live(), Some(accesskit::Live::Polite));
    }

    #[test]
    fn set_a11y_landmark_overrides_role() {
        use crate::a11y::A11yNode;
        use crate::items::RectItem;
        use fern_core::accessibility::{synthetic_node_id, SyntheticKind};

        let mut scene = Scene::new();
        let item = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)));
        // RectItem default role is GraphicsObject. Landmark override
        // should re-set it to Region.
        scene.set_a11y_landmark(A11yNode::Item(item), accesskit::Role::Region);

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        let id = synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);
        let node = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == id)
            .map(|(_, n)| n)
            .expect("item node");
        assert_eq!(node.role(), accesskit::Role::Region);
    }

    #[test]
    fn remove_a11y_group_drops_dependent_decorations() {
        // Removing a group must drop relations / live / landmarks
        // / categories that target the group; child items declared
        // under it fall back to the SceneView root.
        use crate::a11y::{A11yCategory, A11yGroup, A11yNode, A11yRelation};
        use crate::items::RectItem;

        let mut scene = Scene::new();
        let g = scene.add_a11y_group(A11yGroup::builder().label("G"));
        let item = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)));
        scene.set_a11y_parent(A11yNode::Item(item), Some(A11yNode::Group(g)));
        scene.set_a11y_live(A11yNode::Group(g), accesskit::Live::Assertive);
        scene.set_a11y_landmark(A11yNode::Group(g), accesskit::Role::Region);
        scene.add_a11y_relation(
            A11yNode::Item(item),
            A11yRelation::Controls,
            A11yNode::Group(g),
        );
        scene.set_a11y_categories(A11yNode::Group(g), &[A11yCategory::new("act")]);

        scene.remove_a11y_group(g);

        // Decorations that targeted the removed group are gone.
        assert!(scene.a11y_group(g).is_none());
        assert!(scene.a11y_live.is_empty());
        assert!(scene.a11y_landmarks.is_empty());
        assert!(scene.a11y_relations().is_empty());
        assert!(scene.a11y_categories_of(A11yNode::Group(g)).is_none());
        // Item's parent declaration is dropped — falls back to root.
        assert!(scene.a11y_parent_of(A11yNode::Item(item)).is_none());
    }

    #[test]
    fn parent_cycle_does_not_loop_walker() {
        // Malformed: A → B → A. The walker visits each node once
        // (HashSet guard) and never recurses indefinitely.
        use crate::a11y::{A11yGroup, A11yNode};

        let mut scene = Scene::new();
        let a = scene.add_a11y_group(A11yGroup::builder().label("A"));
        let b = scene.add_a11y_group(A11yGroup::builder().label("B"));
        scene.set_a11y_parent(A11yNode::Group(a), Some(A11yNode::Group(b)));
        scene.set_a11y_parent(A11yNode::Group(b), Some(A11yNode::Group(a)));

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // Just running sync_accessibility without panic / hang is
        // the assertion: cycle guard works.
        let _ = tree.sync_accessibility();
    }

    // -- Phase 5b: A11yMode + auto-graft of widget descendants -------------

    /// Helper: a widget with a deterministic accessibility role we
    /// can detect in the AT update.
    #[derive(Debug)]
    struct LabelledFill {
        label: &'static str,
    }
    impl Widget for LabelledFill {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            Size::new(20.0, 20.0).into()
        }
        fn accessibility(
            &self,
            builder: &mut fern_core::accessibility::AccessNodeBuilder,
        ) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name(self.label);
        }
    }

    #[test]
    fn cooperative_default_emits_items_at_root_when_unparented() {
        // Cooperative is the default mode. Items without a declared
        // parent emit as direct children of SceneView — Phase 5a
        // visual-default behaviour, preserved.
        use crate::items::RectItem;
        use fern_core::accessibility::{is_synthetic, widget_id_to_node_id};

        let mut scene = Scene::new();
        scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();
        let view_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(view_id))
            .map(|(_, n)| n)
            .expect("scene view node");
        let synth_kids = view_node
            .children()
            .iter()
            .filter(|id| is_synthetic(**id))
            .count();
        assert_eq!(synth_kids, 1, "Cooperative emits unparented item at root");
    }

    #[test]
    fn strictly_parallel_suppresses_unparented_items() {
        // In StrictlyParallel mode an item without a declared
        // parent does NOT emit. Apps must place every node they
        // want AT-visible.
        use crate::a11y::{A11yGroup, A11yMode, A11yNode};
        use crate::items::RectItem;
        use fern_core::accessibility::{is_synthetic, widget_id_to_node_id};

        let mut scene = Scene::new();
        let g = scene.add_a11y_group(A11yGroup::builder().label("G"));
        let placed = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)));
        let _orphan = scene.add_item(RectItem::new(Rect::new(40.0, 10.0, 20.0, 20.0)));
        scene.set_a11y_parent(A11yNode::Item(placed), Some(A11yNode::Group(g)));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene).a11y_mode(A11yMode::StrictlyParallel),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        // Total synthetic node count: just the group + the placed
        // item. The orphan item is suppressed.
        let synth_total = update
            .nodes
            .iter()
            .filter(|(id, _)| is_synthetic(*id))
            .count();
        assert_eq!(
            synth_total, 2,
            "StrictlyParallel: only group + placed item, orphan suppressed"
        );

        // SceneView's children list contains the group only —
        // not the orphan item.
        let view_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(view_id))
            .map(|(_, n)| n)
            .unwrap();
        let synth_kids: Vec<_> = view_node
            .children()
            .iter()
            .filter(|id| is_synthetic(**id))
            .collect();
        assert_eq!(synth_kids.len(), 1, "only the group reaches root");
    }

    #[test]
    fn auto_graft_widget_appears_under_declared_logical_group() {
        // The headline Phase 5b auto-graft test: a heavyweight
        // widget added via `Scene::add_widget` is declared (via
        // its `ItemId`) under a logical group. The widget's
        // `NodeId` must appear in the group's children list AND
        // must NOT appear in SceneView's own children list.
        use crate::a11y::{A11yGroup, A11yNode};
        use fern_core::accessibility::{
            synthetic_node_id, widget_id_to_node_id, SyntheticKind,
        };

        let mut scene = Scene::new();
        let act_one = scene.add_a11y_group(A11yGroup::builder().label("Act 1"));
        let card_item_id = scene.add_widget(
            LabelledFill { label: "card" },
            Rect::new(10.0, 10.0, 20.0, 20.0),
        );
        // Declare the parent up-front via ItemId — works for both
        // lightweight and heavyweight scene entries.
        scene.set_a11y_parent(
            A11yNode::Item(card_item_id),
            Some(A11yNode::Group(act_one)),
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let view = view_handle(&tree, view_id);
        let card_widget_id = view
            .widget_id_for(card_item_id)
            .expect("card was materialised");

        let update = tree.sync_accessibility();
        let view_node_id = widget_id_to_node_id(view_id);
        let group_node_id =
            synthetic_node_id(view_id, act_one.as_u64(), SyntheticKind::SceneGroup);
        let widget_node_id = widget_id_to_node_id(card_widget_id);

        let find = |id: accesskit::NodeId| {
            update
                .nodes
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, n)| n)
        };
        let scene_view = find(view_node_id).expect("scene view node");
        let group = find(group_node_id).expect("group node");
        let _widget_node = find(widget_node_id).expect("widget node still emitted");

        assert!(
            group.children().contains(&widget_node_id),
            "widget must be a child of its declared logical group"
        );
        assert!(
            !scene_view.children().contains(&widget_node_id),
            "widget must NOT also appear as a direct child of SceneView"
        );
    }

    #[test]
    fn auto_graft_redirect_hook_default_is_none() {
        // Sanity: a SceneView with no widget-parent declarations
        // returns None from the redirect hook, so default
        // behaviour is unchanged.
        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let view = view_handle(&tree, view_id);
        // Pick any descendant — without a declaration the hook
        // returns None.
        let view_widget_id = view_id;
        // Use any non-existent widget id; the hook must still
        // return None.
        assert!(
            Widget::a11y_redirect_descendant(view, view_widget_id, view_widget_id)
                .is_none(),
            "redirect hook returns None when no declaration is in place"
        );
    }

    /// A trivial container widget: takes one child via `build`,
    /// reports it through `children()`, lays it out at full
    /// proposed size, paints nothing, opts OUT of descendant
    /// redirects (default false). Used by deep-descendant tests
    /// to insert an extra arena level between SceneView and the
    /// inner widget so we can verify the ancestor-chain walk
    /// reaches SceneView even past a non-opting intermediate.
    #[derive(Debug)]
    struct PlainContainer {
        inner_id: Option<WidgetId>,
    }
    impl PlainContainer {
        fn new() -> Self {
            Self { inner_id: None }
        }
    }
    impl Widget for PlainContainer {
        fn build(
            &mut self,
            ctx: &mut fern_core::build_context::BuildContext,
        ) -> Vec<WidgetId> {
            let id = ctx.add(LabelledFill { label: "inner" });
            self.inner_id = Some(id);
            vec![id]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            proposal.resolve(40.0, 40.0).into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [fern_core::widget::WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            for placement in children.iter_mut() {
                placement.origin = Point::new(bounds.x, bounds.y);
                placement.size = Size::new(bounds.width, bounds.height);
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            self.inner_id.into_iter().collect()
        }
    }

    #[test]
    fn auto_graft_deep_descendant_under_scene_view_group() {
        // The headline deep-descendant test. Arena shape:
        //   SceneView → PlainContainer → LabelledFill (inner)
        //
        // PlainContainer opts OUT of `wants_descendant_redirects`
        // (default false). SceneView opts IN. Declaring
        // `A11yNode::Widget(inner_id)` causes the framework
        // walker — when iterating PlainContainer's children — to
        // walk up the arena, skip PlainContainer (opt-out), find
        // SceneView (opt-in), and consult its hook. SceneView
        // returns `Some(group_node_id)` and the walker skips the
        // default push.
        //
        // Result: inner's NodeId appears in the declared group's
        // children list, NOT in PlainContainer's. The widget's
        // own AccessKit Node still emits via the recursive walk
        // and lands in `nodes`.
        use crate::a11y::{A11yGroup, A11yNode};
        use fern_core::accessibility::{
            synthetic_node_id, widget_id_to_node_id, SyntheticKind,
        };

        // Stage 1: add a PlainContainer scene-entry, layout once
        // to learn the inner widget's allocated `WidgetId`.
        let mut scene = Scene::new();
        let group = scene.add_a11y_group(A11yGroup::builder().label("Tools"));
        scene.add_widget(
            PlainContainer::new(),
            Rect::new(10.0, 10.0, 40.0, 40.0),
        );
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let container_id = tree.children(view_id)[0];
        let inner_id = tree.children(container_id)[0];

        // Stage 2: declare the deep-descendant relocation via
        // `scene_mut()` reached through `widget_as_any_mut`. The
        // arena assigned `inner_id` during layout; use it.
        let scene_view = tree
            .widget_as_any_mut(view_id)
            .and_then(|a| a.downcast_mut::<SceneView>())
            .expect("downcast SceneView mut");
        scene_view.scene_mut().set_a11y_parent(
            A11yNode::Widget(inner_id),
            Some(A11yNode::Group(group)),
        );

        // Stage 3: re-layout (so AT walker sees the new
        // declaration via the next sync_accessibility) and verify.
        // Re-layout marks dirty; the arena state stays stable so
        // `inner_id` is still valid.
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        let group_node_id =
            synthetic_node_id(view_id, group.as_u64(), SyntheticKind::SceneGroup);
        let inner_node_id = widget_id_to_node_id(inner_id);
        let container_node_id = widget_id_to_node_id(container_id);

        let find = |id: accesskit::NodeId| {
            update
                .nodes
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, n)| n)
        };
        let group_node = find(group_node_id).expect("group emitted");
        let container_node = find(container_node_id).expect("container emitted");
        let _inner_node = find(inner_node_id).expect("inner widget still emitted");

        assert!(
            group_node.children().contains(&inner_node_id),
            "inner widget must appear under its declared logical group, \
             not under its arena parent"
        );
        assert!(
            !container_node.children().contains(&inner_node_id),
            "inner widget must NOT appear under PlainContainer (its arena \
             parent) — the redirect skipped that push"
        );
    }

    #[test]
    fn auto_graft_deep_descendant_no_op_without_optin_ancestor() {
        // If no ancestor opts into `wants_descendant_redirects`,
        // the ancestor-chain walk is a no-op (each ancestor's
        // flag is checked, fast-path returns false), and the
        // descendant emits normally as a child of its arena
        // parent. This pins the opt-in semantic: the cost of the
        // ancestor walk is bounded to subtrees that genuinely
        // need it.
        //
        // We can't run a clean test with no SceneView at all
        // (the auto-graft surface doesn't apply), so instead we
        // verify that without a `set_a11y_parent` declaration,
        // the inner widget appears under its arena parent
        // (PlainContainer) — confirming the SceneView opt-in
        // doesn't accidentally claim every descendant.
        use fern_core::accessibility::widget_id_to_node_id;

        let mut scene = Scene::new();
        scene.add_widget(
            PlainContainer::new(),
            Rect::new(10.0, 10.0, 40.0, 40.0),
        );
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let container_id = tree.children(view_id)[0];
        let inner_id = tree.children(container_id)[0];

        let update = tree.sync_accessibility();
        let container_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(container_id))
            .map(|(_, n)| n)
            .expect("container emitted");
        assert!(
            container_node
                .children()
                .contains(&widget_id_to_node_id(inner_id)),
            "without a redirect declaration, inner widget appears under \
             its arena parent — opt-in does not claim every descendant"
        );
    }

    #[test]
    fn ancestor_chain_walk_skips_optout_intermediate() {
        // Arena shape: SceneView → PlainContainer → LabelledFill.
        // PlainContainer is `wants_descendant_redirects = false`
        // (default). The walker, iterating PlainContainer's
        // children, must skip past it and reach SceneView for
        // the redirect query — proving the opt-out flag doesn't
        // halt the walk. Distinct from the headline test in
        // that we explicitly target the intermediate's opt-out
        // behaviour.
        use crate::a11y::{A11yGroup, A11yNode};
        use fern_core::accessibility::{
            synthetic_node_id, widget_id_to_node_id, SyntheticKind,
        };

        let mut scene = Scene::new();
        let group = scene.add_a11y_group(A11yGroup::builder().label("G"));
        scene.add_widget(
            PlainContainer::new(),
            Rect::new(10.0, 10.0, 40.0, 40.0),
        );
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let container_id = tree.children(view_id)[0];
        let inner_id = tree.children(container_id)[0];

        // Sanity: PlainContainer opts out (default).
        assert!(
            !PlainContainer::new().wants_descendant_redirects(),
            "PlainContainer must default to opt-out for this test to be meaningful"
        );

        let scene_view = tree
            .widget_as_any_mut(view_id)
            .and_then(|a| a.downcast_mut::<SceneView>())
            .unwrap();
        scene_view.scene_mut().set_a11y_parent(
            A11yNode::Widget(inner_id),
            Some(A11yNode::Group(group)),
        );

        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();

        let group_node = update
            .nodes
            .iter()
            .find(|(id, _)| {
                *id == synthetic_node_id(
                    view_id,
                    group.as_u64(),
                    SyntheticKind::SceneGroup,
                )
            })
            .map(|(_, n)| n)
            .unwrap();
        assert!(
            group_node.children().contains(&widget_id_to_node_id(inner_id)),
            "ancestor walk must reach SceneView past the opt-out \
             intermediate"
        );
    }

    // -- Nested-SceneView gap-filling APIs ------------------------------

    #[test]
    fn interactive_default_is_true() {
        let view = SceneView::new(Scene::new());
        assert!(
            view.interactive,
            "SceneView::interactive defaults to true"
        );
    }

    #[test]
    fn non_interactive_ignores_scroll() {
        // When the outer SceneView is locked (chart chrome
        // pattern), scroll events must not pan its view. The
        // gesture handlers aren't registered, so the scroll is
        // ignored at this widget — events bubble through to
        // siblings / inner SceneViews that do handle them.
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).interactive(false));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let view = view_handle(&tree, view_id);
        let pan_before = view.pan();

        // Send a scroll directly to the SceneView. Without an
        // on_scroll handler registered, the event is unhandled
        // here and pan stays put.
        tree.pointer_move(Point::new(100.0, 100.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 50.0, y: 50.0 },
        });

        let view = view_handle(&tree, view_id);
        assert_eq!(
            view.pan(),
            pan_before,
            "non-interactive SceneView must not pan on scroll"
        );
        // Animation target must also be unset — no tween started.
        assert!(view.pan_x_animation_target().is_none());
        assert!(view.pan_y_animation_target().is_none());
    }

    #[test]
    fn interactive_does_pan_on_scroll() {
        // Counterpoint: with the default `interactive = true`,
        // scroll DOES animate pan. Pins that the gating doesn't
        // accidentally drop scroll handling for normal scenes.
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.pointer_move(Point::new(100.0, 100.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 50.0, y: 0.0 },
        });

        let view = view_handle(&tree, view_id);
        let target = view
            .pan_x_animation_target()
            .expect("interactive scene must enqueue a pan animation");
        assert!(target.abs() > 1.0, "pan_x animation target moved");
    }

    #[test]
    fn pan_x_signal_returns_live_handle() {
        // `pan_x_signal()` must return a live handle: external
        // observers see updates when pan changes.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view = view_handle(&tree, view_id);
        let signal = view.pan_x_signal();
        assert_eq!(signal.get(), 0.0);

        // Programmatic pan_to.
        view.pan_to(Vec2::new(123.0, 0.0), Duration::ZERO);
        // Animations land via tree.advance_time but Duration::ZERO
        // settles immediately on `set` for finite-duration tweens.
        // Verify the signal reflects the post-target state.
        let target = view
            .pan_x_animation_target()
            .or_else(|| Some(signal.get()))
            .unwrap();
        assert!(
            (target - 123.0).abs() < 1e-3,
            "pan_x_signal must observe pan_to target (saw {})",
            target
        );
    }

    #[test]
    fn view_transform_signal_updates_on_pan() {
        // The composed view_transform signal must reflect pan
        // changes for reactive consumers.
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view = view_handle(&tree, view_id);
        let xform_signal = view.view_transform_signal();
        let before = xform_signal.get();
        assert!(before.is_identity(), "initial view_transform is identity");

        // Set pan_x directly (bypasses tweening).
        view.pan_x_signal().set(50.0);
        let after = xform_signal.get();
        // Translation component should reflect the pan.
        let projected = after.apply_point(Point::new(0.0, 0.0));
        assert!(
            (projected.x - 50.0).abs() < 1e-3,
            "view_transform_signal must update when pan_x changes \
             (projected x = {})",
            projected.x
        );
    }

    #[test]
    fn text_item_with_signal_text_repaints_on_signal_change() {
        // The chart axis-label use case: TextItem::with_signal_text
        // ties its rendered text to a signal. Changing the signal
        // must dirty the SceneView's paint so the next render
        // walks the items and emits the updated text.
        //
        // Binding dirties are processed at the start of `layout()`
        // (via `process_state_changes`), not eagerly on `set()`.
        // The test mirrors the real per-frame pattern: layout →
        // render → mutate signal → next layout marks paint dirty.
        use crate::items::TextItem;
        use fern_core::signal::Signal;

        let mut scene = Scene::new();
        let label_text = Signal::new(String::from("0.0"));
        scene.add_item(TextItem::with_signal_text(
            label_text.clone(),
            Rect::new(0.0, 0.0, 50.0, 20.0),
        ));

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        assert!(
            !tree.needs_paint(),
            "after initial render, paint should be clean"
        );

        // Mutate the signal — RepaintOnly binding queues a dirty
        // entry; the next `layout()` flushes it and marks the
        // SceneView as needing paint.
        label_text.set(String::from("123.4"));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            tree.needs_paint(),
            "TextItem::with_signal_text signal change must dirty \
             SceneView's paint via register_bindings"
        );
    }

    #[test]
    fn text_item_label_returns_static_text_for_static_items() {
        // Existing semantic preserved: TextItem with static text
        // returns it via `label()` when no override is set.
        use crate::items::TextItem;
        let item = TextItem::new("Hello", Rect::new(0.0, 0.0, 50.0, 20.0));
        assert_eq!(crate::item::SceneItem::label(&item).as_deref(), Some("Hello"));
    }

    #[test]
    fn text_item_label_returns_signal_snapshot_for_bound_items() {
        // Bound text: `label()` snapshots the current signal value.
        use crate::items::TextItem;
        use fern_core::signal::Signal;
        let signal = Signal::new(String::from("initial"));
        let item =
            TextItem::with_signal_text(signal.clone(), Rect::new(0.0, 0.0, 50.0, 20.0));
        assert_eq!(crate::item::SceneItem::label(&item).as_deref(), Some("initial"));
        signal.set(String::from("updated"));
        assert_eq!(crate::item::SceneItem::label(&item).as_deref(), Some("updated"));
    }

    #[test]
    fn nested_scene_chart_pattern_smoke() {
        // End-to-end: outer locked SceneView holding axis-label
        // TextItems bound to inner SceneView's pan_x_signal.
        // Verifies the wiring composes cleanly without panic.
        use crate::items::TextItem;
        use fern_core::signal::Signal;

        // Inner data scene.
        let mut inner_scene = Scene::new();
        inner_scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let inner = SceneView::new(inner_scene);
        let inner_pan_x = inner.pan_x_signal();
        let axis_label_text: Signal<String> =
            inner_pan_x.map(|px| format!("x = {:.1}", px));

        // Outer chrome scene.
        let mut outer_scene = Scene::new();
        outer_scene.add_widget(inner, Rect::new(40.0, 0.0, 360.0, 280.0));
        outer_scene.add_item(TextItem::with_signal_text(
            axis_label_text.clone(),
            Rect::new(0.0, 290.0, 80.0, 10.0),
        ));
        let outer = SceneView::new(outer_scene).interactive(false);

        let mut tree = WidgetTree::new();
        let _root_id = tree.add(outer);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();

        // Mutate inner's pan via the outer scene's child handle.
        // For the smoke test, just mutate the signal directly —
        // axis_label_text is a derived signal, mutating its
        // upstream (inner_pan_x) should propagate through. The
        // next `layout()` flushes binding dirties.
        inner_pan_x.set(42.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            tree.needs_paint(),
            "outer SceneView must dirty paint when inner's \
             pan_x_signal changes — derived axis-label text \
             updates via `register_bindings`"
        );
    }
}
