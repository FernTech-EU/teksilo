use fern_canvas::{Canvas, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::widget_id::WidgetId;

/// A child that is either pre-registered (ID) or waiting to be inserted.
/// Used by the inline `child()` builder pattern: deferred children are stored
/// inside the container and resolved recursively when `BuildContext::add()`
/// inserts the container into the arena.
pub enum PendingChild {
    /// Already in the arena — use this ID directly.
    Id(WidgetId),
    /// Not yet in the arena — insert during resolution.
    Deferred(Box<dyn Widget>),
}

impl std::fmt::Debug for PendingChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PendingChild::Id(id) => write!(f, "PendingChild::Id({:?})", id),
            PendingChild::Deferred(_) => f.write_str("PendingChild::Deferred(..)"),
        }
    }
}

/// A widget's reply to a parent's layout query.
///
/// Carries both the size the widget wants (a floor the parent must honor)
/// and a flex weight that tells the parent how to distribute any leftover
/// **slack** among siblings. `flex == 0.0` means rigid (no opinion on
/// slack); `flex > 0.0` means the widget wants a share proportional to its
/// weight.
///
/// Most widgets just return a `Size`; the `From<Size>` impl wraps it as a
/// rigid response. Flex-bearing widgets (`Spacer`, `Expand`, …) construct
/// `LayoutResponse { size, flex }` directly or use
/// [`LayoutResponse::flexible`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutResponse {
    pub size: Size,
    pub flex: f32,
}

impl LayoutResponse {
    pub const ZERO: Self = Self {
        size: Size::ZERO,
        flex: 0.0,
    };

    pub fn rigid(size: Size) -> Self {
        Self { size, flex: 0.0 }
    }

    pub fn flexible(size: Size, flex: f32) -> Self {
        Self {
            size,
            flex: flex.max(0.0),
        }
    }

    /// Builder: set the flex weight on an existing response.
    pub fn with_flex(mut self, flex: f32) -> Self {
        self.flex = flex.max(0.0);
        self
    }
}

impl From<Size> for LayoutResponse {
    fn from(size: Size) -> Self {
        Self { size, flex: 0.0 }
    }
}

/// Context available during layout.
pub struct LayoutContext<'a> {
    pub theme: &'a fern_tokens::Theme,
    pub layout_direction: crate::environment::LayoutDirection,
    /// Text backend for accurate text measurement during layout.
    pub text_backend: Option<&'a std::rc::Rc<std::cell::RefCell<dyn fern_canvas::TextBackend>>>,
    /// Arena reference for querying child widget sizes.
    pub(crate) arena: Option<&'a crate::arena::WidgetArena>,
    /// Optional bundle of read-only tree state (focus, shortcuts,
    /// overlays). Carried through layout for debug-tooling consumers
    /// like the inspector's Focus / Shortcuts / Overlays tabs. `None`
    /// in test contexts.
    pub(crate) extras: Option<LayoutExtras<'a>>,
}

/// Read-only handles to tree-level state that some widgets (notably
/// the debug inspector) want to query from `layout_response`. Threaded
/// through the recursive layout pass alongside the arena.
#[derive(Clone, Copy)]
pub(crate) struct LayoutExtras<'a> {
    pub focused: Option<WidgetId>,
    pub shortcut_registry: Option<&'a crate::shortcut::ShortcutRegistry>,
    pub overlay_manager: Option<&'a crate::overlay::OverlayManager>,
}

impl<'a> LayoutContext<'a> {
    /// Create a LayoutContext for testing (no arena access).
    pub fn for_testing(theme: &'a fern_tokens::Theme) -> Self {
        Self {
            theme,
            layout_direction: crate::environment::LayoutDirection::LeftToRight,
            text_backend: None,
            arena: None,
            extras: None,
        }
    }

    /// The currently focused widget id, if any. Returns `None` when
    /// the layout pass is unrelated to a tree (test contexts).
    pub fn focused(&self) -> Option<WidgetId> {
        self.extras.as_ref().and_then(|e| e.focused)
    }

    /// Borrow the tree's shortcut registry. Returns `None` outside a
    /// real layout pass. Intended for read-only inspection by the
    /// debug inspector.
    pub fn shortcut_registry(&self) -> Option<&crate::shortcut::ShortcutRegistry> {
        self.extras.as_ref().and_then(|e| e.shortcut_registry)
    }

    /// Borrow the tree's overlay manager. Returns `None` outside a
    /// real layout pass. Intended for read-only inspection by the
    /// debug inspector.
    pub fn overlay_manager(&self) -> Option<&crate::overlay::OverlayManager> {
        self.extras.as_ref().and_then(|e| e.overlay_manager)
    }

    /// Query a child widget's full layout response (wanted size + flex weight).
    /// Returns None if the child doesn't exist, is dormant, or the arena is not available.
    pub fn child_layout_response(
        &self,
        child_id: WidgetId,
        proposal: fern_canvas::SizeProposal,
    ) -> Option<LayoutResponse> {
        let arena = self.arena?;
        if !arena.is_active(child_id) {
            return None;
        }
        let node = arena.get(child_id)?;
        Some(node.widget.layout_response(proposal, self))
    }

    /// Query a child widget's wanted size only (drops the flex weight).
    /// Convenience over [`child_layout_response`](Self::child_layout_response).
    pub fn child_size(
        &self,
        child_id: WidgetId,
        proposal: fern_canvas::SizeProposal,
    ) -> Option<fern_canvas::Size> {
        self.child_layout_response(child_id, proposal)
            .map(|r| r.size)
    }

    /// Query the laid-out bounds of any active widget. Returns `None`
    /// when the arena is not available (test contexts) — otherwise
    /// returns the widget's current bounds (`Rect::ZERO` if unknown).
    /// Useful for inspector-style widgets that need to mirror another
    /// widget's geometry into a `Signal` during the layout pass.
    pub fn widget_bounds(&self, id: WidgetId) -> Option<fern_canvas::Rect> {
        let arena = self.arena?;
        if !arena.is_active(id) {
            return None;
        }
        Some(arena.bounds(id))
    }

    /// Hit-test the active widget tree at `point` and return the
    /// deepest widget under it. Honors `event_pass_through`. The
    /// `exclude` argument lets the caller skip a specific subtree
    /// (e.g. the inspector's picker overlay so it doesn't pick
    /// itself). Returns `None` outside layout (no arena available).
    pub fn widget_at_point(
        &self,
        point: fern_canvas::Point,
        exclude: Option<WidgetId>,
    ) -> Option<WidgetId> {
        let arena = self.arena?;
        arena.hit_test_at(point, exclude)
    }

    /// Borrow the underlying arena. Returns `None` outside a layout
    /// pass (test contexts). Intended for read-only introspection by
    /// debug tooling (the inspector's tree view) — use the typed
    /// accessors above when possible.
    pub fn arena(&self) -> Option<&crate::arena::WidgetArena> {
        self.arena
    }

    /// Query a child's per-widget alignment override, if any.
    pub fn child_alignment(&self, child_id: WidgetId) -> Option<fern_tokens::Alignment> {
        let arena = self.arena?;
        arena.alignment_override(child_id)
    }

    /// Whether the layout direction is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.layout_direction == crate::environment::LayoutDirection::RightToLeft
    }
}

/// Context available during painting.
pub struct PaintContext<'a> {
    pub theme: &'a fern_tokens::Theme,
    pub scale_factor: f32,
    // TODO: Wire from platform accessibility settings (winit doesn't expose these yet)
    pub prefers_high_contrast: bool,
    pub prefers_reduced_motion: bool,
    pub prefers_large_text: bool,
}

/// Placement of a child widget during layout.
#[derive(Debug, Clone, Copy)]
pub struct WidgetPlacement {
    pub id: WidgetId,
    pub origin: fern_canvas::Point,
    pub size: Size,
}

/// Cursor icon for the mouse pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Default,
    Pointer,
    Text,
    Crosshair,
    Move,
    NotAllowed,
    Grab,
    Grabbing,
    ColResize,
    RowResize,
    /// NE/SW diagonal resize — used on the top-right and bottom-left
    /// corners of a resize frame. Maps to winit's `NeswResize`.
    NeswResize,
    /// NW/SE diagonal resize — used on the top-left and bottom-right
    /// corners of a resize frame. Maps to winit's `NwseResize`.
    NwseResize,
}

/// The full Widget trait for Level 2 (custom rendering) widgets.
pub trait Widget: std::fmt::Debug + std::any::Any {
    /// Concrete type name of this widget (e.g.
    /// `"fern_widgets::button::Button"`). The default implementation
    /// resolves at the impl site via `std::any::type_name::<Self>()`,
    /// so calls through `&dyn Widget` correctly dispatch to the
    /// monomorphized fn for the concrete type — getting the
    /// concrete name through the vtable without per-impl boilerplate.
    ///
    /// Used by [`crate::widget_tree::WidgetTree::widget_type_histogram`]
    /// for the `widget.census` telemetry event (Phase 5.3). Custom
    /// widgets that wrap their state in a generic struct may
    /// override to give analytics a stable name independent of the
    /// generic parameter.
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Compose child widgets. Called once after the widget is placed in the
    /// arena, and again on environment change (theme switch, locale switch).
    /// Takes `&mut self` — store child IDs, signal handles, any state needed later.
    /// Returns the list of root child IDs (empty for leaf widgets).
    fn build(
        &mut self,
        _ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        Vec::new()
    }

    /// Respond to the parent's size proposal with this widget's wanted size
    /// and flex weight.
    ///
    /// Most widgets just return a `Size` (auto-converts via `From<Size>` to
    /// a rigid response with `flex = 0`). Flex-bearing widgets (`Spacer`,
    /// `Expand`) return a [`LayoutResponse`] with a non-zero flex.
    ///
    /// The parent honors `size` as a floor and distributes any leftover
    /// **slack** among siblings proportional to `flex`.
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse;

    /// Position children within the allocated bounds.
    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Leaf widgets have no children to place..into()
    }

    /// Draw the widget's visual representation.
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {
        // Default: nothing to paint (layout-only containers).
    }

    /// Declare this widget's accessibility identity.
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    /// Whether this widget wants the AT walker to consult its
    /// [`a11y_redirect_descendant`](Self::a11y_redirect_descendant)
    /// hook for *every* descendant during AT tree emission, not
    /// just its direct arena children.
    ///
    /// Returning `true` opts this widget into ancestor-chain
    /// queries: as the walker iterates each descendant's parent
    /// to decide where the descendant's `NodeId` lands in the AT
    /// tree, it walks up the arena from that parent and asks
    /// every ancestor with this flag set. First `Some(_)` wins
    /// (closest ancestor takes priority — same precedence as a
    /// CSS-like cascade).
    ///
    /// Returning `false` (the default) makes the walker pay the
    /// O(depth) ancestor walk only for trees that genuinely need
    /// it. Only opt in if your widget actively places
    /// non-direct-child descendant `NodeId`s in its own
    /// `accessibility()` emission — `fern_scene::SceneView` is
    /// the canonical example.
    ///
    /// Default: `false`.
    fn wants_descendant_redirects(&self) -> bool {
        false
    }

    /// Optional redirection hook for AT-tree placement of a child.
    ///
    /// The accessibility walker calls this on the immediate arena
    /// parent of each child — and, if the parent
    /// [`wants_descendant_redirects`](Self::wants_descendant_redirects)
    /// returns `false`, on every opt-in ancestor walking up the
    /// arena from that parent. First `Some(_)` wins, scanned
    /// bottom-up (closest ancestor takes priority). Returning
    /// `Some(_)` tells the walker that this widget has *already*
    /// placed `descendant`'s `NodeId` somewhere else (typically
    /// under a synthetic node it emitted in its own
    /// `accessibility()` call), and the walker should NOT add it
    /// to its arena parent's children list.
    ///
    /// The returned `NodeId` is informational — it identifies the
    /// new logical parent in case the walker wants to bookkeep
    /// (e.g., dedupe). The walker does not validate that
    /// `descendant`'s NodeId is actually in that target's children
    /// list; it is the implementing widget's responsibility to
    /// have placed it there during its `accessibility()` emission
    /// (e.g. via `AccessNodeBuilder::attach_scene_child_under`).
    ///
    /// Used by `fern_scene::SceneView` to graft heavyweight
    /// `Widget` items into an app-declared logical AT tree (Phase
    /// 5b). Other layered containers can adopt the same pattern.
    ///
    /// Default: `None` — no redirection.
    fn a11y_redirect_descendant(
        &self,
        _self_id: WidgetId,
        _descendant: WidgetId,
    ) -> Option<accesskit::NodeId> {
        None
    }

    /// Suggest an accessible title to an enclosing container that
    /// wraps this widget as content — typically a modal / dialog
    /// shell that wants to propagate the inner content's visible
    /// title as the shell's own accessible name.
    ///
    /// Example: `ModalContainer` wraps a `DialogContent`. The
    /// container owns the `Role::Dialog` node and needs a name;
    /// `DialogContent` overrides this method to return its own
    /// `title` string. The container queries this on its pending
    /// content at build time and uses the result if set.
    ///
    /// Default: `None` — widgets that don't carry a natural
    /// title don't need to override.
    fn accessible_title_hint(&self) -> Option<String> {
        None
    }

    /// Optional hint that directs initial focus to a specific
    /// descendant when this widget is the root of a deferred-built
    /// modal surface.
    ///
    /// The modal presentation pipeline consults this after building
    /// the content subtree, in priority order: the caller's
    /// `ModalRequest::focus_target` → the content widget's
    /// `initial_focus_hint` → `first_focusable_descendant`.
    /// `MessageBox` overrides this to return the widget id of its
    /// configured default button, so platform-native button orderings
    /// (Cancel-left + Default-right-but-focused) work without
    /// forcing the default button to be the first focusable
    /// descendant in tree-walk order.
    ///
    /// Default: `None` — widgets that don't need to direct initial
    /// focus to a non-first-focusable descendant don't override.
    fn initial_focus_hint(&self) -> Option<WidgetId> {
        None
    }

    /// Return the child widget IDs that this widget manages.
    fn children(&self) -> Vec<WidgetId> {
        Vec::new()
    }

    /// Downcast hook. Default implementation returns `None`; concrete
    /// widgets override with `Some(self)` when they want to expose
    /// their concrete type to test-level introspection or reflection.
    /// The trait already bounds on `std::any::Any` so concrete types
    /// satisfy the `'static` requirement.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Mutable counterpart of [`as_any`](Self::as_any). Default
    /// returns `None`; widgets that want to expose mutable state to
    /// tests (e.g. so a test can mutate a `Scene` inside a
    /// `SceneView` post-layout) override with `Some(self)`. Should
    /// follow the same opt-in pattern as `as_any`: only widgets
    /// that opt into `&` introspection should opt into `&mut`.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Whether this widget clips its children to its bounds.
    fn clips_children(&self) -> bool {
        false
    }

    /// Whether `rebuild_single_widget` should **preserve** existing
    /// children (skip the destroy-subtree-then-rebuild dance) when
    /// re-running this widget's `build()`.
    ///
    /// Default `false` — rebuild is a "tear down and reconstruct"
    /// operation, the right semantic for data-driven widgets like
    /// `Repeater` / `ListView` where every rebuild constructs fresh
    /// children from current model state.
    ///
    /// Override to `true` for widgets whose children are stable
    /// across rebuilds — they were created once at first build via
    /// `ctx.add` / `ctx.add_boxed`, and subsequent rebuilds just
    /// re-push the same `WidgetId`s. `SceneView` is the canonical
    /// example: heavyweight scene widgets are materialised on first
    /// build from `Scene::add_widget` pending entries; subsequent
    /// rebuilds (triggered to drain pending drag-to-move / marquee
    /// commits) MUST keep those widgets attached or the user sees
    /// the cards / nested SceneView "disappear" on every drag end.
    ///
    /// When `true`, `build()` is responsible for returning a
    /// children vec that contains every `WidgetId` it wants to
    /// keep attached — the framework still updates the parent's
    /// `children` field from that return value, so any IDs not in
    /// the returned vec are orphaned (still in the arena, no
    /// parent), exactly as if `false`. The opt-in only skips the
    /// active `destroy_subtree` step that would tear them down
    /// and unmount their state.
    fn preserves_children_on_rebuild(&self) -> bool {
        false
    }

    /// Extract attached handler set from a `WidgetWithHandlers` wrapper.
    /// Called during arena insertion to transfer handlers to the `WidgetNode`.
    /// Default: returns `None` (no attached handlers).
    fn take_handler_set(&mut self) -> Option<crate::widget_builder::HandlerSet> {
        None
    }
}

/// Context available during event handling.
pub struct EventContext<'ops> {
    pub(crate) cursor_request: Option<CursorIcon>,
    pub(crate) tree_mutations: Vec<TreeMutation>,
    pub(crate) idle_callbacks: Vec<crate::idle::IdleCallback>,
    pub(crate) modal_requests: Vec<crate::modal::ModalRequest>,
    pub(crate) dismiss_modal: bool,
    pub(crate) overlay_requests: Vec<crate::overlay::OverlayRequest>,
    pub(crate) overlay_dismissals: Vec<crate::overlay::OverlayId>,
    /// Whether to dismiss all overlays (e.g., after menu item activation).
    pub(crate) dismiss_all_overlays: bool,
    /// Whether to dismiss just the topmost overlay (e.g., ArrowLeft in submenu).
    pub(crate) dismiss_top: bool,
    /// Request to capture or release the pointer.
    pub(crate) pointer_capture: Option<bool>,
    /// Delayed overlay requests (request, delay, optional focus target).
    pub(crate) delayed_overlay_requests: Vec<(
        crate::overlay::OverlayRequest,
        std::time::Duration,
        Option<crate::widget_id::WidgetId>,
    )>,
    /// Timed overlay requests (request, auto-dismiss delay).
    pub(crate) timed_overlay_requests: Vec<(crate::overlay::OverlayRequest, std::time::Duration)>,
    /// Dismiss descendant overlays of the source widget's containing overlay.
    /// Optionally preserve the subtree rooted at a specific content widget ID.
    pub(crate) dismiss_descendant_overlays: Vec<Option<crate::widget_id::WidgetId>>,
    /// Cancel pending delayed overlays by content widget ID.
    pub(crate) cancel_delayed_overlays: Vec<crate::widget_id::WidgetId>,
    /// Widget IDs that need repainting (cross-widget signal propagation).
    pub(crate) repaint_requests: Vec<crate::widget_id::WidgetId>,
    /// Synthetic clicks to dispatch on target widgets after event processing.
    pub(crate) synthetic_clicks: Vec<crate::widget_id::WidgetId>,
    /// Focus requests — transfer focus to a specific widget (e.g., overlay content on open).
    pub(crate) focus_requests: Vec<crate::widget_id::WidgetId>,
    /// Drag start request: (source_widget_id, payload, optional_preview_widget).
    pub(crate) drag_start_request: Option<(
        crate::widget_id::WidgetId,
        crate::drag_payload::DragPayload,
        Option<Box<dyn crate::widget::Widget>>,
    )>,
    /// Cancel any active drag session.
    pub(crate) cancel_drag: bool,
    /// Replace the tree-level theme. Drained after dispatch; triggers a
    /// composite-widget rebuild and full repaint.
    pub(crate) theme_request: Option<fern_tokens::Theme>,
    /// Replace the tree-level locale identifier. Drained after dispatch;
    /// triggers a composite-widget rebuild and full repaint.
    pub(crate) locale_request: Option<String>,
    /// Set by `request_frame()`; consumed by the event dispatcher which
    /// forwards it to `WidgetTree::request_frame()` so the next layout
    /// pass advances the per-frame tick signal.
    pub(crate) frame_requested: bool,
    /// Optional reference to the tree's app-state registry, so handlers
    /// can look up application-scoped values via `app_state::<T>()`.
    /// Populated by the dispatcher before running each handler; `None`
    /// for hand-constructed contexts in tests.
    pub(crate) app_context: Option<std::rc::Rc<crate::event_source::TreeAppContext>>,
    /// App-level window-ops sink. Injected by the dispatcher so
    /// handlers can reach the multi-window API (`open_window`,
    /// `focus_window`, …) synchronously. For `EventContext`
    /// instances constructed outside a dispatch (standalone trees,
    /// tests) this is `None` and the multi-window methods no-op /
    /// return `None`.
    pub(crate) window_ops: Option<&'ops mut dyn crate::window::WindowOps>,
    /// [`WindowState`](crate::window::WindowState) for the window
    /// this tree belongs to. Cloned from the tree at construction.
    /// `None` for standalone trees.
    pub(crate) current_window: Option<crate::window::WindowState>,
    /// Intents queued by handlers via `send_intent`. Drained by the
    /// tree after event dispatch and routed source-widget → root.
    pub(crate) pending_intents: Vec<crate::intent::Intent>,
    /// Phase 5.2 — the dispatcher sets this to the appropriate
    /// [`IntentSource`](crate::telemetry::IntentSource) before
    /// invoking a typed handler (menu select → `Menu`, AccessKit
    /// action → `Accessibility`, on_tap / button activation →
    /// `Handler`, …). `send_intent` reads it and stamps the intent
    /// before queuing. `None` outside a managed handler — bare
    /// programmatic sends keep their `Intent::source` value
    /// (default `Programmatic`).
    pub(crate) current_source: Option<crate::telemetry::IntentSource>,
    /// Key-capture callback armed via `ctx.begin_key_capture(...)`.
    /// The callback + its shared slot are installed on the tree by
    /// `collect_from_ctx`. Only one per ctx; the last caller wins.
    pub(crate) pending_key_capture: Option<crate::shortcut::KeyCaptureSlot>,
    /// Set to request cancellation of any armed key capture.
    pub(crate) cancel_key_capture: bool,
    /// Deferred mutations to the tree's [`ShortcutRegistry`](crate::shortcut::ShortcutRegistry),
    /// typically issued by settings-UI buttons to rebind or clear
    /// overrides. Applied in `collect_from_ctx` after the handler
    /// returns.
    pub(crate) pending_shortcut_mutations: Vec<ShortcutMutation>,
    /// Requests that the app-level event loop close the window this
    /// tree belongs to. Drained after dispatch via
    /// `WidgetTree::take_close_window_request`.
    pub(crate) close_window_requested: bool,
}

/// Deferred edit to the tree's shortcut registry, queued on an
/// `EventContext` and applied in `collect_from_ctx`.
#[derive(Debug, Clone)]
pub(crate) enum ShortcutMutation {
    RebindPrimary {
        id: String,
        keystroke: Option<crate::shortcut::KeyStroke>,
    },
    RebindSecondary {
        id: String,
        keystroke: Option<crate::shortcut::KeyStroke>,
    },
    ClearOverride {
        id: String,
    },
}

/// A structural change to the widget tree, deferred until after event dispatch.
#[derive(Debug)]
pub(crate) enum TreeMutation {
    SetDormant(WidgetId),
    Activate(WidgetId),
    Destroy(WidgetId),
}

impl<'ops> EventContext<'ops> {
    pub(crate) fn new() -> Self {
        Self {
            cursor_request: None,
            tree_mutations: Vec::new(),
            idle_callbacks: Vec::new(),
            modal_requests: Vec::new(),
            dismiss_modal: false,
            overlay_requests: Vec::new(),
            overlay_dismissals: Vec::new(),
            dismiss_all_overlays: false,
            dismiss_top: false,
            pointer_capture: None,
            delayed_overlay_requests: Vec::new(),
            timed_overlay_requests: Vec::new(),
            dismiss_descendant_overlays: Vec::new(),
            cancel_delayed_overlays: Vec::new(),
            repaint_requests: Vec::new(),
            synthetic_clicks: Vec::new(),
            focus_requests: Vec::new(),
            drag_start_request: None,
            cancel_drag: false,
            theme_request: None,
            locale_request: None,
            frame_requested: false,
            app_context: None,
            pending_intents: Vec::new(),
            current_source: None,
            pending_key_capture: None,
            cancel_key_capture: false,
            pending_shortcut_mutations: Vec::new(),
            close_window_requested: false,
            window_ops: None,
            current_window: None,
        }
    }

    /// Attach the app-level window-ops sink and the hosting tree's
    /// [`WindowState`](crate::window::WindowState). Called by the
    /// dispatcher once per event batch so handlers can reach the
    /// multi-window API synchronously.
    pub(crate) fn with_window_context(
        mut self,
        ops: &'ops mut dyn crate::window::WindowOps,
        current_window: Option<crate::window::WindowState>,
    ) -> Self {
        self.window_ops = Some(ops);
        self.current_window = current_window;
        self
    }


    /// Attach the tree's app-state registry so handlers can look up
    /// application-scoped values (`ClipboardHandle`, `SharedTypesetter`,
    /// …). Called by the dispatcher once per event batch.
    pub(crate) fn with_app_context(
        mut self,
        ctx: std::rc::Rc<crate::event_source::TreeAppContext>,
    ) -> Self {
        self.app_context = Some(ctx);
        self
    }

    /// Look up an application-scoped value by type. Mirrors
    /// `BuildContext::app_state`. Returns `None` when the handler was
    /// invoked without a registry (hand-constructed `EventContext` in
    /// tests, or when no value of that type was registered).
    pub fn app_state<T: 'static>(&self) -> Option<&T> {
        self.app_context.as_ref().and_then(|ctx| ctx.app_state::<T>())
    }

    /// Borrow the [`AppEventPoster`](crate::AppEventPoster) installed
    /// by the framework. Used by integrations that need to post
    /// typed payloads back to the UI loop from a worker thread
    /// (`fern_platform::file_dialog`'s `RfdAsyncBackend`, future
    /// async-result features). Returns `None` for hand-constructed
    /// `EventContext`s in tests.
    pub fn poster(&self) -> Option<&std::sync::Arc<dyn crate::AppEventPoster>> {
        self.app_context.as_ref().and_then(|ctx| ctx.poster())
    }

    /// Ask the tree to pump one more frame after this handler returns.
    /// Use from event handlers that kick off per-frame work (pending
    /// document events to drain, drag-select auto-scroll, caret blink
    /// restart on focus). See `WidgetTree::request_frame` for the
    /// draw-when-needed contract.
    pub fn request_frame(&mut self) {
        self.frame_requested = true;
    }

    /// Dispatch an [`Intent`](crate::intent::Intent) as if the source
    /// widget pressed its keyboard shortcut. The framework walks
    /// source-widget → root after the current handler returns,
    /// invoking any matching [`Action`](crate::action::Action) it
    /// finds. Unmatched intents are silently dropped.
    ///
    /// The intent's `source` is overridden by the dispatcher's
    /// current handler-source label (`current_source`) when one is
    /// active. This is how the framework distinguishes
    /// `IntentSource::Handler` (button taps, generic on_tap) from
    /// `IntentSource::Menu`, `IntentSource::Accessibility`, etc.
    /// Programmatic callers outside any handler pass through with
    /// `IntentSource::Programmatic` (the default).
    pub fn send_intent(&mut self, intent: impl Into<crate::intent::Intent>) {
        let mut intent: crate::intent::Intent = intent.into();
        if let Some(src) = self.current_source {
            intent.source = src;
        }
        self.pending_intents.push(intent);
    }

    /// Run a closure with the given [`IntentSource`] active. Any
    /// `ctx.send_intent(...)` issued from within the closure will
    /// be tagged with this source instead of the dispatcher's
    /// default (`Handler` / `Shortcut` / `Accessibility`).
    ///
    /// The previous source is restored after the closure returns.
    /// Panic during the closure unwinds the dispatcher's whole
    /// frame, so the EventContext is destroyed before the next
    /// dispatch — no need for a panic-safe drop guard.
    ///
    /// Used by framework widgets that want a more specific source
    /// label than the default — `MenuItem` wraps its activation
    /// handler to emit `IntentSource::Menu`, etc.
    pub fn with_intent_source<R>(
        &mut self,
        source: crate::telemetry::IntentSource,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.current_source.replace(source);
        let r = f(self);
        self.current_source = prev;
        r
    }

    /// Arm a one-shot key-capture callback, returning a
    /// [`CaptureHandle`](crate::shortcut::CaptureHandle) whose `Drop`
    /// cancels the capture if it hasn't fired yet. The next `KeyDown`
    /// bypasses shortcut resolution and invokes the callback with:
    /// - the captured [`KeyStroke`](crate::shortcut::KeyStroke)
    /// - mutable access to the registry (rebinds in-place)
    /// - a mutable [`EventContext`] (emit commands, send intents,
    ///   dismiss overlays, …)
    ///
    /// The handle must be stored somewhere with an appropriate
    /// lifetime (typically in the calling widget's state) or the
    /// capture will be cancelled immediately when the returned
    /// handle drops at end of scope.
    pub fn begin_key_capture(
        &mut self,
        callback: impl FnOnce(
                crate::shortcut::KeyStroke,
                &mut crate::shortcut::ShortcutRegistry,
                &mut EventContext,
            ) + 'static,
    ) -> crate::shortcut::CaptureHandle {
        let slot: crate::shortcut::KeyCaptureSlot =
            std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(callback))));
        self.pending_key_capture = Some(slot.clone());
        self.cancel_key_capture = false;
        crate::shortcut::CaptureHandle::new(slot)
    }

    /// Cancel any key capture armed earlier in this handler or via
    /// [`WidgetTree::begin_key_capture`] before the handler ran.
    pub fn cancel_key_capture(&mut self) {
        self.pending_key_capture = None;
        self.cancel_key_capture = true;
    }

    /// Queue a deferred rebind of the primary keystroke for the
    /// registered shortcut with the given id. Applied by the tree
    /// after the current handler returns. Use `None` to explicitly
    /// unbind the slot.
    pub fn rebind_shortcut_primary(
        &mut self,
        id: impl Into<String>,
        keystroke: Option<crate::shortcut::KeyStroke>,
    ) {
        self.pending_shortcut_mutations
            .push(ShortcutMutation::RebindPrimary {
                id: id.into(),
                keystroke,
            });
    }

    /// Queue a deferred rebind of the secondary keystroke for the
    /// registered shortcut with the given id.
    pub fn rebind_shortcut_secondary(
        &mut self,
        id: impl Into<String>,
        keystroke: Option<crate::shortcut::KeyStroke>,
    ) {
        self.pending_shortcut_mutations
            .push(ShortcutMutation::RebindSecondary {
                id: id.into(),
                keystroke,
            });
    }

    /// Queue a deferred clear of any user override for the given
    /// shortcut id, restoring its declared defaults.
    pub fn clear_shortcut_override(&mut self, id: impl Into<String>) {
        self.pending_shortcut_mutations
            .push(ShortcutMutation::ClearOverride { id: id.into() });
    }

    /// Request that the application close the window this tree
    /// belongs to. Drained by the app event loop after the handler
    /// returns. Typical use: title-bar close button handlers.
    pub fn close_window(&mut self) {
        self.close_window_requested = true;
    }

    // -------------------- Multi-window API --------------------

    /// The [`WindowState`](crate::window::WindowState) for the window
    /// hosting this handler. `None` only for handlers run outside
    /// of an app (hand-constructed `EventContext` in tests).
    pub fn window(&self) -> Option<&crate::window::WindowState> {
        self.current_window.as_ref()
    }

    /// Open a new window, creating the winit-level surface
    /// synchronously. The returned id is immediately valid for
    /// [`focus_window`](Self::focus_window),
    /// [`window_state`](Self::window_state), and
    /// [`find_window`](Self::find_window).
    ///
    /// Panics when called from a handler on a standalone `WidgetTree`
    /// (no app context) — tests should not invoke this method.
    pub fn open_window(
        &mut self,
        config: crate::window::WindowConfig,
    ) -> crate::window::FernWindowId {
        self.window_ops
            .as_deref_mut()
            .expect("open_window called outside of a dispatch")
            .open_window(config)
    }

    /// Find a window by the string id assigned via
    /// [`WindowConfig::id`](crate::window::WindowConfig::id). Returns
    /// `None` if no open window carries that id.
    pub fn find_window(&self, string_id: &str) -> Option<crate::window::FernWindowId> {
        self.window_ops.as_deref()?.find_window(string_id)
    }

    /// Read the [`WindowState`](crate::window::WindowState) for a
    /// specific window.
    pub fn window_state(
        &self,
        id: crate::window::FernWindowId,
    ) -> Option<crate::window::WindowState> {
        self.window_ops.as_deref()?.window_state(id)
    }

    /// Snapshot of every live window's state.
    pub fn windows(&self) -> Vec<crate::window::WindowState> {
        self.window_ops
            .as_deref()
            .map(|o| o.windows())
            .unwrap_or_default()
    }

    /// Raise a window to the front and give it keyboard focus.
    pub fn focus_window(&mut self, id: crate::window::FernWindowId) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.focus_window(id);
        }
    }

    /// Close a specific window by id. Equivalent to
    /// [`close_window`](Self::close_window) when `id` is the current
    /// window's id.
    pub fn close_window_by_id(&mut self, id: crate::window::FernWindowId) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.close_window_by_id(id);
        }
    }

    /// Resolve the platform parent handle of the window currently
    /// dispatching the event. Used by native-dialog integrations
    /// (`fern_platform::file_dialog`) to parent OS dialogs to the
    /// originating FernUI window.
    ///
    /// Returns `None` when called from a standalone `WidgetTree` (no
    /// app-level [`WindowOps`] sink), or when the platform refuses to
    /// surface a handle (rare; mostly during teardown).
    pub fn parent_window_handle(&self) -> Option<crate::raw_handle::ParentHandle> {
        self.window_ops.as_deref()?.current_parent_handle()
    }

    /// Request a cursor icon change.
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor_request = Some(cursor);
    }

    /// Set a widget subtree as dormant (preserves state, releases rendering).
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::SetDormant(id));
    }

    /// Activate a dormant widget subtree.
    pub fn activate(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::Activate(id));
    }

    /// Destroy a widget subtree (removes from arena entirely, state is gone).
    pub fn destroy(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::Destroy(id));
    }

    /// Show an overlay (tooltip, menu, popover).
    pub fn show_overlay(&mut self, request: crate::overlay::OverlayRequest) {
        self.overlay_requests.push(request);
    }

    /// Show an overlay that dismisses automatically after `duration`.
    pub fn show_overlay_for(
        &mut self,
        request: crate::overlay::OverlayRequest,
        duration: std::time::Duration,
    ) {
        self.timed_overlay_requests.push((request, duration));
    }

    /// Dismiss an overlay by ID.
    pub fn dismiss_overlay(&mut self, id: crate::overlay::OverlayId) {
        self.overlay_dismissals.push(id);
    }

    /// Dismiss all active overlays (e.g., after a menu item is activated).
    pub fn dismiss_all_overlays(&mut self) {
        self.dismiss_all_overlays = true;
    }

    /// Dismiss the topmost overlay only (e.g., closing a submenu while
    /// keeping the parent menu open).
    pub fn dismiss_top_overlay(&mut self) {
        self.dismiss_top = true;
    }

    /// Dismiss descendant overlays of the source widget's containing overlay.
    /// Useful for closing sibling submenu branches while keeping the current
    /// parent menu open.
    pub fn dismiss_child_overlays(&mut self) {
        self.dismiss_descendant_overlays.push(None);
    }

    /// Dismiss descendant overlays of the source widget's containing overlay,
    /// preserving the subtree rooted at `content_id` if it is already open.
    pub fn dismiss_child_overlays_except(&mut self, content_id: crate::widget_id::WidgetId) {
        self.dismiss_descendant_overlays.push(Some(content_id));
    }

    /// Request an idle callback to be run during the next idle period.
    /// Use this for incremental work that takes 5-50ms — too short for a
    /// background thread, too long for a single frame.
    pub fn request_idle_callback(
        &mut self,
        callback: impl FnOnce(crate::idle::IdleDeadline) + 'static,
    ) {
        self.idle_callbacks.push(Box::new(callback));
    }

    /// Request framework-owned modal presentation.
    ///
    /// The widget tree records the request together with the originating
    /// widget, and the application layer can later resolve `Auto` into a
    /// concrete presentation backend.
    pub fn present_modal(&mut self, request: crate::modal::ModalRequest) {
        self.modal_requests.push(request);
    }

    /// Synchronously open a modal as a native window — the single
    /// unified path for native-window modals. Callers that don't
    /// care whether the modal lands in-tree or in a native window
    /// use [`present_modal`](Self::present_modal), which routes
    /// `ModalPresentation::Auto` through the framework's picker.
    ///
    /// Returns the new window's id, or `None` when called outside a
    /// dispatch context (standalone trees). The window's parent is
    /// the current window; focus target and title / size from the
    /// request are honored.
    ///
    /// Only `ModalContent::Deferred` is supported here — an
    /// `ExistingWidget` id wouldn't make sense in a fresh tree.
    pub fn open_modal(
        &mut self,
        request: crate::modal::ModalRequest,
    ) -> Option<crate::window::FernWindowId> {
        let parent = self.current_window.as_ref()?.id();
        let crate::modal::ModalContent::Deferred(builder) = request.content else {
            return None;
        };
        let mut config =
            crate::window::WindowConfig::new().modal(crate::window::ModalConfig {
                parent,
                focus_target: request.focus_target,
            });
        if let Some(title) = request.title {
            config = config.title(title);
        }
        if let Some((w, h)) = request.size {
            config = config.size(w, h);
        }
        let config = config.root(move |tree, _state| builder(tree));
        Some(self.open_window(config))
    }

    /// Dismiss the current framework-owned modal presentation.
    pub fn dismiss_modal(&mut self) {
        self.dismiss_modal = true;
    }

    /// Show an overlay after a delay. The widget tree checks pending delayed
    /// overlays during `layout()` and shows them once the delay elapses.
    /// Use this for submenu hover-open delays.
    ///
    /// The content widget should already be added to the tree (typically
    /// dormant). It will be activated automatically when the delay elapses.
    pub fn show_overlay_after(
        &mut self,
        request: crate::overlay::OverlayRequest,
        delay: std::time::Duration,
    ) {
        self.delayed_overlay_requests.push((request, delay, None));
    }

    /// Show an overlay after a delay and move focus when it opens.
    pub fn show_overlay_after_with_focus(
        &mut self,
        request: crate::overlay::OverlayRequest,
        delay: std::time::Duration,
        focus_target: crate::widget_id::WidgetId,
    ) {
        self.delayed_overlay_requests
            .push((request, delay, Some(focus_target)));
    }

    /// Request a repaint on a specific widget. Use this when an event handler
    /// on one widget changes state that affects a different widget's appearance
    /// (e.g., keyboard navigation highlighting items in an overlay).
    pub fn request_repaint(&mut self, id: crate::widget_id::WidgetId) {
        self.repaint_requests.push(id);
    }

    /// Programmatically click a widget (synthetic PointerDown + PointerUp at
    /// its center). Use this for keyboard activation of a child widget, e.g.,
    /// Enter on a keyboard-focused menu item.
    pub fn synthetic_click(&mut self, id: crate::widget_id::WidgetId) {
        self.synthetic_clicks.push(id);
    }

    /// Transfer focus to a specific widget. Use this when opening overlay
    /// content (menus, dialogs) that should receive keyboard events.
    pub fn request_focus(&mut self, id: crate::widget_id::WidgetId) {
        self.focus_requests.push(id);
    }

    /// Cancel a pending delayed overlay by its content widget ID.
    /// Call this when the hover ends before the delay elapses.
    pub fn cancel_delayed_overlay(&mut self, content_id: crate::widget_id::WidgetId) {
        self.cancel_delayed_overlays.push(content_id);
    }

    /// Capture the pointer: all subsequent `PointerMove` and `PointerUp`
    /// events will be routed to the capturing widget until the capture is
    /// released. Use this when starting a drag operation.
    pub fn capture_pointer(&mut self) {
        self.pointer_capture = Some(true);
    }

    /// Release a previously captured pointer. Pointer events resume normal
    /// hit-test dispatch.
    pub fn release_pointer(&mut self) {
        self.pointer_capture = Some(false);
    }

    /// Start a drag-and-drop operation from the given source widget.
    ///
    /// The `payload` carries the data being dragged. During the drag:
    /// - `PointerMove` events update the drag position and fire `on_drag_hover`
    ///   on widgets under the pointer that have drop handlers
    /// - `PointerUp` fires `on_drop` on the target widget (if any)
    /// - `Escape` cancels the drag
    pub fn start_drag(
        &mut self,
        source_widget: crate::widget_id::WidgetId,
        payload: crate::drag_payload::DragPayload,
    ) {
        self.drag_start_request = Some((source_widget, payload, None));
    }

    /// Start a drag-and-drop with a preview widget that follows the pointer.
    pub fn start_drag_with_preview(
        &mut self,
        source_widget: crate::widget_id::WidgetId,
        payload: crate::drag_payload::DragPayload,
        preview: Box<dyn crate::widget::Widget>,
    ) {
        self.drag_start_request = Some((source_widget, payload, Some(preview)));
    }

    /// Cancel the active drag-and-drop session (if any).
    pub fn cancel_drag(&mut self) {
        self.cancel_drag = true;
    }

    /// Replace the tree-level theme. Composite widgets are rebuilt so any
    /// derived values they captured at build time pick up the new tokens,
    /// and all widgets are marked dirty for repaint.
    pub fn set_theme(&mut self, theme: fern_tokens::Theme) {
        self.theme_request = Some(theme);
    }

    /// Replace the tree-level locale identifier. Composite widgets are
    /// rebuilt so any tr! lookups picked up at build time are re-evaluated
    /// against the new locale.
    pub fn set_locale(&mut self, locale: impl Into<String>) {
        self.locale_request = Some(locale.into());
    }
}

#[cfg(test)]
mod multi_window_tests {
    use super::*;
    use crate::window::state::WindowStateInit;
    use crate::window::{FernWindowId, NoopWindowOps, WindowConfig, WindowOps, WindowState,
        WindowPlacement};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Recording implementation of `WindowOps` so tests can assert
    /// that `EventContext` routes each method through the trait.
    #[derive(Default)]
    struct RecordingOps {
        open_calls: RefCell<Vec<WindowConfig>>,
        focus_calls: RefCell<Vec<FernWindowId>>,
        close_calls: RefCell<Vec<FernWindowId>>,
        next_id: RefCell<u64>,
        // A fake registry so `find_window` / `window_state` / `windows`
        // can return values.
        states: RefCell<Vec<WindowState>>,
    }

    impl RecordingOps {
        fn alloc_id(&self) -> FernWindowId {
            let mut n = self.next_id.borrow_mut();
            *n += 1;
            FernWindowId::new(*n)
        }
    }

    impl WindowOps for RecordingOps {
        fn open_window(&mut self, config: WindowConfig) -> FernWindowId {
            let id = self.alloc_id();
            let state = WindowState::new(WindowStateInit {
                id,
                string_id: config.string_id.clone(),
                placement: config.initial_placement,
                title: config.title.clone(),
                size: config.size,
                position: config.position.unwrap_or((0, 0)),
                focused: true,
                resizable: config.resizable,
                always_on_top: config.always_on_top,
            });
            self.states.borrow_mut().push(state);
            self.open_calls.borrow_mut().push(config);
            id
        }

        fn find_window(&self, string_id: &str) -> Option<FernWindowId> {
            self.states
                .borrow()
                .iter()
                .find(|s| s.string_id() == Some(string_id))
                .map(|s| s.id())
        }

        fn window_state(&self, id: FernWindowId) -> Option<WindowState> {
            self.states
                .borrow()
                .iter()
                .find(|s| s.id() == id)
                .cloned()
        }

        fn windows(&self) -> Vec<WindowState> {
            self.states.borrow().clone()
        }

        fn focus_window(&mut self, id: FernWindowId) {
            self.focus_calls.borrow_mut().push(id);
        }

        fn close_window_by_id(&mut self, id: FernWindowId) {
            self.close_calls.borrow_mut().push(id);
        }
    }

    fn make_state(id: u64, string_id: Option<&str>) -> WindowState {
        WindowState::new(WindowStateInit {
            id: FernWindowId::new(id),
            string_id: string_id.map(String::from),
            placement: WindowPlacement::Floating,
            title: "Test".into(),
            size: (800, 600),
            position: (0, 0),
            focused: true,
            resizable: true,
            always_on_top: false,
        })
    }

    #[test]
    fn window_returns_current_window_state() {
        let state = make_state(1, Some("main"));
        let mut noop = NoopWindowOps;
        let mut ctx = EventContext::new().with_window_context(&mut noop, Some(state.clone()));
        assert_eq!(ctx.window().unwrap().id(), FernWindowId::new(1));
        assert_eq!(ctx.window().unwrap().string_id(), Some("main"));
    }

    #[test]
    fn window_is_none_without_context() {
        let ctx = EventContext::new();
        assert!(ctx.window().is_none());
    }

    #[test]
    fn open_window_routes_through_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, Some("main"));
        let returned_id = {
            let mut ctx = EventContext::new()
                .with_window_context(&mut ops, Some(main_state));
            ctx.open_window(WindowConfig::new().id("help").title("Help"))
        };
        assert_eq!(ops.open_calls.borrow().len(), 1);
        assert_eq!(ops.open_calls.borrow()[0].string_id.as_deref(), Some("help"));
        // Recording ops allocates ids 2+; 1 was reserved for `main`
        // only in this test — Recording's counter starts from 0, so the
        // first alloc yields 1.
        assert_eq!(returned_id, FernWindowId::new(1));
    }

    #[test]
    fn find_window_routes_through_ops() {
        let mut ops = RecordingOps::default();
        ops.states.borrow_mut().push(make_state(7, Some("foo")));
        let main_state = make_state(1, Some("main"));
        let ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
        assert_eq!(ctx.find_window("foo"), Some(FernWindowId::new(7)));
        assert!(ctx.find_window("missing").is_none());
    }

    #[test]
    fn focus_window_records_via_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, None);
        {
            let mut ctx = EventContext::new()
                .with_window_context(&mut ops, Some(main_state));
            ctx.focus_window(FernWindowId::new(42));
        }
        assert_eq!(ops.focus_calls.borrow().as_slice(), &[FernWindowId::new(42)]);
    }

    #[test]
    fn close_window_by_id_records_via_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, None);
        {
            let mut ctx = EventContext::new()
                .with_window_context(&mut ops, Some(main_state));
            ctx.close_window_by_id(FernWindowId::new(9));
        }
        assert_eq!(ops.close_calls.borrow().as_slice(), &[FernWindowId::new(9)]);
    }

    #[test]
    fn windows_enumerates_via_ops() {
        let mut ops = RecordingOps::default();
        ops.states.borrow_mut().push(make_state(1, Some("a")));
        ops.states.borrow_mut().push(make_state(2, Some("b")));
        let main_state = make_state(1, Some("a"));
        let ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
        let ids: Vec<_> = ctx.windows().iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![FernWindowId::new(1), FernWindowId::new(2)]);
    }

    #[test]
    fn standalone_context_returns_empty_windows_and_none_lookups() {
        let ctx = EventContext::new();
        assert!(ctx.find_window("anything").is_none());
        assert!(ctx.window_state(FernWindowId::new(1)).is_none());
        assert!(ctx.windows().is_empty());
    }

    #[test]
    #[should_panic(expected = "open_window called outside of a dispatch")]
    fn open_window_on_standalone_context_panics() {
        let mut ctx = EventContext::new();
        let _ = ctx.open_window(WindowConfig::new());
    }

    #[test]
    fn open_modal_builds_window_config_from_request() {
        use crate::modal::{ModalContent, ModalRequest};
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, Some("main"));
        let built_widget = Rc::new(RefCell::new(false));
        let built_widget_flag = built_widget.clone();
        let request = ModalRequest {
            content: ModalContent::Deferred(Box::new(move |_tree| {
                *built_widget_flag.borrow_mut() = true;
                // Return a dummy WidgetId — not used in this test since
                // the RecordingOps doesn't actually build the tree.
                crate::widget_id::WidgetId::default()
            })),
            presentation: crate::modal::ModalPresentation::NativeWindow,
            close_behavior: crate::modal::ModalCloseBehavior::default(),
            title: Some("Confirm".to_string()),
            size: Some((420, 180)),
            focus_target: None,
            on_dismiss: None,
        };
        {
            let mut ctx = EventContext::new()
                .with_window_context(&mut ops, Some(main_state));
            let id = ctx.open_modal(request);
            assert!(id.is_some());
        }
        // open_modal is a thin wrapper over open_window — the config
        // it built must reach RecordingOps::open_window.
        let calls = ops.open_calls.borrow();
        assert_eq!(calls.len(), 1);
        let cfg = &calls[0];
        assert_eq!(cfg.title, "Confirm");
        assert_eq!(cfg.size, (420, 180));
        assert!(cfg.is_modal());
        assert_eq!(cfg.modal_parent(), Some(FernWindowId::new(1)));
        // Cell is just to let us observe something reachable via cfg.root_builder;
        // the builder hasn't been called yet (RecordingOps records the config
        // but doesn't build the tree).
        let _ = built_widget;
    }

    #[test]
    fn open_modal_requires_current_window() {
        use crate::modal::{ModalContent, ModalRequest};
        let mut ops = RecordingOps::default();
        let mut ctx = EventContext::new().with_window_context(&mut ops, None);
        let request = ModalRequest {
            content: ModalContent::Deferred(Box::new(|_tree| crate::widget_id::WidgetId::default())),
            presentation: crate::modal::ModalPresentation::NativeWindow,
            close_behavior: crate::modal::ModalCloseBehavior::default(),
            title: None,
            size: None,
            focus_target: None,
            on_dismiss: None,
        };
        assert!(ctx.open_modal(request).is_none());
    }
}
