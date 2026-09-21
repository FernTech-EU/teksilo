// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Builder and configuration methods for [`SceneView`].
//!
//! Covers construction (`new` / `with_model`), delegate wiring, selection,
//! camera seeding (`initial_pan` / `initial_zoom` / `view_state`),
//! zoom/pan-bound overrides, drag mode, background/foreground paint hooks,
//! magnetism, debug overlays, accessibility tuning (`a11y_mode`,
//! `a11y_off_screen_mode`, `nested_a11y`), focus-order
//! callbacks, reactive signal accessors, and the `with_scroll_bars` adaptor.

use super::*;
use teksilo_core::signal::Prop;

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
            at_heavy: Rc::new(RefCell::new(None)),
            at_children: Rc::new(RefCell::new(None)),
            retention_margin: DEFAULT_RETENTION_MARGIN,
            default_size: Size::new(800.0, 600.0),
            adopt_scene_size: false,
            drag_mode: Signal::new(crate::item_handlers::DragMode::RubberBand),
            handler_snapshot: Rc::new(RefCell::new(Vec::new())),
            over_claimants: Rc::new(Cell::new(false)),
            veto_memo: Rc::new(Cell::new(None)),
            veto_scans: Rc::new(Cell::new(0)),
            key_probes: Rc::new(Cell::new(0)),
            snapshot_generation: Rc::new(Cell::new(0)),
            hit_sync: Rc::new(RefCell::new(super::hit_snapshot::HitSnapshotSync::default())),
            press_floor: Rc::new(Cell::new(crate::PaintKey::bottom())),
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
            pending_marquee_commit: Rc::new(RefCell::new(None)),
            marquee_mode: crate::shape::ItemSelectionMode::default(),
            drag_target: Rc::new(Cell::new(None)),
            pending_item_move: Rc::new(Cell::new(None)),
            lightweight_bounds_snapshot: Rc::new(RefCell::new(Vec::new())),
            reconcile_dirty: Signal::new(0),
            appearance_dirty: Signal::new(0),
            measure_dirty: Signal::new(0),
            measure_state: Rc::new(RefCell::new(HashMap::new())),
            cursor_pos: Rc::new(Cell::new(None)),
            focus_order_callback: None,
            a11y_nested: false,
            a11y_label: None,
            debug_overlay: DebugOverlay::default(),
            background_paint: None,
            foreground_paint: None,
            item_cache: Rc::new(RefCell::new(crate::cache::ItemCoordinateCache::new())),
            published_visible_region: Rc::new(Cell::new(Rect::ZERO)),
            child_keys: Rc::new(RefCell::new(HashMap::new())),
            interleaved_nodes: HashMap::new(),
            wet_layer: None,
            wet_node: None,
            wet_mount: None,
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
            transform: None,
            transform_rt: Rc::new(crate::transform_session::TransformRuntime::default()),
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

    /// Which rule the rubber band picks items by.
    ///
    /// Default [`ItemSelectionMode::IntersectsItemShape`] —
    /// Qt's default, and the one that makes a rubber band agree with a click
    /// about what an item is: a band that merely grazes a connector's bounding
    /// box no longer selects the connector, it has to actually cross the
    /// stroke. Pass
    /// [`IntersectsItemBoundingRect`](crate::ItemSelectionMode::IntersectsItemBoundingRect)
    /// for the looser box test, which is what the marquee did before
    /// `ItemShape` existed.
    pub fn marquee_selection_mode(mut self, mode: crate::shape::ItemSelectionMode) -> Self {
        self.marquee_mode = mode;
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
        let pending = self.pending_marquee_commit.borrow_mut().take();
        if let Some((region, mode, additive)) = pending {
            self.selection.commit_marquee_region(
                &self.model.0.borrow(),
                &region,
                mode,
                self.view_scale(),
                additive,
            );
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
            // The whole selection when the grab was on a selected item — see
            // `SceneView::drag_group`, which the paint feedback reads too.
            for id in self.drag_group(target_id) {
                if let Some(local_pos) = self.model.local_pos(id) {
                    let new_local_pos = Point::new(local_pos.x + delta.x, local_pos.y + delta.y);
                    self.model.set_local_pos(id, new_local_pos);
                }
            }
            self.drag_target.set(None);
            true
        } else {
            false
        }
    }

    /// Drain a committed transform, applying its delta through one
    /// [`Scene::apply_transform_delta`](crate::Scene::apply_transform_delta).
    ///
    /// The same drain `build()` runs; the public form exists for the headless
    /// driver, exactly like [`flush_pending_item_move`](Self::flush_pending_item_move)
    /// and [`flush_marquee_commit`](Self::flush_marquee_commit). Returns whether
    /// anything was pending.
    pub fn flush_pending_transform(&mut self) -> bool {
        let pending = self.transform_rt.pending_commit.borrow_mut().take();
        match pending {
            Some((roots, delta)) => {
                self.model.apply_transform_delta(&roots, &delta);
                true
            }
            None => false,
        }
    }

    /// Disable user-driven navigation: pinch and keyboard handlers are not
    /// registered, the scroll handler's wheel arm returns early, and the
    /// SceneView is not made focusable. The `on_scroll` slot itself stays
    /// registered, because it also carries a descendant's `ScrollIntoView`
    /// reveal — a focused caret inside an embedded card is entitled to be on
    /// screen whether or not the *user* may drive the camera. Programmatic
    /// [`pan_to`](Self::pan_to) /
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

    /// Configure visual debug overlays. Default: all flags off.
    /// Pass [`DebugOverlay::ALL`] to enable every overlay or
    /// construct a custom config:
    ///
    /// ```
    /// # use teksilo_scene::{Scene, SceneView, DebugOverlay};
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

    /// How far past the viewport edge, in **screen** pixels, a heavyweight
    /// card stays live. Default [`DEFAULT_RETENTION_MARGIN`] (96).
    ///
    /// A card outside the *retention region* — the viewport grown by this
    /// margin, unioned with whatever
    /// [`a11y_off_screen_mode`](Self::a11y_off_screen_mode) asks to be
    /// listed — is parked dormant. It leaves paint, the layout recursion, the
    /// AccessKit tree and the Tab ring, and keeps its focus, text and
    /// animation state for when the camera brings it back.
    ///
    /// **It is a lifecycle knob, not an accessibility one.** Raising it does
    /// not offer assistive technology more of the scene: how much of it a
    /// screen reader is enumerated is decided by
    /// [`a11y_off_screen_mode`](Self::a11y_off_screen_mode) alone. A card in
    /// the margin band but outside that mode's region is alive and unlisted —
    /// laid out, Tab-reachable, holding its state, absent from the AccessKit
    /// tree. Landing focus on it publishes it, in the same pass.
    ///
    /// **Screen pixels, not scene units**, converted through the current zoom
    /// each pass, so the margin is a constant on-screen distance at every zoom
    /// level. That is what it has to be: a pan covers screen distance, not
    /// scene distance, and the margin's job is to have woken a card before the
    /// camera reaches it. At zoom 0.1 the default is 960 scene units.
    ///
    /// Three regions, not one, and they are layered: the *tight* viewport
    /// decides who is laid out at full size versus collapsed to zero
    /// (unchanged, zero margin), the accessibility region decides who is
    /// enumerated, and this widest one decides who is dormant. Each contains
    /// the one before it by construction — so a card being laid out is never
    /// also parked, and the AT walk is never asked to describe a card the
    /// arena has parked.
    ///
    /// Raising this keeps more cards live — more Tab stops, more layout
    /// recursion, more state held; lowering it parks closer to the edge. Zero
    /// is legal and means "park at the viewport edge"; a card entering is
    /// still woken and laid out in the same pass, so nothing is ever a frame
    /// late, but every crossing then pays a wake.
    pub fn retention_margin(mut self, screen_px: f32) -> Self {
        self.retention_margin = screen_px.max(0.0);
        self
    }

    /// The current retention margin in screen pixels.
    pub fn current_retention_margin(&self) -> f32 {
        self.retention_margin
    }

    /// Override the off-screen visibility policy for the AT walker — how much
    /// of a scene that runs past the viewport a screen reader is offered.
    ///
    /// Default: `ViewportPlusN { n: 1 }` — everything inside the viewport plus
    /// a one-screen margin on each side. `AllItems` for small scenes where AT
    /// users want a complete table of contents (nothing is ever parked in that
    /// mode, so it costs what it promises); `ViewportOnly` for very large
    /// scenes, where listing off-screen content would bury an AT client.
    ///
    /// This governs **both** tiers — lightweight items and heavyweight cards —
    /// and it is the only thing that does.
    /// [`retention_margin`](Self::retention_margin) can keep a card alive past
    /// this region, for the camera's sake, without adding it here.
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
        F: Fn(&mut teksilo_canvas::Canvas, &PaintContext, Rect) + 'static,
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
        F: Fn(&mut teksilo_canvas::Canvas, &PaintContext, Rect) + 'static,
    {
        self.foreground_paint = Some(Rc::new(paint));
        self
    }

    /// Install a [`WetLayer`](crate::WetLayer) — a surface for content being
    /// authored right now, which repaints without taking the scene's item
    /// bands with it.
    ///
    /// Its node is always the **last** child of this view, so wet content sits
    /// above every card and every
    /// [`Interleaved`](crate::SceneLayer::Interleaved) item, and under the
    /// `Over` band and the view's own chrome (marquee, magnet feedback,
    /// transform frame, debug overlay).
    ///
    /// Unlike [`foreground`](Self::foreground), which runs inside the view's
    /// own `post_paint` and therefore cannot be invalidated without repainting
    /// the whole `Under` band with it, this is a node of its own:
    /// [`WetLayer::request_repaint`](crate::WetLayer::request_repaint) marks it
    /// and nothing else.
    ///
    /// Paint-only, hidden from assistive technology and transparent to the
    /// pointer — see [`WetLayer`](crate::WetLayer).
    ///
    /// The same layer may be installed in **several** views: each mounts its
    /// own node and paints the same painter, the way several views share one
    /// [`SceneModel`]. Mounting it in a second view does not
    /// unmount it from the first.
    pub fn wet_layer(mut self, layer: crate::WetLayer) -> Self {
        self.wet_layer = Some(layer);
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

    /// Install the **selection transform controller** on this view: a frame
    /// with resize and rotate handles drawn around the selection, a group move,
    /// a keyboard route, and accessibility nodes for all of it.
    ///
    /// Nothing about the view changes until this is called — with no controller
    /// the pointer rules are exactly what they were.
    ///
    /// **Where the gesture may land** is not this controller's business: it is
    /// the scene's, through
    /// [`SceneModel::set_geometry_constraint`](crate::SceneModel::set_geometry_constraint),
    /// which every gesture here consults on every sample. Snap-to-grid, axis
    /// lock and page clamping live there, once for the document, rather than
    /// once per view.
    ///
    /// # What a gesture writes, and when
    ///
    /// Nothing, until the release. The session is per-view and model-free: it
    /// reads [`SceneSelection`](crate::SceneSelection), paints a preview, and
    /// then writes the scene **once**, through a single
    /// [`Scene::apply_transform_delta`](crate::Scene::apply_transform_delta).
    /// So Esc is "drop the session" with no rollback to get wrong, a ten-item
    /// selection emits no `ItemChange` at all until it lands, and an app that
    /// wants the edit to be reversible has exactly one call to record — from
    /// [`TransformConfig::on_end`](crate::TransformConfig::on_end). The history itself is the data layer's, not
    /// this crate's.
    ///
    /// # Which handles appear
    ///
    /// Exactly the operations that will be honoured, never more:
    ///
    /// | Operation | Flag | Offered when |
    /// | --- | --- | --- |
    /// | Move | `IS_DRAGGABLE` | **any** selection root carries it (the others stay put) |
    /// | Resize | `IS_RESIZABLE` | **every** selection root carries it |
    /// | Rotate | `IS_ROTATABLE` | **every** selection root carries it *and* is lightweight |
    ///
    /// Resize and rotate demand unanimity because a partial one would tear the
    /// selection apart — the locked members would stand still while the rest
    /// scaled, and the frame would stop describing what it is drawn around. A
    /// move can be partial because that is what every editor does with a locked
    /// object.
    ///
    /// The flags are **honoured, not bypassed**: a default `ItemFlags` carries
    /// `IS_SELECTABLE` and not `IS_DRAGGABLE`, so selecting a decorative item
    /// does not quietly make it draggable.
    ///
    /// # Reaching the chrome over a heavyweight card
    ///
    /// The frame is drawn [`padding`](crate::TransformConfig::padding) **outside** the
    /// selection, so the band and every handle sit on pixels no card owns. That
    /// matters because a card that claims its own press — one calling
    /// `capture_pointer`, or carrying its own `on_drag`, which is every
    /// `Splitter` handle, `SpinBox` step button and `TextInput` selection drag —
    /// wins that press by design, and this view never sees the gesture. So
    /// **body drag over a card is best-effort and the frame band is the route
    /// that always works.** Over the lightweight tier, and over a card that
    /// claims nothing, both work.
    ///
    /// # Keyboard and assistive technology
    ///
    /// `t` (see [`TransformConfig::transform_key`](crate::TransformConfig::transform_key)) enters a handle-roving
    /// mode: `Tab` / `Shift+Tab` move between the handles the frame is
    /// offering, arrows drive the roved one (`Shift` ×10), `Enter` commits,
    /// `Esc` cancels, `t` leaves. The frame and each handle are published as
    /// accessibility nodes with `Increment` / `Decrement`, and both routes go
    /// through the same session and the same commit — so the non-drag
    /// alternative makes the same model change the drag makes, structurally.
    ///
    /// ```
    /// # use teksilo_scene::{Scene, SceneView, SceneSelectionMode, TransformConfig};
    /// let view = SceneView::new(Scene::new())
    ///     .selection_mode(SceneSelectionMode::Multi)
    ///     .transform_controller(TransformConfig::new().keep_ratio(true));
    /// ```
    pub fn transform_controller(
        mut self,
        config: crate::transform_session::TransformConfig,
    ) -> Self {
        self.transform = Some(Rc::new(config));
        self
    }

    /// The live transform session, for an app-owned inspector, status bar or
    /// size readout. `None` between gestures. Bind at
    /// [`BindingLevel::RepaintOnly`].
    ///
    /// Republished on every **input** sample — each pointer move, each keyboard
    /// step, and at the start and end of a gesture. It is deliberately *not*
    /// republished per frame: during an edge auto-pan the pointer is standing
    /// still while the view slides under it, so the frame keeps changing with
    /// no sample to hang a republish on, and a readout bound here shows the
    /// last sampled numbers until the pointer moves again. What is committed is
    /// always the live resolution, so this lags the frame rather than
    /// disagreeing with the result.
    pub fn transform_session_signal(
        &self,
    ) -> Signal<Option<crate::transform_session::TransformSession>> {
        self.transform_rt.published.clone()
    }

    /// The reactive enabled signal of the installed transform controller, if
    /// any — for a toolbar to read or bind a "transform tools" toggle.
    pub fn transform_enabled_signal(&self) -> Option<Signal<bool>> {
        self.transform.as_ref().map(|c| c.enabled_signal())
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
    /// on `LocalBoundsChanged` / `ItemReplaced` / `AppearanceChanged` /
    /// `MeasuredSizeChanged` / an `IS_ENABLED` `FlagsChanged` / `Removed` —
    /// but never on opacity / transform / z / `local_pos`, which are applied
    /// as wrapping scopes at replay and don't bake into a cached frame.
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
    /// # use teksilo_scene::{Scene, SceneView, ScrollBarMode, ScrollBarPolicy};
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
