// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use crate::accessibility::AccessNodeBuilder;
use crate::widget_id::WidgetId;

mod cursor;
mod event_context;
mod layout_context;
mod paint_context;

pub use cursor::CursorIcon;
pub use event_context::EventContext;
pub(crate) use event_context::{CursorRequest, GestureAct};
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
///
/// # Adding a method here
///
/// A defaulted method added to this trait is **inert for every widget a builder
/// method has touched** until the framework's same-node wrappers forward it.
/// [`WidgetWithHandlers`](crate::widget_builder::WidgetWithHandlers) and the
/// `TeksiBranch{,3,4}` sum types replace the widget at its own arena node, so an
/// unforwarded method answers this default and the widget loses the behaviour
/// silently — no compile error, and no test that drives the hook on a bare
/// struct can see it. Those impls deny `clippy::missing_trait_methods` so the
/// omission surfaces as a lint on them rather than as a defect in an app.
pub trait Widget: std::fmt::Debug + std::any::Any {
    /// Concrete type name of this widget (e.g.
    /// `"teksilo_widgets::button::Button"`). The default implementation
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
    /// arena, and again whenever the widget is marked for rebuild — a
    /// `BindingLevel::Rebuild` binding, `rebuild_single_widget`, or a density
    /// switch. A theme or locale switch does **not** rebuild: `set_theme` and
    /// `set_locale` mark layout + paint dirty and leave the built subtree
    /// standing, so anything derived from the theme or the locale must be read
    /// per pass or bound to a signal rather than captured here.
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
    ///
    /// Called for **every active widget on every layout pass** — including
    /// leaves, which receive an empty `children` slice. It is therefore also the
    /// canonical *"here are your final, parent-assigned bounds"* hook, and the
    /// only one that runs during layout: `layout_response` sees a *proposal*, not
    /// the outcome, and `paint` runs too late for anything the renderer consumes
    /// before it (a node-level transform scope, a text engine's viewport).
    ///
    /// A widget whose bounds feed such a thing must read them here, not in
    /// `paint`. `SceneView` is the motivating case: it folds `bounds.origin` into
    /// the view transform the render walker pushes *around* its subtree, so a
    /// scene that learned its origin only at paint time drew its content offset
    /// by `-bounds.origin` — an error that then scaled with zoom.
    ///
    /// **If you mutate interior state here, keep the write and its consequences
    /// together.** This hook runs *before* `paint`, so writing a field that
    /// `paint` later uses as a compare-then-act change detector will silently
    /// blind that detector. (Both text engines were bitten: `place_children` set
    /// the `viewport_width` that `paint` compared against, so `paint` concluded
    /// "unchanged" and skipped the `engine.set_viewport` + relayout it owed.)
    /// Route such writes through one idempotent helper that both hooks call.
    ///
    /// **Do not `Signal::set` here at [`BindingLevel::Relayout`] or
    /// [`Rebuild`](crate::binding::BindingLevel::Rebuild).** This runs inside the
    /// layout pass, so dirtying the tree from it re-enters layout. A plain
    /// `Cell`/`RefCell` (what the colour-picker leaves use to cache their bounds
    /// for hit-testing) or a `RepaintOnly` signal is fine.
    ///
    /// [`BindingLevel::Relayout`]: crate::binding::BindingLevel::Relayout
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
    /// `accessibility()` emission — `teksilo_scene::SceneView` is
    /// the canonical example.
    ///
    /// Default: `false`.
    fn wants_descendant_redirects(&self) -> bool {
        false
    }

    /// Whether this widget decides, on every layout pass, which of its
    /// children exist at all.
    ///
    /// Opting in changes what [`place_children`](Self::place_children)
    /// receives and what the framework does with it:
    ///
    /// * the `children` slice carries **every** child, dormant ones included,
    ///   rather than only the active ones — otherwise a container could never
    ///   ask for a child back, having parked it;
    /// * each [`WidgetPlacement::dormant`] arrives pre-set to that child's
    ///   current state, and whatever the widget leaves there is applied: a
    ///   child newly cleared is woken and laid out **in the same pass**, a
    ///   child newly set is parked after the pass, with focus revalidated
    ///   behind it.
    ///
    /// The cost of opting in is one iteration per child per pass, which is why
    /// it is a choice rather than the rule: an ordinary container pays for its
    /// active children only.
    ///
    /// This exists for containers that hold far more content than they show —
    /// a scene viewport, a canvas, a map — where the off-screen half is not
    /// merely invisible but should not be *reachable*: a card 90 000 px away
    /// is a Tab stop between two visible ones and a node an assistive client
    /// is offered. Collapsing its `size` to zero answers neither, because a
    /// zero-size widget is still alive.
    ///
    /// A container that only ever hides one branch at a time wants
    /// [`BuildContext::visible_when`](crate::build_context::BuildContext::visible_when)
    /// instead: a gate per branch is cheaper than a pass per child, and that
    /// is the shape of `Switcher`, a popover or a collapsed panel.
    ///
    /// Default: `false`.
    fn culls_children(&self) -> bool {
        false
    }

    /// Optional redirection hook for AT-tree placement of a child.
    ///
    /// The accessibility walker consults every ancestor that opts
    /// in via
    /// [`wants_descendant_redirects`](Self::wants_descendant_redirects),
    /// starting at the child's immediate arena parent and walking
    /// up to the root. An ancestor whose flag is `false` is
    /// skipped without its hook ever being called, the immediate
    /// parent included, and the walk carries on past it. First
    /// `Some(_)` wins, scanned bottom-up (closest opted-in
    /// ancestor takes priority). Returning
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
    /// Used by `teksilo_scene::SceneView` to graft heavyweight
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

    /// The widget that *paints* this one's title, when it has one.
    ///
    /// Preferred over [`accessible_title_hint`](Self::accessible_title_hint)
    /// where both are available: an enclosing container points at the
    /// title node through a `labelled_by` relation instead of copying
    /// its string, so the title keeps its own node and stays reviewable
    /// by character rather than being announced only as part of the
    /// container's name.
    ///
    /// Queried right after the content is mounted — `build()` runs
    /// eagerly on insertion, so the title node already exists by then.
    ///
    /// Default: `None`.
    fn accessible_title_node(&self) -> Option<crate::widget_id::WidgetId> {
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

    /// Which descendant a keyboard request for a context menu should target.
    ///
    /// The context-menu key (and Shift+F10) opens the menu of the **focused**
    /// widget. For a data view that is the wrong node: `ListView`, `TreeView`,
    /// `TableView`, `TreeTableView` and `GridView` are focusable as a whole and
    /// their rows deliberately are not — the container owns focus and
    /// `set_selected` is what tells assistive technology which row is current
    /// (see `list_item_a11y`). Without this hook the chord would open the
    /// *list's* menu rather than the selected row's, in exactly the widget
    /// family where a per-row menu matters most.
    ///
    /// Return the widget id of the row (or cell, or tile) the menu should be
    /// about. The dispatcher then walks up from there, so a view whose rows
    /// carry no factory of their own still finds the container's.
    ///
    /// Default: `None` — the focused widget is the target, which is right for
    /// every widget that is itself the thing the user is pointing at.
    fn context_menu_key_target(&self) -> Option<WidgetId> {
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
    /// The lightweight tier answers the same question from a *value* —
    /// `SceneItem::shape` returns an `ItemShape` that every query in
    /// `teksilo-scene` derives from — rather than from a predicate. The two
    /// are deliberately not the same shape of API and deliberately not the
    /// same name: a widget is hit-tested by the arena, one point at a time,
    /// with its bounds already in hand.
    fn hit_shape(&self, _local_point: Point, _bounds: Rect) -> bool {
        true
    }

    /// Veto a hit on one of this widget's **direct children**, per point.
    ///
    /// Called during the arena's reverse-sibling walk, once per child, before
    /// the recursion descends into it. Returning `false` makes the walk fall
    /// through to the next sibling exactly as a [`hit_shape`](Self::hit_shape)
    /// rejection on that child would, so the point can land on a lower sibling
    /// or, finally, on this widget itself.
    ///
    /// `point` is in the space this widget's
    /// [`place_children`](Self::place_children) writes — for an ordinary node
    /// that is the same absolute space its own bounds are in; for a
    /// content-transform node (a `SceneView`) it is content coordinates, the
    /// point already mapped through the inverse of the content transform.
    ///
    /// # What this is for
    ///
    /// A widget that owns a **second picking system over the same area** — a
    /// `SceneView`'s lightweight items, a chart's overlay marks, a terminal's
    /// link layer. Without it the two systems answer independently: the widget
    /// can paper over the disagreement inside one handler, but press feedback,
    /// focus-on-release, the touch hold route, the cursor and drag all resolve
    /// from the arena's answer, so five of the six stay wrong.
    ///
    /// It is **not** a replacement for
    /// [`hit_transparent`](crate::widget_builder::HandlerSet::hit_transparent),
    /// which is the right tool for a decorative overlay: that is a per-*node*
    /// declaration ("I never absorb a press"), set from the outside by whoever
    /// builds the node, and it cannot express "reject this child here and
    /// accept it one pixel over". This is the per-*point* question, answered by
    /// the parent that knows why.
    ///
    /// # Contract
    ///
    /// Must be **pure and cheap**: it runs once per child per hit test, and a
    /// hit test runs on every pointer sample. It must not mutate anything the
    /// walk can observe, and — because it runs inside the router — it must not
    /// re-enter any `RefCell` a handler might already hold. A widget that
    /// answers from a per-layout snapshot should memoise per walk; the default
    /// returns `true`, so a widget that does not override it pays one
    /// devirtualizable call per child.
    fn accepts_child_hit(&self, _child: WidgetId, _point: Point) -> bool {
        true
    }

    /// How far **outside** its own bounds this widget still absorbs a press,
    /// per edge, for the pointer that is asking.
    ///
    /// Consulted **inside** the exact hit test: within one parent, children
    /// that declare an outset are tested against their outset bounds *before*
    /// the ordinary reverse-sibling walk, so a 6 dp splitter gutter wins over
    /// the panes it overlaps instead of losing to whichever pane is painted on
    /// top. This is the mechanism for a thin **grip**, and the only one of the
    /// three that can beat a competing target.
    ///
    /// Three rules make it safe to add to a widget that already works:
    ///
    /// * **Hit-only.** No layout moves, nothing repaints differently, and a
    ///   Compact build renders byte for byte as it did. The outset exists
    ///   between the pointer and the arena and nowhere else.
    /// * **It never escapes the parent.** The recursion has already tested the
    ///   parent's own bounds before it looks at any child, so an outset can
    ///   only ever claim space the parent already owns — including through a
    ///   `clips_children` ancestor, whose rectangle gated the descent.
    /// * **Zero for a precise pointer** unless the widget deliberately says
    ///   otherwise. A mouse cursor's hot-spot is exact and occludes nothing, so
    ///   widening its targets steals clicks. Check
    ///   `kind.is_direct()` (or accept every kind explicitly, as a control with
    ///   a genuinely undersized mouse grip may) before returning anything
    ///   non-zero.
    ///
    /// The insets are **reading-order**: `leading` is the left edge in an LTR
    /// UI and the right edge in an RTL one. The default returns
    /// [`EdgeInsets::ZERO`](teksilo_canvas::EdgeInsets::ZERO), so every existing
    /// widget is unaffected.
    ///
    /// A grip's conventional value is `9 dp` for a direct pointer and `0 dp`
    /// for a precise one — enough to lift a 6 dp gutter to a 24 dp target.
    fn hit_outset(
        &self,
        _kind: teksilo_tokens::PointerKind,
        _tokens: &teksilo_tokens::InputTokens,
    ) -> teksilo_canvas::EdgeInsets {
        teksilo_canvas::EdgeInsets::ZERO
    }

    /// This widget's own say in the *miss-only* slop pass, overriding the
    /// density default for its node.
    ///
    /// Third link of the precedence chain — `no_hit_slop` beats a node-level
    /// `.hit_slop(..)`, which beats this, which beats
    /// [`HitSlop::for_pointer`]. Return [`HitSlop::NONE`] to opt a widget out
    /// of re-attribution entirely, or a larger `up_to` to say that a control
    /// deserves topping up further than the density asks.
    ///
    /// `None` (the default) means "no opinion — use the density default".
    ///
    /// [`HitSlop::for_pointer`]: crate::pointer::hit_slop::HitSlop::for_pointer
    /// [`HitSlop::NONE`]: crate::pointer::hit_slop::HitSlop::NONE
    fn hit_slop(
        &self,
        _kind: teksilo_tokens::PointerKind,
        _tokens: &teksilo_tokens::InputTokens,
    ) -> Option<crate::pointer::hit_slop::HitSlop> {
        None
    }

    /// How far a *missed* press is from this widget's actual silhouette, in the
    /// widget's own bounds space.
    ///
    /// The key to the miss-only slop pass: once the exact pass has found
    /// nothing eligible, the framework asks every nearby node how far away it
    /// really is and re-attributes the press to the closest one still inside
    /// its earned outset.
    ///
    /// The default measures to the bounding rectangle, which is right for the
    /// rectangular majority. A **round or wedge** control overrides it beside
    /// its existing [`hit_shape`](Self::hit_shape) so the slop follows the
    /// shape the user aimed at rather than the box it was laid out in — a
    /// press past the corner of a radio dot's box is further from the dot than
    /// a press past its edge, and should lose to a neighbour that is nearer.
    ///
    /// Returning `None` withdraws the widget from the pass altogether, which is
    /// the shape-level equivalent of `no_hit_slop`.
    ///
    /// This is **never** consulted by the exact pass, so overriding it cannot
    /// change where an ordinary click lands.
    fn hit_distance(&self, local_point: Point, bounds: Rect) -> Option<f32> {
        Some(crate::pointer::hit_slop::rect_distance(bounds, local_point))
    }

    /// The interactive sub-regions this widget **paints inside its own single
    /// node** — a scroll bar's thumb, a slider's knob, a header cell's filter
    /// affordance.
    ///
    /// Reporting only: implementing it changes no layout and no hit test by
    /// itself. It exists because a control that draws several targets on one
    /// canvas is otherwise opaque — the router cannot route a coarse press to
    /// the nearest one, and the target-conformance audit cannot see that any of
    /// them exists, let alone that it clears the floor.
    ///
    /// `bounds` is this widget's current rectangle, and the returned rects are
    /// in the same space. Build them with
    /// [`partition_targets`](crate::partition::partition_targets) where the
    /// split is a horizontal division, so the geometry the widget paints and
    /// the geometry it reports cannot drift apart.
    ///
    /// The default returns an empty list: a widget whose node *is* its target
    /// has nothing to add.
    fn target_regions(&self, _bounds: Rect) -> Vec<crate::partition::TargetRegion> {
        Vec::new()
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

    /// Whether this widget, used as tooltip content, currently has anything
    /// worth showing.
    ///
    /// Consulted by `WidgetTree` just before a dwell matures into an overlay.
    /// Returning `false` cancels the show — the anchor simply has no tooltip
    /// this time — so a blank or unresolved string does not pop an empty
    /// chromed bubble, which reads as a rendering fault rather than as
    /// "nothing to say here".
    ///
    /// Defaults to `true`: content that hosts an arbitrary widget tree (a
    /// chart, a progress row) is meaningful without any text, and a custom
    /// content widget must never be suppressed by a check it did not opt into.
    /// Only widgets whose *whole* payload is a string — `TooltipWidget` — have
    /// a well-defined notion of being empty.
    fn tooltip_has_content(&self) -> bool {
        true
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

/// A boxed widget is a widget.
///
/// Without this, `Box<dyn Widget>` is the one widget-shaped value that cannot
/// go where a widget goes: `.child(..)`, a `Vec` of children, a `match` arm.
/// 238 functions in `teksilo-widgets` alone return it, the `teksu!` macro's own
/// over-four-arms advice recommends it, and only two containers in the whole
/// catalog (`Switcher`, `Cycle`) shipped a `child_boxed` to take it. Every
/// other call site had to invent an adapter widget, which costs a real arena
/// node per use.
///
/// Every method forwards to the inner widget, so the box is invisible to the
/// arena: it adds no node, no layout pass and no AT element. In particular
/// `as_any` / `as_any_mut` forward, so `EventContext::with_widget_mut::<W>`
/// downcasts to the widget that was boxed rather than to the box.
/// `#[deny(clippy::missing_trait_methods)]` for the reason the trait's own
/// header gives: the arena holds `Box<dyn Widget>`, so a call on a node
/// resolves to *this* impl and not to the vtable. A method left out here
/// answers with the trait default for every widget in the tree, the
/// widget's own override is never reached, and nothing fails to compile —
/// which is exactly how `accepts_child_hit` and `culls_children` went
/// missing when this impl and those two methods were written on separate
/// branches and merged cleanly.
#[deny(clippy::missing_trait_methods)]
impl<W: Widget + ?Sized> Widget for Box<W> {
    fn type_name(&self) -> &'static str {
        (**self).type_name()
    }

    fn build(
        &mut self,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        (**self).build(ctx)
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        (**self).layout_response(proposal, ctx)
    }

    fn cacheable_layout(&self) -> bool {
        (**self).cacheable_layout()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        (**self).place_children(bounds, proposal, children, ctx)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        (**self).paint(bounds, canvas, ctx)
    }

    fn wants_after_paint(&self) -> bool {
        (**self).wants_after_paint()
    }

    fn after_paint(&self, view: &WidgetTreeView<'_>, ctx: &PaintContext) {
        (**self).after_paint(view, ctx)
    }

    fn wants_post_paint(&self) -> bool {
        (**self).wants_post_paint()
    }

    fn post_paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        (**self).post_paint(bounds, canvas, ctx)
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        (**self).accessibility(builder)
    }

    fn wants_descendant_redirects(&self) -> bool {
        (**self).wants_descendant_redirects()
    }

    fn a11y_redirect_descendant(
        &self,
        self_id: WidgetId,
        descendant: WidgetId,
    ) -> Option<accesskit::NodeId> {
        (**self).a11y_redirect_descendant(self_id, descendant)
    }

    fn accessible_title_hint(&self) -> Option<String> {
        (**self).accessible_title_hint()
    }

    fn accessible_title_node(&self) -> Option<crate::widget_id::WidgetId> {
        (**self).accessible_title_node()
    }

    fn initial_focus_hint(&self) -> Option<WidgetId> {
        (**self).initial_focus_hint()
    }

    fn context_menu_key_target(&self) -> Option<WidgetId> {
        (**self).context_menu_key_target()
    }

    fn children(&self) -> Vec<WidgetId> {
        (**self).children()
    }

    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        (**self).accessibility_children()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        (**self).as_any()
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        (**self).as_any_mut()
    }

    fn clips_children(&self) -> bool {
        (**self).clips_children()
    }

    fn focus_reveal_rect(&self, bounds: Rect) -> Option<Rect> {
        (**self).focus_reveal_rect(bounds)
    }

    fn hit_shape(&self, local_point: Point, bounds: Rect) -> bool {
        (**self).hit_shape(local_point, bounds)
    }

    fn hit_outset(
        &self,
        kind: teksilo_tokens::PointerKind,
        tokens: &teksilo_tokens::InputTokens,
    ) -> teksilo_canvas::EdgeInsets {
        (**self).hit_outset(kind, tokens)
    }

    fn hit_slop(
        &self,
        kind: teksilo_tokens::PointerKind,
        tokens: &teksilo_tokens::InputTokens,
    ) -> Option<crate::pointer::hit_slop::HitSlop> {
        (**self).hit_slop(kind, tokens)
    }

    fn hit_distance(&self, local_point: Point, bounds: Rect) -> Option<f32> {
        (**self).hit_distance(local_point, bounds)
    }

    fn accepts_child_hit(&self, child: crate::widget_id::WidgetId, point: Point) -> bool {
        (**self).accepts_child_hit(child, point)
    }

    fn culls_children(&self) -> bool {
        (**self).culls_children()
    }

    fn target_regions(&self, bounds: Rect) -> Vec<crate::partition::TargetRegion> {
        (**self).target_regions(bounds)
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        (**self).preserves_children_on_rebuild()
    }

    fn tooltip_has_content(&self) -> bool {
        (**self).tooltip_has_content()
    }

    fn declare_shortcuts(&self) -> Vec<crate::shortcut::Shortcut> {
        (**self).declare_shortcuts()
    }

    fn take_handler_set(&mut self) -> Option<crate::widget_builder::HandlerSet> {
        (**self).take_handler_set()
    }
}
