// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneView`] — the viewport widget that hosts a [`Scene`] and
//! places its items at scene coordinates.
//!
//! ## Composition
//!
//! - **Placement.** `place_children` plants each materialised
//!   heavyweight widget at its scene-space rect (composed from the
//!   item's `local_pos`, `transform`, and parent chain).
//! - **Paint bands.** Three passes: `paint` draws the `Under` lightweight
//!   items (backdrop), the arena child-walk draws the heavyweight widgets,
//!   then `post_paint` draws the `Over` lightweight items + marquee /
//!   foreground / debug overlays. `z` orders within each tier; the
//!   Under/Over band ([`Scene::set_layer`](crate::Scene::set_layer))
//!   chooses the side. See `docs/bastyde-scene.md` §"Z-order and paint bands".
//! - **View transform.** Pan / zoom / rotation are four animated
//!   `Signal<f32>`s on `SceneView`, composed into a derived
//!   `Signal<Transform2D>` bound via `BuildContext::set_content_transform`
//!   on the view itself. The render walker pushes that scope around
//!   the entire subtree, so every materialised widget is visually
//!   transformed; transform-aware hit-test routes pointer events
//!   through the same scope.
//! - **Spatial index.** `place_children` and the paint walk consult
//!   `Scene::items_in_rect(visible_region)` to skip off-screen items.
//! - **Idle gating.** Pan / zoom that's reached its terminal tick
//!   stops scheduling frames via the engine's per-node `paint_epoch`.
//!
//! ## Input wiring
//!
//! - **`on_scroll`** — trackpad two-finger pan (`ScrollDelta::Pixels`)
//!   and mouse wheel (`ScrollDelta::Lines`) animate the pan signals
//!   via `Easing::EaseOut`. Trackpad momentum events from winit
//!   arrive as further `Pixels` deltas; the existing animation
//!   pipeline turns this into smooth inertial fling without a custom
//!   recognizer.
//! - **`on_pinch`** — OS trackpad pinch (`PinchPhase::Changed`) feeds
//!   `scale` into the zoom signal and `rotation` into the rotation
//!   signal, anchored around the gesture center so the scene point
//!   under the user's fingers stays put.
//! - **Reduced-motion** — at build time, captures
//!   [`BuildContext::prefers_reduced_motion`](bastyde_core::build_context::BuildContext::prefers_reduced_motion).
//!   When set, scroll handlers `set` the signals directly instead of
//!   `animate_to`-ing them; pinch is already instantaneous.
//! - **Drag-to-move** for items carrying `IS_DRAGGABLE`; **marquee**
//!   selection on the empty viewport surface (or under
//!   [`DragMode::ScrollHandDrag`](crate::DragMode), pan-on-drag).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Point, Rect, Size, SizeProposal, Transform2D, Vec2};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use bastyde_core::gesture::PinchPhase;
use bastyde_core::overscroll::OverscrollBehavior;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Easing;

use crate::item::ItemId;
use crate::scene::Scene;
use crate::scene_model::SceneModel;
use crate::transform::{anchor_pan_for_pinch, compose_view};
use bastyde_i18n::LocalizedString;

/// Logical pixels of pan applied per `ScrollDelta::Lines` notch.
/// Mirrors the convention used by `ScrollArea` (`line_height` ≈ 16 in
/// bastyde-widgets).
const DEFAULT_LINE_HEIGHT: f32 = 16.0;
const DEFAULT_PAN_DURATION: Duration = Duration::from_millis(120);
const DEFAULT_ZOOM_DURATION: Duration = Duration::from_millis(180);
const DEFAULT_MIN_ZOOM: f32 = 0.1;
const DEFAULT_MAX_ZOOM: f32 = 10.0;

/// Maximum movement (scene-coord pixels) between PointerDown and
/// PointerUp for the gesture to count as a tap rather than a drag.
const TAP_MOVEMENT_THRESHOLD: f32 = 4.0;
/// Take the tightening intersection of two optional zoom ranges:
/// `(max(lo), min(hi))`. `None` on either side leaves the other
/// untouched; `None` on both returns `None`. Used to compose
/// Scene-level + view-level constraints — neither side can loosen.
fn intersect_zoom_range(
    a: Option<&std::ops::RangeInclusive<f32>>,
    b: Option<&std::ops::RangeInclusive<f32>>,
) -> Option<std::ops::RangeInclusive<f32>> {
    match (a, b) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r.clone()),
        (Some(a), Some(b)) => {
            let lo = a.start().max(*b.start());
            let hi = a.end().min(*b.end());
            // Guard against degenerate intersect: if the ranges
            // don't overlap (lo > hi), collapse to the tighter
            // side's lo so callers see a single allowed value
            // rather than NaN-clamping.
            Some(lo..=hi.max(lo))
        }
    }
}

/// Clamp a zoom factor through an optional range. `None` is the
/// identity — no clamp applied.
fn clamp_zoom(z: f32, range: Option<&std::ops::RangeInclusive<f32>>) -> f32 {
    match range {
        None => z,
        Some(r) => z.clamp(*r.start(), *r.end()),
    }
}

/// Take the tightening intersection of two optional pan-bounds
/// rects. `None` on either side leaves the other untouched; `None`
/// on both returns `None`. If both are `Some` and the rect
/// intersection is empty (no overlap), falls back to the first
/// (Scene-declared) bounds — the more authoritative side.
fn intersect_pan_bounds(scene: Option<Rect>, view: Option<Rect>) -> Option<Rect> {
    match (scene, view) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r),
        (Some(a), Some(b)) => {
            let x = a.x.max(b.x);
            let y = a.y.max(b.y);
            let right = a.right().min(b.right());
            let bottom = a.bottom().min(b.bottom());
            if right > x && bottom > y {
                Some(Rect::new(x, y, right - x, bottom - y))
            } else {
                Some(a)
            }
        }
    }
}

/// Clamp a pan vector against `bounds` so the visible scene region
/// (derived from `viewport` and `zoom`) stays inside the bounds rect.
/// When the rect is smaller than the visible viewport on an axis,
/// that axis is centered on the bounds rather than clamped.
///
/// `bounds` is in scene coords; `viewport` is the SceneView's
/// resolved size in screen pixels; `zoom` is the current zoom
/// factor. Returns `pan` unchanged when `bounds` is `None`.
fn clamp_pan_to_bounds(pan: Vec2, bounds: Option<&Rect>, viewport: Size, zoom: f32) -> Vec2 {
    let Some(b) = bounds else { return pan };
    if zoom <= 0.0 || viewport.width <= 0.0 || viewport.height <= 0.0 {
        return pan;
    }
    // visible_scene_x = [-pan.x / zoom, (viewport_w - pan.x) / zoom]
    // For visible to lie inside [b.x, b.right]:
    //   pan.x in [viewport_w - b.right * zoom, -b.x * zoom]
    let clamp_axis = |pan_c: f32, b_lo: f32, b_hi: f32, vp: f32| {
        let lo = vp - b_hi * zoom;
        let hi = -b_lo * zoom;
        if hi >= lo {
            pan_c.clamp(lo, hi)
        } else {
            // Bounds smaller than viewport on this axis — center.
            // visible_center = b_lo + (b_hi - b_lo)/2
            //                = (b_lo + b_hi) / 2
            //                = (vp/2 - pan_c) / zoom
            // → pan_c = vp/2 - (b_lo + b_hi)/2 * zoom
            vp / 2.0 - ((b_lo + b_hi) / 2.0) * zoom
        }
    };
    Vec2::new(
        clamp_axis(pan.x, b.x, b.right(), viewport.width),
        clamp_axis(pan.y, b.y, b.bottom(), viewport.height),
    )
}

/// In-flight marquee box-select state. Tracked in scene
/// coordinates so pan/zoom mid-drag (e.g. the user holds shift
/// and scrolls while dragging) doesn't break the rectangle's
/// alignment with scene contents.
#[derive(Debug, Clone, Copy)]
struct MarqueeState {
    origin: Point,
    current: Point,
    /// Whether the marquee is additive (Ctrl/Shift held at start).
    /// On commit, additive = `extend`; non-additive = `replace`.
    additive: bool,
}

impl MarqueeState {
    fn rect(self) -> Rect {
        let x = self.origin.x.min(self.current.x);
        let y = self.origin.y.min(self.current.y);
        let w = (self.origin.x - self.current.x).abs();
        let h = (self.origin.y - self.current.y).abs();
        Rect::new(x, y, w, h)
    }
}

/// A drag-to-move in flight: which lightweight item is being
/// translated, in scene coords. The committed delta on `Ended`
/// is `current_scene - anchor_scene`; that delta is applied to
/// the target item *and* every declared descendant via
/// `Scene::collect_descendants`.
#[derive(Debug, Clone, Copy)]
struct DragTarget {
    item_id: ItemId,
    /// Scene-coord position where the drag started.
    anchor_scene: Point,
    /// Current scene-coord position (updated on each Moved /
    /// Ended). Allows paint to render the in-flight offset for
    /// live visual feedback.
    current_scene: Point,
}

/// Snapshot of one item's hit-test geometry + handler closures used
/// by the SceneView's `on_pointer_event` dispatch path. Refreshed
/// per layout pass alongside `lightweight_bounds_snapshot`.
#[derive(Clone)]
struct HandlerSnapshotEntry {
    id: crate::item::ItemId,
    /// Scene-coord AABB used for broad-phase hit-test (normal items).
    scene_rect: Rect,
    /// Local→scene transform — used to inverse-project the
    /// scene-coord pointer into local coords for shape_contains
    /// narrow-phase. Stored so the dispatch path doesn't have to
    /// re-walk the parent chain (which would need `&Scene`).
    scene_transform: bastyde_canvas::Transform2D,
    /// Item-local hit-test predicate, cloned from the trait via a
    /// small wrapper. Returns `true` when a local point is inside
    /// the item's exact shape; the second argument is the live view
    /// scale (zoom) so cosmetic-stroke hit bands convert to scene
    /// coordinates.
    shape_contains: Rc<dyn Fn(Point, f32) -> bool>,
    /// z-order (used to pick topmost on overlap).
    z: f32,
    /// Item-level handler closures, cloned at snapshot time. `None`
    /// when the item has no handler set installed.
    handlers: Option<Box<crate::item_handlers::SceneItemHandlerSet>>,
    /// `true` when the item carries `ItemFlags::IGNORES_TRANSFORMATIONS`.
    /// Dispatch routes hit-test through screen space: the visible
    /// area is `local_bounds` rooted at the screen-projected
    /// `scene_anchor`, and pan/zoom of the view don't change that
    /// area. `scene_rect` is meaningless for these items because
    /// they don't scale with zoom.
    ignores_xform: bool,
    /// For IGNORES items: the item's origin (local `(0,0)`) mapped
    /// to scene coords through the parent chain. The current view
    /// transform projects this to the screen-space anchor at
    /// dispatch time. For normal items, unused.
    scene_anchor: Point,
    /// For IGNORES items: the item's `local_bounds`. Combined with
    /// the screen anchor at dispatch time to form the screen-space
    /// AABB. For normal items, unused.
    local_bounds: Rect,
}

/// Visual debug overlays painted on top of normal scene rendering.
///
/// Every flag defaults to `false`. Use this to verify that culling /
/// hit-test / spatial-index / dragging are doing what you expect
/// while developing a scene-based feature; turn off before shipping.
///
/// Each flag adds a thin overlay paint with a distinct color so
/// multiple flags can be combined without visual confusion:
///
/// - [`item_bounds`](Self::item_bounds): green outline around every
///   visible scene item's `bounds_in_scene`.
/// - [`content_bounds`](Self::content_bounds): blue outline around
///   the scene's overall content extent (the union of all item
///   bounds).
/// - [`viewport`](Self::viewport): red outline around the visible
///   scene region (the cull rect — the inverse-projected viewport).
/// - [`selection_bounds`](Self::selection_bounds): orange outline
///   around every currently-selected item.
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugOverlay {
    pub item_bounds: bool,
    pub content_bounds: bool,
    pub viewport: bool,
    pub selection_bounds: bool,
}

impl DebugOverlay {
    /// All overlays enabled. Useful to catch any anomaly visually.
    pub const ALL: DebugOverlay = DebugOverlay {
        item_bounds: true,
        content_bounds: true,
        viewport: true,
        selection_bounds: true,
    };

    /// Whether at least one debug overlay is enabled.
    pub fn is_active(&self) -> bool {
        self.item_bounds || self.content_bounds || self.viewport || self.selection_bounds
    }
}

/// Direction passed to a [`SceneView::focus_order`] callback when the
/// app wants to override the default Tab cycle.
///
/// `Forward` corresponds to Tab; `Backward` to Shift+Tab. The default
/// SceneView focus traversal is scene insertion order — apps that
/// need data-flow order (graph editor), story-order (corkboard with
/// Acts), chronological order (timeline), etc. install a callback
/// that receives the current focus and returns the next id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Forward,
    Backward,
}

/// A pannable/zoomable viewport hosting a [`Scene`]'s items at scene
/// coordinates.
pub struct SceneView {
    /// Shared, cloneable handle to the scene this view renders. Multiple
    /// `SceneView`s can hold clones of one [`SceneModel`] and reconcile
    /// independently on every mutation.
    model: SceneModel,
    /// Per-view heavyweight builder for `Delegated` items. Each view calls
    /// its own delegate with an item's type-erased payload to build a fresh
    /// `Widget` instance for **this** view's arena. Returns `None` to skip
    /// an item (e.g. a downcast miss). `None` (the field) = no delegate
    /// installed; only single-view `Once` widgets materialise.
    delegate: Option<Rc<dyn Fn(&dyn std::any::Any, ItemId) -> Option<Box<dyn Widget>>>>,
    /// Items whose payload changed since the last build (filled by the
    /// `item_change` observer on `ItemChange::PayloadChanged`). Drained at
    /// the top of `build`, where each is destroyed and re-materialised via the
    /// delegate. `Rc<RefCell>` so the observer closure can push without
    /// borrowing `model`.
    payload_dirty: Rc<RefCell<HashSet<ItemId>>>,
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
    /// When `true`, [`SceneView::layout_response`] returns the
    /// scene's `scene_rect_extent` as the view's wanted size — the
    /// view sizes itself to its scene. User pan / zoom / drag-to-
    /// move are still gated by [`Scene::pan_axes`] and
    /// [`Scene::is_zoomable`]; the default policy is "no pan, no
    /// zoom" because the entire scene is already on-screen.
    adopt_scene_size: bool,
    /// Drag-on-canvas behavior. Default `RubberBand` (item drag →
    /// move; empty area → marquee). `ScrollHandDrag` makes the
    /// canvas pan unconditionally on left-mouse drag; `NoDrag`
    /// disables the on-drag handler entirely.
    /// Drag mode (rubber-band marquee, scroll-hand pan, or no-drag).
    /// Reactive: gesture handlers read this per event, so mutating
    /// the signal at runtime (typically from a toolbar) flips
    /// behaviour on the next pointer event without rebuilding
    /// the view. `.drag_mode(mode)` writes to it directly;
    /// `.bind_drag_mode(sig)` replaces the inner signal with an
    /// app-owned one so toolbars can share state with the view.
    drag_mode: Signal<crate::item_handlers::DragMode>,
    /// Per-layout snapshot of (id, scene_rect, handlers) for items
    /// that have a handler set installed. Used by the
    /// `on_pointer_event` closure to dispatch hover / tap / context
    /// menu without borrowing `&self.scene`. Refreshed in
    /// `layout_response`.
    handler_snapshot: Rc<RefCell<Vec<HandlerSnapshotEntry>>>,
    /// Currently-hovered item id, used to dispatch `on_hover(false)`
    /// when the pointer leaves it.
    hovered_item: Rc<Cell<Option<crate::item::ItemId>>>,
    /// Last press recorded for tap detection: (scene_pt, item_id).
    /// Cleared on PointerUp / PointerLeave.
    pending_tap: Rc<
        Cell<
            Option<(
                Point,
                crate::item::ItemId,
                bastyde_core::event::PointerButton,
            )>,
        >,
    >,
    /// Latest viewport size observed during layout. Cached so
    /// imperative methods like [`SceneView::fit_to_content`] can
    /// reason about the visible rectangle without re-running layout.
    /// `Rc<Cell>` so event-handler closures (e.g. Ctrl+wheel zoom-
    /// about-viewport-center) can read it without touching `&mut self`.
    /// Last viewport size resolved by `layout_response`. Stored as a
    /// `Signal` (not `Cell`) so derived signals like
    /// [`viewport_in_scene_signal`](Self::viewport_in_scene_signal)
    /// can react to viewport changes. Writes are gated by an
    /// equality check at the call site to avoid notifying on
    /// unchanged values.
    last_viewport: Signal<Size>,

    // --- View transform state ---------------------------------
    pan_x: Signal<f32>,
    pan_y: Signal<f32>,
    zoom: Signal<f32>,
    rotation: Signal<f32>,

    // --- View configuration ----------------------------------------
    /// View-level *tightening* override on the underlying
    /// [`Scene`]'s zoom range. The effective clamp applied at
    /// gesture / set_zoom / pan_to time is the intersection of
    /// `Scene::current_zoom_range()` and this override (see
    /// `effective_zoom_range`). `None` means the view does not
    /// constrain zoom; the default is `Some(0.1..=10.0)` so
    /// existing callers see the historical clamp behaviour.
    zoom_range_override: Signal<Option<std::ops::RangeInclusive<f32>>>,
    /// View-level *tightening* override on the underlying
    /// [`Scene`]'s pan bounds. The effective clamp is the rect
    /// intersection of `Scene::current_pan_bounds()` and this
    /// override. `None` (the default) leaves pan unconstrained
    /// from the view side.
    pan_bounds_override: Signal<Option<Rect>>,
    pan_anim_duration: Duration,
    zoom_anim_duration: Duration,
    line_height: f32,
    /// Whether a wheel that the scene can't absorb (already clamped at its
    /// `pan_bounds`) chains to an ancestor scrollable, or is contained.
    overscroll_behavior: OverscrollBehavior,

    // --- A11y configuration — visual-default path ----------------------------------
    a11y_off_screen_mode: crate::a11y::A11yOffScreenMode,

    // --- A11y configuration — logical structural API ----------------------------------
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

    // --- Selection -----------------------------------------
    /// Reactive selection state. Defaults to `SceneSelectionMode::None`
    /// (no selection wired). Apps opt in via
    /// [`selection_mode`](Self::selection_mode); marquee + click-to-
    /// select then activate.
    selection: crate::selection::SceneSelection,
    /// In-flight marquee state: scene-coord origin + current. While
    /// `Some`, `paint` overlays a semi-transparent rect.
    /// `Rc<Cell>` so the on_drag closure (which only borrows
    /// `&self` shape via the closure's capture) can mutate it.
    marquee: Rc<Cell<Option<MarqueeState>>>,
    /// Pending marquee commit: set by the on_drag closure on
    /// `DragPhase::Ended`, consumed at the start of the next
    /// `place_children` (which has direct `&self.scene` access
    /// via `self`). This indirection avoids forcing `Scene` into
    /// an `Rc<RefCell>`.
    pending_marquee_commit: Rc<Cell<Option<(Rect, bool)>>>,
    /// In-flight drag-to-move state: which item is being dragged
    /// and the scene-coord anchor where the drag started. The
    /// total scene-coord delta is computed at `Ended` from
    /// `current - anchor` and posted to `pending_item_move`.
    /// `Rc<Cell>` so the on_drag closure can mutate via `&self`.
    drag_target: Rc<Cell<Option<DragTarget>>>,
    /// Pending drag-to-move commit: `(target_id, delta)` set by
    /// the on_drag `Ended` branch, drained in `build`. The drain
    /// code translates the target item AND every descendant
    /// (declared via `Scene::set_item_parent`) by the same delta
    /// — so a labelled rectangle (Rect parent + TextItem child)
    /// moves as one unit, QGraphicsScene-style.
    pending_item_move: Rc<Cell<Option<(ItemId, Vec2)>>>,
    /// Snapshot of lightweight scene items + their bounds, used
    /// by the on_drag closure for hit-test. Refreshed in
    /// `place_children` each layout pass — the snapshot stays
    /// consistent within a single drag and refreshes between
    /// drags via the spatial-index mutation triggering relayout.
    /// Avoids forcing `Scene` into an `Rc<RefCell>`.
    lightweight_bounds_snapshot: Rc<RefCell<Vec<(ItemId, Rect)>>>,
    /// Bumped by the on_drag closure on `Ended` after posting a
    /// `pending_item_move`. SceneView binds to this at
    /// `BindingLevel::Rebuild` in `build`, so the next build
    /// cycle drains the pending move and calls
    /// `Scene::set_local_pos` (which requires `&mut self.scene`,
    /// only available inside `build`). Without this signal, the
    /// move was queued but never applied — items "snapped back"
    /// to their original positions on drag release.
    reconcile_dirty: Signal<u64>,

    /// Latest pointer position seen on the SceneView (screen-space).
    /// Updated via an on_pointer_event handler in `build`. Used by
    /// Ctrl+wheel zoom to zoom-about-pointer instead of zoom-about-
    /// viewport-center, which is the natural feel users expect (the
    /// scene point under the cursor stays put).
    /// `None` until the first pointer event arrives — Ctrl+wheel
    /// before any pointer event falls back to viewport center.
    cursor_pos: Rc<Cell<Option<Point>>>,

    /// App-supplied focus-order callback. When set, the public
    /// [`next_focus`](Self::next_focus) /
    /// [`previous_focus`](Self::previous_focus) accessors route
    /// through it instead of falling back to insertion order.
    /// `Rc<dyn Fn>` so callers can clone the SceneView while
    /// keeping the closure shared.
    focus_order_callback:
        Option<Rc<dyn Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId>>>,

    /// Whether this SceneView is logically nested inside another
    /// (chart-style outer chrome + inner data scene, or a preview
    /// pane inside a parent scene). Default `false` — every
    /// SceneView reports itself as a top-level `Role::Pane`. When
    /// `true`, the AT walker reports `Role::Region` instead so
    /// screen readers don't announce redundant landmarks.
    a11y_nested: bool,
    /// Optional label announced as the SceneView's own AT name.
    /// When set, becomes the logical region name (e.g. "Chart
    /// data area" for an inner chart SceneView). Default `None`
    /// — the SceneView has no explicit name.
    a11y_label: Option<LocalizedString>,
    /// Coordinate space for `SceneItem` bounds reported to AT.
    /// Default `Screen` (view-projected). Apps with a logical
    /// fixed coordinate system (CAD canvases, blueprint editors)
    /// may want `Scene` so AT users can reason about "where in
    /// the design" an item sits, independent of the current
    /// pan/zoom.
    a11y_bounds_space: crate::a11y::A11yBoundsSpace,
    /// Debug overlay configuration. Default: all flags `false`
    /// — no debug paint. When any flag is set, the SceneView
    /// paints visual diagnostics (item bounding boxes,
    /// content extent, viewport rect, etc.) on top of normal
    /// scene rendering. Use to verify culling, hit-test, and
    /// spatial-index behavior; intended for development only,
    /// don't ship with this on.
    debug_overlay: DebugOverlay,

    // --- Cached derived signals ---------------------------------------
    /// `view_transform` as a derived `Signal<Transform2D>`,
    /// constructed once in `new()` and reused across rebuilds.
    /// Exposed via [`view_transform_signal`](Self::view_transform_signal)
    /// so consumers (e.g. axis labels in a parent SceneView) can
    /// bind to it reactively without taking a snapshot every paint.
    view_transform_signal: Signal<Transform2D>,

    // --- Background / foreground paint hooks --------------------------
    /// App-supplied closure painted **before** the items walk. The
    /// canvas already has the view-transform scope pushed, so the
    /// closure paints in scene coords. The `Rect` argument is the
    /// scene-coord visible region — useful for "every-N-units" tiled
    /// backgrounds (graph-paper grids, ruled lines, dot grids) so the
    /// closure only emits geometry the user can actually see.
    background_paint: Option<Rc<dyn Fn(&mut bastyde_canvas::Canvas, &PaintContext, Rect)>>,
    /// App-supplied closure painted **after** the items walk and the
    /// marquee, but before the debug overlay. Same coordinate
    /// conventions as `background_paint`. Used for scene-coord
    /// chrome that should ride over content (rulers, snap-line
    /// indicators, drop hints).
    foreground_paint: Option<Rc<dyn Fn(&mut bastyde_canvas::Canvas, &PaintContext, Rect)>>,

    // --- Item-coordinate paint cache ----------------------------------
    /// Per-item paint cache for items that opted into
    /// [`CacheMode::ItemCoordinate`](crate::cache::CacheMode::ItemCoordinate).
    /// Keyed by `ItemId`; the entry stores a [`RenderFrame`](bastyde_canvas::RenderFrame)
    /// recorded in the item's local coordinates and replayed via
    /// `Canvas::draw_render_frame` when valid. Invalidated by an
    /// observer on [`Scene::item_change_signal`](crate::Scene::item_change_signal):
    /// `LocalBoundsChanged` / `OpacityChanged` / `Removed` for an id
    /// drop that id's entry. Apps that mutate item-internal state
    /// outside of `Scene` mutators must call
    /// [`SceneView::invalidate_item_cache`] to evict.
    pub(crate) item_cache: Rc<RefCell<crate::cache::ItemCoordinateCache>>,
    /// RAII guard for the cache-invalidation observer wired in
    /// `build()`. Held by `Self` so the observer's lifetime tracks
    /// the SceneView's; dropping it on a fresh `build()` un-installs
    /// the previous observer before re-installing.
    _item_cache_observer: RefCell<Option<bastyde_core::signal::ObserverHandle>>,
    /// RAII guard for the logical-AT-structure observer wired in `build()`.
    /// Held by `Self` so a re-build un-installs the previous observer before
    /// re-installing. Drives a reconcile pass on `Scene::a11y_change_signal`
    /// (group / parent / relation / live / landmark / category mutations),
    /// which don't flow through `item_change_signal`.
    _a11y_observer: RefCell<Option<bastyde_core::signal::ObserverHandle>>,
    /// [`Scene::mutation_version`] as of the end of the build that last
    /// requested an AccessKit re-walk. `None` until the first build. `build()`
    /// re-walks AT only when the version has advanced past this since the last
    /// walk (a structural / geometry / a11y mutation), so a `build()` driven
    /// purely by per-frame dynamic-bounds churn does not re-walk AT 60×/s.
    last_at_version: Option<u64>,
    /// Whether [`Scene::refresh_dynamic_bounds`] reported a change on the
    /// *previous* build. The `true → false` edge (an animation settling) walks
    /// the final animated bounds into AT once — the one AT update the
    /// version-delta gate would otherwise miss while suppressing the churn.
    dynamic_churning: bool,
}

impl std::fmt::Debug for SceneView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manual impl: `focus_order_callback` is `Rc<dyn Fn>` and
        // therefore not `Debug`. Render it as a presence flag instead.
        f.debug_struct("SceneView")
            .field("model", &self.model)
            .field("materialized_count", &self.materialized.len())
            .field("default_size", &self.default_size)
            .field("interactive", &self.interactive)
            .field("zoom_range_override", &self.zoom_range_override.get())
            .field("pan_bounds_override", &self.pan_bounds_override.get())
            .field("a11y_mode", &self.a11y_mode)
            .field("a11y_off_screen_mode", &self.a11y_off_screen_mode)
            .field("selection_mode", &self.selection.mode())
            .field("focus_order_callback", &self.focus_order_callback.is_some())
            .field("a11y_nested", &self.a11y_nested)
            .field("a11y_label", &self.a11y_label)
            .field("a11y_bounds_space", &self.a11y_bounds_space)
            .field("debug_overlay", &self.debug_overlay)
            .finish_non_exhaustive()
    }
}

mod a11y_impl;
mod build_impl;
mod builder_impl;
mod camera_impl;
mod gestures_impl;
mod layout_impl;
mod paint_impl;
mod widget_trait;

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
mod tests;
