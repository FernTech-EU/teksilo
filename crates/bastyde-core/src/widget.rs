// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::widget_id::WidgetId;

mod cursor;
mod event_context;
mod layout_context;
mod paint_context;

pub use cursor::CursorIcon;
pub use event_context::EventContext;
pub use layout_context::{LayoutContext, StackAxis};
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
/// Carries four quantities that together describe how the widget participates
/// in a stack's space distribution along the layout's main axis:
///
/// - `size` — the widget's wanted/ideal size; the floor for **growth**.
/// - `flex` — positive-slack **grow** weight. `0.0` = rigid (no claim on
///   surplus); `> 0.0` = wants a share of surplus proportional to its weight.
/// - `min` — the hard floor for **compression**. A parent must never shrink
///   the widget below this. Defaults to `size` (i.e. "I do not shrink").
/// - `shrink` — negative-slack **shrink** weight. `0.0` = will not compress
///   (the widget overflows before it shrinks); `> 0.0` = absorbs a share of an
///   over-constraint deficit proportional to its weight, down to `min`.
///
/// `flex` and `shrink` are independent — as in CSS flexbox (`flex-grow` vs
/// `flex-shrink`), a widget may grow but not shrink, or shrink but not grow.
/// Most widgets just return a `Size`; the `From<Size>` impl wraps it as fully
/// rigid (`flex = 0`, `shrink = 0`, `min = size`). Grow-bearing widgets
/// (`Spacer`, `Expand`) use [`LayoutResponse::flexible`]; shrink-bearing ones
/// (`Shrinkable`, single-line `TextWidget`) use [`LayoutResponse::shrinkable`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutResponse {
    pub size: Size,
    pub flex: f32,
    pub min: Size,
    pub shrink: f32,
}

impl LayoutResponse {
    pub const ZERO: Self = Self {
        size: Size::ZERO,
        flex: 0.0,
        min: Size::ZERO,
        shrink: 0.0,
    };

    /// A fully rigid response: wanted `size`, no growth, no compression
    /// (`min == size`).
    pub fn rigid(size: Size) -> Self {
        Self {
            size,
            flex: 0.0,
            min: size,
            shrink: 0.0,
        }
    }

    /// A grow-bearing response: wanted `size` with grow weight `flex`. Does
    /// not shrink (`min == size`).
    pub fn flexible(size: Size, flex: f32) -> Self {
        Self {
            size,
            flex: flex.max(0.0),
            min: size,
            shrink: 0.0,
        }
    }

    /// A shrink-bearing response: wanted `size`, compressible down to `min`
    /// with shrink weight `shrink`. Does not grow (`flex == 0`). `min` is
    /// clamped componentwise to be no larger than `size`.
    pub fn shrinkable(size: Size, min: Size, shrink: f32) -> Self {
        Self {
            size,
            flex: 0.0,
            min: Size::new(min.width.min(size.width), min.height.min(size.height)),
            shrink: shrink.max(0.0),
        }
    }

    /// Builder: set the grow weight on an existing response.
    pub fn with_flex(mut self, flex: f32) -> Self {
        self.flex = flex.max(0.0);
        self
    }

    /// Builder: set the shrink weight on an existing response.
    pub fn with_shrink(mut self, shrink: f32) -> Self {
        self.shrink = shrink.max(0.0);
        self
    }

    /// Builder: set the compression floor (clamped componentwise ≤ `size`).
    pub fn with_min(mut self, min: Size) -> Self {
        self.min = Size::new(
            min.width.min(self.size.width),
            min.height.min(self.size.height),
        );
        self
    }
}

impl From<Size> for LayoutResponse {
    fn from(size: Size) -> Self {
        Self {
            size,
            flex: 0.0,
            min: size,
            shrink: 0.0,
        }
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
    /// for the `widget.census` telemetry event. Custom
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

    /// Respond to the parent's size proposal with this widget's wanted size,
    /// grow/shrink weights, and compression floor (see [`LayoutResponse`]).
    ///
    /// Most widgets just return a `Size` (auto-converts via `From<Size>` to a
    /// fully rigid response). Grow-bearing widgets (`Spacer`, `Expand`) return
    /// a non-zero `flex`; shrink-bearing widgets (`Shrinkable`, single-line
    /// `TextWidget`) return a non-zero `shrink` with a `min` floor.
    ///
    /// The parent honors `size` as a floor for growth and distributes positive
    /// slack proportional to `flex`; when over-constrained it distributes the
    /// deficit proportional to `shrink`, never below `min`.
    ///
    /// **Determinism / height-for-width contract.** This must be a *deterministic
    /// function of the widget's state and the `proposal`*: two calls with the
    /// same proposal in one layout pass must return the same value. The result
    /// must be correct *for the proposal given* — in particular a
    /// height-for-width widget queried with `{width: Some(w), height: None}`
    /// must return its height *at width `w`*. The framework memoizes results per
    /// `(widget, proposal)` within a pass (see
    /// [`cacheable_layout`](Self::cacheable_layout)) to keep negotiation O(n).
    ///
    /// Side effects are permitted **as long as they are idempotent** — the cache
    /// may skip them on a repeat query, so a side effect (e.g. snapshotting
    /// measured state into a `Signal`) must be safe to run any number of times
    /// ≥ 1 per pass and leave the same final state. Most measuring widgets
    /// (`Collapse`, `SceneView`, inspector tabs) satisfy this with a guarded or
    /// overwriting set. A side effect that must run on *every* call (e.g. a call
    /// counter) is non-idempotent — opt out via `cacheable_layout`.
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse;

    /// Whether this widget's [`layout_response`](Self::layout_response) may be
    /// memoized by the per-pass layout cache. Defaults to `true`.
    ///
    /// Override to `false` only when `layout_response` has a **non-idempotent**
    /// side effect that must run on every call (a call counter, a one-shot
    /// trigger). Idempotent side effects — the common case, e.g. overwriting a
    /// measured size into a `Signal` — do **not** need to opt out: the cache may
    /// skip a redundant identical-proposal repeat, but the value was already
    /// written and the final state is correct. The debug inspector's
    /// `BoundsTracker` opts out defensively (its whole-tree snapshot is the
    /// payload, not a by-product of sizing).
    fn cacheable_layout(&self) -> bool {
        true
    }

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

    /// Whether this widget wants its [`post_paint`](Self::post_paint)
    /// hook to fire each frame. Returning `false` (the default) saves a
    /// virtual call per widget per frame for the vast majority of widgets
    /// that don't draw a foreground over their children.
    ///
    /// Same opt-in pattern as [`wants_after_paint`](Self::wants_after_paint).
    fn wants_post_paint(&self) -> bool {
        false
    }

    /// Draw a foreground layer *over* this widget's children.
    ///
    /// The normal [`paint`](Self::paint) emits a widget's draws *before*
    /// its children — a backdrop. `post_paint` emits *after* the entire
    /// child subtree, so its draws land on top. This is the supported way
    /// for a composing widget to paint over its own descendants:
    /// inset shadows, a focus ring that must overlay content, a scrim, or
    /// `SceneView`'s "over" lightweight band (selection lasso, highlighted
    /// connectors).
    ///
    /// Runs inside the same clip / transform / opacity / blur scopes as
    /// the widget and its children, so a foreground decoration pans,
    /// scales and clips consistently with the subtree it covers. It is
    /// paint-only — no hit-testing and no accessibility node; for
    /// interactive overlays that must escape the widget's bounds, use the
    /// overlay system instead.
    ///
    /// Default: empty. Only widgets that override
    /// [`wants_post_paint`](Self::wants_post_paint) and return `true` see
    /// this called.
    fn post_paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}

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
    /// `Widget` items into an app-declared logical AT tree.
    /// Other layered containers can adopt the same pattern.
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

    /// Optional override for the child ORDER presented to assistive
    /// technology, when it must differ from the paint / z-order child
    /// order returned by [`children`](Self::children).
    ///
    /// Return `None` (the default) to let the accessibility walker use the
    /// arena's child order — correct for almost every widget. Return
    /// `Some(ids)` to reorder (or restrict) how children appear in the AT
    /// tree and in the linear Tab reading order, WITHOUT affecting layout or
    /// paint. `TableView` / `TreeTableView` use this to read the header
    /// before the body rows even though they build the body first so it
    /// paints beneath the header (WCAG 1.3.2 Meaningful Sequence).
    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        None
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

    /// The rectangle (in **absolute tree coordinates**) that best *represents*
    /// this widget when the framework reveals it into an ancestor scroll area on
    /// focus gain. Returning `None` (the default) reveals the widget's whole
    /// bounds — correct for most controls.
    ///
    /// A widget that can be much taller than a viewport — a `RichTextEditor`
    /// grown inside a page `ScrollArea`, a `ListView` / `TreeView` taller than
    /// its scroller — should override this to return the sub-rectangle the user
    /// actually cares about (the caret line, the selected row). Otherwise
    /// [`scroll_focused_into_view`](crate::widget_tree::WidgetTree) reveals the
    /// *entire* box, which for a tall widget scrolls the page to its bottom on a
    /// click that only meant to place the caret near the top. The returned rect
    /// feeds the same ancestor-only `scroll_rect_into_view` engine the caret /
    /// selection follow uses (the focused widget itself is excluded), so it
    /// never double-scrolls against the widget's own internal follow.
    ///
    /// `bounds` is this widget's current absolute rectangle as the arena stores
    /// it, so an override can place its interior rect without depending on a
    /// paint-set origin.
    fn focus_reveal_rect(&self, _bounds: Rect) -> Option<Rect> {
        None
    }

    /// Whether the point lies inside this widget's *actual* shape, not
    /// just its rectangular bounds. Consulted by hit-testing right after
    /// the bounds check: returning `false` for a point that *is* inside
    /// the bounding box makes the widget transparent to the click there,
    /// so it **falls through** to whatever sibling is painted underneath
    /// (the same machinery as a fully pass-through node, but shape-aware).
    ///
    /// Both arguments are in the widget's bounds space: `local_point` is
    /// the point being tested and `bounds` is this widget's rectangle, so
    /// a non-rectangular widget can test the point against its silhouette.
    ///
    /// The default returns `true` for any in-bounds point (a plain
    /// rectangle), so every existing widget is unaffected. Override for
    /// irregular shapes — an ellipse / cloud scene node, a circular
    /// handle — so a click lands on the shape you see, not its bounding
    /// box, and clicks in the transparent corners reach the node beneath.
    /// This mirrors the lightweight tier's `SceneItem::shape_contains`.
    fn hit_shape(&self, _local_point: Point, _bounds: Rect) -> bool {
        true
    }

    /// How `rebuild_single_widget` treats this widget's existing children
    /// when re-running its `build()`.
    ///
    /// **`false` (default) — re-derive.** Rebuild is "tear down and
    /// reconstruct": every old child subtree is destroyed up front, then
    /// `build()` produces a fresh set. The right semantic for data-driven
    /// widgets like `Repeater` / `ListView` that rebuild their children from
    /// current model state with fresh `WidgetId`s. A `false` widget must NOT
    /// re-attach an old child id — it has already been destroyed.
    ///
    /// **`true` — reconcile.** `build()` re-attaches (by id) the children it
    /// keeps and drops the rest. The framework keeps every re-attached child's
    /// subtree intact — focus, scroll offset, text contents, signal
    /// subscriptions all survive — and destroys only the old children the new
    /// build dropped *and* did not re-parent elsewhere. This is the mode for
    /// widgets that memoize stateful children across rebuilds:
    ///
    /// * `Switcher` keeps every mounted page alive so switching tabs doesn't
    ///   wipe the inactive pages' state.
    /// * `SceneView` re-pushes the same heavyweight scene-widget ids each
    ///   rebuild (draining drag-to-move / marquee commits) — they must stay
    ///   attached or the cards "disappear" on every drag end.
    /// * `TabWidget` / `DockingLayout` / `CompositeTooltip` re-attach memoized
    ///   panes / a one-shot body widget that cannot be reconstructed.
    /// * `MenuBar` re-derives its menu triggers fresh each build (the model may
    ///   have changed — the reconcile reaps the superseded ones) while keeping
    ///   its memoized leading/trailing slot widgets, so a stateful slot control
    ///   survives a model-version rebuild.
    ///
    /// The reconcile follows **authoritative parent pointers**, so a kept
    /// subtree that `build()` re-parents *out* of a dropped sibling and into
    /// the new tree survives — it is not swept via the dropped sibling's now
    /// stale `children` list. Dropped children are genuinely destroyed (state
    /// unmounted, arena slots freed), not left as stranded, still-active
    /// orphans.
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
