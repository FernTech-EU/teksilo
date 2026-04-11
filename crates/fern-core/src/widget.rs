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
    pub(crate) commands: Vec<crate::app_command::ErasedCommand>,
    pub(crate) cursor_request: Option<CursorIcon>,
    pub(crate) tree_mutations: Vec<TreeMutation>,
    pub(crate) idle_callbacks: Vec<crate::idle::IdleCallback>,
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
            commands: Vec::new(),
            cursor_request: None,
            tree_mutations: Vec::new(),
            idle_callbacks: Vec::new(),
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
        }
    }

    /// Emit a typed application command.
    pub fn emit<C: crate::app_command::AppCommand>(&mut self, cmd: C) {
        self.commands
            .push(crate::app_command::ErasedCommand::new(cmd));
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
}
