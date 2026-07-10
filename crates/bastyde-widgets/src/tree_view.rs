// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TreeView — a virtualized, expandable/collapsible hierarchical list widget.
//!
//! Displays a [`TreeModel<T>`](bastyde_data::TreeModel) as an indented tree.
//! Internally each view owns a [`TreeSlice`] for independent
//! expand state, so two `TreeView`s on the same model can be open at different
//! depths simultaneously. Only rows in the visible viewport + a small buffer have
//! live widgets — rows outside the buffer are dormant, matching `ListView`'s
//! virtualization model. An external [`TreeDataSource`]
//! is also accepted via [`TreeView::from_source`] when the data lives outside a
//! `TreeModel`.
//!
//! Row heights come in three modes: uniform (`item_height`, default fast path),
//! exact per-flat-index callback (`item_height_fn`), and auto-measured
//! (`auto_item_height` — height-for-width per row, scroll-anchored).
//!
//! ## Example
//!
//! ```rust
//! # use bastyde_widgets::TreeView;
//! # use bastyde_widgets::primitives::{HStack, Padding, TextWidget};
//! # use bastyde_data::TreeModel;
//! # use bastyde_i18n::lit;
//! # struct Item { title: String }
//! # let tree_model: TreeModel<Item> = TreeModel::new();
//! let _w = TreeView::new(tree_model, |item, entry, _selected| {
//!     let indent = entry.depth as f32 * 20.0;
//!     Box::new(HStack::new()
//!         .child(Padding::new(0.0, 0.0, 0.0, indent))
//!         .child(TextWidget::new(lit!(&item.title))))
//! })
//! .item_height(28.0);
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_tokens::{BorderRole, Easing};

use bastyde_core::DropFeedback;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

use bastyde_data::selection_model::SelectionModel;
use bastyde_data::tree_slice::{TreeSlice, TreeSliceHandle};
use bastyde_data::{
    DragEligibility, DropPosition, DropResponse, FlatEntry, ItemKey, KeyedSelectionModel, NodeId,
    RowState, TreeDataSource, TreeModel,
};

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{DragTransferMode, RowDragData, RowSelection, ViewId, ViewKind};
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};
use crate::tree_source::{TreeRow, TreeRowMeta, TreeSource};

const BUFFER_ITEMS: usize = 5;
const DEFAULT_ITEM_HEIGHT: f32 = 28.0;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Per-row context passed to a 4-arg TreeView delegate. Carries a
/// reference to the slice handle and the row's `NodeId` so the
/// delegate can wire chevron toggles and other tree-aware behavior
/// without manually cloning state outside the closure.
///
/// Created internally by [`TreeView::new_with_context`]. Not
/// constructed directly by user code.
pub struct TreeRowContext<'a, T: 'static> {
    slice: &'a TreeSliceHandle<T>,
    node_id: bastyde_data::NodeId,
}

impl<'a, T: 'static> TreeRowContext<'a, T> {
    /// Toggle callback for this row's chevron. Wires in one line:
    /// `.on_toggle_rc(ctx.toggle_callback())`.
    pub fn toggle_callback(&self) -> std::rc::Rc<dyn Fn(&mut bastyde_core::widget::EventContext)> {
        let slice = self.slice.clone();
        let node = self.node_id;
        std::rc::Rc::new(move |_ctx| slice.toggle_expand(node))
    }

    /// Cloned handle to the slice — call `.toggle_expand(node)`,
    /// `.expand(node)`, `.collapse(node)` directly.
    pub fn slice_handle(&self) -> TreeSliceHandle<T> {
        self.slice.clone()
    }

    /// The `NodeId` of this row in the backing `TreeModel`.
    pub fn node_id(&self) -> bastyde_data::NodeId {
        self.node_id
    }
}

/// Delegate type for the built-in `TreeModel` path: takes the inputs the 3-arg
/// form gets plus the optional `TreeRowContext`. Both the 3-arg `new` and the
/// 4-arg `new_with_context` produce a closure of this shape.
type TreeDelegate<T> = dyn Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget>;

/// Delegate type for the generic [`TreeView::from_source`] path: key-erased, so
/// it receives a [`TreeRow`] (flat metadata + a chevron toggle) instead of the
/// `NodeId`-typed `FlatEntry` / `TreeRowContext`.
type SourceTreeDelegate<T> = dyn Fn(&T, &TreeRow, bool) -> Box<dyn Widget>;

/// Internal, uniform per-row builder both constructors lower to:
/// `(visible_index, &item, &meta, selected) -> row widget`. The built-in
/// wrapper rebuilds the `NodeId` `TreeRowContext` from the index; the generic
/// wrapper builds a key-erased `TreeRow`.
type RowDelegate<T> = dyn Fn(usize, &T, &TreeRowMeta, bool) -> Box<dyn Widget>;

/// A virtualized hierarchical tree widget backed by a `TreeModel<T>`.
///
/// ```rust
/// # use bastyde_widgets::{TreeView};
/// # use bastyde_widgets::primitives::{HStack, Padding, TextWidget};
/// # use bastyde_data::TreeModel;
/// # use bastyde_i18n::lit;
/// # struct Item { title: String }
/// # let tree_model: TreeModel<Item> = TreeModel::new();
/// let _w = TreeView::new(tree_model, |item, entry, _selected| {
///     let indent = entry.depth as f32 * 20.0;
///     Box::new(HStack::new()
///         .child(Padding::new(0.0, 0.0, 0.0, indent))
///         .child(TextWidget::new(lit!(&item.title))))
/// })
/// .item_height(28.0);
/// ```
/// Active drag-drop feedback the `TreeView` paints itself: a between-rows
/// insertion line (Before/After) or a highlighted row (an into-container drop).
#[derive(Clone, Copy, PartialEq)]
enum DropViz {
    /// Horizontal insertion line at `y`, spanning `width`.
    Line { y: f32, width: f32 },
    /// Highlighted target row `[top, top + height]`, spanning `width` — the
    /// "drop into this folder" affordance.
    Rect { top: f32, height: f32, width: f32 },
}

pub struct TreeView<T: 'static> {
    /// Index-keyed erased backing — the built-in `TreeSlice` or an external
    /// `TreeDataSource`. All virtualization / DnD / keyboard work goes through
    /// this in flat indices.
    source: Rc<TreeSource<T>>,
    /// Present only for the built-in `TreeModel` path; backs the `NodeId`-typed
    /// public expand API + [`tree_slice`](Self::tree_slice). `None` for
    /// [`from_source`](Self::from_source).
    slice: Option<Rc<TreeSlice<T>>>,
    /// Uniform per-row builder produced by whichever constructor was used.
    row_delegate: Rc<RowDelegate<T>>,
    item_height: f32,
    /// Height-mode selection (uniform / exact callback / auto-measure).
    height_source: HeightSource,
    /// Row geometry — all virtualization consumers go through this.
    metrics: SharedRowMetrics,
    /// Row selection — index-based `SelectionModel` or keyed
    /// `KeyedSelectionModel<NodeId>`, unified behind the index-facing facade.
    row_selection: Option<RowSelection>,

    /// Keyboard-focused flat index.
    focused_index: Rc<Cell<Option<usize>>>,

    /// Type-ahead ("type to jump") label extractor — opt-in via
    /// [`type_ahead_label`](Self::type_ahead_label).
    type_ahead_label: Option<Rc<dyn Fn(&T) -> String>>,
    /// Reset window for the type-ahead search term.
    type_ahead_timeout: Duration,
    /// Persistent type-ahead buffer (survives the per-keystroke rebuild).
    type_ahead: Rc<crate::common::type_ahead::TypeAheadState>,

    /// Enable intra-widget drag reordering.
    reorderable: bool,

    /// Cross-widget export / foreign-receive machinery — the builders
    /// (`.exportable`, `.export_external`, `.accept_foreign_rows`,
    /// `.on_rows_received`, `.on_rows_transferred_out`), the drag-start payload
    /// build, and the move-out completion, shared by all five data views.
    export: crate::data_views::RowExport<T>,

    /// Whether a row-body PointerUp on a branch row auto-toggles its
    /// expansion. Defaults to `true` (legacy behavior — convenient
    /// for hand-built delegates without an explicit chevron). Set to
    /// `false` when the delegate provides its own chevron tap target
    /// (e.g. `StandardTreeItem`) to avoid the auto-toggle firing in
    /// addition to the chevron's own click and cancelling out.
    row_click_expands: bool,

    /// Active drop feedback (set by on_drag_hover, cleared by on_drag_leave,
    /// read by paint). Reactive Signal — bound at `RepaintOnly` so any
    /// `set(...)` call dirties the TreeView for repaint automatically.
    drop_feedback: Signal<Option<DropViz>>, // insertion line OR folder highlight

    /// Optional row-activation callback (a click on the row body per
    /// `activate_on`, or Enter/Space on the focused row) — distinct from
    /// *selection*, which also moves on arrow navigation. Lets a view
    /// open/commit a row without firing on every navigation step.
    on_activate: Option<Rc<dyn Fn(usize)>>,
    /// Whether activation is a single or double click (default `DoubleClick`).
    activate_on: crate::data_views::ActivateOn,

    /// `true` while this view (root or descendant) holds keyboard focus — the
    /// root's inclusive [`BuildContext::view_focus_active`](bastyde_core::BuildContext::view_focus_active) signal, bound
    /// `RepaintOnly`. With [`focus_visible`](Self::focus_visible) it drives the
    /// **container focus ring**: when the view is Tab-focused but nothing is
    /// selected, no row ring shows, so the whole view outlines itself instead —
    /// the user can see where keyboard focus landed before they arrow.
    view_focused: Signal<bool>,
    /// Input-modality `:focus-visible`. Gates the container ring (and row rings)
    /// to keyboard navigation, never a mouse click. Bound `RepaintOnly`.
    focus_visible: Signal<bool>,

    // Persistent scroll state
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    /// Scroll-chaining behavior at the boundary (default `Chain`).
    overscroll_behavior: OverscrollBehavior,
    viewport_ratio_y: Signal<f32>,

    /// Animate wheel scrolling instead of snapping to the new offset.
    /// Enabled by default — mirrors `ScrollArea`. Without it, each wheel
    /// notch jumps by `item_height` per delivered line (typically 3),
    /// which reads as a coarse multi-row jump rather than a smooth glide.
    smooth_scrolling: bool,
    /// Duration of the smooth scroll animation.
    smooth_scroll_duration: Duration,

    /// How the scroll bar is displayed. Defaults to `Permanent` — a
    /// layout sibling that reserves its own width. `Overlay` / `Thin`
    /// float over the content instead, like `ScrollArea`.
    scroll_bar_style: ScrollBarMode,

    /// Rebuild trigger. A persistent field (re-bound each build) so
    /// `place_children`'s post-measure realization re-check can request
    /// a rebuild when corrected offsets reveal unrealized viewport rows.
    version: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    prev_built_start: Rc<Cell<usize>>,
    prev_built_end: Rc<Cell<usize>>,

    // Set during build
    item_entries: Vec<(usize, WidgetId)>, // (flat_index, widget_id)
    scrollbar_id: Option<WidgetId>,
    viewport_height: Rc<Cell<f32>>,
    /// The TreeView's own absolute (window) bounds, cached from
    /// `place_children` so the keyboard handler can chase the selected row
    /// into enclosing scroll areas via
    /// [`EventContext::ensure_visible`](bastyde_core::widget::EventContext::ensure_visible).
    /// Rows are not distinct focusable nodes, so the focus-driven follow never
    /// reveals the selected row in an outer scroller — this closes that gap.
    viewport_bounds: Rc<Cell<Rect>>,
    tree_id: ViewId,

    /// Whole-view enabled state, statically or reactively. Forwarded to the
    /// arena via `ctx.enabled_when(self_id, self.enabled.clone())` at build
    /// time; a disabled view greys out and stops accepting focus /
    /// selection / keyboard input (arena-gated).
    enabled: Prop<bool>,
}

impl<T: 'static> TreeView<T> {
    /// Create a new TreeView backed by a `TreeModel<T>`.
    ///
    /// The delegate receives `(&item, &FlatEntry, selected)` and returns a
    /// boxed widget. The `FlatEntry` provides `depth`, `has_children`, and
    /// `is_expanded` for rendering indentation and expand/collapse toggles.
    pub fn new(
        model: TreeModel<T>,
        delegate: impl Fn(&T, &FlatEntry, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        // Adapt the 3-arg delegate to the internal 4-arg shape by
        // discarding the context.
        let adapted =
            move |item: &T, entry: &FlatEntry, sel: bool, _ctx: &TreeRowContext<'_, T>| {
                delegate(item, entry, sel)
            };
        Self::new_internal(model, Rc::new(adapted))
    }

    /// Like [`new`](Self::new), but the delegate also receives a
    /// [`TreeRowContext`] from which `.toggle_callback()` can be
    /// pulled in a single line — eliminating the need to manually
    /// clone the slice handle outside the closure.
    ///
    /// ```rust
    /// # use bastyde_widgets::{TreeView, StandardTreeItem};
    /// # use bastyde_data::TreeModel;
    /// # use bastyde_i18n::lit;
    /// # struct Item { title: String }
    /// # let model: TreeModel<Item> = TreeModel::new();
    /// let _w = TreeView::new_with_context(model, |item, entry, selected, ctx| {
    ///     Box::new(
    ///         StandardTreeItem::new(lit!(&item.title))
    ///             .from_entry(entry)
    ///             .selected(selected)
    ///             .on_toggle_rc(ctx.toggle_callback())
    ///     )
    /// });
    /// ```
    pub fn new_with_context(
        model: TreeModel<T>,
        delegate: impl Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::new_internal(model, Rc::new(delegate))
    }

    fn new_internal(model: TreeModel<T>, delegate: Rc<TreeDelegate<T>>) -> Self {
        let slice = Rc::new(TreeSlice::new(model));
        let source = Rc::new(TreeSource::from_data_source(slice.clone()));
        // Built-in wrapper: rebuild the `NodeId` `FlatEntry` + `TreeRowContext`
        // from the visible index so the existing 3-/4-arg delegate keeps its
        // exact API. `with_row` only invokes this for a present row, so
        // `visible_node_id(i)` is `Some`; the `None` arm is an unreachable guard.
        let slice_for_rows = slice.clone();
        let row_delegate: Rc<RowDelegate<T>> = Rc::new(move |i, item, meta, selected| {
            let handle = slice_for_rows.handle();
            match handle.visible_node_id(i) {
                Some(node_id) => {
                    let entry = FlatEntry {
                        node_id,
                        depth: meta.depth,
                        has_children: meta.has_children,
                        is_expanded: meta.is_expanded,
                    };
                    let row_ctx = TreeRowContext {
                        slice: &handle,
                        node_id,
                    };
                    delegate(item, &entry, selected, &row_ctx)
                }
                None => crate::data_views::default_placeholder(),
            }
        });
        Self::assemble(source, Some(slice), row_delegate)
    }

    /// Create a TreeView backed by any [`TreeDataSource`] — an external source of
    /// truth (e.g. an entity store) carrying its own `Key`, so it needs no
    /// `TreeModel` mirror. The delegate receives `(&item, &TreeRow, selected)`;
    /// [`TreeRow`] exposes `depth` / `has_children` / `is_expanded` and a one-call
    /// chevron `toggle_callback()`. Drop validation + lazy windowing route
    /// through the source's `can_accept` / `accept_drop` / `row_state`.
    pub fn from_source<S: TreeDataSource<Item = T>>(
        source: S,
        delegate: impl Fn(&T, &TreeRow, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self::from_source_rc(Rc::new(source), Rc::new(delegate))
    }

    fn from_source_rc<S: TreeDataSource<Item = T>>(
        s: Rc<S>,
        delegate: Rc<SourceTreeDelegate<T>>,
    ) -> Self {
        let source = Rc::new(TreeSource::from_data_source(s));
        let source_for_rows = source.clone();
        let row_delegate: Rc<RowDelegate<T>> = Rc::new(move |i, item, _meta, selected| {
            let row = TreeSource::row_context(&source_for_rows, i);
            delegate(item, &row, selected)
        });
        Self::assemble(source, None, row_delegate)
    }

    /// Like [`from_source`](Self::from_source) but with **keyed** selection: the
    /// `KeyedSelectionModel<S::Key>` tracks selection by source identity, so it
    /// survives expand / collapse / filter / reorder and stays consistent across
    /// two views of the same source. The view stays `TreeView<T>` — the `Key` is
    /// captured here. Pruning consults the source's
    /// [`contains_key`](bastyde_data::TreeDataSource::contains_key), so a
    /// collapsed-but-present node keeps its selection.
    pub fn from_source_keyed<S: TreeDataSource<Item = T>>(
        source: S,
        keyed: KeyedSelectionModel<S::Key>,
        delegate: impl Fn(&T, &TreeRow, bool) -> Box<dyn Widget> + 'static,
    ) -> Self
    where
        S::Key: ItemKey,
    {
        let s = Rc::new(source);
        let key_at = {
            let s = s.clone();
            Rc::new(move |i| s.key_at(i)) as Rc<dyn Fn(usize) -> Option<S::Key>>
        };
        let len = {
            let s = s.clone();
            Rc::new(move || s.visible_count()) as Rc<dyn Fn() -> usize>
        };
        let contains = {
            let s = s.clone();
            Rc::new(move |k: &S::Key| s.contains_key(k)) as Rc<dyn Fn(&S::Key) -> bool>
        };
        let row_selection = RowSelection::from_keyed(keyed, key_at, len, contains);
        let mut view = Self::from_source_rc(s, Rc::new(delegate));
        view.row_selection = Some(row_selection);
        view
    }

    fn assemble(
        source: Rc<TreeSource<T>>,
        slice: Option<Rc<TreeSlice<T>>>,
        row_delegate: Rc<RowDelegate<T>>,
    ) -> Self {
        let view_id = ViewId::next(ViewKind::Tree);
        Self {
            source,
            slice,
            row_delegate,
            item_height: DEFAULT_ITEM_HEIGHT,
            height_source: HeightSource::Uniform,
            metrics: Rc::new(RefCell::new(RowMetrics::uniform(DEFAULT_ITEM_HEIGHT, 0.0))),
            row_selection: None,
            focused_index: Rc::new(Cell::new(None)),
            type_ahead_label: None,
            type_ahead_timeout: crate::common::type_ahead::DEFAULT_TYPE_AHEAD_TIMEOUT,
            type_ahead: crate::common::type_ahead::TypeAheadState::new(),
            reorderable: false,
            export: crate::data_views::RowExport::default(),
            row_click_expands: true,
            drop_feedback: Signal::new(None),
            // Replaced at build with the live tree signals.
            view_focused: Signal::new(false),
            focus_visible: Signal::new(false),
            on_activate: None,
            activate_on: crate::data_views::ActivateOn::default(),
            overscroll_behavior: OverscrollBehavior::default(),
            smooth_scrolling: true,
            smooth_scroll_duration: Duration::from_millis(150),
            scroll_bar_style: ScrollBarMode::Permanent,
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_y: Signal::new(1.0),
            version: Signal::new(0_u64),
            prev_built_start: Rc::new(Cell::new(0)),
            prev_built_end: Rc::new(Cell::new(0)),
            item_entries: Vec::new(),
            scrollbar_id: None,
            viewport_height: Rc::new(Cell::new(600.0)),
            viewport_bounds: Rc::new(Cell::new(Rect::ZERO)),
            tree_id: view_id,
            enabled: Prop::Static(true),
        }
    }

    /// Enable or disable the whole view. A disabled view greys out and stops
    /// accepting focus / selection / keyboard input (arena-gated).
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Set the scroll-chaining behavior at the boundary (default
    /// [`OverscrollBehavior::Chain`]; [`Contain`](OverscrollBehavior::Contain)
    /// disables chaining to an ancestor scrollable).
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    /// Re-materialize `self.metrics` after a height-mode / item-height
    /// builder call.
    fn remake_metrics(&self) {
        *self.metrics.borrow_mut() = self.height_source.make_metrics(self.item_height, 0.0);
    }

    /// Set the fixed height per row (default 28.0) — the uniform fast
    /// path. Mutually exclusive with [`item_height_fn`](Self::item_height_fn)
    /// and [`auto_item_height`](Self::auto_item_height); the last mode
    /// setter wins.
    pub fn item_height(mut self, height: f32) -> Self {
        self.item_height = height;
        self.height_source = HeightSource::Uniform;
        self.remake_metrics();
        self
    }

    /// Enable or disable animated wheel scrolling (enabled by default).
    /// When disabled, wheel events snap immediately to the new offset.
    pub fn smooth_scrolling(mut self, enabled: bool) -> Self {
        self.smooth_scrolling = enabled;
        self
    }

    /// Duration of the smooth scroll animation (default 150 ms).
    pub fn smooth_scroll_duration(mut self, duration: Duration) -> Self {
        self.smooth_scroll_duration = duration;
        self
    }

    /// How the scroll bar is displayed (default `Permanent`). `Overlay`
    /// and `Thin` float the bar over the content instead of reserving a
    /// layout column for it, mirroring `ScrollArea::scroll_bar_style`.
    pub fn scroll_bar_style(mut self, style: ScrollBarMode) -> Self {
        self.scroll_bar_style = style;
        self
    }

    /// Per-row heights from a callback over the *flat (visible) index*.
    /// The callback must be pure (same index + same data → same height);
    /// it is re-swept from the first changed flat index on every model
    /// change or expand/collapse. No measurement pass runs.
    pub fn item_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self {
        self.height_source = HeightSource::Exact(Rc::new(f));
        self.remake_metrics();
        self
    }

    /// Auto-measured row heights: each realized row is measured at the
    /// tree's content width (height-for-width), unrealized rows assume
    /// `estimated`. Scroll anchoring keeps content above the viewport
    /// stationary as estimates are corrected; measured heights above a
    /// toggled row survive expand/collapse (divergence-driven
    /// invalidation).
    pub fn auto_item_height(mut self, estimated: f32) -> Self {
        self.height_source = HeightSource::Auto { estimated };
        self.remake_metrics();
        self
    }

    /// Whether a row-body PointerUp on a branch row auto-toggles its
    /// expansion (default `true`). Set to `false` when the delegate
    /// provides its own chevron tap target (e.g. `StandardTreeItem`)
    /// — without this, the auto-toggle fires in addition to the
    /// chevron's own click and they cancel out, leaving the row
    /// expanded only on body clicks.
    pub fn row_click_expands(mut self, b: bool) -> Self {
        self.row_click_expands = b;
        self
    }

    /// Set the index-based selection model (visible positions). For
    /// identity-based selection that survives expand / collapse / filter and
    /// node moves, use [`keyed_selection`](Self::keyed_selection) instead.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.row_selection = Some(RowSelection::from_index(sel));
        self
    }

    /// Set a keyed selection model (by `NodeId`). Selection is tracked by node
    /// identity, so it survives expand / collapse, filtering, and node moves —
    /// and stays consistent if two views share the model. Pruned of deleted
    /// nodes on each slice change. Mutually exclusive with
    /// [`selection`](Self::selection) (last one set wins).
    pub fn keyed_selection(mut self, keyed: KeyedSelectionModel<NodeId>) -> Self {
        // Built-in `TreeModel` path only; on `from_source` use
        // [`from_source_keyed`](Self::from_source_keyed) (the `Key` differs).
        let Some(slice) = self.slice.clone() else {
            return self;
        };
        let key_at = {
            let tsh = slice.handle();
            Rc::new(move |i| tsh.visible_node_id(i)) as Rc<dyn Fn(usize) -> Option<NodeId>>
        };
        let len = {
            let tsh = slice.handle();
            Rc::new(move || tsh.visible_count()) as Rc<dyn Fn() -> usize>
        };
        // A collapsed-but-present node must NOT be pruned, so existence is
        // checked against the tree, not the visible projection.
        let contains = {
            let tsh = slice.handle();
            Rc::new(move |n: &NodeId| tsh.tree().with_item(*n, |_| ()).is_some())
                as Rc<dyn Fn(&NodeId) -> bool>
        };
        self.row_selection = Some(RowSelection::from_keyed(keyed, key_at, len, contains));
        self
    }

    /// Enable intra-widget drag reordering.
    ///
    /// When enabled, tree rows can be dragged to reparent or reorder them.
    /// Before/Into/After is chosen by where in the row the pointer drops; the
    /// move is cycle-guarded — a drop onto the node itself or into its own
    /// subtree is refused and shows no insertion line. Keyboard equivalent:
    /// Alt+ArrowUp/Down.
    pub fn reorderable(mut self, enabled: bool) -> Self {
        self.reorderable = enabled;
        self
    }

    /// Make rows **droppable outside this view** — on a
    /// [`DropTarget`](crate::DropTarget), another data view, or the OS.
    ///
    /// A dragged row (or the whole selection, when the pressed row is part of a
    /// multi-selection) carries clones of its items in a public
    /// [`RowDragData<T>`](crate::RowDragData), so a foreign receiver can pull
    /// them out with `payload.get_typed::<RowDragData<T>>()` /
    /// `DropTarget::on_drop_typed::<RowDragData<T>>()` — no serialization. This
    /// also makes rows a drag source even without [`reorderable`](Self::reorderable).
    ///
    /// `mode` chooses what happens to the origin rows once a *foreign* target
    /// accepts them: [`DragTransferMode::Move`] removes them (via the source's
    /// `on_drag_out`, or [`on_rows_transferred_out`](Self::on_rows_transferred_out)),
    /// [`DragTransferMode::Copy`] leaves them. A same-view reorder is never a
    /// transfer, so `mode` never affects it. Requires `T: Clone`.
    pub fn exportable(mut self, mode: DragTransferMode) -> Self
    where
        T: Clone,
    {
        self.export.set_exportable(mode);
        self
    }

    /// Additionally advertise the dragged rows as MIME data so they can be
    /// dropped on a [`DropZone`](crate::DropZone) or exported to another
    /// application / window via the OS. `f` maps the dragged items to
    /// `(mime_type, bytes)` pairs (e.g. `text/plain`, `text/uri-list`, an
    /// app-specific `application/x-…`). Implies [`exportable`](Self::exportable)
    /// (defaulting to [`DragTransferMode::Move`] if not already set). Requires
    /// `T: Clone`.
    pub fn export_external(mut self, f: impl Fn(&[T]) -> Vec<(String, Vec<u8>)> + 'static) -> Self
    where
        T: Clone,
    {
        self.export.set_export_external(f);
        self
    }

    /// Override how rows moved out to a foreign target are removed from this
    /// view. Receives the dragged rows' indices (descending-safe) and the live
    /// context. Without this, an [`exportable`](Self::exportable)
    /// [`Move`](DragTransferMode::Move) drag removes them through the source's
    /// `on_drag_out` (works out of the box for a `TreeSlice`/`TreeModel`).
    pub fn on_rows_transferred_out(
        mut self,
        f: impl Fn(&[usize], &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_transferred_out(f);
        self
    }

    /// Accept exported rows dropped from a **different** view or source without
    /// writing a custom `TreeDataSource`. Pair with
    /// [`on_rows_received`](Self::on_rows_received), which is handed the dropped
    /// items and the insertion index. (Same-view reorder is
    /// [`reorderable`](Self::reorderable); a custom `TreeDataSource` can still
    /// accept foreign drops through its `can_accept`/`accept_drop` instead.)
    pub fn accept_foreign_rows(mut self, accept: bool) -> Self {
        self.export.accept_foreign_rows = accept;
        self
    }

    /// Handler for rows accepted via [`accept_foreign_rows`](Self::accept_foreign_rows):
    /// `(items, insertion_index, ctx)`. Insert them into your model at the
    /// index.
    pub fn on_rows_received(
        mut self,
        f: impl Fn(Vec<T>, usize, &mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_received(f);
        self
    }

    /// Set the row-**activation** handler — invoked with the flat row index on a
    /// primary click on the row body, or **Enter** on the focused row.
    /// Activation is distinct from *selection*: arrow-key navigation and
    /// **Space** move / toggle the selection but do **not** activate, so a view
    /// can open/commit a row on a deliberate click/Enter without firing on
    /// every navigation step.
    pub fn on_activate(mut self, f: impl Fn(usize) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Choose single- vs double-click activation (default
    /// [`ActivateOn::DoubleClick`](crate::ActivateOn) — the cross-platform
    /// convention; pass [`SingleClick`](crate::ActivateOn::SingleClick) for the
    /// KDE/web/Scrivener feel). Enter activates in either mode.
    pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self {
        self.activate_on = mode;
        self
    }

    /// Enable **type-ahead** ("type to jump"): typing a printable character
    /// while the tree has keyboard focus jumps the selection to the next
    /// *visible* row whose label starts with the accumulated search term,
    /// wrapping around (Qt `keyboardSearch` / macOS & Windows type-select).
    /// `label(&item)` yields the searchable text; matching is
    /// ASCII-case-insensitive. A pause longer than the
    /// [`type_ahead_timeout`](Self::type_ahead_timeout) starts a fresh term.
    pub fn type_ahead_label(mut self, label: impl Fn(&T) -> String + 'static) -> Self {
        self.type_ahead_label = Some(Rc::new(label));
        self
    }

    /// Reset window between keystrokes before the type-ahead search term
    /// clears (default 500 ms). A zero duration disables type-ahead.
    pub fn type_ahead_timeout(mut self, timeout: Duration) -> Self {
        self.type_ahead_timeout = timeout;
        self
    }

    /// Expand a node programmatically. No-op on the `from_source` path (which
    /// owns its own expand state — use the source's `set_expanded`).
    pub fn expand(&self, node: bastyde_data::NodeId) {
        if let Some(slice) = &self.slice {
            slice.expand(node);
        }
    }

    /// Collapse a node programmatically. No-op on the `from_source` path.
    pub fn collapse(&self, node: bastyde_data::NodeId) {
        if let Some(slice) = &self.slice {
            slice.collapse(node);
        }
    }

    /// Toggle a node's expand/collapse state. No-op on the `from_source` path.
    pub fn toggle(&self, node: bastyde_data::NodeId) {
        if let Some(slice) = &self.slice {
            slice.toggle(node);
        }
    }

    /// Expand all nodes. No-op on the `from_source` path.
    pub fn expand_all(&self) {
        if let Some(slice) = &self.slice {
            slice.expand_all();
        }
    }

    /// Collapse all nodes. No-op on the `from_source` path.
    pub fn collapse_all(&self) {
        if let Some(slice) = &self.slice {
            slice.collapse_all();
        }
    }

    /// Access the internal `TreeSlice` (for persistence of expand state).
    /// `None` on the [`from_source`](Self::from_source) path, which has no
    /// `TreeSlice` (the external source owns expand state).
    pub fn tree_slice(&self) -> Option<&TreeSlice<T>> {
        self.slice.as_deref()
    }

    fn total_content_height(&self) -> f32 {
        self.metrics
            .borrow_mut()
            .total_height(self.source.visible_count())
    }

    fn visible_range(&self) -> (usize, usize) {
        self.metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.visible_count(),
            BUFFER_ITEMS,
        )
    }

    fn clamp_scroll(&self) {
        let max = self.max_scroll_y.get();
        let current = self.scroll_y.get();
        let clamped = current.clamp(0.0, max);
        if (clamped - current).abs() > 0.001 {
            self.scroll_y.set(clamped);
        }
    }
}

impl<T: 'static> std::fmt::Debug for TreeView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeView")
            .field("visible_count", &self.source.visible_count())
            .field("item_height", &self.item_height)
            .field("scroll_bar_style", &self.scroll_bar_style)
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for TreeView<T> {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        // --- Version signal for rebuild triggering ---
        // A persistent field (not `ctx.signal`) so the realization
        // re-check in `place_children` can bump it after measurement.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (repositions items) without a full rebuild.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        ctx.register_animated_signal(&self.scroll_y);

        // Bind drop_feedback at RepaintOnly so `set(...)` calls from
        // on_drag_hover / on_drag_leave dirty the TreeView's paint cache
        // without triggering a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Focus signals for the container ring. `begin_view_focus` keys the
        // scope signal on this root id directly (independent of the arena
        // focusable flag, not yet wired here): a plain `view_focus_active()`
        // would find no focusable ancestor and fall back to the constant-`true`
        // "outside any scope" signal — lighting the ring whenever ANY other
        // widget takes keyboard focus. Pop straight back; the real row scope
        // below resolves the same cached signal. `focus_visible` is the
        // keyboard/pointer modality. Bound `RepaintOnly` so focus-in/out
        // redraws the ring. (Selection-emptiness changes already rebuild via
        // `version`, so paint re-reads the selection without extra binding.)
        self.view_focused = ctx.begin_view_focus();
        ctx.end_view_focus();
        self.focus_visible = ctx.focus_visible();
        self.view_focused.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.focus_visible.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // --- Observe source version (covers both data mutations and expand/collapse) ---
        let source_version = self.source.version_signal();
        let version_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&source_version, {
            let dv = data_ver.clone();
            let ver = version_for_data.clone();
            let metrics = self.metrics.clone();
            let source = self.source.clone();
            let row_sel = self.row_selection.clone();
            move |_| {
                // Source version observers fire synchronously per reflatten, so
                // `first_changed_index()` describes exactly this change:
                // heights of flat rows before it (e.g. above an
                // expand/collapse point) stay valid.
                metrics
                    .borrow_mut()
                    .apply_divergence(source.first_changed_index(), source.visible_count());
                // Drop any keyed selection whose node was deleted (no-op for
                // the index model). A collapse does not delete, so a collapsed
                // node's selection survives.
                if let Some(ref rs) = row_sel {
                    rs.prune();
                }
                let next = dv.get() + 1;
                dv.set(next);
                ver.set(next);
            }
        });

        // --- Observe selection changes (rebuild to update delegate's `selected` param) ---
        if let Some(ref rs) = self.row_selection {
            let version_for_sel = version.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                version_for_sel.set(next);
            });
            ctx.own_handle(handle);
        }

        // --- Observe scroll position changes (rebuild only when items leave/enter buffer) ---
        let viewport_h = self.viewport_height.clone();
        // Track the buffered range from this build. Only trigger a rebuild
        // when the visible range exceeds the buffer — most scrolls just need
        // a relayout (handled by scroll_y's Relayout binding above).
        let (built_start, built_end) = self.visible_range();
        self.prev_built_start.set(built_start);
        self.prev_built_end.set(built_end);
        let version_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let pbs = self.prev_built_start.clone();
            let pbe = self.prev_built_end.clone();
            let sv = scroll_ver.clone();
            let metrics = self.metrics.clone();
            let source = self.source.clone();
            move |y| {
                let count = source.visible_count();
                let (visible_start, visible_end) =
                    metrics
                        .borrow_mut()
                        .visible_range(*y, viewport_h.get(), count, 0);
                // Only rebuild when visible items fall outside the currently-built range
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ITEMS);
                    // Clamp to `count` — build() realizes a `min(end, count)`
                    // window, so an unclamped `pbe` past the end leaves the
                    // dirty-check believing rows are built that never were,
                    // so the bottom rows of a large tree never realize on a
                    // fast scroll. Mirrors TableView's BodyPane.
                    let new_end = (visible_end + BUFFER_ITEMS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    let next = sv.get() + 1;
                    sv.set(next);
                    version_for_scroll.set(next);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // --- Scroll event handler + DnD ---
        let scroll_y = self.scroll_y.clone();
        let max_scroll = self.max_scroll_y.clone();
        let line_height = self.item_height;
        let overscroll_behavior = self.overscroll_behavior;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;
        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                bastyde_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        bastyde_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        bastyde_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y.get();
                    let max = max_scroll.get();
                    // Base off the animation target (not the rendered offset)
                    // so a mid-fling boundary correctly chains and successive
                    // notches accumulate instead of restarting from the
                    // partway-animated position.
                    let base = scroll_y.animation_target().unwrap_or(current);
                    let (new_y, moved) = crate::common::scroll::scroll_clamp_axis(base, dy, max);
                    if moved {
                        if smooth_scrolling {
                            scroll_y.animate_to(new_y, smooth_scroll_duration, Easing::EaseOut);
                        } else {
                            scroll_y.set(new_y);
                        }
                    }
                    // Chain to an ancestor scrollable when fully clamped
                    // (unless Contain), otherwise consume.
                    crate::common::scroll::scroll_response(
                        moved,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                _ => bastyde_core::event::EventResponse::Ignored,
            })
            .clips_children(true)
            .focusable(true);

        // --- Keyboard navigation + expand/collapse + Alt+Arrow reorder ---
        {
            let source = self.source.clone();
            let sel_for_key = self.row_selection.clone();
            let activate_key = self.on_activate.clone();
            let fi = self.focused_index.clone();
            let reorderable = self.reorderable;
            let scroll_for_nav = self.scroll_y.clone();
            let metrics_for_nav = self.metrics.clone();
            let max_for_nav = self.max_scroll_y.clone();
            let vh_for_nav = self.viewport_height.clone();
            let vb_for_nav = self.viewport_bounds.clone();
            let ta_state = self.type_ahead.clone();
            let ta_label = self.type_ahead_label.clone();
            let ta_timeout = self.type_ahead_timeout;

            handlers = handlers.on_key(move |event, ctx| {
                if let bastyde_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    use bastyde_core::event::Key;
                    let visible_count = source.visible_count();
                    if visible_count == 0 {
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    let current = fi.get().unwrap_or(0).min(visible_count - 1);

                    // Helper: scroll so flat row `idx` is visible in the tree's
                    // OWN viewport; returns the resulting scroll offset so the
                    // caller can chain the reveal to enclosing scroll areas.
                    let ensure_visible = |idx: usize| -> f32 {
                        let scroll = scroll_for_nav.get();
                        let new_scroll = metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                            idx,
                            scroll,
                            vh_for_nav.get(),
                            max_for_nav.get(),
                        );
                        if (new_scroll - scroll).abs() > f32::EPSILON {
                            scroll_for_nav.set(new_scroll);
                        }
                        new_scroll
                    };

                    // Ctrl+A: select all visible rows (Multi only).
                    if modifiers.ctrl() && matches!(key, Key::A) {
                        if let Some(ref sel) = sel_for_key
                            && sel.mode() == bastyde_data::SelectionMode::Multi
                        {
                            sel.select_all(visible_count);
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // Type-ahead: a printable char (no Ctrl/Alt/Super) jumps the
                    // selection to the next visible row whose label starts with
                    // the accumulated term. Opt-in via `type_ahead_label`.
                    if ta_label.is_some()
                        && !modifiers.ctrl()
                        && !modifiers.alt()
                        && !modifiers.super_key()
                        && let Some(c) = key.to_char()
                    {
                        let label = ta_label.as_ref().unwrap();
                        let source_ref = &source;
                        if let Some(idx) =
                            ta_state.search(c, current, visible_count, ta_timeout, |i| {
                                source_ref.with_row_str(i, &|item| label(item))
                            })
                        {
                            fi.set(Some(idx));
                            if let Some(ref sel) = sel_for_key {
                                sel.select(idx);
                            }
                            let new_scroll = ensure_visible(idx);
                            crate::common::row_metrics::chase_row_into_outer_view(
                                ctx,
                                &metrics_for_nav,
                                vb_for_nav.get(),
                                idx,
                                new_scroll,
                            );
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // Alt+Arrow: sibling reorder (when reorderable). Routed
                    // through the source's own `accept_drop` (cycle-guarded),
                    // which returns the moved row's new flat index.
                    if modifiers.alt() && reorderable {
                        let flat_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied())
                            .or(fi.get())
                            .unwrap_or(current);
                        let down = match key {
                            bastyde_core::event::Key::ArrowUp => false,
                            bastyde_core::event::Key::ArrowDown => true,
                            _ => return bastyde_core::event::EventResponse::Ignored,
                        };
                        if let Some(new_flat) = source.keyboard_reorder(flat_idx, down) {
                            fi.set(Some(new_flat));
                            if let Some(ref sel) = sel_for_key {
                                sel.select(new_flat);
                            }
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // ArrowRight: expand / ArrowLeft: collapse or move to parent
                    match key {
                        bastyde_core::event::Key::ArrowRight => {
                            if let Some(meta) = source.meta(current)
                                && meta.has_children
                                && !meta.is_expanded
                            {
                                source.set_expanded_at(current, true);
                                return bastyde_core::event::EventResponse::Handled;
                            }
                        }
                        bastyde_core::event::Key::ArrowLeft => {
                            if let Some(meta) = source.meta(current) {
                                if meta.is_expanded {
                                    source.set_expanded_at(current, false);
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                                // If leaf or collapsed, move to parent.
                                if let Some(parent_idx) = source.parent_index(current) {
                                    fi.set(Some(parent_idx));
                                    if let Some(ref sel) = sel_for_key {
                                        sel.select(parent_idx);
                                    }
                                    // Reveal the parent row (own viewport, then
                                    // any enclosing scroll area) like every
                                    // other focus-moving key.
                                    let new_scroll = ensure_visible(parent_idx);
                                    crate::common::row_metrics::chase_row_into_outer_view(
                                        ctx,
                                        &metrics_for_nav,
                                        vb_for_nav.get(),
                                        parent_idx,
                                        new_scroll,
                                    );
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                            }
                        }
                        _ => {}
                    }

                    // Navigation keys
                    let new_idx = match key {
                        Key::ArrowDown => Some((current + 1).min(visible_count - 1)),
                        Key::ArrowUp => Some(current.saturating_sub(1)),
                        Key::Home => Some(0),
                        Key::End => Some(visible_count - 1),
                        // Page keys: jump one viewport of rows by visual distance
                        // (variable heights honored), then ensure-visible scrolls.
                        Key::PageDown => {
                            let vh = vh_for_nav.get();
                            let r = {
                                let mut m = metrics_for_nav.borrow_mut();
                                m.resize(visible_count);
                                let target = m.row_top(current) + vh;
                                m.row_at(target)
                            };
                            Some(if r == current {
                                (current + 1).min(visible_count - 1)
                            } else {
                                r.min(visible_count - 1)
                            })
                        }
                        Key::PageUp => {
                            let vh = vh_for_nav.get();
                            let r = {
                                let mut m = metrics_for_nav.borrow_mut();
                                m.resize(visible_count);
                                let target = (m.row_top(current) - vh).max(0.0);
                                m.row_at(target)
                            };
                            Some(if r == current {
                                current.saturating_sub(1)
                            } else {
                                r
                            })
                        }
                        Key::Enter => {
                            // Enter activates the focused row (open / commit).
                            if let Some(ref sel) = sel_for_key {
                                sel.select(current);
                            }
                            if let Some(ref cb) = activate_key {
                                cb(current);
                            }
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        Key::Space => {
                            // Space moves/toggles the selection but does NOT
                            // activate (Enter is the activator). Multi: toggle;
                            // Single: select.
                            if let Some(ref sel) = sel_for_key {
                                if sel.mode() == bastyde_data::SelectionMode::Multi {
                                    sel.toggle(current);
                                } else {
                                    sel.select(current);
                                }
                            }
                            fi.set(Some(current));
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        _ => None,
                    };

                    if let Some(idx) = new_idx {
                        fi.set(Some(idx));
                        if let Some(ref sel) = sel_for_key {
                            if modifiers.shift() {
                                sel.extend_to(idx);
                            } else {
                                sel.select(idx);
                            }
                        }
                        let new_scroll = ensure_visible(idx);
                        crate::common::row_metrics::chase_row_into_outer_view(
                            ctx,
                            &metrics_for_nav,
                            vb_for_nav.get(),
                            idx,
                            new_scroll,
                        );
                        return bastyde_core::event::EventResponse::Handled;
                    }
                }
                bastyde_core::event::EventResponse::Ignored
            });
        }

        // --- DnD: register as drop target when reorderable OR accept foreign
        // rows. The source's `can_accept` decides per-hover whether the drop is
        // allowed (and a forbidden verdict shows no insertion line / highlight);
        // a foreign exported row that the source itself rejects can still be
        // accepted via the `accept_foreign_rows` sugar (shown as a plain
        // between-rows insertion — a foreign source has no Into/reparent
        // semantics). ---
        if self.export.is_drop_target(self.reorderable) {
            let my_view_id = self.tree_id;

            // Shared across hover / tick / leave: the visible row index under the
            // pointer + when first seen, for spring-loaded folder expansion.
            // Reset whenever the hovered row changes or the drag leaves.
            let hovered_row: Rc<Cell<Option<(usize, std::time::Instant)>>> =
                Rc::new(Cell::new(None));

            // ----- hover: geometry → (target, position) → source.can_accept -----
            let metrics_for_hover = self.metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let source_for_hover = self.source.clone();
            let feedback_for_hover = self.drop_feedback.clone();
            let hr_for_hover = hovered_row.clone();
            let export_for_hover = self.export.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                let vc = source_for_hover.visible_count();
                if vc == 0 {
                    feedback_for_hover.set(None);
                    hr_for_hover.set(None);
                    return DropFeedback::NoFeedback;
                }
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y + scroll;
                let (insertion_top, row_idx, row_top, row_h) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(vc);
                    let ins = m.insertion_index(content_y);
                    let r = m.row_at(content_y);
                    let insertion_top = m.row_top(ins);
                    let row_top = m.row_top(r);
                    let row_h = m.row_height(r);
                    (insertion_top, r, row_top, row_h)
                };
                // Spring-load tracking (dwell-to-expand the hovered branch).
                match hr_for_hover.get() {
                    Some((p, t)) if p == row_idx => hr_for_hover.set(Some((row_idx, t))),
                    _ => hr_for_hover.set(Some((row_idx, std::time::Instant::now()))),
                }
                // Drop position from Y within the row (top third Before / middle
                // Into / bottom After). The source's `can_accept` is the verdict
                // — a Reject shows NO line (the pre-commit forbidden affordance).
                let y_in_row = content_y - row_top;
                let third = (row_h / 3.0).max(f32::EPSILON);
                let drop_pos = if y_in_row < third {
                    DropPosition::Before
                } else if y_in_row > 2.0 * third {
                    DropPosition::After
                } else {
                    DropPosition::Into
                };
                // The source's verdict decides the *effective* position: a
                // `Redirect` (e.g. Into-a-leaf → After) overrides the raw zone.
                let effective = match (source_for_hover.dnd.can_accept_fn)(
                    payload, row_idx, drop_pos, my_view_id,
                ) {
                    DropResponse::Reject => {
                        // The source itself won't take this drop — fall back to
                        // the foreign-export sugar, shown as a plain between-rows
                        // insertion (a foreign source has no Into/reparent
                        // semantics to honor).
                        let foreign_ok =
                            export_for_hover.accepts_foreign_export(payload, my_view_id);
                        if !foreign_ok {
                            feedback_for_hover.set(None);
                            return DropFeedback::NoFeedback;
                        }
                        DropPosition::Before
                    }
                    DropResponse::Accept => drop_pos,
                    DropResponse::Redirect(p) => p,
                };
                if effective == DropPosition::Into {
                    // Drop *into* the hovered container → highlight its whole row.
                    let top = row_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Rect {
                        top,
                        height: row_h,
                        width: 400.0,
                    }));
                    DropFeedback::HighlightRect {
                        rect: Rect::new(0.0, top, 400.0, row_h),
                        color: bastyde_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25),
                    }
                } else {
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Line {
                        y: insertion_y,
                        width: 400.0,
                    }));
                    DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: 400.0,
                    }
                }
            });

            // ----- drop: re-derive (target, position), route to accept_drop -----
            let metrics_for_drop = self.metrics.clone();
            let scroll_for_drop = self.scroll_y.clone();
            let source_for_drop = self.source.clone();
            let feedback_for_drop = self.drop_feedback.clone();
            let export_for_drop = self.export.clone();
            let reorderable_for_drop = self.reorderable;
            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                feedback_for_drop.set(None);
                let vc = source_for_drop.visible_count();
                if vc == 0 {
                    return false;
                }
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y + scroll;
                let (row_idx, row_top, row_h, ins) = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(vc);
                    let r = m.row_at(content_y);
                    let ins = m.insertion_index(content_y);
                    (r, m.row_top(r), m.row_height(r), ins)
                };
                let y_in_row = content_y - row_top;
                let third = (row_h / 3.0).max(f32::EPSILON);
                let drop_pos = if y_in_row < third {
                    DropPosition::Before
                } else if y_in_row > 2.0 * third {
                    DropPosition::After
                } else {
                    DropPosition::Into
                };
                let is_same_view = payload
                    .get_typed::<RowDragData<T>>()
                    .is_some_and(|rd| rd.source == my_view_id);
                // Route the drop to the source's accept_drop first. A same-view
                // reorder/reparent only happens when the view is `reorderable`;
                // a foreign payload the source itself recognises is the
                // source's call.
                if (reorderable_for_drop || !is_same_view)
                    && (source_for_drop.dnd.accept_drop_fn)(&payload, row_idx, drop_pos, my_view_id)
                {
                    // Only suppress our OWN move-out for a genuine same-view drop.
                    if is_same_view {
                        export_for_drop.note_self_reorder();
                    }
                    return true;
                }
                // Otherwise, the shared foreign-receive sugar (peek-before-take):
                // accept exported rows from a different view/source without a
                // custom TreeDataSource, at the flat insertion index.
                export_for_drop.foreign_receive(&mut payload, my_view_id, ins, ctx)
            });

            // Clear insertion line + spring-load timer whenever the drag leaves.
            let feedback_for_leave = self.drop_feedback.clone();
            let hr_for_leave = hovered_row.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
                hr_for_leave.set(None);
            });

            // Per-frame tick: viewport-edge auto-scroll plus spring-loaded
            // folders. The tick fires regardless of pointer movement, so
            // edge-scroll and spring-open still progress when the hand is
            // stationary.
            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            let hr_for_tick = hovered_row.clone();
            let source_for_tick = self.source.clone();
            const SPRING_DELAY_MS: u64 = 700;
            handlers = handlers.on_drag_tick(move |pos, _ctx| {
                // --- 1. Edge auto-scroll ---
                const EDGE: f32 = 32.0;
                const MAX_VELOCITY: f32 = 12.0;
                let h = viewport_for_tick.get();
                let above = (EDGE - pos.y).max(0.0);
                let below = (pos.y - (h - EDGE)).max(0.0);
                let delta = if above > 0.0 {
                    -(above / EDGE) * MAX_VELOCITY
                } else if below > 0.0 {
                    (below / EDGE) * MAX_VELOCITY
                } else {
                    0.0
                };
                if delta.abs() > 0.01 {
                    let max = max_scroll_for_tick.get();
                    let new_y = (scroll_for_tick.get() + delta).clamp(0.0, max);
                    scroll_for_tick.set(new_y);
                }

                // --- 2. Spring-loaded folders ---
                if let Some((row_idx, first_seen)) = hr_for_tick.get() {
                    let elapsed_ms = first_seen.elapsed().as_millis() as u64;
                    let has_children = source_for_tick
                        .meta(row_idx)
                        .map(|m| m.has_children)
                        .unwrap_or(false);
                    if elapsed_ms >= SPRING_DELAY_MS
                        && has_children
                        && !source_for_tick.is_expanded_at(row_idx)
                    {
                        source_for_tick.set_expanded_at(row_idx, true);
                        // Reset so we don't keep re-firing on the same row.
                        hr_for_tick.set(None);
                    }
                }
            });
        }

        // --- Export completion: remove rows moved out to a FOREIGN target. The
        // handler fires on the drag source (this view's root id, the stable id
        // start_drag was given). A same-view reorder called
        // `export.note_self_reorder()`, so it is skipped here (already applied).
        //
        // FIXED (was a known limitation): move-out no longer resolves the
        // dragged rows from flat indices at completion time. `build_payload`
        // captures a stable-key removal thunk via `source.dnd.snapshot_out_fn`
        // at drag-start, so a Move that dwelled over a collapsing/expanding
        // folder mid-drag (spring-load auto-expand reshuffling flat indices)
        // still removes the correct node.
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        // Lazy: nudge the source to load the realized window, and fetch more
        // as the viewport nears the end (append-only sources).
        (self.source.dnd.request_window_fn)(start..end);
        if (self.source.dnd.can_fetch_more_fn)()
            && end + BUFFER_ITEMS >= self.source.visible_count()
        {
            (self.source.dnd.fetch_more_fn)();
        }
        let is_drag_source = self.export.is_drag_source(self.reorderable);
        let tree_id = self.tree_id;
        let self_id = ctx.self_id();
        let row_state_fn = self.source.dnd.row_state_fn.clone();
        // Establish this TreeView as the focus scope for the rows it builds, so
        // their `StandardItem`s read *its* keyboard focus deterministically
        // (rows may build before arena parenting is wired).
        ctx.begin_view_focus();
        for i in start..end {
            let selected = self
                .row_selection
                .as_ref()
                .map(|s| s.is_selected(i))
                .unwrap_or(false);
            // Row metadata (a11y level / expand state) from the source.
            let meta = self.source.meta(i);
            let item_has_children = meta.as_ref().is_some_and(|m| m.has_children);
            // A `Loading` row (data not yet resident) renders a placeholder
            // skeleton instead of being skipped, so the scrollbar and layout
            // stay stable while the window loads. A placeholder reports no
            // metadata, so the expand/drag wiring below is gated off.
            let row_widget = self
                .source
                .with_row(i, &|item, m| (self.row_delegate)(i, item, m, selected))
                .or_else(|| {
                    ((row_state_fn)(i) == RowState::Loading)
                        .then(crate::data_views::default_placeholder)
                });
            if let Some(widget) = row_widget {
                let inner_id = ctx.add_boxed(widget);
                let (level, position_1based, total_siblings, expanded_opt) =
                    if let Some(ref m) = meta {
                        let exp = if m.has_children {
                            Some(m.is_expanded)
                        } else {
                            None
                        };
                        let (pos, total) = self.source.sibling_pos(i);
                        (m.depth + 1, pos, total, exp)
                    } else {
                        (1, 1, 1, None)
                    };
                let child_id = ctx.add(crate::list_item_a11y::TreeItemWrapper::new(
                    inner_id,
                    level,
                    position_1based,
                    total_siblings,
                    expanded_opt,
                    selected,
                ));

                // Click handling: selection + expand/collapse for branch rows.
                {
                    let sel_click = self.row_selection.clone();
                    let click_index = i;
                    let source_click = self.source.clone();
                    let fi_click = self.focused_index.clone();
                    let has_children = item_has_children && self.row_click_expands;
                    // Deferred collapse: pressing an already-selected row keeps
                    // the whole (multi-)selection so it can be dragged; the
                    // collapse-to-single happens on release WITHOUT a drag.
                    let pending_collapse = Rc::new(Cell::new(false));

                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_pointer_event(move |event, ctx| match event {
                            bastyde_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: bastyde_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // The press belongs to an interactive child (the
                                // chevron, or an inline control) — toggling/acting
                                // is its job; don't also select the row. Clear any
                                // stale deferred-collapse (left by a prior drag
                                // whose PointerUp the drag machinery consumed) so
                                // it can't fire on this unrelated interaction. (This
                                // guards the no-selection-model branch below — the
                                // shared helper does the equivalent for its own
                                // branch.)
                                if ctx.press_claimed_by_interactive_child() {
                                    pending_collapse.set(false);
                                    return bastyde_core::event::EventResponse::Ignored;
                                }
                                // The shared deferred-select helper owns the
                                // press-claimed guard, Ctrl/Shift handling, and
                                // the defer-collapse-on-already-selected rule; it
                                // returns false (skip the nav-cursor move) when an
                                // interactive child claimed the press. Without a
                                // selection model there's nothing to defer — a
                                // plain click still moves the nav cursor.
                                let moved = match sel_click.as_ref() {
                                    Some(sel) => crate::data_views::deferred_select::on_down(
                                        sel,
                                        click_index,
                                        *modifiers,
                                        &pending_collapse,
                                        ctx,
                                    ),
                                    None => true,
                                };
                                if moved {
                                    // Move the keyboard-navigation cursor to the
                                    // clicked row so a subsequent Arrow keypress
                                    // steps from here — `focused_index` is the
                                    // arrow-nav origin (`fi.get().unwrap_or(0)`)
                                    // and is otherwise only written by the
                                    // keyboard handler, so without this a click
                                    // would select a row yet leave arrows
                                    // stepping from the stale keyboard cursor.
                                    fi_click.set(Some(click_index));
                                }
                                // Ignored lets the gesture arena also see the
                                // PointerDown so DragRecognizer can capture the
                                // press position and enable drag-to-reorder.
                                bastyde_core::event::EventResponse::Ignored
                            }
                            bastyde_core::event::WidgetEvent::PointerUp {
                                button: bastyde_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // A release on the chevron (or another interactive
                                // child) is handled by that child's own tap — don't
                                // also toggle from the row body.
                                if ctx.press_claimed_by_interactive_child() {
                                    return bastyde_core::event::EventResponse::Ignored;
                                }
                                // Reached only on a click WITHOUT a drag (an
                                // active drag consumes PointerUp). Collapse the
                                // deferred multi-selection to the clicked row.
                                if let Some(ref sel) = sel_click {
                                    crate::data_views::deferred_select::on_up(
                                        sel,
                                        click_index,
                                        &pending_collapse,
                                        ctx,
                                    );
                                }
                                // Expand/collapse fires on release so a drag
                                // gesture pre-empts it (once active_drag is
                                // set, PointerUp is routed to handle_drag_drop
                                // and never reaches this widget).
                                if has_children {
                                    source_click.toggle_at(click_index);
                                }
                                bastyde_core::event::EventResponse::Ignored
                            }
                            _ => bastyde_core::event::EventResponse::Ignored,
                        }),
                    );

                    // Row activation (open/commit) — a gesture, so it arbitrates
                    // against the reorder drag via the gesture arena (a click
                    // activates, a drag does not). `SingleClick` → `on_tap`,
                    // `DoubleClick` → `on_double_tap`; Enter/Space activates too
                    // (keyboard handler). Distinct from selection, which also
                    // moves on arrow navigation.
                    if let Some(ref cb) = self.on_activate {
                        let cb = cb.clone();
                        let activate_index = i;
                        let handlers = match self.activate_on {
                            crate::data_views::ActivateOn::SingleClick => {
                                HandlerSet::new().on_tap(move |tap, _ctx| {
                                    // A Ctrl/Shift click is a selection-extension
                                    // gesture (applied on PointerDown), not an
                                    // activation — suppress open/commit so a
                                    // multi-select click doesn't also fire the
                                    // activate callback. Mirrors the PointerDown
                                    // selection condition (`ctrl` toggles, `shift`
                                    // extends) so the two stay in lock-step.
                                    if tap.modifiers.ctrl() || tap.modifiers.shift() {
                                        return;
                                    }
                                    cb(activate_index)
                                })
                            }
                            crate::data_views::ActivateOn::DoubleClick => HandlerSet::new()
                                .on_double_tap(move |_tap, _ctx| cb(activate_index)),
                        };
                        ctx.apply_handlers(child_id, handlers);
                    }
                }

                // Drag handler when reorderable OR exportable, gated by the
                // source's transferable verdict (`drag`). Emits the public
                // `RowDragData<T>`; the source recovers the key + validates at
                // hover/drop. The floating preview re-invokes the row delegate.
                if is_drag_source && (self.source.dnd.drag_fn)(i) == DragEligibility::CanDrag {
                    let drag_view_id = tree_id;
                    let drag_self_id = self_id;
                    let row_delegate = self.row_delegate.clone();
                    let source_for_preview = self.source.clone();
                    let flat_idx = i;
                    let metrics_for_preview = self.metrics.clone();
                    // Export capture: the dragged set is selection-aware; the
                    // shared `RowExport` builds the payload (clones / MIME /
                    // Loading-filter / stash) when the view opted in.
                    let sel_for_drag = self.row_selection.clone();
                    let export_for_drag = self.export.clone();
                    let read_for_drag = self.source.read_item_fn.clone();
                    let snapshot_for_drag = self.source.dnd.snapshot_out_fn.clone();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_drag(move |phase, ctx| {
                            if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                                // Selection-aware dragged set: the whole
                                // selection when the pressed row is part of a
                                // multi-selection, else just the pressed row.
                                let rows: Vec<usize> = match sel_for_drag.as_ref() {
                                    Some(s) if s.is_selected(flat_idx) => {
                                        let mut v = s.selected_indices();
                                        v.sort_unstable();
                                        if v.len() <= 1 { vec![flat_idx] } else { v }
                                    }
                                    _ => vec![flat_idx],
                                };
                                let payload = export_for_drag.build_payload(
                                    drag_view_id,
                                    rows,
                                    &*read_for_drag,
                                    &snapshot_for_drag,
                                );
                                const PREVIEW_WIDTH: f32 = 240.0;
                                let h = metrics_for_preview.borrow_mut().row_height(flat_idx);
                                let rd = row_delegate.clone();
                                let preview_opt =
                                    source_for_preview.with_row(flat_idx, &move |item, m| {
                                        Box::new(crate::drag_preview::DragPreview::new(
                                            PREVIEW_WIDTH,
                                            h,
                                            rd(flat_idx, item, m, false),
                                        ))
                                            as Box<dyn Widget>
                                    });
                                if let Some(preview) = preview_opt {
                                    ctx.start_drag_with_preview(drag_self_id, payload, preview);
                                } else {
                                    ctx.start_drag(drag_self_id, payload);
                                }
                            }
                        }),
                    );
                }

                self.item_entries.push((i, child_id));
            }
        }
        ctx.end_view_focus();

        // --- Scrollbar ---
        let scrollbar = ScrollBar::new(
            ScrollBarOrientation::Vertical,
            self.scroll_y.clone(),
            self.max_scroll_y.clone(),
            self.viewport_ratio_y.clone(),
        )
        .visual(match self.scroll_bar_style {
            ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
            ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
            ScrollBarMode::Thin => ScrollBarVisual::Thin,
        });
        let sb_id = ctx.add(scrollbar);
        self.scrollbar_id = Some(sb_id);

        let mut children: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        children.push(sb_id);
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(300.0);
        let height = proposal.height.unwrap_or(200.0);
        self.viewport_height.set(height);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Cache our own absolute bounds for the keyboard handler's
        // outer-scroll chase (`ensure_visible`), before the empty-children bail.
        self.viewport_bounds.set(bounds);

        if children.is_empty() {
            return;
        }

        let viewport_height = bounds.height;
        let count = self.source.visible_count();
        let item_count = self.item_entries.len();
        // Permanent reserves a column for the bar; Overlay / Thin float
        // over the content, so rows span the full width.
        let reserves_bar = self.scroll_bar_style == ScrollBarMode::Permanent;
        let content_width = if reserves_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };

        // Auto-measure pass: measure every realized row at the content
        // width (height-for-width), feed the heights back, and apply the
        // scroll-anchor delta so content above the viewport stays put.
        // Measurements are collected with NO metrics borrow held.
        if self.metrics.borrow().needs_measure() {
            let mut measured = Vec::with_capacity(item_count);
            for (idx, child) in children.iter().enumerate() {
                if idx < item_count
                    && let Some(size) =
                        ctx.child_size(child.id, SizeProposal::with_width(content_width))
                {
                    let (flat_index, _) = self.item_entries[idx];
                    measured.push((flat_index, size.height));
                }
            }
            let anchor = self
                .metrics
                .borrow_mut()
                .observe_measured(&measured, self.scroll_y.get());
            if anchor.abs() > 0.01 {
                // Safe from place_children: the dirty flag is set but the
                // binding flush already ran this pass — lands next frame.
                self.scroll_y.set((self.scroll_y.get() + anchor).max(0.0));
            }

            // Realization re-check: corrected offsets may reveal viewport
            // rows the estimated offsets never realized. Request a
            // rebuild for next frame; the 0.01 measurement epsilon
            // guarantees convergence.
            let (vs, ve) = self.metrics.borrow_mut().visible_range(
                self.scroll_y.get(),
                viewport_height,
                count,
                0,
            );
            if vs < self.prev_built_start.get() || ve > self.prev_built_end.get() {
                self.prev_built_start.set(vs.saturating_sub(BUFFER_ITEMS));
                self.prev_built_end.set((ve + BUFFER_ITEMS).min(count));
                self.version.set(self.version.get() + 1);
            }
        }

        // Post-measure totals so even frame 1's scrollbar reflects the
        // measured window.
        let total_height = self.total_content_height();
        let max_y = (total_height - viewport_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let ratio = if total_height > 0.0 {
            (viewport_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio);
        self.clamp_scroll();

        let scroll_y = self.scroll_y.get();

        for (idx, child) in children.iter_mut().enumerate() {
            if idx < item_count {
                let (flat_index, _) = self.item_entries[idx];
                let (top, height) = {
                    let mut m = self.metrics.borrow_mut();
                    (m.row_top(flat_index), m.row_height(flat_index))
                };
                let y = bounds.y + top - scroll_y;
                child.origin = Point::new(bounds.x, y);
                child.size = Size::new(content_width, height);
            }
        }

        // Scrollbar
        if let Some(sb_child) = children.last_mut() {
            let needs_scrollbar = total_height > viewport_height + 0.5;
            if needs_scrollbar {
                sb_child.origin =
                    Point::new(bounds.x + bounds.width - SCROLLBAR_THICKNESS, bounds.y);
                sb_child.size = Size::new(SCROLLBAR_THICKNESS, bounds.height);
            } else {
                sb_child.origin = bounds.origin();
                sb_child.size = Size::ZERO;
            }
        }
    }

    fn paint(
        &self,
        bounds: Rect,
        canvas: &mut bastyde_canvas::Canvas,
        ctx: &bastyde_core::widget::PaintContext,
    ) {
        // Draw insertion line during drag hover — recipe-driven role +
        // thickness via `ListContainerStyle::insertion()`.
        if let Some(viz) = self.drop_feedback.get() {
            let recipe = ctx
                .theme
                .style_slots
                .list_container
                .as_ref()
                .map(|s| s.insertion())
                .unwrap_or_default();
            let color = recipe.role.resolve(&ctx.theme.colors);
            // Own paint isn't covered by `clips_children` — clip so feedback at
            // the after-last boundary can't bleed past the widget's bottom edge.
            canvas.set_clip(bounds);
            match viz {
                DropViz::Line { y, width } => {
                    let line_y = bounds.y + y;
                    let half = recipe.thickness * 0.5;
                    canvas.fill_rect(
                        Rect::new(bounds.x, line_y - half, width, recipe.thickness),
                        color,
                    );
                }
                DropViz::Rect { top, height, width } => {
                    // Into-container highlight: a translucent fill plus a solid
                    // outline at the insertion role's color.
                    let rect = Rect::new(bounds.x, bounds.y + top, width, height);
                    canvas.fill_rect(rect, color.with_alpha(0.18));
                    canvas.stroke_rect(rect, color, recipe.thickness.max(1.5));
                }
            }
            canvas.clear_clip();
        }

        // Container focus ring. When the view is Tab-focused (keyboard modality)
        // but nothing is selected, no row paints a ring — so outline the whole
        // view, giving the user a visible focus landing point before they arrow.
        // Once a row is selected its own ring takes over and this clears.
        let has_selection = self
            .row_selection
            .as_ref()
            .is_some_and(|s| !s.selected_indices().is_empty());
        if self.view_focused.get() && self.focus_visible.get() && !has_selection {
            let color = BorderRole::Focused.resolve(&ctx.theme.colors);
            let inset = 1.0_f32;
            let rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rect(rect, color, 1.5);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Tree);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        if let Some(sb) = self.scrollbar_id {
            ids.push(sb);
        }
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// Build a sample tree:
    /// A (has children: A1, A2)
    /// B (has children: B1)
    /// C (leaf)
    fn sample_tree() -> TreeModel<&'static str> {
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        tree.insert_child(a, 0, "A1");
        tree.insert_child(a, 1, "A2");
        let b = tree.insert_root(1, "B");
        tree.insert_child(b, 0, "B1");
        tree.insert_root(2, "C");
        tree
    }

    fn make_tree_view(tree: TreeModel<&'static str>) -> (WidgetTree, WidgetId) {
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, entry, _selected| {
                // Width encodes depth, height is fixed
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0),
        );
        (wtree, tv_id)
    }

    #[test]
    fn initial_shows_only_roots() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        // 3 root items + 1 scrollbar
        assert_eq!(children.len() - 1, 3);
    }

    #[test]
    fn insert_child_into_root_updates_view() {
        let tree = sample_tree();
        let a = tree.root(0);
        let (mut wtree, tv_id) = make_tree_view(tree.clone());
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Insert a new child under A — since A is collapsed, visible count stays 3
        tree.insert_child(a, 2, "A3");
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        // Still 3 visible (A collapsed), but the tree knows about A3
        assert_eq!(wtree.children(tv_id).len() - 1, 3);
    }

    #[test]
    fn model_mutation_triggers_rebuild() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree.clone());
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        tree.insert_root(3, "D");
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(wtree.children(tv_id).len() - 1, 4);
    }

    #[test]
    fn remove_triggers_rebuild() {
        let tree = sample_tree();
        let c = tree.root(2);
        let (mut wtree, tv_id) = make_tree_view(tree.clone());
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        tree.remove(c);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(wtree.children(tv_id).len() - 1, 2);
    }

    #[test]
    fn items_positioned_vertically() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        let y0 = wtree.bounds(children[0]).y;
        let y1 = wtree.bounds(children[1]).y;
        let y2 = wtree.bounds(children[2]).y;
        assert!((y0 - 0.0).abs() < 0.01);
        assert!((y1 - 28.0).abs() < 0.01);
        assert!((y2 - 56.0).abs() < 0.01);
    }

    #[test]
    fn virtualization_with_large_tree() {
        // Create a tree with 500 root nodes
        let tree = TreeModel::new();
        for i in 0..500 {
            tree.insert_root(i, format!("Node {}", i).leak() as &'static str);
        }
        let (mut wtree, tv_id) = make_tree_view(tree);
        // Viewport 300px, item height 28px → ~11 visible + 2*5 buffer = ~21
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let item_count = wtree.children(tv_id).len() - 1;
        assert!(
            item_count < 30,
            "Expected fewer than 30 items, got {}",
            item_count
        );
        assert!(
            item_count >= 10,
            "Expected at least 10 items, got {}",
            item_count
        );
    }

    #[test]
    fn scrollbar_collapses_when_not_needed() {
        let tree = sample_tree(); // 3 roots, 3*28=84 < 300
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        let sb = children.last().unwrap();
        let sb_bounds = wtree.bounds(*sb);
        assert!(
            sb_bounds.width < 0.01 && sb_bounds.height < 0.01,
            "Scrollbar should be collapsed"
        );
    }

    #[test]
    fn accessibility_role_is_tree() {
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        let info = wtree.accessibility_node(tv_id);
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Tree);
    }

    #[test]
    fn empty_tree() {
        let tree: TreeModel<&str> = TreeModel::new();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Only scrollbar
        assert_eq!(wtree.children(tv_id).len(), 1);
    }

    #[test]
    fn tree_item_has_a11y_role_and_expanded() {
        let tree = sample_tree(); // A (has children), B (has children), C (leaf)
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let children = wtree.children(tv_id);
        // First child (A) should be a TreeItemWrapper with TreeItem role
        let info_a = wtree.accessibility_node(children[0]);
        assert_eq!(info_a.role(), bastyde_core::accesskit::Role::TreeItem);
        // A has children and is collapsed → is_expanded returns false
        assert!(
            !info_a.is_expanded(),
            "Root A should report not expanded (collapsed)"
        );

        // Third child (C) is a leaf → also not expanded
        let info_c = wtree.accessibility_node(children[2]);
        assert_eq!(info_c.role(), bastyde_core::accesskit::Role::TreeItem);
        assert!(!info_c.is_expanded(), "Leaf C should not be expanded");
    }

    #[test]
    fn keyboard_arrow_down_navigates() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let tree = sample_tree(); // A, B, C (3 roots)
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, entry, _selected| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the TreeView
        wtree.focus(tv_id);

        // ArrowDown should select item 0 first (from no focus), then 1
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::NONE,
            text: None,
        });

        // focused_index starts at None → unwrap_or(0) → ArrowDown moves to 1
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "ArrowDown from initial state should select index 1 (second root)"
        );

        // Another ArrowDown should move to index 2
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "Second ArrowDown should select index 2 (third root)"
        );
    }

    #[test]
    fn arrow_nav_resumes_from_the_clicked_row() {
        // Regression: a row click must move the keyboard-navigation cursor
        // (`focused_index`) to the clicked row, so the next Arrow step continues
        // from there — not from the stale keyboard cursor / index 0. Rows are
        // 20px tall; row 3's body is at y≈70, x=100 (past any chevron column).
        use bastyde_core::event::{Key, Modifiers};
        let (mut wtree, tv, selection) = flat_tree_view(10, bastyde_data::SelectionMode::Single);
        wtree.layout(SizeProposal::exact(400.0, 300.0)); // 10 rows × 20px all visible
        wtree.focus(tv);

        // Click row 3 (selects it AND should set the nav cursor to 3).
        press_at(&mut wtree, 100.0, 70.0);
        assert_eq!(
            selection.selected_indices(),
            vec![3],
            "precondition: body click selects row 3"
        );

        // ArrowDown must step to 4 (from the clicked row), not to 1 (from index 0).
        wtree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![4],
            "ArrowDown after a click resumes from the clicked row (3 → 4)"
        );

        // And ArrowUp steps back above the clicked row (4 → 3).
        wtree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![3],
            "ArrowUp resumes (4 → 3)"
        );
    }

    /// A flat tree of `n` roots labelled "Node {i}", with a single-select model.
    fn flat_tree_view(
        n: usize,
        mode: bastyde_data::SelectionMode,
    ) -> (WidgetTree, WidgetId, bastyde_data::SelectionModel) {
        use bastyde_data::SelectionModel;
        let tree = TreeModel::new();
        for i in 0..n {
            tree.insert_root(i, format!("Node {i}"));
        }
        let selection = SelectionModel::new(mode);
        let sel = selection.clone();
        let mut wtree = WidgetTree::new();
        let tv = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(120.0, 20.0)))
                .item_height(20.0)
                .selection(sel)
                .type_ahead_label(|s: &String| s.clone())
                .on_activate(|_| {}),
        );
        (wtree, tv, selection)
    }

    #[test]
    fn page_down_up_moves_selection_by_viewport() {
        use bastyde_core::event::{Key, Modifiers};
        let (mut wtree, tv, selection) = flat_tree_view(100, bastyde_data::SelectionMode::Single);
        let p = SizeProposal::exact(400.0, 200.0); // ~10 rows
        wtree.layout(p);
        wtree.focus(tv);
        selection.select(0);

        wtree.press_key(Key::PageDown, Modifiers::NONE);
        wtree.layout(p);
        let after = selection.selected_indices()[0];
        assert!(after >= 8, "PageDown advances ~one viewport, got {after}");

        wtree.press_key(Key::PageUp, Modifiers::NONE);
        wtree.layout(p);
        assert!(
            selection.selected_indices()[0] < after,
            "PageUp moves selection back up"
        );
    }

    #[test]
    fn ctrl_a_selects_all_visible_in_multi_mode() {
        use bastyde_core::event::{Key, Modifiers};
        let (mut wtree, tv, selection) = flat_tree_view(7, bastyde_data::SelectionMode::Multi);
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        wtree.focus(tv);
        wtree.press_key(Key::A, Modifiers::CTRL);
        assert_eq!(selection.selected_indices().len(), 7, "Ctrl+A selects all");
    }

    #[test]
    fn space_toggles_enter_activates() {
        use bastyde_core::event::{Key, Modifiers};
        use std::cell::Cell;
        let tree = TreeModel::new();
        for i in 0..5 {
            tree.insert_root(i, format!("Node {i}"));
        }
        let selection = bastyde_data::SelectionModel::new(bastyde_data::SelectionMode::Multi);
        let sel = selection.clone();
        let activated = Rc::new(Cell::new(None));
        let act = activated.clone();
        let mut wtree = WidgetTree::new();
        let tv = wtree.add(
            TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
                .item_height(20.0)
                .selection(sel)
                .on_activate(move |i| act.set(Some(i))),
        );
        wtree.layout(SizeProposal::exact(400.0, 200.0));
        wtree.focus(tv);

        wtree.press_key(Key::ArrowDown, Modifiers::NONE); // → row 1
        assert_eq!(selection.selected_indices(), vec![1]);
        assert_eq!(activated.get(), None);

        wtree.press_key(Key::Space, Modifiers::NONE); // toggle row 1 OFF
        assert!(selection.selected_indices().is_empty(), "Space toggles off");
        assert_eq!(activated.get(), None, "Space must NOT activate");

        wtree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(activated.get(), Some(1), "Enter activates");
    }

    #[test]
    fn type_ahead_jumps_to_matching_visible_row() {
        use bastyde_core::event::{Key, Modifiers};
        let tree = TreeModel::new();
        tree.insert_root(0, "Apple".to_string());
        tree.insert_root(1, "Banana".to_string());
        tree.insert_root(2, "Cherry".to_string());
        tree.insert_root(3, "Date".to_string());
        let selection = bastyde_data::SelectionModel::new(bastyde_data::SelectionMode::Single);
        let sel = selection.clone();
        let mut wtree = WidgetTree::new();
        let tv = wtree.add(
            TreeView::new(tree, |_i, _e, _s| Box::new(FixedLeaf(120.0, 20.0)))
                .item_height(20.0)
                .selection(sel)
                .type_ahead_label(|s: &String| s.clone()),
        );
        wtree.layout(SizeProposal::exact(400.0, 200.0));
        wtree.focus(tv);
        selection.select(0);

        wtree.press_key(Key::C, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![2], "'c' → Cherry");
        wtree.press_key(Key::B, Modifiers::NONE);
        // "cb" matches nothing → selection unchanged.
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "'cb' no match, stays"
        );
    }

    // --- Chevron-vs-selection regression tests ---

    /// A `TreeView` whose rows are real `StandardTreeItem`s (with a live chevron)
    /// over `sample_tree()`, plus a single-select model. The chevron is the only
    /// toggle target (`row_click_expands(false)`), mirroring app usage.
    fn make_standard_tree_view() -> (WidgetTree, WidgetId, bastyde_data::SelectionModel) {
        use bastyde_data::{SelectionMode, SelectionModel};
        let tree = sample_tree();
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new_with_context(tree, |item: &&'static str, entry, selected, ctx| {
                Box::new(
                    crate::StandardTreeItem::new(lit!((*item).to_string()))
                        .from_entry(entry)
                        .selected(selected)
                        .on_toggle_rc(ctx.toggle_callback()),
                ) as Box<dyn Widget>
            })
            .item_height(28.0)
            .selection(sel_clone)
            .row_click_expands(false),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        (wtree, tv_id, selection)
    }

    fn press_at(w: &mut WidgetTree, x: f32, y: f32) {
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        w.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(x, y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        w.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(x, y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    #[test]
    fn chevron_press_toggles_without_selecting_the_row() {
        // Regression: pressing the expand chevron must toggle the subtree but
        // NOT select the row. The row's select-on-press handler yields to the
        // chevron's own tap via `ctx.press_claimed_by_interactive_child()`.
        let (mut wtree, tv_id, selection) = make_standard_tree_view();
        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "precondition: 3 collapsed roots"
        );

        // Row A is depth 0 (indent 0); the chevron column is x in [0, 16].
        press_at(&mut wtree, 8.0, 14.0);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            5,
            "chevron press should expand A, revealing A1 and A2"
        );
        assert!(
            selection.selected_indices().is_empty(),
            "chevron press must not select the row (got {:?})",
            selection.selected_indices()
        );
    }

    #[test]
    fn body_press_selects_the_row() {
        // Companion: pressing the row BODY (past the chevron column) still
        // selects, and does not expand when row_click_expands=false.
        let (mut wtree, tv_id, selection) = make_standard_tree_view();

        press_at(&mut wtree, 100.0, 14.0);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "body press should select row 0"
        );
        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "body press must not expand when row_click_expands=false"
        );
    }

    /// Like [`make_standard_tree_view`] (real `StandardTreeItem` chevrons) but
    /// **reorderable**, so each row also owns a drag recognizer — the exact shape
    /// where a chevron tap and an ancestor row drag compete.
    fn make_reorderable_standard_tree_view() -> (WidgetTree, WidgetId) {
        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new_with_context(tree, |item: &&'static str, entry, selected, ctx| {
                Box::new(
                    crate::StandardTreeItem::new(lit!((*item).to_string()))
                        .from_entry(entry)
                        .selected(selected)
                        .on_toggle_rc(ctx.toggle_callback()),
                ) as Box<dyn Widget>
            })
            .item_height(28.0)
            .row_click_expands(false)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        (wtree, tv_id)
    }

    #[test]
    fn chevron_tap_with_jitter_toggles_in_a_reorderable_tree() {
        // Regression: the expand chevron sits inside a reorderable row that owns a
        // drag recognizer. Tap and drag share a 5px threshold — a tap fails only
        // once movement is *strictly* past 5px, while a drag arms at exactly 5px.
        // So a press that drifts to exactly the threshold is still a valid tap,
        // yet — unless the chevron is a gesture dead zone — that drift arms the
        // ancestor row drag, which steals the gesture: the toggle never fires and
        // a row drag starts instead (the "click chevron → new drag" bug). With the
        // dead zone, the ancestor drag is never armed, so the tap wins and toggles.
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        let (mut wtree, tv_id) = make_reorderable_standard_tree_view();
        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "precondition: 3 collapsed roots"
        );

        // Row A is depth 0; the chevron column is x in [0, 16], row y in [0, 28].
        // Down, drift to exactly 5px (arms an ancestor drag but keeps the tap
        // alive), then release back within tolerance — a valid tap that the drag
        // must not steal.
        wtree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(8.0, 10.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        wtree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(8.0, 15.0), // exactly 5px from down
        });
        wtree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(8.0, 13.0), // 3px from down → within tap tolerance
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            5,
            "the chevron tap must expand A (revealing A1, A2), not start a row drag"
        );
    }

    // --- Drag-and-drop integration tests ---

    /// Run a full drag gesture: PointerDown on source, Move to cross threshold,
    /// Move to target, Up. Mirrors `list_view::tests::drag_item`.
    fn drag_item(tree: &mut WidgetTree, from: Point, to: Point) {
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(from.x + 10.0, from.y),
        });
        tree.dispatch_event(WidgetEvent::PointerMove { position: to });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// Build a reorderable TreeView at the tree root with three top-level
    /// nodes A (collapsed, with A1/A2 children), B (collapsed, with B1), C
    /// (leaf). Item height is 28px, so rows are at y=0..28, 28..56, 56..84.
    fn make_reorderable_tree_view() -> (
        WidgetTree,
        WidgetId,
        TreeModel<&'static str>,
        NodeId,
        NodeId,
        NodeId,
    ) {
        let model = sample_tree();
        let a = model.root(0);
        let b = model.root(1);
        let c = model.root(2);
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model.clone(), |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .reorderable(true),
        );
        (wtree, tv_id, model, a, b, c)
    }

    #[test]
    fn drag_reorders_root_before() {
        // Drag C (row 2, y=56..84) to the top third of row 0 (before A).
        let (mut wtree, _tv_id, model, a, _b, c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 2.0));

        // After move: C becomes root 0, A shifts to root 1.
        assert_eq!(model.root(0), c, "C should be first root");
        assert_eq!(model.root(1), a, "A should be second root");
    }

    #[test]
    fn drag_reorders_root_after() {
        // Drag B (row 1, y=28..56) to the bottom third of row 2 (after C).
        let (mut wtree, _tv_id, model, _a, b, c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        drag_item(&mut wtree, Point::new(50.0, 42.0), Point::new(50.0, 80.0));

        // After move: order is [A, C, B]
        assert_eq!(model.root_count(), 3);
        assert_eq!(model.root(1), c, "C should shift up to root 1");
        assert_eq!(model.root(2), b, "B should land at root 2");
    }

    #[test]
    fn drag_reparents_into_target() {
        // Drag C (row 2) into the middle third of row 0 (into A as last child —
        // drop-into appends, the standard folder convention).
        let (mut wtree, _tv_id, model, a, _b, c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Middle third of a 28px row is [9.33, 18.67]. Use y=14.
        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 14.0));

        // C should now be A's last child (A's existing children were A1, A2).
        let a_children = model.children(a);
        assert_eq!(a_children.len(), 3, "A should have three children");
        assert_eq!(a_children[2], c, "C should be A's last child");
        // C is no longer a root.
        assert_eq!(model.root_count(), 2);
    }

    #[test]
    fn drag_into_reparents_the_node() {
        // Drag C onto the middle third of A's row → C is reparented under A
        // (the move is applied via the cycle-guarded reorder helper).
        let model = sample_tree();
        let a = model.root(0);
        let c = model.root(2);
        let mut wtree = WidgetTree::new();
        let _tv = wtree.add(
            TreeView::new(model.clone(), |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag C (root 2, y≈70) into the middle third of row 0 ("into A").
        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 14.0));

        assert_eq!(model.root_count(), 2, "C is no longer a root");
        assert_eq!(model.parent(c), Some(a), "C is now a child of A");
    }

    #[test]
    fn drag_into_own_descendant_is_refused_without_panicking() {
        // The cycle guard: dragging A into its own child A1 must be refused —
        // no move, and (critically) no panic in TreeModel::move_node.
        let model = sample_tree();
        let a = model.root(0);
        let a1 = model.children(a)[0];
        let tv = TreeView::new(model.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .reorderable(true);
        // Expand A so A1 is a visible row before the drag.
        tv.expand(a);
        let mut wtree = WidgetTree::new();
        let _tv = wtree.add(tv);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Rows: A(0), A1(1), A2(2), B(3), C(4). Drag A (row 0, y≈14) into the
        // middle third of A1 (row 1, y≈42).
        drag_item(&mut wtree, Point::new(50.0, 14.0), Point::new(50.0, 42.0));

        // Refused: A is still a root, A1 still A's child. No panic occurred.
        assert_eq!(model.root_count(), 3, "A unchanged (cycle refused)");
        assert_eq!(model.parent(a1), Some(a), "A1 still under A");
    }

    #[test]
    fn drag_emits_node_moved_change() {
        use bastyde_data::TreeChange;
        use std::cell::Cell;
        use std::rc::Rc;

        let (mut wtree, _tv_id, model, _a, b, _c) = make_reorderable_tree_view();
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let emitted = Rc::new(Cell::new(false));
        let e = emitted.clone();
        let moved_node = Rc::new(Cell::new(None::<NodeId>));
        let mn = moved_node.clone();
        let handle = model.observe_changes(move |change| {
            if let TreeChange::NodeMoved { node, .. } = change {
                e.set(true);
                mn.set(Some(*node));
            }
        });

        // Drag B up — before A.
        drag_item(&mut wtree, Point::new(50.0, 42.0), Point::new(50.0, 2.0));

        assert!(emitted.get(), "TreeChange::NodeMoved should be emitted");
        assert_eq!(moved_node.get(), Some(b));
        drop(handle);
    }

    #[test]
    fn click_on_branch_with_nested_delegate_expands() {
        // Like click_on_branch_expands_and_collapses, but the delegate
        // builds a nested subtree (ZStack + Padding + HStack + Texts +
        // Spacer) so the pointer hit-target is a deep leaf, NOT the
        // TreeItemWrapper. Regression for the case where the wrapper's
        // on_pointer_event has to route through the preview/bubble path
        // to fire toggle_expand.
        use crate::RectWidget;
        use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};

        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |name, entry, selected| {
                let arrow: &'static str = if entry.has_children {
                    if entry.is_expanded { "v" } else { ">" }
                } else {
                    " "
                };
                let bg = if selected {
                    bastyde_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25)
                } else {
                    bastyde_tokens::Color::TRANSPARENT
                };
                Box::new(
                    ZStack::new().child(RectWidget::new().background(bg)).child(
                        Padding::symmetric(4.0, 12.0).child(
                            HStack::new()
                                .spacing(8.0)
                                .child(TextWidget::new(lit!(arrow)))
                                .child(TextWidget::new(lit!(name.to_string())))
                                .child(Spacer::new()),
                        ),
                    ),
                )
            })
            .item_height(28.0),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Sanity check: 3 roots visible.
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Click A (row 0). Use the wrapper's bounds center — hit_test will
        // walk down to whatever deep leaf is at that point.
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            5,
            "Click on A (branch) should expand it even with a nested delegate"
        );
    }

    #[test]
    fn drag_with_nested_delegate_still_works() {
        // Same nested delegate as above, but exercising drag. Regression
        // for the real-app scenario where the pointer hit-target is a
        // deep leaf (TextWidget) and the wrapper holding the gesture
        // arena + on_drag is an ancestor.
        use crate::RectWidget;
        use crate::primitives::{HStack, Padding, Spacer, TextWidget, ZStack};

        let tree = sample_tree();
        let a = tree.root(0);
        let c = tree.root(2);
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |name, _entry, _sel| {
                Box::new(
                    ZStack::new()
                        .child(RectWidget::new().background(bastyde_tokens::Color::TRANSPARENT))
                        .child(
                            Padding::symmetric(4.0, 12.0).child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(TextWidget::new(lit!(name.to_string())))
                                    .child(Spacer::new()),
                            ),
                        ),
                )
            })
            .item_height(28.0)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Drag C (row 2, y=70) to the top third of row 0 (y=2) → drop-before A.
        drag_item(&mut wtree, Point::new(50.0, 70.0), Point::new(50.0, 2.0));

        assert_eq!(tree.root(0), c, "C should be first root after drag");
        assert_eq!(tree.root(1), a, "A should shift to second root");
    }

    #[test]
    fn click_on_branch_expands_and_collapses() {
        // Click a folder-with-children and verify its subtree appears; click
        // again and verify it collapses. Regression test for the previous
        // on_pointer_event double-dispatch bug that toggled expand twice per
        // click (net no-op).
        let tree = sample_tree();
        let (mut wtree, tv_id) = make_tree_view(tree);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Initially collapsed — 3 roots visible.
        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Click A (row 0, center y=14).
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // A should now be expanded, showing its two children A1, A2.
        assert_eq!(
            wtree.children(tv_id).len() - 1,
            5,
            "After clicking A, its two children should become visible"
        );

        // Click A again — collapses.
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "Second click should collapse A back to 3 visible roots"
        );
    }

    #[test]
    fn row_click_expands_false_disables_auto_toggle() {
        // With `.row_click_expands(false)` set, clicking a branch
        // row's body must NOT toggle its expansion. This is the
        // contract used by `StandardTreeItem`, which provides its
        // own chevron tap target — without this opt-out, body clicks
        // would still toggle (and chevron clicks would toggle twice,
        // cancelling out).
        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, entry, _selected| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .row_click_expands(false),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(wtree.children(tv_id).len() - 1, 3);

        // Click A (a branch with children). Body click should NOT
        // expand it.
        let children = wtree.children(tv_id);
        wtree.click(children[0]);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        assert_eq!(
            wtree.children(tv_id).len() - 1,
            3,
            "row body click on a branch must not auto-expand when row_click_expands=false"
        );
    }

    #[test]
    fn spring_loaded_folder_expands_after_dwell() {
        // Drag a leaf over a collapsed folder and hold. After the dwell
        // delay (SPRING_DELAY_MS = 700 real ms), the folder should
        // auto-expand. Test drives real wall-clock time via `sleep` —
        // it's slow but accurate. Runs in ~750 ms; still headless.
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        use std::thread::sleep;
        use std::time::Duration;

        let tree = sample_tree(); // A (A1 A2), B (B1), C (leaf)
        let a = tree.root(0);
        let b = tree.root(1);
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Start a drag on C (y=70, row 2), then hover over B (row 1, y=42).
        wtree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 70.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        wtree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 70.0),
        });
        wtree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(60.0, 42.0),
        });

        // Confirm B is currently collapsed.
        assert!(tree.with_item(b, |_| ()).is_some());
        assert_eq!(
            wtree.children(_tv_id).len() - 1,
            3,
            "Precondition: 3 visible roots, nothing expanded"
        );

        // Wait past the 700 ms spring delay, then drive a layout tick
        // so on_drag_tick fires and the elapsed check passes.
        sleep(Duration::from_millis(750));
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // B should now be expanded, revealing B1 (4 visible rows).
        assert_eq!(
            wtree.children(_tv_id).len() - 1,
            4,
            "B should have spring-opened after the dwell"
        );

        // A was never hovered — still collapsed.
        assert!(!wtree.children(_tv_id).is_empty());
        let _ = a;

        // Clean up drag.
        wtree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(60.0, 42.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    // --- Alt+Arrow keyboard reorder test ---

    #[test]
    fn alt_arrow_reorders_flat_root_sibling() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let model = sample_tree(); // A, B, C (3 roots)
        let _a = model.root(0);
        let _b = model.root(1);
        let _c = model.root(2);
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model_clone, move |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the TreeView and select the middle item (B)
        wtree.focus(tv_id);
        wtree.click(wtree.children(tv_id)[1]); // B at index 1
        assert_eq!(selection.selected_indices(), vec![1]);

        // Press Alt+ArrowUp: B should move above A
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // After move: the roots should be reordered as B, A, C
        let new_roots: Vec<NodeId> = (0..model.root_count()).map(|i| model.root(i)).collect();
        assert_eq!(
            model.with_item(new_roots[0], |&v| v),
            Some("B"),
            "B should now be first root"
        );
        assert_eq!(
            model.with_item(new_roots[1], |&v| v),
            Some("A"),
            "A should now be second root"
        );
        // Selection should follow the moved node
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "Selection should now be at index 0 (B moved to top)"
        );

        // Press Alt+ArrowDown on B: B should move below A
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });

        // After move: order should be A, B, C again
        let new_roots: Vec<NodeId> = (0..model.root_count()).map(|i| model.root(i)).collect();
        assert_eq!(
            model.with_item(new_roots[0], |&v| v),
            Some("A"),
            "A should be back at first root"
        );
        assert_eq!(
            model.with_item(new_roots[1], |&v| v),
            Some("B"),
            "B should be back at second root"
        );
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "Selection should be back at index 1"
        );
    }

    #[test]
    fn alt_arrow_reorders_nested_sibling() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        // Tree: A with children A1, A2 (in that order)
        let tree = TreeModel::new();
        let a = tree.insert_root(0, "A");
        let _a1 = tree.insert_child(a, 0, "A1");
        let _a2 = tree.insert_child(a, 1, "A2");
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model = tree.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model, |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the TreeView so ArrowRight expands the focused node (A)
        wtree.focus(tv_id);

        // Expand A so children are visible
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::NONE,
            text: None,
        });
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Select A2 (flat index 2: A at 0, A1 at 1, A2 at 2)
        let children = wtree.children(tv_id);
        wtree.click(children[2]);
        assert_eq!(selection.selected_indices(), vec![2]);

        // Press Alt+ArrowUp: A2 should move above A1
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });
        // After move, relayout to refresh the tree view
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Check model: A2 should now be at index 0 under A, A1 at index 1
        let children_of_a = tree.children(a);
        assert_eq!(children_of_a.len(), 2, "A should still have 2 children");
        assert_eq!(
            tree.with_item(children_of_a[0], |&v| v),
            Some("A2"),
            "A2 should now be first child of A"
        );
        assert_eq!(
            tree.with_item(children_of_a[1], |&v| v),
            Some("A1"),
            "A1 should now be second child of A"
        );

        // Selection should now be at flat index 1 (A2 moved up, now at position 1)
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "Selection should follow A2 to flat index 1"
        );
    }

    #[test]
    fn alt_arrow_cannot_move_past_boundaries() {
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let model = sample_tree(); // A, B, C (3 roots)
        let selection = SelectionModel::new(SelectionMode::Single);
        let sel_clone = selection.clone();
        let model_clone = model.clone();

        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(model_clone, move |_item, entry, _sel| {
                Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
            })
            .item_height(28.0)
            .selection(sel_clone)
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus and select first item (A)
        wtree.focus(tv_id);
        wtree.click(wtree.children(tv_id)[0]);

        let a = model.root(0);
        let c = model.root(2);

        // Alt+ArrowUp on first item should do nothing
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowUp,
            modifiers: Modifiers::ALT,
            text: None,
        });
        assert_eq!(
            model.with_item(a, |&v| v),
            Some("A"),
            "A should still be first after Alt+Up on first item"
        );

        // Select last item (C)
        wtree.click(wtree.children(tv_id)[2]);

        // Alt+ArrowDown on last item should do nothing
        wtree.dispatch_event(bastyde_core::event::WidgetEvent::KeyDown {
            key: Key::ArrowDown,
            modifiers: Modifiers::ALT,
            text: None,
        });
        assert_eq!(
            model.with_item(c, |&v| v),
            Some("C"),
            "C should still be last after Alt+Down on last item"
        );
    }

    // -- Boundary scroll chaining -------------------------------------------

    /// A TreeView of 40 flat roots (20px each → 800px) in a 100px viewport,
    /// above a filler inside an outer ScrollArea. TreeView doesn't expose its
    /// scroll signal, so chaining is observed via the outer: the inner
    /// absorbing the first (huge) scroll leaves the outer at 0 (the
    /// anti-trivial guard), and the clamped second scroll then moves the
    /// outer under `Chain` but not under `Contain`.
    fn nested_tree_fixture(inner: OverscrollBehavior) -> (WidgetTree, Signal<f32>) {
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, VStack};
        let model = TreeModel::new();
        for i in 0..40 {
            model.insert_root(i, i as i32);
        }
        let mut tree = WidgetTree::new();
        let tv = TreeView::new(model, |_item: &i32, _entry, _sel| {
            Box::new(FixedLeaf(180.0, 20.0))
        })
        .item_height(20.0)
        .overscroll_behavior(inner);
        let tv_id = tree.add(tv);
        let viewport = tree.add(FixedSize::new().width(200.0).height(100.0).child_id(tv_id));
        let filler = tree.add(FixedLeaf(200.0, 200.0));
        let outer_content = tree.add(VStack::new().add_child(viewport).add_child(filler));
        let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let _outer = tree.add(outer);
        tree.layout(SizeProposal::exact(200.0, 150.0));
        (tree, outer_y)
    }

    #[test]
    fn nested_tree_chains_to_outer_at_boundary() {
        use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, outer_y) = nested_tree_fixture(OverscrollBehavior::Chain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        // The inner tree absorbed the big scroll (didn't chain) → outer at 0.
        assert!(
            outer_y.get() < 0.01,
            "outer must not move while the inner absorbs"
        );

        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        assert!(
            outer_y.get() > 0.01,
            "outer scrolled because the clamped tree chained"
        );
    }

    #[test]
    fn nested_tree_contain_blocks_chaining() {
        use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, outer_y) = nested_tree_fixture(OverscrollBehavior::Contain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal::exact(200.0, 150.0));
        assert!(
            outer_y.get() < 0.01,
            "Contain must prevent chaining: outer stays put"
        );
    }

    #[test]
    fn keyboard_selection_chases_outer_scroll_area() {
        // A 200px TreeView (20px rows) whose lower half is below a 100px outer
        // ScrollArea's fold. Arrow-key navigation keeps focus on the container
        // (rows aren't focusable), so the focus-driven follow can't reveal the
        // selected row — ctx.ensure_visible must.
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, VStack};
        use bastyde_core::event::{Key, Modifiers};

        let model = TreeModel::new();
        for i in 0..20 {
            model.insert_root(i, i as i32);
        }
        let mut tree = WidgetTree::new();
        let tv = TreeView::new(model, |_item: &i32, _entry, _sel| {
            Box::new(FixedLeaf(180.0, 20.0))
        })
        .item_height(20.0);
        let tv_id = tree.add(tv);
        let tv_box = tree.add(FixedSize::new().width(200.0).height(200.0).child_id(tv_id));
        let filler = tree.add(FixedLeaf(200.0, 200.0));
        let outer_content = tree.add(VStack::new().add_child(tv_box).add_child(filler));
        let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let _outer = tree.add(outer);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(tv_id);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        outer_y.set(0.0);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert!(outer_y.get().abs() < 0.01, "reset outer to top");

        for _ in 0..20 {
            tree.press_key(Key::ArrowDown, Modifiers::NONE);
        }
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert!(
            outer_y.get() > 0.01,
            "arrow-navigating below the fold must scroll the enclosing ScrollArea (got {})",
            outer_y.get()
        );
    }

    // --- Variable row heights ---

    /// Collect the (y, height) bounds of the realized rows (the
    /// scrollbar is always the last child), sorted by y.
    fn row_spans(tree: &WidgetTree, tv_id: WidgetId) -> Vec<(f32, f32)> {
        let children = tree.children(tv_id);
        let mut spans: Vec<(f32, f32)> = children[..children.len() - 1]
            .iter()
            .map(|c| {
                let b = tree.bounds(*c);
                (b.y, b.height)
            })
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        spans
    }

    #[test]
    fn exact_item_height_fn_positions_tree_rows() {
        let tree = sample_tree();
        let heights = [60.0_f32, 20.0, 40.0];
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 28.0)))
                .item_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0)),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = row_spans(&wtree, tv_id);
        assert_eq!(spans.len(), 3);
        assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 60.0).abs() < 0.01);
        assert!((spans[1].0 - 60.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
        assert!((spans[2].0 - 80.0).abs() < 0.01 && (spans[2].1 - 40.0).abs() < 0.01);
    }

    #[test]
    fn auto_measure_tree_rows_at_measured_heights() {
        // Delegate rows are 30 px tall; estimate says 50 → row 1 must
        // settle at y = 30.
        let tree = sample_tree();
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .auto_item_height(50.0),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = row_spans(&wtree, tv_id);
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "row 1 should sit at measured 30, got {}",
            spans[1].0
        );
    }

    #[test]
    fn expand_preserves_measured_heights_above_toggle() {
        // Rows measure 30 (estimate 50). Expanding B (flat index 1) must
        // keep A's measured height — row B stays at y = 30, it doesn't
        // snap back to the estimate.
        let tree = sample_tree();
        let b = tree.root(1);
        let mut wtree = WidgetTree::new();
        let tv_id = wtree.add(
            TreeView::new(tree, |_item, _entry, _sel| Box::new(FixedLeaf(100.0, 30.0)))
                .auto_item_height(50.0)
                .row_click_expands(false),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        wtree
            .widget_as_any(tv_id)
            .and_then(|any| any.downcast_ref::<TreeView<&'static str>>())
            .expect("TreeView exposes itself via as_any")
            .expand(b);
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        let spans = row_spans(&wtree, tv_id);
        assert_eq!(spans.len(), 4); // A, B, B1, C
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "A's measured height must survive the expand below it, got {}",
            spans[1].0
        );
    }

    #[test]
    fn drop_zone_thirds_with_variable_heights() {
        // Roots A (60 px), B (20 px), C (40 px), reorderable. Dropping C
        // in the top third of the SHORT row B (y ∈ 60..~66) must insert
        // it before B — uniform math would misattribute that y band.
        let tree = TreeModel::new();
        tree.insert_root(0, "A");
        tree.insert_root(1, "B");
        tree.insert_root(2, "C");
        let heights = [60.0_f32, 20.0, 40.0];
        let mut wtree = WidgetTree::new();
        let _tv_id = wtree.add(
            TreeView::new(tree.clone(), |_item, _entry, _sel| {
                Box::new(FixedLeaf(100.0, 28.0))
            })
            .item_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0))
            .reorderable(true),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        // C spans 80..120; grab its center. Drop at y = 62: row B's top
        // third (60..60+20/3).
        drag_item(&mut wtree, Point::new(50.0, 100.0), Point::new(50.0, 62.0));

        let order: Vec<&str> = (0..tree.root_count())
            .map(|i| tree.with_item(tree.root(i), |v| *v).unwrap())
            .collect();
        assert_eq!(order, vec!["A", "C", "B"]);
    }

    #[test]
    fn keyed_selection_tracks_identity_and_prunes_deleted() {
        // Keyed (identity) selection by NodeId: selecting nodes stores their
        // ids, and deleting a node prunes only that key on the next slice
        // change — the other selected node survives.
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let model = sample_tree();
        let a = model.root(0);
        let a1 = model.children(a)[0];
        let b = model.root(1);
        let b1 = model.children(b)[0];
        let keyed = KeyedSelectionModel::<NodeId>::new(SelectionMode::Multi);
        let mut wtree = WidgetTree::new();
        wtree.add(
            TreeView::new(model.clone(), |_item, _entry, _sel| {
                Box::new(FixedLeaf(100.0, 28.0))
            })
            .item_height(28.0)
            .keyed_selection(keyed.clone()),
        );
        wtree.layout(SizeProposal::exact(400.0, 300.0));

        keyed.select(a1);
        keyed.toggle(b1);
        assert!(keyed.is_selected(&a1) && keyed.is_selected(&b1));

        // Delete A1 → the slice reflattens (version bump) and prune drops the
        // orphaned key; B1 (still present) survives.
        model.remove(a1);
        assert!(
            !keyed.is_selected(&a1),
            "deleted node is pruned from selection"
        );
        assert!(keyed.is_selected(&b1), "surviving node stays selected");
    }

    // ── Generic `TreeDataSource` path (Stage 8a) ─────────────────────────────
    // An external source of truth keyed on `i64` (an entity id), driving a
    // `TreeView<String>` with NO `TreeModel` mirror — the designer's case.

    struct MockNode {
        id: i64,
        parent: Option<i64>,
        label: String,
    }

    /// Minimal in-memory `TreeDataSource<Item = String, Key = i64>`. Nodes are
    /// stored in pre-order; visibility is derived from the expand set.
    struct MockI64Source {
        nodes: RefCell<Vec<MockNode>>,
        expanded: RefCell<std::collections::HashSet<i64>>,
        version: Signal<u64>,
        accept_log: RefCell<Vec<(i64, i64, DropPosition)>>,
    }

    impl MockI64Source {
        fn new() -> Self {
            // root1(1) { a(2), b(3) }   root2(4)
            let nodes = vec![
                MockNode {
                    id: 1,
                    parent: None,
                    label: "root1".into(),
                },
                MockNode {
                    id: 2,
                    parent: Some(1),
                    label: "a".into(),
                },
                MockNode {
                    id: 3,
                    parent: Some(1),
                    label: "b".into(),
                },
                MockNode {
                    id: 4,
                    parent: None,
                    label: "root2".into(),
                },
            ];
            Self {
                nodes: RefCell::new(nodes),
                expanded: RefCell::new([1, 4].into_iter().collect()),
                version: Signal::new(0),
                accept_log: RefCell::new(Vec::new()),
            }
        }
        fn parent_of(&self, id: i64) -> Option<i64> {
            self.nodes
                .borrow()
                .iter()
                .find(|n| n.id == id)
                .and_then(|n| n.parent)
        }
        fn exists(&self, id: i64) -> bool {
            self.nodes.borrow().iter().any(|n| n.id == id)
        }
        fn is_descendant(&self, node: i64, ancestor: i64) -> bool {
            let mut cur = Some(node);
            while let Some(c) = cur {
                if c == ancestor {
                    return true;
                }
                cur = self.parent_of(c);
            }
            false
        }
        fn visible_ids(&self) -> Vec<i64> {
            let nodes = self.nodes.borrow();
            let expanded = self.expanded.borrow();
            nodes
                .iter()
                .filter(|n| {
                    // Visible iff every ancestor is expanded.
                    let mut cur = n.parent;
                    while let Some(p) = cur {
                        if !expanded.contains(&p) {
                            return false;
                        }
                        cur = nodes.iter().find(|m| m.id == p).and_then(|m| m.parent);
                    }
                    true
                })
                .map(|n| n.id)
                .collect()
        }
        fn depth_of(&self, id: i64) -> usize {
            let mut d = 0;
            let mut cur = self.parent_of(id);
            while let Some(p) = cur {
                d += 1;
                cur = self.parent_of(p);
            }
            d
        }
        fn remove(&self, id: i64) {
            self.nodes
                .borrow_mut()
                .retain(|n| n.id != id && n.parent != Some(id));
            self.bump();
        }
        fn bump(&self) {
            let v = self.version.get() + 1;
            self.version.set(v);
        }
    }

    impl TreeDataSource for MockI64Source {
        type Item = String;
        type Key = i64;
        fn visible_count(&self) -> usize {
            self.visible_ids().len()
        }
        fn with_entry<R>(
            &self,
            flat_index: usize,
            f: impl FnOnce(&String, &FlatEntry<i64>) -> R,
        ) -> Option<R> {
            let id = *self.visible_ids().get(flat_index)?;
            let entry = FlatEntry {
                node_id: id,
                depth: self.depth_of(id),
                has_children: self.nodes.borrow().iter().any(|n| n.parent == Some(id)),
                is_expanded: self.expanded.borrow().contains(&id),
            };
            let nodes = self.nodes.borrow();
            let label = &nodes.iter().find(|n| n.id == id)?.label;
            Some(f(label, &entry))
        }
        fn key_at(&self, flat_index: usize) -> Option<i64> {
            self.visible_ids().get(flat_index).copied()
        }
        fn flat_index_of(&self, key: &i64) -> Option<usize> {
            self.visible_ids().iter().position(|k| k == key)
        }
        fn parent(&self, key: &i64) -> Option<i64> {
            self.parent_of(*key)
        }
        fn child_keys(&self, key: &i64) -> Vec<i64> {
            self.nodes
                .borrow()
                .iter()
                .filter(|n| n.parent == Some(*key))
                .map(|n| n.id)
                .collect()
        }
        fn version_signal(&self) -> Signal<u64> {
            self.version.clone()
        }
        fn is_expanded(&self, key: &i64) -> bool {
            self.expanded.borrow().contains(key)
        }
        fn set_expanded(&self, key: &i64, expanded: bool) {
            if expanded {
                self.expanded.borrow_mut().insert(*key);
            } else {
                self.expanded.borrow_mut().remove(key);
            }
            self.bump();
        }
        fn contains_key(&self, key: &i64) -> bool {
            // Whole-tree existence (survives collapse), not visibility.
            self.exists(*key)
        }
        fn drag(&self, _key: &i64) -> DragEligibility {
            DragEligibility::CanDrag
        }
        fn can_accept(&self, query: &bastyde_data::DropQuery<'_, i64>) -> DropResponse {
            match &query.source {
                bastyde_data::DragSource::SameView { key: src } => {
                    if *src == query.target || self.is_descendant(query.target, *src) {
                        DropResponse::Reject
                    } else {
                        DropResponse::Accept
                    }
                }
                bastyde_data::DragSource::Foreign { .. } => DropResponse::Reject,
            }
        }
        fn accept_drop(&self, commit: bastyde_data::DropCommit<'_, i64>) -> bool {
            let bastyde_data::DragSource::SameView { key: src } = commit.source else {
                return false;
            };
            if src == commit.target || self.is_descendant(commit.target, src) {
                return false;
            }
            self.accept_log
                .borrow_mut()
                .push((src, commit.target, commit.position));
            self.bump();
            true
        }
    }

    #[test]
    fn from_source_row_scope_is_the_treeview_not_a_higher_ancestor() {
        // Reproduces the Skribisto shell: an outer focusable container holds the
        // binder TreeView and a sibling focusable ("the editor"). A row's focus
        // scope MUST be the TreeView — so when focus moves to the editor, the
        // row's scope goes inactive (selection mutes, focus ring clears). If the
        // scope resolved to the outer shell instead, it would stay active and the
        // ring would never clear.
        use crate::primitives::ZStack;
        use bastyde_core::widget_builder::WidgetBuilder;
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let source = Rc::new(MockI64Source::new());
        let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let tv = tree.add(
            TreeView::from_source_keyed(
                MockI64Wrapper(source.clone()),
                keyed.clone(),
                |_l: &String, _r: &TreeRow, _s| Box::new(FixedLeaf(120.0, 24.0)),
            )
            .item_height(24.0),
        );
        let editor = tree.add(FixedLeaf(100.0, 24.0).focusable(true));
        // Outer shell holds both, and is itself focusable (like `App`).
        let _shell = tree.add(
            ZStack::new()
                .add_child(tv)
                .add_child(editor)
                .focusable(true),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let rows = tree.children(tv);
        let scope = tree.view_focus_active_for(rows[0]);

        tree.focus(tv);
        assert!(scope.get(), "row scope active when the TreeView is focused");
        tree.focus(editor);
        assert!(
            !scope.get(),
            "focus moved to the sibling editor → the row's TreeView scope must go \
             inactive (so selection mutes and the focus ring clears)"
        );
    }

    #[test]
    fn view_focus_active_tracks_view_focus_for_rows() {
        // Diagnostic for focus-aware selection: a row's focus scope (its nearest
        // focusable ancestor = the TreeView) must read inactive before focus and
        // active once a click focuses the view.
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let source = Rc::new(MockI64Source::new());
        let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let tv = tree.add(
            TreeView::from_source_keyed(
                MockI64Wrapper(source.clone()),
                keyed.clone(),
                |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
            )
            .item_height(24.0)
            // Mirror Skribisto's binder: reorderable rows (drag recognizer) +
            // single-click activation (tap recognizer). These install gesture
            // arenas on each row — verify they don't preempt focusing the view.
            .reorderable(true)
            .activate_on(crate::data_views::ActivateOn::SingleClick)
            .on_activate(|_| {}),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let rows = tree.children(tv);
        assert_eq!(tree.focused(), None, "no focus yet");
        tree.click(rows[1]);
        // Clicking a row must move keyboard focus to the TreeView (or a focusable
        // descendant of it) — that is what makes focus-aware selection render
        // active. If this regresses, selected rows render with the muted
        // `SelectedInactive` chrome and look unselected.
        let focused = tree.focused();
        assert!(focused.is_some(), "clicking a row focuses something");
        assert!(
            focused == Some(tv) || tree.is_descendant_of(focused.unwrap(), tv),
            "focus landed inside the TreeView (got {focused:?}, tv = {tv:?})"
        );
    }

    #[test]
    fn container_focus_ring_shows_when_tab_focused_without_selection() {
        // The reported gap: Tab into the tree with nothing selected and there is
        // no visible focus indicator — no row paints a ring because no row is
        // selected. The container focus ring fills it: paint outlines the whole
        // view when it holds keyboard focus (`view_focus_active`), the modality
        // is keyboard (`focus_visible`), and the selection is empty. This guards
        // those three paint inputs (paint output itself isn't unit-observable).
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let source = Rc::new(MockI64Source::new());
        let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
        let mut tree = WidgetTree::new();
        let tv = tree.add(
            TreeView::from_source_keyed(
                MockI64Wrapper(source.clone()),
                keyed.clone(),
                |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
            )
            .item_height(24.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let view_focused = tree.view_focus_active_for(tv);
        let focus_visible = tree.focus_visible_signal();
        assert!(
            !view_focused.get() && !focus_visible.get(),
            "before focus: no container ring (not focused, pointer modality)"
        );

        // Tab in: focus the view under keyboard modality. `Tab` is ignored by the
        // tree's key handler, so the selection stays empty (no row ring).
        tree.focus(tv);
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert!(view_focused.get(), "view holds keyboard focus");
        assert!(focus_visible.get(), "keyboard input → focus-visible");
        assert_eq!(
            keyed.count(),
            0,
            "nothing selected → no row ring, container ring shows"
        );

        // A pointer press flips modality off → container ring clears (matches the
        // row ring's `:focus-visible` rule; clicking never leaves a ring).
        tree.click(tv);
        assert!(
            !focus_visible.get(),
            "pointer input clears focus-visible → ring hides"
        );
    }

    #[test]
    fn container_focus_ring_hidden_when_a_sibling_holds_focus() {
        // Regression: the container ring must track THIS view's own keyboard
        // focus, not a global signal. The view captured its focus signal at
        // build time; a plain `view_focus_active()` there found no focusable
        // ancestor (the root's `.focusable(true)` isn't wired into the arena
        // yet) and fell back to the constant-`true` "outside any scope" signal —
        // so every data view lit its container ring whenever ANY other widget
        // took keyboard focus. `begin_view_focus` keys the signal on the root
        // id and fixes it. This observes the painted ring (not just the signal,
        // which `view_focus_active_for` resolves correctly post-build).
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_core::widget_builder::WidgetBuilder;
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let source = Rc::new(MockI64Source::new());
        let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let root = tree.add(
            crate::primitives::VStack::new()
                .child(
                    TreeView::from_source_keyed(
                        MockI64Wrapper(source.clone()),
                        keyed.clone(),
                        |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
                    )
                    .item_height(24.0),
                )
                // A focusable sibling that paints no chrome of its own.
                .child(FixedLeaf(40.0, 24.0).focusable(true)),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let children = tree.children(root);
        let (tv, sibling) = (children[0], children[1]);

        let ring = bastyde_tokens::BorderRole::Focused
            .resolve(&bastyde_core::presets::intui::light().colors)
            .to_array();

        // The sibling holds focus under keyboard modality. It is NOT inside the
        // tree view, so the tree view's container ring must stay hidden even
        // though focus-visible is true and nothing is selected.
        tree.focus(sibling);
        tree.press_key(Key::ArrowDown, Modifiers::NONE); // sibling ignores it; flips focus-visible
        assert_eq!(tree.focused(), Some(sibling), "sibling holds focus");
        assert_eq!(keyed.count(), 0, "nothing selected");
        let frame = tree.render();
        assert!(
            !frame.decorations.iter().any(|d| d.color == ring)
                && !frame.shapes.iter().any(|s| s.color == ring)
                && !frame.cosmetic_lines.iter().any(|l| l.color == ring),
            "container ring must NOT paint while a sibling holds focus",
        );

        // Move focus to the tree view (programmatic — focus-visible stays true).
        // Now the view holds keyboard focus with no selection → the ring shows.
        tree.focus(tv);
        assert_eq!(tree.focused(), Some(tv), "tree view holds focus");
        assert_eq!(keyed.count(), 0, "still nothing selected");
        let frame = tree.render();
        assert!(
            frame.decorations.iter().any(|d| d.color == ring)
                || frame.shapes.iter().any(|s| s.color == ring)
                || frame.cosmetic_lines.iter().any(|l| l.color == ring),
            "container ring paints when the view holds keyboard focus",
        );
    }

    #[test]
    fn from_source_keyed_i64_survives_collapse_and_prunes() {
        // A `TreeView<String>` driven by a generic `TreeDataSource<Key = i64>`
        // (no `TreeModel` mirror). Selection is keyed by the entity id: a click
        // stores the row's i64, a collapse keeps it (whole-tree existence), and
        // a delete prunes it.
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let source = Rc::new(MockI64Source::new());
        let keyed = KeyedSelectionModel::<i64>::new(SelectionMode::Multi);
        let mut tree = WidgetTree::new();
        let tv = tree.add(
            TreeView::from_source_keyed(
                MockI64Wrapper(source.clone()),
                keyed.clone(),
                |_label: &String, _row: &TreeRow, _sel| Box::new(FixedLeaf(120.0, 24.0)),
            )
            .item_height(24.0),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Visible order is [1, 2, 3, 4]; row 1 is node "a" (id 2). Clicking it
        // must store the KEY 2, proving index→key translation + the render path.
        let rows = tree.children(tv);
        tree.click(rows[1]);
        assert!(
            keyed.is_selected(&2),
            "click stores the row's i64 key, not its index"
        );
        assert!(!keyed.is_selected(&1));

        // Collapse root1 → node 2 leaves the visible projection (version bump
        // runs the prune). It must survive — still present in the source.
        source.set_expanded(&1, false);
        assert!(
            keyed.is_selected(&2),
            "a collapsed-but-present i64 node keeps its selection"
        );

        // Delete node 2 → prune drops the now-missing key.
        source.remove(2);
        assert!(!keyed.is_selected(&2), "a deleted i64 node is pruned");
    }

    /// Newtype so we can hand the same `Rc<MockI64Source>` to the view while
    /// keeping a handle for assertions (the view erases the source).
    struct MockI64Wrapper(Rc<MockI64Source>);
    impl TreeDataSource for MockI64Wrapper {
        type Item = String;
        type Key = i64;
        fn visible_count(&self) -> usize {
            self.0.visible_count()
        }
        fn with_entry<R>(
            &self,
            i: usize,
            f: impl FnOnce(&String, &FlatEntry<i64>) -> R,
        ) -> Option<R> {
            self.0.with_entry(i, f)
        }
        fn key_at(&self, i: usize) -> Option<i64> {
            self.0.key_at(i)
        }
        fn flat_index_of(&self, k: &i64) -> Option<usize> {
            self.0.flat_index_of(k)
        }
        fn parent(&self, k: &i64) -> Option<i64> {
            self.0.parent(k)
        }
        fn child_keys(&self, k: &i64) -> Vec<i64> {
            self.0.child_keys(k)
        }
        fn version_signal(&self) -> Signal<u64> {
            self.0.version_signal()
        }
        fn is_expanded(&self, k: &i64) -> bool {
            self.0.is_expanded(k)
        }
        fn set_expanded(&self, k: &i64, e: bool) {
            self.0.set_expanded(k, e)
        }
        fn contains_key(&self, k: &i64) -> bool {
            self.0.contains_key(k)
        }
        fn drag(&self, k: &i64) -> DragEligibility {
            self.0.drag(k)
        }
        fn can_accept(&self, q: &bastyde_data::DropQuery<'_, i64>) -> DropResponse {
            self.0.can_accept(q)
        }
        fn accept_drop(&self, c: bastyde_data::DropCommit<'_, i64>) -> bool {
            self.0.accept_drop(c)
        }
    }

    #[test]
    fn from_source_drop_routes_through_source_and_refuses_cycle() {
        // Reorderable generic source: a valid drop reaches `accept_drop`; a drop
        // that would create a cycle (a parent onto its own child) is refused by
        // the source — proving the view delegates DnD to `can_accept`/
        // `accept_drop` instead of mutating a model itself.

        // Valid: drag node "b" (id 3, row 2) onto "root2" (id 4, row 3).
        let valid = Rc::new(MockI64Source::new());
        let mut t1 = WidgetTree::new();
        let v1 = t1.add(
            TreeView::from_source(
                MockI64Wrapper(valid.clone()),
                |_l: &String, _r: &TreeRow, _s| Box::new(FixedLeaf(120.0, 24.0)),
            )
            .item_height(24.0)
            .reorderable(true),
        );
        t1.layout(SizeProposal::exact(400.0, 300.0));
        let rows = t1.children(v1);
        let from = t1.bounds(rows[2]).center();
        let to = t1.bounds(rows[3]).center();
        drag_item(&mut t1, from, to);
        assert_eq!(
            valid.accept_log.borrow().len(),
            1,
            "a valid drop is routed to the source's accept_drop"
        );
        assert_eq!(
            valid.accept_log.borrow()[0].0,
            3,
            "dragged key recovered from RowDragData"
        );
        assert_eq!(
            valid.accept_log.borrow()[0].1,
            4,
            "target key resolved from the hovered row"
        );

        // Cycle: drag "root1" (id 1, row 0) onto its child "a" (id 2, row 1).
        let cyclic = Rc::new(MockI64Source::new());
        let mut t2 = WidgetTree::new();
        let v2 = t2.add(
            TreeView::from_source(
                MockI64Wrapper(cyclic.clone()),
                |_l: &String, _r: &TreeRow, _s| Box::new(FixedLeaf(120.0, 24.0)),
            )
            .item_height(24.0)
            .reorderable(true),
        );
        t2.layout(SizeProposal::exact(400.0, 300.0));
        let rows2 = t2.children(v2);
        let from2 = t2.bounds(rows2[0]).center();
        let to2 = t2.bounds(rows2[1]).center();
        drag_item(&mut t2, from2, to2);
        assert!(
            cyclic.accept_log.borrow().is_empty(),
            "a cyclic drop is refused by the source — no mutation applied"
        );
    }

    #[test]
    fn from_source_reorder_bubbles_past_a_per_row_drop_target() {
        // Mirrors the designer outline: each row is a `DropTarget` that accepts
        // only a palette payload (here `Palette`). A row-reorder `RowDragData`
        // must bubble PAST that DropTarget to the TreeView and reorder — i.e. the
        // per-row drop target and the view's drag-to-reorder coexist.
        use crate::drop_target::DropTarget;
        use bastyde_core::event::EventResponse;
        use bastyde_core::widget_builder::WidgetBuilder;
        #[derive(Clone)]
        struct Palette;
        let src = Rc::new(MockI64Source::new());
        let mut t = WidgetTree::new();
        let v = t.add(
            TreeView::from_source(
                MockI64Wrapper(src.clone()),
                |_l: &String, _r: &TreeRow, _s| {
                    // Match the designer row exactly: a DropTarget wrapped by an
                    // on_pointer_event (selection) + context_menu handler node.
                    Box::new(
                        DropTarget::new()
                            .on_drop_typed::<Palette>(|_p, _pos, _ctx| true)
                            .child(FixedLeaf(120.0, 24.0))
                            .on_pointer_event(|_ev, _ctx| EventResponse::Ignored)
                            .context_menu(|_pos, _ctx| None),
                    ) as Box<dyn Widget>
                },
            )
            .item_height(24.0)
            .reorderable(true),
        );
        t.layout(SizeProposal::exact(400.0, 300.0));
        let rows = t.children(v);
        // Drag node "b" (id 3, row 2) onto "root2" (id 4, row 3).
        let from = t.bounds(rows[2]).center();
        let to = t.bounds(rows[3]).center();
        drag_item(&mut t, from, to);
        assert_eq!(
            src.accept_log.borrow().len(),
            1,
            "row reorder must bubble past the per-row DropTarget to the TreeView"
        );
    }

    #[test]
    fn from_source_row_with_pointer_event_selection_stays_draggable() {
        // A row that selects on press via `on_pointer_event` (raw, returns
        // `Ignored`) installs NO gesture arena, so it never captures the pointer
        // and the row stays draggable — the pattern a reorderable view uses for
        // per-row selection (selection must land on *press* and carry the
        // Ctrl/Shift modifiers, which `TapRecognizer` fires-on-release and
        // strips). The framework also disambiguates a descendant `on_tap`
        // against an ancestor drag now (the ancestor observes the pointer while
        // the tap holds capture — see `ancestor_drag_starts_through_descendant_tap_capture`),
        // but `on_pointer_event` remains the right press-time + modifier-aware
        // choice here.
        use bastyde_core::event::{EventResponse, WidgetEvent};
        use bastyde_core::widget_builder::WidgetBuilder;
        let src = Rc::new(MockI64Source::new());
        let picked = Rc::new(Cell::new(None::<i64>));
        let mut t = WidgetTree::new();
        let picked_for_rows = picked.clone();
        let v = t.add(
            TreeView::from_source(
                MockI64Wrapper(src.clone()),
                move |_l: &String, _r: &TreeRow, _s| {
                    let picked = picked_for_rows.clone();
                    Box::new(FixedLeaf(120.0, 24.0).on_pointer_event(move |ev, _c| {
                        if let WidgetEvent::PointerDown { .. } = ev {
                            picked.set(Some(7));
                        }
                        EventResponse::Ignored
                    })) as Box<dyn Widget>
                },
            )
            .item_height(24.0)
            .reorderable(true),
        );
        t.layout(SizeProposal::exact(400.0, 300.0));
        let rows = t.children(v);
        let from = t.bounds(rows[2]).center();
        let to = t.bounds(rows[3]).center();
        drag_item(&mut t, from, to);
        assert!(
            picked.get().is_some(),
            "press still selects via on_pointer_event"
        );
        assert_eq!(
            src.accept_log.borrow().len(),
            1,
            "on_pointer_event selection must not block the row drag"
        );
    }

    #[test]
    fn lazy_loading_tree_rows_render_placeholders_and_request_the_window() {
        // A windowed `TreeDataSource` with nothing resident: every visible row
        // is `Loading`, so the TreeView must render placeholder skeletons (not
        // skip the rows) and nudge the source to load the realized window —
        // the tree analogue of the ListView lazy path.
        use std::ops::Range;

        struct WindowedTree {
            total: usize,
            version: Signal<u64>,
            requested: Rc<RefCell<Vec<Range<usize>>>>,
        }
        impl TreeDataSource for WindowedTree {
            type Item = String;
            type Key = usize;
            fn visible_count(&self) -> usize {
                self.total
            }
            fn with_entry<R>(
                &self,
                _flat_index: usize,
                _f: impl FnOnce(&String, &FlatEntry<usize>) -> R,
            ) -> Option<R> {
                None // nothing resident yet
            }
            fn key_at(&self, i: usize) -> Option<usize> {
                (i < self.total).then_some(i)
            }
            fn flat_index_of(&self, key: &usize) -> Option<usize> {
                (*key < self.total).then_some(*key)
            }
            fn parent(&self, _key: &usize) -> Option<usize> {
                None
            }
            fn child_keys(&self, _key: &usize) -> Vec<usize> {
                Vec::new()
            }
            fn version_signal(&self) -> Signal<u64> {
                self.version.clone()
            }
            fn is_expanded(&self, _key: &usize) -> bool {
                false
            }
            fn set_expanded(&self, _key: &usize, _expanded: bool) {}
            fn row_state(&self, _flat_index: usize) -> RowState {
                RowState::Loading
            }
            fn request_window(&self, range: Range<usize>) {
                self.requested.borrow_mut().push(range);
            }
        }

        let requested = Rc::new(RefCell::new(Vec::new()));
        let source = WindowedTree {
            total: 1000,
            version: Signal::new(0),
            requested: requested.clone(),
        };
        let mut t = WidgetTree::new();
        let v = t.add(
            TreeView::from_source(source, |_l: &String, _r: &TreeRow, _s| {
                Box::new(FixedLeaf(120.0, 28.0)) as Box<dyn Widget>
            })
            .item_height(28.0),
        );
        t.layout(SizeProposal::exact(400.0, 300.0));

        // 300px / 28px ≈ 10 visible + buffer → the loading rows are realized as
        // placeholder child widgets (children minus the scrollbar), NOT skipped.
        let placeholder_rows = t.children(v).len() - 1;
        assert!(
            placeholder_rows >= 10,
            "loading tree rows must render as placeholders, got {placeholder_rows}"
        );
        assert!(
            !requested.borrow().is_empty(),
            "request_window must be called for the visible range"
        );
    }

    #[test]
    fn treeview_exportable_row_drops_on_foreign_sink_with_items() {
        use crate::primitives::{FixedSize, VStack};
        use bastyde_core::widget_builder::WidgetBuilder as _;
        // sample_tree(): roots A, B, C (collapsed), item type = &'static str.
        let model = sample_tree();
        #[allow(clippy::type_complexity)]
        let cap: Rc<RefCell<Option<(Vec<usize>, Option<Vec<&'static str>>)>>> =
            Rc::new(RefCell::new(None));
        let cap2 = cap.clone();
        let tv = TreeView::new(model.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .exportable(DragTransferMode::Copy);
        let sink = FixedLeaf(180.0, 80.0).on_drop(move |mut payload, _pos, _ctx| {
            if let Some(rd) = payload.take_typed::<RowDragData<&'static str>>() {
                *cap2.borrow_mut() = Some((rd.rows, rd.items));
                true
            } else {
                false
            }
        });
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(84.0).child(tv))
                .child(sink),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        // Row 0 = root A at y≈14; the sink spans y=84..164 (drop at y≈120).
        drag_item(&mut tree, Point::new(50.0, 14.0), Point::new(50.0, 120.0));
        let (rows, items) = cap.borrow().clone().expect("sink received a RowDragData");
        assert_eq!(rows, vec![0]);
        assert_eq!(items, Some(vec!["A"]));
    }

    #[test]
    fn treeview_exportable_move_removes_source_node_via_stable_key() {
        use crate::primitives::{FixedSize, VStack};
        use bastyde_core::widget_builder::WidgetBuilder as _;
        // A Move-export drops the origin node once accepted elsewhere. The
        // move-out resolves a STABLE NodeId at drag-start (not a flat index at
        // completion), so it removes exactly the dragged node.
        let model = sample_tree(); // roots A, B, C
        let a = model.root(0);
        let accepted: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        let acc2 = accepted.clone();
        let tv = TreeView::new(model.clone(), |_item, entry, _sel| {
            Box::new(FixedLeaf(100.0 + entry.depth as f32 * 20.0, 28.0))
        })
        .item_height(28.0)
        .exportable(DragTransferMode::Move);
        let sink = FixedLeaf(180.0, 80.0).on_drop(move |mut payload, _pos, _ctx| {
            if payload.take_typed::<RowDragData<&'static str>>().is_some() {
                *acc2.borrow_mut() = true;
                true
            } else {
                false
            }
        });
        let mut tree = WidgetTree::new();
        tree.add(
            VStack::new()
                .spacing(0.0)
                .child(FixedSize::new().height(84.0).child(tv))
                .child(sink),
        );
        tree.layout(SizeProposal::exact(200.0, 300.0));
        drag_item(&mut tree, Point::new(50.0, 14.0), Point::new(50.0, 120.0));
        assert!(*accepted.borrow(), "sink accepted the Move drop");
        // The exact node A (stable id captured at drag-start) was removed.
        assert_eq!(
            model.with_item(a, |v| *v),
            None,
            "node A was removed on Move"
        );
    }
}
