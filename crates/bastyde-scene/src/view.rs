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
use crate::transform::{anchor_pan_for_pinch, compose_view};

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
    drag_dirty: Signal<u64>,

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
    /// SceneView reports itself as a top-level [`Role::Pane`]. When
    /// `true`, the AT walker reports [`Role::Region`] instead so
    /// screen readers don't announce redundant landmarks.
    a11y_nested: bool,
    /// Optional label announced as the SceneView's own AT name.
    /// When set, becomes the logical region name (e.g. "Chart
    /// data area" for an inner chart SceneView). Default `None`
    /// — the SceneView has no explicit name.
    a11y_label: Option<bastyde_i18n::LocalizedString>,
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
    /// [`Canvas::draw_render_frame`] when valid. Invalidated by an
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
}

impl std::fmt::Debug for SceneView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manual impl: `focus_order_callback` is `Rc<dyn Fn>` and
        // therefore not `Debug`. Render it as a presence flag instead.
        f.debug_struct("SceneView")
            .field("scene", &self.scene)
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
        // `set_content_transform` in `build` and exposed publicly via
        // [`view_transform_signal`](Self::view_transform_signal).
        let view_transform_signal = pan_x
            .zip3(&pan_y, &zoom)
            .zip(&rotation)
            .zip(&bounds_origin_signal)
            // Coalesce the five underlying sources into one. Without
            // this, every animation tick that updates pan/zoom/rotation
            // simultaneously would register five binding entries per
            // observing widget, multiplying the per-tick dirty-poll
            // work. `map_coalesced` collapses to a single composite
            // source with the same dirty-on-any / clear-all semantics.
            .map_coalesced(|(((px, py, z), r), bo)| {
                compose_view(Vec2::new(*px + bo.x, *py + bo.y), *z, *r)
            });
        Self {
            scene,
            materialized: HashMap::new(),
            widget_to_item: HashMap::new(),
            default_size: Size::new(800.0, 600.0),
            adopt_scene_size: false,
            drag_mode: Signal::new(crate::item_handlers::DragMode::RubberBand),
            handler_snapshot: Rc::new(RefCell::new(Vec::new())),
            hovered_item: Rc::new(Cell::new(None)),
            pending_tap: Rc::new(Cell::new(None)),
            last_viewport: Signal::new(Size::new(800.0, 600.0)),
            pan_x,
            pan_y,
            zoom,
            rotation,
            bounds_origin_signal,
            zoom_range_override: Signal::new(Some(DEFAULT_MIN_ZOOM..=DEFAULT_MAX_ZOOM)),
            pan_bounds_override: Signal::new(None),
            pan_anim_duration: DEFAULT_PAN_DURATION,
            zoom_anim_duration: DEFAULT_ZOOM_DURATION,
            line_height: DEFAULT_LINE_HEIGHT,
            overscroll_behavior: OverscrollBehavior::Chain,
            a11y_off_screen_mode: crate::a11y::A11yOffScreenMode::default(),
            a11y_mode: crate::a11y::A11yMode::default(),
            self_widget_id: Cell::new(None),
            interactive: true,
            view_transform_signal,
            selection: crate::selection::SceneSelection::new(
                crate::selection::SceneSelectionMode::None,
            ),
            marquee: Rc::new(Cell::new(None)),
            pending_marquee_commit: Rc::new(Cell::new(None)),
            drag_target: Rc::new(Cell::new(None)),
            pending_item_move: Rc::new(Cell::new(None)),
            lightweight_bounds_snapshot: Rc::new(RefCell::new(Vec::new())),
            drag_dirty: Signal::new(0),
            cursor_pos: Rc::new(Cell::new(None)),
            focus_order_callback: None,
            a11y_nested: false,
            a11y_label: None,
            a11y_bounds_space: crate::a11y::A11yBoundsSpace::default(),
            debug_overlay: DebugOverlay::default(),
            background_paint: None,
            foreground_paint: None,
            item_cache: Rc::new(RefCell::new(crate::cache::ItemCoordinateCache::new())),
            _item_cache_observer: RefCell::new(None),
        }
    }

    /// Configure selection behavior. Default
    /// [`SceneSelectionMode::None`](crate::SceneSelectionMode::None) —
    /// click and marquee do nothing. Set to `Single` for
    /// at-most-one selection (click replaces) or `Multi` for
    /// multi-select with marquee box-select, Ctrl+click toggle,
    /// and Ctrl+drag additive marquee.
    pub fn selection_mode(mut self, mode: crate::selection::SceneSelectionMode) -> Self {
        self.selection = crate::selection::SceneSelection::new(mode);
        self
    }

    /// Borrow the SceneView's [`SceneSelection`](crate::SceneSelection).
    /// Use this from external code to bind to the selection signal,
    /// query selected ids, or call `select_one` / `clear` /
    /// `replace` programmatically.
    pub fn selection(&self) -> &crate::selection::SceneSelection {
        &self.selection
    }

    /// Drain any pending marquee commit synchronously. Normal
    /// per-frame use never needs this — `place_children` consumes
    /// the pending commit at the start of every layout pass. Tests
    /// that drive on_drag without a follow-up layout call this to
    /// materialise the box-select result.
    pub fn flush_marquee_commit(&self) -> bool {
        if let Some((rect, additive)) = self.pending_marquee_commit.take() {
            self.selection.commit_marquee(&self.scene, rect, additive);
            self.marquee.set(None);
            true
        } else {
            false
        }
    }

    /// Drain any pending drag-to-move commit by translating the
    /// dragged item's `local_pos` by the queued delta. Descendants
    /// follow automatically: their `local_pos` is unchanged but
    /// their `scene_pos` derives from the parent's chain.
    pub fn flush_pending_item_move(&mut self) -> bool {
        if let Some((target_id, delta)) = self.pending_item_move.take() {
            if let Some(local_pos) = self.scene.local_pos(target_id) {
                let new_local_pos = Point::new(local_pos.x + delta.x, local_pos.y + delta.y);
                self.scene.set_local_pos(target_id, new_local_pos);
            }
            self.drag_target.set(None);
            true
        } else {
            false
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

    /// Mark this SceneView as logically nested inside another
    /// SceneView. Affects only the AT walker — the inner
    /// SceneView reports [`Role::Region`] instead of the default
    /// [`Role::Pane`], so screen readers don't announce a
    /// redundant top-level landmark for what's logically a sub-
    /// region. Pair with [`a11y_label`](Self::a11y_label) to give
    /// the inner region a useful announce name.
    ///
    /// Use case: chart-style nested scenes (outer SceneView holds
    /// axis chrome, inner SceneView holds data) — the inner one
    /// should announce as "Data area" or similar, not as another
    /// "Pane" sibling to the outer.
    ///
    /// Default `false`. Apps explicitly set this when they know
    /// they're nesting; the framework doesn't introspect the
    /// widget tree to detect nesting automatically (deliberately
    /// kept declarative — the visual layout doesn't always match
    /// logical nesting).
    pub fn nested_a11y(mut self, nested: bool) -> Self {
        self.a11y_nested = nested;
        self
    }

    /// Set the AT label announced as this SceneView's own name.
    /// Particularly useful for nested SceneViews via
    /// [`nested_a11y`](Self::nested_a11y), where the inner
    /// region should have a domain-specific name (e.g. "Chart
    /// data area"). Default `None` — the SceneView has no
    /// explicit AT name.
    pub fn a11y_label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.a11y_label = Some(ls);
        self
    }

    /// Whether the SceneView is currently marked as logically
    /// nested. Read-only accessor for tests / diagnostics.
    pub fn is_nested(&self) -> bool {
        self.a11y_nested
    }

    /// Coordinate space for `SceneItem` bounds reported to AT.
    /// Default [`A11yBoundsSpace::Screen`](crate::A11yBoundsSpace::Screen)
    /// (view-projected, matches the framework's standard widget
    /// behavior). Switch to
    /// [`A11yBoundsSpace::Scene`](crate::A11yBoundsSpace::Scene) for
    /// apps where AT users reason about scene topology rather than
    /// viewport position (CAD canvases, blueprint editors).
    pub fn a11y_bounds_space(mut self, space: crate::a11y::A11yBoundsSpace) -> Self {
        self.a11y_bounds_space = space;
        self
    }

    /// Read-only accessor for the configured a11y bounds space.
    pub fn current_a11y_bounds_space(&self) -> crate::a11y::A11yBoundsSpace {
        self.a11y_bounds_space
    }

    /// Configure visual debug overlays. Default: all flags off.
    /// Pass [`DebugOverlay::ALL`] to enable every overlay or
    /// construct a custom config:
    ///
    /// ```ignore
    /// SceneView::new(scene)
    ///     .debug_overlay(DebugOverlay {
    ///         item_bounds: true,
    ///         viewport: true,
    ///         ..Default::default()
    ///     });
    /// ```
    ///
    /// Intended for development only — overlay paint is cheap but
    /// not free; ship with the default (off) config.
    pub fn debug_overlay(mut self, overlay: DebugOverlay) -> Self {
        self.debug_overlay = overlay;
        self
    }

    /// Read-only accessor for the active debug overlay config.
    pub fn current_debug_overlay(&self) -> DebugOverlay {
        self.debug_overlay
    }

    /// Install a custom focus-order callback. When set,
    /// [`next_focus`](Self::next_focus) /
    /// [`previous_focus`](Self::previous_focus) route through the
    /// closure instead of falling back to scene insertion order.
    ///
    /// Apps wire this to a Tab / Shift+Tab handler in their root
    /// shortcut/action map. Typical implementations:
    ///
    /// - **Graph editor:** walk outgoing-port connections from the
    ///   current node, return the connected-node `ItemId`.
    /// - **Corkboard with Acts:** walk a parallel `BTreeMap<ActId,
    ///   Vec<CardId>>` declared by the app and Tab through cards in
    ///   story order, not reading order.
    /// - **Timeline:** sort items by `start_time`, return the next.
    ///
    /// The callback receives the full [`Scene`] (read-only), the
    /// requested [`FocusDirection`], and the currently focused item
    /// (`None` on the first Tab into the scene). Return `None` to
    /// signal "no next item" (the framework can then advance focus
    /// outside the SceneView).
    ///
    /// Calling [`next_focus`](Self::next_focus) /
    /// [`previous_focus`](Self::previous_focus) without a callback
    /// installed walks scene insertion order — adequate for simple
    /// scenes; replace as needed.
    pub fn focus_order<F>(mut self, callback: F) -> Self
    where
        F: Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId> + 'static,
    {
        self.focus_order_callback = Some(Rc::new(callback));
        self
    }

    /// Compute the next item the focus should advance to in the
    /// given direction. If a [`focus_order`](Self::focus_order)
    /// callback is installed, routes through it; otherwise falls
    /// back to scene insertion order — `Forward` returns the item
    /// after `current` (or the first if `current` is `None`),
    /// `Backward` returns the previous (or the last if `current`
    /// is `None`).
    pub fn focus_in_direction(
        &self,
        direction: FocusDirection,
        current: Option<ItemId>,
    ) -> Option<ItemId> {
        if let Some(cb) = &self.focus_order_callback {
            return cb(&self.scene, direction, current);
        }
        let ids = self.scene.ids();
        if ids.is_empty() {
            return None;
        }
        match (direction, current) {
            (FocusDirection::Forward, None) => ids.first().copied(),
            (FocusDirection::Backward, None) => ids.last().copied(),
            (FocusDirection::Forward, Some(cur)) => ids
                .iter()
                .position(|id| *id == cur)
                .and_then(|i| ids.get(i + 1).copied()),
            (FocusDirection::Backward, Some(cur)) => {
                ids.iter().position(|id| *id == cur).and_then(|i| {
                    if i == 0 {
                        None
                    } else {
                        ids.get(i - 1).copied()
                    }
                })
            }
        }
    }

    /// Convenience: forward-Tab traversal. See
    /// [`focus_in_direction`](Self::focus_in_direction).
    pub fn next_focus(&self, current: Option<ItemId>) -> Option<ItemId> {
        self.focus_in_direction(FocusDirection::Forward, current)
    }

    /// Convenience: backward-Tab (Shift+Tab) traversal. See
    /// [`focus_in_direction`](Self::focus_in_direction).
    pub fn previous_focus(&self, current: Option<ItemId>) -> Option<ItemId> {
        self.focus_in_direction(FocusDirection::Backward, current)
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
        if self.last_viewport.get() != self.default_size {
            self.last_viewport.set(self.default_size);
        }
        self
    }

    /// When set, the view's `layout_response` returns the scene's
    /// `scene_rect_extent` as its own wanted size — the view sizes
    /// itself to its scene rather than to `default_size`. Pairs
    /// naturally with [`Scene::pan_axes`] / [`Scene::zoomable`]
    /// to embed bounded, non-navigable scenes inline (mini diagrams,
    /// fixed corkboards). Default `false`.
    pub fn adopt_scene_size(mut self, on: bool) -> Self {
        self.adopt_scene_size = on;
        self
    }

    /// Configure how left-mouse drag-on-canvas behaves. Default
    /// [`DragMode::RubberBand`](crate::DragMode) — drag-on-an-item
    /// moves it (when `IS_DRAGGABLE`), drag-on-empty-space creates
    /// a marquee. [`DragMode::ScrollHandDrag`] makes left-drag
    /// pan the view unconditionally; [`DragMode::NoDrag`] disables
    /// the on-drag handler entirely.
    pub fn drag_mode(self, mode: crate::item_handlers::DragMode) -> Self {
        self.drag_mode.set(mode);
        self
    }

    /// Replace the view's internal drag-mode signal with an
    /// app-owned one — so a toolbar widget can hold the same
    /// `Signal<DragMode>` and toggle Hand vs Select vs NoDrag
    /// at runtime without touching the SceneView directly.
    ///
    /// After binding, the view's own `drag_mode(...)` setter
    /// writes to the shared signal too. To stop sharing, call
    /// `bind_drag_mode(Signal::new(mode))` with a fresh signal.
    pub fn bind_drag_mode(mut self, sig: Signal<crate::item_handlers::DragMode>) -> Self {
        self.drag_mode = sig;
        self
    }

    /// Reactive accessor for the drag mode. Useful for toolbars
    /// that need to read the current mode (e.g. to highlight the
    /// active tool button) and write to it.
    pub fn drag_mode_signal(&self) -> Signal<crate::item_handlers::DragMode> {
        self.drag_mode.clone()
    }

    /// Install a closure painted **before** the items walk. The
    /// canvas already has the view-transform scope pushed, so the
    /// closure paints in scene coords. The `Rect` argument is the
    /// scene-coord visible region — useful for tiled backgrounds
    /// (graph-paper grids, ruled lines, dot grids) so the closure
    /// only emits geometry the user can actually see.
    ///
    /// ```ignore
    /// SceneView::new(scene).background(|canvas, _ctx, region| {
    ///     // Draw a 50-unit grid covering only the visible region.
    ///     let step = 50.0;
    ///     let x0 = (region.x / step).floor() * step;
    ///     let mut x = x0;
    ///     while x < region.x + region.width {
    ///         canvas.draw_line(/* ... */);
    ///         x += step;
    ///     }
    /// })
    /// ```
    pub fn background<F>(mut self, paint: F) -> Self
    where
        F: Fn(&mut bastyde_canvas::Canvas, &PaintContext, Rect) + 'static,
    {
        self.background_paint = Some(Rc::new(paint));
        self
    }

    /// Install a closure painted **after** the items walk and the
    /// marquee, but before any debug overlay. Same coordinate
    /// conventions as [`background`](Self::background). Used for
    /// scene-coord chrome that should ride over content (rulers,
    /// snap-line indicators, drop hints).
    pub fn foreground<F>(mut self, paint: F) -> Self
    where
        F: Fn(&mut bastyde_canvas::Canvas, &PaintContext, Rect) + 'static,
    {
        self.foreground_paint = Some(Rc::new(paint));
        self
    }

    /// Drop the cached paint output for `id`. Apps that mutate
    /// item-internal state without going through a [`Scene`] mutator
    /// (e.g. a custom item whose paint depends on a private
    /// `Signal<Color>` that doesn't drive `local_bounds`) call this
    /// to invalidate. The cache is otherwise dropped automatically
    /// on `LocalBoundsChanged` / `OpacityChanged` / `Removed`.
    pub fn invalidate_item_cache(&self, id: ItemId) {
        self.item_cache.borrow_mut().evict(id);
    }

    /// Number of cached entries currently held. Diagnostic / test
    /// hook — apps shouldn't normally need this.
    pub fn item_cache_len(&self) -> usize {
        self.item_cache.borrow().len()
    }

    /// Minimum zoom factor (default 0.1×). Applied as a clamp to all
    /// programmatic and gesture-driven zoom changes via the
    /// view-level [`zoom_range_override`](Self::zoom_range_override).
    /// Shim — updates the lower bound of the override range. The
    /// effective clamp is the intersection of Scene-level
    /// [`Scene::set_zoom_range`](crate::Scene::set_zoom_range) and
    /// this override (tightening-only — neither side can loosen).
    pub fn min_zoom(self, v: f32) -> Self {
        let lo = v.max(0.0001);
        let current = self.zoom_range_override.get();
        let hi = current
            .as_ref()
            .map(|r| *r.end())
            .unwrap_or(DEFAULT_MAX_ZOOM);
        self.zoom_range_override.set(Some(lo..=hi.max(lo)));
        self
    }

    /// Maximum zoom factor (default 10×). Shim — updates the upper
    /// bound of the override range. See [`min_zoom`](Self::min_zoom).
    pub fn max_zoom(self, v: f32) -> Self {
        let current = self.zoom_range_override.get();
        let lo = current
            .as_ref()
            .map(|r| *r.start())
            .unwrap_or(DEFAULT_MIN_ZOOM);
        self.zoom_range_override.set(Some(lo..=v.max(lo)));
        self
    }

    /// Replace the view-level zoom-range override wholesale.
    /// `None` clears the override so this view imposes no zoom
    /// clamp of its own (Scene-level constraints still apply).
    /// Tightening rule: the effective clamp is the intersection
    /// with `Scene::current_zoom_range()` — neither can loosen.
    pub fn zoom_range_override(self, range: Option<std::ops::RangeInclusive<f32>>) -> Self {
        self.zoom_range_override.set(range);
        self
    }

    /// Reactive accessor for the view-level zoom-range override.
    /// Use this to mutate the override at runtime (e.g. from a
    /// toolbar). Mutations take effect on the next gesture.
    pub fn zoom_range_override_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>> {
        self.zoom_range_override.clone()
    }

    /// View-level *tightening* override on pan bounds, in scene
    /// coords. The effective clamp at gesture-time is the rect
    /// intersection with `Scene::current_pan_bounds()` — view
    /// overrides cannot loosen what the `Scene` declares. `None`
    /// (default) means no view-side clamp.
    pub fn pan_bounds_override(self, bounds: Option<Rect>) -> Self {
        self.pan_bounds_override.set(bounds);
        self
    }

    /// Reactive accessor for the view-level pan-bounds override.
    /// Use this to mutate the override at runtime (e.g. dynamically
    /// shrinking the navigable area). Mutations take effect on the
    /// next gesture.
    pub fn pan_bounds_override_signal(&self) -> Signal<Option<Rect>> {
        self.pan_bounds_override.clone()
    }

    /// Logical pixels of pan applied per scroll-wheel line notch.
    /// Defaults to 16 px (matches `ScrollArea`).
    pub fn line_height(mut self, px: f32) -> Self {
        self.line_height = px.max(0.0);
        self
    }

    /// Whether a wheel the scene can't absorb (already clamped at its
    /// `pan_bounds`) chains to an ancestor scrollable
    /// ([`OverscrollBehavior::Chain`], the default — matches the widget
    /// scrollables) or is contained ([`OverscrollBehavior::Contain`]). Use
    /// `Contain` for a tightly-bounded scene embedded in a scroll view that
    /// should never steal the scene's wheel.
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// Read access to the underlying scene model.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Mutable access to the underlying scene model. Intended for
    /// pre-build configuration or runtime mutation
    /// after `SceneView` has been added to the tree, fresh
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

    /// Paint one lightweight band into `canvas`, under the active view
    /// transform. Queries the visible items, z-sorts them, keeps only those
    /// in `band`, and paints each. Heavyweight ids are skipped (they paint
    /// via the arena walker). Shared by [`paint`](Self::paint) (the `Under`
    /// backdrop, before the children) and [`post_paint`](Self::post_paint)
    /// (the `Over` foreground, after the children). Within a band, `z`
    /// orders items among themselves.
    fn paint_band(
        &self,
        canvas: &mut bastyde_canvas::Canvas,
        bounds: Rect,
        band: crate::scene::SceneLayer,
    ) {
        let region = self.visible_scene_region(bounds);
        let view_transform = self.view_transform();
        let item_ctx = crate::item::SceneItemPaintContext::new(view_transform, Some(region));
        let drag_target = self.drag_target.get();
        let mut visible_ids = self.scene.items_in_rect(region);
        // Z-order within the band: higher z paints last (on top); equal-z
        // preserves insertion order (stable sort). Heavyweight ids stay in the
        // list but are skipped below — they paint via the arena walker.
        self.scene.sort_by_z(&mut visible_ids);
        for id in visible_ids {
            if self.scene.item(id).is_none() {
                continue;
            }
            // Only this band; items default to Under.
            if self
                .scene
                .layer(id)
                .unwrap_or(crate::scene::SceneLayer::Under)
                != band
            {
                continue;
            }
            // Skip items whose chain is invisible or which carry the
            // HAS_NO_CONTENTS flag (logical-only).
            if !self.scene.is_effectively_visible(id) {
                continue;
            }
            let flags = self.scene.flags(id).unwrap_or_default();
            if flags.contains(crate::flags::ItemFlags::HAS_NO_CONTENTS) {
                continue;
            }
            // Items that are the drag target or a declared descendant paint
            // with a visual delta in scene coords — a child follows its
            // dragged parent until the rebuild commits the new local_pos.
            let drag_delta = drag_target
                .filter(|t| t.item_id == id || self.scene.is_descendant_of(id, t.item_id))
                .map(|t| {
                    bastyde_canvas::Transform2D::translate(
                        t.current_scene.x - t.anchor_scene.x,
                        t.current_scene.y - t.anchor_scene.y,
                    )
                });

            // Compose `local→scene`, optionally with a scene-coord drag
            // offset baked in. Push beneath the view transform so the item's
            // `paint` works in local coords. `save` / `restore` isolate
            // neighbouring items' transforms.
            let mut local_to_scene = self.scene.scene_transform(id);
            if let Some(t) = drag_delta {
                local_to_scene = local_to_scene.then(&t);
            }
            canvas.save();
            // IGNORES_TRANSFORMATIONS items pin at their parent-relative
            // position but render at a fixed pixel size (Qt's
            // `ItemIgnoresTransformations`). Project the anchor through the
            // parent chain + view transform, then push a transform that —
            // composed with the outer view transform on the canvas —
            // collapses to a pure `Translate(screen_anchor)`.
            if flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS) {
                let scene_anchor = local_to_scene.apply_point(Point::ZERO);
                let screen_anchor = view_transform.apply_point(scene_anchor);
                let view_inv = view_transform
                    .inverse()
                    .unwrap_or_else(Transform2D::identity);
                let t = Transform2D::translate(screen_anchor.x, screen_anchor.y).then(&view_inv);
                canvas.apply_transform(t);
            } else {
                canvas.apply_transform(local_to_scene);
            }
            // Effective opacity composes through the parent chain. Pushed via
            // `set_opacity` / `restore_opacity` so the scope is balanced.
            let alpha = self.scene.effective_opacity(id);
            let opacity_pushed = alpha < 0.999;
            if opacity_pushed {
                canvas.set_opacity(alpha);
            }
            if let Some(item) = self.scene.item(id) {
                // Item-coordinate cache: replay a cached local-coord
                // RenderFrame instead of re-running paint when the item opted
                // into `CacheMode::ItemCoordinate`; record on a miss.
                match item.cache_mode() {
                    crate::cache::CacheMode::ItemCoordinate => {
                        let cached = self.item_cache.borrow().get(id).cloned();
                        if let Some(frame) = cached {
                            canvas.draw_render_frame(&frame, Point::ZERO);
                        } else {
                            let mut sub = match canvas.text_backend() {
                                Some(tb) => bastyde_canvas::Canvas::with_text_backend(tb.clone()),
                                None => bastyde_canvas::Canvas::new(),
                            };
                            item.paint(&mut sub, &item_ctx);
                            let frame = sub.into_render_frame();
                            canvas.draw_render_frame(&frame, Point::ZERO);
                            self.item_cache.borrow_mut().insert(id, frame);
                        }
                    }
                    crate::cache::CacheMode::None => {
                        item.paint(canvas, &item_ctx);
                    }
                }
            }
            if opacity_pushed {
                canvas.restore_opacity();
            }
            canvas.restore();
        }
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
        if !self.scene.is_zoomable() || self.adopt_scene_size {
            return;
        }
        let clamped = self.gate_zoom_target(target);
        self.zoom.animate_to(clamped, duration, Easing::EaseOut);
    }

    /// Snap zoom to `target` without animation, clamped. No-op when
    /// the scene declares zoom disabled.
    pub fn set_zoom(&self, target: f32) {
        if !self.scene.is_zoomable() || self.adopt_scene_size {
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
        use crate::scene::PanAxes;
        if self.adopt_scene_size {
            return Vec2::new(self.pan_x.get(), self.pan_y.get());
        }
        let axes = self.scene.current_pan_axes();
        let after_axes = match axes {
            PanAxes::Both => target,
            PanAxes::None => Vec2::new(self.pan_x.get(), self.pan_y.get()),
            PanAxes::Horizontal => Vec2::new(target.x, self.pan_y.get()),
            PanAxes::Vertical => Vec2::new(self.pan_x.get(), target.y),
        };
        clamp_pan_to_bounds(
            after_axes,
            self.effective_pan_bounds().as_ref(),
            self.last_viewport.get(),
            self.zoom.get(),
        )
    }

    /// Clamp `zoom` to the effective zoom range (intersection of
    /// Scene-declared `zoom_range` and view-level `zoom_range_override`).
    /// `None` on either side means "no constraint from that side";
    /// when both are `None` this is the identity. Tightening-only —
    /// neither side can loosen what the other imposes.
    fn gate_zoom_target(&self, zoom: f32) -> f32 {
        clamp_zoom(zoom, self.effective_zoom_range().as_ref())
    }

    /// Effective zoom range = intersect(Scene declared, view override).
    fn effective_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>> {
        intersect_zoom_range(
            self.scene.current_zoom_range().as_ref(),
            self.zoom_range_override.get().as_ref(),
        )
    }

    /// Effective pan bounds = intersect(Scene declared, view override).
    fn effective_pan_bounds(&self) -> Option<Rect> {
        intersect_pan_bounds(
            self.scene.current_pan_bounds(),
            self.pan_bounds_override.get(),
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
        let ids: Vec<ItemId> = self.scene.ids();
        union_rects(ids.iter().filter_map(|id| self.scene.scene_rect(*id)))
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
        let union = union_rects(ids.iter().filter_map(|id| self.scene.scene_rect(*id)));
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

impl Widget for SceneView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Drain any pending drag-to-move commit. The drop closure
        // queued `(target_id, delta)` and bumped `drag_dirty` which
        // flagged this widget for rebuild. Translate the dragged
        // item's `local_pos` by the queued delta — descendants
        // follow automatically because their `local_pos` is
        // unchanged but their `scene_pos` derives from the parent
        // chain. Clear `drag_target` here (not on `Ended`) so paint
        // keeps translating the item to its dragged position until
        // the move actually lands; otherwise the item would visibly
        // "snap back" between drag-end and the rebuild.
        if let Some((target_id, delta)) = self.pending_item_move.take() {
            if let Some(local_pos) = self.scene.local_pos(target_id) {
                let new_local_pos = Point::new(local_pos.x + delta.x, local_pos.y + delta.y);
                self.scene.set_local_pos(target_id, new_local_pos);
            }
            self.drag_target.set(None);
        }

        // Drain any pending marquee commit posted by the on_drag
        // closure on its `Ended` branch, then clear the in-flight
        // marquee Cell so paint stops overlaying the rect. Without
        // this the lasso would linger on screen until something
        // else triggered a layout pass (next user drag, etc.).
        if let Some((rect, additive)) = self.pending_marquee_commit.take() {
            self.selection.commit_marquee(&self.scene, rect, additive);
            self.marquee.set(None);
        }

        // Pull fresh `local_bounds` for every item flagged
        // `dynamic_bounds` (added via [`Scene::add_item_dynamic`]).
        // Static items pay nothing here; dynamic items get their
        // signal-driven AABBs read back into the entry + spatial
        // index so hit-test and viewport-cull stay correct.
        self.scene.refresh_dynamic_bounds();

        // Bind the drag-rebuild signal so the next drop triggers a
        // rebuild and the drains above run. `BindingLevel::Rebuild`
        // is the level that re-runs `build()` on signal change.
        self.drag_dirty
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Wire the item-coordinate cache invalidation observer.
        // Cached frames are recorded in **local** coordinates, so
        // only changes that alter the local-coord paint output
        // dirty an entry: `LocalBoundsChanged` (geometry redraw)
        // and `Removed` (entry orphaned). Opacity, transform, z,
        // local_pos, flags don't bake into the cached frame —
        // they're applied as wrapping scopes at replay time.
        // The handle is held by `Self`; dropping the previous
        // handle on rebuild un-installs the prior observer before
        // re-installing.
        {
            let cache = self.item_cache.clone();
            let drag_dirty = self.drag_dirty.clone();
            let handle = self.scene.item_change_signal().observe(move |change| {
                use crate::scene::ItemChange;
                match *change {
                    ItemChange::LocalBoundsChanged { id, .. } | ItemChange::Removed { id } => {
                        cache.borrow_mut().evict(id);
                    }
                    // A z-change must restack the heavyweight arena children
                    // (only `build()` reorders them) and a band change moves a
                    // lightweight item between the paint / post_paint passes.
                    // Both take effect on rebuild, so bump the rebuild signal.
                    // Lightweight z is re-sorted every paint, so the rebuild is
                    // harmless (and cheap — children are preserved) there too.
                    ItemChange::ZChanged { .. } | ItemChange::LayerChanged { .. } => {
                        drag_dirty.set(drag_dirty.get().wrapping_add(1));
                    }
                    _ => {}
                }
            });
            *self._item_cache_observer.borrow_mut() = Some(handle);
        }

        // Materialise pending widgets (drained the first time, idempotent
        // afterwards). Also keeps the reverse lookup
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

        // Heavyweight z-order: sort the arena children by their scene-entry
        // z so higher-z cards paint later (on top). Equal-z keeps insertion
        // order (stable sort). This is the heavyweight-tier analogue of the
        // lightweight `sort_by_z` — `Scene::set_z` / `bring_to_front` on a
        // widget entry restacks the cards here, on the next rebuild.
        // Reordering `node.children` (rather than destroying / recreating the
        // widgets) preserves each card's focus, text-edit and animation state.
        child_ids.sort_by(|a, b| {
            let za = self
                .widget_to_item
                .get(a)
                .and_then(|id| self.scene.z(*id))
                .unwrap_or(0.0);
            let zb = self
                .widget_to_item
                .get(b)
                .and_then(|id| self.scene.z(*id))
                .unwrap_or(0.0);
            za.partial_cmp(&zb).unwrap_or(std::cmp::Ordering::Equal)
        });

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

        // Bind the four signals at Relayout on this node so
        // `place_children` re-runs and the viewport-cull set is
        // recomputed when pan/zoom/rotation change. The Repaint
        // binding from `set_content_transform` below is kept in addition;
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
        // `view_transform_signal()`). Bind it as a `set_content_transform`
        // scope on this widget; the render walker pushes it around
        // our entire subtree. The composition folds `bounds.origin`
        // into the final translate so a SceneView at a non-zero
        // parent offset still maps scene-coord (sx, sy) to screen
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        let self_id = ctx.self_id();
        // A *content* transform: the SceneView's bounds are a fixed screen
        // viewport and this pan/zoom only moves the scene content. Marking it
        // as such keeps the whole viewport hit-testable at any pan (a *self*
        // transform like Scale/Rotate would shift the hittable region with the
        // content — see `WidgetNode::content_transform`).
        ctx.set_content_transform(self_id, self.view_transform_signal.clone());
        // Capture for the AT-redirect auto-graft hook.
        // The hook is `&self`; without a stash here it has no way
        // to derive its own `WidgetId` to compute synthetic NodeIds.
        self.self_widget_id.set(Some(self_id));

        // Wire scroll + pinch handlers. Captures are by clone so they
        // outlive the build call. Reactive constraint signals
        // (pan_axes, zoom_range, pan_bounds, zoomable) are captured
        // as Signal clones — the closures read `.get()` per event,
        // so runtime mutations of the underlying signals take effect
        // on the next gesture without rebuilding the view.
        let prefers_reduced = ctx.prefers_reduced_motion();
        let line_height = self.line_height;
        let pan_dur = self.pan_anim_duration;
        let overscroll = self.overscroll_behavior;

        // Reusable tooltip surface for lightweight scene items. Items
        // have no `WidgetId`, so the per-widget `.tooltip()` attach path
        // (arena-hover-keyed, `NearAnchor`-positioned) can't be used.
        // Instead we keep ONE dormant `TooltipWidget` whose body is bound
        // to a `Signal<String>`; the hover seam below sets the text and
        // shows/dismisses it as a point-anchored (`AtPointer`) overlay,
        // mirroring how the per-item cursor override is applied. Resolving
        // the item's `LocalizedString` at show time keeps it locale-correct.
        let tooltip_text = ctx.signal(String::new());
        let tooltip_content_id =
            ctx.add(bastyde_widgets::TooltipWidget::bound(tooltip_text.clone()));
        ctx.set_dormant(tooltip_content_id);
        let tooltip_fade = if prefers_reduced {
            None
        } else {
            Some(ctx.theme().motion.duration_fast)
        };
        // Scene hover is exploratory — the pointer sweeps across many items
        // while the eye pans — so lightweight-item tips use the heavier
        // (longer) tooltip dwell to avoid flashing during that sweep.
        let tooltip_delay = ctx.theme().motion.tooltip_delay_heavy;

        let mut handlers = HandlerSet::new();

        // Track the latest pointer position so Ctrl+wheel can
        // zoom-about-pointer (the scene point under the cursor
        // stays put). Updated even when not interactive — the
        // outer SceneView in a nested chart still benefits from
        // knowing where the mouse is.
        //
        // Also flips the system cursor to `Move` whenever the
        // pointer is over a draggable lightweight item, and back
        // to `Default` otherwise. The visual hint matches the
        // user's affordance check ("can I grab this?") without
        // forcing app-side wiring.
        // Track the latest pointer position so Ctrl+wheel can
        // zoom-about-pointer (the scene point under the cursor
        // stays put). Updated even when not interactive — the
        // outer SceneView in a nested chart still benefits from
        // knowing where the mouse is.
        //
        // Also flips the system cursor based on the standard
        // grab/grabbing convention:
        //   - Pointer over a draggable item, no active drag → `Grab`
        //     (open hand, "you can pick this up")
        //   - Active drag in progress                       → `Grabbing`
        //     (closed fist, "you are holding it")
        //   - Anywhere else                                 → `Default`
        // Hover detection uses the same draggable-bounds snapshot
        // the on_drag::Started path consults, so the cursor and
        // the hit-test agree on what's draggable.
        {
            let cursor_pos = self.cursor_pos.clone();
            let bounds_snapshot = self.lightweight_bounds_snapshot.clone();
            let handler_snapshot = self.handler_snapshot.clone();
            let view_xform_signal = self.view_transform_signal.clone();
            let drag_target_for_cursor = self.drag_target.clone();
            let hovered_item = self.hovered_item.clone();
            let pending_tap = self.pending_tap.clone();
            let tooltip_text = tooltip_text.clone();
            let tooltip_content_id = tooltip_content_id;
            let tooltip_anchor_id = self_id;
            let tooltip_fade = tooltip_fade;
            let tooltip_delay = tooltip_delay;
            handlers = handlers.on_pointer_event(move |ev, ctx| {
                use bastyde_core::event::PointerButton;
                use bastyde_core::event::WidgetEvent as Ev;
                use bastyde_core::widget::CursorIcon;

                // Project a screen point to scene coords for
                // hit-testing. Returns `Point::ZERO` when the view
                // transform is degenerate.
                let to_scene = |p: Point| {
                    let xform = view_xform_signal.get();
                    xform
                        .inverse()
                        .map(|inv| inv.apply_point(p))
                        .unwrap_or(Point::ZERO)
                };

                // Hit-test the handler-snapshot for the topmost
                // item under the pointer. Snapshot is z-sorted desc.
                //
                // Normal items: broad-phase tests `scene_pt` against
                // `scene_rect`, narrow-phase inverse-projects to
                // local and calls `shape_contains`.
                //
                // IGNORES_TRANSFORMATIONS items: pin at a fixed
                // screen position with their natural local-pixel
                // size, so we project `scene_anchor` through the
                // CURRENT view transform (snapshot stores the
                // pan/zoom-invariant scene_anchor; the snapshot
                // doesn't rebuild on pan/zoom). Broad-phase tests
                // `screen_pt` against the projected screen rect;
                // narrow-phase passes `(screen_pt - screen_anchor)`
                // as the item-local point.
                let hit_handler_item =
                    |screen_pt: Point, scene_pt: Point| -> Option<HandlerSnapshotEntry> {
                        let snap = handler_snapshot.borrow();
                        let view_xform = view_xform_signal.get();
                        // Logical view zoom (uniform scale of the linear part) —
                        // passed to each item's shape-test so a cosmetic
                        // (device-pixel) stroke's clickable band is converted to
                        // scene coordinates at the current zoom.
                        let view_scale = view_xform.m[0].hypot(view_xform.m[1]);
                        for entry in snap.iter() {
                            if entry.ignores_xform {
                                let screen_anchor = view_xform.apply_point(entry.scene_anchor);
                                let screen_rect = Rect::new(
                                    screen_anchor.x + entry.local_bounds.x,
                                    screen_anchor.y + entry.local_bounds.y,
                                    entry.local_bounds.width,
                                    entry.local_bounds.height,
                                );
                                if !screen_rect.contains(screen_pt) {
                                    continue;
                                }
                                let local_pt = Point::new(
                                    screen_pt.x - screen_anchor.x,
                                    screen_pt.y - screen_anchor.y,
                                );
                                // Screen-anchored items ignore the view transform,
                                // so their hit-test runs at unit scale.
                                if (entry.shape_contains)(local_pt, 1.0) {
                                    return Some(entry.clone());
                                }
                                continue;
                            }
                            if !entry.scene_rect.contains(scene_pt) {
                                continue;
                            }
                            // Inverse-project to local for narrow-phase.
                            let local_pt = entry
                                .scene_transform
                                .inverse()
                                .map(|inv| inv.apply_point(scene_pt))
                                .unwrap_or(Point::ZERO);
                            if (entry.shape_contains)(local_pt, view_scale) {
                                return Some(entry.clone());
                            }
                        }
                        None
                    };

                match ev {
                    Ev::PointerMove { position, .. } => {
                        cursor_pos.set(Some(*position));
                        let scene_pt = to_scene(*position);

                        // Hover transitions: compare current hit
                        // with previously-hovered item; fire
                        // on_hover(false) on the old, on_hover(true)
                        // on the new.
                        let new_hit = hit_handler_item(*position, scene_pt);
                        let new_id = new_hit.as_ref().map(|e| e.id);
                        let prev_id = hovered_item.get();
                        if prev_id != new_id {
                            if let Some(prev) = prev_id
                                && let Some(prev_entry) =
                                    handler_snapshot.borrow().iter().find(|e| e.id == prev)
                                && let Some(h) = prev_entry.handlers.as_deref()
                                && let Some(cb) = h.on_hover.as_ref()
                            {
                                cb(false, ctx);
                            }
                            if let Some(new_entry) = new_hit.as_ref()
                                && let Some(h) = new_entry.handlers.as_deref()
                                && let Some(cb) = h.on_hover.as_ref()
                            {
                                cb(true, ctx);
                            }
                            hovered_item.set(new_id);

                            // Tooltip: retract the previous item's tooltip,
                            // then (re)schedule for the new item. We only
                            // dismiss the *shown* overlay here (active
                            // stack); `show_overlay_after` already replaces
                            // any stale *pending* show for the same content,
                            // so calling `cancel_delayed_overlay` as well
                            // would cancel the new show (drain order:
                            // delayed-requests apply before cancels).
                            ctx.dismiss_overlay_by_content(tooltip_content_id);
                            if let Some(ls) = new_hit
                                .as_ref()
                                .and_then(|e| e.handlers.as_deref())
                                .and_then(|h| h.tooltip.as_ref())
                            {
                                tooltip_text.set(ls.resolve_now());
                                ctx.show_overlay_after(
                                    bastyde_core::overlay::OverlayRequest {
                                        content_id: tooltip_content_id,
                                        anchor: tooltip_anchor_id,
                                        // Drop the tooltip just below-right
                                        // of the cursor so it doesn't sit
                                        // under the pointer.
                                        placement: bastyde_core::overlay::OverlayPlacement::AtPointer(
                                            Point::new(position.x + 12.0, position.y + 16.0),
                                        ),
                                        // Manual: every dismiss path (item
                                        // change, pointer-down, pointer-leave)
                                        // is driven explicitly below.
                                        dismiss: bastyde_core::overlay::DismissBehavior::Manual,
                                        layer: bastyde_core::overlay::OverlayLayer::InTree,
                                        parent_overlay: None,
                                        on_dismiss: None,
                                        fade_duration: tooltip_fade,
                                    },
                                    tooltip_delay,
                                );
                            } else {
                                // Moved onto an item with no tooltip (or onto
                                // empty space): cancel any pending show.
                                ctx.cancel_delayed_overlay(tooltip_content_id);
                            }
                        }

                        // Cursor: per-item override → grab/grabbing
                        // for draggable items → default.
                        let item_cursor = new_hit
                            .as_ref()
                            .and_then(|e| e.handlers.as_deref())
                            .and_then(|h| h.cursor);
                        let snap = bounds_snapshot.borrow();
                        let over_draggable = snap.iter().any(|(_, rect)| rect.contains(scene_pt));
                        let cursor = if drag_target_for_cursor.get().is_some() {
                            CursorIcon::Grabbing
                        } else if let Some(c) = item_cursor {
                            c
                        } else if over_draggable {
                            CursorIcon::Grab
                        } else {
                            CursorIcon::Default
                        };
                        ctx.set_cursor(cursor);
                    }
                    Ev::PointerDown {
                        position,
                        button,
                        modifiers,
                    } => {
                        cursor_pos.set(Some(*position));
                        // Any press retracts a hover tooltip (shown or
                        // pending) — the user has committed to an action.
                        ctx.cancel_delayed_overlay(tooltip_content_id);
                        ctx.dismiss_overlay_by_content(tooltip_content_id);
                        let scene_pt = to_scene(*position);
                        let hit = hit_handler_item(*position, scene_pt);
                        match button {
                            PointerButton::Secondary => {
                                if let Some(entry) = hit.as_ref()
                                    && let Some(h) = entry.handlers.as_deref()
                                    && let Some(cb) = h.on_context_menu.as_ref()
                                {
                                    let ev = crate::item_handlers::SceneTapEvent::new(
                                        scene_pt, *button, *modifiers,
                                    );
                                    cb(&ev, ctx);
                                    return EventResponse::Handled;
                                }
                            }
                            _ => {
                                // Gate on the item's accept_tap_buttons
                                // mask. PRIMARY is the default; items
                                // wanting middle-click-as-tap opt in
                                // via `accept_tap_buttons(...)`.
                                if let Some(entry) = hit.as_ref() {
                                    let accept = entry
                                        .handlers
                                        .as_deref()
                                        .map(|h| h.accept_tap_buttons)
                                        .unwrap_or(bastyde_core::event::ButtonMask::PRIMARY);
                                    if accept.contains(*button) {
                                        pending_tap.set(Some((scene_pt, entry.id, *button)));
                                    } else {
                                        pending_tap.set(None);
                                    }
                                } else {
                                    pending_tap.set(None);
                                }
                            }
                        }
                    }
                    Ev::PointerUp {
                        position,
                        button,
                        modifiers,
                    } => {
                        // Tap dispatch only fires when the button that
                        // came back up matches the one we recorded on
                        // the press. Mixed-button down/up sequences
                        // discard the pending tap.
                        if let Some((press_scene, item_id, press_button)) = pending_tap.take()
                            && press_button == *button
                        {
                            let scene_pt = to_scene(*position);
                            let dx = scene_pt.x - press_scene.x;
                            let dy = scene_pt.y - press_scene.y;
                            if (dx * dx + dy * dy).sqrt() <= TAP_MOVEMENT_THRESHOLD {
                                // Genuine tap — dispatch if the
                                // pressed item still has a tap
                                // handler installed.
                                if let Some(entry) =
                                    handler_snapshot.borrow().iter().find(|e| e.id == item_id)
                                    && let Some(h) = entry.handlers.as_deref()
                                    && let Some(cb) = h.on_tap.as_ref()
                                {
                                    let ev = crate::item_handlers::SceneTapEvent::new(
                                        scene_pt, *button, *modifiers,
                                    );
                                    cb(&ev, ctx);
                                    return EventResponse::Handled;
                                }
                            }
                        }
                    }
                    Ev::PointerLeave => {
                        cursor_pos.set(None);
                        // Pointer left the view entirely — retract the
                        // tooltip (shown or pending).
                        ctx.cancel_delayed_overlay(tooltip_content_id);
                        ctx.dismiss_overlay_by_content(tooltip_content_id);
                        // Clear any pending hover.
                        if let Some(prev) = hovered_item.take()
                            && let Some(prev_entry) =
                                handler_snapshot.borrow().iter().find(|e| e.id == prev)
                            && let Some(h) = prev_entry.handlers.as_deref()
                            && let Some(cb) = h.on_hover.as_ref()
                        {
                            cb(false, ctx);
                        }
                        pending_tap.set(None);
                        ctx.set_cursor(CursorIcon::Default);
                    }
                    _ => {}
                }
                EventResponse::Ignored
            });
        }

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
                let zoom = self.zoom.clone();
                let rotation = self.rotation.clone();
                let bounds_origin_for_scroll = self.bounds_origin_signal.clone();
                let last_viewport_for_scroll = self.last_viewport.clone();
                let cursor_pos_for_scroll = self.cursor_pos.clone();
                let pan_axes_sig = self.scene.pan_axes_signal();
                let zoomable_sig = self.scene.zoomable_signal();
                let scene_zoom_range_sig = self.scene.zoom_range_signal();
                let view_zoom_range_sig = self.zoom_range_override.clone();
                let scene_pan_bounds_sig = self.scene.pan_bounds_signal();
                let view_pan_bounds_sig = self.pan_bounds_override.clone();
                let adopt_scene_size = self.adopt_scene_size;
                handlers = handlers.on_scroll(move |event, _ctx| {
                    use crate::scene::PanAxes;
                    let WidgetEvent::Scroll { delta, modifiers } = event else {
                        return EventResponse::Ignored;
                    };
                    let (mut dx, mut dy) = match delta {
                        ScrollDelta::Pixels { x, y } => (*x, *y),
                        ScrollDelta::Lines { x, y } => (*x * line_height, *y * line_height),
                    };
                    // Apply the scene's pan-axes policy live: zero out
                    // the restricted axis so it passes through to ancestor
                    // scrollables instead of being absorbed.
                    match pan_axes_sig.get() {
                        PanAxes::Both => {}
                        PanAxes::None => {
                            dx = 0.0;
                            dy = 0.0;
                        }
                        PanAxes::Horizontal => {
                            dy = 0.0;
                        }
                        PanAxes::Vertical => {
                            dx = 0.0;
                        }
                    }
                    let zoomable = zoomable_sig.get() && !adopt_scene_size;
                    // Ctrl+wheel = zoom about the viewport center.
                    // Unmodified wheel / trackpad pan = pan the view.
                    if modifiers.ctrl() {
                        if !zoomable {
                            return EventResponse::Ignored;
                        }
                        // Zoom magnitude scales with vertical scroll
                        // distance. Sign convention: scroll up (negative
                        // ScrollDelta after platform negation) → zoom in.
                        // Pixels deltas are large; rescale so the
                        // step size matches one wheel notch.
                        let step_px = match delta {
                            ScrollDelta::Pixels { y, .. } => *y / 60.0,
                            ScrollDelta::Lines { y, .. } => *y,
                        };
                        if step_px == 0.0 {
                            return EventResponse::Handled;
                        }
                        // Compute multiplicative factor: each notch = 1.1×
                        // (or 1/1.1 for zoom-out). Using exp-form keeps
                        // repeated notches consistent.
                        let factor = (-step_px * 0.1).exp();
                        let z_old = zoom.get();
                        let r_now = rotation.get();
                        let scene_range = scene_zoom_range_sig.get();
                        let view_range = view_zoom_range_sig.get();
                        let effective_zoom =
                            intersect_zoom_range(scene_range.as_ref(), view_range.as_ref());
                        let z_new = clamp_zoom(z_old * factor, effective_zoom.as_ref());
                        if (z_new - z_old).abs() < 1e-6 {
                            return EventResponse::Handled;
                        }
                        let viewport_size = last_viewport_for_scroll.get();
                        let bo = bounds_origin_for_scroll.get();
                        // Anchor the zoom at the cursor when known
                        // (zoom-about-pointer — the scene point under
                        // the mouse stays put). Fall back to viewport
                        // center if no cursor position has been seen.
                        let anchor_screen = match cursor_pos_for_scroll.get() {
                            Some(p) => p,
                            None => bastyde_canvas::Point::new(
                                bo.x + viewport_size.width * 0.5,
                                bo.y + viewport_size.height * 0.5,
                            ),
                        };
                        let pan_old = Vec2::new(pan_x.get(), pan_y.get());
                        let new_pan = anchor_pan_for_pinch(
                            anchor_screen,
                            pan_old,
                            z_old,
                            r_now,
                            z_new,
                            r_now,
                            bo,
                        )
                        .unwrap_or(pan_old);
                        // Clamp the zoom-induced pan adjustment against
                        // the effective pan_bounds so wheel-zoom over
                        // a doc-style bounded scene doesn't push the
                        // viewport off the document.
                        let effective_bounds = intersect_pan_bounds(
                            scene_pan_bounds_sig.get(),
                            view_pan_bounds_sig.get(),
                        );
                        let new_pan = clamp_pan_to_bounds(
                            new_pan,
                            effective_bounds.as_ref(),
                            viewport_size,
                            z_new,
                        );
                        // Snap zoom + pan together. Animating the two
                        // signals independently with EaseOut would drift
                        // mid-tween (the anchor math is exact only at
                        // start and end states). Snap is also the
                        // standard wheel-zoom feel — each notch produces
                        // an immediate, predictable step. The pinch
                        // path uses the same snap rule.
                        zoom.set(z_new);
                        pan_x.set(new_pan.x);
                        pan_y.set(new_pan.y);
                        return EventResponse::Handled;
                    }
                    // No-op / pass-through when both axes are zeroed by
                    // the policy.
                    if dx == 0.0 && dy == 0.0 {
                        return EventResponse::Ignored;
                    }
                    // Convention: positive scroll delta on the y-axis
                    // means content scrolls "up" in the viewport, which
                    // is equivalent to panning the *view* down — i.e. the
                    // pan offset increases. This matches `ScrollArea` and
                    // the natural-scroll feel of trackpads.
                    let base_x = pan_x.animation_target().unwrap_or_else(|| pan_x.get());
                    let base_y = pan_y.animation_target().unwrap_or_else(|| pan_y.get());
                    // Clamp the projected pan against effective bounds.
                    let effective_bounds =
                        intersect_pan_bounds(scene_pan_bounds_sig.get(), view_pan_bounds_sig.get());
                    let clamped = clamp_pan_to_bounds(
                        Vec2::new(base_x + dx, base_y + dy),
                        effective_bounds.as_ref(),
                        last_viewport_for_scroll.get(),
                        zoom.get(),
                    );
                    // Boundary-based scroll chaining: if the pan can't move on
                    // either axis (already clamped at a bound), decline so the
                    // event bubbles to an ancestor scrollable. Mirrors the
                    // ScrollArea / ListView / TreeView / TableView behavior.
                    // `OverscrollBehavior::Contain` opts out — the scene keeps
                    // the wheel even at its bound (no chaining).
                    let moved_x =
                        (clamped.x - base_x).abs() > bastyde_core::overscroll::SCROLL_MOVE_EPSILON;
                    let moved_y =
                        (clamped.y - base_y).abs() > bastyde_core::overscroll::SCROLL_MOVE_EPSILON;
                    // Only the *total* boundary (neither axis can move) is a
                    // chain/contain decision point. If one axis still moves the
                    // event is consumed below (Handled) regardless of
                    // `overscroll` — the partial-absorb / drop-the-pinned-axis
                    // tradeoff, matching the widget scrollables and the browser.
                    if !moved_x && !moved_y {
                        return match overscroll {
                            OverscrollBehavior::Contain => EventResponse::Handled,
                            OverscrollBehavior::Chain => EventResponse::Ignored,
                        };
                    }
                    if prefers_reduced {
                        pan_x.set(clamped.x);
                        pan_y.set(clamped.y);
                    } else {
                        pan_x.animate_to(clamped.x, pan_dur, Easing::EaseOut);
                        pan_y.animate_to(clamped.y, pan_dur, Easing::EaseOut);
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
                let last_viewport_for_pinch = self.last_viewport.clone();
                let zoomable_sig_pinch = self.scene.zoomable_signal();
                let pan_axes_sig_pinch = self.scene.pan_axes_signal();
                let scene_zoom_range_sig_pinch = self.scene.zoom_range_signal();
                let view_zoom_range_sig_pinch = self.zoom_range_override.clone();
                let scene_pan_bounds_sig_pinch = self.scene.pan_bounds_signal();
                let view_pan_bounds_sig_pinch = self.pan_bounds_override.clone();
                let adopt_scene_size_pinch = self.adopt_scene_size;
                handlers = handlers.on_pinch(move |phase, _ctx| {
                    use crate::scene::PanAxes;
                    if !zoomable_sig_pinch.get() || adopt_scene_size_pinch {
                        return;
                    }
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
                    let scene_range = scene_zoom_range_sig_pinch.get();
                    let view_range = view_zoom_range_sig_pinch.get();
                    let effective_zoom =
                        intersect_zoom_range(scene_range.as_ref(), view_range.as_ref());
                    let z_new = clamp_zoom(z_old * scale, effective_zoom.as_ref());
                    let r_new = r_old + rotation_delta;
                    let pan_old = Vec2::new(pan_x.get(), pan_y.get());
                    let bo = bounds_origin_for_pinch.get();
                    let new_pan =
                        anchor_pan_for_pinch(center, pan_old, z_old, r_old, z_new, r_new, bo)
                            .unwrap_or(pan_old);
                    // Pinch is a continuous, user-driven gesture — set
                    // directly so each frame's update lands without
                    // queuing a tween. Idle gates still apply: at rest
                    // (pinch released, no further events), no frames are
                    // requested.
                    zoom.set(z_new);
                    rotation.set(r_new);
                    // Apply pan-axes policy live, then clamp to
                    // effective pan_bounds (intersection of Scene +
                    // view-override).
                    let new_pan = match pan_axes_sig_pinch.get() {
                        PanAxes::Both => new_pan,
                        PanAxes::None => pan_old,
                        PanAxes::Horizontal => Vec2::new(new_pan.x, pan_old.y),
                        PanAxes::Vertical => Vec2::new(pan_old.x, new_pan.y),
                    };
                    let effective_bounds = intersect_pan_bounds(
                        scene_pan_bounds_sig_pinch.get(),
                        view_pan_bounds_sig_pinch.get(),
                    );
                    let new_pan = clamp_pan_to_bounds(
                        new_pan,
                        effective_bounds.as_ref(),
                        last_viewport_for_pinch.get(),
                        z_new,
                    );
                    pan_x.set(new_pan.x);
                    pan_y.set(new_pan.y);
                });
            }

            // --- Keyboard navigation -------------------------------
            //
            // Default scheme:
            // - Arrow keys: pan by ~one viewport-quarter per press. Released
            //   here for now; held-key repeat naturally chains tweens via
            //   `animate_to`. Apps that wire `focus_order(...)`
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
                use bastyde_core::event::{EventResponse, Key, WidgetEvent};
                let pan_x = self.pan_x.clone();
                let pan_y = self.pan_y.clone();
                let zoom = self.zoom.clone();
                let pan_dur = self.pan_anim_duration;
                let zoom_dur = self.zoom_anim_duration;
                let viewport_size = self.last_viewport.clone();
                let pan_x_for_xform = self.pan_x.clone();
                let pan_y_for_xform = self.pan_y.clone();
                let zoom_for_xform = self.zoom.clone();
                let rotation_for_xform = self.rotation.clone();
                let bounds_origin_for_xform = self.bounds_origin_signal.clone();
                let pan_axes_sig_keys = self.scene.pan_axes_signal();
                let zoomable_sig_keys = self.scene.zoomable_signal();
                let scene_zoom_range_sig_keys = self.scene.zoom_range_signal();
                let view_zoom_range_sig_keys = self.zoom_range_override.clone();
                let scene_pan_bounds_sig_keys = self.scene.pan_bounds_signal();
                let view_pan_bounds_sig_keys = self.pan_bounds_override.clone();
                let adopt_scene_size_keys = self.adopt_scene_size;
                handlers = handlers.on_key(move |event, _ctx| {
                    use crate::scene::PanAxes;
                    let WidgetEvent::KeyDown { key, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    let pan_axes_keys = pan_axes_sig_keys.get();
                    let zoomable_keys = zoomable_sig_keys.get() && !adopt_scene_size_keys;
                    let allow_pan_x = matches!(pan_axes_keys, PanAxes::Both | PanAxes::Horizontal);
                    let allow_pan_y = matches!(pan_axes_keys, PanAxes::Both | PanAxes::Vertical);
                    let clamp_to_zoom = |z: f32| -> f32 {
                        let scene_range = scene_zoom_range_sig_keys.get();
                        let view_range = view_zoom_range_sig_keys.get();
                        let effective =
                            intersect_zoom_range(scene_range.as_ref(), view_range.as_ref());
                        clamp_zoom(z, effective.as_ref())
                    };
                    let clamp_to_pan = |p: Vec2, z: f32| -> Vec2 {
                        let effective_bounds = intersect_pan_bounds(
                            scene_pan_bounds_sig_keys.get(),
                            view_pan_bounds_sig_keys.get(),
                        );
                        clamp_pan_to_bounds(p, effective_bounds.as_ref(), viewport_size.get(), z)
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
                        let anchor_screen = bastyde_canvas::Point::new(
                            bo.x + viewport.width * 0.5,
                            bo.y + viewport.height * 0.5,
                        );
                        let z_old = zoom_for_xform.get();
                        let r = rotation_for_xform.get();
                        let pan_old = Vec2::new(pan_x_for_xform.get(), pan_y_for_xform.get());
                        if let Some(new_pan) =
                            anchor_pan_for_pinch(anchor_screen, pan_old, z_old, r, z_new, r, bo)
                        {
                            pan_x_for_xform.animate_to(new_pan.x, pan_dur, Easing::EaseOut);
                            pan_y_for_xform.animate_to(new_pan.y, pan_dur, Easing::EaseOut);
                        }
                    };
                    // Arrow-key pan helper: take the current animation
                    // target (or live value if no tween in flight),
                    // shift by step on the requested axis, clamp the
                    // resulting pan vector against the effective
                    // pan_bounds, then animate to the clamped target.
                    let pan_axis = |dx: f32, dy: f32| {
                        let base_x = pan_x.animation_target().unwrap_or_else(|| pan_x.get());
                        let base_y = pan_y.animation_target().unwrap_or_else(|| pan_y.get());
                        let target = clamp_to_pan(Vec2::new(base_x + dx, base_y + dy), zoom.get());
                        if dx != 0.0 {
                            pan_x.animate_to(target.x, pan_dur, Easing::EaseOut);
                        }
                        if dy != 0.0 {
                            pan_y.animate_to(target.y, pan_dur, Easing::EaseOut);
                        }
                    };
                    match key {
                        Key::ArrowLeft if allow_pan_x => pan_axis(pan_step, 0.0),
                        Key::ArrowRight if allow_pan_x => pan_axis(-pan_step, 0.0),
                        Key::ArrowUp if allow_pan_y => pan_axis(0.0, pan_step),
                        Key::ArrowDown if allow_pan_y => pan_axis(0.0, -pan_step),
                        other
                            if zoomable_keys
                                && (other.to_char() == Some('+')
                                    || other.to_char() == Some('=')) =>
                        {
                            let z_new = clamp_to_zoom(zoom.get() * 1.25);
                            zoom.animate_to(z_new, zoom_dur, Easing::EaseOut);
                            recenter_zoom(z_new);
                        }
                        other if zoomable_keys && other.to_char() == Some('-') => {
                            let z_new = clamp_to_zoom(zoom.get() * 0.8);
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

        // --- Marquee box-select via on_drag --------------------
        //
        // Active when selection mode is not None. The on_drag
        // closure tracks the marquee rectangle in *screen* coords
        // (the position the recognizer hands us is screen-space at
        // the time of the gesture) and, on `Ended`, posts a
        // pending-commit entry. The next `place_children` call
        // consumes it: that step has direct `&self.scene` access
        // and can call `SceneSelection::commit_marquee` without
        // forcing `Scene` into an `Rc<RefCell>`. Tests can also
        // trigger the commit via `flush_marquee_commit()`.
        //
        // Drag mode (RubberBand / ScrollHandDrag / NoDrag) is
        // reactive — the handler is always installed when
        // selection is on, and the closure reads `drag_mode_sig`
        // live each event so a toolbar can flip between Select /
        // Hand tools without rebuilding the view. NoDrag is
        // treated as "all branches early-return", same observable
        // effect as not registering the handler.
        if !matches!(
            self.selection.mode(),
            crate::selection::SceneSelectionMode::None
        ) {
            let marquee = self.marquee.clone();
            let pending_marquee_commit = self.pending_marquee_commit.clone();
            let drag_target = self.drag_target.clone();
            let pending_item_move = self.pending_item_move.clone();
            let drag_dirty = self.drag_dirty.clone();
            let view_xform_signal = self.view_transform_signal.clone();
            let bounds_snapshot = self.lightweight_bounds_snapshot.clone();
            let pan_x_for_drag = self.pan_x.clone();
            let pan_y_for_drag = self.pan_y.clone();
            let drag_mode_sig = self.drag_mode.clone();
            // Live signal captures — runtime mutations to pan_axes /
            // pan_bounds take effect on the next drag event.
            let pan_axes_sig_drag = self.scene.pan_axes_signal();
            let scene_pan_bounds_sig_drag = self.scene.pan_bounds_signal();
            let view_pan_bounds_sig_drag = self.pan_bounds_override.clone();
            let zoom_for_drag = self.zoom.clone();
            let last_viewport_for_drag = self.last_viewport.clone();
            handlers = handlers.on_drag(move |phase, _ctx| {
                // Read drag mode live so a toolbar can flip
                // between Select / Hand / NoDrag at runtime.
                let drag_mode_inner = drag_mode_sig.get();
                if drag_mode_inner == crate::item_handlers::DragMode::NoDrag {
                    return;
                }
                // ScrollHandDrag mode bypasses item / marquee logic
                // entirely — drag pans the view by the cursor
                // delta in scene coords. Marquee and drag-to-move
                // are inactive in this mode.
                if drag_mode_inner == crate::item_handlers::DragMode::ScrollHandDrag {
                    use crate::scene::PanAxes;
                    use bastyde_core::gesture::DragPhase;
                    if let DragPhase::Moved { delta, .. } = phase {
                        // `delta` is in screen coords. Apply the
                        // scene's pan-axes policy live: zero out the
                        // restricted axis so an axis-locked scene
                        // can't be hand-dragged off-axis. Sign
                        // convention matches scroll (drag right →
                        // pan right).
                        let (dx, dy) = match pan_axes_sig_drag.get() {
                            PanAxes::Both => (delta.x, delta.y),
                            PanAxes::None => (0.0, 0.0),
                            PanAxes::Horizontal => (delta.x, 0.0),
                            PanAxes::Vertical => (0.0, delta.y),
                        };
                        if dx == 0.0 && dy == 0.0 {
                            return;
                        }
                        // Clamp to effective pan_bounds (intersection of
                        // Scene + view-override) so the user can't drag
                        // the document off the viewport.
                        let effective_bounds = intersect_pan_bounds(
                            scene_pan_bounds_sig_drag.get(),
                            view_pan_bounds_sig_drag.get(),
                        );
                        let target = clamp_pan_to_bounds(
                            Vec2::new(pan_x_for_drag.get() + dx, pan_y_for_drag.get() + dy),
                            effective_bounds.as_ref(),
                            last_viewport_for_drag.get(),
                            zoom_for_drag.get(),
                        );
                        pan_x_for_drag.set(target.x);
                        pan_y_for_drag.set(target.y);
                    }
                    return;
                }
                use bastyde_core::gesture::DragPhase;
                match phase {
                    DragPhase::Started { position, button } => {
                        if !matches!(button, bastyde_core::event::PointerButton::Primary) {
                            return;
                        }
                        // Project screen press to scene coords for
                        // hit-test against the snapshot.
                        let xform = view_xform_signal.get();
                        let scene_press = match xform.inverse() {
                            Some(inv) => inv.apply_point(position),
                            None => Point::ZERO,
                        };
                        // Hit-test scene items in reverse insertion
                        // order so items painted on top get
                        // priority. The snapshot is refreshed each
                        // layout pass — see `place_children`.
                        let snap = bounds_snapshot.borrow();
                        let hit = snap
                            .iter()
                            .rev()
                            .find(|(_, rect)| rect.contains(scene_press));
                        if let Some(&(item_id, _)) = hit {
                            // Drag-to-move: enter that mode,
                            // not marquee.
                            drag_target.set(Some(DragTarget {
                                item_id,
                                anchor_scene: scene_press,
                                current_scene: scene_press,
                            }));
                        } else {
                            // Empty area — start a marquee.
                            marquee.set(Some(MarqueeState {
                                origin: position,
                                current: position,
                                additive: false,
                            }));
                        }
                    }
                    DragPhase::Moved { position, .. } => {
                        if let Some(mut target) = drag_target.get() {
                            // Update current scene-coord position
                            // for live paint feedback (the paint
                            // method will pick this up).
                            let xform = view_xform_signal.get();
                            if let Some(inv) = xform.inverse() {
                                target.current_scene = inv.apply_point(position);
                                drag_target.set(Some(target));
                            }
                        } else if let Some(mut state) = marquee.get() {
                            state.current = position;
                            marquee.set(Some(state));
                        }
                    }
                    DragPhase::Ended { position } => {
                        if let Some(mut target) = drag_target.get() {
                            // Drag-to-move commit: compute the
                            // delta (current − anchor) in scene
                            // coords and post (id, delta) so the
                            // drain code can apply the same delta
                            // to every descendant.
                            let xform = view_xform_signal.get();
                            if let Some(inv) = xform.inverse() {
                                target.current_scene = inv.apply_point(position);
                            }
                            let delta = Vec2::new(
                                target.current_scene.x - target.anchor_scene.x,
                                target.current_scene.y - target.anchor_scene.y,
                            );
                            // Keep `drag_target` set with the final
                            // current_scene so `paint` continues to
                            // translate the item to the dragged
                            // position. The rebuild that drains
                            // `pending_item_move` will clear
                            // `drag_target` once the move has
                            // actually been applied to the scene —
                            // until then, clearing here would let
                            // one or more frames paint at the
                            // ORIGINAL (pre-drag) bounds and the
                            // item appears to "snap back" before
                            // the rebuild lands. Update the saved
                            // current_scene so the visual delta
                            // stays right.
                            drag_target.set(Some(target));
                            pending_item_move.set(Some((target.item_id, delta)));
                            // Bump the rebuild signal so SceneView's
                            // `build()` runs and drains the pending
                            // move (where `&mut self.scene` is
                            // available and `Scene::set_local_pos` can
                            // commit + re-bucket the spatial index).
                            drag_dirty.set(drag_dirty.get().wrapping_add(1));
                            return;
                        }
                        // Marquee commit path. Same drain-via-rebuild
                        // pattern as drag-to-move: post the pending
                        // commit, bump `drag_dirty` so `build()` runs
                        // and drains it (which also clears the
                        // marquee Cell so the visual lasso disappears
                        // after release).
                        let Some(mut state) = marquee.get() else {
                            return;
                        };
                        state.current = position;
                        let screen_rect = state.rect();
                        let xform = view_xform_signal.get();
                        let scene_rect = match xform.inverse() {
                            Some(inv) => inv.apply_rect(screen_rect),
                            None => Rect::ZERO,
                        };
                        pending_marquee_commit.set(Some((scene_rect, state.additive)));
                        drag_dirty.set(drag_dirty.get().wrapping_add(1));
                    }
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        child_ids
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // When `adopt_scene_size` is set, the view sizes itself to
        // the scene's resolved extent so the entire scene fits
        // inside the view's bounds. Falls back to `default_size`
        // when the scene has no extent declared and no items.
        // We size to the extent's width/height, NOT its right/bottom
        // — for scenes with items at non-origin (e.g. negative) scene
        // coordinates, right/bottom is inflated by the bounding rect's
        // origin offset.
        let (default_w, default_h) = if self.adopt_scene_size {
            match self.scene.scene_rect_extent() {
                Some(r) => (r.width, r.height),
                None => (self.default_size.width, self.default_size.height),
            }
        } else {
            (self.default_size.width, self.default_size.height)
        };
        let size = proposal.resolve(default_w, default_h);
        // Cache for `fit_to_content` and friends. `bounds_origin` is
        // refreshed in `place_children`, which runs whenever the
        // SceneView has at least one child — i.e. always in real
        // apps, since an empty SceneView doesn't render anything to
        // interact with. The set is gated by an equality check so
        // unchanged sizes don't spuriously fire viewport observers.
        if self.last_viewport.get() != size {
            self.last_viewport.set(size);
        }

        // Refresh the lightweight-bounds snapshot used
        // by the on_drag closure for hit-test. Done here (rather
        // than in `place_children`) because `place_children` only
        // runs when the SceneView has at least one heavyweight
        // child — a scene with only lightweight items would never
        // get its snapshot populated. `layout_response` runs every
        // layout pass regardless.
        {
            // Snapshot of *draggable* lightweight items. Decorative
            // items (background tiles, group chrome, connector paths,
            // captions) opt into drag via `.draggable(true)` on the
            // built-in builders or by overriding `is_draggable()` on
            // a custom impl; everything else stays anchored, which
            // is the default. Without this filter, every visible
            // RectItem would respond to drags and the scene would
            // feel unstable to the user.
            let mut snapshot = self.lightweight_bounds_snapshot.borrow_mut();
            snapshot.clear();
            // Snapshot draggable lightweight items' scene-AABBs for
            // the drag-start hit-test. Refreshed each layout pass so
            // a parent move between drag events doesn't leave the
            // snapshot stale.
            let ids: Vec<crate::item::ItemId> = self.scene.ids();
            for id in ids {
                if self.scene.item(id).is_none() {
                    continue;
                }
                let Some(flags) = self.scene.flags(id) else {
                    continue;
                };
                if !flags.contains(crate::flags::ItemFlags::IS_DRAGGABLE) {
                    continue;
                }
                if let Some(scene_rect) = self.scene.scene_rect(id) {
                    snapshot.push((id, scene_rect));
                }
            }
        }

        // Refresh the handler-dispatch snapshot used by
        // `on_pointer_event` to route hover / tap / context-menu
        // events to the item under the pointer. Only items with a
        // handler set installed need to be considered for routing,
        // but we include every item so cursor-over-item-without-
        // handler can still consult the per-item cursor field.
        {
            let mut snap = self.handler_snapshot.borrow_mut();
            snap.clear();
            for id in self.scene.ids() {
                let Some(item) = self.scene.item(id) else {
                    continue;
                };
                let Some(scene_rect) = self.scene.scene_rect(id) else {
                    continue;
                };
                let scene_xform = self.scene.scene_transform(id);
                let z = self.scene.z(id).unwrap_or(0.0);
                let handlers = self.scene.handlers(id).cloned().map(Box::new);
                // Capture the item's shape-test as a stand-alone
                // closure so the snapshot can answer narrow-phase
                // hit-test without holding a borrow on the Scene.
                // Items with non-AABB geometry (PathItem stroke-only,
                // GroupItem logical-only) override `clone_shape_test`
                // to capture the data they need; default impl returns
                // an AABB predicate over `local_bounds`.
                let shape_contains: Rc<dyn Fn(Point, f32) -> bool> = item.clone_shape_test().into();
                let local_bounds = self.scene.local_bounds(id).unwrap_or(Rect::ZERO);
                let flags = self.scene.flags(id).unwrap_or_default();
                let ignores_xform =
                    flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS);
                // For IGNORES items the scene_anchor is fixed across
                // pan/zoom (it lives in scene coords); the dispatch
                // closure projects it through the live view transform
                // at event time to obtain the current screen anchor.
                let scene_anchor = if ignores_xform {
                    scene_xform.apply_point(Point::ZERO)
                } else {
                    Point::ZERO
                };
                snap.push(HandlerSnapshotEntry {
                    id,
                    scene_rect,
                    scene_transform: scene_xform,
                    shape_contains,
                    z,
                    handlers,
                    ignores_xform,
                    scene_anchor,
                    local_bounds,
                });
            }
            // Sort by z descending so hit-test picks topmost first.
            snap.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));
        }

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
        // bound at `BindingLevel::RepaintOnly` via `set_content_transform`,
        // so changes only trigger repaint — never relayout — which
        // keeps idle behaviour intact when the SceneView is at rest.
        let new_origin = Vec2::new(bounds.x, bounds.y);
        if self.bounds_origin_signal.get() != new_origin {
            self.bounds_origin_signal.set(new_origin);
        }

        // Drain any pending marquee commit posted by the
        // on_drag closure on the previous `Ended`. We do it here
        // (not in the closure) because `place_children` has direct
        // access to `&self.scene` for the spatial-index query
        // — keeping `Scene` plain instead of `Rc<RefCell<Scene>>`.
        // After commit, clear the in-flight marquee so paint stops
        // overlaying the rect.
        if let Some((rect, additive)) = self.pending_marquee_commit.take() {
            self.selection.commit_marquee(&self.scene, rect, additive);
            self.marquee.set(None);
        }

        // Drain any pending drag-to-move commit, applied
        // via the public `flush_pending_mutations` helper to keep
        // the borrow tractable (`place_children` takes `&self`,
        // and `Scene::set_local_pos` needs `&mut Scene`). The
        // framework calls layout from `&mut tree`, which gives
        // `&mut self` access elsewhere — but inside this trait
        // method we have only `&self`. Defer to a separate
        // `flush_pending_mutations(&mut self)` step instead. For
        // headless tests that drive the closure directly, the
        // public `flush_marquee_commit` / `flush_pending_mutations`
        // methods materialise the result.

        // (Lightweight-bounds snapshot is refreshed in
        // `layout_response` so it's available even when the
        // SceneView has zero heavyweight children — see comment
        // there.)

        // Place each child at its **pure scene coordinate** — not
        // offset by `bounds.origin`. The renderer's transform stack
        // composes `bounds.origin` in via the view transform's final
        // translate, so a child at scene (sx, sy) lands visually at
        // (bounds.x + zoom*sx + pan.x, bounds.y + zoom*sy + pan.y).
        // The transform-aware hit-test routes through the same
        // scope automatically.
        //
        // Cull: compute the visible scene-coord region by
        // inverse-transforming the SceneView's screen-space rect,
        // then collapse the size of any child whose `scene_rect`
        // doesn't intersect it. The placement's `origin` stays at
        // its canonical scene-coord position (so focus-follow /
        // scroll-into-view see consistent coordinates whether or not
        // the child is visible); only `size` flips to zero, which
        // short-circuits the recursive layout walk under that child
        // and skips its paint entirely. Heavyweight children stay
        // materialised — true demand-load is a follow-up once
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

    fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        // Sync `bounds_origin_signal` with the bounds the framework
        // assigned. `place_children` is the canonical site for this,
        // but it only runs when the SceneView has heavyweight widget
        // children — a SceneView with only lightweight items would
        // never see a place_children call and its bounds_origin
        // would stay at its default. That breaks nested-SceneView
        // placement (the inner view draws at outer-scene origin
        // instead of at its own scene_rect). Updating from `paint`
        // costs a one-frame lag on first display (the Signal change
        // dirties view_transform_signal which dirties paint for the
        // next frame). For static nested SceneViews this is
        // unnoticeable; for moving ones, every frame the bounds
        // change, the signal updates, and the next frame catches up.
        let new_origin = Vec2::new(bounds.x, bounds.y);
        if self.bounds_origin_signal.get() != new_origin {
            self.bounds_origin_signal.set(new_origin);
        }

        // The SceneView's `set_content_transform` scope wraps both this paint
        // call and the children walk, so any `canvas.fill_*` /
        // `canvas.stroke_*` / `canvas.draw_*` call we make here lands
        // through the same view-transform projection as the heavyweight
        // children. We pass scene-coord rects directly — the renderer
        // composes pan / zoom / rotation / bounds-origin on top.
        let region = self.visible_scene_region(bounds);

        // App-supplied background closure: paints under all items in
        // scene coords, with the visible scene region passed so the
        // closure can skip off-screen geometry.
        if let Some(bg) = &self.background_paint {
            bg(canvas, ctx, region);
        }

        // Under band: lightweight items below the heavyweight children
        // (background furniture — connector lines, tiled grids, decorations).
        // The render walker invokes the parent's paint first, then descends
        // into children, so these render under the cards. The Over band and
        // the marquee / foreground / debug overlays paint in `post_paint`
        // (after the children) so they sit on top.
        self.paint_band(canvas, bounds, crate::scene::SceneLayer::Under);
    }

    fn wants_post_paint(&self) -> bool {
        // SceneView has a foreground pass (post_paint) only when something
        // must render over the heavyweight children: a selection marquee, an
        // app-supplied foreground closure, debug overlays, or any lightweight
        // item raised to the Over band.
        self.marquee.get().is_some()
            || self.foreground_paint.is_some()
            || self.debug_overlay.is_active()
            || self.scene.has_over_layer_items()
    }

    fn post_paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        // Foreground pass — runs after the heavyweight children, inside the
        // same view-transform / clip scope as `paint`. Everything here sits
        // on top of the cards: the Over-band lightweight items, then the
        // selection marquee, then the app foreground hook, then debug overlays.
        let view_transform = self.view_transform();

        // Over band: lightweight items explicitly raised above the cards
        // (highlighted connectors, selection halos, annotations).
        self.paint_band(canvas, bounds, crate::scene::SceneLayer::Over);

        // Marquee overlay — semi-transparent fill plus a single-pixel
        // stroke. The marquee state is in screen coords (set by the on_drag
        // closure); the view-transform scope is on the canvas, so we project
        // the screen-rect back to scene coords and paint there.
        if let Some(state) = self.marquee.get() {
            let screen_rect = state.rect();
            if let Some(inv) = view_transform.inverse() {
                let scene_rect = inv.apply_rect(screen_rect);
                let fill = bastyde_tokens::Color::new(0.40, 0.55, 0.85, 0.18);
                let stroke = bastyde_tokens::Color::new(0.40, 0.55, 0.85, 0.85);
                canvas.fill_rect(scene_rect, fill);
                canvas.stroke_rect(scene_rect, stroke, bastyde_canvas::StrokeStyle::solid(1.0));
            }
        }

        // App-supplied foreground closure: paints over all items (and the
        // marquee), under the debug overlay.
        if let Some(fg) = &self.foreground_paint {
            let region = self.visible_scene_region(bounds);
            fg(canvas, ctx, region);
        }

        // Visual-debug overlays, on top of everything.
        if self.debug_overlay.is_active() {
            self.paint_debug_overlay(bounds, canvas);
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

    fn preserves_children_on_rebuild(&self) -> bool {
        // SceneView's children come from `Scene::add_widget` calls,
        // materialised once on the first build via
        // `ctx.add_boxed(pending.take())`. Subsequent rebuilds —
        // triggered by `drag_dirty` to drain pending drag-to-move /
        // marquee commits — re-push the same `WidgetId`s without
        // calling `ctx.add_boxed` again (the `pending` slot is
        // already taken). The default rebuild semantics would
        // destroy the children's subtrees before build runs, so
        // the re-pushed IDs would dangle and the cards / nested
        // SceneViews would visibly disappear after every drag-end.
        // Opt out of the destruction so children stay attached.
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
        // tell the framework walker to skip
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
        use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id};
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

    fn accessibility(&self, builder: &mut bastyde_core::accessibility::AccessNodeBuilder) {
        use crate::a11y::A11yNode;
        use crate::scene::SceneEntryKind;
        use bastyde_core::accessibility::{SyntheticKind, synthetic_node_id};
        use std::collections::{HashMap, HashSet};

        // SceneView itself is `Role::Pane` for a top-level scene
        // and `Role::Region` for a logically nested scene (set via
        // `nested_a11y(true)`). Heavyweight children (real widgets
        // in the arena) are emitted by the tree walker as natural
        // descendants; we only need to add the lightweight tier
        // here.
        if self.a11y_nested {
            builder.set_role(accesskit::Role::Region);
        } else {
            builder.set_role(accesskit::Role::Pane);
        }
        if let Some(label) = &self.a11y_label {
            builder.set_name(label.resolve_now());
        }

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

        // Place groups. Groups always emit — they have no
        // visual default to fall back to. A group with no declared
        // parent goes to SceneView root, regardless of mode.
        for group in &self.scene.a11y_groups {
            let node = A11yNode::Group(group.id);
            let parent = self.scene.a11y_parent_of(node);
            logical_children.entry(parent).or_default().push(node);
        }

        // Place all visible scene entries — lightweight
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

        // Ad-hoc widget relocations addressed by `WidgetId`
        // (rare — typically a descendant of a heavyweight scene
        // item that should belong elsewhere logically). Widgets
        // referenced via `A11yNode::Item(item_id)` are already
        // handled by the visible-entries pass.
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
        let roots = logical_children.get(&None).cloned().unwrap_or_default();
        for root in roots {
            self.emit_logical_node(builder, root, None, &logical_children, &mut visited);
        }

        // Apply cross-tree decorations (relations / live
        // regions / landmarks). Items / groups must already be in
        // `children_collected` for the writes to land on the right
        // node. Heavyweight widgets are not yet routed through here
        // — the walker can't decorate widget-derived NodeIds from a
        // sibling's accessibility() impl. Apps that need to point
        // a `flow_to`/`controls` at a widget should use the
        // synthetic NodeIds (decorating widgets is part of
        // the deferred auto-graft work).
        let owner = builder.owner_id();
        let resolve = |node: A11yNode| -> Option<accesskit::NodeId> {
            match node {
                A11yNode::Item(id) => {
                    owner.map(|o| synthetic_node_id(o, id.as_u64(), SyntheticKind::SceneItem))
                }
                A11yNode::Group(id) => {
                    owner.map(|o| synthetic_node_id(o, id.as_u64(), SyntheticKind::SceneGroup))
                }
                A11yNode::Widget(id) => Some(bastyde_core::accessibility::widget_id_to_node_id(id)),
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

    /// Paint enabled debug overlays on top of the scene rendering.
    /// All paint commands are in scene coords — they ride the
    /// same view-transform scope as items, so the overlays follow
    /// the user's pan/zoom naturally.
    fn paint_debug_overlay(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas) {
        let cfg = self.debug_overlay;
        let region = self.visible_scene_region(bounds);
        let stroke_w = 1.0;
        // Distinct color per overlay so multiple flags compose
        // visually without confusion.
        let item_color = bastyde_tokens::Color::new(0.20, 0.75, 0.35, 0.85);
        let content_color = bastyde_tokens::Color::new(0.30, 0.45, 0.95, 0.85);
        let viewport_color = bastyde_tokens::Color::new(0.95, 0.30, 0.30, 0.85);
        let selection_color = bastyde_tokens::Color::new(1.00, 0.60, 0.20, 0.95);

        // The canvas rides the view-transform scope, so paint commands
        // are in scene coords. For IGNORES_TRANSFORMATIONS items, the
        // visible area on screen is `local_bounds` rooted at the
        // screen-projected `scene_anchor` (NOT at the zoom-scaled
        // scene_rect). To stroke that visible area correctly through
        // a view-transform-scoped canvas we inverse-project the
        // screen-space rect back to scene coords — same trick the
        // marquee overlay uses. Falls through to scene_rect when the
        // view transform is degenerate.
        let view_transform = self.view_transform();
        let visible_bounds = |id: crate::item::ItemId| -> Option<Rect> {
            let scene_rect = self.scene.scene_rect(id)?;
            let flags = self.scene.flags(id).unwrap_or_default();
            if !flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS) {
                return Some(scene_rect);
            }
            let local_bounds = self.scene.local_bounds(id)?;
            let scene_xform = self.scene.scene_transform(id);
            let scene_anchor = scene_xform.apply_point(Point::ZERO);
            let screen_anchor = view_transform.apply_point(scene_anchor);
            let screen_rect = Rect::new(
                screen_anchor.x + local_bounds.x,
                screen_anchor.y + local_bounds.y,
                local_bounds.width,
                local_bounds.height,
            );
            view_transform
                .inverse()
                .map(|inv| inv.apply_rect(screen_rect))
                .or(Some(scene_rect))
        };

        if cfg.item_bounds {
            for id in self.scene.ids() {
                if let Some(rect) = visible_bounds(id) {
                    canvas.stroke_rect(
                        rect,
                        item_color,
                        bastyde_canvas::StrokeStyle::solid(stroke_w),
                    );
                }
            }
        }
        if cfg.content_bounds
            && let Some(content) = self.scene_content_bounds()
        {
            canvas.stroke_rect(
                content,
                content_color,
                bastyde_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.viewport {
            canvas.stroke_rect(
                region,
                viewport_color,
                bastyde_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.selection_bounds {
            for id in self.selection.selected() {
                if let Some(rect) = visible_bounds(id) {
                    canvas.stroke_rect(
                        rect,
                        selection_color,
                        bastyde_canvas::StrokeStyle::solid(stroke_w * 2.0),
                    );
                }
            }
        }
    }

    // -- A11y-walker helpers used by `accessibility` -----------------------

    /// Recursive DFS step: emit one node of the logical tree under
    /// `parent_id` (`None` = SceneView's own node), then descend.
    /// Cycle-guards via `visited`; the same node visited twice is
    /// skipped on the second appearance, so a malformed parent
    /// declaration doesn't infinite-loop the walker.
    fn emit_logical_node(
        &self,
        builder: &mut bastyde_core::accessibility::AccessNodeBuilder,
        node: crate::a11y::A11yNode,
        parent_id: Option<accesskit::NodeId>,
        logical_children: &std::collections::HashMap<
            Option<crate::a11y::A11yNode>,
            Vec<crate::a11y::A11yNode>,
        >,
        visited: &mut std::collections::HashSet<crate::a11y::A11yNode>,
    ) {
        use crate::a11y::A11yNode;
        use bastyde_core::accessibility::SyntheticKind;

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
                    let _ = item; // borrowed below for accessibility() call
                    let scene_bounds = self.scene.scene_rect(item_id).unwrap_or(Rect::ZERO);
                    let screen_bounds = view_transform.apply_rect(scene_bounds);
                    // Choose which space to advertise to AT clients
                    // per `a11y_bounds_space`. The `SceneItemA11yContext`
                    // always carries the screen-projected rect (so item
                    // impls don't have to re-do the math); only the
                    // `set_bounds` write to AccessKit varies.
                    let advertised_bounds = match self.a11y_bounds_space {
                        crate::a11y::A11yBoundsSpace::Screen => screen_bounds,
                        crate::a11y::A11yBoundsSpace::Scene => scene_bounds,
                    };
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
                                x0: advertised_bounds.x as f64,
                                y0: advertised_bounds.y as f64,
                                x1: (advertised_bounds.x + advertised_bounds.width) as f64,
                                y1: (advertised_bounds.y + advertised_bounds.height) as f64,
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
                        bastyde_core::accessibility::widget_id_to_node_id(widget_id);
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
                            child.set_name(label.resolve_now());
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
                let widget_node_id = bastyde_core::accessibility::widget_id_to_node_id(widget_id);
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
        builder: &mut bastyde_core::accessibility::AccessNodeBuilder,
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
        builder: &mut bastyde_core::accessibility::AccessNodeBuilder,
        node_id: accesskit::NodeId,
        live: accesskit::Live,
    ) {
        builder.with_collected_node(node_id, |node| {
            node.set_live(live);
        });
    }

    fn set_collected_role(
        &self,
        builder: &mut bastyde_core::accessibility::AccessNodeBuilder,
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
mod tests;
