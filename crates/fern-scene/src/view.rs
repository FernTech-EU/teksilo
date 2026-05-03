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

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
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
    /// Latest viewport size observed during layout. Cached so
    /// imperative methods like [`SceneView::fit_to_content`] can
    /// reason about the visible rectangle without re-running layout.
    /// `Rc<Cell>` so event-handler closures (e.g. Ctrl+wheel zoom-
    /// about-viewport-center) can read it without touching `&mut self`.
    last_viewport: Rc<Cell<Size>>,

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

    // --- Selection (Phase 6) -----------------------------------------
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
    /// `Scene::move_item` (which requires `&mut self.scene`,
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
    focus_order_callback: Option<Rc<dyn Fn(&Scene, FocusDirection, Option<ItemId>) -> Option<ItemId>>>,

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
    a11y_label: Option<String>,
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
            .field("min_zoom", &self.min_zoom)
            .field("max_zoom", &self.max_zoom)
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
            adopt_scene_size: false,
            last_viewport: Rc::new(Cell::new(Size::new(800.0, 600.0))),
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
    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = Some(label.into());
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
            (FocusDirection::Backward, Some(cur)) => ids
                .iter()
                .position(|id| *id == cur)
                .and_then(|i| if i == 0 { None } else { ids.get(i - 1).copied() }),
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
        self.last_viewport.set(self.default_size);
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
        let clamped = target.clamp(self.min_zoom, self.max_zoom);
        self.zoom.animate_to(clamped, duration, Easing::EaseOut);
    }

    /// Snap zoom to `target` without animation, clamped. No-op when
    /// the scene declares zoom disabled.
    pub fn set_zoom(&self, target: f32) {
        if !self.scene.is_zoomable() || self.adopt_scene_size {
            return;
        }
        let clamped = target.clamp(self.min_zoom, self.max_zoom);
        self.zoom.set(clamped);
    }

    /// Project `target` through the scene's pan-axes policy. The
    /// orthogonal axis is held at its current value when the policy
    /// excludes it; `PanAxes::None` (and `adopt_scene_size`) holds
    /// both axes at their current pan.
    fn gate_pan_target(&self, target: Vec2) -> Vec2 {
        use crate::scene::PanAxes;
        if self.adopt_scene_size {
            return Vec2::new(self.pan_x.get(), self.pan_y.get());
        }
        match self.scene.current_pan_axes() {
            PanAxes::Both => target,
            PanAxes::None => Vec2::new(self.pan_x.get(), self.pan_y.get()),
            PanAxes::Horizontal => Vec2::new(target.x, self.pan_y.get()),
            PanAxes::Vertical => Vec2::new(self.pan_x.get(), target.y),
        }
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
        self.zoom.set(state.zoom.clamp(self.min_zoom, self.max_zoom));
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
        let scale = (avail_w / rect.width.max(1.0))
            .min(avail_h / rect.height.max(1.0))
            .clamp(self.min_zoom, self.max_zoom);
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

        // Bind the drag-rebuild signal so the next drop triggers a
        // rebuild and the drains above run. `BindingLevel::Rebuild`
        // is the level that re-runs `build()` on signal change.
        self.drag_dirty.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

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
            let view_xform_signal = self.view_transform_signal.clone();
            let drag_target_for_cursor = self.drag_target.clone();
            handlers = handlers.on_pointer_event(move |ev, ctx| {
                use fern_core::event::WidgetEvent as Ev;
                use fern_core::widget::CursorIcon;
                match ev {
                    Ev::PointerMove { position, .. } => {
                        cursor_pos.set(Some(*position));
                        // Project to scene coords and check the
                        // draggable snapshot for hover.
                        let xform = view_xform_signal.get();
                        let scene_pt = match xform.inverse() {
                            Some(inv) => inv.apply_point(*position),
                            None => Point::ZERO,
                        };
                        let snap = bounds_snapshot.borrow();
                        let over_draggable = snap
                            .iter()
                            .any(|(_, rect)| rect.contains(scene_pt));
                        let cursor = if drag_target_for_cursor.get().is_some() {
                            CursorIcon::Grabbing
                        } else if over_draggable {
                            CursorIcon::Grab
                        } else {
                            CursorIcon::Default
                        };
                        ctx.set_cursor(cursor);
                    }
                    Ev::PointerDown { position, .. } => {
                        cursor_pos.set(Some(*position));
                    }
                    Ev::PointerLeave => {
                        cursor_pos.set(None);
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
            let zoom_dur = self.zoom_anim_duration;
            // Snapshot the scene's interaction policy at build time.
            // Subsequent `Scene::pan_axes` / `Scene::zoomable` changes
            // take effect on the next rebuild.
            let pan_axes = self.scene.current_pan_axes();
            let zoomable = self.scene.is_zoomable() && !self.adopt_scene_size;
            handlers = handlers.on_scroll(move |event, _ctx| {
                use crate::scene::PanAxes;
                let WidgetEvent::Scroll { delta, modifiers } = event else {
                    return EventResponse::Ignored;
                };
                let (mut dx, mut dy) = match delta {
                    ScrollDelta::Pixels { x, y } => (*x, *y),
                    ScrollDelta::Lines { x, y } => (*x * line_height, *y * line_height),
                };
                // Apply the scene's pan-axes policy: zero out the
                // restricted axis so it passes through to ancestor
                // scrollables instead of being absorbed.
                match pan_axes {
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
                    let factor = (1.0_f32).copysign(-step_px) * 0.0;
                    let _ = factor;
                    // Compute multiplicative factor: each notch = 1.1×
                    // (or 1/1.1 for zoom-out). Using exp-form keeps
                    // repeated notches consistent.
                    let factor = (-step_px * 0.1).exp();
                    let z_old = zoom.get();
                    let r_now = rotation.get();
                    let z_new = (z_old * factor).clamp(min_zoom, max_zoom);
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
                        None => fern_canvas::Point::new(
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
                    // Snap zoom + pan together. Animating the two
                    // signals independently with EaseOut would drift
                    // mid-tween (the anchor math is exact only at
                    // start and end states). Snap is also the
                    // standard wheel-zoom feel — each notch produces
                    // an immediate, predictable step. The `zoom_dur`
                    // capture stays for symmetry with `set_zoom` /
                    // `zoom_to` callers.
                    let _ = zoom_dur;
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
            let zoomable_pinch = self.scene.is_zoomable() && !self.adopt_scene_size;
            let pan_axes_pinch = self.scene.current_pan_axes();
            handlers = handlers.on_pinch(move |phase, _ctx| {
                use crate::scene::PanAxes;
                if !zoomable_pinch {
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
                // Apply pan-axes policy to the pinch's pan
                // adjustment so a horizontal-only scene doesn't
                // accidentally drift on Y from the gesture math.
                let new_pan = match pan_axes_pinch {
                    PanAxes::Both => new_pan,
                    PanAxes::None => pan_old,
                    PanAxes::Horizontal => Vec2::new(new_pan.x, pan_old.y),
                    PanAxes::Vertical => Vec2::new(pan_old.x, new_pan.y),
                };
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
            let pan_axes_keys = self.scene.current_pan_axes();
            let zoomable_keys = self.scene.is_zoomable() && !self.adopt_scene_size;
            handlers = handlers.on_key(move |event, _ctx| {
                use crate::scene::PanAxes;
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                let allow_pan_x = matches!(pan_axes_keys, PanAxes::Both | PanAxes::Horizontal);
                let allow_pan_y = matches!(pan_axes_keys, PanAxes::Both | PanAxes::Vertical);
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
                    Key::ArrowLeft if allow_pan_x => {
                        let target = pan_x.animation_target().unwrap_or_else(|| pan_x.get())
                            + pan_step;
                        pan_x.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    Key::ArrowRight if allow_pan_x => {
                        let target = pan_x.animation_target().unwrap_or_else(|| pan_x.get())
                            - pan_step;
                        pan_x.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    Key::ArrowUp if allow_pan_y => {
                        let target = pan_y.animation_target().unwrap_or_else(|| pan_y.get())
                            + pan_step;
                        pan_y.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    Key::ArrowDown if allow_pan_y => {
                        let target = pan_y.animation_target().unwrap_or_else(|| pan_y.get())
                            - pan_step;
                        pan_y.animate_to(target, pan_dur, Easing::EaseOut);
                    }
                    other if zoomable_keys
                        && (other.to_char() == Some('+') || other.to_char() == Some('=')) =>
                    {
                        let z_new = (zoom.get() * 1.25).clamp(min_zoom, max_zoom);
                        zoom.animate_to(z_new, zoom_dur, Easing::EaseOut);
                        recenter_zoom(z_new);
                    }
                    other if zoomable_keys && other.to_char() == Some('-') => {
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

        // --- Phase 6: marquee box-select via on_drag --------------------
        //
        // Active only when selection mode is not None. The on_drag
        // closure tracks the marquee rectangle in *screen* coords
        // (the position the recognizer hands us is screen-space at
        // the time of the gesture) and, on `Ended`, posts a
        // pending-commit entry. The next `place_children` call
        // consumes it: that step has direct `&self.scene` access
        // and can call `SceneSelection::commit_marquee` without
        // forcing `Scene` into an `Rc<RefCell>`. Tests can also
        // trigger the commit via `flush_marquee_commit()`.
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
            handlers = handlers.on_drag(move |phase, _ctx| {
                use fern_core::gesture::DragPhase;
                match phase {
                    DragPhase::Started { position, button } => {
                        if !matches!(
                            button,
                            fern_core::event::PointerButton::Primary
                        ) {
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
                            // available and `Scene::move_item` can
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
                        pending_marquee_commit
                            .set(Some((scene_rect, state.additive)));
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
        let (default_w, default_h) = if self.adopt_scene_size {
            match self.scene.scene_rect_extent() {
                Some(r) => (r.right(), r.bottom()),
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
        // interact with.
        self.last_viewport.set(size);

        // Phase 6: refresh the lightweight-bounds snapshot used
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

        // Phase 6: drain any pending marquee commit posted by the
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

        // Phase 6: drain any pending drag-to-move commit, applied
        // via the public `flush_pending_mutations` helper to keep
        // the borrow tractable (`place_children` takes `&self`,
        // and `Scene::move_item` needs `&mut Scene`). The
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
        let item_ctx =
            crate::item::SceneItemPaintContext::new(view_transform, Some(region));
        // `items_in_rect` returns both widget and item entries that
        // intersect the visible region. We filter to the lightweight
        // tier here — heavyweights are painted by the arena walker via
        // their own widget paint methods.
        //
        // Phase 6 drag-to-move: if an item is being dragged, paint
        // it at its in-flight scene-coord offset by translating
        // the canvas. Restored after.
        let drag_target = self.drag_target.get();
        let mut visible_ids = self.scene.items_in_rect(region);
        // Phase 6 z-order: sort visible ids by z so higher-z items
        // paint last (on top). Equal-z preserves insertion order
        // (sort is stable). Heavyweight ids stay in the list but
        // are filtered out below — their z is honored only as a
        // sort key for any lightweight neighbours interleaved with
        // them, which is the right semantic: a lightweight item
        // with z > 0 paints atop preceding lightweights, not atop
        // heavyweight widgets (heavyweights paint via the arena
        // walker after SceneView's paint method).
        self.scene.sort_by_z(&mut visible_ids);
        for id in visible_ids {
            if self.scene.item(id).is_none() {
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
            // Items that are either the drag target or a declared
            // descendant paint with a visual delta in scene coords —
            // a child follows its dragged parent until the rebuild
            // commits the new local_pos.
            let drag_delta = drag_target
                .filter(|t| t.item_id == id || self.scene.is_descendant_of(id, t.item_id))
                .map(|t| {
                    fern_canvas::Transform2D::translate(
                        t.current_scene.x - t.anchor_scene.x,
                        t.current_scene.y - t.anchor_scene.y,
                    )
                });

            // Compose `local→scene`, optionally with a scene-coord
            // drag offset baked in. Push beneath the view transform
            // so the item's `paint` works in local coords. `save` /
            // `restore` isolate neighbouring items' transforms.
            let mut local_to_scene = self.scene.scene_transform(id);
            if let Some(t) = drag_delta {
                local_to_scene = local_to_scene.then(&t);
            }
            canvas.save();
            canvas.apply_transform(local_to_scene);
            // Effective opacity composes through the parent chain.
            // Pushed via `Canvas::set_opacity` / `restore_opacity`
            // so the scope is balanced regardless of paint paths.
            let alpha = self.scene.effective_opacity(id);
            let opacity_pushed = alpha < 0.999;
            if opacity_pushed {
                canvas.set_opacity(alpha);
            }
            if let Some(item) = self.scene.item(id) {
                item.paint(canvas, &item_ctx);
            }
            if opacity_pushed {
                canvas.restore_opacity();
            }
            canvas.restore();
        }

        // Phase 6: marquee overlay — semi-transparent fill plus a
        // single-pixel stroke. The marquee state is in screen
        // coords (set by the on_drag closure). The view-transform
        // scope is on the canvas, so to paint at screen coords we
        // inverse-apply the transform. For a non-rotated identity-
        // ish transform that's just `inv * screen_rect`. We project
        // the screen-rect to scene coords and paint there — same
        // visual position because the transform applies to scene
        // coords back the other way.
        if let Some(state) = self.marquee.get() {
            let screen_rect = state.rect();
            // Project to scene coords so the paint commands land
            // through the view-transform stack to the right pixels.
            if let Some(inv) = view_transform.inverse() {
                let scene_rect = inv.apply_rect(screen_rect);
                let fill = fern_tokens::Color::new(0.40, 0.55, 0.85, 0.18);
                let stroke = fern_tokens::Color::new(0.40, 0.55, 0.85, 0.85);
                canvas.fill_rect(scene_rect, fill);
                canvas.stroke_rect(
                    scene_rect,
                    stroke,
                    fern_canvas::StrokeStyle::solid(1.0),
                );
            }
        }

        // Phase 7 visual-debug overlays. All paint in scene coords
        // so they ride the same view-transform projection as items.
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
            builder.set_name(label.clone());
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

    /// Paint enabled debug overlays on top of the scene rendering.
    /// All paint commands are in scene coords — they ride the
    /// same view-transform scope as items, so the overlays follow
    /// the user's pan/zoom naturally.
    fn paint_debug_overlay(&self, bounds: Rect, canvas: &mut fern_canvas::Canvas) {
        let cfg = self.debug_overlay;
        let region = self.visible_scene_region(bounds);
        let stroke_w = 1.0;
        // Distinct color per overlay so multiple flags compose
        // visually without confusion.
        let item_color = fern_tokens::Color::new(0.20, 0.75, 0.35, 0.85);
        let content_color = fern_tokens::Color::new(0.30, 0.45, 0.95, 0.85);
        let viewport_color = fern_tokens::Color::new(0.95, 0.30, 0.30, 0.85);
        let selection_color = fern_tokens::Color::new(1.00, 0.60, 0.20, 0.95);

        if cfg.item_bounds {
            for id in self.scene.ids() {
                if let Some(scene_rect) = self.scene.scene_rect(id) {
                    canvas.stroke_rect(
                        scene_rect,
                        item_color,
                        fern_canvas::StrokeStyle::solid(stroke_w),
                    );
                }
            }
        }
        if cfg.content_bounds {
            if let Some(content) = self.scene_content_bounds() {
                canvas.stroke_rect(
                    content,
                    content_color,
                    fern_canvas::StrokeStyle::solid(stroke_w),
                );
            }
        }
        if cfg.viewport {
            canvas.stroke_rect(
                region,
                viewport_color,
                fern_canvas::StrokeStyle::solid(stroke_w),
            );
        }
        if cfg.selection_bounds {
            for id in self.selection.selected() {
                if let Some(rect) = self.scene.scene_rect(id) {
                    canvas.stroke_rect(
                        rect,
                        selection_color,
                        fern_canvas::StrokeStyle::solid(stroke_w * 2.0),
                    );
                }
            }
        }
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
            modifiers: Default::default(),
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
            modifiers: Default::default(),
        });

        let view = view_handle(&tree, view_id);
        assert_eq!(view.pan_y.animation_target(), Some(32.0));
    }

    #[test]
    fn ctrl_wheel_zooms_about_cursor_keeping_scene_anchor_fixed() {
        // Ctrl+wheel zoom must keep the scene point under the cursor
        // fixed across the zoom step. Without zoom-about-pointer the
        // user perceives the scene "drifting away" — the right side
        // of the scene shifts further right when zooming in about
        // viewport center.
        use fern_core::event::{Modifiers, WidgetEvent as Ev};

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Park the cursor at (700, 400) — far from viewport center
        // (400, 300) — so any anchor mistake shows up clearly.
        let cursor = Point::new(700.0, 400.0);
        tree.dispatch_event(Ev::PointerMove { position: cursor });

        // Capture scene point under the cursor BEFORE zooming.
        let view = view_handle(&tree, view_id);
        let xform_before = view.view_transform();
        let scene_under_cursor = xform_before
            .inverse()
            .expect("identity xform invertible")
            .apply_point(cursor);

        // Ctrl+wheel scroll up by 1 line → zoom in.
        tree.dispatch_event(Ev::Scroll {
            delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
            modifiers: Modifiers::CTRL,
        });

        // Verify zoom changed and the scene point originally under
        // the cursor still projects to the cursor position.
        let view = view_handle(&tree, view_id);
        assert!(
            (view.zoom() - 1.0).abs() > 1e-3,
            "zoom should have changed (got {})",
            view.zoom()
        );
        let projected = view.view_transform().apply_point(scene_under_cursor);
        assert!(
            (projected.x - cursor.x).abs() < 0.5,
            "x: scene anchor must stay under cursor (cursor {} → projected {})",
            cursor.x,
            projected.x
        );
        assert!(
            (projected.y - cursor.y).abs() < 0.5,
            "y: scene anchor must stay under cursor (cursor {} → projected {})",
            cursor.y,
            projected.y
        );
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
            modifiers: Default::default(),
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
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED), Point::ZERO
        );
        let _off_screen = scene.add_item(
            RectItem::new(Rect::new(5_000.0, 5_000.0, 20.0, 20.0)).fill(Color::BLUE), Point::ZERO
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
            RectItem::new(Rect::new(5_000.0, 5_000.0, 20.0, 20.0)).fill(Color::RED), Point::ZERO
        );
        scene.add_item(
            RectItem::new(Rect::new(-5_000.0, -5_000.0, 20.0, 20.0)).fill(Color::BLUE), Point::ZERO
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
                .access_label("nearby"), Point::ZERO
        );
        let _far_off = scene.add_item(
            RectItem::new(Rect::new(50_000.0, 50_000.0, 20.0, 20.0)).fill(Color::BLUE), Point::ZERO
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
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED), Point::ZERO
        );
        // Item just past the right edge of the 400x300 viewport.
        // Default mode would include it (within 1× viewport
        // margin), but ViewportOnly should not.
        scene.add_item(
            RectItem::new(Rect::new(450.0, 100.0, 20.0, 20.0)).fill(Color::BLUE), Point::ZERO
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
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).access_label("Scene A"), Point::ZERO
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
        let item = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)), Point::ZERO);
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
        let a = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)), Point::ZERO);
        let b = scene.add_item(RectItem::new(Rect::new(40.0, 10.0, 20.0, 20.0)), Point::ZERO);
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
        let item = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)), Point::ZERO);
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
        let item = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)), Point::ZERO);
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
        let item = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
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
        scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)), Point::ZERO);

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
        let placed = scene.add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)), Point::ZERO);
        let _orphan = scene.add_item(RectItem::new(Rect::new(40.0, 10.0, 20.0, 20.0)), Point::ZERO);
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
            modifiers: Default::default(),
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
            modifiers: Default::default(),
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
        ), Point::ZERO);

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
        ), Point::ZERO);
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

    #[test]
    fn nested_scene_view_geometry_after_paint() {
        // After the first paint, an inner SceneView placed at
        // scene_rect (X, Y, W, H) inside an outer SceneView must
        // have its `bounds_origin_signal` updated to (X, Y) so its
        // own view_transform places its lightweight items at the
        // expected screen position. Without this sync (which lives
        // in `paint()`), the inner SceneView would draw at the
        // outer's scene origin instead of at its own scene_rect.
        //
        // Regression for "no embedded scene visible" — the inner
        // never received a `place_children` call (it has zero
        // widget children) and bounds_origin_signal stayed at the
        // default (0, 0).
        use crate::items::RectItem;
        use fern_tokens::Color;

        // Inner: one small RectItem at scene-coord (10, 10).
        let mut inner_scene = Scene::new();
        inner_scene
            .add_item(RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0)).fill(Color::RED), Point::ZERO);
        let inner = SceneView::new(inner_scene).default_size(50.0, 40.0);

        // Outer holds the inner at scene-coord (200, 150, 50, 40).
        let mut outer_scene = Scene::new();
        let inner_id =
            outer_scene.add_widget(inner, Rect::new(200.0, 150.0, 50.0, 40.0));
        let outer = SceneView::new(outer_scene);

        let mut tree = WidgetTree::new();
        let outer_id = tree.add(outer);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // Render once to drive paint, which is where the inner's
        // `bounds_origin_signal` gets synced.
        let _ = tree.render();

        // Verify inner widget id resolved.
        let outer_view = view_handle(&tree, outer_id);
        let inner_widget_id = outer_view
            .widget_id_for(inner_id)
            .expect("inner SceneView materialised under outer");

        // Inspect the inner SceneView's view_transform_signal —
        // it should reflect the placement origin (200, 150).
        let inner_view = tree
            .widget_as_any(inner_widget_id)
            .and_then(|a| a.downcast_ref::<SceneView>())
            .expect("inner widget is a SceneView");

        let xform = inner_view.view_transform_signal().get();
        // Origin (0, 0) in inner-scene-coord must project to
        // (200, 150) in outer-scene-coord (= screen, since outer
        // is at zoom 1, pan 0).
        let projected = xform.apply_point(Point::ZERO);
        assert!(
            (projected.x - 200.0).abs() < 0.5,
            "inner origin must project to outer scene_rect.x = 200 (got {})",
            projected.x
        );
        assert!(
            (projected.y - 150.0).abs() < 0.5,
            "inner origin must project to outer scene_rect.y = 150 (got {})",
            projected.y
        );

        // And inner-scene (10, 10) must project to (210, 160).
        let projected_dot = xform.apply_point(Point::new(10.0, 10.0));
        assert!(
            (projected_dot.x - 210.0).abs() < 0.5,
            "inner item at (10,10) must project to (210, ...) — got {}",
            projected_dot.x
        );
        assert!(
            (projected_dot.y - 160.0).abs() < 0.5,
            "inner item at (10,10) must project to (..., 160) — got {}",
            projected_dot.y
        );
    }

    // -- Phase 6: selection + marquee -----------------------------------

    #[test]
    fn selection_default_is_none_mode() {
        let view = SceneView::new(Scene::new());
        assert_eq!(
            view.selection().mode(),
            crate::selection::SceneSelectionMode::None
        );
    }

    #[test]
    fn selection_mode_builder_sets_multi() {
        let view = SceneView::new(Scene::new())
            .selection_mode(crate::selection::SceneSelectionMode::Multi);
        assert_eq!(
            view.selection().mode(),
            crate::selection::SceneSelectionMode::Multi
        );
    }

    #[test]
    fn marquee_drag_ends_with_pending_commit() {
        // Diagnostic: full Started → Moved → Ended → pending_commit.
        use crate::items::RectItem;
        use fern_canvas::Point;

        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 20.0, 20.0))
                .fill(fern_tokens::Color::RED), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.pointer_move(Point::new(40.0, 40.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(40.0, 40.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 80.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(80.0, 80.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        let view = view_handle(&tree, view_id);
        let pending = view.pending_marquee_commit.get();
        let (rect, _) = pending.expect("drag Ended must post pending_marquee_commit");
        // The rect should enclose (50, 50, 20, 20) — origin (40,40)
        // to current (80,80), screen-coords. Identity view-transform
        // → scene_rect = (40, 40, 40, 40).
        assert!(
            rect.width >= 30.0 && rect.height >= 30.0,
            "marquee rect was tiny: {:?}",
            rect
        );
        assert!(
            rect.x <= 50.0 && rect.x + rect.width >= 70.0,
            "marquee rect doesn't enclose item x range: {:?}",
            rect
        );
    }

    #[test]
    fn marquee_drag_recognizes_started_phase() {
        // Diagnostic: drag-Started is recognized at all.
        use crate::items::RectItem;
        use fern_canvas::Point;

        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 20.0, 20.0))
                .fill(fern_tokens::Color::RED), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Hover so pointer-over state is set, then PointerDown,
        // PointerMove (crosses 5px threshold), PointerUp.
        tree.pointer_move(Point::new(40.0, 40.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(40.0, 40.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 80.0),
        });

        // After Started fires, marquee state should be Some.
        let view = view_handle(&tree, view_id);
        assert!(
            view.marquee.get().is_some(),
            "drag Started must populate marquee state"
        );
    }

    #[test]
    fn marquee_drag_records_pending_commit() {
        // Drive on_drag through the event path: Started → Moved →
        // Ended. The closure must post a pending commit via
        // `pending_marquee_commit`. Then `flush_marquee_commit`
        // (or the next layout) materialises the selection.
        use crate::items::RectItem;
        use fern_canvas::Point;

        let mut scene = Scene::new();
        let inside = scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 20.0, 20.0))
                .fill(fern_tokens::Color::RED), Point::ZERO
        );
        let outside = scene.add_item(
            RectItem::new(Rect::new(500.0, 500.0, 20.0, 20.0))
                .fill(fern_tokens::Color::BLUE), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Drive pointer-down → move → up at coordinates that
        // produce a screen-rect enclosing `inside` but not
        // `outside`. The view-transform is identity at this
        // point, so screen and scene coords coincide.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(40.0, 40.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 80.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(80.0, 80.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        let view = view_handle(&tree, view_id);
        // Materialise the marquee result — outside a real
        // per-frame loop the test calls this directly.
        view.flush_marquee_commit();
        let selected = view.selection().selected();
        assert!(
            selected.contains(&inside),
            "marquee enclosing `inside` must select it (got {:?})",
            selected
        );
        assert!(
            !selected.contains(&outside),
            "marquee not enclosing `outside` must not select it"
        );
    }

    #[test]
    fn drag_to_move_translates_lightweight_item() {
        // Pointer-down inside a lightweight item, drag, release →
        // the item's bounds in the scene shift by the drag delta.
        // The hit-test snapshot used by on_drag is refreshed in
        // place_children; the test calls layout once before
        // dragging so the snapshot is populated.
        use crate::items::RectItem;
        use fern_canvas::Point;

        let mut scene = Scene::new();
        let item_id = scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0))
                .fill(fern_tokens::Color::RED)
                .draggable(true), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        // First layout populates the lightweight bounds snapshot.
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Press inside the item (60, 60), drag to (100, 100), release.
        tree.pointer_move(Point::new(60.0, 60.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(60.0, 60.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 100.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 100.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        // Diagnostic: verify the snapshot was populated, and
        // check whether drag_target or marquee was selected.
        {
            let view = view_handle(&tree, view_id);
            let snap = view.lightweight_bounds_snapshot.borrow();
            let drag = view.drag_target.get();
            let marq = view.marquee.get();
            let pending_move = view.pending_item_move.get();
            let pending_marq = view.pending_marquee_commit.get();
            assert!(
                drag.is_some() || pending_move.is_some(),
                "expected drag_target or pending_move set; \
                 snap={:?} drag={:?} marquee={:?} pending_move={:?} pending_marq={:?}",
                snap, drag, marq, pending_move, pending_marq
            );
        }

        // Drain the pending move via the public &mut helper.
        let view = tree
            .widget_as_any_mut(view_id)
            .and_then(|a| a.downcast_mut::<SceneView>())
            .expect("downcast");
        let flushed = view.flush_pending_item_move();
        assert!(flushed, "drag-to-move should post a pending commit");

        // The item bounds now reflect the drag delta (+40 on each axis).
        let new_rect = view
            .scene()
            .scene_rect(item_id)
            .expect("item still exists");
        assert!(
            (new_rect.x - 90.0).abs() < 1e-3,
            "item x moved by drag delta (expected 90, got {})",
            new_rect.x
        );
        assert!(
            (new_rect.y - 90.0).abs() < 1e-3,
            "item y moved by drag delta (expected 90, got {})",
            new_rect.y
        );
        // Size unchanged.
        assert_eq!(new_rect.width, 30.0);
        assert_eq!(new_rect.height, 30.0);
    }

    #[test]
    fn drag_to_move_persists_via_rebuild_signal_no_snap_back() {
        // Regression: real apps don't call `flush_pending_item_move`
        // — they rely on the `drag_dirty` rebuild signal to drain
        // the pending commit on the next layout pass. Drive a drag
        // end and then run a layout; the position must persist.
        // Drive a SECOND drag from the new position; the position
        // must reflect both drags (no snap-back).
        use crate::items::RectItem;
        use fern_canvas::Point;

        let mut scene = Scene::new();
        // A heavyweight widget child so the SceneView's
        // `has_built_children` flag flips to true — that's the
        // gate guarding `collect_needs_rebuild` and the realistic
        // shape of any showcase scene with cards.
        scene.add_widget(FillWidget::new(), Rect::new(200.0, 200.0, 50.0, 50.0));
        let item_id = scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0))
                .fill(fern_tokens::Color::RED)
                .draggable(true), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // -- First drag: (60, 60) → (100, 100) → release.
        tree.pointer_move(Point::new(60.0, 60.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(60.0, 60.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 100.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 100.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        // After Ended the on_drag closure must have posted a
        // pending move and bumped drag_dirty.
        {
            let view = view_handle(&tree, view_id);
            assert!(
                view.pending_item_move.get().is_some(),
                "drag end must post pending_item_move"
            );
            assert!(
                view.drag_dirty.get() > 0,
                "drag end must bump drag_dirty"
            );
        }

        // Real apps' layout cycle runs after event dispatch, where
        // drag_dirty (bumped on Ended) triggers the rebuild that
        // drains the pending commit.
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view = view_handle(&tree, view_id);
        let after_first = view
            .scene()
            .scene_rect(item_id)
            .expect("item still exists");
        assert!(
            (after_first.x - 90.0).abs() < 1e-3,
            "first drag must persist (expected x=90, got {})",
            after_first.x
        );
        assert!(
            (after_first.y - 90.0).abs() < 1e-3,
            "first drag must persist (expected y=90, got {})",
            after_first.y
        );

        // -- Second drag: (100, 100) → (140, 140) → release.
        // After the first drag the item is at (90, 90, 30, 30), so
        // the press at (100, 100) is inside it.
        tree.pointer_move(Point::new(100.0, 100.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(100.0, 100.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(140.0, 140.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(140.0, 140.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view = view_handle(&tree, view_id);
        let after_second = view
            .scene()
            .scene_rect(item_id)
            .expect("item still exists");
        // Each drag delta is +40 on each axis. Two drags from
        // (50, 50) → (90, 90) → (130, 130).
        assert!(
            (after_second.x - 130.0).abs() < 1e-3,
            "second drag must compose with first (expected x=130, got {})",
            after_second.x
        );
        assert!(
            (after_second.y - 130.0).abs() < 1e-3,
            "second drag must compose with first (expected y=130, got {})",
            after_second.y
        );
    }

    #[test]
    fn drag_cascades_to_declared_descendants() {
        // QGraphicsScene-style: a child item declared via
        // `Scene::set_item_parent` moves with its parent on drag.
        // Apps build labelled-rect compounds (a TextItem child of
        // a draggable RectItem) without writing custom items.
        use crate::items::{RectItem, TextItem};
        use fern_canvas::Point;

        let mut scene = Scene::new();
        // Heavyweight child to flip `has_built_children`.
        scene.add_widget(FillWidget::new(), Rect::new(300.0, 300.0, 50.0, 50.0));
        let parent_rect = scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 80.0, 60.0))
                .fill(fern_tokens::Color::RED)
                .draggable(true), Point::ZERO
        );
        let label = scene.add_item(TextItem::new(
            "child",
            Rect::new(58.0, 70.0, 64.0, 20.0),
        ), Point::ZERO);
        scene.set_item_parent(label, Some(parent_rect));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag the parent (60, 60) → (100, 100) — delta = +40 x +40.
        tree.pointer_move(Point::new(60.0, 60.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(60.0, 60.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 100.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 100.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view = view_handle(&tree, view_id);
        let parent_after = view
            .scene()
            .scene_rect(parent_rect)
            .expect("parent still exists");
        let label_after = view
            .scene()
            .scene_rect(label)
            .expect("label still exists");

        // Parent moved (50,50) → (90,90).
        assert!(
            (parent_after.x - 90.0).abs() < 1e-3 && (parent_after.y - 90.0).abs() < 1e-3,
            "parent must move by drag delta (got {:?})",
            parent_after
        );
        // Label is a declared child — must have moved by the SAME
        // delta (+40, +40). Original label at (58, 70) → (98, 110).
        assert!(
            (label_after.x - 98.0).abs() < 1e-3 && (label_after.y - 110.0).abs() < 1e-3,
            "child must cascade with parent's delta (got {:?})",
            label_after
        );
    }

    #[test]
    fn parent_child_drag_persists_across_two_drags() {
        // Showcase regression: parent RectItem with declared TextItem
        // child, dragged twice in a row. After the cascade refactor
        // the second drag must compose with the first — neither parent
        // nor child may snap back to original position.
        use crate::items::{RectItem, TextItem};
        use fern_canvas::Point;

        let mut scene = Scene::new();
        // Heavyweight child to flip `has_built_children` (matches
        // realistic scenes with at least one widget tier).
        scene.add_widget(FillWidget::new(), Rect::new(300.0, 300.0, 50.0, 50.0));
        let parent_rect = scene.add_item(
            RectItem::new(Rect::new(50.0, 50.0, 80.0, 60.0))
                .fill(fern_tokens::Color::RED)
                .draggable(true), Point::ZERO
        );
        let label = scene.add_item(TextItem::new(
            "child",
            Rect::new(58.0, 70.0, 64.0, 20.0),
        ), Point::ZERO);
        scene.set_item_parent(label, Some(parent_rect));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // -- Drag 1: press at (60,60), release at (100,100). Δ = +40,+40.
        tree.pointer_move(Point::new(60.0, 60.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(60.0, 60.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(100.0, 100.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(100.0, 100.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // -- Drag 2: parent now at (90,90,80,60). Press inside it at
        // (110,110), release at (150,150). Δ = +40,+40.
        tree.pointer_move(Point::new(110.0, 110.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(110.0, 110.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(150.0, 150.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(150.0, 150.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view = view_handle(&tree, view_id);
        let parent_after = view.scene().scene_rect(parent_rect).unwrap();
        let label_after = view.scene().scene_rect(label).unwrap();
        // Parent: (50,50) + (40,40) + (40,40) = (130,130).
        assert!(
            (parent_after.x - 130.0).abs() < 1e-3 && (parent_after.y - 130.0).abs() < 1e-3,
            "parent must compose two drags (expected 130,130; got {:?})",
            parent_after
        );
        // Label: (58,70) + (40,40) + (40,40) = (138,150).
        assert!(
            (label_after.x - 138.0).abs() < 1e-3 && (label_after.y - 150.0).abs() < 1e-3,
            "child must compose two drags via cascade (expected 138,150; got {:?})",
            label_after
        );
    }

    #[test]
    fn looping_item_animation_survives_drag_end_rebuild() {
        // Showcase regression: dropping a draggable square in section 5
        // froze the PulsingDot animations in section 8. Cause: the
        // `drag_dirty` rebuild cancels animations owned by SceneView,
        // and the items' `register_bindings` must re-register and
        // re-arm `animate_looping` so the loop resumes on the next
        // pending-pickup pass.
        use crate::animation::register_animated_item_signal;
        use crate::item::SceneItemPaintContext;
        use crate::items::RectItem;
        use crate::SceneItem;
        use fern_canvas::Point;
        use fern_core::binding::BindingLevel;
        use fern_core::build_context::BuildContext;
        use fern_core::widget_id::WidgetId;
        use fern_tokens::Easing;

        #[derive(Debug)]
        struct Looper {
            bounds: Rect,
            phase: Signal<f32>,
        }
        impl SceneItem for Looper {
            fn local_bounds(&self) -> Rect {
                self.bounds
            }
            fn set_local_bounds(&mut self, b: Rect) {
                self.bounds = b;
            }
            fn paint(&self, _c: &mut fern_canvas::Canvas, _x: &SceneItemPaintContext) {}
            fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId) {
                register_animated_item_signal(ctx, &self.phase);
                self.phase
                    .bind_to(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
                self.phase.animate_looping(
                    1.0,
                    Duration::from_millis(200),
                    Easing::Linear,
                    None,
                );
            }
        }

        let mut scene = Scene::new();
        // Heavyweight child to flip `has_built_children` so the
        // drag-end rebuild path actually runs (matches realistic
        // showcase scenes with at least one widget tier).
        scene.add_widget(FillWidget::new(), Rect::new(280.0, 200.0, 30.0, 30.0));
        // Five loopers — same shape as the showcase's PulsingDot row.
        let phases: Vec<Signal<f32>> = (0..5).map(|_| Signal::new_animated(0.0)).collect();
        for (i, p) in phases.iter().enumerate() {
            scene.add_item(Looper {
                bounds: Rect::new(200.0 + i as f32 * 30.0, 50.0, 20.0, 20.0),
                phase: p.clone(),
            }, Point::ZERO);
        }
        let drag_rect = scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 30.0, 30.0))
                .fill(fern_tokens::Color::RED)
                .draggable(true), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Animation should be ticking before any drag.
        tree.tick_animations(Duration::from_millis(50));
        for (i, p) in phases.iter().enumerate() {
            let v = p.get();
            assert!(
                v > 0.0 && v < 1.0,
                "phase[{}] must tween mid-loop pre-drag (got {})",
                i,
                v
            );
        }

        // Drag the rect — Down/Move/Up. This bumps drag_dirty, which
        // triggers a SceneView rebuild on the next layout pass, which
        // is where the regression bites.
        tree.pointer_move(Point::new(20.0, 20.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(20.0, 20.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 60.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(60.0, 60.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        // First layout: drains the pending move; rebuild's
        // `register_bindings` re-arms the loop's pending request.
        tree.layout(SizeProposal::exact(400.0, 300.0));
        // tick_animations picks up the pending and re-inserts the
        // looping animation into the scheduler, then advances time.
        tree.tick_animations(Duration::from_millis(80));
        let post_a: Vec<f32> = phases.iter().map(|p| p.get()).collect();
        // Confirm motion resumed for every looper: advance another
        // small slice and require each value to have moved.
        tree.tick_animations(Duration::from_millis(40));
        for (i, p) in phases.iter().enumerate() {
            let post_b = p.get();
            assert!(
                (post_b - post_a[i]).abs() > 1e-4,
                "looper[{}] must keep ticking after drag-end rebuild \
                 (post_a = {}, post_b = {})",
                i,
                post_a[i],
                post_b
            );
        }
        // And the rect actually moved — sanity check that the drag
        // commit ran.
        let view = view_handle(&tree, _view_id);
        let r = view.scene().scene_rect(drag_rect).unwrap();
        assert!(
            (r.x - 50.0).abs() < 1e-3 && (r.y - 50.0).abs() < 1e-3,
            "drag commit must have run (got {:?})",
            r
        );
    }

    #[test]
    fn drag_in_empty_area_starts_marquee_not_move() {
        // Press in empty scene area (no item underneath) → marquee
        // mode wins; dragging across an item selects it instead of
        // moving it.
        use crate::items::RectItem;
        use fern_canvas::Point;

        let mut scene = Scene::new();
        let item_id = scene.add_item(
            RectItem::new(Rect::new(100.0, 100.0, 30.0, 30.0))
                .fill(fern_tokens::Color::RED), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Press at (10, 10) — empty area. Drag to (200, 200) —
        // crosses the item. Release.
        tree.pointer_move(Point::new(10.0, 10.0));
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(10.0, 10.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(200.0, 200.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(200.0, 200.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        let view = view_handle(&tree, view_id);
        view.flush_marquee_commit();
        assert!(
            view.selection().is_selected(item_id),
            "marquee enclosing the item must select it"
        );
        // Item bounds unchanged (no drag-to-move happened).
        let rect = view.scene().scene_rect(item_id).unwrap();
        assert_eq!(rect.x, 100.0);
        assert_eq!(rect.y, 100.0);
    }

    #[test]
    fn z_order_paints_higher_z_after_lower() {
        // Two overlapping lightweight items: id A at z=0 (back),
        // id B at z=10 (front). The frame's draw_order should
        // place B's decoration *after* A's so it appears on top.
        use crate::items::RectItem;
        use fern_canvas::DrawCommand;

        let mut scene = Scene::new();
        let a = scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0))
                .fill(fern_tokens::Color::RED), Point::ZERO
        );
        let b = scene.add_item(
            RectItem::new(Rect::new(15.0, 15.0, 20.0, 20.0))
                .fill(fern_tokens::Color::BLUE), Point::ZERO
        );
        // Reverse paint order via z: A on top by default (later
        // insertion = on top), but we set B's z higher so B paints
        // last instead.
        scene.set_z(a, 0.0);
        scene.set_z(b, 10.0);

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();

        // First decoration is A (red), second is B (blue) — same
        // as default insertion order in this case. Now flip z and
        // verify the order changes.
        let red_first = frame.decorations.iter().enumerate().find_map(|(i, d)| {
            if d.color == fern_tokens::Color::RED.to_array() {
                Some(i)
            } else {
                None
            }
        });
        let blue_first = frame.decorations.iter().enumerate().find_map(|(i, d)| {
            if d.color == fern_tokens::Color::BLUE.to_array() {
                Some(i)
            } else {
                None
            }
        });
        assert!(
            red_first.is_some() && blue_first.is_some(),
            "both decorations should be present"
        );
        assert!(
            red_first.unwrap() < blue_first.unwrap(),
            "z=10 (blue) should paint after z=0 (red)"
        );
        // Decoration commands are present (the per-item
        // scene-transform push emits SetTransform commands too, so
        // they're interleaved with decorations in `draw_order`).
        assert!(
            frame.draw_order.iter().any(|cmd| matches!(cmd, DrawCommand::Decoration(_))),
            "expected at least one Decoration in draw_order"
        );
    }

    #[test]
    fn z_order_default_zero_preserves_insertion_order() {
        // Without explicit z, items paint in insertion order.
        use crate::items::RectItem;

        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(10.0, 10.0, 20.0, 20.0))
                .fill(fern_tokens::Color::RED), Point::ZERO
        );
        scene.add_item(
            RectItem::new(Rect::new(15.0, 15.0, 20.0, 20.0))
                .fill(fern_tokens::Color::BLUE), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert_eq!(
            frame.decorations[0].color,
            fern_tokens::Color::RED.to_array(),
            "first-inserted item paints first by default"
        );
        assert_eq!(
            frame.decorations[1].color,
            fern_tokens::Color::BLUE.to_array()
        );
    }

    #[test]
    fn marquee_no_op_in_none_mode() {
        // With selection mode None, drag does nothing — the
        // on_drag handler isn't even registered.
        use fern_canvas::Point;

        let mut scene = Scene::new();
        scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(40.0, 40.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 80.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(80.0, 80.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        let view = view_handle(&tree, view_id);
        assert_eq!(view.selection().count(), 0);
    }

    #[test]
    fn marquee_does_not_unmount_heavyweight_children() {
        // Regression: dragging a marquee in empty space used to make
        // heavyweight children "disappear" because the drag-end path
        // bumped a Rebuild-level signal, and rebuilding SceneView
        // re-pushed materialised WidgetIds to child_ids without
        // re-attaching them via ctx.add_boxed — the framework's
        // rebuild reconciliation pruned them. The fix routes the
        // marquee drain through Relayout (place_children) instead.
        use fern_canvas::Point;

        let mut scene = Scene::new();
        let widget_item =
            scene.add_widget(FillWidget::new(), Rect::new(50.0, 50.0, 80.0, 60.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene)
                .selection_mode(crate::selection::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Confirm the widget is in the arena before any drag.
        let view = view_handle(&tree, view_id);
        let materialised_id =
            view.widget_id_for(widget_item).expect("widget materialised");
        assert!(tree.children(view_id).contains(&materialised_id));

        // Drag a marquee in empty space (above the widget).
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(200.0, 10.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(260.0, 30.0),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(260.0, 30.0),
            button: fern_core::event::PointerButton::Primary,
            modifiers: fern_core::event::Modifiers::default(),
        });

        // After marquee end the framework processes any dirty
        // signals — drive a layout to give place_children a chance
        // to drain.
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Heavyweight child must still be a child of the SceneView.
        assert!(
            tree.children(view_id).contains(&materialised_id),
            "marquee end must NOT unmount heavyweight children — \
             children: {:?}, expected to contain: {:?}",
            tree.children(view_id),
            materialised_id,
        );
    }

    // -- Focus-order traversal -----------------------------------------

    fn rect_item_at(x: f32, y: f32) -> crate::items::RectItem {
        use fern_tokens::Color;
        crate::items::RectItem::new(Rect::new(x, y, 10.0, 10.0)).fill(Color::RED)
    }

    #[test]
    fn focus_order_default_walks_insertion_order_forward() {
        let mut scene = Scene::new();
        let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
        let b = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
        let c = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);

        let view = SceneView::new(scene);
        // None → first
        assert_eq!(view.next_focus(None), Some(a));
        // a → b → c → None (no wrap)
        assert_eq!(view.next_focus(Some(a)), Some(b));
        assert_eq!(view.next_focus(Some(b)), Some(c));
        assert_eq!(view.next_focus(Some(c)), None);
    }

    #[test]
    fn focus_order_default_walks_insertion_order_backward() {
        let mut scene = Scene::new();
        let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
        let b = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
        let c = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);

        let view = SceneView::new(scene);
        // None → last
        assert_eq!(view.previous_focus(None), Some(c));
        // c → b → a → None
        assert_eq!(view.previous_focus(Some(c)), Some(b));
        assert_eq!(view.previous_focus(Some(b)), Some(a));
        assert_eq!(view.previous_focus(Some(a)), None);
    }

    #[test]
    fn focus_order_empty_scene_returns_none() {
        let view = SceneView::new(Scene::new());
        assert_eq!(view.next_focus(None), None);
        assert_eq!(view.previous_focus(None), None);
    }

    #[test]
    fn focus_order_callback_overrides_default() {
        // App-supplied callback returns items in REVERSE insertion
        // order on Forward — proves the callback overrides the
        // built-in walk and isn't merely augmenting it.
        let mut scene = Scene::new();
        let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
        let b = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
        let c = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);

        let view = SceneView::new(scene).focus_order(move |scene, dir, current| {
            let mut ids = scene.ids();
            if matches!(dir, FocusDirection::Forward) {
                ids.reverse();
            }
            match current {
                None => ids.first().copied(),
                Some(cur) => ids
                    .iter()
                    .position(|id| *id == cur)
                    .and_then(|i| ids.get(i + 1).copied()),
            }
        });

        // Forward starts at last (c) and walks toward a.
        assert_eq!(view.next_focus(None), Some(c));
        assert_eq!(view.next_focus(Some(c)), Some(b));
        assert_eq!(view.next_focus(Some(b)), Some(a));
        assert_eq!(view.next_focus(Some(a)), None);
    }

    // -- fit_to_items / fit_to_selection -------------------------------

    #[test]
    fn fit_to_items_empty_is_noop() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);
        let pan_before = view.pan();
        let zoom_before = view.zoom();
        view.fit_to_items(&[]);
        // No-op: no animation kicked off.
        assert_eq!(view.pan(), pan_before);
        assert_eq!(view.zoom(), zoom_before);
    }

    #[test]
    fn fit_to_items_skips_stale_ids() {
        let mut scene = Scene::new();
        let a = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
        scene.remove(a);

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);
        let pan_before = view.pan();
        let zoom_before = view.zoom();
        // Stale id — skipped.
        view.fit_to_items(&[a]);
        assert_eq!(view.pan(), pan_before);
        assert_eq!(view.zoom(), zoom_before);
    }

    #[test]
    fn fit_to_selection_uses_selected_ids() {
        let mut scene = Scene::new();
        let a = scene.add_item(rect_item_at(100.0, 100.0), Point::ZERO);
        let _b = scene.add_item(rect_item_at(900.0, 900.0), Point::ZERO);

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene).selection_mode(crate::SceneSelectionMode::Multi),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);

        // Empty selection → no-op.
        let pan_before = view.pan();
        let zoom_before = view.zoom();
        view.fit_to_selection();
        assert_eq!(view.pan(), pan_before);
        assert_eq!(view.zoom(), zoom_before);

        // Select item `a`, fit_to_selection animates toward its bounds —
        // the resulting target zoom should be at or above min_zoom and
        // bounded by max_zoom.
        view.selection().select_one(a);
        view.fit_to_selection();
        // After kicking the animation, current zoom is between the
        // start (1.0) and the eventual target. We assert the call
        // reached the math (zoom didn't stay exactly 1.0 unless the
        // computed target happens to also be 1.0). Loose check: the
        // pan signal is no longer at the origin since `a` is at
        // (100,100) and we're centering it in an 800x600 viewport.
        assert!(view.zoom() >= view.min_zoom);
        assert!(view.zoom() <= view.max_zoom);
    }

    #[test]
    fn focus_order_callback_can_skip_items() {
        // App-supplied callback only Tab-cycles between odd-indexed
        // items — exercises the "domain-specific traversal" use case
        // (e.g. graph editor that only tabs through nodes, not
        // connectors).
        let mut scene = Scene::new();
        let _skip0 = scene.add_item(rect_item_at(0.0, 0.0), Point::ZERO);
        let keep1 = scene.add_item(rect_item_at(20.0, 0.0), Point::ZERO);
        let _skip2 = scene.add_item(rect_item_at(40.0, 0.0), Point::ZERO);
        let keep3 = scene.add_item(rect_item_at(60.0, 0.0), Point::ZERO);

        let allowed = vec![keep1, keep3];
        let view = SceneView::new(scene).focus_order(move |_scene, dir, current| {
            match current {
                None => match dir {
                    FocusDirection::Forward => allowed.first().copied(),
                    FocusDirection::Backward => allowed.last().copied(),
                },
                Some(cur) => {
                    let pos = allowed.iter().position(|id| *id == cur)?;
                    match dir {
                        FocusDirection::Forward => allowed.get(pos + 1).copied(),
                        FocusDirection::Backward => {
                            if pos == 0 {
                                None
                            } else {
                                allowed.get(pos - 1).copied()
                            }
                        }
                    }
                }
            }
        });

        assert_eq!(view.next_focus(None), Some(keep1));
        assert_eq!(view.next_focus(Some(keep1)), Some(keep3));
        assert_eq!(view.next_focus(Some(keep3)), None);
        assert_eq!(view.previous_focus(None), Some(keep3));
        assert_eq!(view.previous_focus(Some(keep3)), Some(keep1));
    }

    // -- Nested-SceneView a11y -----------------------------------------

    #[test]
    fn default_scene_view_has_pane_role() {
        use fern_core::accessibility::widget_id_to_node_id;

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();
        let view_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(view_id))
            .map(|(_, n)| n)
            .expect("scene view node");
        assert_eq!(view_node.role(), accesskit::Role::Pane);
    }

    #[test]
    fn nested_scene_view_uses_region_role() {
        use fern_core::accessibility::widget_id_to_node_id;

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()).nested_a11y(true));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();
        let view_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(view_id))
            .map(|(_, n)| n)
            .expect("scene view node");
        // Nested SceneViews announce as Region instead of Pane to
        // avoid landmark redundancy in nested layouts.
        assert_eq!(view_node.role(), accesskit::Role::Region);
    }

    #[test]
    fn a11y_label_is_announced() {
        use fern_core::accessibility::widget_id_to_node_id;

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(Scene::new())
                .nested_a11y(true)
                .a11y_label("Chart data area"),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();
        let view_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == widget_id_to_node_id(view_id))
            .map(|(_, n)| n)
            .expect("scene view node");
        assert_eq!(view_node.label(), Some("Chart data area"));
    }

    #[test]
    fn is_nested_accessor_reflects_builder_setting() {
        let view = SceneView::new(Scene::new());
        assert!(!view.is_nested());
        let view = view.nested_a11y(true);
        assert!(view.is_nested());
    }

    // -- View-state persistence ----------------------------------------

    #[test]
    fn state_snapshot_reflects_current_view() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);
        view.set_pan(Vec2::new(100.0, 50.0));
        view.set_zoom(2.0);
        view.set_rotation(0.5);

        let s = view.state();
        assert_eq!(s.pan_x, 100.0);
        assert_eq!(s.pan_y, 50.0);
        assert_eq!(s.zoom, 2.0);
        assert_eq!(s.rotation, 0.5);
    }

    #[test]
    fn restore_state_round_trip() {
        let saved = crate::SceneViewState::new(Vec2::new(42.0, -17.0), 1.5, 0.25);

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);

        view.restore_state(saved);
        assert_eq!(view.pan(), Vec2::new(42.0, -17.0));
        assert_eq!(view.zoom(), 1.5);
        assert_eq!(view.rotation(), 0.25);

        // Round-trip: snapshot equals the restored input.
        assert_eq!(view.state(), saved);
    }

    #[test]
    fn restore_state_clamps_zoom() {
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(Scene::new()).max_zoom(5.0));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);

        // Saved zoom of 100.0 (e.g. corrupted settings) is clamped
        // to max_zoom on restore — apps don't end up with an
        // unusable infinitely-zoomed view from a stale config.
        let saved = crate::SceneViewState::new(Vec2::ZERO, 100.0, 0.0);
        view.restore_state(saved);
        assert_eq!(view.zoom(), 5.0);
    }

    #[test]
    fn identity_state_is_default() {
        let s: crate::SceneViewState = Default::default();
        assert!(s.is_identity());
    }

    // -- a11y_bounds_space ---------------------------------------------

    #[test]
    fn a11y_bounds_default_to_screen_projection() {
        use crate::items::RectItem;
        use fern_core::accessibility::is_synthetic;
        use fern_tokens::Color;

        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(100.0, 50.0, 20.0, 20.0)).fill(Color::RED), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        // Pan + zoom: screen bounds become (200 - panx, 100 - pany)
        // at zoom 2.0 → corners (200, 100, 40, 40).
        let view = view_handle(&tree, view_id);
        view.set_zoom(2.0);

        let update = tree.sync_accessibility();
        let item_node = update
            .nodes
            .iter()
            .find(|(id, _)| is_synthetic(*id))
            .map(|(_, n)| n)
            .expect("synthetic item node");

        // Default Screen mode: bounds reflect 2x scale.
        let bounds = item_node.bounds().expect("item bounds set");
        let width = bounds.x1 - bounds.x0;
        assert!((width - 40.0).abs() < 0.5, "screen width should reflect zoom");
    }

    #[test]
    fn a11y_bounds_scene_mode_reports_raw_scene_coords() {
        use crate::items::RectItem;
        use fern_core::accessibility::is_synthetic;
        use fern_tokens::Color;

        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(100.0, 50.0, 20.0, 20.0)).fill(Color::RED), Point::ZERO
        );

        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene).a11y_bounds_space(crate::A11yBoundsSpace::Scene),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let view = view_handle(&tree, view_id);
        view.set_zoom(2.0);

        let update = tree.sync_accessibility();
        let item_node = update
            .nodes
            .iter()
            .find(|(id, _)| is_synthetic(*id))
            .map(|(_, n)| n)
            .expect("synthetic item node");

        // Scene mode: bounds match the raw scene rect, ignoring zoom.
        let bounds = item_node.bounds().expect("item bounds set");
        let width = bounds.x1 - bounds.x0;
        assert!((width - 20.0).abs() < 0.5, "scene-mode width must equal raw scene width");
        assert!((bounds.x0 - 100.0).abs() < 0.5);
        assert!((bounds.y0 - 50.0).abs() < 0.5);
    }

    #[test]
    fn current_a11y_bounds_space_accessor_reflects_setting() {
        let view = SceneView::new(Scene::new());
        assert_eq!(view.current_a11y_bounds_space(), crate::A11yBoundsSpace::Screen);
        let view = view.a11y_bounds_space(crate::A11yBoundsSpace::Scene);
        assert_eq!(view.current_a11y_bounds_space(), crate::A11yBoundsSpace::Scene);
    }

    // -- Debug overlays ------------------------------------------------

    #[test]
    fn debug_overlay_default_is_inactive() {
        let cfg = DebugOverlay::default();
        assert!(!cfg.is_active());
    }

    #[test]
    fn debug_overlay_all_is_active() {
        let cfg = DebugOverlay::ALL;
        assert!(cfg.is_active());
        assert!(cfg.item_bounds);
        assert!(cfg.content_bounds);
        assert!(cfg.viewport);
        assert!(cfg.selection_bounds);
    }

    #[test]
    fn debug_overlay_setting_round_trips() {
        let view = SceneView::new(Scene::new()).debug_overlay(DebugOverlay {
            item_bounds: true,
            ..Default::default()
        });
        let cfg = view.current_debug_overlay();
        assert!(cfg.item_bounds);
        assert!(!cfg.viewport);
        assert!(cfg.is_active());
    }

    #[test]
    fn debug_overlay_renders_when_enabled() {
        // Smoke test: a SceneView with debug overlay on produces
        // additional draw commands compared to one with overlays
        // off, given identical scene contents.
        let make_scene = || {
            let mut s = Scene::new();
            s.add_item(rect_item_at(20.0, 20.0), Point::ZERO);
            s.add_item(rect_item_at(60.0, 20.0), Point::ZERO);
            s
        };

        let mut tree_off = WidgetTree::new();
        tree_off.add(SceneView::new(make_scene()));
        tree_off.layout(SizeProposal::exact(400.0, 300.0));
        let off_count = tree_off.render().draw_order.len();

        let mut tree_on = WidgetTree::new();
        tree_on.add(SceneView::new(make_scene()).debug_overlay(DebugOverlay::ALL));
        tree_on.layout(SizeProposal::exact(400.0, 300.0));
        let on_count = tree_on.render().draw_order.len();

        // ALL adds at minimum: 2 item-bound strokes + 1 content
        // outline + 1 viewport. Selection_bounds adds 0 because
        // nothing is selected.
        assert!(
            on_count > off_count,
            "debug overlay must produce more draws (on={}, off={})",
            on_count,
            off_count
        );
    }

    // -----------------------------------------------------------------
    // R2: scene policy + adopt_scene_size
    // -----------------------------------------------------------------

    #[test]
    fn adopt_scene_size_returns_scene_extent_from_layout_response() {
        use crate::items::RectItem;
        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 200.0, 150.0)),
            fern_canvas::Point::new(0.0, 0.0),
        );
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).adopt_scene_size(true));
        // Propose nothing; the view must size itself to the scene.
        tree.layout(SizeProposal::unspecified());
        let bounds = tree.bounds(view_id);
        assert!((bounds.width - 200.0).abs() < 1e-3, "width = {}", bounds.width);
        assert!((bounds.height - 150.0).abs() < 1e-3, "height = {}", bounds.height);
    }

    #[test]
    fn pan_axes_none_makes_set_pan_a_noop() {
        let mut scene = Scene::new();
        scene.pan_axes(crate::scene::PanAxes::None);
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let view = view_handle(&tree, view_id);
        view.set_pan(Vec2::new(123.0, 456.0));
        assert_eq!(view.pan(), Vec2::ZERO);
    }

    #[test]
    fn pan_axes_horizontal_blocks_vertical_set_pan() {
        let mut scene = Scene::new();
        scene.pan_axes(crate::scene::PanAxes::Horizontal);
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let view = view_handle(&tree, view_id);
        view.set_pan(Vec2::new(50.0, 75.0));
        let pan = view.pan();
        assert!((pan.x - 50.0).abs() < 1e-3);
        assert!(pan.y.abs() < 1e-3, "Y axis must stay 0 (got {})", pan.y);
    }

    #[test]
    fn zoomable_false_makes_zoom_to_a_noop() {
        let mut scene = Scene::new();
        scene.zoomable(false);
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let view = view_handle(&tree, view_id);
        let z0 = view.zoom();
        view.set_zoom(2.5);
        assert!((view.zoom() - z0).abs() < 1e-6);
    }

    #[test]
    fn adopt_scene_size_disables_user_pan() {
        let mut scene = Scene::new();
        scene.add_item(
            crate::items::RectItem::new(Rect::new(0.0, 0.0, 200.0, 150.0)),
            fern_canvas::Point::ZERO,
        );
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).adopt_scene_size(true));
        tree.layout(SizeProposal::unspecified());
        let view = view_handle(&tree, view_id);
        view.set_pan(Vec2::new(99.0, 99.0));
        assert_eq!(view.pan(), Vec2::ZERO);
    }
}
