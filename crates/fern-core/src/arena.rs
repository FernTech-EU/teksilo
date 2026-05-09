use slotmap::SlotMap;

use crate::environment::ThemeOverride;
use crate::event_handlers::EventHandlers;
use crate::event_source::{SubscriptionHandle, SubscriptionId};
use crate::signal::{ObserverHandle, Prop, Signal};
use crate::widget::{CursorIcon, Widget};
use crate::widget_id::WidgetId;
use fern_canvas::RenderFrame;

/// Minimal placeholder widget used during composite rebuild and ID reservation.
#[derive(Debug)]
pub(crate) struct PlaceholderWidget;

impl Widget for PlaceholderWidget {
    fn layout_response(
        &self,
        _proposal: fern_canvas::SizeProposal,
        _ctx: &crate::widget::LayoutContext,
    ) -> crate::widget::LayoutResponse {
        fern_canvas::Size::ZERO.into()
    }
}

/// Activation state for a widget in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationState {
    Active,
    Dormant,
    Destroyed,
}

/// Where a `HandlerSet` should land on the node: handlers the widget
/// attaches to itself (cleared on rebuild) vs handlers attached from
/// outside (persist across rebuilds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandlerScope {
    /// Handlers registered during the widget's own `build()` via
    /// `BuildContext::apply_self_handlers`.
    Own,
    /// Handlers attached externally — at insertion time via
    /// `WidgetBuilder::on_tap` et al., or by a composing parent's
    /// `BuildContext::apply_handlers(child_id, ...)`.
    External,
}

/// Dirty flags for a widget.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirtyFlags {
    pub needs_layout: bool,
    pub needs_paint: bool,
    /// When true, the widget's `build()` should be re-run to regenerate children.
    /// Set by `BindingLevel::Rebuild` bindings (data-driven widgets).
    pub needs_rebuild: bool,
}

/// A node in the widget arena storing a widget and its metadata.
pub struct WidgetNode {
    pub widget: Box<dyn Widget>,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
    pub activation: ActivationState,
    pub dirty: DirtyFlags,
    pub bounds: fern_canvas::Rect,
    pub(crate) theme_override: Option<ThemeOverride>,
    pub(crate) visible_state: Option<Prop<bool>>,
    pub(crate) enabled_state: Option<Prop<bool>>,
    /// User-bound signal that the framework sets to `true` whenever
    /// the focused widget is a strict descendant of this node, and
    /// `false` otherwise. Used by `Panel` / `Card` / composite
    /// widgets that want a unified focus halo without per-child
    /// `on_focus` plumbing. See `WidgetBuilder::focus_within`.
    pub(crate) focus_within_signal: Option<Signal<bool>>,
    /// User-bound signal that the framework sets to `true` whenever
    /// the hovered widget is a strict descendant of this node.
    /// Symmetric to `focus_within_signal`. See
    /// `WidgetBuilder::hover_within`.
    pub(crate) hover_within_signal: Option<Signal<bool>>,
    pub(crate) alignment_override: Option<fern_tokens::Alignment>,
    /// When true, the paint pass clips child rendering to this widget's bounds.
    /// Set by scroll areas and overflow-hidden containers.
    pub clips_children: bool,
    /// When true, hit-testing skips this node — pointer events fall
    /// through to whatever sits behind it. Descendants are still
    /// hit-tested normally (the recursion walks into children before
    /// the pass-through check), so an interactive subtree under a
    /// pass-through wrapper stays usable. Used by the debug inspector's
    /// `HighlightLayer` and `HoverProbe` to paint over the user's
    /// content without absorbing clicks. Default `false`.
    pub event_pass_through: bool,
    /// Optional opacity multiplier (0..1) applied to this widget's
    /// entire subtree during paint. The render walker emits
    /// `SetOpacity(value)` before walking the widget's own paint and
    /// children, then `RestoreOpacity` afterwards — so the multiplier
    /// composes with ancestor opacity scopes via the canvas's
    /// already-stacked opacity model. Bound at `Repaint` level: opacity
    /// changes never trigger relayout. `None` means "no opacity scope"
    /// (the default for almost every widget). The `Fade` widget sets
    /// this on its own node to drive an animated visibility tween.
    pub(crate) opacity_prop: Option<Prop<f32>>,
    /// Optional 2D affine transform applied to this widget's entire
    /// subtree during paint. The render walker emits
    /// `PushTransform(value)` before walking the widget's own paint
    /// and children, then `PopTransform` afterwards — the renderer
    /// composes it onto its transform stack so nested wrappers and
    /// widget-internal canvas transforms compose correctly. Bound at
    /// `Repaint` level by default (visual-only); a wrapper that wants
    /// the transform to drive layout (e.g. `Scale::reflow(true)`)
    /// must additionally bind its driver signal at `Relayout`.
    /// `None` means "no transform scope" (the default for almost every
    /// widget). The `Scale` and `Rotate` widgets set this on their own
    /// node.
    pub(crate) transform_prop: Option<Prop<fern_canvas::Transform2D>>,
    /// Optional Gaussian-equivalent blur radius applied to this widget's
    /// entire subtree during paint. The render walker emits
    /// `BeginBlurredSubtree { bounds, radius }` before walking the
    /// widget's own paint and children, then `EndBlurredSubtree`
    /// afterwards — the renderer redirects drawing into an intermediate
    /// texture, runs a dual-Kawase blur chain at the requested radius,
    /// and composites the blurred result back into the parent pass.
    /// Bound at `Repaint` level: blur radius changes never trigger
    /// relayout. `None` (or `Some(radius < 0.5)`) means "no blur scope"
    /// — the walker skips the Begin/End pair entirely so disabled blur
    /// has zero per-frame cost. The `Blur` widget sets this on its own
    /// node.
    pub(crate) blur_prop: Option<Prop<f32>>,
    /// Cached paint output for this widget (excludes children).
    /// Reused when `needs_paint` is false to avoid re-running `paint()`.
    pub(crate) cached_paint: Option<RenderFrame>,
    /// The `WidgetTree::paint_epoch` at which this widget's bounds were
    /// last observed inside the window viewport by the paint pass.
    /// The animation scheduler uses this to pause looping animations
    /// for offscreen widgets: an animation whose
    /// `last_painted_epoch + 1 < tree.paint_epoch` is considered
    /// off-screen and skipped. `0` means "not yet painted" — treated
    /// as "always visible" to keep headless tests (no `render()` call)
    /// from regressing.
    pub last_painted_epoch: u64,

    // --- V2 fields ---
    /// Event handlers the widget attached to itself during its own
    /// `build()` via `BuildContext::apply_self_handlers`. Cleared on
    /// rebuild so accumulating `apply_self_handlers` calls across
    /// rebuilds don't stack N-fold handler chains.
    pub(crate) handlers: EventHandlers,
    /// Event handlers attached *externally* — either via the
    /// `WidgetBuilder` chain at the widget's creation site
    /// (`SomeWidget::new().on_tap(...)`) or by a parent's
    /// `BuildContext::apply_handlers(child_id, ...)`. These survive
    /// rebuilds: the widget didn't register them and shouldn't decide
    /// when they go away.
    pub(crate) external_handlers: EventHandlers,
    /// Focusable override set via HandlerSet. Takes precedence over widget.is_focusable().
    pub(crate) node_focusable: Option<bool>,
    /// Tab index override set via HandlerSet.
    pub(crate) node_tab_index: Option<i32>,
    /// Cursor override set via HandlerSet.
    pub(crate) node_cursor: Option<CursorIcon>,
    /// Whether build() returned children (for rebuild on environment change).
    pub(crate) has_built_children: bool,
    /// RAII observer handles for effects registered during build().
    /// Dropped on rebuild or widget destruction.
    pub(crate) effect_handles: Vec<ObserverHandle>,
    /// Backend-event subscriptions registered during build() via
    /// `BuildContext::subscribe_event`. Each entry pairs a subscription id
    /// (used to remove the UI-side callback from `TreeAppContext`) with the
    /// opaque source-side handle whose `Drop` removes the subscriber from
    /// the source's internal registry. See architecture §9.4.5.
    pub(crate) subscription_handles: Vec<(SubscriptionId, SubscriptionHandle)>,
    /// Context menu factory — invoked on right-click to produce overlay content.
    pub(crate) context_menu_factory: Option<crate::widget_builder::ContextMenuFactory>,
    /// Intent-bound actions attached by this widget during `build()`.
    /// Consulted during intent dispatch (source-widget → root walk).
    /// Cleared on rebuild in the same pass that clears handlers.
    pub(crate) actions: Vec<crate::action::Action>,
    /// Builder-level accessibility overrides (`access_label`,
    /// `access_role`, etc.). Mirrored from the wrapper's `HandlerSet`
    /// at insertion via `apply_handler_set`. Applied by the
    /// accessibility tree walker after the inner widget's
    /// `accessibility(&self, builder)` runs. Action callbacks
    /// (`actions`, `custom_actions` inside this struct) are dispatched
    /// by `event_dispatch_impl.rs` when handling
    /// `WidgetEvent::AccessAction`.
    pub(crate) access_overrides: Option<Box<crate::widget_builder::AccessibilityOverrides>>,
    /// Subtree visibility / merge mode (`access_exclude_subtree` /
    /// `access_merge_subtree`). Mirrored from the wrapper's
    /// `HandlerSet`.
    pub(crate) access_subtree: crate::widget_builder::AccessSubtreeMode,
}

impl std::fmt::Debug for WidgetNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetNode")
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("activation", &self.activation)
            .field("dirty", &self.dirty)
            .field("bounds", &self.bounds)
            .field("has_gesture_arena", &self.handlers.gesture_arena.is_some())
            .field("has_theme_override", &self.theme_override.is_some())
            .field("has_visible_state", &self.visible_state.is_some())
            .field("has_enabled_state", &self.enabled_state.is_some())
            .finish()
    }
}

impl WidgetNode {
    /// Does EITHER handler slot (own or external) have a handler of the
    /// requested kind? Use this when deciding whether to build a gesture
    /// arena, mark the node as a drop target, etc.
    pub(crate) fn any_handler<F>(&self, f: F) -> bool
    where
        F: Fn(&EventHandlers) -> bool,
    {
        f(&self.handlers) || f(&self.external_handlers)
    }
}

/// Flat arena storage for all widgets, using SlotMap for O(1) access.
pub struct WidgetArena {
    nodes: SlotMap<WidgetId, WidgetNode>,
    /// Number of nodes with theme overrides. When zero, resolve_theme is O(1).
    pub(crate) theme_override_count: usize,
    /// Cached root widget IDs (widgets with no parent).
    cached_roots: Vec<WidgetId>,
    /// Whether the cached_roots list needs rebuilding.
    roots_dirty: bool,
}

impl WidgetArena {
    pub fn new() -> Self {
        Self {
            nodes: SlotMap::with_key(),
            theme_override_count: 0,
            cached_roots: Vec::new(),
            roots_dirty: true,
        }
    }

    /// Insert a widget into the arena as a root-level widget.
    pub fn insert(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        self.roots_dirty = true;
        let children = widget.children();
        let id = self.nodes.insert(WidgetNode {
            widget,
            parent: None,
            children: Vec::new(),
            activation: ActivationState::Active,
            dirty: DirtyFlags {
                needs_layout: true,
                needs_paint: true,
                needs_rebuild: false,
            },
            bounds: fern_canvas::Rect::ZERO,
            theme_override: None,
            visible_state: None,
            enabled_state: None,
            focus_within_signal: None,
            hover_within_signal: None,
            alignment_override: None,
            clips_children: false,
            event_pass_through: false,
            opacity_prop: None,
            transform_prop: None,
            blur_prop: None,
            cached_paint: None,
            last_painted_epoch: 0,
            handlers: EventHandlers::new(),
            external_handlers: EventHandlers::new(),
            node_focusable: None,
            node_tab_index: None,
            node_cursor: None,
            has_built_children: false,
            effect_handles: Vec::new(),
            subscription_handles: Vec::new(),
            context_menu_factory: None,
            actions: Vec::new(),
            access_overrides: None,
            access_subtree: crate::widget_builder::AccessSubtreeMode::default(),
        });
        // Set up parent-child for declared children
        for &child_id in &children {
            if let Some(child_node) = self.nodes.get_mut(child_id) {
                child_node.parent = Some(id);
            }
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.children = children;
        }
        id
    }

    /// Insert a widget as a child of the given parent.
    pub fn insert_child(&mut self, parent: WidgetId, widget: Box<dyn Widget>) -> WidgetId {
        assert!(
            self.nodes.contains_key(parent),
            "insert_child() called with invalid parent WidgetId {parent:?}"
        );
        self.roots_dirty = true;
        let children = widget.children();
        let id = self.nodes.insert(WidgetNode {
            widget,
            parent: Some(parent),
            children: Vec::new(),
            activation: ActivationState::Active,
            dirty: DirtyFlags {
                needs_layout: true,
                needs_paint: true,
                needs_rebuild: false,
            },
            bounds: fern_canvas::Rect::ZERO,
            theme_override: None,
            visible_state: None,
            enabled_state: None,
            focus_within_signal: None,
            hover_within_signal: None,
            alignment_override: None,
            clips_children: false,
            event_pass_through: false,
            opacity_prop: None,
            transform_prop: None,
            blur_prop: None,
            cached_paint: None,
            last_painted_epoch: 0,
            handlers: EventHandlers::new(),
            external_handlers: EventHandlers::new(),
            node_focusable: None,
            node_tab_index: None,
            node_cursor: None,
            has_built_children: false,
            effect_handles: Vec::new(),
            subscription_handles: Vec::new(),
            context_menu_factory: None,
            actions: Vec::new(),
            access_overrides: None,
            access_subtree: crate::widget_builder::AccessSubtreeMode::default(),
        });
        // Set up parent-child for declared children
        for &child_id in &children {
            if let Some(child_node) = self.nodes.get_mut(child_id) {
                child_node.parent = Some(id);
            }
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.children = children;
        }
        if let Some(parent_node) = self.nodes.get_mut(parent) {
            parent_node.children.push(id);
        }
        id
    }

    pub fn get(&self, id: WidgetId) -> Option<&WidgetNode> {
        self.nodes.get(id)
    }

    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut WidgetNode> {
        self.nodes.get_mut(id)
    }

    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.nodes
            .get(id)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes.get(id).and_then(|n| n.parent)
    }

    pub fn bounds(&self, id: WidgetId) -> fern_canvas::Rect {
        self.nodes
            .get(id)
            .map(|n| n.bounds)
            .unwrap_or(fern_canvas::Rect::ZERO)
    }

    /// The accumulated 2D affine transform that maps `id`'s pre-transform
    /// local-space points to screen space — equivalent to the renderer's
    /// `transform_stack` top by the time it begins painting `id`. Used by
    /// hit-testing and any consumer that needs to project a node's
    /// pre-transform bounds into screen space (e.g. fern-scene's a11y
    /// bounds projection of view-transformed scene items).
    ///
    /// **Composition order.** Mirrors `crates/fern-render/src/renderer.rs`'s
    /// `PushTransform` handling: each push composes as
    /// `new_top = device_t.then(prev_top)`, so the deepest (innermost)
    /// transform is applied **first** to a local point and outer ancestors
    /// compose afterward. Walking root→leaf, each ancestor's
    /// `transform_prop` is folded in via `t.then(effective)` (NOT
    /// `effective.then(t)`).
    ///
    /// Returns `Transform2D::IDENTITY` if no ancestor sets a non-identity
    /// transform, which is the common case (90%+ of widgets).
    pub fn effective_transform(&self, id: WidgetId) -> fern_canvas::Transform2D {
        // Collect leaf→root, then iterate root→leaf. Composition is
        // `t_new.then(effective_so_far)` so the outer ancestor is applied
        // *after* the deeper push — matching the renderer's stack semantic
        // (`device_t.then(prev_top)` at PushTransform).
        let mut chain: Vec<WidgetId> = Vec::new();
        let mut current = Some(id);
        while let Some(c) = current {
            chain.push(c);
            current = self.parent(c);
        }
        let mut effective = fern_canvas::Transform2D::IDENTITY;
        for node_id in chain.iter().rev() {
            if let Some(node) = self.nodes.get(*node_id)
                && let Some(p) = node.transform_prop.as_ref()
            {
                let t = p.get();
                if !t.is_identity() {
                    effective = t.then(&effective);
                }
            }
        }
        effective
    }

    /// Get all root-level widget IDs (widgets with no parent).
    pub fn roots(&self) -> Vec<WidgetId> {
        if self.roots_dirty {
            // Fall back to scanning when cache is stale.
            // refresh_roots() should be called from layout() for the fast path.
            return self
                .nodes
                .iter()
                .filter(|(_, node)| node.parent.is_none())
                .map(|(id, _)| id)
                .collect();
        }
        self.cached_roots.clone()
    }

    /// Refresh the cached roots list. Call once per frame from layout().
    pub fn refresh_roots(&mut self) {
        if self.roots_dirty {
            self.cached_roots = self
                .nodes
                .iter()
                .filter(|(_, node)| node.parent.is_none())
                .map(|(id, _)| id)
                .collect();
            self.roots_dirty = false;
        }
    }

    /// Walk the active widget tree at `point` and return the deepest
    /// widget under it (the front-most hit, last child wins). Honors
    /// `event_pass_through` (such nodes pass through to whatever sits
    /// behind them but their descendants are still hit-testable). Does
    /// not consider overlays — for the full pointer-routing hit-test
    /// see [`WidgetTree::hit_test`].
    ///
    /// `exclude`: if `Some(id)`, that widget (and any descendants
    /// within its subtree) are skipped during the walk. Used by the
    /// debug inspector's picker tool to ignore the picker overlay
    /// itself, and by drag-and-drop to ignore the drag preview.
    pub fn hit_test_at(
        &self,
        point: fern_canvas::Point,
        exclude: Option<WidgetId>,
    ) -> Option<WidgetId> {
        for &root in self.roots().iter().rev() {
            if let Some(hit) = self.hit_test_recursive(root, point, exclude) {
                return Some(hit);
            }
        }
        None
    }

    /// Hit-test starting from a specific subtree root rather than the
    /// arena's top-level roots. Same semantics as
    /// [`hit_test_at`](Self::hit_test_at) but scoped — useful when
    /// callers want to ignore everything outside a known subtree
    /// (e.g. the inspector's picker hit-tests inside the user-root
    /// subtree so it never resolves to its own chrome).
    pub fn hit_test_in_subtree(
        &self,
        start: WidgetId,
        point: fern_canvas::Point,
    ) -> Option<WidgetId> {
        self.hit_test_recursive(start, point, None)
    }

    fn hit_test_recursive(
        &self,
        id: WidgetId,
        point: fern_canvas::Point,
        exclude: Option<WidgetId>,
    ) -> Option<WidgetId> {
        if !self.is_active(id) || Some(id) == exclude {
            return None;
        }
        // If this node carries a `set_transform` scope, the render walker
        // pushes that transform onto its stack around this node's own
        // paint AND its subtree. Hit-testing must mirror that composition:
        // the input point arrives in this node's parent-effective space;
        // apply this node's transform inverse once, then both the node's
        // own bounds test and the recursion into children operate in the
        // new local space. Identity transforms (and missing transform_prop)
        // are skipped so the hot path stays scalar.
        let local_point = match self
            .get(id)
            .and_then(|n| n.transform_prop.as_ref())
            .map(|p| p.get())
            .filter(|t| !t.is_identity())
        {
            Some(t) => match t.inverse() {
                Some(inv) => inv.apply_point(point),
                // A degenerate transform (collapsed axis) hides the
                // entire subtree visually; mirror that for hit-testing.
                None => return None,
            },
            None => point,
        };
        let bounds = self.bounds(id);
        if !bounds.contains(local_point) {
            return None;
        }
        let pass_through = self.get(id).map(|n| n.event_pass_through).unwrap_or(false);
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for &child in children.iter().rev() {
            if let Some(hit) = self.hit_test_recursive(child, local_point, exclude) {
                return Some(hit);
            }
        }
        if pass_through {
            return None;
        }
        Some(id)
    }

    /// Iterate over all active widget IDs.
    ///
    /// Allocating wrapper around [`Self::active_ids_iter`]. Hot-path
    /// callers that hold `&self` for the whole iteration should call
    /// the iterator directly to avoid the per-call `Vec` allocation;
    /// callers that need an owned snapshot (because they mutate
    /// arena state inside the loop) should use
    /// [`Self::fill_active_ids`] with a reusable buffer.
    pub fn active_ids(&self) -> Vec<WidgetId> {
        self.active_ids_iter().collect()
    }

    /// Stream all active widget IDs without allocating. The iterator
    /// borrows the arena, so the caller cannot mutate it while
    /// iterating — for that case use [`Self::fill_active_ids`].
    pub fn active_ids_iter(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.nodes
            .iter()
            .filter(|(_, node)| node.activation == ActivationState::Active)
            .map(|(id, _)| id)
    }

    /// Fill `out` with every active widget ID. Clears `out` first so
    /// callers can reuse a long-lived buffer across calls. Use this
    /// when the iteration site needs an owned snapshot independent
    /// of the arena borrow (typically because it mutates per-widget
    /// state with `arena.get_mut(id)` inside the loop).
    pub fn fill_active_ids(&self, out: &mut Vec<WidgetId>) {
        out.clear();
        out.extend(self.active_ids_iter());
    }

    /// Set a widget subtree to dormant state (state preserved, not rendered).
    /// Recursively dormants all children.
    pub fn set_dormant(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.activation = ActivationState::Dormant;
        }
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.set_dormant(child);
        }
    }

    /// Activate a dormant widget subtree (triggers relayout and repaint).
    /// Recursively activates all children.
    pub fn activate(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.activation = ActivationState::Active;
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.activate(child);
        }
    }

    /// Destroy a widget and remove it from the arena entirely.
    /// Recursively destroys all children. State is gone.
    pub fn destroy(&mut self, id: WidgetId) {
        self.roots_dirty = true;
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.destroy(child);
        }
        // Remove from parent's children list
        if let Some(parent_id) = self.parent(id)
            && let Some(parent) = self.nodes.get_mut(parent_id)
        {
            parent.children.retain(|&c| c != id);
        }
        self.nodes.remove(id);
    }

    pub fn is_active(&self, id: WidgetId) -> bool {
        self.nodes
            .get(id)
            .map(|n| n.activation == ActivationState::Active)
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn mark_all_clean(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.dirty = DirtyFlags::default();
        }
    }

    pub fn any_needs_layout(&self) -> bool {
        self.nodes
            .values()
            .any(|n| n.activation == ActivationState::Active && n.dirty.needs_layout)
    }

    pub fn any_needs_paint(&self) -> bool {
        self.nodes
            .values()
            .any(|n| n.activation == ActivationState::Active && n.dirty.needs_paint)
    }

    pub fn mark_needs_paint(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_paint = true;
        }
    }

    /// Recursively mark a widget and all its descendants needs_paint.
    /// Used by callers that want a fresh paint of an entire subtree
    /// — e.g. a rich tooltip whose dwell indicator child would
    /// otherwise reuse its cached_paint while the parent re-runs
    /// some per-frame logic.
    pub fn mark_subtree_needs_paint(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_paint = true;
        }
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.mark_subtree_needs_paint(child);
        }
    }

    pub fn mark_needs_layout(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
    }

    /// Mark a widget as needing its `build()` re-run.
    /// Also marks for layout and paint since rebuilt children need both.
    pub fn mark_needs_rebuild(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_rebuild = true;
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
    }

    /// Collect widgets that need their `build()` re-run (data-driven rebuild).
    /// Only returns active widgets with `has_built_children == true` and
    /// `needs_rebuild == true`.
    ///
    /// Allocating wrapper around [`Self::needs_rebuild_iter`]. Prefer
    /// the iterator on hot paths.
    pub fn collect_needs_rebuild(&self) -> Vec<WidgetId> {
        self.needs_rebuild_iter().collect()
    }

    /// Stream widgets that need `build()` re-run without allocating.
    pub fn needs_rebuild_iter(&self) -> impl Iterator<Item = WidgetId> + '_ {
        self.nodes
            .iter()
            .filter(|(_, n)| {
                n.activation == ActivationState::Active
                    && n.has_built_children
                    && n.dirty.needs_rebuild
            })
            .map(|(id, _)| id)
    }

    /// Check all widgets with visible_state bindings and return
    /// (id, is_currently_active, should_be_visible) tuples.
    ///
    /// Allocating wrapper around [`Self::visibility_checks_iter`].
    pub fn visibility_checks(&self) -> Vec<(WidgetId, bool, bool)> {
        self.visibility_checks_iter().collect()
    }

    /// Stream widgets with `visible_state` bindings without
    /// allocating. Each entry is `(id, is_currently_active,
    /// should_be_visible)`.
    pub fn visibility_checks_iter(
        &self,
    ) -> impl Iterator<Item = (WidgetId, bool, bool)> + '_ {
        self.nodes.iter().filter_map(|(id, node)| {
            node.visible_state.as_ref().map(|state| {
                let is_active = node.activation == ActivationState::Active;
                let should_be_visible = state.get();
                (id, is_active, should_be_visible)
            })
        })
    }

    /// Check if a widget is effectively enabled, walking up the parent chain.
    ///
    /// Returns `false` if the widget itself or any ancestor has `enabled_state`
    /// bound to `false`. This lets containers like `GroupBox` disable a whole
    /// subtree by binding a single signal on their content wrapper.
    pub fn is_enabled(&self, id: WidgetId) -> bool {
        let mut current = Some(id);
        while let Some(node_id) = current {
            if let Some(node) = self.nodes.get(node_id) {
                if let Some(ref state) = node.enabled_state
                    && !state.get()
                {
                    return false;
                }
                current = node.parent;
            } else {
                return true;
            }
        }
        true
    }

    /// Set a per-child alignment override on a widget.
    pub fn set_alignment_override(&mut self, id: WidgetId, alignment: fern_tokens::Alignment) {
        if let Some(node) = self.get_mut(id) {
            node.alignment_override = Some(alignment);
        }
    }

    /// Mark a widget as clipping its children (scroll area, overflow hidden).
    pub fn set_clips_children(&mut self, id: WidgetId, clips: bool) {
        if let Some(node) = self.get_mut(id) {
            node.clips_children = clips;
        }
    }

    /// Apply a `HandlerSet` to an existing node, merging handlers and
    /// transferring node-level metadata (focusable, cursor, clips,
    /// context menu). The `scope` argument controls whether the
    /// handlers go into the rebuild-cleared `handlers` slot or the
    /// persistent `external_handlers` slot.
    pub(crate) fn apply_handler_set(
        &mut self,
        id: WidgetId,
        handler_set: crate::widget_builder::HandlerSet,
        scope: HandlerScope,
    ) {
        if let Some(node) = self.get_mut(id) {
            let target = match scope {
                HandlerScope::Own => &mut node.handlers,
                HandlerScope::External => &mut node.external_handlers,
            };
            let existing = std::mem::take(target);
            *target = existing.merge(handler_set.handlers);
            if let Some(focusable) = handler_set.focusable {
                node.node_focusable = Some(focusable);
            }
            if let Some(tab_index) = handler_set.tab_index {
                node.node_tab_index = Some(tab_index);
            }
            if let Some(cursor) = handler_set.cursor {
                node.node_cursor = Some(cursor);
            }
            if let Some(clips) = handler_set.clips_children {
                node.clips_children = clips;
            }
            if let Some(pass_through) = handler_set.event_pass_through {
                node.event_pass_through = pass_through;
            }
            if handler_set.context_menu_factory.is_some() {
                node.context_menu_factory = handler_set.context_menu_factory;
            }
            if let Some(sig) = handler_set.focus_within {
                node.focus_within_signal = Some(sig);
            }
            if let Some(sig) = handler_set.hover_within {
                node.hover_within_signal = Some(sig);
            }
            // Mirror builder-level accessibility overrides + subtree mode
            // onto the persistent WidgetNode so the accessibility tree
            // walker (and the event dispatcher, for action callbacks) can
            // read them after handler extraction.
            if handler_set.access.is_some() {
                node.access_overrides = handler_set.access;
            }
            if let Some(mode) = handler_set.access_subtree {
                node.access_subtree = mode;
            }
        }
    }

    /// Get a widget's alignment override, if any.
    pub fn alignment_override(&self, id: WidgetId) -> Option<fern_tokens::Alignment> {
        self.get(id)?.alignment_override
    }

    /// Temporarily take the widget box out of a node (for rebuild).
    /// The node remains in the arena with a placeholder.
    pub fn take_widget(&mut self, id: WidgetId) -> Option<Box<dyn Widget>> {
        let node = self.nodes.get_mut(id)?;
        // Replace with a minimal placeholder
        let taken = std::mem::replace(&mut node.widget, Box::new(PlaceholderWidget));
        Some(taken)
    }

    /// Restore a widget box that was previously taken out.
    pub fn restore_widget(&mut self, id: WidgetId, widget: Box<dyn Widget>) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.widget = widget;
        }
    }

    /// Walk up the parent chain from `id` and mark each ancestor as needing layout.
    /// Called when a relayout-level binding changes, since a child's size change
    /// may affect its parent's size, and so on up to the root.
    pub fn mark_ancestors_need_layout(&mut self, id: WidgetId) {
        let mut current = self.parent(id);
        while let Some(pid) = current {
            if let Some(node) = self.get_mut(pid) {
                node.dirty.needs_layout = true;
                node.dirty.needs_paint = true;
            }
            current = self.parent(pid);
        }
    }

    /// Mark all widgets as needing layout and paint (e.g. after a theme change).
    /// Also clears per-widget paint caches since the visual output is stale.
    pub fn mark_all_dirty(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
            node.cached_paint = None;
        }
    }

    /// Resolve the effective theme for a widget by walking ancestors and
    /// applying any theme overrides encountered along the way.
    /// The base theme is the tree-level default.
    pub fn resolve_theme(&self, id: WidgetId, base: &fern_tokens::Theme) -> fern_tokens::Theme {
        // Fast path: if no widget has a theme override, skip the ancestor walk.
        if self.theme_override_count == 0 {
            return base.clone();
        }

        // Collect ancestor chain from root to widget
        let mut chain = vec![id];
        let mut current = self.parent(id);
        while let Some(pid) = current {
            chain.push(pid);
            current = self.parent(pid);
        }
        chain.reverse(); // root first

        let mut theme = base.clone();
        for nid in chain {
            if let Some(node) = self.nodes.get(nid)
                && let Some(ovr) = &node.theme_override
            {
                (ovr.func)(&mut theme);
            }
        }
        theme
    }
}

impl Default for WidgetArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;

    #[test]
    fn insert_and_retrieve() {
        let mut arena = WidgetArena::new();
        let id = arena.insert(Box::new(FillWidget::new()));
        assert!(arena.get(id).is_some());
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn new_widget_is_dirty() {
        let mut arena = WidgetArena::new();
        let id = arena.insert(Box::new(FillWidget::new()));
        let node = arena.get(id).unwrap();
        assert!(node.dirty.needs_layout);
        assert!(node.dirty.needs_paint);
    }

    #[test]
    fn roots_returns_parentless_widgets() {
        let mut arena = WidgetArena::new();
        let root = arena.insert(Box::new(FillWidget::new()));
        let _child = arena.insert_child(root, Box::new(FillWidget::new()));
        let roots = arena.roots();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], root);
    }
}
