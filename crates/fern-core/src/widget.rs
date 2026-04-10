use fern_canvas::{Canvas, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::event::{EventResponse, WidgetEvent};
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
    /// Request to capture or release the pointer.
    pub(crate) pointer_capture: Option<bool>,
    /// Delayed overlay requests (content_id, request, delay).
    pub(crate) delayed_overlay_requests: Vec<(crate::overlay::OverlayRequest, std::time::Duration)>,
    /// Cancel pending delayed overlays by content widget ID.
    pub(crate) cancel_delayed_overlays: Vec<crate::widget_id::WidgetId>,
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
            pointer_capture: None,
            delayed_overlay_requests: Vec::new(),
            cancel_delayed_overlays: Vec::new(),
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

    /// Dismiss an overlay by ID.
    pub fn dismiss_overlay(&mut self, id: crate::overlay::OverlayId) {
        self.overlay_dismissals.push(id);
    }

    /// Dismiss all active overlays (e.g., after a menu item is activated).
    pub fn dismiss_all_overlays(&mut self) {
        self.dismiss_all_overlays = true;
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
        self.delayed_overlay_requests.push((request, delay));
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
}
