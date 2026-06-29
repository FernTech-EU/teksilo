// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneScrollView`] — a thin composite that gives a [`SceneView`] draggable
//! scroll bars, mirroring the widget-tier
//! [`ScrollArea`](bastyde_widgets::ScrollArea)'s options: the same
//! [`ScrollBarMode`] (Overlay / Permanent / Thin, with its Tier-3
//! `ScrollBarStyle`), per-axis [`ScrollBarPolicy`] (AsNeeded / AlwaysOn /
//! AlwaysOff), and thickness. Smooth wheel / keyboard panning and the
//! overscroll policy stay configured on the wrapped `SceneView` itself (it
//! already animates pan and honours reduced-motion); the scroll bars simply
//! track that motion.
//!
//! ## Why a wrapper
//!
//! A `SceneView` wraps its **entire child subtree** in the pan/zoom view
//! transform (via `set_content_transform`), so scroll bars added as its own
//! children would pan and zoom along with the content. Instead — exactly like
//! `ScrollArea` wraps arbitrary content and `SceneMinimap` is a sibling overlay
//! — this widget hosts the `SceneView` as content plus two reusable
//! [`ScrollBar`] children *outside* the transform,
//! and bridges the bars' scroll signals to the view's `pan_x`/`pan_y`.
//!
//! ## How the bridge works
//!
//! The scene's scrollable extent is its **effective pan bounds** (the
//! `Scene`-declared `pan_bounds` intersected with any view-level
//! `pan_bounds_override`), falling back to the union of item bounds. With the
//! standard view transform `screen = zoom*scene + pan + bounds_origin` and the
//! `SceneView` placed flush at this widget's origin (so `bounds_origin` cancels
//! the viewport's screen offset), the per-axis mapping in **screen-pixel
//! units** is:
//!
//! ```text
//! scroll_pos_x   = -pan_x - extent.x * zoom
//! max_scroll_x   = (extent.width * zoom - viewport_width).max(0)
//! viewport_ratio = viewport_width / (extent.width * zoom)
//! ```
//!
//! and the inverse, when a bar writes a new `scroll_pos_x`:
//!
//! ```text
//! pan_x = -extent.x * zoom - scroll_pos_x
//! ```
//!
//! The display direction (camera → bar metrics) is recomputed each
//! `place_children` — the same place `ScrollArea` computes its metrics — so it
//! never lags a layout pass. The interaction direction (bar drag → pan) is a
//! pair of guarded effects, one per axis, that snap the pan **immediately** so
//! the thumb tracks the cursor 1:1 (the desktop scroll-bar convention). Both
//! use an epsilon equality guard (the `color_picker` bidirectional-bridge
//! idiom) so a write arriving from the opposite direction is a no-op and the
//! loop closes — in particular the bars track the `SceneView`'s own smooth
//! wheel / keyboard pan animation without fighting it.
//!
//! Rotation is supported but **approximate**: the mapping is exact only when
//! `rotation == 0`; while rotated the thumbs track the camera using the
//! axis-aligned formula above.

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;

use bastyde_widgets::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};
pub use bastyde_widgets::{ScrollBarMode, ScrollBarPolicy};

use crate::scene::PanAxes;
use crate::scene_model::SceneModel;
use crate::view::SceneView;

/// Resolve the scrollable extent (scene coords): the effective pan bounds
/// (Scene-declared `pan_bounds` intersected with the view-level override —
/// tightening-only, falling back to either side alone), and finally to the
/// union of all item rects when no bounds are declared.
fn effective_extent(model: &SceneModel, override_bounds: Option<Rect>) -> Option<Rect> {
    let scene_bounds = model.current_pan_bounds();
    let merged = match (scene_bounds, override_bounds) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r),
        (Some(a), Some(b)) => Some(intersect_rect(a, b)),
    };
    merged.or_else(|| model.0.borrow().scene_rect_extent())
}

/// Intersection of two rects; falls back to `a` when they don't overlap
/// (mirrors `SceneView`'s `intersect_pan_bounds`: a non-overlapping override
/// can't loosen the Scene-declared bounds, so keep the declared one).
fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    if right > x && bottom > y {
        Rect::new(x, y, right - x, bottom - y)
    } else {
        a
    }
}

/// Per-axis scroll metrics in screen-pixel units.
#[derive(Clone, Copy)]
struct AxisMetrics {
    max_x: f32,
    max_y: f32,
    ratio_x: f32,
    ratio_y: f32,
    pos_x: f32,
    pos_y: f32,
}

/// A [`SceneView`] with draggable scroll bars.
///
/// Construct directly from a configured view, or via the
/// [`SceneView::with_scroll_bars`] convenience method:
///
/// ```rust
/// # use bastyde_scene::{Scene, SceneView, SceneScrollView, ScrollBarMode};
/// let scrollable = SceneView::new(Scene::new())
///     .with_scroll_bars()
///     .scroll_bar_mode(ScrollBarMode::Overlay);
/// # let _ = scrollable;
/// ```
pub struct SceneScrollView {
    /// The wrapped view, moved into the arena on first build.
    scene_view: Option<Box<SceneView>>,
    scene_view_id: Option<WidgetId>,

    // --- signals captured from the SceneView before it is moved in ---
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    model: SceneModel,
    pan_bounds_override: Signal<Option<Rect>>,

    // --- owned bridge signals (screen-pixel units) ---
    /// Horizontal scroll position; read by the h-bar, written by both the bar
    /// drag and the display recompute in `place_children`.
    scroll_pos_x: Signal<f32>,
    scroll_pos_y: Signal<f32>,
    max_scroll_x: Signal<f32>,
    max_scroll_y: Signal<f32>,
    viewport_ratio_x: Signal<f32>,
    viewport_ratio_y: Signal<f32>,

    /// Resolved children: `[scene_view, v_scrollbar, h_scrollbar]`.
    child_ids: Vec<WidgetId>,

    // --- configuration (mirrors ScrollArea) ---
    scroll_bar_mode: ScrollBarMode,
    vertical_policy: ScrollBarPolicy,
    horizontal_policy: ScrollBarPolicy,
    scroll_bar_thickness: f32,
}

impl std::fmt::Debug for SceneScrollView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneScrollView")
            .field("mode", &self.scroll_bar_mode)
            .field("v_policy", &self.vertical_policy)
            .field("h_policy", &self.horizontal_policy)
            .field("max_scroll_x", &self.max_scroll_x.get())
            .field("max_scroll_y", &self.max_scroll_y.get())
            .finish()
    }
}

impl SceneScrollView {
    /// Wrap a configured [`SceneView`] in a scroll-bar host. Captures the
    /// view's pan/zoom/model signals before moving it into the arena.
    pub fn new(view: SceneView) -> Self {
        let pan_x = view.pan_x_signal();
        let pan_y = view.pan_y_signal();
        let zoom = view.zoom_signal();
        let model = view.model();
        let pan_bounds_override = view.pan_bounds_override_signal();

        Self {
            scene_view: Some(Box::new(view)),
            scene_view_id: None,
            pan_x,
            pan_y,
            zoom,
            model,
            pan_bounds_override,
            scroll_pos_x: Signal::new(0.0),
            scroll_pos_y: Signal::new(0.0),
            max_scroll_x: Signal::new(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_x: Signal::new(1.0),
            viewport_ratio_y: Signal::new(1.0),
            child_ids: Vec::new(),
            scroll_bar_mode: ScrollBarMode::default(),
            vertical_policy: ScrollBarPolicy::default(),
            horizontal_policy: ScrollBarPolicy::default(),
            scroll_bar_thickness: 12.0,
        }
    }

    /// Set the scroll-bar display mode (Overlay / Permanent / Thin).
    pub fn scroll_bar_mode(mut self, mode: ScrollBarMode) -> Self {
        self.scroll_bar_mode = mode;
        self
    }

    /// Set the vertical scroll-bar visibility policy.
    pub fn vertical_policy(mut self, policy: ScrollBarPolicy) -> Self {
        self.vertical_policy = policy;
        self
    }

    /// Set the horizontal scroll-bar visibility policy.
    pub fn horizontal_policy(mut self, policy: ScrollBarPolicy) -> Self {
        self.horizontal_policy = policy;
        self
    }

    /// Set the scroll-bar thickness (and the gutter width in Permanent mode).
    pub fn scroll_bar_thickness(mut self, thickness: f32) -> Self {
        self.scroll_bar_thickness = thickness.max(0.0);
        self
    }

    /// Horizontal scroll position signal (screen-pixel units), for external
    /// observation. `0` = content's leading edge flush with the viewport.
    pub fn scroll_pos_x_signal(&self) -> &Signal<f32> {
        &self.scroll_pos_x
    }

    /// Vertical scroll position signal (screen-pixel units).
    pub fn scroll_pos_y_signal(&self) -> &Signal<f32> {
        &self.scroll_pos_y
    }

    /// Maximum horizontal scroll offset (`extent.width*zoom - viewport_width`,
    /// or 0 when the content fits). Bind for "is there more to scroll?" chrome.
    pub fn max_scroll_x_signal(&self) -> &Signal<f32> {
        &self.max_scroll_x
    }

    /// Maximum vertical scroll offset.
    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    /// Horizontal viewport/content ratio (0.0..1.0) — the relative thumb size.
    pub fn viewport_ratio_x_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_x
    }

    /// Vertical viewport/content ratio (0.0..1.0).
    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Compute the per-axis scroll metrics for a given viewport size, from the
    /// current pan / zoom / extent. Returns all-zero (ratios 1.0) when there is
    /// no usable extent, which collapses `AsNeeded` bars.
    fn metrics(&self, viewport_w: f32, viewport_h: f32) -> AxisMetrics {
        let zoom = self.zoom.get();
        let extent = effective_extent(&self.model, self.pan_bounds_override.get());
        match extent {
            Some(e) if e.width > 0.0 && e.height > 0.0 && zoom > 0.0 => {
                let content_w = e.width * zoom;
                let content_h = e.height * zoom;
                let max_x = (content_w - viewport_w).max(0.0);
                let max_y = (content_h - viewport_h).max(0.0);
                let ratio_x = (viewport_w / content_w).clamp(0.0, 1.0);
                let ratio_y = (viewport_h / content_h).clamp(0.0, 1.0);
                let pos_x = (-self.pan_x.get() - e.x * zoom).clamp(0.0, max_x);
                let pos_y = (-self.pan_y.get() - e.y * zoom).clamp(0.0, max_y);
                AxisMetrics {
                    max_x,
                    max_y,
                    ratio_x,
                    ratio_y,
                    pos_x,
                    pos_y,
                }
            }
            _ => AxisMetrics {
                max_x: 0.0,
                max_y: 0.0,
                ratio_x: 1.0,
                ratio_y: 1.0,
                pos_x: 0.0,
                pos_y: 0.0,
            },
        }
    }

    /// Register the guarded `scroll_pos → pan` effect for one axis. `is_x`
    /// selects the X (true) or Y (false) axis. Re-installed every build (effect
    /// handles are dropped on rebuild).
    fn register_pan_effect(&self, ctx: &mut BuildContext, is_x: bool) {
        let scroll_pos = if is_x {
            self.scroll_pos_x.clone()
        } else {
            self.scroll_pos_y.clone()
        };
        let pan = if is_x {
            self.pan_x.clone()
        } else {
            self.pan_y.clone()
        };
        let zoom = self.zoom.clone();
        let model = self.model.clone();
        let pan_bounds_override = self.pan_bounds_override.clone();

        ctx.effect(&scroll_pos, move |new_pos| {
            // Respect the scene's pan-axes policy: a locked axis never pans,
            // even if its (hidden) metrics still report overflow.
            let axes = model.current_pan_axes();
            let allowed = if is_x {
                matches!(axes, PanAxes::Horizontal | PanAxes::Both)
            } else {
                matches!(axes, PanAxes::Vertical | PanAxes::Both)
            };
            if !allowed {
                return;
            }
            let Some(extent) = effective_extent(&model, pan_bounds_override.get()) else {
                return;
            };
            let z = zoom.get();
            if z <= 0.0 {
                return;
            }
            let extent_origin = if is_x { extent.x } else { extent.y };
            let target_pan = -extent_origin * z - *new_pos;
            // Guard: skip the write that the display recompute already
            // reflected (the value came from `place_children` tracking the
            // camera, not from a drag/click on the bar).
            let implied = -pan.get() - extent_origin * z;
            if (implied - *new_pos).abs() < 0.5 {
                return;
            }
            // Snap, not animate: the thumb must track the cursor 1:1, and an
            // animated pan would fight the per-frame display recompute.
            pan.set(target_pan);
        });
    }
}

/// Decide whether a scroll bar is shown given its policy, axis permission, and
/// current overflow.
fn resolve_show(policy: ScrollBarPolicy, axis_allowed: bool, max: f32) -> bool {
    axis_allowed
        && match policy {
            ScrollBarPolicy::AlwaysOn => true,
            ScrollBarPolicy::AlwaysOff => false,
            ScrollBarPolicy::AsNeeded => max > 0.0,
        }
}

/// Guarded write — only notify observers when the value actually changed
/// (mirrors `ScrollArea::place_children`). Prevents re-dirtying the bar
/// children on every relayout that reaches this node with identical metrics.
fn set_if_changed(sig: &Signal<f32>, v: f32) {
    if (sig.get() - v).abs() > f32::EPSILON {
        sig.set(v);
    }
}

impl Widget for SceneScrollView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // First build only: move the view in and create the bars.
        if self.child_ids.is_empty() {
            let view = self
                .scene_view
                .take()
                .expect("SceneScrollView: SceneView already consumed");
            let sv_id = ctx.add(*view);
            self.scene_view_id = Some(sv_id);

            let visual = match self.scroll_bar_mode {
                ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
                ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
                ScrollBarMode::Thin => ScrollBarVisual::Thin,
            };

            let v_bar = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_pos_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            )
            .thickness(self.scroll_bar_thickness)
            .visual(visual);
            let v_id = ctx.add(v_bar);

            let h_bar = ScrollBar::new(
                ScrollBarOrientation::Horizontal,
                self.scroll_pos_x.clone(),
                self.max_scroll_x.clone(),
                self.viewport_ratio_x.clone(),
            )
            .thickness(self.scroll_bar_thickness)
            .visual(visual);
            let h_id = ctx.add(h_bar);

            self.child_ids = vec![sv_id, v_id, h_id];

            // Any camera or extent change must re-run `place_children` so the
            // metrics (and `AsNeeded` visibility) refresh. The bars' own thumb
            // bindings are RepaintOnly; this drives the layout side.
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            self.pan_x
                .bind_to(self_id, registry, BindingLevel::Relayout);
            self.pan_y
                .bind_to(self_id, registry, BindingLevel::Relayout);
            self.zoom.bind_to(self_id, registry, BindingLevel::Relayout);
            self.model
                .pan_bounds_signal()
                .bind_to(self_id, registry, BindingLevel::Relayout);
            // `place_children`/`metrics` also read the pan axes (gates the
            // AsNeeded show/hide) and the view-level pan-bounds override (feeds
            // `effective_extent`). Both are runtime-mutable, so bind them too —
            // otherwise mutating either leaves max_scroll_*/viewport_ratio_*
            // stale until an unrelated relayout fires.
            self.model
                .pan_axes_signal()
                .bind_to(self_id, registry, BindingLevel::Relayout);
            self.pan_bounds_override
                .bind_to(self_id, registry, BindingLevel::Relayout);
        }

        // Always (re)register the bar→pan effects — handles are dropped on
        // rebuild.
        self.register_pan_effect(ctx, true);
        self.register_pan_effect(ctx, false);

        self.child_ids.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Delegate to the SceneView, which fills the proposed area.
        self.scene_view_id
            .and_then(|id| ctx.child_size(id, proposal))
            .map(LayoutResponse::from)
            .unwrap_or_else(|| proposal.resolve(800.0, 600.0).into())
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.len() < 3 {
            return;
        }

        let axes = self.model.current_pan_axes();
        let h_allowed = matches!(axes, PanAxes::Horizontal | PanAxes::Both);
        let v_allowed = matches!(axes, PanAxes::Vertical | PanAxes::Both);
        let sb = self.scroll_bar_thickness;
        let permanent = self.scroll_bar_mode == ScrollBarMode::Permanent;

        // Pass 1: decide visibility from metrics against the full bounds, so a
        // Permanent gutter reservation can be computed.
        let m1 = self.metrics(bounds.width, bounds.height);
        let show_v1 = resolve_show(self.vertical_policy, v_allowed, m1.max_y);
        let show_h1 = resolve_show(self.horizontal_policy, h_allowed, m1.max_x);
        let v_reserved = if permanent && show_v1 { sb } else { 0.0 };
        let h_reserved = if permanent && show_h1 { sb } else { 0.0 };

        // Pass 2: final metrics against the reserved viewport.
        let viewport_w = (bounds.width - v_reserved).max(0.0);
        let viewport_h = (bounds.height - h_reserved).max(0.0);
        let m = self.metrics(viewport_w, viewport_h);
        let show_v = resolve_show(self.vertical_policy, v_allowed, m.max_y);
        let show_h = resolve_show(self.horizontal_policy, h_allowed, m.max_x);

        // Publish metrics (guarded). Writing `scroll_pos_*` triggers the
        // bar→pan effect, whose guard absorbs the round-trip.
        set_if_changed(&self.max_scroll_x, m.max_x);
        set_if_changed(&self.max_scroll_y, m.max_y);
        set_if_changed(&self.viewport_ratio_x, m.ratio_x);
        set_if_changed(&self.viewport_ratio_y, m.ratio_y);
        set_if_changed(&self.scroll_pos_x, m.pos_x);
        set_if_changed(&self.scroll_pos_y, m.pos_y);

        // Place the SceneView filling the (possibly gutter-reduced) area.
        // A reserved vertical gutter sits on the right in LTR but on the left
        // in RTL (see the vertical bar placement below), so in RTL the content
        // must start `v_reserved` to the right or it overlaps the bar. The
        // horizontal gutter is always at the bottom, so `y` never shifts.
        let content_x = if ctx.is_rtl() {
            bounds.x + v_reserved
        } else {
            bounds.x
        };
        children[0].origin = Point::new(content_x, bounds.y);
        children[0].size = Size::new(viewport_w, viewport_h);

        // Vertical scroll bar.
        if show_v {
            let sb_x = if ctx.is_rtl() {
                bounds.x
            } else {
                bounds.right() - sb
            };
            let sb_h = if h_reserved > 0.0 || (!permanent && show_h) {
                bounds.height - sb
            } else {
                bounds.height
            };
            children[1].origin = Point::new(sb_x, bounds.y);
            children[1].size = Size::new(sb, sb_h);
        } else {
            children[1].origin = bounds.origin();
            children[1].size = Size::ZERO;
        }

        // Horizontal scroll bar.
        if show_h {
            let sb_y = bounds.bottom() - sb;
            let sb_x = if ctx.is_rtl() && v_reserved > 0.0 {
                bounds.x + sb
            } else {
                bounds.x
            };
            let sb_w = if v_reserved > 0.0 || (!permanent && show_v) {
                bounds.width - sb
            } else {
                bounds.width
            };
            children[2].origin = Point::new(sb_x, sb_y);
            children[2].size = Size::new(sb_w, sb);
        } else {
            children[2].origin = bounds.origin();
            children[2].size = Size::ZERO;
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut bastyde_canvas::Canvas, _ctx: &PaintContext) {
        // The SceneView and the ScrollBar children paint themselves.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Transparent grouping container: the SceneView owns the real scene AT
        // tree, and the ScrollBar children hide themselves from AT. Claiming
        // Role::ScrollView here would add a redundant node above the scene.
        builder.set_role(bastyde_core::accesskit::Role::Group);
        builder.inner_mut().set_clips_children();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    use bastyde_core::widget_tree::WidgetTree;

    /// A bounded scene of `extent` size, wrapped in a `SceneScrollView`.
    fn bounded(extent: Rect, axes: PanAxes, mode: ScrollBarMode) -> SceneScrollView {
        let mut scene = Scene::new();
        scene.set_pan_bounds(Some(extent));
        scene.pan_axes(axes);
        SceneView::new(scene)
            .with_scroll_bars()
            .scroll_bar_mode(mode)
    }

    #[test]
    fn metrics_from_known_extent() {
        let mut tree = WidgetTree::new();
        let scrollable = bounded(
            Rect::new(0.0, 0.0, 800.0, 600.0),
            PanAxes::Both,
            ScrollBarMode::Overlay,
        );
        let max_x = scrollable.max_scroll_x_signal().clone();
        let max_y = scrollable.max_scroll_y_signal().clone();
        let ratio_x = scrollable.viewport_ratio_x_signal().clone();
        let pos_x = scrollable.scroll_pos_x_signal().clone();

        let _id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        assert!((max_x.get() - 400.0).abs() < 0.5, "max_x = {}", max_x.get());
        assert!((max_y.get() - 300.0).abs() < 0.5, "max_y = {}", max_y.get());
        assert!(
            (ratio_x.get() - 0.5).abs() < 0.01,
            "ratio_x = {}",
            ratio_x.get()
        );
        assert!(pos_x.get().abs() < 0.5, "pos_x = {}", pos_x.get());
    }

    #[test]
    fn as_needed_hides_bar_when_content_fits() {
        // Extent smaller than the viewport → nothing to scroll.
        let mut tree = WidgetTree::new();
        let scrollable = bounded(
            Rect::new(0.0, 0.0, 200.0, 200.0),
            PanAxes::Both,
            ScrollBarMode::Overlay,
        );
        let id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(id);
        assert_eq!(children.len(), 3);
        let v_sb = tree.bounds(children[1]);
        assert!(
            v_sb.width.abs() < 0.01 && v_sb.height.abs() < 0.01,
            "v_sb = {:?}",
            v_sb
        );
        let h_sb = tree.bounds(children[2]);
        assert!(
            h_sb.width.abs() < 0.01 && h_sb.height.abs() < 0.01,
            "h_sb = {:?}",
            h_sb
        );
    }

    #[test]
    fn permanent_mode_reserves_gutter() {
        let mut tree = WidgetTree::new();
        let scrollable = bounded(
            Rect::new(0.0, 0.0, 800.0, 600.0),
            PanAxes::Both,
            ScrollBarMode::Permanent,
        )
        .scroll_bar_thickness(12.0);
        let id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(id);
        let scene_view = tree.bounds(children[0]);
        assert!(
            (scene_view.width - 388.0).abs() < 0.5,
            "scene view width = {}",
            scene_view.width
        );
        assert!(
            (scene_view.height - 288.0).abs() < 0.5,
            "scene view height = {}",
            scene_view.height
        );
    }

    #[test]
    fn pan_axes_horizontal_hides_vertical_bar() {
        // Content overflows both axes, but only horizontal pan is allowed.
        let mut tree = WidgetTree::new();
        let scrollable = bounded(
            Rect::new(0.0, 0.0, 800.0, 600.0),
            PanAxes::Horizontal,
            ScrollBarMode::Overlay,
        );
        let id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let children = tree.children(id);
        let v_sb = tree.bounds(children[1]);
        assert!(
            v_sb.width.abs() < 0.01 && v_sb.height.abs() < 0.01,
            "vertical bar should be hidden when axis locked, got {:?}",
            v_sb
        );
        // Horizontal bar still present.
        let h_sb = tree.bounds(children[2]);
        assert!(
            h_sb.width > 0.0,
            "horizontal bar should be visible, got {:?}",
            h_sb
        );
    }

    #[test]
    fn zoom_changes_max_scroll() {
        let mut scene = Scene::new();
        scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 800.0, 600.0)));
        let view = SceneView::new(scene);
        let zoom = view.zoom_signal();
        let scrollable = view.with_scroll_bars();
        let max_x = scrollable.max_scroll_x_signal().clone();

        let mut tree = WidgetTree::new();
        let _id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            (max_x.get() - 400.0).abs() < 0.5,
            "zoom=1 max_x = {}",
            max_x.get()
        );

        zoom.set(2.0);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // content width = 800 * 2 = 1600; max_x = 1600 - 400 = 1200.
        assert!(
            (max_x.get() - 1200.0).abs() < 0.5,
            "zoom=2 max_x = {}",
            max_x.get()
        );
    }

    /// The load-bearing interaction direction: writing `scroll_pos_x` (what a
    /// ScrollBar drag does) drives `pan_x`, and the display recompute that
    /// follows does not re-trigger the effect into a feedback loop.
    #[test]
    fn scroll_pos_drives_pan_without_feedback_loop() {
        let mut scene = Scene::new();
        scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 800.0, 600.0)));
        let view = SceneView::new(scene);
        let pan_x = view.pan_x_signal();
        let scrollable = view.with_scroll_bars();
        let scroll_pos_x = scrollable.scroll_pos_x_signal().clone();

        let mut tree = WidgetTree::new();
        let _id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(pan_x.get().abs() < 0.5, "initial pan_x = {}", pan_x.get());

        // Simulate a drag that scrolls 200 px right (extent.x = 0, zoom = 1, so
        // pan_x = -extent.x*zoom - scroll_pos = -200).
        scroll_pos_x.set(200.0);
        assert!(
            (pan_x.get() - (-200.0)).abs() < 0.5,
            "pan_x should follow scroll, got {}",
            pan_x.get()
        );

        // Re-layout recomputes scroll_pos_x from the new pan — it must settle at
        // 200 (consistent), not drift, which would betray a feedback loop.
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            (scroll_pos_x.get() - 200.0).abs() < 0.5,
            "scroll_pos_x should settle at 200, got {}",
            scroll_pos_x.get()
        );
        assert!(
            (pan_x.get() - (-200.0)).abs() < 0.5,
            "pan_x should stay at -200, got {}",
            pan_x.get()
        );
    }

    /// A locked pan axis never moves even if its (hidden) bar's position signal
    /// is written.
    #[test]
    fn locked_axis_ignores_scroll_writes() {
        let mut scene = Scene::new();
        scene.set_pan_bounds(Some(Rect::new(0.0, 0.0, 800.0, 600.0)));
        scene.pan_axes(PanAxes::Horizontal); // vertical pan locked
        let view = SceneView::new(scene);
        let pan_y = view.pan_y_signal();
        let scrollable = view.with_scroll_bars();
        let scroll_pos_y = scrollable.scroll_pos_y_signal().clone();

        let mut tree = WidgetTree::new();
        let _id = tree.add(scrollable);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        scroll_pos_y.set(150.0);
        assert!(
            pan_y.get().abs() < 0.5,
            "locked vertical axis must not pan, got {}",
            pan_y.get()
        );
    }
}
