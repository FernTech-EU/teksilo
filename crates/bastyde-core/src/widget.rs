use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::widget_id::WidgetId;

mod cursor;
mod event_context;
mod layout_context;
mod paint_context;

pub use cursor::CursorIcon;
pub use event_context::EventContext;
pub use layout_context::LayoutContext;
pub use paint_context::{PaintContext, WidgetPlacement, WidgetTreeView};

pub(crate) use event_context::{DismissScope, ShortcutMutation, TreeMutation};
pub(crate) use layout_context::LayoutExtras;

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

/// The full Widget trait for Level 2 (custom rendering) widgets.
pub trait Widget: std::fmt::Debug + std::any::Any {
    /// Concrete type name of this widget (e.g.
    /// `"bastyde_widgets::button::Button"`). The default implementation
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

    /// Whether this widget wants its [`after_paint`](Self::after_paint)
    /// hook to fire each frame. Returning `false` (the default) saves
    /// a virtual call per widget per frame for the vast majority of
    /// widgets that don't aggregate descendant geometry.
    ///
    /// Same opt-in pattern as
    /// [`wants_descendant_redirects`](Self::wants_descendant_redirects).
    fn wants_after_paint(&self) -> bool {
        false
    }

    /// Called once per frame after this widget's subtree has finished
    /// painting. Receives a read-only view of the layout-resolved
    /// arena so a parent can read its descendants' final bounds —
    /// e.g. `TitleBar` aggregates its drag region and control-button
    /// rects into a single `HitRegions` payload for the Windows
    /// backend's `WM_NCHITTEST`.
    ///
    /// Walk order is depth-first **post**-order: a parent's
    /// `after_paint` runs after every descendant's `paint` has
    /// committed.
    ///
    /// Default: empty. Only widgets that override
    /// [`wants_after_paint`](Self::wants_after_paint) and return `true`
    /// see this called.
    fn after_paint(&self, _view: &WidgetTreeView<'_>, _ctx: &PaintContext) {}

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
    /// `accessibility()` emission — `bastyde_scene::SceneView` is
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
    /// Used by `bastyde_scene::SceneView` to graft heavyweight
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

    /// Declare the rebindable keyboard shortcuts this widget exposes,
    /// *without* installing handlers. The framework calls this at
    /// arena insertion time (before `build()`) and at certain lazy
    /// boundaries (e.g. `Switcher` walks declarations on its
    /// not-yet-mounted `Pending` slots), so settings UIs and the
    /// `ShortcutRegistry` see the keystrokes the moment the owning
    /// container mounts — even if `build()` hasn't run.
    ///
    /// Pair this with `BuildContext::register_shortcut` in `build()`
    /// to install the matching `on_activate` handler: the build-time
    /// registration *upserts* the declared entry, preserving any user
    /// override and the declared keystrokes while attaching the
    /// closure that actually fires.
    ///
    /// The returned shortcuts may omit `on_activate` (a metadata-only
    /// declaration). When matched at dispatch time without a
    /// registered handler, the framework synthesizes a no-parameter
    /// intent from the shortcut's id — same path as a build-time
    /// registration with `on_activate: None`.
    ///
    /// Default: empty (no declared shortcuts).
    fn declare_shortcuts(&self) -> Vec<crate::shortcut::Shortcut> {
        Vec::new()
    }

    /// Extract attached handler set from a `WidgetWithHandlers` wrapper.
    /// Called during arena insertion to transfer handlers to the `WidgetNode`.
    /// Default: returns `None` (no attached handlers).
    fn take_handler_set(&mut self) -> Option<crate::widget_builder::HandlerSet> {
        None
    }
}
