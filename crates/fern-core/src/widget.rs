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

/// Context available during layout.
pub struct LayoutContext<'a> {
    pub theme: &'a fern_tokens::Theme,
    pub layout_direction: crate::environment::LayoutDirection,
    /// Text backend for accurate text measurement during layout.
    pub text_backend: Option<&'a std::rc::Rc<std::cell::RefCell<dyn fern_canvas::TextBackend>>>,
    /// Arena reference for querying child widget sizes.
    pub(crate) arena: Option<&'a crate::arena::WidgetArena>,
}

impl<'a> LayoutContext<'a> {
    /// Create a LayoutContext for testing (no arena access).
    pub fn for_testing(theme: &'a fern_tokens::Theme) -> Self {
        Self {
            theme,
            layout_direction: crate::environment::LayoutDirection::LeftToRight,
            text_backend: None,
            arena: None,
        }
    }

    /// Query a child widget's preferred size for a given proposal.
    /// Returns None if the child doesn't exist, is dormant, or the arena is not available.
    pub fn child_size(
        &self,
        child_id: WidgetId,
        proposal: fern_canvas::SizeProposal,
    ) -> Option<fern_canvas::Size> {
        let arena = self.arena?;
        if !arena.is_active(child_id) {
            return None;
        }
        let node = arena.get(child_id)?;
        Some(node.widget.size_that_fits(proposal, self))
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

    /// Query whether a child widget is a spacer.
    /// Returns false for dormant children.
    pub fn child_is_spacer(&self, child_id: WidgetId) -> bool {
        self.arena
            .filter(|arena| arena.is_active(child_id))
            .and_then(|arena| arena.get(child_id))
            .is_some_and(|node| node.widget.is_spacer())
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

    /// Respond to the parent's size proposal with the size this widget wants.
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size;

    /// Position children within the allocated bounds.
    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Leaf widgets have no children to place.
    }

    /// Draw the widget's visual representation.
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {
        // Default: nothing to paint (layout-only containers).
    }

    /// Declare this widget's accessibility identity.
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

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

    /// Whether this widget is a flexible spacer (claims remaining space in stacks).
    fn is_spacer(&self) -> bool {
        false
    }

    /// Whether this widget clips its children to its bounds.
    fn clips_children(&self) -> bool {
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
pub struct EventContext {
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
    /// Intents queued by handlers via `send_intent`. Drained by the
    /// tree after event dispatch and routed source-widget → root.
    pub(crate) pending_intents: Vec<crate::intent::Intent>,
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

impl EventContext {
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
            pending_key_capture: None,
            cancel_key_capture: false,
            pending_shortcut_mutations: Vec::new(),
            close_window_requested: false,
        }
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
    pub fn send_intent(&mut self, intent: impl Into<crate::intent::Intent>) {
        self.pending_intents.push(intent.into());
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
