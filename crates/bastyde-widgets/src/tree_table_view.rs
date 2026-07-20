// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeTableView<T>` — hierarchical multi-column data table with expand/collapse.
//!
//! Sibling of [`TableView`](crate::TableView) for tree-shaped data. Each row carries
//! a depth level; one designated column (the *tree column*, defaulting to the first)
//! shows a twist (chevron) and an indent gutter that toggles the row's children.
//! Backed by a [`SortFilterTreeModel<T>`] so sort, filter, and expand state compose
//! without extra bookkeeping. Shares the header, column, keyboard, and selection
//! modules with `TableView`.
//!
//! Rows live in a `TreeBodyPane` — a sibling of the scrollbar — so buffer-exit /
//! selection / expand rebuilds are never deferred mid-thumb-drag. Three row-height
//! modes: uniform (`row_height`, fast path), exact per-flat-index callback
//! (`row_height_fn`), and auto-measured (`auto_row_height` — grows to tallest cell).
//!
//! ## Common patterns
//!
//! **A checkbox column.** Selection and "checked" are different things — a
//! checkbox column wants its own state, with parent/child propagation. Build it
//! from [`TreeCheckedModel`](bastyde_data::TreeCheckedModel) over the same tree
//! the view projects.
//!
//! A cell delegate receives `(&T, &CellContext)` and **`CellContext` carries no
//! node identity** — only [`row_index`](crate::CellContext::row_index). So
//! capture the projection and resolve the row's `NodeId` through it:
//!
//! ```ignore
//! let proxy = SortFilterTreeModel::new(tree);
//! let checks = TreeCheckedModel::new(proxy.tree());
//! let for_cells = proxy.clone();
//! let col = Column::new("done", lit!("Done"), move |_item, cx: &CellContext| {
//!     match for_cells.visible_node_id(cx.row_index) {
//!         Some(node) => Box::new(Checkbox::new(checks.check_state(node))) as Box<dyn Widget>,
//!         None => Box::new(Spacer::new()),
//!     }
//! });
//! ```
//!
//! For a tree whose identity is a domain key rather than a `NodeId`, use
//! [`KeyedTreeCheckedModel`](bastyde_data::KeyedTreeCheckedModel) instead — it
//! survives a full re-source, which a `NodeId`-keyed set cannot.
//!
//! ## Accessibility
//!
//! Root emits `Role::TreeGrid`; rows carry `set_level` + `set_expanded`.
//! ArrowLeft / ArrowRight on the tree column collapse / expand.
//!
//! ```ignore
//! // Column delegates capture closures — use ignore.
//! use bastyde_widgets::TreeTableView;
//! use bastyde_data::TreeModel;
//! # struct File { name: String }
//! # let model: TreeModel<File> = TreeModel::new();
//! let _view = TreeTableView::new(model).row_height(28.0);
//! ```

mod body_pane;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use bastyde_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::drag_payload::DragPayload;
use bastyde_core::event::EventResponse;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{
    DropPosition, KeyedSelectionModel, NodeId, SelectionModel, SortDirection, SortFilterTreeModel,
    TreeFilterMode, TreeModel,
};
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{BorderRole, Easing, SurfaceRole};

use crate::styles::recipe_table_style as cp;

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{DragTransferMode, RowDragData, RowSelection, ViewId, ViewKind};
use crate::data_views::{DropViz, drop_into_tint};
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};
use crate::table_view::body::SharedColumnWidths;
use crate::table_view::column::{
    Column, ColumnResizePolicy, EditTrigger, GridLines, PinnedSide, TabTraversal,
};
use crate::table_view::header::{HeaderCell, HeaderRow, ResizeStateHandle};
use crate::table_view::imperative;
use crate::table_view::keyboard;
use crate::table_view::layout;
use crate::table_view::row_navigator::RowNavigator;
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};
use crate::tree_source::TreeSource;
use bastyde_data::{DropResponse, TreeDataSource};

const BUFFER_ROWS: usize = 5;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Hierarchical row navigator. Adapts a [`TreeSource`]'s flat-list view to the
/// [`RowNavigator`] interface used by the shared keyboard handler.
///
/// Index-keyed throughout, so it works over any [`TreeDataSource`] — a
/// `SortFilterTreeModel` over a `TreeModel`, or an external store carrying its
/// own `Key`.
pub(crate) struct TreeNavigator<T: 'static> {
    source: Rc<TreeSource<T>>,
}

impl<T: 'static> TreeNavigator<T> {
    pub(crate) fn new(source: Rc<TreeSource<T>>) -> Self {
        Self { source }
    }
}

impl<T: 'static> RowNavigator for TreeNavigator<T> {
    fn row_count(&self) -> usize {
        self.source.visible_count()
    }

    fn depth(&self, row: usize) -> Option<usize> {
        self.source.meta(row).map(|m| m.depth)
    }

    fn has_children(&self, row: usize) -> bool {
        self.source
            .meta(row)
            .map(|m| m.has_children)
            .unwrap_or(false)
    }

    fn is_expanded(&self, row: usize) -> bool {
        self.source
            .meta(row)
            .map(|m| m.is_expanded)
            .unwrap_or(false)
    }

    fn toggle_expanded(&self, row: usize) {
        self.source.toggle_at(row);
    }
}

/// Hierarchical multi-column widget. See module documentation.
pub struct TreeTableView<T: 'static> {
    /// Erased row access — every read (counts, entries, expansion, DnD,
    /// keyboard reorder) goes through here, so the widget works over any
    /// [`TreeDataSource`] and never needs to know the source's `Key`.
    source: Rc<TreeSource<T>>,
    /// Present only on the [`from_projection`](Self::from_projection) /
    /// [`new`](Self::new) paths. It backs the `NodeId`-typed public API
    /// ([`expand`](Self::expand), [`projection`](Self::projection), …), which is
    /// meaningless for an external source carrying its own key — those methods
    /// no-op when this is `None`.
    proxy: Option<SortFilterTreeModel<T>>,

    columns: Vec<Column<T>>,
    /// Column id hosting the twist + indent. `None` defaults to the
    /// first column at build time.
    tree_column_id: Option<String>,
    indent_per_level: Option<f32>,
    row_height: Option<f32>,
    /// Height-mode selection (uniform / exact callback / auto-measure).
    height_source: HeightSource,
    /// Row geometry — shared with the keyboard handler and the body
    /// pane.
    row_metrics: SharedRowMetrics,
    header_height: Option<f32>,
    show_header: bool,
    selection_mode: TableSelectionMode,
    /// Row selection — index-based `SelectionModel` or keyed
    /// `KeyedSelectionModel<NodeId>`, unified behind the index-facing facade.
    row_selection: Option<RowSelection>,
    cell_selection: Option<CellSelectionModel>,
    alternating_rows: bool,
    grid_lines: GridLines,
    a11y_label: Option<LocalizedString>,
    show_internal_scrollbars: bool,
    column_resize_policy: ColumnResizePolicy,
    tab_traversal: TabTraversal,
    edit_trigger: EditTrigger,
    #[allow(clippy::type_complexity)]
    on_cell_edit_request: Option<Rc<dyn Fn(usize, &str, &mut EventContext)>>,
    #[allow(clippy::type_complexity)]
    on_row_activate: Option<Rc<dyn Fn(usize, &mut EventContext)>>,

    /// Animate wheel scrolling instead of snapping to the new offset.
    /// Enabled by default — mirrors `ScrollArea`. Without it, each wheel
    /// notch jumps by `row_height` per delivered line, which reads as a
    /// coarse multi-row jump rather than a smooth glide.
    smooth_scrolling: bool,
    /// Duration of the smooth scroll animation.
    smooth_scroll_duration: Duration,

    /// How the scroll bar is displayed (default `Permanent`). `Overlay`
    /// and `Thin` float the bar over the content instead of reserving a
    /// layout column for it, mirroring `ScrollArea::scroll_bar_style`.
    scroll_bar_style: ScrollBarMode,

    // Public reactive signals
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    /// Scroll-chaining behavior at the boundary (default `Chain`).
    overscroll_behavior: OverscrollBehavior,
    viewport_ratio_y: Signal<f32>,
    sort_signal: Signal<Option<(String, SortDirection)>>,
    column_widths_signal: Signal<HashMap<String, f32>>,
    column_order_signal: Signal<Vec<String>>,
    column_pinning_signal: Signal<HashMap<String, PinnedSide>>,
    filters_signal: Signal<HashMap<String, String>>,
    focused_cell: Signal<Option<(usize, usize)>>,
    editing_cell: Signal<Option<(usize, usize)>>,
    /// Type-ahead ("type to jump") label extractor — opt-in via
    /// [`type_ahead_label`](Self::type_ahead_label).
    #[allow(clippy::type_complexity)]
    type_ahead_label: Option<Rc<dyn Fn(&T) -> String>>,
    /// Reset window for the type-ahead search term.
    type_ahead_timeout: Duration,
    /// Persistent type-ahead buffer (survives the per-keystroke rebuild).
    type_ahead: Rc<crate::common::type_ahead::TypeAheadState>,
    /// Widget shown in place of the rows when nothing is visible — an empty
    /// tree, or a filter that matched nothing.
    #[allow(clippy::type_complexity)]
    empty_view: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    /// Set on the first `place_children`. Until then `viewport_height` still
    /// holds its construction placeholder, so viewport-relative imperatives
    /// (`ensure_row_visible`) would scroll against a size that was never real.
    laid_out: Rc<Cell<bool>>,
    /// Anchor for the row with an open cell editor, so the editor follows its
    /// row instead of its index. See `reconcile_editing_row`.
    editing_anchor: Rc<RefCell<Option<crate::data_views::RowAnchor>>>,

    // Build state
    header_row_id: Option<WidgetId>,
    body_pane_id: Option<WidgetId>,
    scrollbar_id: Option<WidgetId>,
    empty_id: Option<WidgetId>,
    /// Pane-local rebuild trigger + buffered range, owned here so they
    /// survive `TreeTableView` rebuilds (each rebuild constructs a fresh
    /// `TreeBodyPane` struct that inherits these handles).
    pane_version: Signal<u64>,
    pane_built_start: Rc<Cell<usize>>,
    pane_built_end: Rc<Cell<usize>>,
    /// Bumped by the pane when a measure pass changes the content
    /// total; bound at `Relayout` on this root so `max_scroll_y` / the
    /// thumb ratio are recomputed with the corrected total next frame.
    pane_total_refresh: Signal<u64>,

    /// Enable drag-to-reorder of rows (pointer drag + Alt+Arrow). The move
    /// reparents/reorders nodes in the underlying `TreeModel`, cycle-guarded.
    /// Suppressed while a sort is active (the visible order then differs from
    /// the tree order, so a manual reorder would be meaningless).
    reorderable: bool,
    /// Active row-drop insertion indicator `(body_local_y, width)`. Set by
    /// `on_drag_hover`, cleared on leave / drop, read by `paint`.
    drop_feedback: Signal<Option<DropViz>>,

    /// Whether activation is a single or double click (default `DoubleClick`).
    activate_on: crate::data_views::ActivateOn,

    /// `true` while this view — its root or any descendant — holds keyboard
    /// focus. Captured at build from [`BuildContext::view_focus_active`], bound
    /// `RepaintOnly`. Drives focus-aware selection: the band paints `Selected`
    /// while focused, muted `SelectedInactive` once focus leaves the view.
    view_focused: Signal<bool>,
    /// Input-modality `:focus-visible`. Gates the cell focus ring to keyboard
    /// navigation (never a mouse click). Bound `RepaintOnly`.
    focus_visible: Signal<bool>,

    // Layout state
    column_widths: SharedColumnWidths,
    display_indices: Rc<RefCell<Vec<usize>>>,
    /// `(row, display_pos) -> WidgetId` for every cell realized by the
    /// body pane's latest `build()`. Mirrors `TableView::cell_map` (the
    /// GridView `tile_map` pattern — shared between the root and its
    /// sibling-of-scrollbar pane); `accessibility()` reads it to point
    /// `active_descendant` at the keyboard-focused cell's own AT node.
    cell_map: Rc<RefCell<Vec<((usize, usize), WidgetId)>>>,
    viewport_height: Rc<Cell<f32>>,
    /// The row-area's absolute (window) rect (below the header), cached by
    /// `place_children`. Threaded into the keyboard handler so it can chase the
    /// focused row into any *enclosing* scroll area via
    /// [`EventContext::ensure_visible`](bastyde_core::widget::EventContext::ensure_visible).
    body_bounds: Rc<Cell<Rect>>,
    resize_state: ResizeStateHandle,
    /// Stable id grouping the column-header reorder/resize drag (an
    /// unrelated mechanism to the row DnD below — see `table_view::header`).
    table_id: usize,

    /// Stable, kind-tagged identity for this view's **row** drag-and-drop —
    /// distinct from `table_id` above. Minted via
    /// `ViewId::next(ViewKind::TreeTable)`.
    model_id: ViewId,

    /// Cross-widget export / foreign-receive machinery — the builders
    /// (`.exportable`, `.export_external`, `.accept_foreign_rows`,
    /// `.on_rows_received`, `.on_rows_transferred_out`), the drag-start
    /// payload build, and the move-out completion, shared by all five data
    /// views. `TreeTableView` builds its reader + stable-key removal thunk
    /// inline at drag-start (see `TreeBodyPane::build`'s `on_drag`) rather
    /// than from source capability closures, so the key it removes by is
    /// resolved once at drag-start and stays correct even if a mid-drag
    /// spring-load reflattens the rows under the pointer.
    export: crate::data_views::RowExport<T>,
    /// Raw escape hatch for a payload this view cannot interpret itself.
    ///
    /// A source-backed view ([`from_source`](Self::from_source)) expresses
    /// foreign-accept through its source's capability closures, like
    /// `ListView` / `TableView`. This hook is what a **projection**-backed
    /// view ([`from_projection`](Self::from_projection) / [`new`](Self::new))
    /// has instead, since a `SortFilterTreeModel` carries no such closures.
    /// Fires for any payload NOT recognized as this view's own row drag,
    /// dropped on a node —
    /// `(payload, target node, drop position, ctx) -> accepted`. Tried after
    /// [`on_rows_received`](Self::on_rows_received).
    #[allow(clippy::type_complexity)]
    on_foreign_drop:
        Option<Rc<dyn Fn(&DragPayload, NodeId, DropPosition, &mut EventContext) -> bool>>,

    /// Whole-view enabled state, statically or reactively. Forwarded to the
    /// arena via `ctx.enabled_when(self_id, self.enabled.clone())` at build
    /// time; a disabled view greys out and stops accepting focus /
    /// selection / keyboard input (arena-gated).
    enabled: Prop<bool>,
}

impl<T: 'static> TreeTableView<T> {
    /// Wrap a `SortFilterTreeModel<T>`.
    /// Wrap a `SortFilterTreeModel<T>`.
    pub fn from_projection(proxy: SortFilterTreeModel<T>) -> Self {
        let source = Rc::new(TreeSource::from_data_source(Rc::new(proxy.clone())));
        Self::assemble(source, Some(proxy))
    }

    /// Build a tree table over any [`TreeDataSource`] — an external source of
    /// truth (a Qleany entity store, a database, a virtual filesystem) carrying
    /// its own `Key`, so it needs no `TreeModel` mirror.
    ///
    /// This is the tree-table sibling of
    /// [`TreeView::from_source`](crate::TreeView::from_source). Because the
    /// source owns identity, its expand state (and a keyed selection) survive a
    /// full re-source — which a `TreeModel` mirror cannot guarantee, since
    /// `NodeId`s are reassigned on rebuild.
    ///
    /// The `NodeId`-typed methods ([`expand`](Self::expand),
    /// [`projection`](Self::projection), [`keyed_selection`](Self::keyed_selection))
    /// do not apply here and no-op; drive expansion through the source itself.
    ///
    /// Row drag-reorder **is** wired on this path: a drop routes through the source's
    /// own `drag` / `can_accept` / `accept_drop`, exactly as
    /// [`TreeView`](crate::TreeView) does — so the
    /// source owns both the cycle guard and the commit. Note that
    /// [`TreeDataSlice::drag`](bastyde_data::TreeDataSlice) defaults to `NoDrag`: an
    /// external source must opt its rows in before anything can be dragged.
    pub fn from_source<S: TreeDataSource<Item = T> + 'static>(source: S) -> Self {
        Self::assemble(Rc::new(TreeSource::from_data_source(Rc::new(source))), None)
    }

    /// Like [`from_source`](Self::from_source) but with **keyed** selection:
    /// the `KeyedSelectionModel<S::Key>` tracks rows by source identity, so it
    /// survives expand / collapse, sort / filter and a full re-source. Pruning
    /// consults the source's `contains_key`, so a collapsed-but-present row
    /// keeps its selection. The view stays `TreeTableView<T>` — the `Key` is
    /// captured here.
    pub fn from_source_keyed<S: TreeDataSource<Item = T> + 'static>(
        source: S,
        keyed: KeyedSelectionModel<S::Key>,
    ) -> Self
    where
        S::Key: bastyde_data::ItemKey,
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
        let mut view = Self::assemble(Rc::new(TreeSource::from_data_source(s)), None);
        view.row_selection = Some(RowSelection::from_keyed(keyed, key_at, len, contains));
        view
    }

    fn assemble(source: Rc<TreeSource<T>>, proxy: Option<SortFilterTreeModel<T>>) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let table_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            source,
            proxy,
            columns: Vec::new(),
            tree_column_id: None,
            indent_per_level: None,
            row_height: None,
            height_source: HeightSource::Uniform,
            row_metrics: Rc::new(RefCell::new(RowMetrics::uniform(cp::ROW_HEIGHT, 0.0))),
            header_height: None,
            show_header: true,
            selection_mode: TableSelectionMode::default(),
            row_selection: None,
            cell_selection: None,
            alternating_rows: false,
            grid_lines: GridLines::None,
            a11y_label: None,
            show_internal_scrollbars: true,
            column_resize_policy: ColumnResizePolicy::default(),
            tab_traversal: TabTraversal::default(),
            edit_trigger: EditTrigger::default(),
            on_cell_edit_request: None,
            on_row_activate: None,
            reorderable: false,
            drop_feedback: Signal::new(None),
            activate_on: crate::data_views::ActivateOn::default(),
            smooth_scrolling: true,
            smooth_scroll_duration: Duration::from_millis(150),
            scroll_bar_style: ScrollBarMode::Permanent,
            scroll_y: Signal::new_animated(0.0),
            max_scroll_y: Signal::new(0.0),
            overscroll_behavior: OverscrollBehavior::default(),
            viewport_ratio_y: Signal::new(1.0),
            sort_signal: Signal::new(None),
            column_widths_signal: Signal::new(HashMap::new()),
            column_order_signal: Signal::new(Vec::new()),
            column_pinning_signal: Signal::new(HashMap::new()),
            filters_signal: Signal::new(HashMap::new()),
            focused_cell: Signal::new(None),
            type_ahead_label: None,
            type_ahead_timeout: crate::common::type_ahead::DEFAULT_TYPE_AHEAD_TIMEOUT,
            type_ahead: crate::common::type_ahead::TypeAheadState::new(),
            // Replaced at build with the live tree signals.
            view_focused: Signal::new(true),
            focus_visible: Signal::new(false),
            editing_cell: Signal::new(None),
            empty_view: None,
            laid_out: Rc::new(Cell::new(false)),
            editing_anchor: Rc::new(RefCell::new(None)),
            header_row_id: None,
            body_pane_id: None,
            scrollbar_id: None,
            empty_id: None,
            pane_version: Signal::new(0_u64),
            pane_built_start: Rc::new(Cell::new(0)),
            pane_built_end: Rc::new(Cell::new(0)),
            pane_total_refresh: Signal::new(0_u64),
            column_widths: Rc::new(RefCell::new(Vec::new())),
            display_indices: Rc::new(RefCell::new(Vec::new())),
            cell_map: Rc::new(RefCell::new(Vec::new())),
            viewport_height: Rc::new(Cell::new(600.0)),
            body_bounds: Rc::new(Cell::new(Rect::ZERO)),
            resize_state: Rc::new(RefCell::new(None)),
            table_id,
            model_id: ViewId::next(ViewKind::TreeTable),
            export: crate::data_views::RowExport::default(),
            on_foreign_drop: None,
            enabled: Prop::Static(true),
        }
    }

    /// Wrap a raw `TreeModel<T>` — convenience for callers that don't
    /// need sort/filter. Internally builds an identity
    /// `SortFilterTreeModel`.
    pub fn new(model: TreeModel<T>) -> Self {
        Self::from_projection(SortFilterTreeModel::new(model))
    }

    // ── Builder ────────────────────────────────────────────────────────

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

    /// Enable or disable animated wheel scrolling (enabled by default).
    /// When disabled, wheel events snap immediately to the new offset.
    pub fn smooth_scrolling(mut self, enabled: bool) -> Self {
        self.smooth_scrolling = enabled;
        self
    }

    /// Enable **type-ahead** ("type to jump"): typing a printable character
    /// while the tree-table has keyboard focus jumps the focused row to the
    /// next *visible* row whose label starts with the accumulated search term,
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

    /// Append a column definition. Columns are displayed in declaration order unless
    /// reordered by the user.
    pub fn add_column(mut self, col: Column<T>) -> Self {
        self.columns.push(col);
        self
    }

    /// Enable drag-to-reorder of **rows** (pointer drag + keyboard
    /// Alt+ArrowUp/Down). Distinct from
    /// [`Column::reorderable`](crate::Column::reorderable), which reorders
    /// *columns* and defaults to `true`; this defaults to `false`.
    ///
    /// A drop reparents/reorders the dragged node in the underlying
    /// `TreeModel` (top third of a row = Before, middle = Into / make-child,
    /// bottom = After). The move is cycle-guarded — dropping a node onto
    /// itself or into its own subtree is refused (no insertion line). Reorder
    /// is **suppressed while a sort is active**: with the visible order driven
    /// by the sort, a manual reorder would have no visible effect.
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
    /// accepts them: [`DragTransferMode::Move`] removes them — by default,
    /// directly from the underlying `TreeModel` (any dragged node that is a
    /// descendant of another dragged node is skipped, since removing the
    /// ancestor already removes it); override via
    /// [`on_rows_transferred_out`](Self::on_rows_transferred_out).
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
    /// view. Receives the dragged rows' flat visible indices (as captured at
    /// drag-start) and the live context. Without this, an
    /// [`exportable`](Self::exportable) [`Move`](DragTransferMode::Move) drag
    /// removes the dragged nodes directly from the underlying `TreeModel`
    /// (leaf-first / descending — a dragged node that is a descendant of
    /// another dragged node is skipped, since removing the ancestor already
    /// removes its whole subtree).
    pub fn on_rows_transferred_out(
        mut self,
        f: impl Fn(&[usize], &mut EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_transferred_out(f);
        self
    }

    /// Accept exported rows dropped from a **different** view or source
    /// without writing a custom source. Pair with
    /// [`on_rows_received`](Self::on_rows_received), which is handed the
    /// dropped items and the target flat row index. (Same-view reorder is
    /// [`reorderable`](Self::reorderable).)
    pub fn accept_foreign_rows(mut self, accept: bool) -> Self {
        self.export.accept_foreign_rows = accept;
        self
    }

    /// Handler for rows accepted via
    /// [`accept_foreign_rows`](Self::accept_foreign_rows): `(items, target
    /// flat row index, ctx)`. Insert them into your tree at/near the index.
    pub fn on_rows_received(
        mut self,
        f: impl Fn(Vec<T>, usize, &mut EventContext) + 'static,
    ) -> Self {
        self.export.set_on_rows_received(f);
        self
    }

    /// Raw escape hatch for a foreign drop.
    ///
    /// **Projection path only.** This hook is `NodeId`-typed and predates
    /// [`from_source`](Self::from_source); over an external source there is no
    /// `NodeId` to hand it, so it never fires. Prefer
    /// [`accept_foreign_rows`](Self::accept_foreign_rows) +
    /// [`on_rows_received`](Self::on_rows_received), which are source-agnostic. Unlike `ListView` / `TableView`,
    /// `TreeTableView` is backed by a concrete `SortFilterTreeModel<T>` rather
    /// than a pluggable source, so it cannot express foreign-accept purely
    /// through source capability closures (`can_accept` / `accept_drop`).
    /// This fires for **any** payload NOT recognized as this view's own row
    /// drag — a different view's [`RowDragData<T>`](crate::RowDragData), or a
    /// completely different payload type — dropped on a node: `(payload,
    /// target node, drop position, ctx) -> accepted`. Tried after
    /// [`on_rows_received`](Self::on_rows_received), so the typed sugar wins
    /// when both are set and the payload happens to carry an exportable
    /// `RowDragData<T>`.
    pub fn on_foreign_drop(
        mut self,
        f: impl Fn(&DragPayload, NodeId, DropPosition, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_foreign_drop = Some(Rc::new(f));
        self
    }

    /// Choose single- vs double-click activation for `on_row_activate` (default
    /// [`ActivateOn::DoubleClick`](crate::ActivateOn)). Enter/Space activates in
    /// either mode.
    pub fn activate_on(mut self, mode: crate::data_views::ActivateOn) -> Self {
        self.activate_on = mode;
        self
    }

    /// Append multiple columns from an iterator.
    pub fn columns(mut self, cols: impl IntoIterator<Item = Column<T>>) -> Self {
        self.columns.extend(cols);
        self
    }

    /// Designate which column hosts the twist + indent. Default: the
    /// first column.
    pub fn tree_column(mut self, col_id: impl Into<String>) -> Self {
        self.tree_column_id = Some(col_id.into());
        self
    }

    /// Override the per-depth indent in the tree column in logical pixels (default
    /// comes from the active `TableStyle`).
    pub fn indent_per_level(mut self, px: f32) -> Self {
        self.indent_per_level = Some(px);
        self
    }

    /// Re-materialize `self.row_metrics` after a height-mode /
    /// row-height builder call.
    fn remake_metrics(&self) {
        *self.row_metrics.borrow_mut() = self
            .height_source
            .make_metrics(self.effective_row_height(), 0.0);
    }

    /// Fixed row height (default: the table style's 28 px) — the
    /// uniform fast path. Mutually exclusive with
    /// [`row_height_fn`](Self::row_height_fn) and
    /// [`auto_row_height`](Self::auto_row_height); the last mode setter
    /// wins.
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = Some(height);
        self.height_source = HeightSource::Uniform;
        self.remake_metrics();
        self
    }

    /// Per-row heights from a callback over the flat (visible) row
    /// index. The callback must be pure (same index + same data → same
    /// height); it is re-swept from the first changed flat index on
    /// every projection rebuild (expand/collapse/sort/filter/mutation).
    /// No measurement pass runs.
    pub fn row_height_fn(mut self, f: impl Fn(usize) -> f32 + 'static) -> Self {
        self.height_source = HeightSource::Exact(Rc::new(f));
        self.remake_metrics();
        self
    }

    /// Auto-measured row heights: each realized row reports the height
    /// of its tallest cell measured at the cell's column width
    /// (height-for-width), unrealized rows assume `estimated`. Scroll
    /// anchoring keeps content above the viewport stationary; measured
    /// heights above a toggled row survive expand/collapse
    /// (divergence-driven invalidation). The scrollbar settles one
    /// frame after a measurement change.
    pub fn auto_row_height(mut self, estimated: f32) -> Self {
        self.height_source = HeightSource::Auto { estimated };
        self.remake_metrics();
        self
    }

    /// Override the header row height in logical pixels.
    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = Some(height);
        self
    }

    /// Show or hide the column header row (default `true`).
    pub fn show_header(mut self, visible: bool) -> Self {
        self.show_header = visible;
        self
    }

    /// Set the row/cell selection mode (default
    /// [`TableSelectionMode::MultiRow`]).
    pub fn selection_mode(mut self, mode: TableSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    /// Set the index-based row selection model (visible positions). For
    /// identity-based selection that survives expand / collapse / sort /
    /// filter / structural edits, use [`keyed_selection`](Self::keyed_selection)
    /// instead.
    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.row_selection = Some(RowSelection::from_index(sel));
        self
    }

    /// Set a keyed row selection model (by `NodeId`). Selection is tracked by
    /// node identity, so it survives expand / collapse, sort / filter, and node
    /// moves — and stays consistent if two views share the projection. Pruned
    /// of deleted nodes on each projection change. Mutually exclusive with
    /// [`selection`](Self::selection) (last one set wins).
    /// Only meaningful on the [`from_projection`](Self::from_projection) /
    /// [`new`](Self::new) paths, whose identity *is* `NodeId`; a no-op over an
    /// external source, which carries its own key — use
    /// [`from_source_keyed`](Self::from_source_keyed) there.
    pub fn keyed_selection(mut self, keyed: KeyedSelectionModel<NodeId>) -> Self {
        let Some(proxy) = self.proxy.clone() else {
            return self;
        };
        let key_at = {
            let p = proxy.clone();
            Rc::new(move |i| p.visible_node_id(i)) as Rc<dyn Fn(usize) -> Option<NodeId>>
        };
        let len = {
            let p = proxy.clone();
            Rc::new(move || p.visible_count()) as Rc<dyn Fn() -> usize>
        };
        // A collapsed-but-present node must NOT be pruned, so existence is
        // checked against the tree, not the (visible) projection window.
        let contains = {
            let p = proxy;
            Rc::new(move |n: &NodeId| p.tree().with_item(*n, |_| ()).is_some())
                as Rc<dyn Fn(&NodeId) -> bool>
        };
        self.row_selection = Some(RowSelection::from_keyed(keyed, key_at, len, contains));
        self
    }

    /// Attach a cell-level selection model (row and column axes tracked
    /// independently).
    pub fn cell_selection(mut self, sel: CellSelectionModel) -> Self {
        self.cell_selection = Some(sel);
        self
    }

    /// Paint odd-indexed rows with the `SurfaceRole::AlternatingRow` tint
    /// (default `false`).
    pub fn alternating_rows(mut self, enabled: bool) -> Self {
        self.alternating_rows = enabled;
        self
    }

    /// Paint horizontal and/or vertical dividers between cells.
    pub fn grid_lines(mut self, kind: GridLines) -> Self {
        self.grid_lines = kind;
        self
    }

    /// Accessible label for the whole tree table, announced by AT as the
    /// table's name.
    pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

    /// Show or hide the widget's internal vertical and horizontal scroll bars
    /// (default `true`). Set to `false` when the table lives inside an external
    /// `ScrollArea`.
    pub fn show_internal_scrollbars(mut self, show: bool) -> Self {
        self.show_internal_scrollbars = show;
        self
    }

    /// Control how column widths are distributed when the table is resized
    /// (default `Proportional`).
    pub fn column_resize_policy(mut self, policy: ColumnResizePolicy) -> Self {
        self.column_resize_policy = policy;
        self
    }

    /// Set the keyboard Tab traversal direction inside the table (default `Cells`).
    pub fn tab_traversal(mut self, mode: TabTraversal) -> Self {
        self.tab_traversal = mode;
        self
    }

    /// Set which user gesture starts an in-place cell edit (default
    /// `DoubleClick`).
    pub fn edit_trigger(mut self, trigger: EditTrigger) -> Self {
        self.edit_trigger = trigger;
        self
    }

    /// Callback invoked when the user requests an in-place cell edit (e.g.
    /// double-click when `edit_trigger` is `DoubleClick`). Receives the flat row
    /// index, the column id, and a mutable `EventContext`.
    pub fn on_cell_edit_request(
        mut self,
        f: impl Fn(usize, &str, &mut EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_request = Some(Rc::new(f));
        self
    }

    /// Callback invoked when a row is activated (double-click or Enter, per
    /// `activate_on`). Receives the flat row index.
    pub fn on_row_activate(mut self, f: impl Fn(usize, &mut EventContext) + 'static) -> Self {
        self.on_row_activate = Some(Rc::new(f));
        self
    }

    /// Forward `mode` to the underlying projection. The proxy holds its
    /// state behind `Rc<RefCell>`, so calling `.filter_mode()` on a
    /// clone mutates the shared inner — effectively persisting the
    /// choice on `self.proxy`.
    pub fn filter_mode(self, mode: TreeFilterMode) -> Self {
        if let Some(p) = &self.proxy {
            let _ = p.clone().filter_mode(mode);
        }
        self
    }

    // ── Reactive signals ──────────────────────────────────────────────

    /// Current vertical scroll offset in logical pixels.
    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    /// Maximum vertical scroll offset (content height − viewport height).
    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    /// Viewport-to-content height ratio — drives the scrollbar thumb size.
    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    /// Active sort state: `Some((col_id, direction))` or `None` for unsorted.
    ///
    /// **This is the header's state, not the data's.** Clicking a sort header
    /// writes here; nothing reorders rows until you bind this onto the backing
    /// projection yourself:
    ///
    /// ```ignore
    /// let proxy = SortFilterTreeModel::new(tree)
    ///     .with_comparator("name", |a: &Row, b: &Row| a.name.cmp(&b.name));
    /// proxy.sort_signal(view.sort_signal().clone());
    /// ```
    ///
    /// The binding is deliberately not automatic: a projection may already
    /// carry preset comparators, predicates, and a filter mode, and adopting
    /// the view's empty signal at construction would clobber them.
    pub fn sort_signal(&self) -> &Signal<Option<(String, SortDirection)>> {
        &self.sort_signal
    }

    /// Active per-column filters keyed by column id.
    ///
    /// Like [`sort_signal`](Self::sort_signal), this holds the header's state
    /// only — bind it onto the projection to actually filter rows:
    ///
    /// ```ignore
    /// let proxy = SortFilterTreeModel::new(tree)
    ///     .with_predicate("name", |t| {
    ///         let needle = t.to_string();
    ///         Box::new(move |r: &Row| r.name.contains(&needle))
    ///     });
    /// proxy.filters_signal(view.filters_signal().clone());
    /// ```
    pub fn filters_signal(&self) -> &Signal<HashMap<String, String>> {
        &self.filters_signal
    }

    /// Current column widths in logical pixels, keyed by column id.
    pub fn column_widths_signal(&self) -> &Signal<HashMap<String, f32>> {
        &self.column_widths_signal
    }

    /// Current column display order as a list of column ids.
    pub fn column_order_signal(&self) -> &Signal<Vec<String>> {
        &self.column_order_signal
    }

    /// Keyboard-focused cell as `(row, display_column_index)`, or `None`.
    pub fn focused_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.focused_cell
    }

    /// Cell currently being edited as `(row, display_column_index)`, or `None`.
    pub fn editing_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.editing_cell
    }

    /// Access the underlying `SortFilterTreeModel` (for programmatic sort /
    /// filter / expand outside of the builder API).
    /// `None` when the view was built from an external
    /// [`bastyde_data::TreeDataSource`] via
    /// [`from_source`](Self::from_source) — there is no `TreeModel`-backed
    /// projection to hand back in that case.
    pub fn projection(&self) -> Option<&SortFilterTreeModel<T>> {
        self.proxy.as_ref()
    }

    // ── Imperative API ─────────────────────────────────────────────────

    /// Expand the subtree rooted at `node`.
    pub fn expand(&self, node: NodeId) {
        if let Some(p) = &self.proxy {
            p.expand(node);
        }
    }

    /// Collapse the subtree rooted at `node`.
    pub fn collapse(&self, node: NodeId) {
        if let Some(p) = &self.proxy {
            p.collapse(node);
        }
    }

    /// Toggle the expand/collapse state of `node`.
    pub fn toggle(&self, node: NodeId) {
        if let Some(p) = &self.proxy {
            p.toggle(node);
        }
    }

    /// Expand all nodes in the tree.
    pub fn expand_all(&self) {
        if let Some(p) = &self.proxy {
            p.expand_all();
        }
    }

    /// Collapse all nodes in the tree.
    pub fn collapse_all(&self) {
        if let Some(p) = &self.proxy {
            p.collapse_all();
        }
    }

    /// Move keyboard focus to the cell at `(row, col)`.
    pub fn set_focused_cell(&self, row: usize, col: usize) {
        self.focused_cell.set(Some((row, col)));
    }

    /// Clear the keyboard-focused cell.
    pub fn clear_focused_cell(&self) {
        self.focused_cell.set(None);
    }

    /// Programmatically sort by `col_id` (pass `None` to clear the sort).
    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        self.sort_signal.set(col_id.map(|c| (c.to_string(), dir)));
    }

    /// Set or clear the filter text for a single column.
    pub fn set_filter(&self, col_id: &str, text: &str) {
        let mut m = self.filters_signal.get();
        if text.is_empty() {
            m.remove(col_id);
        } else {
            m.insert(col_id.to_string(), text.to_string());
        }
        self.filters_signal.set(m);
    }

    pub fn clear_filters(&self) {
        self.filters_signal.set(HashMap::new());
    }

    /// Widget shown when no rows are visible — an empty tree, or a filter
    /// that matched nothing. Without one, the body region is simply blank.
    pub fn empty_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.empty_view = Some(Rc::new(f));
        self
    }

    /// Clear the active sort.
    pub fn clear_sort(&self) {
        self.sort_signal.set(None);
    }

    /// Scroll so that `row` is aligned to the top of the viewport. A no-op
    /// before the first layout pass.
    pub fn scroll_to_row(&self, row: usize) {
        if !self.laid_out.get() {
            return;
        }
        imperative::scroll_to_row(row, &self.row_metrics, &self.scroll_y, &self.max_scroll_y);
    }

    /// Scroll the minimum distance needed to make `row` visible. A no-op
    /// before the first layout pass, when the viewport height is not yet known.
    pub fn ensure_row_visible(&self, row: usize) {
        imperative::ensure_row_visible(
            row,
            &self.row_metrics,
            &self.scroll_y,
            &self.max_scroll_y,
            self.viewport_height.get(),
            self.laid_out.get(),
        );
    }

    /// Set or remove a single column's user-resized width override.
    /// A non-positive `width` removes the entry (the column reverts to
    /// its declared width policy).
    pub fn set_column_width(&self, col_id: &str, width: f32) {
        imperative::set_column_width(&self.column_widths_signal, col_id, width);
    }

    /// Replace the full width-override map (typically used to restore
    /// a persisted layout).
    pub fn set_column_widths(&self, widths: HashMap<String, f32>) {
        self.column_widths_signal.set(widths);
    }

    /// Replace the column-order list. Ids not declared on this table
    /// are silently dropped on the next layout pass.
    pub fn set_column_order(&self, order: Vec<String>) {
        self.column_order_signal.set(order);
    }

    /// Current column pinning overrides, keyed by column id. Wins over
    /// each column's declared [`Column::pinned`].
    pub fn column_pinning_signal(&self) -> &Signal<HashMap<String, PinnedSide>> {
        &self.column_pinning_signal
    }

    /// Pin or unpin a single column. [`PinnedSide::None`] removes the
    /// override, reverting the column to its declared pinning.
    pub fn set_column_pinning(&self, col_id: &str, side: PinnedSide) {
        imperative::set_column_pinning(&self.column_pinning_signal, col_id, side);
    }

    /// Begin editing the cell `(row, col_id)`. Silently no-ops if `col_id`
    /// isn't a currently-displayed column, or if `row` is outside the visible
    /// range — an out-of-range target would otherwise strand `editing_cell` on
    /// a row nothing can match.
    ///
    /// Callable **before the view is mounted**, which is the only point at
    /// which a consumer can seed a freshly constructed view with an edit
    /// target it already holds. `display_indices` is a cache `build()` fills,
    /// so a pre-mount call finds it empty; the order is recomputed on demand
    /// in that case rather than resolving against nothing and no-opping for a
    /// third, undocumented reason.
    pub fn begin_edit(&self, row: usize, col_id: &str) {
        let cached = self.display_indices.borrow();
        let recomputed;
        let display: &[usize] = if cached.is_empty() {
            recomputed = self.display_order();
            &recomputed
        } else {
            &cached
        };
        if let Some(target) = imperative::resolve_edit_target(
            row,
            col_id,
            &self.columns,
            display,
            self.source.visible_count(),
        ) {
            drop(cached);
            self.editing_cell.set(Some(target));
        }
    }

    /// Close the active cell editor without committing (the field's `on_blur` still fires).
    pub fn end_edit(&self) {
        self.editing_cell.set(None);
    }

    // ── Internals ──────────────────────────────────────────────────────

    fn effective_row_height(&self) -> f32 {
        self.row_height.unwrap_or(cp::ROW_HEIGHT)
    }

    fn effective_header_height(&self) -> f32 {
        if self.show_header {
            self.header_height.unwrap_or(cp::HEADER_HEIGHT)
        } else {
            0.0
        }
    }

    fn effective_indent(&self) -> f32 {
        self.indent_per_level.unwrap_or(cp::TREE_INDENT_PER_LEVEL)
    }

    /// Resolve the tree column id to a declaration index. Falls back
    /// to column 0 when the configured id isn't found or unset.
    fn tree_column_decl_index(&self) -> usize {
        if let Some(ref id) = self.tree_column_id {
            for (i, col) in self.columns.iter().enumerate() {
                if &col.id == id {
                    return i;
                }
            }
        }
        0
    }

    fn display_order(&self) -> Vec<usize> {
        let order_signal = self.column_order_signal.get();
        let mut order_map: HashMap<&str, usize> = HashMap::new();
        for (i, id) in order_signal.iter().enumerate() {
            order_map.insert(id.as_str(), i);
        }
        let mut leading: Vec<usize> = Vec::new();
        let mut middle: Vec<usize> = Vec::new();
        let mut trailing: Vec<usize> = Vec::new();
        for (i, col) in self.columns.iter().enumerate() {
            let pinning = self
                .column_pinning_signal
                .get()
                .get(&col.id)
                .copied()
                .unwrap_or(col.pinned);
            match pinning {
                PinnedSide::Leading => leading.push(i),
                PinnedSide::None => middle.push(i),
                PinnedSide::Trailing => trailing.push(i),
            }
        }
        const FALLBACK_BASE: usize = usize::MAX / 2;
        let cols = &self.columns;
        let key_for = |i: usize| {
            order_map
                .get(cols[i].id.as_str())
                .copied()
                .unwrap_or(FALLBACK_BASE + i)
        };
        leading.sort_by_key(|&i| key_for(i));
        middle.sort_by_key(|&i| key_for(i));
        trailing.sort_by_key(|&i| key_for(i));
        let mut out = Vec::with_capacity(leading.len() + middle.len() + trailing.len());
        out.extend(leading);
        out.extend(middle);
        out.extend(trailing);
        out
    }

    fn clamp_scroll(&self) {
        let max = self.max_scroll_y.get();
        let current = self.scroll_y.get();
        let clamped = current.clamp(0.0, max);
        if (clamped - current).abs() > 0.001 {
            self.scroll_y.set(clamped);
        }
    }

    /// Buffered realized range — mirrors `TableView::visible_range`. Used
    /// only to nudge the lazy source (`request_window`/`fetch_more`); the
    /// pane recomputes its own copy independently for actual row
    /// realization.
    fn visible_range(&self) -> (usize, usize) {
        self.row_metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.visible_count(),
            BUFFER_ROWS,
        )
    }
}

impl<T: 'static> std::fmt::Debug for TreeTableView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeTableView")
            .field("rows", &self.source.visible_count())
            .field("columns", &self.columns.len())
            .field("tree_column", &self.tree_column_id)
            .field("scroll_bar_style", &self.scroll_bar_style)
            .finish()
    }
}

impl<T: 'static> Widget for TreeTableView<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        let row_h = self.effective_row_height();
        let header_h = self.effective_header_height();
        let indent_per_level = self.effective_indent();

        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Pane → root total refresh (auto-measure mode): re-place this
        // root when the body pane's measurements changed the content
        // total, so `max_scroll_y` / the thumb ratio pick up the
        // corrected value.
        self.pane_total_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        self.column_widths_signal.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // Also at AccessibilityOnly (orthogonal — see `BindingLevel`) so a
        // keyboard focus move re-walks the AT tree and re-resolves
        // `active_descendant` in `accessibility()` below, even though
        // nothing about the cell's own node changed.
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Focus-aware selection + modality-gated focus ring (mirrors TableView).
        // `begin_view_focus` keys the scope signal on this root id directly —
        // the same id the body pane uses for its row scope, and independent of
        // the arena focusable flag (not yet wired here). A plain
        // `view_focus_active()` would find no focusable ancestor and fall back
        // to the constant-`true` "outside any scope" signal, lighting the ring
        // whenever ANY widget takes focus. Pop straight back; the body pane
        // re-pushes the same cached signal. `focus_visible` is the
        // keyboard/pointer modality. Both `RepaintOnly`.
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
        // Row-drop insertion indicator at RepaintOnly so on_drag_hover /
        // on_drag_leave `set(...)` calls dirty paint without a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Bump version on projection version (data + sort/filter +
        // expand/collapse all in one signal). Proxy observers fire
        // synchronously per rebuild, so `first_changed_index()`
        // describes exactly this change — heights of flat rows before
        // it (e.g. above an expand/collapse point) stay valid.
        let v_for_proj = version.clone();
        let proj_ver = Rc::new(Cell::new(0_u64));
        let prev_visible_count = Rc::new(Cell::new(self.source.visible_count()));
        ctx.effect(&self.source.version_signal(), {
            let metrics = self.row_metrics.clone();
            let src = self.source.clone();
            let row_sel = self.row_selection.clone();
            let cell_sel = self.cell_selection.clone();
            let prev_visible_count = prev_visible_count.clone();
            move |_| {
                metrics
                    .borrow_mut()
                    .apply_divergence(src.first_changed_index(), src.visible_count());
                // Drop any keyed selection whose node was deleted (no-op for
                // the index model). Cheap; runs on every projection change.
                if let Some(ref rs) = row_sel {
                    rs.prune();
                }
                // Cell selection is index-based (unlike the keyed row
                // selection above), and a `TreeDataSource`'s flattening
                // collapses every structural change — expand/collapse,
                // insert/remove, a re-sort — into one version bump with no
                // per-change delta to follow, unlike `TableView`'s
                // `ListModel` `DataChange` granularity. A changed visible
                // row count is a structural signal we CAN act on
                // honestly: clear the selection rather than let it point
                // at whatever node now occupies that flat index. Leave it
                // alone when the count is unchanged — a content-only
                // update (e.g. an in-place item edit) never moves a row,
                // and clearing on every projection bump would drop the
                // selection on a plain data refresh.
                let new_visible_count = src.visible_count();
                if let Some(ref cs) = cell_sel
                    && new_visible_count != prev_visible_count.get()
                {
                    cs.clear();
                }
                prev_visible_count.set(new_visible_count);
                let next = proj_ver.get() + 1;
                proj_ver.set(next);
                v_for_proj.set(next);
            }
        });

        // Sort + filter signals are NOT auto-bound onto the proxy.
        // The proxy may already carry preset comparators/predicates
        // and a custom filter mode; auto-binding would clobber them.
        // Callers wire the proxy explicitly:
        //
        //   proxy.sort_signal(tree_table.sort_signal().clone());
        //   proxy.filters_signal(tree_table.filters_signal().clone());
        //
        // Documented in the module-level comment.

        let v_for_sort = version.clone();
        let sv = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.sort_signal, move |_| {
            let next = sv.get() + 1;
            sv.set(next);
            v_for_sort.set(next);
        });
        let v_for_order = version.clone();
        let ov = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_order_signal, move |_| {
            let next = ov.get() + 1;
            ov.set(next);
            v_for_order.set(next);
        });
        let v_for_pin = version.clone();
        let pv = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_pinning_signal, move |_| {
            let next = pv.get() + 1;
            pv.set(next);
            v_for_pin.set(next);
        });
        // Selection / focus / editing effects live on the TreeBodyPane
        // (they only affect row content) — rebuilding the pane instead
        // of the root keeps those rebuilds out of the scrollbar's
        // ancestor chain during a thumb drag.

        // Display order.
        let display_indices = self.display_order();

        // Remap any `(row, display_pos)` pairs the *previous* order left in
        // `focused_cell` / `editing_cell` / `cell_selection` onto their
        // column's position under the order just computed, before it
        // overwrites `self.display_indices` below. See the identical block
        // in `TableView::build` for why this is a no-op unless THIS
        // rebuild's cause was a column reorder/pinning change.
        {
            let old_display = self.display_indices.borrow();
            if !old_display.is_empty() {
                let old_to_new: Vec<Option<usize>> = old_display
                    .iter()
                    .map(|&decl_idx| {
                        let id = &self.columns[decl_idx].id;
                        display_indices
                            .iter()
                            .position(|&new_decl_idx| self.columns[new_decl_idx].id == *id)
                    })
                    .collect();
                drop(old_display);
                imperative::remap_cell_state(
                    &self.focused_cell,
                    &self.editing_cell,
                    self.cell_selection.as_ref(),
                    &old_to_new,
                );
            }
        }
        *self.display_indices.borrow_mut() = display_indices.clone();
        let tree_decl = self.tree_column_decl_index();
        let tree_display_pos = display_indices
            .iter()
            .position(|&i| i == tree_decl)
            .unwrap_or(0);

        // Self handlers: scroll wheel + keyboard.
        let scroll_y_for_wheel = self.scroll_y.clone();
        let max_scroll_for_wheel = self.max_scroll_y.clone();
        let line_height = row_h;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;

        let column_ids_in_display_order: Vec<String> = display_indices
            .iter()
            .map(|&i| self.columns[i].id.clone())
            .collect();
        let display_col_to_id: Rc<dyn Fn(usize) -> Option<String>> = {
            let ids = column_ids_in_display_order;
            Rc::new(move |pos| ids.get(pos).cloned())
        };
        let display_col_editable: Rc<dyn Fn(usize) -> bool> = {
            let editable_in_display_order: Vec<bool> = display_indices
                .iter()
                .map(|&i| self.columns[i].editable)
                .collect();
            Rc::new(move |pos| editable_in_display_order.get(pos).copied().unwrap_or(false))
        };

        let navigator: Rc<dyn RowNavigator> = Rc::new(TreeNavigator::new(self.source.clone()));
        // Type-ahead resolver: read the visible row's item text through the
        // projection (`None` if the flat index isn't currently visible).
        let type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>> =
            self.type_ahead_label.clone().map(|user| {
                let src = self.source.clone();
                Rc::new(move |i: usize| src.with_row_str(i, &|item| user(item)))
                    as Rc<dyn Fn(usize) -> Option<String>>
            });

        let key_cfg = keyboard::KeyHandlerConfig {
            navigator,
            col_count: display_indices.len().max(1),
            // The same resolved position the twist and indent gutter render at
            // (see `tree_display_pos` above), so the arrow keys keep following
            // the chevron when `.tree_column()` or a user column-reorder moves
            // it off the leading position.
            tree_column_display_pos: tree_display_pos,
            focused_cell: self.focused_cell.clone(),
            selection_mode: self.selection_mode,
            selection: self.row_selection.clone(),
            cell_selection: self.cell_selection.clone(),
            scroll_y: self.scroll_y.clone(),
            max_scroll_y: self.max_scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            body_bounds: self.body_bounds.clone(),
            row_metrics: self.row_metrics.clone(),
            tab_traversal: self.tab_traversal,
            editing_cell: self.editing_cell.clone(),
            edit_trigger: self.edit_trigger,
            display_col_to_id,
            display_col_editable,
            on_cell_edit_request: self.on_cell_edit_request.clone(),
            on_row_activate: self.on_row_activate.clone(),
            type_ahead: self.type_ahead.clone(),
            type_ahead_label,
            type_ahead_timeout: self.type_ahead_timeout,
        };

        // Alt+Arrow tree sibling reorder wraps the shared key handler: a move
        // among the node's siblings in the underlying `TreeModel` (cycle-free
        // by construction). Suppressed while sorted. Every other key falls
        // through to the navigator (cell/row movement, expand/collapse, edit).
        let mut shared_key = keyboard::build_key_handler(key_cfg);
        let reorderable_kbd = self.reorderable;
        let source_kbd = self.source.clone();
        let focused_kbd = self.focused_cell.clone();
        let sel_kbd = self.row_selection.clone();
        let sort_kbd = self.sort_signal.clone();
        let key_handler = move |event: &bastyde_core::event::WidgetEvent,
                                ctx: &mut EventContext|
              -> EventResponse {
            use bastyde_core::event::{Key, WidgetEvent};
            if reorderable_kbd
                && sort_kbd.get().is_none()
                && let WidgetEvent::KeyDown { key, modifiers, .. } = event
                && modifiers.alt()
                && matches!(key, Key::ArrowUp | Key::ArrowDown)
            {
                let row = focused_kbd.get().map(|(r, _)| r).or_else(|| {
                    sel_kbd
                        .as_ref()
                        .and_then(|s| s.selected_indices().first().copied())
                });
                // Sibling reorder + the "follow the moved row" bookkeeping live
                // in the source (key-typed there, so it works for an external
                // store too) and hand back the row's new flat index.
                if let Some(flat_idx) = row
                    && let Some(new_flat) =
                        source_kbd.keyboard_reorder(flat_idx, matches!(key, Key::ArrowDown))
                {
                    let col = focused_kbd.get().map(|(_, c)| c).unwrap_or(0);
                    focused_kbd.set(Some((new_flat, col)));
                    if let Some(ref s) = sel_kbd {
                        s.select(new_flat);
                    }
                    return EventResponse::Handled;
                }
            }
            shared_key(event, ctx)
        };

        let mut handlers = HandlerSet::new()
            .on_scroll({
                let overscroll_behavior = self.overscroll_behavior;
                move |event, _ctx| match event {
                    bastyde_core::event::WidgetEvent::Scroll { delta, .. } => {
                        let dy = match delta {
                            bastyde_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                            bastyde_core::event::ScrollDelta::Pixels { y, .. } => *y,
                        };
                        let current = scroll_y_for_wheel.get();
                        let max = max_scroll_for_wheel.get();
                        // Base off the animation target (not the rendered offset)
                        // so a mid-fling boundary correctly chains and successive
                        // notches accumulate instead of restarting from the
                        // partway-animated position.
                        let base = scroll_y_for_wheel.animation_target().unwrap_or(current);
                        let (new_y, moved) =
                            crate::common::scroll::scroll_clamp_axis(base, dy, max);
                        if moved {
                            if smooth_scrolling {
                                scroll_y_for_wheel.animate_to(
                                    new_y,
                                    smooth_scroll_duration,
                                    Easing::EaseOut,
                                );
                            } else {
                                scroll_y_for_wheel.set(new_y);
                            }
                        }
                        // Chain to an ancestor scrollable when fully
                        // clamped (unless Contain), otherwise consume —
                        // same contract as ListView/TreeView/TableView.
                        crate::common::scroll::scroll_response(
                            moved,
                            overscroll_behavior == OverscrollBehavior::Contain,
                        )
                    }
                    _ => EventResponse::Ignored,
                }
            })
            .on_key(key_handler)
            .clips_children(true)
            .focusable(true);

        // Row DnD: same-view reorder (reorderable) reparents/reorders the
        // dragged node(s) in the underlying `TreeModel`, cycle-guarded and
        // suppressed while sorted; plus optional foreign receive
        // (accept_foreign_rows / on_foreign_drop). Registered whenever ANY
        // of the three capabilities is enabled — a foreign-receive-only view
        // (reorderable == false) still needs to be a drop target.
        // NOTE: row DnD is still `NodeId`-typed, so it is registered only on the
        // projection path. A source-backed view (`from_source`) gets every other
        // capability but no built-in row drag yet — routing this through
        // `source.dnd.{can_accept,accept_drop}_fn` (as `TreeView` already does)
        // is a follow-up, because those closures also carry Into/Before/After
        // redirect semantics this widget does not model yet.
        // Row DnD: same-view reorder/reparent plus foreign receive, both routed
        // through the source's `can_accept` / `accept_drop` capability closures
        // — so this works over a `TreeModel`-backed projection AND an external
        // `TreeDataSource`, exactly like `TreeView`. Drop zones are the row's
        // thirds (Before / Into / After); the source's verdict decides the
        // effective position and may `Redirect` (e.g. Into-a-leaf becomes
        // After). Suppressed while sorted, where a manual order has no meaning.
        if self.export.is_drop_target(self.reorderable) || self.on_foreign_drop.is_some() {
            let my_model_id = self.model_id;
            let source_for_hover = self.source.clone();
            let metrics_for_hover = self.row_metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let header_h_for_hover = header_h;
            let feedback_for_hover = self.drop_feedback.clone();
            let sort_for_hover = self.sort_signal.clone();
            let reorderable_hover = self.reorderable;
            let export_for_hover = self.export.clone();
            let has_foreign_hook_hover = self.on_foreign_drop.is_some();
            let bounds_for_hover = self.body_bounds.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                // Real body width, so the affordance spans the actual row area
                // rather than a placeholder.
                let viz_width = bounds_for_hover.get().width.max(1.0);
                let count = source_for_hover.visible_count();
                if count == 0 {
                    feedback_for_hover.set(None);
                    return bastyde_core::DropFeedback::NoFeedback;
                }
                let rd = payload.get_typed::<RowDragData<T>>();
                let is_same_view = rd.is_some_and(|r| r.source == my_model_id);
                let reorder_ok =
                    is_same_view && reorderable_hover && sort_for_hover.get().is_none();
                // The typed `accept_foreign_rows`/`on_rows_received` path can
                // only consume an EXPORT payload (items present); the raw
                // `on_foreign_drop` hook takes any foreign payload.
                let foreign_ok = !is_same_view
                    && (has_foreign_hook_hover
                        || export_for_hover.accepts_foreign_export(payload, my_model_id));
                if !reorder_ok && !foreign_ok {
                    feedback_for_hover.set(None);
                    return bastyde_core::DropFeedback::NoFeedback;
                }
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y - header_h_for_hover + scroll;
                let (insertion_top, row_idx, row_top, row_h) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(count);
                    let ins = m.insertion_index(content_y);
                    let r = m.row_at(content_y);
                    (m.row_top(ins), r, m.row_top(r), m.row_height(r))
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
                // The source owns the structural verdict — including the cycle
                // guard (a node may not land inside its own subtree), which used
                // to be re-derived here against the `TreeModel`.
                let effective = if reorder_ok {
                    match (source_for_hover.dnd.can_accept_fn)(
                        payload,
                        row_idx,
                        drop_pos,
                        my_model_id,
                    ) {
                        DropResponse::Reject => {
                            if !foreign_ok {
                                feedback_for_hover.set(None);
                                return bastyde_core::DropFeedback::NoFeedback;
                            }
                            DropPosition::Before
                        }
                        DropResponse::Accept => drop_pos,
                        DropResponse::Redirect(p) => p,
                    }
                } else {
                    // A foreign source has no Into/reparent semantics to honor.
                    DropPosition::Before
                };
                if effective == DropPosition::Into {
                    let top = row_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Rect {
                        top,
                        height: row_h,
                        width: viz_width,
                    }));
                    bastyde_core::DropFeedback::HighlightRect {
                        rect: Rect::new(0.0, top, viz_width, row_h),
                        color: drop_into_tint(),
                    }
                } else {
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Line {
                        y: insertion_y,
                        width: viz_width,
                    }));
                    bastyde_core::DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: viz_width,
                    }
                }
            });

            let drop_model_id = self.model_id;
            let source_for_drop = self.source.clone();
            let metrics_for_drop = self.row_metrics.clone();
            let scroll_for_drop = self.scroll_y.clone();
            let header_h_for_drop = header_h;
            let feedback_for_drop = self.drop_feedback.clone();
            let sort_for_drop = self.sort_signal.clone();
            let reorderable_drop = self.reorderable;
            let on_foreign_for_drop = self.on_foreign_drop.clone();
            let proxy_for_foreign_hook = self.proxy.clone();
            let export_for_drop = self.export.clone();
            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                feedback_for_drop.set(None);
                let count = source_for_drop.visible_count();
                if count == 0 {
                    return false;
                }
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y - header_h_for_drop + scroll;
                let (flat_idx, row_top, row_h, ins) = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(count);
                    let idx = m.row_at(content_y);
                    let ins = m.insertion_index(content_y);
                    (idx, m.row_top(idx), m.row_height(idx), ins)
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
                    .is_some_and(|rd| rd.source == drop_model_id);
                if is_same_view && (!reorderable_drop || sort_for_drop.get().is_some()) {
                    return false;
                }
                // The source applies the move (cycle-guarded, undo-aware for an
                // external store) and reports whether it took. Gated exactly as
                // `TreeView` does, so a foreign payload the source does NOT
                // recognise still reaches the `on_rows_received` sugar below.
                if (reorderable_drop || !is_same_view)
                    && (source_for_drop.dnd.accept_drop_fn)(
                        &payload,
                        flat_idx,
                        drop_pos,
                        drop_model_id,
                    )
                {
                    if is_same_view {
                        export_for_drop.note_self_reorder();
                    }
                    return true;
                }
                // Foreign payload: the typed receive sugar first, then the raw
                // escape hatch.
                if export_for_drop.foreign_receive(&mut payload, drop_model_id, ins, ctx) {
                    return true;
                }
                // `on_foreign_drop` predates the source path and is
                // `NodeId`-typed, so it only fires when there is a projection to
                // resolve the target node through.
                if let Some(ref hook) = on_foreign_for_drop
                    && let Some(ref p) = proxy_for_foreign_hook
                    && let Some(node) = p.visible_node_id(flat_idx)
                {
                    return hook(&payload, node, drop_pos, ctx);
                }
                false
            });

            let feedback_for_leave = self.drop_feedback.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
            });

            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            let header_h_for_tick = header_h;
            handlers = handlers.on_drag_tick(move |pos, _ctx| {
                // Auto-scroll near the body band's top/bottom edge during a
                // drag (body-relative so the header doesn't count as the top).
                const EDGE: f32 = 32.0;
                const MAX_VELOCITY: f32 = 12.0;
                let body_h = (viewport_for_tick.get() - header_h_for_tick).max(0.0);
                let y = pos.y - header_h_for_tick;
                let above = (EDGE - y).max(0.0);
                let below = (y - (body_h - EDGE)).max(0.0);
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
            });
        }

        // Export completion (move-out): fires on the drag source — this
        // view's root id, the stable id `start_drag` is given via the body
        // pane's `drag_anchor`. A same-view reorder called
        // `self.export.note_self_reorder()` in `on_drop` above, so it is
        // skipped here (already applied). Absent an
        // `on_rows_transferred_out` override, the default move-out runs the
        // stable-`NodeId` removal thunk `TreeBodyPane::build`'s `on_drag`
        // resolved at drag-start (ascending pre-order, so an already-removed
        // descendant of another dragged node is safely skipped).
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // ── Build children ────────────────────────────────────────────

        self.header_row_id = None;
        self.body_pane_id = None;
        self.scrollbar_id = None;
        self.empty_id = None;

        // Header strip.
        if self.show_header {
            let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
            let active_sort = self.sort_signal.get();
            for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                let col = &self.columns[col_idx];
                let current_sort = active_sort
                    .as_ref()
                    .and_then(|(id, dir)| if id == &col.id { Some(*dir) } else { None });
                let filter_zone_width = cp::FILTER_INDICATOR_SIZE + cp::CELL_PADDING_HORIZONTAL;
                let cell = HeaderCell::new(
                    col.id.clone(),
                    col.header_label.resolve_now(),
                    display_pos + 1,
                    col.sortable,
                    col.resizable,
                    col.reorderable,
                    cp::RESIZE_HANDLE_WIDTH,
                    current_sort,
                    self.sort_signal.clone(),
                    self.column_widths_signal.clone(),
                    self.column_widths.clone(),
                    display_pos,
                    col.min_width.unwrap_or(cp::MIN_COLUMN_WIDTH_DEFAULT),
                    self.column_resize_policy,
                    self.resize_state.clone(),
                    self.table_id,
                    col.filterable,
                    filter_zone_width,
                    self.filters_signal.clone(),
                );
                cell_ids.push(ctx.add(cell));
            }
            let header_row = HeaderRow::new(
                cell_ids,
                self.column_widths.clone(),
                cp::GRID_LINE_THICKNESS,
            );
            self.header_row_id = Some(ctx.add(header_row));
        }

        // Body rows live in a TreeBodyPane — a sibling of the
        // scrollbar, so buffer-exit / selection / editing / expand
        // rebuilds target the pane and are never deferred by the
        // gesture-capture protection during a thumb drag.
        let row_count = self.source.visible_count();

        // Lazy: nudge the source to load the realized window, and fetch
        // the next page as the viewport nears the end (append-only
        // sources). `TreeSource` already erases a `TreeDataSource`'s
        // `row_state`/`request_window`/`can_fetch_more`/`fetch_more`
        // into `self.source.dnd` (mirrors `list_source::DndLazy` — see
        // `TableView::build`); a fully-resident source's default (inert)
        // impls leave this a no-op.
        let (vis_start, vis_end) = self.visible_range();
        (self.source.dnd.request_window_fn)(vis_start..vis_end);
        if (self.source.dnd.can_fetch_more_fn)() && vis_end + BUFFER_ROWS >= row_count {
            (self.source.dnd.fetch_more_fn)();
        }

        if row_count > 0 {
            let pane = body_pane::TreeBodyPane::<T> {
                source: self.source.clone(),
                editing_anchor: self.editing_anchor.clone(),
                columns: self.columns.clone(),
                display_indices: self.display_indices.clone(),
                column_widths: self.column_widths.clone(),
                tree_display_pos,
                indent_per_level,
                row_metrics: self.row_metrics.clone(),
                selection_mode: self.selection_mode,
                selection: self.row_selection.clone(),
                cell_selection: self.cell_selection.clone(),
                scroll_y: self.scroll_y.clone(),
                viewport_height: self.viewport_height.clone(),
                editing_cell: self.editing_cell.clone(),
                focused_cell: self.focused_cell.clone(),
                reorderable: self.reorderable,
                model_id: self.model_id,
                export: self.export.clone(),
                drag_anchor: ctx.self_id(),
                on_row_activate: self.on_row_activate.clone(),
                activate_on: self.activate_on,
                version: self.pane_version.clone(),
                prev_built_start: self.pane_built_start.clone(),
                prev_built_end: self.pane_built_end.clone(),
                total_refresh: self.pane_total_refresh.clone(),
                row_entries: Vec::new(),
                cell_map: self.cell_map.clone(),
            };
            self.body_pane_id = Some(ctx.add(pane));
        } else if let Some(ref f) = self.empty_view {
            // Empty state — an empty tree, or a filter that matched nothing.
            self.empty_id = Some(ctx.add_boxed(f()));
        }

        // Scrollbar.
        if self.show_internal_scrollbars {
            let sb = ScrollBar::new(
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
            self.scrollbar_id = Some(ctx.add(sb));
        }

        // Z-order mirrors TableView: body pane first, header last so it
        // paints above any row that bleeds into the header band on
        // overscroll.
        let mut children: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
            children.push(id);
        }
        if let Some(id) = self.empty_id {
            children.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            children.push(id);
        }
        if let Some(id) = self.header_row_id {
            children.push(id);
        }
        let _ = (header_h, row_h);
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(400.0);
        let height = proposal.height.unwrap_or(300.0);
        self.viewport_height.set(height);
        // Viewport-relative imperatives are meaningful from here on.
        self.laid_out.set(true);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }
        let rtl = ctx.is_rtl();
        let header_h = self.effective_header_height();
        let body_height = (bounds.height - header_h).max(0.0);

        // Parent-before-child layout order means this runs before the
        // body pane's measure pass — in auto-measure mode the scrollbar
        // totals settle one frame after a measurement change.
        let total_height = self
            .row_metrics
            .borrow_mut()
            .total_height(self.source.visible_count());
        let max_y = (total_height - body_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let ratio = if total_height > 0.0 {
            (body_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio);
        self.clamp_scroll();

        let needs_scrollbar = self.show_internal_scrollbars && total_height > body_height + 0.5;
        // Permanent reserves a layout column for the bar; Overlay / Thin
        // float over the content, so the body spans the full width.
        let reserves_bar = needs_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_width = if reserves_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        // RTL mirror (see TableView::place_children): scrollbar to the
        // physical left, body/header band shifted right by its thickness.
        // Only shift when the bar actually reserves a column (Permanent).
        let band_left = if rtl && reserves_bar {
            bounds.x + SCROLLBAR_THICKNESS
        } else {
            bounds.x
        };
        let scrollbar_x = if rtl {
            bounds.x
        } else {
            bounds.x + bounds.width - SCROLLBAR_THICKNESS
        };

        let overrides = self.column_widths_signal.get();
        let display = self.display_indices.borrow().clone();
        let widths = layout::ColumnSolver::resolve_in_order(
            &self.columns,
            &display,
            body_width,
            cp::MIN_COLUMN_WIDTH_DEFAULT,
            &overrides,
        );
        *self.column_widths.borrow_mut() = widths;
        let body_origin_y = bounds.y + header_h;
        // Cache the row-area rect for the keyboard handler's outer-scroll chase.
        self.body_bounds
            .set(Rect::new(band_left, body_origin_y, body_width, body_height));

        let mut next = 0;

        // Body pane fills the body region; it positions its rows
        // internally and clips them to its own bounds.
        if self.body_pane_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(band_left, body_origin_y);
                child.size = Size::new(body_width, body_height);
            }
            next += 1;
        }

        // Empty-state child fills the body region (below the header).
        if self.empty_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(band_left, body_origin_y);
                child.size = Size::new(body_width, body_height);
            }
            next += 1;
        }

        // Scrollbar — alongside the body, below the header.
        if self.scrollbar_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                if needs_scrollbar {
                    child.origin = Point::new(scrollbar_x, body_origin_y);
                    child.size = Size::new(SCROLLBAR_THICKNESS, body_height);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
            next += 1;
        }

        // Header strip last — placed at top y but emitted last so paint
        // z-order draws it above any overscrolled body rows.
        if self.header_row_id.is_some()
            && let Some(child) = children.get_mut(next)
        {
            child.origin = Point::new(band_left, bounds.y);
            child.size = Size::new(body_width, header_h);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let header_h = self.effective_header_height();
        let colors = &ctx.theme.colors;
        let scroll_y = self.scroll_y.get();
        let body_origin_y = bounds.y + header_h;
        let body_height = (bounds.height - header_h).max(0.0);
        let widths = self.column_widths.borrow();
        let body_width = widths.iter().sum::<f32>();
        let body_width_for_paint = if body_width > 0.0 {
            body_width.min(bounds.width)
        } else {
            bounds.width
        };
        // Physical left edge of the column content (see TableView::paint).
        let rtl = ctx.layout_direction == bastyde_core::environment::LayoutDirection::RightToLeft;
        let content_left = if rtl {
            bounds.x + bounds.width - body_width_for_paint
        } else {
            bounds.x
        };

        // Visible row window for the paint passes — offset-table-driven
        // so variable heights paint correctly.
        let row_count = self.source.visible_count();
        let (first_visible, last_visible) =
            self.row_metrics
                .borrow_mut()
                .visible_range(scroll_y, body_height, row_count, 0);

        // Clip the root-painted row decorations (alt-row stripes,
        // selection bands, grid lines, focus ring) to the body band —
        // `clips_children` only clips child widgets, not this widget's
        // own paint, which would otherwise bleed past the bottom edge
        // for the partially visible last row.
        canvas.set_clip(Rect::new(
            content_left,
            body_origin_y,
            body_width_for_paint,
            body_height,
        ));

        if self.alternating_rows {
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in first_visible..last_visible {
                if row_idx % 2 == 1 {
                    let y = body_origin_y + m.row_top(row_idx) - scroll_y;
                    let h = m.row_height(row_idx);
                    let rect = Rect::new(content_left, y, body_width_for_paint, h);
                    canvas.fill_rect(rect, SurfaceRole::AltRow.resolve(colors));
                }
            }
        }

        if let Some(ref sel) = self.row_selection
            && matches!(
                self.selection_mode,
                TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
            )
        {
            // Focus- and window-aware: vivid while the view holds keyboard
            // focus AND the host window is active, muted otherwise (the same
            // `SelectedInactive` serves view-unfocused and window-inactive).
            let bg = if self.view_focused.get() && ctx.window_active {
                SurfaceRole::Selected.resolve(colors)
            } else {
                SurfaceRole::SelectedInactive.resolve(colors)
            };
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in sel.selected_indices() {
                let y = body_origin_y + m.row_top(row_idx) - scroll_y;
                let h = m.row_height(row_idx);
                if y + h < body_origin_y || y > body_origin_y + body_height {
                    continue;
                }
                let rect = Rect::new(content_left, y, body_width_for_paint, h);
                canvas.fill_rect(rect, bg);
            }
        }

        let line_color = BorderRole::Divider.resolve(colors);
        let line_w = cp::GRID_LINE_THICKNESS.max(1.0);
        if matches!(self.grid_lines, GridLines::Horizontal | GridLines::Both) {
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in first_visible..last_visible {
                let bottom = m.row_top(row_idx) + m.row_height(row_idx);
                let y = body_origin_y + bottom - scroll_y - line_w;
                let rect = Rect::new(content_left, y, body_width_for_paint, line_w);
                canvas.fill_rect(rect, line_color);
            }
        }
        if matches!(self.grid_lines, GridLines::Vertical | GridLines::Both) {
            let content_right = content_left + body_width_for_paint;
            if rtl {
                let mut x = content_right;
                for &w in widths.iter() {
                    x -= w;
                    if x > content_left + 0.5 {
                        let rect = Rect::new(x, body_origin_y, line_w, body_height);
                        canvas.fill_rect(rect, line_color);
                    }
                }
            } else {
                let mut x = content_left;
                for &w in widths.iter() {
                    x += w;
                    if x < content_right - 0.5 {
                        let rect = Rect::new(x - line_w, body_origin_y, line_w, body_height);
                        canvas.fill_rect(rect, line_color);
                    }
                }
            }
        }

        // Focus ring — keyboard-only (`:focus-visible`) and only while the
        // view holds focus, so a mouse click never leaves a ring.
        if self.view_focused.get()
            && self.focus_visible.get()
            && let Some((focus_row, focus_col)) = self.focused_cell.get()
            && focus_col < widths.len()
        {
            let mut x_off = 0.0_f32;
            for &w in widths.iter().take(focus_col) {
                x_off += w;
            }
            let cell_w = widths[focus_col];
            let (focus_top, focus_h) = {
                let mut m = self.row_metrics.borrow_mut();
                (m.row_top(focus_row), m.row_height(focus_row))
            };
            let y = body_origin_y + focus_top - scroll_y;
            if y + focus_h >= body_origin_y && y <= body_origin_y + body_height {
                let inset = cp::FOCUS_RING_INSET;
                let stroke = cp::GRID_LINE_THICKNESS.max(1.5);
                let ring_color = BorderRole::Focused.resolve(colors);
                let rx = if rtl {
                    content_left + body_width_for_paint - x_off - cell_w + inset
                } else {
                    content_left + x_off + inset
                };
                let ry = y + inset;
                let rw = (cell_w - inset * 2.0).max(0.0);
                let rh = (focus_h - inset * 2.0).max(0.0);
                canvas.fill_rect(Rect::new(rx, ry, rw, stroke), ring_color);
                canvas.fill_rect(Rect::new(rx, ry + rh - stroke, rw, stroke), ring_color);
                canvas.fill_rect(Rect::new(rx, ry, stroke, rh), ring_color);
                canvas.fill_rect(Rect::new(rx + rw - stroke, ry, stroke, rh), ring_color);
            }
        }

        // Row-drop insertion indicator (source-accepted positions only — a
        // forbidden hover clears the signal). `y` is stored body-local.
        match self.drop_feedback.get() {
            Some(DropViz::Line { y, .. }) => {
                let line_color = BorderRole::Focused.resolve(colors);
                let thickness = 2.0_f32;
                let line_y = body_origin_y + y - thickness * 0.5;
                canvas.fill_rect(
                    Rect::new(content_left, line_y, body_width_for_paint, thickness),
                    line_color,
                );
            }
            // "Drop into this container" — highlight the whole target row, the
            // same affordance `TreeView` paints for an `Into` verdict.
            Some(DropViz::Rect { top, height, .. }) => {
                canvas.fill_rect(
                    Rect::new(
                        content_left,
                        body_origin_y + top,
                        body_width_for_paint,
                        height,
                    ),
                    drop_into_tint(),
                );
            }
            None => {}
        }

        canvas.clear_clip();

        // Container focus ring — keyboard focus on the view but no current cell
        // and no selection, so nothing else marks the focus. Outline the whole
        // view (see TableView / TreeView).
        let nothing_indicated = self.focused_cell.get().is_none()
            && self
                .row_selection
                .as_ref()
                .is_none_or(|s| s.selected_indices().is_empty())
            && self.cell_selection.as_ref().is_none_or(|s| s.count() == 0);
        if self.view_focused.get() && self.focus_visible.get() && nothing_indicated {
            let inset = 1.0_f32;
            let rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rect(rect, BorderRole::Focused.resolve(colors), 1.5);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::TreeGrid);
        if let Some(ref label) = self.a11y_label {
            builder.set_name(label.resolve_now());
        }
        let row_count = self.source.visible_count() + if self.show_header { 1 } else { 0 };
        let col_count = self.columns.len();
        let n = builder.inner_mut();
        n.set_row_count(row_count);
        n.set_column_count(col_count);

        // Roving focus: point active_descendant at the focused cell's own
        // AT node so a screen reader follows arrow-key cell navigation
        // and ArrowLeft/Right expand/collapse. `cell_map` is a snapshot
        // of the body pane's last realized cells; a focused cell that
        // scrolled (or collapsed) out of the realized buffer simply
        // isn't in it, so no stale id is emitted.
        if let Some(target) = self.focused_cell.get() {
            let map = self.cell_map.borrow();
            if let Some(&(_, cell_id)) = map.iter().find(|&&(pos, _)| pos == target) {
                builder.set_active_descendant(widget_id_to_node_id(cell_id));
            }
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        // Same order as `build()` — body pane first, header last so it
        // paints on top of any overscrolled rows.
        let mut out: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
            out.push(id);
        }
        if let Some(id) = self.empty_id {
            out.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            out.push(id);
        }
        if let Some(id) = self.header_row_id {
            out.push(id);
        }
        out
    }

    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        // WCAG 1.3.2 (audit G17): read the column-header row FIRST, then the
        // body, even though `build()` / `children()` list the body first so it
        // paints beneath the header. Same id set as `children()`, reordered.
        let out: Vec<WidgetId> = [
            self.header_row_id,
            self.body_pane_id,
            self.empty_id,
            self.scrollbar_id,
        ]
        .into_iter()
        .flatten()
        .collect();
        if out.is_empty() { None } else { Some(out) }
    }

    fn clips_children(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table_view::column::{CellContext, ColumnWidth};
    use bastyde_canvas::SizeProposal;
    use bastyde_core::accesskit::Role;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_data::{SortFilterTreeModel, TreeFilterMode, TreeModel};
    use bastyde_i18n::lit;

    fn sample_tree() -> TreeModel<&'static str> {
        let t = TreeModel::new();
        let docs = t.insert_root(0, "docs");
        t.insert_child(docs, 0, "readme");
        t.insert_child(docs, 1, "guide");
        let src = t.insert_root(1, "src");
        t.insert_child(src, 0, "main.rs");
        t
    }

    fn name_col() -> Column<&'static str> {
        Column::<&str>::new("name", lit!("Name"), |row, _: &CellContext| {
            Box::new(crate::primitives::TextWidget::new(lit!(*row)))
        })
        .width(ColumnWidth::Flex(1.0))
    }

    fn size_col() -> Column<&'static str> {
        Column::<&str>::new("size", lit!("Size"), |_row, _: &CellContext| {
            Box::new(crate::primitives::TextWidget::new(lit!("0")))
        })
        .width(ColumnWidth::Fixed(60.0))
    }

    #[test]
    fn row_selection_click_repaints_immediately_without_expand_collapse() {
        // Regression for "row selection in TreeTableView only fires on
        // expand/collapse": before the selection_signal was observed,
        // calling `sel.select(row)` mutated the model but the rendered
        // `BodyRow.selected` flag (computed at build time from
        // `sel.is_selected(...)`) was stale until something else
        // bumped the version signal — typically a twist toggle.
        use bastyde_canvas::Point;
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        use bastyde_data::{SelectionMode, SelectionModel};
        let proxy = SortFilterTreeModel::new(sample_tree());
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(selection.clone())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        // Selection starts empty.
        assert_eq!(selection.selected_indices().len(), 0);
        // Click on the first body row — visible at flat_idx 0
        // ("docs"), which sits below the header at y ≈ header + 0.
        let header_h = cp::HEADER_HEIGHT;
        let click_y = header_h + 10.0;
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(40.0, click_y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(40.0, click_y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        // Selection updated.
        assert_eq!(selection.selected_indices(), vec![0]);
        // And — the regression — the rendered tree must reflect the
        // new selection without us manually expanding/collapsing.
        // We trigger a layout (which renders the selection bg paint
        // path) and verify the selection IS still there: i.e., a
        // version-signal observer on `selection_signal` would have
        // fired and queued a rebuild.
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert_eq!(selection.selected_indices(), vec![0]);
    }

    #[test]
    fn first_arrow_lands_on_an_end_row_instead_of_skipping_it() {
        // `TreeTableView` plugs its own hierarchical `RowNavigator` into
        // `TableView`'s key handler, so it inherited the same bug: "no cursor
        // yet" was read as "cursor on (0, 0)", which made the first ArrowDown
        // step to flat row 1 (skipping row 0) and the first ArrowUp a DEAD KEY
        // (`prev_row(0)` is `None`). Entry now uses the navigator's own
        // first/last visible row, so it is hierarchy-aware.
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        for (key, want, what) in [
            (
                Key::ArrowDown,
                0usize,
                "first ArrowDown enters at the first visible row",
            ),
            (
                Key::ArrowUp,
                3usize,
                "first ArrowUp enters at the last visible row",
            ),
        ] {
            let t = TreeModel::new();
            t.insert_root(0, "a");
            t.insert_root(1, "b");
            t.insert_root(2, "c");
            t.insert_root(3, "d");
            let proxy = SortFilterTreeModel::new(t);
            let selection = SelectionModel::new(SelectionMode::Single);
            let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
            let id = tree.add(
                TreeTableView::from_projection(proxy.clone())
                    .add_column(name_col())
                    .selection_mode(TableSelectionMode::SingleRow)
                    .selection(selection.clone())
                    .row_height(20.0),
            );
            tree.layout(SizeProposal {
                width: Some(400.0),
                height: Some(200.0),
            });
            tree.focus(id);
            assert_eq!(proxy.visible_count(), 4, "four flat roots");
            assert!(
                selection.selected_indices().is_empty(),
                "precondition: no cursor, nothing selected"
            );

            tree.press_key(key, Modifiers::NONE);
            assert_eq!(selection.selected_indices(), vec![want], "{what}");
        }
    }

    #[test]
    fn expanded_children_are_reachable_by_the_first_arrow() {
        // Hierarchy-aware entry: with "docs" expanded, the last VISIBLE row is a
        // child, not a root — so the first ArrowUp must land on that child. A
        // raw `row_count - 1` would happen to agree here, but going through the
        // navigator is what keeps it correct for any projection (filtered,
        // sorted, partially collapsed).
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_data::{SelectionMode, SelectionModel};

        let proxy = SortFilterTreeModel::new(sample_tree()); // docs{readme,guide}, src{main.rs}
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(selection.clone())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);

        let last = proxy.visible_count() - 1;
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![last],
            "first ArrowUp enters at the last VISIBLE row, whatever the hierarchy shows"
        );
    }

    #[test]
    fn row_click_moves_focus_so_arrow_nav_resumes_there() {
        // Regression: in row-selection mode a row click set the selection but
        // NOT `focused_cell` (the arrow-nav origin, `unwrap_or((0,0))`), so the
        // next Arrow stepped from row 0 rather than the clicked row. Click flat
        // row 1 with ≥3 visible rows so the fall-back-to-0 bug is observable
        // (buggy: 0 → 1; fixed: 1 → 2).
        use bastyde_canvas::Point;
        use bastyde_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
        use bastyde_data::{SelectionMode, SelectionModel};
        let t = TreeModel::new();
        t.insert_root(0, "a");
        t.insert_root(1, "b");
        t.insert_root(2, "c");
        t.insert_root(3, "d");
        let proxy = SortFilterTreeModel::new(t);
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(selection.clone())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        assert_eq!(proxy.visible_count(), 4, "four flat roots");

        // Click flat row 1 ("b"): 20px rows starting below the header.
        let click_y = cp::HEADER_HEIGHT + 1.0 * 20.0 + 10.0;
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(40.0, click_y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(40.0, click_y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            selection.selected_indices(),
            vec![1],
            "click selects flat row 1"
        );

        // ArrowDown must resume from the clicked row (1 → 2), not from row 0.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "ArrowDown after a click resumes from the clicked row (1 → 2)"
        );
    }

    #[test]
    fn role_is_treegrid() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), Role::TreeGrid);
    }

    #[test]
    fn initial_state_shows_only_roots() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert_eq!(proxy.visible_count(), 2); // docs, src
    }

    #[test]
    fn expand_via_widget_reveals_children() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.expand(docs);
        }
        assert_eq!(proxy.visible_count(), 4); // docs, readme, guide, src
    }

    #[test]
    fn arrow_right_expands_and_left_collapses_on_tree_column() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 0);
        }
        // ArrowRight on first row (docs, has children, collapsed) →
        // expand.
        tree.press_key(
            bastyde_core::event::Key::ArrowRight,
            bastyde_core::event::Modifiers::NONE,
        );
        assert_eq!(proxy.visible_count(), 4);
        // ArrowLeft on first row (now expanded) → collapse.
        tree.press_key(
            bastyde_core::event::Key::ArrowLeft,
            bastyde_core::event::Modifiers::NONE,
        );
        assert_eq!(proxy.visible_count(), 2);
    }

    /// Rows for the external-source tests: an indent-ordered stream keyed by a
    /// domain id, the shape `TreeDataSlice` derives a hierarchy from.
    fn slice_rows() -> Vec<bastyde_data::TreeRow<u64, &'static str>> {
        use bastyde_data::TreeRow;
        vec![
            TreeRow {
                key: 1,
                item: "docs",
                depth: 0,
            },
            TreeRow {
                key: 2,
                item: "readme",
                depth: 1,
            },
            TreeRow {
                key: 3,
                item: "guide",
                depth: 1,
            },
            TreeRow {
                key: 4,
                item: "src",
                depth: 0,
            },
        ]
    }

    fn external_slice() -> bastyde_data::TreeDataSlice<u64, &'static str> {
        let slice = bastyde_data::TreeDataSlice::<u64, &'static str>::new();
        slice.set_source(slice_rows);
        slice.reload();
        slice
    }

    #[test]
    fn from_source_renders_an_external_tree_without_a_tree_model() {
        // The point of `from_source`: no `TreeModel` mirror anywhere. The slice
        // owns identity (`u64`), derives the hierarchy from row depths, and the
        // table reads it through the erased `TreeDataSource`.
        let slice = external_slice();
        slice.expand(&1);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(slice.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert_eq!(slice.visible_count(), 4, "docs + 2 children + src");

        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert!(
            tt.projection().is_none(),
            "a source-backed view has no TreeModel projection to expose"
        );
        assert!(tt.body_pane_id.is_some(), "rows rendered from the source");
    }

    #[test]
    fn from_source_keyed_selection_survives_a_full_resource() {
        // The property a `TreeModel` mirror cannot offer: `NodeId`s are
        // reassigned on rebuild, but a domain key is not — so a keyed selection
        // still points at the same row after the source is re-materialised.
        let slice = external_slice();
        slice.expand(&1);
        let keyed = KeyedSelectionModel::<u64>::new(bastyde_data::SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _id = tree.add(
            TreeTableView::from_source_keyed(slice.clone(), keyed.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        keyed.select(3); // "guide"
        assert!(keyed.is_selected(&3));

        // Re-source from scratch — every row is rebuilt.
        slice.reload();
        assert!(
            keyed.is_selected(&3),
            "a domain-keyed selection must survive a re-source"
        );
    }

    #[test]
    fn from_source_supports_drag_reorder_like_the_tree_view() {
        // Parity check: a source-backed table reorders through the source's own
        // `accept_drop`, the same path `TreeView` uses — no `TreeModel`, no
        // `NodeId` anywhere. Here the slice commits the move into its own store.
        use bastyde_canvas::Point;
        use std::cell::RefCell;
        use std::rc::Rc;

        // The store the slice re-sources from; the reorder mutates it.
        let order: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(vec![1, 4]));
        let slice = bastyde_data::TreeDataSlice::<u64, &'static str>::new();
        {
            let order = order.clone();
            slice.set_source(move || {
                let names: std::collections::HashMap<u64, &'static str> =
                    [(1, "docs"), (4, "src")].into_iter().collect();
                order
                    .borrow()
                    .iter()
                    .map(|k| bastyde_data::TreeRow {
                        key: *k,
                        item: names[k],
                        depth: 0,
                    })
                    .collect()
            });
        }
        {
            let order = order.clone();
            // Domain policy: apply the move to the backing store.
            slice.set_reorder(move |dragged, target, _pos| {
                let mut o = order.borrow_mut();
                let Some(from) = o.iter().position(|k| *k == dragged) else {
                    return false;
                };
                let item = o.remove(from);
                let to = o
                    .iter()
                    .position(|k| *k == target)
                    .map_or(o.len(), |i| i + 1);
                o.insert(to, item);
                true
            });
        }
        // An external source must opt into dragging: `TreeDataSlice::drag`
        // defaults to `NoDrag` (pinned by its own `drag_default_is_nodrag`).
        slice.set_drag_policy(|_| bastyde_data::DragEligibility::CanDrag);
        slice.reload();
        assert_eq!(*order.borrow(), vec![1, 4], "docs, src");

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_source(slice.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        // Drag docs (flat 0) onto the bottom third of src (flat 1) → After src.
        let h = cp::HEADER_HEIGHT;
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 38.0),
        );
        assert_eq!(
            *order.borrow(),
            vec![4, 1],
            "the source applied the reorder: src now precedes docs"
        );
    }

    #[test]
    fn a_source_that_forbids_dragging_a_row_is_honored() {
        // The source owns drag eligibility. A view that ignored it would happily
        // move a row the store considers locked.
        use bastyde_canvas::Point;
        use std::cell::RefCell;
        use std::rc::Rc;

        let order: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(vec![1, 4]));
        let slice = bastyde_data::TreeDataSlice::<u64, &'static str>::new();
        {
            let order = order.clone();
            slice.set_source(move || {
                let names: std::collections::HashMap<u64, &'static str> =
                    [(1, "docs"), (4, "src")].into_iter().collect();
                order
                    .borrow()
                    .iter()
                    .map(|k| bastyde_data::TreeRow {
                        key: *k,
                        item: names[k],
                        depth: 0,
                    })
                    .collect()
            });
        }
        {
            let order = order.clone();
            slice.set_reorder(move |dragged, target, _pos| {
                let mut o = order.borrow_mut();
                let Some(from) = o.iter().position(|k| *k == dragged) else {
                    return false;
                };
                let item = o.remove(from);
                let to = o
                    .iter()
                    .position(|k| *k == target)
                    .map_or(o.len(), |i| i + 1);
                o.insert(to, item);
                true
            });
        }
        // Row 1 ("docs") is pinned in place by the store.
        slice.set_drag_policy(|k| {
            if *k == 1 {
                bastyde_data::DragEligibility::NoDrag
            } else {
                bastyde_data::DragEligibility::CanDrag
            }
        });
        slice.reload();

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_source(slice.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        let h = cp::HEADER_HEIGHT;
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 38.0),
        );
        assert_eq!(
            *order.borrow(),
            vec![1, 4],
            "a NoDrag row must not move, even onto a valid target"
        );
    }

    #[test]
    fn drop_on_the_middle_third_reparents_into_the_target() {
        // The Into zone: dropping on a row's middle third makes the dragged node
        // that row's child, rather than a sibling before/after it.
        use bastyde_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.collapse_all(); // roots only: docs@0, src@1
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        let h = cp::HEADER_HEIGHT;
        // Drag docs (flat 0) onto the MIDDLE third of src (flat 1, [h+20, h+40])
        // → Into src.
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 30.0),
        );
        assert_eq!(proxy.tree().root_count(), 1, "docs is no longer a root");
        assert_eq!(
            proxy.tree().parent(docs),
            Some(src),
            "docs became a child of src"
        );
    }

    #[test]
    fn an_active_sort_suppresses_drag_reorder() {
        // With the visible order driven by a sort, a manual reorder would have no
        // visible effect — so it must be refused outright rather than silently
        // mutating the tree behind the sort.
        use bastyde_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree())
            .with_comparator("name", |a: &&'static str, b: &&'static str| a.cmp(b));
        proxy.collapse_all();
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_sort(Some("name"), SortDirection::Ascending);
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        let h = cp::HEADER_HEIGHT;
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 38.0),
        );
        assert_eq!(
            proxy.tree().root(0),
            docs,
            "structure unchanged while sorted"
        );
        assert_eq!(
            proxy.tree().root(1),
            src,
            "structure unchanged while sorted"
        );
    }

    #[test]
    fn an_open_cell_editor_follows_its_row_and_closes_if_the_row_vanishes() {
        // `editing_cell` is a (row, col) pair that outlives rebuilds. Without
        // reconciliation, filtering a row away above an open editor slides the
        // editor onto a different row and silently edits the wrong item.
        let slice = bastyde_data::TreeDataSlice::<u64, &'static str>::new();
        let all: Vec<u64> = vec![1, 2, 3];
        slice.set_source(move || {
            let names: std::collections::HashMap<u64, &'static str> =
                [(1, "one"), (2, "two"), (3, "three")].into_iter().collect();
            all.iter()
                .map(|k| bastyde_data::TreeRow {
                    key: *k,
                    item: names[k],
                    depth: 0,
                })
                .collect()
        });
        slice.reload();

        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(slice.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        let proposal = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(proposal);

        // Edit row 2 ("three" sits at index 2).
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.begin_edit(2, "name");
            assert_eq!(tt.editing_cell_signal().get(), Some((2, 0)));
        }
        tree.layout(proposal); // captures the anchor

        // Drop the FIRST row: "three" is now at index 1.
        let fewer: Vec<u64> = vec![2, 3];
        slice.set_source(move || {
            let names: std::collections::HashMap<u64, &'static str> =
                [(2, "two"), (3, "three")].into_iter().collect();
            fewer
                .iter()
                .map(|k| bastyde_data::TreeRow {
                    key: *k,
                    item: names[k],
                    depth: 0,
                })
                .collect()
        });
        slice.reload();
        tree.layout(proposal);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(
                tt.editing_cell_signal().get(),
                Some((1, 0)),
                "the editor must follow its row to index 1, not stay on index 2"
            );
        }

        // Now delete the edited row itself: the editor must close, not move.
        let last: Vec<u64> = vec![2];
        slice.set_source(move || {
            last.iter()
                .map(|k| bastyde_data::TreeRow {
                    key: *k,
                    item: "two",
                    depth: 0,
                })
                .collect()
        });
        slice.reload();
        tree.layout(proposal);
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.editing_cell_signal().get(),
            None,
            "the editor must close when its row is gone"
        );
    }

    #[test]
    fn default_selection_mode_is_multi_row() {
        // The doc claimed `RowSingle` — a variant that does not exist. Pin the
        // real default behaviorally so prose can't drift from it again:
        // Shift+ArrowDown twice extends to 3 rows, which only MultiRow allows.
        use bastyde_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(wide_tree(10));
        let selection = bastyde_data::SelectionModel::new(bastyde_data::SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .selection(selection.clone())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 0);
        }
        selection.select(0);
        tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
        tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
        assert_eq!(
            selection.selection_signal().get().len(),
            3,
            "default mode must extend a multi-row selection"
        );
    }

    #[test]
    fn empty_view_renders_when_the_tree_has_no_rows() {
        let proxy = SortFilterTreeModel::new(TreeModel::<&'static str>::new());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .empty_view(|| Box::new(crate::primitives::TextWidget::new(lit!("Nothing here"))))
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert!(tt.empty_id.is_some(), "placeholder should be built");
        assert!(tt.body_pane_id.is_none(), "no body pane for zero rows");
    }

    #[test]
    fn empty_view_appears_when_live_rows_drop_to_zero() {
        // The transition case: rows exist, the widget is live, then a filter
        // removes them all. The body pane must be torn down and the
        // placeholder built — constructing already-empty (the two tests below)
        // never exercises that path.
        let proxy = SortFilterTreeModel::new(sample_tree()).with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |r: &&'static str| r.contains(&needle))
        });
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .empty_view(|| Box::new(crate::primitives::TextWidget::new(lit!("No matches"))))
                .row_height(20.0),
        );
        let proposal = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(proposal);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert!(tt.body_pane_id.is_some(), "starts with a body pane");
            assert!(tt.empty_id.is_none(), "no placeholder while rows exist");
        }

        proxy.set_filter("name", "zzz-no-such-row");
        tree.layout(proposal);
        assert_eq!(proxy.visible_count(), 0);
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert!(
            tt.empty_id.is_some(),
            "placeholder must appear once rows drop to zero"
        );
        assert!(tt.body_pane_id.is_none(), "stale body pane must be gone");
    }

    #[test]
    fn empty_view_renders_when_a_filter_matches_nothing() {
        // The other half of the empty state: rows exist, but none survive the
        // filter. Without this the user sees a blank pane and no explanation.
        let proxy = SortFilterTreeModel::new(sample_tree()).with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |r: &&'static str| r.contains(&needle))
        });
        proxy.set_filter("name", "zzz-no-such-row");
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .empty_view(|| Box::new(crate::primitives::TextWidget::new(lit!("No matches"))))
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert_eq!(proxy.visible_count(), 0);
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert!(tt.empty_id.is_some());
    }

    #[test]
    fn scroll_to_row_and_ensure_row_visible_move_the_offset() {
        let proxy = SortFilterTreeModel::new(wide_tree(100));
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();

        // Aligns the row to the top: row 50 × 20 px.
        tt.scroll_to_row(50);
        assert!((tt.scroll_y_signal().get() - 1000.0).abs() < 1.0);

        // Already-visible row: minimum scroll means no movement.
        let before = tt.scroll_y_signal().get();
        tt.ensure_row_visible(51);
        assert!((tt.scroll_y_signal().get() - before).abs() < f32::EPSILON);

        // Off-screen upward: scrolls back just far enough.
        tt.ensure_row_visible(10);
        assert!((tt.scroll_y_signal().get() - 200.0).abs() < 1.0);
    }

    #[test]
    fn begin_edit_resolves_a_column_id_and_end_edit_clears() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();

        tt.begin_edit(1, "size");
        assert_eq!(tt.editing_cell_signal().get(), Some((1, 1)));
        tt.end_edit();
        assert_eq!(tt.editing_cell_signal().get(), None);

        // Unknown id is a silent no-op, not a panic or a bogus position.
        tt.begin_edit(0, "no-such-column");
        assert_eq!(tt.editing_cell_signal().get(), None);

        // An out-of-range row is refused too: without the bounds check this
        // stranded `editing_cell` on a row nothing could ever match, and only
        // an explicit `end_edit` would clear it.
        tt.begin_edit(9999, "name");
        assert_eq!(tt.editing_cell_signal().get(), None);

        // ...and a refused call must not clobber a live editor.
        tt.begin_edit(1, "size");
        tt.begin_edit(9999, "size");
        assert_eq!(tt.editing_cell_signal().get(), Some((1, 1)));
    }

    #[test]
    fn begin_edit_resolves_before_the_view_is_mounted() {
        // Seeding a freshly constructed view with an edit target it already
        // holds is only possible on the builder — a rebuild makes a brand-new
        // view whose `editing_cell` starts `None`, and there is no post-mount
        // handle (`as_any_mut` is not overridden). `display_indices` is filled
        // by `build()`, so before the fix this resolved against an empty cache
        // and silently did nothing: the caller's edit request vanished.
        //
        // `size` is pinned Leading, so display order is [size, name] and the
        // correct answer for "name" is 1, not its declaration index 0 — which
        // is what makes this a test of `display_order()` and not of a shortcut
        // that happens to agree when nothing is pinned.
        let proxy = SortFilterTreeModel::new(sample_tree());
        let view = TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col().pinned(PinnedSide::Leading))
            .row_height(20.0);

        view.begin_edit(1, "name");
        assert_eq!(view.editing_cell_signal().get(), Some((1, 1)));

        // The documented no-ops still hold with no cache to consult.
        view.end_edit();
        view.begin_edit(0, "no-such-column");
        assert_eq!(view.editing_cell_signal().get(), None);
        view.begin_edit(9999, "name");
        assert_eq!(view.editing_cell_signal().get(), None);

        // And the seed survives mounting: the target it resolved is the one
        // the body pane reads back.
        view.begin_edit(1, "name");
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(view);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let tt = tree
            .widget_as_any(id)
            .unwrap()
            .downcast_ref::<TreeTableView<&'static str>>()
            .unwrap();
        assert_eq!(tt.editing_cell_signal().get(), Some((1, 1)));
    }

    #[test]
    fn column_imperatives_write_their_signals() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();

        tt.set_column_width("name", 123.0);
        assert_eq!(tt.column_widths_signal().get().get("name"), Some(&123.0));
        // A non-positive width removes the override rather than pinning 0 px.
        tt.set_column_width("name", 0.0);
        assert!(!tt.column_widths_signal().get().contains_key("name"));

        // Order and pinning must actually reach `display_order()`, not just sit
        // in a signal nothing reads. Columns are declared name(0), size(1).
        assert_eq!(
            tt.display_order(),
            vec![0, 1],
            "declaration order initially"
        );

        tt.set_column_order(vec!["size".into(), "name".into()]);
        assert_eq!(tt.column_order_signal().get(), vec!["size", "name"]);
        assert_eq!(
            tt.display_order(),
            vec![1, 0],
            "set_column_order must reorder the display, not only the signal"
        );

        // Pinning outranks the order list: a Leading-pinned column sorts into
        // the leading band regardless of where the order puts it.
        tt.set_column_pinning("name", PinnedSide::Leading);
        assert_eq!(
            tt.column_pinning_signal().get().get("name"),
            Some(&PinnedSide::Leading)
        );
        assert_eq!(
            tt.display_order(),
            vec![0, 1],
            "set_column_pinning must pull the pinned column back to the front"
        );
        tt.set_column_pinning("name", PinnedSide::None);
        assert!(!tt.column_pinning_signal().get().contains_key("name"));
        assert_eq!(
            tt.display_order(),
            vec![1, 0],
            "clearing the pin restores the order list's arrangement"
        );

        tt.set_sort(Some("name"), SortDirection::Ascending);
        assert!(tt.sort_signal().get().is_some());
        tt.clear_sort();
        assert_eq!(tt.sort_signal().get(), None);
    }

    // ── Cell state survives a column reorder/pin ───────────────────────
    //
    // `focused_cell`, `editing_cell`, and `CellSelectionModel` all store
    // `(row, display_position)`. A drag-to-reorder or a pin toggle only
    // bumps the rebuild version — without a remap, the stored display
    // position would silently relabel onto whatever column now sits
    // there. Pinning makes display order diverge from declaration order
    // (columns are declared name(0), size(1)), so a shortcut that merely
    // keeps the same index would fail these.

    #[test]
    fn column_pinning_remaps_focused_cell_to_follow_its_column() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 1); // focus `size`, at display position 1
            // Pinning `size` Leading swaps it ahead of `name` — display
            // order becomes [size, name]. A stale (0, 1) would now land
            // on `name`.
            tt.set_column_pinning("size", PinnedSide::Leading);
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.focused_cell_signal().get(),
            Some((0, 0)),
            "focus must follow `size` to its new display position"
        );
    }

    #[test]
    fn column_pinning_remaps_editing_cell_to_follow_its_column() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.begin_edit(0, "size"); // size @ display position 1
            assert_eq!(tt.editing_cell_signal().get(), Some((0, 1)));
            tt.set_column_pinning("size", PinnedSide::Leading);
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.editing_cell_signal().get(),
            Some((0, 0)),
            "the open editor must follow `size` to its new display \
             position, not relabel onto whatever column now sits at \
             position 1"
        );
    }

    #[test]
    fn column_pinning_remaps_cell_selection_to_follow_its_column() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let cs = CellSelectionModel::new(TableSelectionMode::MultiCell);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0)
                .selection_mode(TableSelectionMode::MultiCell)
                .cell_selection(cs.clone()),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        cs.select(0, 1); // select `size` at display position 1
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_column_pinning("size", PinnedSide::Leading);
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert!(
            cs.is_selected(0, 0),
            "selection must follow `size` to its new display position"
        );
        assert!(!cs.is_selected(0, 1));
    }

    #[test]
    fn collapsing_a_node_above_a_selected_cell_clears_stale_cell_selection() {
        // Cell selection is index-based; a `TreeDataSource`'s flattening
        // gives no per-row delta to reindex it by (unlike `TableView`'s
        // `ListModel` `DataChange`), so the honest fix on a structural
        // change is to drop the selection rather than let a stale flat
        // row index silently point at whatever node now occupies it.
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        let cs = CellSelectionModel::new(TableSelectionMode::MultiCell);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0)
                .selection_mode(TableSelectionMode::MultiCell)
                .cell_selection(cs.clone()),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.expand(docs);
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert_eq!(proxy.visible_count(), 4); // docs, readme, guide, src
        cs.select(3, 0); // `src`, the last flat row
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.collapse(docs);
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert_eq!(proxy.visible_count(), 2); // docs, src — `src` is now row 1
        assert_eq!(
            cs.count(),
            0,
            "a stale (row, col) surviving the collapse must be dropped, not \
             silently point at whatever node now sits at flat row 3"
        );
    }

    #[test]
    fn content_only_update_leaves_cell_selection_untouched() {
        // A version bump that doesn't change the flat row count — an
        // in-place item edit, no expand/collapse/insert/remove — must not
        // disturb an existing cell selection.
        let model = sample_tree();
        let proxy = SortFilterTreeModel::new(model);
        let docs = proxy.tree().root(0);
        let cs = CellSelectionModel::new(TableSelectionMode::MultiCell);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0)
                .selection_mode(TableSelectionMode::MultiCell)
                .cell_selection(cs.clone()),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        cs.select(0, 0); // `docs`
        // In-place content update — same node, same position, new label.
        proxy.tree().update(docs, "docs-renamed");
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        assert!(
            cs.is_selected(0, 0),
            "a content-only update must leave an unrelated selection alone"
        );
    }

    // ── AT active_descendant follows cell focus ─────────────────────────

    #[test]
    fn focused_cell_sets_active_descendant_to_the_cell_node() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 1);
        }
        let update = tree.sync_accessibility();
        let root_node_id = widget_id_to_node_id(id);
        let root_node = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == root_node_id)
            .map(|(_, n)| n)
            .expect("root node present in the AT tree");
        let active = root_node
            .active_descendant()
            .expect("a focused cell must set active_descendant");
        let cell_node = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == active)
            .map(|(_, n)| n)
            .expect("active_descendant must reference a node present in the TreeUpdate");
        assert_eq!(cell_node.role(), Role::Cell);
    }

    #[test]
    fn active_descendant_clears_after_the_focused_cell_scrolls_out_of_realization() {
        let proxy = SortFilterTreeModel::new(wide_tree(1000));
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(1, 0);
        }
        let root_node_id = widget_id_to_node_id(id);
        let update = tree.sync_accessibility();
        let active_before = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == root_node_id)
            .and_then(|(_, n)| n.active_descendant());
        assert!(active_before.is_some(), "row 1 is realized initially");

        // Scroll far enough that row 1 leaves the realized+buffer window.
        // Nothing clears `focused_cell` on scroll, so this exercises the
        // "stale id" hazard directly: the pre-scroll build's cell WidgetId
        // has no live AT node once the pane rebuilds without it.
        let signal = {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.scroll_y_signal().clone()
        };
        signal.set(2000.0);
        tree.request_frame();
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let update = tree.sync_accessibility();
        let active_after = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == root_node_id)
            .and_then(|(_, n)| n.active_descendant());
        assert_eq!(
            active_after, None,
            "a focused cell that scrolled out of realization must not leave \
             a stale active_descendant pointing at a destroyed node"
        );
    }

    #[test]
    fn lazy_loading_rows_render_placeholder_cells_and_request_the_window() {
        // A windowed tree source with nothing resident: every visible row
        // is `Loading`, so the pane must render placeholder cells (not
        // skip the rows — `meta()` returning `None` used to mean "off the
        // end of `start..end`" unconditionally) and the view must nudge
        // the source to load the realized window. Mirrors TableView's
        // `lazy_loading_rows_render_placeholder_cells_and_request_the_window`.
        use bastyde_data::{FlatEntry, RowState};
        use std::cell::RefCell;
        use std::ops::Range;

        struct Windowed {
            total: usize,
            requested: Rc<RefCell<Vec<Range<usize>>>>,
            version: Signal<u64>,
        }
        impl TreeDataSource for Windowed {
            type Item = &'static str;
            type Key = usize;
            fn visible_count(&self) -> usize {
                self.total
            }
            fn with_entry<R>(
                &self,
                _i: usize,
                _f: impl FnOnce(&&'static str, &FlatEntry<usize>) -> R,
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
                vec![]
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(Windowed {
                total: 1000,
                requested: requested.clone(),
                version: Signal::new(0),
            })
            .add_column(name_col())
            .show_header(false)
            .row_height(30.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        // The body pane is the view's first child (header suppressed).
        // 300px / 30px = 10 visible + buffer → the loading rows realize
        // as placeholder row widgets, NOT skipped.
        let body_pane = tree.children(id)[0];
        let placeholder_rows = tree.children(body_pane).len();
        assert!(
            placeholder_rows >= 10,
            "loading rows must render as placeholders, got {placeholder_rows}"
        );
        // And the source was asked to load the realized window.
        assert!(
            !requested.borrow().is_empty(),
            "request_window must be called for the visible range"
        );
    }

    #[test]
    fn arrow_expand_collapse_follows_a_non_leading_tree_column() {
        // Regression: the key handler hardcoded `col == 0` as "the tree
        // column", so designating any other column via `.tree_column()` moved
        // the twist visually but left ArrowLeft/ArrowRight expanding nothing.
        // Here the tree column is "size", at display position 1.
        use bastyde_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .add_column(size_col())
                .tree_column("size")
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);

        // Off the tree column: the arrows are pure cursor movement, so the
        // visible set must not change.
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 0);
        }
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            proxy.visible_count(),
            2,
            "ArrowRight off the tree column must not expand"
        );

        // On the tree column (display position 1): expand, then collapse.
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 1);
        }
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            proxy.visible_count(),
            4,
            "docs expands to reveal 2 children"
        );
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(proxy.visible_count(), 2, "docs collapses again");
    }

    #[test]
    fn arrow_nav_scroll_follows_focused_row() {
        // 100 flat rows × 20 px in a 200 px viewport. Walking focus down
        // past the visible window must scroll to keep the focused row on
        // screen ("selection always visible"), matching TreeView / the
        // newly-fixed TableView. Regression for: TreeTableView keyboard
        // nav left scroll_y untouched.
        use bastyde_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(wide_tree(100));
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        let proposal = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(proposal);
        tree.focus(id);
        let read_scroll = |tree: &WidgetTree| {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .scroll_y_signal()
                .get()
        };
        let read_focus = |tree: &WidgetTree| {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .focused_cell_signal()
                .get()
        };
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(0, 0);
        }
        assert_eq!(read_scroll(&tree), 0.0, "starts at top");

        for _ in 0..20 {
            tree.press_key(Key::ArrowDown, Modifiers::NONE);
            tree.layout(proposal);
        }
        assert_eq!(read_focus(&tree), Some((20, 0)));
        assert!(
            read_scroll(&tree) > 200.0,
            "arrow-down nav must scroll to reveal row 20, got {}",
            read_scroll(&tree)
        );

        // Ctrl+Home returns focus AND scroll to the top.
        tree.press_key(Key::Home, Modifiers::CTRL);
        tree.layout(proposal);
        assert_eq!(read_focus(&tree), Some((0, 0)));
        assert_eq!(read_scroll(&tree), 0.0, "Ctrl+Home scrolls to top");
    }

    #[test]
    fn type_ahead_jumps_to_matching_row() {
        use bastyde_core::event::{Key, Modifiers};
        let model = TreeModel::new();
        model.insert_root(0, "Apple");
        model.insert_root(1, "Banana");
        model.insert_root(2, "Cherry");
        model.insert_root(3, "Cranberry");
        let proxy = SortFilterTreeModel::new(model);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0)
                .type_ahead_label(|s: &&'static str| s.to_string()),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        let read_focus = |tree: &WidgetTree| {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .focused_cell_signal()
                .get()
        };
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(0, 0);
        }
        tree.press_key(Key::C, Modifiers::NONE);
        assert_eq!(read_focus(&tree), Some((2, 0)), "'c' → Cherry");
        tree.press_key(Key::R, Modifiers::NONE);
        assert_eq!(read_focus(&tree), Some((3, 0)), "'cr' → Cranberry");
    }

    #[test]
    fn ctrl_tab_escapes_the_cell_grid() {
        use crate::primitives::{TextWidget, VStack};
        use bastyde_core::event::{Key, Modifiers};
        use bastyde_core::widget_builder::WidgetBuilder;

        let proxy = SortFilterTreeModel::new(wide_tree(5));
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        let sink = tree.add(TextWidget::new(lit!("sink")).focusable(true));
        let _root = tree.add(VStack::new().add_child(id).add_child(sink));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let read_focus = |tree: &WidgetTree| {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .focused_cell_signal()
                .get()
        };
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(0, 0);
        }
        let before = read_focus(&tree);
        tree.press_key(Key::Tab, Modifiers::CTRL);
        assert_eq!(
            read_focus(&tree),
            before,
            "Ctrl+Tab must not navigate cells"
        );
        assert_eq!(
            tree.focused(),
            Some(sink),
            "Ctrl+Tab moves focus out of the tree-table"
        );
    }

    #[test]
    fn rows_carry_role_row_with_level_indicator() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        proxy.expand(docs);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        // Walk the tree and count Role::Row entries.
        let mut q = vec![id];
        let mut row_count = 0;
        while let Some(n) = q.pop() {
            if tree.accessibility_node(n).role() == Role::Row {
                row_count += 1;
            }
            for c in tree.children(n) {
                q.push(c);
            }
        }
        // 1 header + 4 visible body rows (docs, readme, guide, src).
        assert!(
            row_count >= 5,
            "expected at least 5 Role::Row nodes, got {row_count}"
        );
    }

    #[test]
    fn filter_mode_keep_ancestors_works_via_proxy() {
        let proxy = SortFilterTreeModel::new(sample_tree())
            .filter_mode(TreeFilterMode::KeepAncestors)
            .with_predicate("name", |t| {
                let needle = t.to_string();
                Box::new(move |row: &&str| row.contains(&needle))
            });
        proxy.expand_all();
        proxy.set_filter("name", "main");
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let _id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        // Visible: src (ancestor), main.rs (matches).
        assert_eq!(proxy.visible_count(), 2);
    }

    #[test]
    fn collapse_all_then_expand_all_round_trips() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.expand_all();
        }
        assert_eq!(proxy.visible_count(), 5);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.collapse_all();
        }
        assert_eq!(proxy.visible_count(), 2);
    }

    #[test]
    fn row_count_in_a11y_includes_header() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), Role::TreeGrid);
        // We can't read row_count from AccessibilityInfo directly,
        // but we can verify Role::TreeGrid + Role::Row count matches
        // (header + 2 body rows = 3).
        let mut q = vec![id];
        let mut rows = 0;
        while let Some(n) = q.pop() {
            if tree.accessibility_node(n).role() == Role::Row {
                rows += 1;
            }
            for c in tree.children(n) {
                q.push(c);
            }
        }
        assert_eq!(rows, 3); // header + docs + src
    }

    // ── RTL (right-to-left) ──────────────────────────────────────────────

    /// A tree of `n` collapsed roots — enough to force a vertical scrollbar.
    fn wide_tree(n: u32) -> TreeModel<&'static str> {
        let t = TreeModel::new();
        for i in 0..n {
            t.insert_root(i as usize, "node");
        }
        t
    }

    /// All `Role::Row` node bounds (header + body), for picking a body row.
    fn row_bounds(tree: &WidgetTree, root: WidgetId) -> Vec<bastyde_canvas::Rect> {
        let mut q = vec![root];
        let mut out = Vec::new();
        while let Some(n) = q.pop() {
            if tree.accessibility_node(n).role() == Role::Row {
                out.push(tree.bounds(n));
            }
            for c in tree.children(n) {
                q.push(c);
            }
        }
        out
    }

    #[test]
    fn rtl_swaps_tree_expand_collapse_keys() {
        use bastyde_core::environment::LayoutDirection;
        use bastyde_core::event::{Key, Modifiers};

        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let table = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        // Roots start collapsed: docs + src visible.
        assert_eq!(proxy.visible_count(), 2);

        tree.set_layout_direction(LayoutDirection::RightToLeft);
        tree.focus(table);
        {
            let any = tree.widget_as_any(table).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(0, 0);
        }

        // Under RTL the collapsed chevron points left, so ArrowLeft expands
        // (toward the children) and ArrowRight collapses.
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(
            proxy.visible_count(),
            4,
            "RTL ArrowLeft on the tree column should expand docs"
        );
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            proxy.visible_count(),
            2,
            "RTL ArrowRight on the tree column should collapse docs"
        );
    }

    #[test]
    fn rtl_tree_band_shifts_for_left_scrollbar() {
        use bastyde_core::environment::LayoutDirection;
        // 50 roots → vertical scrollbar present. Under RTL it sits on the
        // physical left, so the body band (and its rows) shift right by
        // SCROLLBAR_THICKNESS.
        let proxy = SortFilterTreeModel::new(wide_tree(50));
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let table = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let table_bounds = tree.bounds(table);
        // Pick a body row (below the header, which sits at the top).
        let body_row = row_bounds(&tree, table)
            .into_iter()
            .filter(|r| r.y > table_bounds.y + 5.0)
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .expect("a body row");
        assert!(
            (body_row.x - SCROLLBAR_THICKNESS).abs() < 0.5,
            "RTL body row should start at SCROLLBAR_THICKNESS, got x={}",
            body_row.x
        );
        // LTR control: same table laid out left-to-right starts at 0.
        let mut tree2 = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let proxy2 = SortFilterTreeModel::new(wide_tree(50));
        let table2 = tree2.add(
            TreeTableView::from_projection(proxy2)
                .add_column(name_col())
                .row_height(20.0),
        );
        tree2.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let tb2 = tree2.bounds(table2);
        let body_row2 = row_bounds(&tree2, table2)
            .into_iter()
            .filter(|r| r.y > tb2.y + 5.0)
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .expect("a body row");
        assert!(body_row2.x.abs() < 0.5, "LTR body row x={}", body_row2.x);
    }

    // ── Boundary scroll chaining ─────────────────────────────────────────

    /// A TreeTableView (40 root rows × 20 px in a ~120 px viewport) above a
    /// filler inside an outer ScrollArea, so chaining from the inner
    /// tree-table to the outer area is observable.
    fn nested_tree_table_fixture(
        inner: OverscrollBehavior,
    ) -> (WidgetTree, Signal<f32>, Signal<f32>) {
        use crate::ScrollArea;
        use crate::primitives::{FixedSize, TextWidget, VStack};
        let model = TreeModel::new();
        for i in 0..40 {
            model.insert_root(i, "row");
        }
        let proxy = SortFilterTreeModel::new(model);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let tt = TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .show_header(false)
            .row_height(20.0)
            .overscroll_behavior(inner);
        let inner_y = tt.scroll_y_signal().clone();
        let tt_id = tree.add(tt);
        let viewport = tree.add(FixedSize::new().width(220.0).height(120.0).child_id(tt_id));
        let filler = tree.add(
            FixedSize::new()
                .width(220.0)
                .height(300.0)
                .child(TextWidget::new(lit!(""))),
        );
        let outer_content = tree.add(VStack::new().add_child(viewport).add_child(filler));
        let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
        let outer_y = outer.scroll_y_signal().clone();
        let _outer = tree.add(outer);
        tree.layout(SizeProposal {
            width: Some(220.0),
            height: Some(150.0),
        });
        (tree, inner_y, outer_y)
    }

    #[test]
    fn nested_tree_table_chains_to_outer_at_boundary() {
        use bastyde_canvas::Point;
        use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, inner_y, outer_y) = nested_tree_table_fixture(OverscrollBehavior::Chain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: Some(220.0),
            height: Some(150.0),
        });
        let inner_bottom = inner_y.get();
        assert!(
            inner_bottom > 0.0,
            "inner tree-table should scroll down; got {inner_bottom}"
        );
        // A second wheel at the boundary must chain to the outer area.
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: Some(220.0),
            height: Some(150.0),
        });
        assert!(
            (inner_y.get() - inner_bottom).abs() < 0.01,
            "inner stays clamped at bottom"
        );
        assert!(
            outer_y.get() > 0.01,
            "outer must scroll because the inner chained the boundary"
        );
    }

    #[test]
    fn nested_tree_table_contain_blocks_chaining() {
        use bastyde_canvas::Point;
        use bastyde_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, _inner_y, outer_y) = nested_tree_table_fixture(OverscrollBehavior::Contain);
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: Some(220.0),
            height: Some(150.0),
        });
        tree.pointer_move(Point::new(50.0, 40.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Pixels { x: 0.0, y: 100.0 },
            modifiers: Modifiers::NONE,
        });
        tree.layout(SizeProposal {
            width: Some(220.0),
            height: Some(150.0),
        });
        assert!(
            outer_y.get() < 0.01,
            "Contain must prevent chaining: outer stays put"
        );
    }

    // ── TreeBodyPane split + variable row heights ───────────────────────

    fn count_role(tree: &WidgetTree, root: WidgetId, role: Role) -> usize {
        let mut walker = vec![root];
        let mut n = 0;
        while let Some(id) = walker.pop() {
            if tree.accessibility_node(id).role() == role {
                n += 1;
            }
            for c in tree.children(id) {
                walker.push(c);
            }
        }
        n
    }

    /// Collect the (y, height) bounds of the materialised `Role::Row`
    /// widgets, sorted by y.
    fn row_spans(tree: &WidgetTree, root: WidgetId) -> Vec<(f32, f32)> {
        let mut walker = vec![root];
        let mut spans = Vec::new();
        while let Some(id) = walker.pop() {
            if tree.accessibility_node(id).role() == Role::Row {
                let b = tree.bounds(id);
                spans.push((b.y, b.height));
            }
            for c in tree.children(id) {
                walker.push(c);
            }
        }
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        spans
    }

    #[test]
    fn rows_rebuild_during_scrollbar_thumb_drag() {
        // The reason the TreeBodyPane split exists: rebuilds targeting
        // an ancestor of the pointer-captured scrollbar are deferred
        // for the whole Down→Up sequence. Pre-split, the scroll-buffer
        // exit had to rebuild the TreeTableView root (an ancestor), so
        // dragging the thumb past the buffered window left the body
        // stale/empty until release. The pane is a sibling of the
        // scrollbar, so its buffer-exit rebuild goes through mid-drag.
        use bastyde_canvas::Point;
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        let model = TreeModel::new();
        for i in 0..100 {
            model.insert_root(i, "root");
        }
        let proxy = SortFilterTreeModel::new(model);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let table = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        // Press the scrollbar thumb (right edge, inside the body band)
        // and hold — the framework captures the pointer on it.
        let thumb = Point::new(395.0, cp::HEADER_HEIGHT + 10.0);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: thumb,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Scroll far past the buffered window while captured (the
        // thumb drag drives scroll_y exactly like this).
        let scroll = {
            let any = tree.widget_as_any(table).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.scroll_y_signal().clone()
        };
        scroll.set(1000.0);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        // At least one materialised row must land inside the viewport
        // at the new scroll position — i.e. the pane rebuilt mid-drag.
        let mut walker = vec![table];
        let mut any_in_viewport = false;
        while let Some(id) = walker.pop() {
            if tree.accessibility_node(id).role() == Role::Row {
                let b = tree.bounds(id);
                if b.y >= 0.0 && b.y < 200.0 {
                    any_in_viewport = true;
                }
            }
            for c in tree.children(id) {
                walker.push(c);
            }
        }
        assert!(
            any_in_viewport,
            "pane must rebuild past the buffer while the thumb is captured"
        );

        tree.dispatch_event(WidgetEvent::PointerUp {
            position: thumb,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    #[test]
    fn exact_row_height_fn_positions_tree_rows() {
        let heights = [60.0_f32, 20.0, 40.0];
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let table = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .show_header(false)
                .row_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0)),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        // Roots only: docs (60), src (20).
        let spans = row_spans(&tree, table);
        assert_eq!(spans.len(), 2);
        assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 60.0).abs() < 0.01);
        assert!((spans[1].0 - 60.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
    }

    #[test]
    fn auto_row_height_measures_tree_cells() {
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
        let col = Column::<&str>::new("name", lit!("Name"), |_row, _: &CellContext| {
            Box::new(FixedLeaf(50.0, 30.0))
        })
        .width(ColumnWidth::Flex(1.0));
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let table = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(col)
                .show_header(false)
                .auto_row_height(50.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        // Rows measured to 30 from the 50 estimate.
        let spans = row_spans(&tree, table);
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "row 1 should sit at measured 30, got {}",
            spans[1].0
        );

        // Expanding docs (flat 0) keeps measured heights — the
        // divergence is the toggled row, not a full reset, so the
        // expanded children appear right below the measured row 0.
        proxy.expand(docs);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        let spans = row_spans(&tree, table);
        assert_eq!(spans.len(), 4); // docs, readme, guide, src
        assert!(
            (spans[1].0 - 30.0).abs() < 0.01,
            "measured row 0 must survive the expand, got {}",
            spans[1].0
        );
    }

    // ── Row reorder (Stage 5) ──────────────────────────────────────────────

    /// Full drag gesture: down on source, move to cross the threshold, move to
    /// target, up.
    fn drag(tree: &mut WidgetTree, from: bastyde_canvas::Point, to: bastyde_canvas::Point) {
        use bastyde_canvas::Point;
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

    #[test]
    fn drag_reorders_roots_after() {
        use bastyde_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.collapse_all(); // roots only: docs@0, src@1
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        let h = cp::HEADER_HEIGHT;
        // Drag docs (flat 0, [h, h+20]) onto the bottom third of src (flat 1,
        // [h+20, h+40]) → After src.
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 38.0),
        );
        assert_eq!(proxy.tree().root_count(), 2);
        assert_eq!(proxy.tree().root(0), src, "src becomes the first root");
        assert_eq!(proxy.tree().root(1), docs, "docs moves after src");
    }

    #[test]
    fn drag_into_own_descendant_is_refused() {
        use bastyde_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.expand_all(); // docs@0, readme@1, guide@2, src@3, main.rs@4
        let docs = proxy.tree().root(0);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        let h = cp::HEADER_HEIGHT;
        // Drag docs (flat 0) into the middle third of readme (flat 1, a child
        // of docs) → cycle → refused; tree unchanged, no panic.
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 30.0),
        );
        assert_eq!(proxy.tree().parent(docs), None, "docs stays a root");
        assert_eq!(proxy.tree().root_count(), 2);
    }

    #[test]
    fn reorder_is_suppressed_while_sorted() {
        use bastyde_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.collapse_all();
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .reorderable(true)
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });
        // Activate a sort: the drop gate must refuse the reorder (a manual
        // reorder is meaningless once the visible order is sort-driven).
        tree.widget_as_any(id)
            .and_then(|a| a.downcast_ref::<TreeTableView<&str>>())
            .expect("TreeTableView")
            .set_sort(Some("name"), bastyde_data::SortDirection::Ascending);
        let h = cp::HEADER_HEIGHT;
        drag(
            &mut tree,
            Point::new(40.0, h + 10.0),
            Point::new(40.0, h + 38.0),
        );
        assert_eq!(proxy.tree().root(0), docs, "docs unchanged while sorted");
        assert_eq!(proxy.tree().root(1), src, "src unchanged while sorted");
    }

    #[test]
    fn keyed_selection_survives_collapse() {
        // Keyed (identity) selection: a node selected by NodeId stays selected
        // when its parent collapses (the row scrolls out of the projection).
        // The prune on every projection change must NOT drop a collapsed-but-
        // present node — existence is checked against the tree, not visibility.
        use bastyde_data::{KeyedSelectionModel, SelectionMode};
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.expand_all();
        let docs = proxy.tree().root(0);
        let readme = proxy.tree().children(docs)[0];
        let keyed = KeyedSelectionModel::<NodeId>::new(SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .selection_mode(TableSelectionMode::MultiRow)
                .keyed_selection(keyed.clone())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(300.0),
        });

        keyed.select(readme);
        assert!(keyed.is_selected(&readme));

        // Collapse docs → readme leaves the visible projection, bumping the
        // version (which runs the prune). It must survive (still in the tree).
        proxy.collapse(docs);
        assert!(
            keyed.is_selected(&readme),
            "a collapsed-but-present node stays selected by identity"
        );

        // Re-expand → still selected.
        proxy.expand(docs);
        assert!(keyed.is_selected(&readme));
    }
}
