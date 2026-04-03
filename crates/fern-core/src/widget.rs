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
    Deferred(Box<dyn IntoWidgetTree>),
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
    /// Returns None if the child doesn't exist or the arena is not available.
    pub fn child_size(
        &self,
        child_id: WidgetId,
        proposal: fern_canvas::SizeProposal,
    ) -> Option<fern_canvas::Size> {
        let arena = self.arena?;
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
    pub fn child_is_spacer(&self, child_id: WidgetId) -> bool {
        self.arena
            .and_then(|arena| arena.get(child_id))
            .map_or(false, |node| node.widget.is_spacer())
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

    /// Handle an event during the bubble (target → root) pass.
    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    /// Handle an event during the preview (root → target) pass.
    fn preview_event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    /// Declare this widget's accessibility identity.
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}

    /// Whether this widget can receive keyboard focus.
    fn is_focusable(&self) -> bool {
        false
    }

    /// Optional tab index override (default: tree order).
    fn tab_index(&self) -> Option<i32> {
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

    /// Whether this widget clips its children to its bounds. When true, the
    /// framework sets `clips_children` on the arena node automatically,
    /// enabling viewport clipping in the renderer and `ScrollIntoView`
    /// dispatch. Override this in scroll containers and similar widgets.
    fn clips_children(&self) -> bool {
        false
    }

    /// Whether this widget is a composite adapter that needs rebuild on
    /// environment changes (theme switch, locale switch).
    fn is_composite(&self) -> bool {
        false
    }

    /// Downcast to `&mut dyn Any` for type-specific operations (e.g. composite rebuild).
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        // This default implementation won't work for all types because of object safety.
        // Concrete types that need downcasting override this.
        panic!("as_any_mut not implemented for this widget")
    }

    /// Register any reactive property bindings for dirty tracking.
    /// Called automatically by `BuildContext::add()` after the widget
    /// receives its WidgetId. Widgets with `Reactive::Bound` fields
    /// override this to register their bindings.
    fn register_bindings(
        &self,
        _id: WidgetId,
        _registry: &crate::state::BindingRegistry,
    ) {
        // Default: no reactive bindings.
    }

    /// Return any `State<f32>` values that may be animated via `set_animated`.
    /// Called automatically during widget insertion to register them with the
    /// animation scheduler. Override this if your widget uses `set_animated`
    /// on internally owned states.
    fn animated_states(&self) -> Vec<crate::state::State<f32>> {
        Vec::new()
    }

    /// Take any deferred children out of this widget for resolution.
    /// Called by `WidgetTree::add_widget_direct()` before inserting the widget.
    /// Containers override this to drain their pending children list.
    fn take_pending_children(&mut self) -> Vec<PendingChild> {
        Vec::new()
    }

    /// Wire resolved child IDs back into the widget after pending children
    /// have been inserted into the arena. Containers override this to set
    /// their internal `child_ids` field.
    fn set_resolved_children(&mut self, _ids: Vec<WidgetId>) {
        // Default: no children to wire.
    }

    /// Take a deferred `visible_when` binding stored by the builder pattern.
    /// Called after insertion to register with the tree.
    fn take_visible_when(&mut self) -> Option<crate::state::Reactive<bool>> {
        None
    }

    /// Take a deferred `enabled_when` binding stored by the builder pattern.
    /// Called after insertion to register with the tree.
    fn take_enabled_when(&mut self) -> Option<crate::state::Reactive<bool>> {
        None
    }

}

/// Trait for anything that can be added to a WidgetTree via `add_widget()`.
/// Blanket-implemented for all `Widget` types. Composite widgets use the
/// `impl_composite_into_widget_tree!` macro to generate the implementation.
pub trait IntoWidgetTree: 'static {
    fn register(self: Box<Self>, tree: &mut crate::widget_tree::WidgetTree) -> WidgetId;
}

impl<W: Widget + 'static> IntoWidgetTree for W {
    fn register(self: Box<Self>, tree: &mut crate::widget_tree::WidgetTree) -> WidgetId {
        tree.add_widget_direct(self)
    }
}

/// Implement `IntoWidgetTree` for a `CompositeWidget` type, routing it
/// through the composite build path. Use this instead of writing the
/// boilerplate manually.
///
/// ```ignore
/// impl_composite_into_widget_tree!(MyWidget);
/// ```
#[macro_export]
macro_rules! impl_composite_into_widget_tree {
    ($t:ty) => {
        impl $crate::widget::IntoWidgetTree for $t {
            fn register(
                self: Box<Self>,
                tree: &mut $crate::widget_tree::WidgetTree,
            ) -> $crate::widget_id::WidgetId {
                tree.add_composite_inner(self)
            }
        }
    };
}

/// Context available during event handling.
pub struct EventContext {
    pub(crate) commands: Vec<crate::app_command::ErasedCommand>,
    pub(crate) cursor_request: Option<CursorIcon>,
    pub(crate) tree_mutations: Vec<TreeMutation>,
    pub(crate) idle_callbacks: Vec<crate::idle::IdleCallback>,
    pub(crate) overlay_requests: Vec<crate::overlay::OverlayRequest>,
    pub(crate) overlay_dismissals: Vec<crate::overlay::OverlayId>,
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

    /// Request an idle callback to be run during the next idle period.
    /// Use this for incremental work that takes 5-50ms — too short for a
    /// background thread, too long for a single frame.
    pub fn request_idle_callback(
        &mut self,
        callback: impl FnOnce(crate::idle::IdleDeadline) + 'static,
    ) {
        self.idle_callbacks.push(Box::new(callback));
    }
}
