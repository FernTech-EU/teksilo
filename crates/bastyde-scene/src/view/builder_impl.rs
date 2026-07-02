// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Builder and configuration methods for [`SceneView`].
//!
//! Covers construction (`new` / `with_model`), delegate wiring, selection,
//! camera seeding (`initial_pan` / `initial_zoom` / `view_state`),
//! zoom/pan-bound overrides, drag mode, background/foreground paint hooks,
//! magnetism, debug overlays, accessibility tuning (`a11y_mode`,
//! `a11y_off_screen_mode`, `a11y_bounds_space`, `nested_a11y`), focus-order
//! callbacks, reactive signal accessors, and the `with_scroll_bars` adaptor.

use super::*;
use bastyde_core::signal::Prop;

impl SceneView {
    /// Wrap a [`Scene`] in a viewport (single-view sugar). The scene is moved
    /// into a fresh [`SceneModel`]; for multi-view, build a `SceneModel`
    /// yourself and use [`with_model`](Self::with_model).
    pub fn new(scene: Scene) -> Self {
        Self::with_model(SceneModel::from_scene(scene))
    }

    /// Attach a viewport to a (possibly shared) [`SceneModel`]. Clone one
    /// model into several `SceneView::with_model(model.clone())` to render the
    /// same scene in multiple panes, each with its own camera and delegate.
    pub fn with_model(model: SceneModel) -> Self {
        let pan_x = Signal::new_animated(0.0);
        let pan_y = Signal::new_animated(0.0);
        let zoom = Signal::new_animated(1.0);
        let rotation = Signal::new_animated(0.0);
        let bounds_origin_signal = Signal::new(Vec2::ZERO);
        // Derived view-transform signal — composed once in `new` so
        // it's stable across rebuilds. The same instance is used by
        // `set_content_transform` in `build` and exposed publicly via
        // [`view_transform_signal`](Self::view_transform_signal).
        let view_transform_signal =
            Self::compose_view_transform(&pan_x, &pan_y, &zoom, &rotation, &bounds_origin_signal);
        Self {
            model,
            delegate: None,
            payload_dirty: Rc::new(RefCell::new(HashSet::new())),
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
            reconcile_dirty: Signal::new(0),
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
            _a11y_observer: RefCell::new(None),
            last_at_version: None,
            dynamic_churning: false,
            magnetism: None,
            port_drag: Rc::new(RefCell::new(None)),
            item_snap: Rc::new(RefCell::new(None)),
            magnet_connect_mode: Rc::new(Cell::new(false)),
            magnet_focus: Rc::new(Cell::new(None)),
            magnet_pending: Rc::new(Cell::new(None)),
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

    /// Install the per-view heavyweight builder for `Delegated` items
    /// (those added via [`SceneModel::add_widget_item`](crate::SceneModel::add_widget_item)).
    /// The closure receives the item's type-erased payload and its [`ItemId`]
    /// and returns the widget to materialise in **this** view's arena.
    /// Prefer the typed [`delegate_typed`](Self::delegate_typed) wrapper.
    pub fn delegate(
        mut self,
        f: impl Fn(&dyn std::any::Any, ItemId) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.delegate = Some(Rc::new(move |payload, id| Some(f(payload, id))));
        self
    }

    /// Typed convenience over [`delegate`](Self::delegate): downcasts the
    /// payload to `P` before calling `f`. A downcast miss debug-asserts and
    /// skips the item (no widget is materialised) in release.
    pub fn delegate_typed<P: 'static>(
        mut self,
        f: impl Fn(&P, ItemId) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.delegate = Some(Rc::new(move |payload, id| {
            match payload.downcast_ref::<P>() {
                Some(typed) => Some(f(typed, id)),
                None => {
                    debug_assert!(
                        false,
                        "SceneView delegate_typed: payload for {id:?} is not a {}",
                        std::any::type_name::<P>()
                    );
                    None
                }
            }
        }));
        self
    }

    /// Replace this view's selection with a (typically shared) one. Pass the
    /// same [`SceneSelection`](crate::SceneSelection) clone to several views so
    /// they select together; capture its `selection_signal()` in your delegate
    /// to highlight selected items reactively (no rebuild). Distinct from the
    /// [`selection()`](Self::selection) getter; supersedes any
    /// [`selection_mode`](Self::selection_mode) set earlier.
    pub fn selection_model(mut self, selection: crate::selection::SceneSelection) -> Self {
        self.selection = selection;
        self
    }

    /// A clone of this view's [`SceneModel`] handle — for handler closures
    /// that mutate the scene (every mutator is `&self`) or wire additional views.
    pub fn model(&self) -> SceneModel {
        self.model.clone()
    }

    /// Borrow this view's [`SceneModel`] handle.
    pub fn model_ref(&self) -> &SceneModel {
        &self.model
    }

    /// Drain any pending marquee commit synchronously. Normal
    /// per-frame use never needs this — `place_children` consumes
    /// the pending commit at the start of every layout pass. Tests
    /// that drive on_drag without a follow-up layout call this to
    /// materialise the box-select result.
    pub fn flush_marquee_commit(&self) -> bool {
        if let Some((rect, additive)) = self.pending_marquee_commit.take() {
            self.selection
                .commit_marquee(&self.model.0.borrow(), rect, additive);
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
            if let Some(local_pos) = self.model.local_pos(target_id) {
                let new_local_pos = Point::new(local_pos.x + delta.x, local_pos.y + delta.y);
                self.model.set_local_pos(target_id, new_local_pos);
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
    /// SceneView reports `Role::Region` instead of the default
    /// `Role::Pane`, so screen readers don't announce a
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
    pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
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
    /// ```
    /// # use bastyde_scene::{Scene, SceneView, DebugOverlay};
    /// # let scene = Scene::new();
    /// let _view = SceneView::new(scene)
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
            return cb(&self.model.0.borrow(), direction, current);
        }
        let ids = self.scene().ids();
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
    /// a marquee. `DragMode::ScrollHandDrag` makes left-drag
    /// pan the view unconditionally; `DragMode::NoDrag` disables
    /// the on-drag handler entirely.
    ///
    /// Accepts a static `DragMode` — which sets the current value on the
    /// view's internal signal — or a `Signal<DragMode>` (via
    /// `impl Into<Prop<DragMode>>`), which **replaces** the internal signal
    /// with the app-owned one so a toolbar can hold the same handle and
    /// toggle Hand vs Select vs NoDrag at runtime. To stop sharing, pass a
    /// fresh `Signal::new(mode)`.
    pub fn drag_mode(mut self, mode: impl Into<Prop<crate::item_handlers::DragMode>>) -> Self {
        match mode.into() {
            Prop::Static(m) => self.drag_mode.set(m),
            Prop::Bound(sig) => self.drag_mode = sig,
        }
        self
    }

    /// Compose the derived view-transform signal from the four view-state
    /// signals plus the bounds origin. Coalesced so a simultaneous pan/zoom/
    /// rotation tick registers a single binding per observing widget (instead
    /// of five). Called in `new` and re-called by
    /// [`view_state`](Self::view_state) after the signals are swapped.
    fn compose_view_transform(
        pan_x: &Signal<f32>,
        pan_y: &Signal<f32>,
        zoom: &Signal<f32>,
        rotation: &Signal<f32>,
        bounds_origin: &Signal<Vec2>,
    ) -> Signal<Transform2D> {
        pan_x
            .zip3(pan_y, zoom)
            .zip(rotation)
            .zip(bounds_origin)
            .map_coalesced(|(((px, py, z), r), bo)| {
                compose_view(Vec2::new(*px + bo.x, *py + bo.y), *z, *r)
            })
    }

    /// Replace the view's pan / zoom / rotation signals with app-owned ones.
    ///
    /// The four view-state signals become the app's to hold, share, and
    /// persist — so view state survives a *rebuild-from-state* (a wrapper that
    /// reconstructs the `Scene` + `SceneView` keeps the same signals and the
    /// viewport doesn't jump back to the origin), a "Reset View" button can
    /// snap them, and two views could share one camera. The derived
    /// [`view_transform_signal`](Self::view_transform_signal) is recomposed
    /// from the injected signals.
    ///
    /// Must be called before the view is added to the tree (like the other
    /// builder methods) — `build()` reads `view_transform_signal` once.
    pub fn view_state(
        mut self,
        pan_x: Signal<f32>,
        pan_y: Signal<f32>,
        zoom: Signal<f32>,
        rotation: Signal<f32>,
    ) -> Self {
        // Recompose first (borrows the new signals), then move them into self.
        self.view_transform_signal = Self::compose_view_transform(
            &pan_x,
            &pan_y,
            &zoom,
            &rotation,
            &self.bounds_origin_signal,
        );
        self.pan_x = pan_x;
        self.pan_y = pan_y;
        self.zoom = zoom;
        self.rotation = rotation;
        self
    }

    /// Seed the initial pan offset (logical pixels). The view keeps ownership
    /// of the signals; for app-owned signals use [`view_state`](Self::view_state).
    pub fn initial_pan(self, x: f32, y: f32) -> Self {
        self.pan_x.set(x);
        self.pan_y.set(y);
        self
    }

    /// Seed the initial zoom factor (clamped to the active zoom range).
    pub fn initial_zoom(self, zoom: f32) -> Self {
        let gated = self.gate_zoom_target(zoom);
        self.zoom.set(gated);
        self
    }

    /// Seed the initial rotation (radians).
    pub fn initial_rotation(self, radians: f32) -> Self {
        self.rotation.set(radians);
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

    /// Enable magnetism on this view with the given
    /// [`MagnetismConfig`].
    ///
    /// Once installed, this view's lightweight item drags snap their
    /// magnets onto compatible magnets on other items, magnet handles
    /// become grabbable for port-drag wires, the keyboard connect flow
    /// (the config's connect key) is available while the view is
    /// focused, magnet markers paint, and each enabled magnet is
    /// emitted as a synthetic AT node. A view with no magnetism config
    /// ignores magnets entirely.
    pub fn magnetism(mut self, config: crate::magnet::MagnetismConfig) -> Self {
        self.magnetism = Some(Rc::new(config));
        self
    }

    /// The reactive enabled signal of the installed magnetism config, if
    /// any — for a toolbar to read or bind a magnetism on/off toggle.
    pub fn magnetism_enabled_signal(&self) -> Option<Signal<bool>> {
        self.magnetism.as_ref().map(|c| c.enabled_signal())
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

    /// Wrap this view in a [`SceneScrollView`](crate::SceneScrollView), adding
    /// draggable scroll bars with the widget-tier `ScrollArea`'s options (mode,
    /// per-axis policy, thickness). The bars track the camera and drive panning;
    /// native wheel / drag panning — and its smoothing — keeps working.
    ///
    /// Configure the result with the `SceneScrollView` builder methods:
    ///
    /// ```no_run
    /// # use bastyde_scene::{Scene, SceneView, ScrollBarMode, ScrollBarPolicy};
    /// let scrollable = SceneView::new(Scene::new())
    ///     .with_scroll_bars()
    ///     .scroll_bar_mode(ScrollBarMode::Overlay)
    ///     .vertical_policy(ScrollBarPolicy::AsNeeded);
    /// # let _ = scrollable;
    /// ```
    pub fn with_scroll_bars(self) -> crate::SceneScrollView {
        crate::SceneScrollView::new(self)
    }
}
