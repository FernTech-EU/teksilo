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
//! from [`TreeCheckedModel`](teksilo_data::TreeCheckedModel) over the same tree
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
//! [`KeyedTreeCheckedModel`](teksilo_data::KeyedTreeCheckedModel) instead — it
//! survives a full re-source, which a `NodeId`-keyed set cannot.
//!
//! ## Accessibility
//!
//! Root emits `Role::TreeGrid`; rows carry `set_level` + `set_expanded`.
//! ArrowLeft / ArrowRight on the tree column collapse / expand.
//!
//! ```ignore
//! // Column delegates capture closures — use ignore.
//! use teksilo_widgets::TreeTableView;
//! use teksilo_data::TreeModel;
//! # struct File { name: String }
//! # let model: TreeModel<File> = TreeModel::new();
//! let _view = TreeTableView::new(model).row_height(28.0);
//! ```

mod body_pane;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use teksilo_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::drag_payload::DragPayload;
use teksilo_core::event::EventResponse;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{
    DropPosition, KeyedSelectionModel, NodeId, SelectionModel, SortDirection, SortFilterTreeModel,
    TreeFilterMode, TreeModel,
};
use teksilo_i18n::LocalizedString;
use teksilo_tokens::{BorderRole, Easing, SurfaceRole};

use crate::styles::recipe_table_style as cp;

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{DragTransferMode, RowDragData, RowSelection, ViewId, ViewKind};
use crate::data_views::{DropViz, drop_into_tint};
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};
use crate::table_view::ColumnReorderDragData;
use crate::table_view::body::SharedColumnWidths;
use crate::table_view::column::{
    Column, ColumnResizePolicy, EditTriggers, GridLines, PinnedSide, TabTraversal,
};
use crate::table_view::header::{
    ColumnResizeInfo, ColumnResizeTable, HeaderCell, HeaderCellSpec, HeaderRow, ResizeStateHandle,
    attach_header_reorder_handlers,
};
use crate::table_view::imperative;
use crate::table_view::keyboard;
use crate::table_view::layout;
use crate::table_view::row_navigator::RowNavigator;
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};
use crate::tree_source::TreeSource;
use teksilo_data::{DropResponse, TreeDataSource};

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
    edit_triggers: EditTriggers,
    #[allow(clippy::type_complexity)]
    on_cell_edit_request: Option<Rc<dyn Fn(usize, &str, &mut EventContext)>>,
    on_cell_edit_dismissed: Option<Rc<dyn Fn(usize, &str, &mut EventContext)>>,
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
    /// Horizontal scroll offset of the Middle (unpinned) pane — mirrors
    /// `TableView::scroll_x`. See `table_view::PaneBoundaries`.
    scroll_x: Signal<f32>,
    max_scroll_x: Signal<f32>,
    viewport_ratio_x: Signal<f32>,
    sort_signal: Signal<Option<(String, SortDirection)>>,
    column_widths_signal: Signal<HashMap<String, f32>>,
    column_order_signal: Signal<Vec<String>>,
    column_pinning_signal: Signal<HashMap<String, PinnedSide>>,
    filters_signal: Signal<HashMap<String, String>>,
    focused_cell: Signal<Option<(usize, usize)>>,
    /// The realized `(row index -> row wrapper id)` map, filled by the body
    /// pane each build. Lets this widget's `&self` methods resolve a row index
    /// to a widget without reaching into the pane. Mirrors `ListView::row_map`.
    row_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,
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
    /// Horizontal scroll bar along the bottom of the Middle pane only —
    /// mirrors `TableView::h_scrollbar_id`.
    h_scrollbar_id: Option<WidgetId>,
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
    /// Counts of (leading-pinned, middle, trailing-pinned) columns —
    /// mirrors `TableView::pane_boundaries`. Populated by `display_order()`.
    pane_boundaries: Rc<RefCell<crate::table_view::PaneBoundaries>>,
    /// `(row, display_pos) -> WidgetId` for every cell realized by the
    /// body pane's latest `build()`. Mirrors `TableView::cell_map` (the
    /// GridView `tile_map` pattern — shared between the root and its
    /// sibling-of-scrollbar pane); `accessibility()` reads it to point
    /// `active_descendant` at the keyboard-focused cell's own AT node.
    cell_map: Rc<RefCell<Vec<((usize, usize), WidgetId)>>>,
    viewport_height: Rc<Cell<f32>>,
    /// Middle-pane viewport width, snapshotted by `place_children` —
    /// mirrors `TableView::middle_viewport_width`.
    middle_viewport_width: Rc<Cell<f32>>,
    /// The row-area's absolute (window) rect (below the header), cached by
    /// `place_children`. Threaded into the keyboard handler so it can chase the
    /// focused row into any *enclosing* scroll area via
    /// [`EventContext::ensure_visible`](teksilo_core::widget::EventContext::ensure_visible).
    body_bounds: Rc<Cell<Rect>>,
    resize_state: ResizeStateHandle,
    /// Display slot of the column under an active resize drag, or `None`.
    /// Mirrors `TableView::resize_target` — shared with every `HeaderCell`
    /// so the *target* column carries the "resizing" chrome even when the
    /// gesture is anchored on its neighbour's half of the grip.
    resize_target: Signal<Option<usize>>,
    /// Window x of the prospective divider during a
    /// [`ColumnResizePolicy::OnRelease`] drag. Mirrors
    /// `TableView::resize_preview_x`.
    resize_preview_x: Signal<Option<f32>>,
    /// Width of the header strip (= the column band) snapshotted by
    /// `place_children`. Mirrors `TableView::header_strip_width` — the
    /// column-reorder drop handler needs it to mirror the drop x under RTL.
    header_strip_width: Rc<Cell<f32>>,
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
    /// [`TreeDataSlice::drag`](teksilo_data::TreeDataSlice) defaults to `NoDrag`: an
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
        S::Key: teksilo_data::ItemKey,
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
            edit_triggers: EditTriggers::default(),
            on_cell_edit_request: None,
            on_cell_edit_dismissed: None,
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
            scroll_x: Signal::new_animated(0.0),
            max_scroll_x: Signal::new(0.0),
            viewport_ratio_x: Signal::new(1.0),
            sort_signal: Signal::new(None),
            column_widths_signal: Signal::new(HashMap::new()),
            column_order_signal: Signal::new(Vec::new()),
            column_pinning_signal: Signal::new(HashMap::new()),
            filters_signal: Signal::new(HashMap::new()),
            focused_cell: Signal::new(None),
            row_map: Rc::new(RefCell::new(Vec::new())),
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
            h_scrollbar_id: None,
            empty_id: None,
            pane_version: Signal::new(0_u64),
            pane_built_start: Rc::new(Cell::new(0)),
            pane_built_end: Rc::new(Cell::new(0)),
            pane_total_refresh: Signal::new(0_u64),
            column_widths: Rc::new(RefCell::new(Vec::new())),
            display_indices: Rc::new(RefCell::new(Vec::new())),
            pane_boundaries: Rc::new(RefCell::new(crate::table_view::PaneBoundaries::default())),
            cell_map: Rc::new(RefCell::new(Vec::new())),
            viewport_height: Rc::new(Cell::new(600.0)),
            middle_viewport_width: Rc::new(Cell::new(600.0)),
            body_bounds: Rc::new(Cell::new(Rect::ZERO)),
            resize_state: Rc::new(RefCell::new(None)),
            resize_target: Signal::new(None),
            resize_preview_x: Signal::new(None),
            header_strip_width: Rc::new(Cell::new(0.0)),
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
    pub fn edit_triggers(mut self, trigger: EditTriggers) -> Self {
        self.edit_triggers = trigger;
        self
    }

    /// Callback invoked when the user requests an in-place cell edit (e.g.
    /// double-click when `edit_triggers` is `DoubleClick`). Receives the flat row
    /// index, the column id, and a mutable `EventContext`.
    pub fn on_cell_edit_request(
        mut self,
        f: impl Fn(usize, &str, &mut EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_request = Some(Rc::new(f));
        self
    }

    /// Callback invoked when an **open** cell editor should end because the
    /// pointer went somewhere else: a press that lands on any cell other than
    /// the one being edited. Receives the editing cell's flat row index and
    /// column id, so the owner can commit (or discard) whatever is in its
    /// buffer, then clear its own editing state.
    ///
    /// The counterpart of [`on_cell_edit_request`](Self::on_cell_edit_request),
    /// and the view cannot do it alone: the framework owns *which* cell is being
    /// edited, but only the owner knows what an ended edit means — commit,
    /// discard, or refuse a value that will not parse.
    ///
    /// **Why a press and not a focus change.** "The editor lost focus" is the
    /// obvious signal and it cannot be used: a body pane rebuilds constantly —
    /// selection, filtering, scroll, a reload from elsewhere — and every rebuild
    /// destroys and re-creates the open editor, so focus leaves it many times
    /// during an edit the writer never interrupted. A press on another cell is
    /// unambiguous and happens exactly once.
    pub fn on_cell_edit_dismissed(
        mut self,
        f: impl Fn(usize, &str, &mut EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_dismissed = Some(Rc::new(f));
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

    /// Current horizontal scroll offset of the Middle (unpinned) pane, in
    /// logical pixels. Leading/Trailing-pinned columns are unaffected.
    pub fn scroll_x_signal(&self) -> &Signal<f32> {
        &self.scroll_x
    }

    /// Maximum horizontal scroll offset — `middle_content_width −
    /// middle_viewport_width`.
    pub fn max_scroll_x_signal(&self) -> &Signal<f32> {
        &self.max_scroll_x
    }

    /// Middle-pane viewport-to-content width ratio.
    pub fn viewport_ratio_x_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_x
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

    /// The widget realized for the cell at `(row, display column)` in the body
    /// pane's latest build, or `None` once it has scrolled (or collapsed) out
    /// of the realized buffer — `cell_map` is a snapshot, not an index of every
    /// row the source holds, so a miss here means "not on screen", never "no
    /// such cell".
    fn realized_cell(&self, row: usize, col: usize) -> Option<WidgetId> {
        self.cell_map
            .borrow()
            .iter()
            .find(|&&(pos, _)| pos == (row, col))
            .map(|&(_, id)| id)
    }

    /// Access the underlying `SortFilterTreeModel` (for programmatic sort /
    /// filter / expand outside of the builder API).
    /// `None` when the view was built from an external
    /// [`teksilo_data::TreeDataSource`] via
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
    ///
    /// Equality-guarded, like every persisted-layout setter here — see
    /// [`set_column_widths`](Self::set_column_widths).
    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        imperative::set_if_changed(&self.sort_signal, col_id.map(|c| (c.to_string(), dir)));
    }

    /// Set or clear the filter text for a single column.
    pub fn set_filter(&self, col_id: &str, text: &str) {
        imperative::set_filter(&self.filters_signal, col_id, text);
    }

    pub fn clear_filters(&self) {
        imperative::set_if_changed(&self.filters_signal, HashMap::new());
    }

    /// Widget shown when no rows are visible — an empty tree, or a filter
    /// that matched nothing. Without one, the body region is simply blank.
    pub fn empty_view(mut self, f: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.empty_view = Some(Rc::new(f));
        self
    }

    /// Clear the active sort.
    pub fn clear_sort(&self) {
        imperative::set_if_changed(&self.sort_signal, None);
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

    /// Scroll the row the keyboard cursor sits on into view when this view
    /// takes focus.
    ///
    /// Only the rows near the viewport are realized, so on a tree taller than
    /// the window the cursor row frequently has no widget. Everything that
    /// speaks for it then has nothing to speak about: no cell node exists, so
    /// `accessibility()` below finds nothing in `cell_map` and nominates no
    /// `active_descendant`, and a screen reader taking focus here is told
    /// nothing at all. The first arrow press steps *past* that row as well,
    /// because the cursor was somewhere the user was never shown.
    ///
    /// The cursor is read exactly as the shared keyboard handler reads it
    /// (`table_view::keyboard::build_key_handler`, `keyboard.rs:134-139`): the
    /// focused cell's row, else the first selected row. Anything else would
    /// reveal a row the next arrow press does not step from.
    ///
    /// That row index is a **flat visible** index, not a position in the
    /// unflattened tree: a collapsed node's descendants have no index at all
    /// here. Checked on both sides of the read. `focused_cell` is clamped to
    /// `TreeNavigator::row_count()`, which returns `TreeSource::visible_count()`
    /// (`tree_table_view.rs:129-131`), and the keyed selection facade builds
    /// its indices by scanning `0..visible_count()` through
    /// `SortFilterTreeModel::visible_node_id` (`data_views.rs:545-552`). On the
    /// spending side, `RowMetrics` is sized by `place_children` from that same
    /// `visible_count()`, so `row_top(i)` is the top of the *i*-th visible row.
    ///
    /// `ensure_row_visible`, the view's own imperative path, rather than
    /// `scroll_to_row`: a row already on screen must not jump under somebody
    /// who can see it. It carries the `laid_out` guard too, so a focus that
    /// arrives before the first real height is a no-op instead of scrolling
    /// against a viewport that was never measured. It is the same
    /// `RowMetrics::scroll_for_ensure_visible` arithmetic the keyboard runs on
    /// every arrow press; what the keyboard's own wrapper
    /// (`table_view/keyboard.rs:445`) adds on top is chasing the row into an
    /// *enclosing* scroll area, and that needs an `EventContext`, which an
    /// effect does not have. Nothing is lost: the same keyboard or programmatic
    /// focus change makes the framework reveal the newly focused widget in
    /// every ancestor scroll area itself (`focus_impl.rs:95-96`,
    /// `WidgetTree::scroll_focused_into_view`), so the enclosing viewport is
    /// somebody else's job here.
    ///
    /// The handles are cloned into the effect rather than reaching through
    /// `self`, which the closure cannot borrow.
    fn reveal_current_row_on_focus(&self, ctx: &mut BuildContext) {
        let focused_cell = self.focused_cell.clone();
        let selection = self.row_selection.clone();
        let row_metrics = self.row_metrics.clone();
        let scroll_y = self.scroll_y.clone();
        let max_scroll_y = self.max_scroll_y.clone();
        let viewport_height = self.viewport_height.clone();
        let laid_out = self.laid_out.clone();

        ctx.effect(&self.view_focused, move |focused| {
            if !*focused {
                return;
            }
            let Some(row) = focused_cell.get().map(|(row, _col)| row).or_else(|| {
                selection
                    .as_ref()
                    .and_then(|s| s.selected_indices().first().copied())
            }) else {
                return;
            };
            imperative::ensure_row_visible(
                row,
                &row_metrics,
                &scroll_y,
                &max_scroll_y,
                viewport_height.get(),
                laid_out.get(),
            );
        });
    }

    /// Set or remove a single column's user-resized width override.
    /// A non-positive `width` removes the entry (the column reverts to
    /// its declared width policy).
    pub fn set_column_width(&self, col_id: &str, width: f32) {
        imperative::set_column_width(&self.column_widths_signal, col_id, width);
    }

    /// Replace the full width-override map (typically used to restore
    /// a persisted layout).
    ///
    /// Equality-guarded for the same reason as
    /// [`TableView::set_column_widths`](crate::TableView::set_column_widths):
    /// the documented settings round-trip would otherwise recurse without
    /// bound on the first tick of a live resize drag.
    pub fn set_column_widths(&self, widths: HashMap<String, f32>) {
        imperative::set_column_widths(&self.column_widths_signal, widths);
    }

    /// Replace the column-order list. Ids not declared on this table
    /// are silently dropped on the next layout pass.
    pub fn set_column_order(&self, order: Vec<String>) {
        imperative::set_if_changed(&self.column_order_signal, order);
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
        let leading_count = out.len();
        out.extend(middle);
        let middle_end = out.len();
        out.extend(trailing);
        // Stash the boundaries so paint / place_children / the keyboard
        // handler's ensure-column-visible can read them — mirrors
        // `TableView::display_order`.
        *self.pane_boundaries.borrow_mut() =
            crate::table_view::PaneBoundaries::new(leading_count, middle_end);
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

        self.scroll_x.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_x);

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
        // `OnRelease` resize guide line — paint-only, nothing moves until the
        // button comes up.
        self.resize_preview_x.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Abandon an in-flight resize when the window goes inactive — see
        // `TableView::build` for why the missing PointerUp would otherwise
        // leave the column dragging with no button held.
        {
            let resize_state = self.resize_state.clone();
            let resize_target = self.resize_target.clone();
            let resize_preview_x = self.resize_preview_x.clone();
            ctx.effect(&ctx.window_active_signal(), move |active| {
                if !*active && resize_state.borrow().is_some() {
                    *resize_state.borrow_mut() = None;
                    resize_target.set(None);
                    resize_preview_x.set(None);
                }
            });
        }
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
        self.reveal_current_row_on_focus(ctx);
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
        let scroll_x_for_wheel = self.scroll_x.clone();
        let max_scroll_x_for_wheel = self.max_scroll_x.clone();
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
        // The effective trigger set per display column: the view's, overridden
        // by the column's own, and `NONE` for a non-editable one. Resolved here
        // so the keyboard handler never has to reach a `Column<T>`.
        let display_col_triggers: Rc<dyn Fn(usize) -> EditTriggers> = {
            let view_triggers = self.edit_triggers;
            let per_display_column: Vec<EditTriggers> = display_indices
                .iter()
                .map(|&i| self.columns[i].effective_edit_triggers(view_triggers))
                .collect();
            Rc::new(move |pos| {
                per_display_column
                    .get(pos)
                    .copied()
                    .unwrap_or(EditTriggers::NONE)
            })
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
            display_col_to_id,
            display_col_triggers,
            on_cell_edit_request: self.on_cell_edit_request.clone(),
            on_row_activate: self.on_row_activate.clone(),
            type_ahead: self.type_ahead.clone(),
            type_ahead_label,
            type_ahead_timeout: self.type_ahead_timeout,
            column_widths: self.column_widths.clone(),
            pane_boundaries: *self.pane_boundaries.borrow(),
            scroll_x: self.scroll_x.clone(),
            max_scroll_x: self.max_scroll_x.clone(),
            middle_viewport_width: self.middle_viewport_width.clone(),
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
        let key_handler = move |event: &teksilo_core::event::WidgetEvent,
                                ctx: &mut EventContext|
              -> EventResponse {
            use teksilo_core::event::{Key, WidgetEvent};
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
                    teksilo_core::event::WidgetEvent::Scroll { delta, modifiers } => {
                        let (raw_dx, raw_dy) = match delta {
                            teksilo_core::event::ScrollDelta::Lines { x, y } => {
                                (x * line_height, y * line_height)
                            }
                            teksilo_core::event::ScrollDelta::Pixels { x, y } => (*x, *y),
                        };
                        // Shift+wheel remaps a vertical-only wheel to
                        // horizontal scroll (the `TabBar` precedent).
                        let (dx, dy) = if modifiers.shift() && raw_dx.abs() < f32::EPSILON {
                            (raw_dy, 0.0)
                        } else {
                            (raw_dx, raw_dy)
                        };

                        let mut moved_any = false;
                        if dy.abs() > 0.0 {
                            let current = scroll_y_for_wheel.get();
                            let max = max_scroll_for_wheel.get();
                            // Base off the animation target (not the rendered
                            // offset) so a mid-fling boundary correctly chains
                            // and successive notches accumulate instead of
                            // restarting from the partway-animated position.
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
                            moved_any |= moved;
                        }
                        if dx.abs() > 0.0 {
                            let current = scroll_x_for_wheel.get();
                            let max = max_scroll_x_for_wheel.get();
                            let base = scroll_x_for_wheel.animation_target().unwrap_or(current);
                            let (new_x, moved) =
                                crate::common::scroll::scroll_clamp_axis(base, dx, max);
                            if moved {
                                if smooth_scrolling {
                                    scroll_x_for_wheel.animate_to(
                                        new_x,
                                        smooth_scroll_duration,
                                        Easing::EaseOut,
                                    );
                                } else {
                                    scroll_x_for_wheel.set(new_x);
                                }
                            }
                            moved_any |= moved;
                        }
                        // Chain to an ancestor scrollable when fully
                        // clamped (unless Contain), otherwise consume —
                        // same contract as ListView/TreeView/TableView.
                        crate::common::scroll::scroll_response(
                            moved_any,
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
                // Column reorder is handled by the header strip
                // (`attach_header_reorder_handlers`); only row-level drops
                // get an insertion/into affordance here. Without this bail,
                // a `ColumnReorderDragData` dragged past the header into the
                // body would fall through to `on_foreign_drop` (which
                // accepts any payload type) and paint a row-drop visual for
                // a drag the header strip is already handling.
                if payload.has_typed::<ColumnReorderDragData>() {
                    feedback_for_hover.set(None);
                    return teksilo_core::DropFeedback::NoFeedback;
                }
                // Real body width, so the affordance spans the actual row area
                // rather than a placeholder.
                let viz_width = bounds_for_hover.get().width.max(1.0);
                let count = source_for_hover.visible_count();
                if count == 0 {
                    feedback_for_hover.set(None);
                    return teksilo_core::DropFeedback::NoFeedback;
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
                    return teksilo_core::DropFeedback::NoFeedback;
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
                // `depth` rides along so `paint` can indent the affordance to
                // the level the dropped row lands at — see `TreeView`'s twin of
                // this block. A foreign drop lands at a flat index the view
                // cannot promise a nesting for, so it claims none: depth 0.
                let (effective, depth) = if reorder_ok {
                    match (source_for_hover.dnd.can_accept_fn)(
                        payload,
                        row_idx,
                        drop_pos,
                        my_model_id,
                    ) {
                        DropResponse::Reject => {
                            if !foreign_ok {
                                feedback_for_hover.set(None);
                                return teksilo_core::DropFeedback::NoFeedback;
                            }
                            (DropPosition::Before, 0)
                        }
                        DropResponse::Accept => (drop_pos, source_for_hover.depth(row_idx)),
                        DropResponse::Redirect(p) => (p, source_for_hover.depth(row_idx)),
                    }
                } else {
                    // A foreign source has no Into/reparent semantics to honor.
                    (DropPosition::Before, 0)
                };
                if effective == DropPosition::Into {
                    let top = row_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Rect {
                        top,
                        height: row_h,
                        width: viz_width,
                        depth,
                    }));
                    teksilo_core::DropFeedback::HighlightRect {
                        rect: Rect::new(0.0, top, viz_width, row_h),
                        color: drop_into_tint(),
                    }
                } else {
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Line {
                        y: insertion_y,
                        width: viz_width,
                        depth,
                    }));
                    teksilo_core::DropFeedback::InsertionLine {
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
                // See the matching bail in `on_drag_hover` above — a column
                // reorder drop is the header strip's, never the body's
                // (`on_foreign_drop` would otherwise swallow it).
                if payload.has_typed::<ColumnReorderDragData>() {
                    return false;
                }
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
        self.h_scrollbar_id = None;
        self.empty_id = None;

        // Header strip.
        if self.show_header {
            // See `TableView::build`: a rebuild drops the pointer capture an
            // in-flight resize rides on, so the shared drag state must go with
            // it or a later bare PointerMove would resize with no button held.
            *self.resize_state.borrow_mut() = None;
            self.resize_target.set(None);
            self.resize_preview_x.set(None);

            let boundaries = *self.pane_boundaries.borrow();
            let resize_columns: ColumnResizeTable = Rc::new(
                display_indices
                    .iter()
                    .map(|&i| {
                        let c = &self.columns[i];
                        ColumnResizeInfo {
                            id: c.id.clone(),
                            min_width: c.min_width.unwrap_or(cp::MIN_COLUMN_WIDTH_DEFAULT),
                            max_width: c.max_width,
                            resizable: c.resizable,
                        }
                    })
                    .collect(),
            );
            let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
            let active_sort = self.sort_signal.get();
            for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                let col = &self.columns[col_idx];
                let current_sort = active_sort
                    .as_ref()
                    .and_then(|(id, dir)| if id == &col.id { Some(*dir) } else { None });
                let filter_zone_width = cp::FILTER_INDICATOR_SIZE + cp::CELL_PADDING_HORIZONTAL;
                let cell = HeaderCell::new(HeaderCellSpec {
                    col_id: col.id.clone(),
                    label: col.header_label.resolve_now(),
                    col_index_1based: display_pos + 1,
                    sortable: col.sortable,
                    reorderable: col.reorderable,
                    filterable: col.filterable,
                    resize_grip: cp::RESIZE_HANDLE_WIDTH,
                    filter_zone_width,
                    current_sort,
                    width_index: display_pos,
                    pane_boundaries: boundaries,
                    resize_columns: resize_columns.clone(),
                    resize_policy: self.column_resize_policy,
                    resize_state: self.resize_state.clone(),
                    resize_target: self.resize_target.clone(),
                    resize_preview_x: self.resize_preview_x.clone(),
                    table_id: self.table_id,
                    sort_signal: self.sort_signal.clone(),
                    column_widths_signal: self.column_widths_signal.clone(),
                    column_widths: self.column_widths.clone(),
                    filters_signal: self.filters_signal.clone(),
                });
                cell_ids.push(ctx.add(cell));
            }
            let header_row = HeaderRow::new(
                cell_ids,
                self.column_widths.clone(),
                cp::GRID_LINE_THICKNESS,
                *self.pane_boundaries.borrow(),
                self.scroll_x.clone(),
            );
            // Wire reorder drag-target handlers on the header strip — the
            // shared drop-target half of the mechanism `HeaderCell` already
            // escalates a press into (see `table_view::header`). The tree
            // column reorders like any other column: it carries no special
            // case here, since `tree_display_pos` (re-resolved from
            // `display_indices` on every rebuild — see below) is what makes
            // the indent/twist gutter and Left/Right expand-collapse follow
            // it wherever the drop lands, including into the leading- or
            // trailing-pinned pane.
            let header_row_id = ctx.add(header_row);
            attach_header_reorder_handlers(
                ctx,
                header_row_id,
                self.table_id,
                self.column_widths.clone(),
                self.display_indices.clone(),
                self.pane_boundaries.clone(),
                self.column_order_signal.clone(),
                self.column_pinning_signal.clone(),
                self.columns.iter().map(|c| c.id.clone()).collect(),
                self.header_strip_width.clone(),
                self.scroll_x.clone(),
            );
            self.header_row_id = Some(header_row_id);
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
                pane_boundaries: *self.pane_boundaries.borrow(),
                scroll_x: self.scroll_x.clone(),
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
                edit_triggers: self.edit_triggers,
                on_cell_edit_request: self.on_cell_edit_request.clone(),
                on_cell_edit_dismissed: self.on_cell_edit_dismissed.clone(),
                version: self.pane_version.clone(),
                prev_built_start: self.pane_built_start.clone(),
                prev_built_end: self.pane_built_end.clone(),
                total_refresh: self.pane_total_refresh.clone(),
                row_entries: Vec::new(),
                row_map: self.row_map.clone(),
                cell_map: self.cell_map.clone(),
            };
            self.body_pane_id = Some(ctx.add(pane));
            // An open cell editor also ends on a press that lands on no cell at
            // all — the empty band under the last row. Mounted here rather than
            // on the pane because the pane is not the hit target there.
            if let Some(handlers) = crate::table_view::body_pane::root_edit_dismiss_handler(
                &self.on_cell_edit_dismissed,
                &self.editing_cell,
                &Rc::new(
                    display_indices
                        .iter()
                        .map(|&i| self.columns[i].id.clone())
                        .collect::<Vec<_>>(),
                ),
            ) {
                ctx.apply_self_handlers(handlers);
            }
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

            // Horizontal bar — the Middle pane only, mirrors `TableView`.
            let hsb = ScrollBar::new(
                ScrollBarOrientation::Horizontal,
                self.scroll_x.clone(),
                self.max_scroll_x.clone(),
                self.viewport_ratio_x.clone(),
            )
            .visual(match self.scroll_bar_style {
                ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
                ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
                ScrollBarMode::Thin => ScrollBarVisual::Thin,
            });
            self.h_scrollbar_id = Some(ctx.add(hsb));
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
        if let Some(id) = self.h_scrollbar_id {
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
    ) -> teksilo_core::widget::LayoutResponse {
        // Only an allocation may seed the cached viewport (`common::viewport`);
        // the body pane shares this very cell, so a measurement's fallback
        // would desync its realization window.
        let size = crate::common::viewport::viewport_size(
            proposal,
            &self.viewport_height,
            Size::new(400.0, 300.0),
        );
        if proposal.height.is_some() {
            // Viewport-relative imperatives are meaningful from here on — but
            // only once a real height has landed, for the reason `laid_out`
            // exists at all.
            self.laid_out.set(true);
        }
        size.into()
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
        let body_height_provisional = (bounds.height - header_h).max(0.0);

        // Parent-before-child layout order means this runs before the
        // body pane's measure pass — in auto-measure mode the scrollbar
        // totals settle one frame after a measurement change.
        let total_height = self
            .row_metrics
            .borrow_mut()
            .total_height(self.source.visible_count());
        let needs_v_scrollbar =
            self.show_internal_scrollbars && total_height > body_height_provisional + 0.5;
        // Permanent reserves a layout column for the bar; Overlay / Thin
        // float over the content, so the body spans the full width.
        let reserves_v_bar = needs_v_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_width = if reserves_v_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        // RTL mirror (see TableView::place_children): scrollbar to the
        // physical left, body/header band shifted right by its thickness.
        // Only shift when the bar actually reserves a column (Permanent).
        let band_left = if rtl && reserves_v_bar {
            bounds.x + SCROLLBAR_THICKNESS
        } else {
            bounds.x
        };
        let scrollbar_x = if rtl {
            bounds.x
        } else {
            bounds.x + bounds.width - SCROLLBAR_THICKNESS
        };
        // The header strip spans the band; snapshot its width for the
        // reorder-drop handler's RTL mirror (see `TableView::place_children`).
        self.header_strip_width.set(body_width);

        let overrides = self.column_widths_signal.get();
        let display = self.display_indices.borrow().clone();
        let widths = layout::ColumnSolver::resolve_in_order(
            &self.columns,
            &display,
            body_width,
            cp::MIN_COLUMN_WIDTH_DEFAULT,
            &overrides,
        );

        // Pane geometry (see `TableView::place_children`).
        let boundaries = *self.pane_boundaries.borrow();
        let (leading_w, middle_content_w, trailing_w) = layout::pane_widths(&widths, boundaries);
        let middle_viewport_w = (body_width - leading_w - trailing_w).max(0.0);
        let max_x = (middle_content_w - middle_viewport_w).max(0.0);
        self.max_scroll_x.set(max_x);
        self.middle_viewport_width.set(middle_viewport_w);
        let x_ratio = if middle_content_w > 0.0 {
            (middle_viewport_w / middle_content_w).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_x.set(x_ratio);
        {
            let current = self.scroll_x.get();
            let clamped = current.clamp(0.0, max_x);
            if (clamped - current).abs() > 0.001 {
                self.scroll_x.set(clamped);
            }
        }

        *self.column_widths.borrow_mut() = widths;

        let needs_h_scrollbar = self.show_internal_scrollbars && max_x > 0.5;
        let reserves_h_bar = needs_h_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_height = if reserves_h_bar {
            (body_height_provisional - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            body_height_provisional
        };

        let max_y = (total_height - body_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let y_ratio = if total_height > 0.0 {
            (body_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(y_ratio);
        self.clamp_scroll();

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
                if needs_v_scrollbar {
                    child.origin = Point::new(scrollbar_x, body_origin_y);
                    child.size = Size::new(SCROLLBAR_THICKNESS, body_height);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
            next += 1;
        }

        // Horizontal scrollbar — the Middle pane's own band, below the body.
        if self.h_scrollbar_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                if needs_h_scrollbar {
                    let h_x = if rtl {
                        band_left + trailing_w
                    } else {
                        band_left + leading_w
                    };
                    child.origin = Point::new(h_x, body_origin_y + body_height);
                    child.size = Size::new(middle_viewport_w, SCROLLBAR_THICKNESS);
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
        let rtl = ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;
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

        // Pane geometry for the two column-position-dependent decorations
        // below — see `TableView::paint`.
        let boundaries = *self.pane_boundaries.borrow();
        let scroll_x = self.scroll_x.get();
        let content_bounds = Rect::new(
            content_left,
            body_origin_y,
            body_width_for_paint,
            body_height,
        );
        let (leading_rect, middle_rect, trailing_rect) =
            layout::band_rects(content_bounds, &widths, boundaries, rtl);

        if matches!(self.grid_lines, GridLines::Vertical | GridLines::Both) {
            let leading_end = boundaries.leading_count.min(widths.len());
            let middle_end = boundaries.middle_end.min(widths.len()).max(leading_end);
            crate::table_view::draw_pane_dividers(
                canvas,
                leading_rect,
                &widths[..leading_end],
                0.0,
                rtl,
                line_color,
                line_w,
            );
            crate::table_view::draw_pane_dividers(
                canvas,
                middle_rect,
                &widths[leading_end..middle_end],
                scroll_x,
                rtl,
                line_color,
                line_w,
            );
            crate::table_view::draw_pane_dividers(
                canvas,
                trailing_rect,
                &widths[middle_end..],
                0.0,
                rtl,
                line_color,
                line_w,
            );
        }

        // Focus ring — keyboard-only (`:focus-visible`) and only while the
        // view holds focus, so a mouse click never leaves a ring.
        if self.view_focused.get()
            && self.focus_visible.get()
            && let Some((focus_row, focus_col)) = self.focused_cell.get()
            && focus_col < widths.len()
            && let Some(x_off) = layout::column_logical_x(
                &widths,
                boundaries,
                scroll_x,
                body_width_for_paint,
                focus_col,
            )
        {
            let cell_w = widths[focus_col];
            let (focus_top, focus_h) = {
                let mut m = self.row_metrics.borrow_mut();
                (m.row_top(focus_row), m.row_height(focus_row))
            };
            let y = body_origin_y + focus_top - scroll_y;
            if y + focus_h >= body_origin_y && y <= body_origin_y + body_height {
                let pane_rect = if focus_col < boundaries.leading_count {
                    leading_rect
                } else if focus_col >= boundaries.middle_end {
                    trailing_rect
                } else {
                    middle_rect
                };
                canvas.set_clip(pane_rect);
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
                canvas.clear_clip();
            }
        }

        // Row-drop insertion indicator (source-accepted positions only — a
        // forbidden hover clears the signal). `y` is stored body-local.
        //
        // Both affordances are indented to the level the dropped row lands at,
        // measured from the **tree column's** own leading edge rather than the
        // body's: `.tree_column()` and a user column-reorder can move the
        // twist/indent gutter off the leading slot, and an indent measured from
        // the wrong origin points at nothing. The per-level step is this view's
        // `effective_indent()` — the very value its indent gutter renders with
        // — not the container recipe's, which describes `StandardTreeItem`.
        let drop_indent_origin = |depth: usize| -> f32 {
            let step = self.effective_indent();
            let tree_decl = self.tree_column_decl_index();
            let tree_slot = self
                .display_indices
                .borrow()
                .iter()
                .position(|&i| i == tree_decl)
                .unwrap_or(0);
            let col_x = layout::column_logical_x(
                &widths,
                boundaries,
                scroll_x,
                body_width_for_paint,
                tree_slot,
            )
            .unwrap_or(0.0);
            (col_x + depth as f32 * step).clamp(0.0, body_width_for_paint)
        };
        match self.drop_feedback.get() {
            Some(DropViz::Line { y, depth, .. }) => {
                let recipe = ctx
                    .theme
                    .style_slots
                    .list_container
                    .as_ref()
                    .map(|s| s.insertion())
                    .unwrap_or_default();
                let line_color = recipe.role.resolve(colors);
                let thickness = recipe.thickness;
                let line_y = body_origin_y + y - thickness * 0.5;
                let indent = drop_indent_origin(depth);
                // RTL mirrors the row, so the indent eats into the *right* edge
                // and the line still runs away from the row's leading side.
                let x = if rtl {
                    content_left
                } else {
                    content_left + indent
                };
                canvas.fill_rect(
                    Rect::new(x, line_y, body_width_for_paint - indent, thickness),
                    line_color,
                );
            }
            // "Drop into this container" — a box round the target row, inset on
            // every side so its horizontal edges can never be mistaken for the
            // Before / After line. Same affordance `TreeView` paints for an
            // `Into` verdict; see `ListDropIntoRecipe`.
            Some(DropViz::Rect {
                top, height, depth, ..
            }) => {
                let into = ctx
                    .theme
                    .style_slots
                    .list_container
                    .as_ref()
                    .map(|s| s.drop_into())
                    .unwrap_or_default();
                let color = into.role.resolve(colors);
                let indent = drop_indent_origin(depth);
                let x = if rtl {
                    content_left
                } else {
                    content_left + indent
                };
                let rect = Rect::new(
                    x + into.inset,
                    body_origin_y + top + into.inset,
                    (body_width_for_paint - indent - into.inset * 2.0).max(0.0),
                    (height - into.inset * 2.0).max(0.0),
                );
                let radius = teksilo_tokens::CornerRadius::uniform(into.corner_radius);
                canvas.fill_rounded_rect(rect, radius, color.with_alpha(into.fill_alpha));
                canvas.stroke_rounded_rect(rect, radius, color, into.thickness);
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

        // `OnRelease` column-resize guide — see `TableView::paint`.
        if let Some(x) = self.resize_preview_x.get() {
            let thickness = cp::GRID_LINE_THICKNESS.max(1.5);
            canvas.fill_rect(
                Rect::new(x - thickness * 0.5, bounds.y, thickness, bounds.height),
                BorderRole::Focused.resolve(colors),
            );
        }
    }

    /// The context-menu key opens the *current row's* menu, not the view's.
    ///
    /// A `TreeTableView` is focusable and its rows deliberately are not — the
    /// container owns focus and `set_selected` is what tells assistive
    /// technology which row is current. So the dispatcher's default of "the
    /// focused widget" would open the view's own menu, in the widget family
    /// where a per-row menu matters most.
    ///
    /// The row the user means is the focused cell's row if they have navigated,
    /// else the first selected row. Only realized rows have a widget, so a
    /// cursor scrolled outside the virtualization window resolves to nothing
    /// and the menu falls back to the view — right, because there is no row on
    /// screen for it to be about.
    fn context_menu_key_target(&self) -> Option<WidgetId> {
        let index = self.focused_cell.get().map(|(row, _col)| row).or_else(|| {
            self.row_selection
                .as_ref()
                .and_then(|s| s.selected_indices().first().copied())
        })?;
        let map = self.row_map.borrow();
        map.iter().find(|(i, _)| *i == index).map(|(_, id)| *id)
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::TreeGrid);
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
        if let Some((row, col)) = self.focused_cell.get()
            && let Some(cell_id) = self.realized_cell(row, col)
        {
            builder.set_active_descendant(widget_id_to_node_id(cell_id));
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
        if let Some(id) = self.h_scrollbar_id {
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
            self.h_scrollbar_id,
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
    use teksilo_canvas::SizeProposal;
    use teksilo_core::accesskit::Role;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_data::{SortFilterTreeModel, TreeFilterMode, TreeModel};
    use teksilo_i18n::lit;

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
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};
        let proxy = SortFilterTreeModel::new(sample_tree());
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

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
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_data::{SelectionMode, SelectionModel};

        let proxy = SortFilterTreeModel::new(sample_tree()); // docs{readme,guide}, src{main.rs}
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_canvas::Point;
        use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
        use teksilo_data::{SelectionMode, SelectionModel};
        let t = TreeModel::new();
        t.insert_root(0, "a");
        t.insert_root(1, "b");
        t.insert_root(2, "c");
        t.insert_root(3, "d");
        let proxy = SortFilterTreeModel::new(t);
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
            teksilo_core::event::Key::ArrowRight,
            teksilo_core::event::Modifiers::NONE,
        );
        assert_eq!(proxy.visible_count(), 4);
        // ArrowLeft on first row (now expanded) → collapse.
        tree.press_key(
            teksilo_core::event::Key::ArrowLeft,
            teksilo_core::event::Modifiers::NONE,
        );
        assert_eq!(proxy.visible_count(), 2);
    }

    /// Rows for the external-source tests: an indent-ordered stream keyed by a
    /// domain id, the shape `TreeDataSlice` derives a hierarchy from.
    fn slice_rows() -> Vec<teksilo_data::TreeRow<u64, &'static str>> {
        use teksilo_data::TreeRow;
        vec![
            TreeRow::new(1, "docs", 0),
            TreeRow::new(2, "readme", 1),
            TreeRow::new(3, "guide", 1),
            TreeRow::new(4, "src", 0),
        ]
    }

    fn external_slice() -> teksilo_data::TreeDataSlice<u64, &'static str> {
        let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let keyed = KeyedSelectionModel::<u64>::new(teksilo_data::SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_canvas::Point;

        // The store the slice re-sources from; the reorder mutates it.
        let order: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(vec![1, 4]));
        let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
        {
            let order = order.clone();
            slice.set_source(move || {
                let names: std::collections::HashMap<u64, &'static str> =
                    [(1, "docs"), (4, "src")].into_iter().collect();
                order
                    .borrow()
                    .iter()
                    .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
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
        slice.set_drag_policy(|_| teksilo_data::DragEligibility::CanDrag);
        slice.reload();
        assert_eq!(*order.borrow(), vec![1, 4], "docs, src");

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_canvas::Point;

        let order: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(vec![1, 4]));
        let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
        {
            let order = order.clone();
            slice.set_source(move || {
                let names: std::collections::HashMap<u64, &'static str> =
                    [(1, "docs"), (4, "src")].into_iter().collect();
                order
                    .borrow()
                    .iter()
                    .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
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
                teksilo_data::DragEligibility::NoDrag
            } else {
                teksilo_data::DragEligibility::CanDrag
            }
        });
        slice.reload();

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.collapse_all(); // roots only: docs@0, src@1
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    fn the_into_box_is_inset_and_the_insertion_line_is_indented() {
        // The twin of `TreeView`'s pair: the two drop affordances must not read
        // alike. Flush to the row, the Into box's top edge is the very pixel a
        // Before line occupies — and the drag ghost hides the vertical sides
        // that would have told them apart.
        use teksilo_canvas::{DrawCommand, Point, ShapeKind};
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.expand_all(); // docs@0 readme@1 guide@2 src@3 main.rs@4
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

        // Hold a drag from "main.rs" (flat 4) — nothing is inside its subtree,
        // so every target below accepts.
        let start = Point::new(40.0, h + 90.0);
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: start,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(52.0, start.y),
        });

        // Bottom third of "readme" (flat 1, depth 1) → After, at depth 1.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(52.0, h + 38.0),
        });
        let frame = tree.render();
        let line_recipe = teksilo_core::styles::ListInsertionRecipe::default();
        // The insertion line is the only decoration exactly `thickness` tall
        // that spans the body — identify it by that, not by "something at x>0",
        // which any future row stripe would satisfy vacuously.
        let lines: Vec<_> = frame
            .decorations
            .iter()
            .filter(|d| (d.rect[3] - line_recipe.thickness).abs() < 0.01 && d.rect[2] > 100.0)
            .collect();
        assert_eq!(lines.len(), 1, "exactly one insertion line, got {lines:?}");
        assert!(
            lines[0].rect[0] >= line_recipe.indent_step,
            "the After line must start one indent step in for a depth-1 target, \
             got x = {} (step {})",
            lines[0].rect[0],
            line_recipe.indent_step
        );

        // Middle third of "docs" (flat 0, depth 0) → Into, a box round the row.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(52.0, h + 10.0),
        });
        let frame = tree.render();
        let recipe = teksilo_core::styles::ListDropIntoRecipe::default();
        let boxes: Vec<_> = frame
            .draw_order
            .iter()
            .filter_map(|c| match c {
                DrawCommand::Shape(i) => frame.shapes.get(*i),
                _ => None,
            })
            .filter(|s| s.shape == ShapeKind::RoundedRect && s.corner_radii[0] > 0.0)
            .filter(|s| (s.screen[3] - (20.0 - recipe.inset * 2.0)).abs() < 0.01)
            .collect();
        assert!(
            !boxes.is_empty(),
            "no inset rounded box for the Into hover; shapes = {:?}",
            frame.shapes.iter().map(|s| s.screen).collect::<Vec<_>>()
        );
        // Row 0 spans [h, h + 20]. The box's top edge must sit *inside* that
        // band — on the boundary it is pixel-identical to a Before line.
        assert!(
            boxes
                .iter()
                .all(|s| (s.screen[1] - (h + recipe.inset)).abs() < 0.01),
            "the Into box must be inset from the row's top edge ({}), got {:?}",
            h,
            boxes.iter().map(|s| s.screen).collect::<Vec<_>>()
        );
        assert!(
            boxes.iter().any(|s| s.stroke_width > 0.0)
                && boxes.iter().any(|s| s.stroke_width == 0.0),
            "the Into box needs both a wash and an outline"
        );
    }

    #[test]
    fn an_active_sort_suppresses_drag_reorder() {
        // With the visible order driven by a sort, a manual reorder would have no
        // visible effect — so it must be refused outright rather than silently
        // mutating the tree behind the sort.
        use teksilo_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree())
            .with_comparator("name", |a: &&'static str, b: &&'static str| a.cmp(b));
        proxy.collapse_all();
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
        let all: Vec<u64> = vec![1, 2, 3];
        slice.set_source(move || {
            let names: std::collections::HashMap<u64, &'static str> =
                [(1, "one"), (2, "two"), (3, "three")].into_iter().collect();
            all.iter()
                .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
                .collect()
        });
        slice.reload();

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
                .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
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
                .map(|k| teksilo_data::TreeRow::new(*k, "two", 0))
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
        use teksilo_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(wide_tree(10));
        let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    fn ctrl_arrow_moves_cursor_without_touching_selection() {
        // Explorer/Finder convention (shared with `TableView` via the
        // common `keyboard::build_key_handler`): Ctrl+Arrow repositions the
        // keyboard cursor without touching selection; plain Arrow keeps its
        // existing select-follow behavior.
        use teksilo_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(wide_tree(5));
        let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

        tree.press_key(Key::ArrowDown, Modifiers::CTRL);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(
                tt.focused_cell_signal().get(),
                Some((1, 0)),
                "cursor advances"
            );
        }
        assert_eq!(
            selection.selected_indices(),
            vec![0],
            "Ctrl+Arrow must not touch selection"
        );

        // Plain Arrow (no Ctrl) resumes select-follow from the cursor.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(tt.focused_cell_signal().get(), Some((2, 0)));
        }
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "plain Arrow selects the row it lands on"
        );
    }

    #[test]
    fn ctrl_space_toggles_the_cursor_row_after_a_ctrl_arrow_move() {
        use teksilo_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(wide_tree(5));
        let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        tree.press_key(Key::ArrowDown, Modifiers::CTRL);
        tree.press_key(Key::ArrowDown, Modifiers::CTRL);
        assert!(selection.selected_indices().is_empty());

        tree.press_key(Key::Space, Modifiers::CTRL);
        assert_eq!(
            selection.selected_indices(),
            vec![2],
            "Ctrl+Space toggles the focused row on"
        );

        tree.press_key(Key::Space, Modifiers::CTRL);
        assert!(
            selection.selected_indices().is_empty(),
            "Ctrl+Space toggles it back off"
        );
    }

    #[test]
    fn empty_view_renders_when_the_tree_has_no_rows() {
        let proxy = SortFilterTreeModel::new(TreeModel::<&'static str>::new());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

    /// Taking focus reveals the row the keyboard cursor sits on.
    ///
    /// Only the rows near the viewport are realized, so a cursor placed before
    /// the view is ever looked at (a restored position, a preselected row)
    /// usually has no cell widget at all. Nothing then speaks for it:
    /// `accessibility()` finds no entry in `cell_map`, so it nominates no
    /// `active_descendant`, and a screen reader arriving here is told nothing.
    /// The first arrow press steps *past* that row as well, because the cursor
    /// was somewhere nobody was shown.
    ///
    /// Asserted on the accessibility tree, since that is what the failure was
    /// about: the cell has to be a node a platform can name.
    #[test]
    fn taking_focus_reveals_the_cursor_row() {
        let proxy = SortFilterTreeModel::new(wide_tree(1000));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0),
        );
        let viewport = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(viewport);

        // Place the cursor far below the viewport WITHOUT giving the view
        // focus. `set_focused_cell` never scrolls on its own: only the key
        // handler does (`table_view/keyboard.rs:445`), and no key was pressed.
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(500, 0);
        }
        tree.request_frame();
        tree.layout(viewport);

        let root_node_id = widget_id_to_node_id(id);
        // What a platform adapter is handed: the container's
        // `active_descendant`, resolved inside the same `TreeUpdate` that
        // published it.
        let nominated = |tree: &mut WidgetTree| -> Option<(Role, Option<usize>)> {
            let update = tree.sync_accessibility();
            let active = update
                .nodes
                .iter()
                .find(|(nid, _)| *nid == root_node_id)
                .and_then(|(_, n)| n.active_descendant())?;
            update
                .nodes
                .iter()
                .find(|(nid, _)| *nid == active)
                .map(|(_, n)| (n.role(), n.row_index()))
        };

        assert_eq!(
            nominated(&mut tree),
            None,
            "row 500 starts far outside the realized window, which is the case \
             this is about"
        );

        tree.focus(id);
        tree.layout(viewport);

        assert_eq!(
            nominated(&mut tree),
            // `row_index` is stored zero-based and the adapters add the 1 back,
            // so the cursor's row 500 reads as 501 with the header counted.
            Some((Role::Cell, Some(501))),
            "taking focus has to bring the cursor's row into the realized \
             window, or nothing in the tree can be told about it"
        );
    }

    /// With no cell navigated to yet, the selected row is the one revealed.
    ///
    /// A view restored into a selection has no `focused_cell`, so reading the
    /// cursor only from that signal would leave the selection off-screen and
    /// unrealized: no row node carrying `selected` for AT-SPI to announce
    /// either, and the next arrow press stepping from a row nobody saw. The
    /// fallback order is the keyboard handler's own
    /// (`table_view/keyboard.rs:134-139`).
    #[test]
    fn taking_focus_reveals_the_selected_row_when_no_cell_has_been_navigated_to() {
        use teksilo_data::{SelectionMode, SelectionModel};

        let proxy = SortFilterTreeModel::new(wide_tree(1000));
        let sel = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .row_height(20.0)
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(sel.clone()),
        );
        let viewport = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(viewport);

        sel.select(500);
        tree.request_frame();
        tree.layout(viewport);

        let selected_rows = |tree: &mut WidgetTree| -> Vec<usize> {
            tree.sync_accessibility()
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == Role::Row && n.is_selected() == Some(true))
                .filter_map(|(_, n)| n.row_index())
                .collect()
        };

        assert!(
            selected_rows(&mut tree).is_empty(),
            "row 500 starts far outside the realized window, which is the case \
             this is about"
        );

        tree.focus(id);
        tree.layout(viewport);

        assert_eq!(
            selected_rows(&mut tree),
            vec![501],
            "taking focus has to realize the selected row, or there is no node \
             carrying `selected` for a screen reader to find"
        );
    }

    /// The index revealed is a **visible** row, not a position in the
    /// unflattened tree.
    ///
    /// The two differ by every descendant hidden above the cursor, so a tree
    /// with collapsed branches is where a confusion between them shows. Here
    /// the first twenty roots keep their nine children each and show none of
    /// them, which slides every row below 180 places up the flat order while
    /// nothing about the tree itself moves. The gap is deliberately far wider
    /// than the realized window: a reveal aimed at the unflattened position
    /// would leave the cursor's row with no widget at all, which is the state
    /// this whole change is about.
    ///
    /// The reveal is fed `focused_cell`, whose row the keyboard handler clamps
    /// against `TreeNavigator::row_count()` = `TreeSource::visible_count()`
    /// (`tree_table_view.rs:129-131`), and it spends that index on
    /// `RowMetrics`, which `place_children` sizes from the same
    /// `visible_count()`. Both ends are therefore the flat order this test
    /// reads through `SortFilterTreeModel::visible_node_id`.
    ///
    /// Checked by identity rather than by arithmetic: the nominated cell has
    /// to be the one holding the node the cursor was put on.
    #[test]
    fn the_revealed_row_is_a_visible_index_not_a_position_in_the_unflattened_tree() {
        use teksilo_core::accessibility::node_id_to_widget_id;

        let model = TreeModel::new();
        let mut to_collapse = Vec::new();
        let mut needle = None;
        for r in 0..40usize {
            let root = model.insert_root(r, "root");
            if r < 20 {
                to_collapse.push(root);
            }
            for c in 0..9usize {
                let label = if r == 30 && c == 4 { "needle" } else { "leaf" };
                let child = model.insert_child(root, c, label);
                if r == 30 && c == 4 {
                    needle = Some(child);
                }
            }
        }
        let needle = needle.expect("the needle inserted");

        let proxy = SortFilterTreeModel::new(model);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        let viewport = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(viewport);
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .expand_all();
        }
        tree.layout(viewport);

        let flat_of = |node| {
            (0..proxy.visible_count())
                .find(|&i| proxy.visible_node_id(i) == Some(node))
                .expect("the node is visible")
        };
        let flat_expanded = flat_of(needle);

        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            for root in to_collapse {
                tt.collapse(root);
            }
        }
        tree.layout(viewport);

        let flat = flat_of(needle);
        assert_eq!(
            flat,
            flat_expanded - 180,
            "collapsing the first twenty roots has to move the needle up the \
             flat order without moving it in the tree, or this test proves \
             nothing"
        );

        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(flat, 0);
        }
        tree.request_frame();
        tree.layout(viewport);
        tree.focus(id);
        tree.layout(viewport);

        let root_node_id = widget_id_to_node_id(id);
        let update = tree.sync_accessibility();
        let active = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == root_node_id)
            .and_then(|(_, n)| n.active_descendant())
            .expect(
                "taking focus has to bring the cursor's row into the realized \
                 window, or nothing in the tree can be told about it",
            );
        let cell = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == active)
            .map(|(_, n)| n)
            .expect("active_descendant must reference a node in the TreeUpdate");
        assert_eq!(cell.row_index(), Some(flat + 1));

        // And it is the needle's own cell: walk the nominated cell's widget
        // subtree for the label the delegate rendered.
        let mut q = vec![node_id_to_widget_id(active)];
        let mut names = Vec::new();
        while let Some(w) = q.pop() {
            if let Some(name) = tree.accessibility_node(w).name() {
                names.push(name.to_string());
            }
            for c in tree.children(w) {
                q.push(c);
            }
        }
        assert!(
            names.iter().any(|n| n == "needle"),
            "the revealed row must be the node the cursor was put on, got {names:?}"
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
        use std::cell::RefCell;
        use std::ops::Range;
        use teksilo_data::{FlatEntry, RowState};

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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::event::{Key, Modifiers};
        let proxy = SortFilterTreeModel::new(wide_tree(100));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        tree.press_key(Key::Home, Modifiers::COMMAND);
        tree.layout(proposal);
        assert_eq!(read_focus(&tree), Some((0, 0)));
        assert_eq!(read_scroll(&tree), 0.0, "Ctrl+Home scrolls to top");
    }

    #[test]
    fn type_ahead_jumps_to_matching_row() {
        use teksilo_core::event::{Key, Modifiers};
        let model = TreeModel::new();
        model.insert_root(0, "Apple");
        model.insert_root(1, "Banana");
        model.insert_root(2, "Cherry");
        model.insert_root(3, "Cranberry");
        let proxy = SortFilterTreeModel::new(model);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_core::widget_builder::WidgetBuilder;

        let proxy = SortFilterTreeModel::new(wide_tree(5));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    fn rows_report_sibling_position_and_size_among_siblings() {
        // docs (root 1/2) -> readme (child 1/2), guide (child 2/2)
        // src  (root 2/2) -> main.rs (child 1/1)
        //
        // `TreeView`'s `TreeItemWrapper` already announces
        // position_in_set/size_of_set (`list_item_a11y.rs`);
        // `TreeTableView` never wired `TreeSource::sibling_pos` into its own
        // row wrapper (`TreeRowA11y`) despite the data being one call away.
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(400.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .expand_all();
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(400.0),
        });
        assert_eq!(proxy.visible_count(), 5);

        // Collect all Role::Row body widgets (the header shares the role but
        // is excluded below by having no accesskit node y inside the body
        // band — simplest: sort every Role::Row by y and drop the topmost
        // one, which is always the header).
        let mut rows: Vec<WidgetId> = Vec::new();
        let mut q = vec![id];
        while let Some(n) = q.pop() {
            if tree.accessibility_node(n).role() == Role::Row {
                rows.push(n);
            }
            for c in tree.children(n) {
                q.push(c);
            }
        }
        rows.sort_by(|a, b| tree.bounds(*a).y.partial_cmp(&tree.bounds(*b).y).unwrap());
        assert_eq!(rows.len(), 6, "header + five body rows");
        let body_rows = &rows[1..];

        // `position_in_set`/`size_of_set` aren't on the summarized
        // `AccessibilityInfo` — read them off the real accesskit node via a
        // fresh `TreeUpdate`, mirroring `docking::tests::find_a11y_node`.
        let update = tree.sync_accessibility();
        let find = |wid: WidgetId| -> &teksilo_core::accesskit::Node {
            let nid = widget_id_to_node_id(wid);
            update
                .nodes
                .iter()
                .find(|(n, _)| *n == nid)
                .map(|(_, n)| n)
                .expect("row must be in the a11y tree")
        };
        let positions: Vec<usize> = body_rows
            .iter()
            .map(|&r| find(r).position_in_set().expect("position_in_set"))
            .collect();
        // The row passes ARIA's 1-based sibling position; AccessKit stores it
        // zero-based, and the Windows and AT-SPI adapters add the 1 back — so
        // "the first of two siblings" is 0 on the node and "1" to the user.
        assert_eq!(
            positions,
            vec![0, 0, 1, 1, 0],
            "docs(1st) readme(1st) guide(2nd) src(2nd) main.rs(1st)"
        );
        // No sibling *count*, deliberately. AccessKit resolves a set size by
        // walking up from an item, so the only value a flattened tree could
        // publish is one shared by every row at every depth — which is not what
        // "of 2 siblings" means. See `TreeRowA11y::accessibility`.
        for &r in body_rows {
            assert_eq!(
                find(r).size_of_set(),
                None,
                "a per-sibling count is unrepresentable and must not be faked"
            );
        }
    }

    #[test]
    fn row_count_in_a11y_includes_header() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    fn row_bounds(tree: &WidgetTree, root: WidgetId) -> Vec<teksilo_canvas::Rect> {
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
        use teksilo_core::environment::LayoutDirection;
        use teksilo_core::event::{Key, Modifiers};

        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_core::environment::LayoutDirection;
        // 50 roots → vertical scrollbar present. Under RTL it sits on the
        // physical left, so the body band (and its rows) shift right by
        // SCROLLBAR_THICKNESS.
        let proxy = SortFilterTreeModel::new(wide_tree(50));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree2 = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
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
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
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
        // The reason `TreeBodyPane` exists — see `common::thumb_drag_test`'s
        // module docs for the invariant, and for why every virtualized view
        // asserts it through the same driver.
        let model = TreeModel::new();
        for i in 0..500 {
            model.insert_root(i, "root");
        }
        let proxy = SortFilterTreeModel::new(model);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let table = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        crate::common::thumb_drag_test::assert_body_survives_thumb_drag(
            &mut tree,
            table,
            400.0,
            200.0,
            cp::HEADER_HEIGHT,
            "TreeTableView",
            |t| {
                let mut n = 0;
                let mut walker = vec![table];
                while let Some(id) = walker.pop() {
                    if t.accessibility_node(id).role() == Role::Row {
                        let b = t.bounds(id);
                        if b.y >= 0.0 && b.y < 200.0 {
                            n += 1;
                        }
                    }
                    for c in t.children(id) {
                        walker.push(c);
                    }
                }
                n
            },
        );
    }

    #[test]
    fn exact_row_height_fn_positions_tree_rows() {
        let heights = [60.0_f32, 20.0, 40.0];
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
            ) -> teksilo_core::widget::LayoutResponse {
                Size::new(self.0, self.1).into()
            }
        }
        let col = Column::<&str>::new("name", lit!("Name"), |_row, _: &CellContext| {
            Box::new(FixedLeaf(50.0, 30.0))
        })
        .width(ColumnWidth::Flex(1.0));
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
    fn drag(tree: &mut WidgetTree, from: teksilo_canvas::Point, to: teksilo_canvas::Point) {
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
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
        use teksilo_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.collapse_all(); // roots only: docs@0, src@1
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.expand_all(); // docs@0, readme@1, guide@2, src@3, main.rs@4
        let docs = proxy.tree().root(0);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
        use teksilo_canvas::Point;
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.collapse_all();
        let docs = proxy.tree().root(0);
        let src = proxy.tree().root(1);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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
            .set_sort(Some("name"), teksilo_data::SortDirection::Ascending);
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
        use teksilo_data::{KeyedSelectionModel, SelectionMode};
        let proxy = SortFilterTreeModel::new(sample_tree());
        proxy.expand_all();
        let docs = proxy.tree().root(0);
        let readme = proxy.tree().children(docs)[0];
        let keyed = KeyedSelectionModel::<NodeId>::new(SelectionMode::Multi);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
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

    // ── Horizontal scroll ───────────────────────────────────────────────
    //
    // TreeTableView reuses TableView's `body::BodyRow` / `header::HeaderRow`
    // / `layout::` pane machinery wholesale, so these mirror the TableView
    // suite (`table_view::tests`) at reduced breadth: enough to confirm the
    // shared plumbing threads through this widget's own `build()` /
    // `place_children()` / `paint()` / `on_scroll` correctly, not to
    // re-verify the pane math itself (already unit-tested in `layout.rs`
    // and exercised end-to-end by TableView's suite).

    /// Expand any AT-transparent id (the pane-band wrapper `RowBand`
    /// inserts under column pinning — see `table_view::body`'s module
    /// docs — never calls `set_role`, so it reads back as the
    /// `AccessNodeBuilder` default `Role::Unknown`) into its own children,
    /// recursively.
    fn tt_flatten_through_bands(tree: &WidgetTree, ids: Vec<WidgetId>) -> Vec<WidgetId> {
        let mut out = Vec::new();
        for id in ids {
            if matches!(
                tree.accessibility_node(id).role(),
                Role::GenericContainer | Role::Unknown
            ) {
                out.extend(tt_flatten_through_bands(tree, tree.children(id)));
            } else {
                out.push(id);
            }
        }
        out
    }

    /// The first BODY `Role::Row` (band-flattened children include a
    /// `Role::Cell`) — distinguishes it from the header, which shares
    /// `Role::Row` but has only `Role::ColumnHeader` children.
    fn tt_first_body_row_id(tree: &WidgetTree, root: WidgetId) -> WidgetId {
        let mut walker = vec![root];
        while let Some(id) = walker.pop() {
            if tree.accessibility_node(id).role() == Role::Row {
                let flat = tt_flatten_through_bands(tree, tree.children(id));
                if flat
                    .iter()
                    .any(|&c| tree.accessibility_node(c).role() == Role::Cell)
                {
                    return id;
                }
            }
            for c in tree.children(id) {
                walker.push(c);
            }
        }
        panic!("no body Role::Row found");
    }

    fn tt_header_row_id(tree: &WidgetTree, root: WidgetId) -> WidgetId {
        let mut walker = vec![root];
        while let Some(id) = walker.pop() {
            if tree.accessibility_node(id).role() == Role::Row {
                let flat = tt_flatten_through_bands(tree, tree.children(id));
                if !flat.is_empty()
                    && flat
                        .iter()
                        .all(|&c| tree.accessibility_node(c).role() == Role::ColumnHeader)
                {
                    return id;
                }
            }
            for c in tree.children(id) {
                walker.push(c);
            }
        }
        panic!("no header Role::Row found");
    }

    fn tt_body_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
        tt_flatten_through_bands(tree, tree.children(tt_first_body_row_id(tree, root)))
    }

    fn tt_header_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
        tt_flatten_through_bands(tree, tree.children(tt_header_row_id(tree, root)))
    }

    /// Leading `lead` (60px, pinned) + unpinned `mid` (`middle_w` px) +
    /// Trailing `trail` (60px, pinned), over the default `sample_tree()`
    /// (roots collapsed — 2 visible rows).
    fn build_tt_pinned_scroll_table(middle_w: f32, table_w: f32) -> (WidgetTree, WidgetId) {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(
                    Column::<&'static str>::new("lead", lit!("Lead"), |row, _: &CellContext| {
                        Box::new(crate::primitives::TextWidget::new(lit!(*row)))
                    })
                    .width(ColumnWidth::Fixed(60.0))
                    .pinned(PinnedSide::Leading),
                )
                .add_column(
                    Column::<&'static str>::new("mid", lit!("Mid"), |row, _: &CellContext| {
                        Box::new(crate::primitives::TextWidget::new(lit!(*row)))
                    })
                    .width(ColumnWidth::Fixed(middle_w)),
                )
                .add_column(
                    Column::<&'static str>::new("trail", lit!("Trail"), |_row, _: &CellContext| {
                        Box::new(crate::primitives::TextWidget::new(lit!("x")))
                    })
                    .width(ColumnWidth::Fixed(60.0))
                    .pinned(PinnedSide::Trailing),
                )
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(table_w),
            height: Some(200.0),
        });
        (tree, id)
    }

    /// `n` unpinned Fixed columns of `col_w` px each.
    fn build_tt_wide_unpinned_table(col_w: f32, n: usize, table_w: f32) -> (WidgetTree, WidgetId) {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let mut tv = TreeTableView::from_projection(proxy);
        for i in 0..n {
            let col_id = format!("c{i}");
            tv = tv.add_column(
                Column::<&'static str>::new(
                    col_id.clone(),
                    lit!(col_id.clone()),
                    |row, _: &CellContext| Box::new(crate::primitives::TextWidget::new(lit!(*row))),
                )
                .width(ColumnWidth::Fixed(col_w)),
            );
        }
        let id = tree.add(tv.row_height(20.0));
        tree.layout(SizeProposal {
            width: Some(table_w),
            height: Some(200.0),
        });
        (tree, id)
    }

    fn tt_scroll_x(tree: &WidgetTree, id: WidgetId) -> f32 {
        tree.widget_as_any(id)
            .unwrap()
            .downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .scroll_x_signal()
            .get()
    }

    fn tt_max_scroll_x(tree: &WidgetTree, id: WidgetId) -> f32 {
        tree.widget_as_any(id)
            .unwrap()
            .downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .max_scroll_x_signal()
            .get()
    }

    fn tt_set_scroll_x(tree: &WidgetTree, id: WidgetId, x: f32) {
        tree.widget_as_any(id)
            .unwrap()
            .downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .scroll_x_signal()
            .set(x);
    }

    #[test]
    fn tt_scroll_x_clamps_after_the_pane_widens() {
        let (mut tree, id) = build_tt_wide_unpinned_table(200.0, 3, 300.0);
        let max = tt_max_scroll_x(&tree, id);
        assert!(max > 0.0, "columns must overflow the narrow table");
        tt_set_scroll_x(&tree, id, max);
        assert_eq!(tt_scroll_x(&tree, id), max);

        tree.layout(SizeProposal {
            width: Some(700.0),
            height: Some(200.0),
        });
        assert_eq!(tt_max_scroll_x(&tree, id), 0.0, "content now fits");
        assert_eq!(
            tt_scroll_x(&tree, id),
            0.0,
            "scroll_x must clamp down with the new (smaller) max_scroll_x"
        );
    }

    #[test]
    fn tt_pinned_columns_keep_their_bands_under_scroll() {
        let (mut tree, id) = build_tt_pinned_scroll_table(400.0, 200.0);

        let cells0 = tt_body_row_cells(&tree, id);
        assert_eq!(cells0.len(), 3, "lead, mid, trail");
        let lead_x0 = tree.bounds(cells0[0]).x;
        let mid_x0 = tree.bounds(cells0[1]).x;
        let trail_x0 = tree.bounds(cells0[2]).x;

        // `tt_first_body_row_id` returns the `TreeRowA11y` wrapper (the
        // `Role::Row` carrier); its sole child is the `.a11y_hidden()`
        // `BodyRow`, one level further in, whose own children are the
        // pane bands.
        let tree_row_a11y = tt_first_body_row_id(&tree, id);
        let body_row = tree.children(tree_row_a11y)[0];
        let raw_bands = tree.children(body_row);
        assert_eq!(raw_bands.len(), 3, "leading + middle + trailing bands");
        assert!(!tree.widget_clips_children(raw_bands[0]));
        assert!(
            tree.widget_clips_children(raw_bands[1]),
            "the Middle band must clip"
        );
        assert!(!tree.widget_clips_children(raw_bands[2]));

        let max = tt_max_scroll_x(&tree, id);
        assert!(max > 0.0);
        tt_set_scroll_x(&tree, id, 50.0_f32.min(max));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: Some(200.0),
        });

        let cells1 = tt_body_row_cells(&tree, id);
        assert_eq!(tree.bounds(cells1[0]).x, lead_x0, "Leading never moves");
        assert_eq!(tree.bounds(cells1[2]).x, trail_x0, "Trailing never moves");
        let mid_x1 = tree.bounds(cells1[1]).x;
        assert!(
            (mid_x1 - (mid_x0 - 50.0)).abs() < 0.5,
            "the Middle column shifts left by exactly scroll_x: got {mid_x1}, want ~{}",
            mid_x0 - 50.0
        );
    }

    #[test]
    fn tt_header_and_body_x_offsets_agree_under_scroll() {
        let (mut tree, id) = build_tt_pinned_scroll_table(400.0, 200.0);
        tt_set_scroll_x(&tree, id, 37.0);
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: Some(200.0),
        });

        let header_cells = tt_header_row_cells(&tree, id);
        let body_cells = tt_body_row_cells(&tree, id);
        assert_eq!(header_cells.len(), body_cells.len());
        for (i, (&h, &b)) in header_cells.iter().zip(body_cells.iter()).enumerate() {
            let hx = tree.bounds(h).x;
            let bx = tree.bounds(b).x;
            assert!(
                (hx - bx).abs() < 0.01,
                "column {i}: header x {hx} must equal body x {bx}"
            );
        }
    }

    #[test]
    fn tt_shift_wheel_scrolls_horizontally() {
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
        let (mut tree, id) = build_tt_wide_unpinned_table(200.0, 4, 300.0);
        tree.pointer_move(Point::new(50.0, 60.0));
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: ScrollDelta::Lines { x: 0.0, y: 3.0 },
            modifiers: Modifiers::SHIFT,
        });
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: Some(200.0),
        });
        assert!(
            tt_scroll_x(&tree, id) > 0.0,
            "Shift+wheel must remap a vertical-only wheel to horizontal scroll"
        );
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.scroll_y_signal().get(),
            0.0,
            "Shift+wheel must not also scroll vertically"
        );
    }

    #[test]
    fn tt_ensure_col_visible_follows_focus_in_both_directions() {
        use teksilo_core::event::{Key, Modifiers};
        let (mut tree, id) = build_tt_wide_unpinned_table(150.0, 5, 300.0);
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(0, 0);
        }
        assert_eq!(tt_scroll_x(&tree, id), 0.0);

        tree.press_key(Key::End, Modifiers::NONE);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(tt.focused_cell_signal().get(), Some((0, 4)));
        }
        assert!(
            tt_scroll_x(&tree, id) > 0.0,
            "ensure-column-visible must scroll right to reveal column 4"
        );

        tree.press_key(Key::Home, Modifiers::NONE);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(tt.focused_cell_signal().get(), Some((0, 0)));
        }
        assert_eq!(
            tt_scroll_x(&tree, id),
            0.0,
            "ensure-column-visible must scroll left back to 0 for column 0"
        );
    }

    // ── Column header drag-to-reorder ───────────────────────────────────
    //
    // `HeaderCell` escalates a header press into a `ColumnReorderDragData`
    // drag past a 5px threshold (`table_view::header`); the drop-target
    // half — hover feedback, insertion-slot math, pane classification,
    // `column_order_signal`/`column_pinning_signal` writes — is
    // `header::attach_header_reorder_handlers`, shared verbatim with
    // `TableView` (moved there by this commit, not duplicated). These
    // tests drive the mechanism end-to-end through real pointer events
    // (`drag`, defined above for row reorder — the header strip is just
    // another drop target) rather than the imperative
    // `set_column_order`/`set_column_pinning` setters already covered
    // above, and additionally confirm the tree column carries no special
    // case through the shared path: its indent/twist gutter and the
    // ArrowLeft/Right expand-collapse binding both re-resolve from
    // `display_indices` on every rebuild, so they follow it to wherever a
    // drag lands it — including into a pinned pane, same as any other
    // column.

    /// Column `id` at a distinct `width`, so a header/body cell's bounds
    /// alone identify which column it is after a reorder.
    fn reorder_col(id: &'static str, width: f32) -> Column<&'static str> {
        Column::<&'static str>::new(id, lit!(id), |row, _: &CellContext| {
            Box::new(crate::primitives::TextWidget::new(lit!(*row)))
        })
        .width(ColumnWidth::Fixed(width))
    }

    /// Four unpinned columns "a" (60px, the default tree column since it's
    /// declared first), "b" (70px), "c" (80px), "d" (90px) — over
    /// `sample_tree()` (2 visible roots, "docs" has children).
    fn build_tt_reorder_table() -> (WidgetTree, WidgetId, SortFilterTreeModel<&'static str>) {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(reorder_col("a", 60.0))
                .add_column(reorder_col("b", 70.0))
                .add_column(reorder_col("c", 80.0))
                .add_column(reorder_col("d", 90.0))
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        (tree, id, proxy)
    }

    /// Whether `id` or any descendant is a `TwistArrow` — the indent/twist
    /// gutter `TreeBodyPane` wraps around whichever cell is currently the
    /// tree column. Identified by `widget_type_name` (a plain `type_name`
    /// readout, no opt-in needed) rather than `widget_as_any` downcast,
    /// since `TwistArrow` — a layout-only primitive nobody has needed to
    /// downcast before — doesn't override `Widget::as_any`.
    fn tt_subtree_has_twist_arrow(tree: &WidgetTree, id: WidgetId) -> bool {
        if tree.widget_type_name(id) == Some("teksilo_widgets::primitives::twist_arrow::TwistArrow")
        {
            return true;
        }
        tree.children(id)
            .into_iter()
            .any(|c| tt_subtree_has_twist_arrow(tree, c))
    }

    #[test]
    fn header_drag_reorders_column_before_an_earlier_sibling() {
        // Drag "d" (display 3) to a slot strictly inside the unpinned band
        // (before "b") — a plain reorder with no pane-boundary side effect.
        let (mut tree, id, _proxy) = build_tt_reorder_table();
        let header = tt_header_row_cells(&tree, id);
        assert_eq!(header.len(), 4);
        let from = tree.bounds(header[3]).center(); // "d"
        let to = teksilo_canvas::Point::new(65.0, from.y); // inside "b"'s leading half
        drag(&mut tree, from, to);

        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(
                tt.column_order_signal().get(),
                vec![
                    "a".to_string(),
                    "d".to_string(),
                    "b".to_string(),
                    "c".to_string()
                ],
                "dropping \"d\" before \"b\" must write [a, d, b, c]"
            );
            assert_eq!(
                tt.column_pinning_signal().get().get("d"),
                None,
                "a mid-band drop must not pin the moved column"
            );
        }

        // display_indices re-derive: a fresh layout must actually reflow
        // the header cells into the new order (Fixed widths, so an exact
        // width sequence identifies each column unambiguously).
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        let after = tt_header_row_cells(&tree, id);
        let widths: Vec<f32> = after.iter().map(|&c| tree.bounds(c).width).collect();
        assert!(
            widths
                .iter()
                .zip([60.0, 90.0, 70.0, 80.0])
                .all(|(&w, want)| (w - want).abs() < 0.5),
            "header cells must reflow to widths [60, 90, 70, 80], got {widths:?}"
        );
    }

    #[test]
    fn header_drag_to_the_leading_edge_pins_the_dropped_column() {
        // The pane-boundary classification in `attach_header_reorder_handlers`
        // (`insertion_display_idx <= panes.leading_count`) is the exact same
        // code TableView's header shares — dropping at the very leading
        // edge pins the dragged column Leading, growing the leading pane.
        let (mut tree, id, _proxy) = build_tt_reorder_table();
        let header = tt_header_row_cells(&tree, id);
        let from = tree.bounds(header[3]).center(); // "d"
        let to = teksilo_canvas::Point::new(5.0, from.y); // before "a"
        drag(&mut tree, from, to);

        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.column_order_signal().get(),
            vec![
                "d".to_string(),
                "a".to_string(),
                "b".to_string(),
                "c".to_string()
            ],
        );
        assert_eq!(
            tt.column_pinning_signal().get().get("d").copied(),
            Some(PinnedSide::Leading),
            "dropping at the leading edge must pin the column, same as TableView"
        );
    }

    #[test]
    fn header_drag_reorder_remaps_focused_and_editing_cell_to_follow_their_columns() {
        // `focused_cell` / `editing_cell` store `(row, display_position)` —
        // `imperative::remap_cell_state` (already exercised by the
        // `column_pinning_remaps_*` tests above via the imperative setters)
        // must fire the same way when the reorder arrives through a real
        // header drag.
        let (mut tree, id, _proxy) = build_tt_reorder_table();
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.set_focused_cell(0, 1); // "b"
            tt.begin_edit(0, "d"); // "d"
        }

        let header = tt_header_row_cells(&tree, id);
        let from = tree.bounds(header[3]).center(); // "d"
        let to = teksilo_canvas::Point::new(65.0, from.y); // before "b" — see the plain-reorder test above
        drag(&mut tree, from, to);
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.column_order_signal().get(),
            vec![
                "a".to_string(),
                "d".to_string(),
                "b".to_string(),
                "c".to_string()
            ],
        );
        assert_eq!(
            tt.focused_cell_signal().get(),
            Some((0, 2)),
            "focus must follow \"b\" to its new display position"
        );
        assert_eq!(
            tt.editing_cell_signal().get(),
            Some((0, 1)),
            "the open editor must follow \"d\" to its new display position"
        );
    }

    #[test]
    fn header_drag_moves_the_tree_column_and_twist_follows() {
        // The tree column carries no special case anywhere in the reorder
        // path: `is_tree_column` in `TreeBodyPane::build` is a plain
        // `display_pos == tree_display_pos` comparison, and
        // `tree_display_pos` is re-resolved from `display_indices` on
        // every rebuild (see the comment on `TreeTableView::build`'s
        // `key_cfg.tree_column_display_pos`). So dragging "a" (the tree
        // column) to a later, unpinned slot must carry the indent/twist
        // gutter with it, and ArrowLeft/Right must stay bound to it there.
        let (mut tree, id, proxy) = build_tt_reorder_table();
        let header = tt_header_row_cells(&tree, id);
        let from = tree.bounds(header[0]).center(); // "a", the tree column
        let to = teksilo_canvas::Point::new(220.0, from.y); // lands "a" between "c" and "d"
        drag(&mut tree, from, to);

        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            assert_eq!(
                tt.column_order_signal().get(),
                vec![
                    "b".to_string(),
                    "c".to_string(),
                    "a".to_string(),
                    "d".to_string()
                ],
            );
            assert_eq!(
                tt.column_pinning_signal().get().get("a"),
                None,
                "a mid-band drop must not pin the tree column either"
            );
        }
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let body = tt_body_row_cells(&tree, id);
        assert_eq!(body.len(), 4);
        assert!(
            !tt_subtree_has_twist_arrow(&tree, body[0]),
            "\"b\" is no longer the tree column"
        );
        assert!(
            !tt_subtree_has_twist_arrow(&tree, body[1]),
            "\"c\" is no longer the tree column"
        );
        assert!(
            tt_subtree_has_twist_arrow(&tree, body[2]),
            "the twist must follow \"a\" to its new display position"
        );
        assert!(
            !tt_subtree_has_twist_arrow(&tree, body[3]),
            "\"d\" is not the tree column"
        );

        // ArrowLeft/Right stay bound to the tree column at its new slot.
        use teksilo_core::event::{Key, Modifiers};
        tree.focus(id);
        {
            let any = tree.widget_as_any(id).unwrap();
            any.downcast_ref::<TreeTableView<&'static str>>()
                .unwrap()
                .set_focused_cell(0, 2); // row 0 ("docs"), tree column's new slot
        }
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            proxy.visible_count(),
            4,
            "ArrowRight on the relocated tree column must expand \"docs\""
        );
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(proxy.visible_count(), 2, "and ArrowLeft collapses it again");
    }

    #[test]
    fn header_drag_from_a_different_table_is_rejected() {
        // Each TreeTableView mints its own `table_id`; a drop whose
        // `ColumnReorderDragData::source_table_id` doesn't match the
        // hovered header's own id must be a no-op — otherwise dragging a
        // column between two independent tree-tables on screen would
        // silently reorder the wrong one.
        use crate::primitives::{FixedSize, HStack};
        let proxy1 = SortFilterTreeModel::new(sample_tree());
        let proxy2 = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());

        let tt1 = TreeTableView::from_projection(proxy1)
            .add_column(reorder_col("x", 100.0))
            .add_column(reorder_col("y", 100.0))
            .row_height(20.0);
        let order1 = tt1.column_order_signal().clone();
        let id1 = tree.add(tt1);
        let tt2 = TreeTableView::from_projection(proxy2)
            .add_column(reorder_col("x", 100.0))
            .add_column(reorder_col("y", 100.0))
            .row_height(20.0);
        let order2 = tt2.column_order_signal().clone();
        let id2 = tree.add(tt2);

        let fixed1 = tree.add(FixedSize::new().width(200.0).height(150.0).child_id(id1));
        let fixed2 = tree.add(FixedSize::new().width(200.0).height(150.0).child_id(id2));
        tree.add(HStack::new().add_child(fixed1).add_child(fixed2));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(150.0),
        });

        // tt1 occupies window x[0, 200), tt2 x[200, 400) — drag tt1's
        // leading header cell into tt2's header strip.
        let from = tree.bounds(tt_header_row_cells(&tree, id1)[0]).center();
        let to = teksilo_canvas::Point::new(250.0, from.y); // inside tt2's "x" cell
        drag(&mut tree, from, to);

        assert!(order1.get().is_empty(), "tt1's own order must be untouched");
        assert!(
            order2.get().is_empty(),
            "tt2 must reject a drop whose payload names a different table_id"
        );
    }

    #[test]
    fn header_drag_released_over_the_body_does_not_trigger_foreign_row_drop() {
        // Regression: `on_foreign_drop` fires for "any payload NOT
        // recognized as this view's own row drag" — without the
        // `ColumnReorderDragData` bail at the top of the row-level
        // `on_drag_hover`/`on_drop` (added alongside wiring up header
        // reorder — TreeTableView never carried a `ColumnReorderDragData`
        // payload before), a header drag released past the header strip's
        // own y-range would fall through into this hatch, or into a
        // row-insertion-line hover affordance, for a drag the header is
        // already handling.
        use std::cell::Cell;
        let foreign_fired = Rc::new(Cell::new(false));
        let flag = foreign_fired.clone();
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .on_foreign_drop(move |_payload, _node, _pos, _ctx| {
                    flag.set(true);
                    true
                })
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let header = tt_header_row_cells(&tree, id);
        let from = tree.bounds(header[0]).center();
        let to = teksilo_canvas::Point::new(from.x, cp::HEADER_HEIGHT + 10.0); // below the header
        drag(&mut tree, from, to);

        assert!(
            !foreign_fired.get(),
            "a column-reorder drag must never reach on_foreign_drop"
        );
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert!(
            tt.column_order_signal().get().is_empty(),
            "no header drop occurred either — the release point was outside the header strip"
        );
    }

    #[test]
    fn header_drag_insertion_is_scroll_aware() {
        // The insertion-slot math (`layout::insertion_slot_at_x`) is unit
        // tested directly for scroll-awareness; this proves the SHARED
        // drop-target wiring actually reaches it under a nonzero
        // `scroll_x`, for TreeTableView same as TableView.
        let (mut tree, id) = build_tt_wide_unpinned_table(100.0, 4, 200.0);
        let max = tt_max_scroll_x(&tree, id);
        assert!(max > 0.0, "4×100px columns must overflow a 200px viewport");
        tt_set_scroll_x(&tree, id, max); // scrolled fully right
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: Some(200.0),
        });

        // At full scroll the 200px viewport shows logical [200, 400): "c2"
        // fills local [0, 100), "c3" fills local [100, 200). Dropping "c3"
        // at local x=10 (deep in "c2"'s own zone) must resolve against the
        // scrolled position and land before "c2" — an unscrolled read of
        // the same raw x=10 would instead land before "c0".
        let header = tt_header_row_cells(&tree, id);
        let from = tree.bounds(header[3]).center(); // "c3"
        let to = teksilo_canvas::Point::new(10.0, from.y);
        drag(&mut tree, from, to);

        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.column_order_signal().get(),
            vec![
                "c0".to_string(),
                "c1".to_string(),
                "c3".to_string(),
                "c2".to_string()
            ],
            "\"c3\" must land before \"c2\" (scroll-aware), not before \"c0\""
        );
    }

    // ── Column resize grip (parity with TableView) ─────────────────────────
    //
    // The grip machinery lives in the shared `table_view::header::HeaderCell`,
    // but `TreeTableView` fills its own `HeaderCellSpec` and owns its own
    // `resize_state` / `resize_target` / `resize_preview_x` handles — so the
    // wiring is asserted here too rather than assumed from the TableView side.

    fn tt_resize_table() -> (WidgetTree, WidgetId) {
        // `name` Flex(1) then `size` Fixed(60) at a 400 px viewport: `name`
        // spans [0, 340], `size` spans [340, 400], divider at x = 340.
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy)
                .add_column(name_col())
                .add_column(size_col())
                .row_height(20.0)
                .show_internal_scrollbars(false),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        (tree, id)
    }

    fn tt_overrides(tree: &WidgetTree, id: WidgetId) -> std::collections::HashMap<String, f32> {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .column_widths_signal()
            .get()
    }

    #[test]
    fn tt_grip_reaches_into_the_next_column() {
        use teksilo_canvas::Point;
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        let (mut tree, id) = tt_resize_table();
        let y = cp::HEADER_HEIGHT * 0.5;
        // One pixel PAST the name/size divider, i.e. inside `size`.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(341.0, y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(311.0, y),
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(311.0, y),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        let w = tt_overrides(&tree, id);
        assert!(
            (w.get("name").copied().unwrap_or(0.0) - 310.0).abs() < 0.5,
            "dragging the divider left from `size` must shrink `name` from 340 \
             to 310; got {w:?}"
        );
    }

    #[test]
    fn tt_header_strip_paints_column_separators() {
        let (mut tree, _id) = tt_resize_table();
        let frame = tree.render();
        let found = frame.decorations.iter().any(|d| {
            let [x, y, w, h] = d.rect;
            (x - 339.0).abs() < 0.6
                && w <= 1.5
                && y.abs() < 0.6
                && (h - cp::HEADER_HEIGHT).abs() < 0.6
        });
        assert!(
            found,
            "expected a header separator at the name/size divider (x≈339); \
             decorations={:?}",
            frame.decorations.iter().map(|d| d.rect).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tree_column_chrome_is_clipped_to_its_column() {
        // The indent gutter and the twist chevron are rigid: a tree column
        // dragged narrower than `depth * indent + twist + gap` cannot shrink
        // to fit, and without a clip the chevron — and the whole label after
        // it — draws on top of the next column. Clipping the chrome wrapper
        // is what lets the grip shrink the tree column all the way to its
        // floor without the row bleeding sideways.
        let (tree, id) = tt_resize_table();
        // Find the first body cell of the tree column (column index 1 in the
        // 1-based AccessKit numbering) and check its chrome wrapper clips.
        let mut walker = vec![id];
        let mut checked = false;
        while let Some(node) = walker.pop() {
            if tree.accessibility_node(node).role() == Role::Cell {
                let kids = tree.children(node);
                if let Some(&wrapper) = kids.first()
                    && tree.widget_clips_children(wrapper)
                {
                    checked = true;
                    break;
                }
            }
            for c in tree.children(node) {
                walker.push(c);
            }
        }
        assert!(
            checked,
            "the tree column's indent + twist wrapper must clip its children"
        );
    }

    /// An editable column whose delegate swaps in a real `TextInput`, so a test
    /// can ask where the keyboard actually went.
    fn editable_name_col() -> Column<&'static str> {
        Column::<&str>::new("name", lit!("Name"), |row, cx: &CellContext| {
            if cx.is_editing {
                Box::new(crate::text_input::TextInput::new(Signal::new(
                    (*row).to_string(),
                )))
            } else {
                Box::new(crate::primitives::TextWidget::new(lit!(*row)))
            }
        })
        .width(ColumnWidth::Flex(1.0))
        .editable(true)
    }

    fn three_row_slice() -> teksilo_data::TreeDataSlice<u64, &'static str> {
        let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
        slice.set_source(move || {
            [(1_u64, "one"), (2, "two"), (3, "three")]
                .into_iter()
                .map(|(k, n)| teksilo_data::TreeRow::new(k, n, 0))
                .collect()
        });
        slice.reload();
        slice
    }

    /// Two primary clicks at one point, close enough together to read as a
    /// double-click. `WidgetTree::click` twice would be two separate taps.
    fn double_click_at(tree: &mut WidgetTree, at: Point) {
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        for _ in 0..2 {
            tree.dispatch_event(WidgetEvent::PointerDown {
                position: at,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
            tree.dispatch_event(WidgetEvent::PointerUp {
                position: at,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
        }
    }

    /// **An open cell editor holds the keyboard.**
    ///
    /// `TableView`'s body pane has always focused into the editing cell; the
    /// line was left behind when the tree table was split out of it, so
    /// `TreeTableView`'s inline editing was reachable only with the mouse. With
    /// focus still on the table, every keystroke went to the table's own key
    /// handler instead: Escape cancelled nothing, Enter activated the row, and
    /// typing ran type-ahead over the value being edited.
    #[test]
    fn opening_a_cell_editor_moves_the_keyboard_into_it() {
        let slice = three_row_slice();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(slice)
                .add_column(editable_name_col())
                .row_height(20.0),
        );
        let proposal = SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        };
        tree.layout(proposal);
        tree.focus(id);

        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.begin_edit(1, "name");
        }
        tree.layout(proposal);

        let focused = tree.focused().expect("something must hold focus");
        assert_ne!(
            focused, id,
            "focus is still on the table, not in the editor"
        );
        let cell = {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.realized_cell(1, 0).expect("the edited cell is realized")
        };
        assert!(
            tree.is_descendant_of(focused, cell),
            "focus must land inside the edited cell, not on {:?}",
            tree.widget_type_name(focused)
        );
    }

    /// ...and it still holds it after the pane rebuilds under it.
    ///
    /// A table rebuilds its rows constantly — selection, filtering, scroll, the
    /// edit signal itself — and each rebuild destroys and re-creates every cell
    /// widget, the open editor included. Restoring focus is therefore not a
    /// one-shot at edit-open: without it the first click on another row would
    /// silently deafen the editor the writer is still typing into. Driven
    /// through a selection change because that is the rebuild a click produces.
    #[test]
    fn an_open_editor_still_holds_the_keyboard_after_the_pane_rebuilds() {
        let slice = three_row_slice();
        let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(slice)
                .selection(selection.clone())
                .add_column(editable_name_col())
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
            tt.begin_edit(1, "name");
        }
        tree.layout(proposal);
        tree.focused().expect("the editor took focus");

        selection.select(2);
        tree.layout(proposal);

        let focused = tree.focused().expect("focus survived the rebuild");
        assert_ne!(focused, id, "the rebuild dropped focus back onto the table");
        let cell = {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
            tt.realized_cell(1, 0)
                .expect("the edited cell is still realized")
        };
        assert!(
            tree.is_descendant_of(focused, cell),
            "focus must still be inside the edited cell, not on {:?}",
            tree.widget_type_name(focused)
        );
    }

    /// **Double-click opens the editor on an editable cell** — one arm of
    /// [`EditTriggers`], and one that had no implementation anywhere.
    /// `F2 | ANY_KEY | DOUBLE_CLICK` is the default set, so every table has
    /// been promising this; only `keyboard.rs`'s F2 and type-to-edit ever
    /// reached `on_cell_edit_request`.
    #[test]
    fn a_double_click_on_an_editable_cell_opens_its_editor() {
        let (mut tree, id, seen, _) = click_probe(EditTriggers::DOUBLE_CLICK);
        let cell = realized(&tree, id, 1, 0);
        let at = tree.bounds(cell).center();
        double_click_at(&mut tree, at);

        assert_eq!(
            seen.borrow().as_slice(),
            &[(1, "name".to_string())],
            "a double-click on an editable cell must request its editor"
        );
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(tt.editing_cell_signal().get(), Some((1, 0)));
    }

    /// **One click opens it** when the column asks for `SINGLE_CLICK` — the
    /// case the old closed enum could not express at all.
    #[test]
    fn a_single_click_opens_the_editor_when_the_column_asks_for_it() {
        let (mut tree, id, seen, _) = click_probe(EditTriggers::SINGLE_CLICK);
        let cell = realized(&tree, id, 1, 0);
        tree.click(cell);

        assert_eq!(
            seen.borrow().as_slice(),
            &[(1, "name".to_string())],
            "one click on a SINGLE_CLICK column must request its editor"
        );
    }

    /// ...and a column that asked for neither is not opened by any click.
    /// `NONE` has to mean none, or "read-only in practice" would be
    /// unexpressible for an otherwise editable column.
    #[test]
    fn a_click_opens_nothing_when_the_column_asks_for_no_click_trigger() {
        let (mut tree, id, seen, _) = click_probe(EditTriggers::F2);
        let cell = realized(&tree, id, 1, 0);
        tree.click(cell);
        let at = tree.bounds(cell).center();
        double_click_at(&mut tree, at);

        assert!(
            seen.borrow().is_empty(),
            "an F2-only column opened an editor from a click: {:?}",
            seen.borrow()
        );
    }

    /// A double-click that opens an editor does **not** also activate the row.
    ///
    /// The collision this rules out is opening the item *and* starting to edit
    /// it on one gesture, which is why the click arm could not simply be
    /// switched on. The framework settles it with no guard in the pane: the
    /// cell's gesture arena answers `Handled` to the press, so the bubble never
    /// reaches the row.
    ///
    /// One gesture per tree, and the read-only baseline is the **separate**
    /// test below: a second synthetic double-click in the same tree never
    /// reaches the row's `on_double_tap` at all (the recognizer reads clicks 3
    /// and 4 as a continuing run), so a single test doing both would pass with
    /// the behaviour removed — an earlier draft did, which is why this note
    /// exists.
    #[test]
    fn editing_a_cell_by_double_click_does_not_also_activate_the_row() {
        let (mut tree, id, _, activated) = click_probe(EditTriggers::DOUBLE_CLICK);
        let cell = realized(&tree, id, 1, 0);
        let at = tree.bounds(cell).center();
        double_click_at(&mut tree, at);
        assert_eq!(
            activated.get(),
            0,
            "double-clicking an editable cell opened the item as well as the editor"
        );
    }

    /// The read-only column beside it still activates, which is what makes the
    /// guard a rule about *this gesture on an editable cell* rather than about
    /// the whole table.
    #[test]
    fn a_double_click_off_an_editable_cell_still_activates_the_row() {
        let (mut tree, id, _, activated) = click_probe(EditTriggers::DOUBLE_CLICK);
        let cell = realized(&tree, id, 1, 1);
        let at = tree.bounds(cell).center();
        double_click_at(&mut tree, at);
        assert_eq!(
            activated.get(),
            1,
            "a double-click away from an editable cell must still activate the row"
        );
    }

    /// **A cell that edits on double-click still lets its row select on a
    /// plain click.**
    ///
    /// `press_claimed_by_interactive_child` counted `on_double_tap` as owning
    /// the press, so merely giving a cell double-click-to-edit silently stopped
    /// its row selecting — while every file manager selects a row on the first
    /// click of the double-click that opens it. The claim is now about
    /// handlers that act on a single press (`on_tap` / `on_long_press`).
    #[test]
    fn a_double_click_editable_cell_still_lets_its_row_select_on_one_click() {
        let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(three_row_slice())
                .selection(selection.clone())
                .add_column(editable_name_col().edit_triggers(EditTriggers::DOUBLE_CLICK))
                .row_height(20.0)
                .on_cell_edit_request(|_row, _col, _ctx| {}),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let cell = realized(&tree, id, 1, 0);
        tree.click(cell);
        assert!(
            selection.is_selected(1),
            "one click on a double-click-editable cell must still select its row"
        );
    }

    /// ...whereas `SINGLE_CLICK` deliberately does claim the press: that cell's
    /// click means "edit this value", not "select this row". Documented on
    /// [`EditTriggers::SINGLE_CLICK`] and the reason the set is per column.
    #[test]
    fn a_single_click_editable_cell_claims_the_press_from_row_selection() {
        let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(three_row_slice())
                .selection(selection.clone())
                .add_column(editable_name_col().edit_triggers(EditTriggers::SINGLE_CLICK))
                .add_column(size_col())
                .row_height(20.0)
                .on_cell_edit_request(|_row, _col, _ctx| {}),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });

        let editable = realized(&tree, id, 1, 0);
        tree.click(editable);
        assert!(
            !selection.is_selected(1),
            "a SINGLE_CLICK cell's click must go to the editor, not to selection"
        );

        // The column beside it selects as always — which is what makes this a
        // property of the column rather than of the table.
        let plain = realized(&tree, id, 2, 1);
        tree.click(plain);
        assert!(
            selection.is_selected(2),
            "a click on a non-editing column must still select its row"
        );
    }

    /// The cell realized at `(row, display column)`.
    fn realized(tree: &WidgetTree, id: WidgetId, row: usize, col: usize) -> WidgetId {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.realized_cell(row, col)
            .unwrap_or_else(|| panic!("cell ({row}, {col}) is not realized"))
    }

    /// A laid-out table whose first column is editable under `triggers` and
    /// whose second is read-only, with the edit requests it receives and a
    /// count of row activations.
    #[allow(clippy::type_complexity)]
    fn click_probe(
        triggers: EditTriggers,
    ) -> (
        WidgetTree,
        WidgetId,
        Rc<RefCell<Vec<(usize, String)>>>,
        Rc<Cell<usize>>,
    ) {
        let seen: Rc<RefCell<Vec<(usize, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let activated: Rc<Cell<usize>> = Rc::new(Cell::new(0));
        let counter = activated.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_source(three_row_slice())
                .add_column(editable_name_col().edit_triggers(triggers))
                .add_column(size_col())
                .row_height(20.0)
                .on_cell_edit_request(move |row, col, _ctx| {
                    sink.borrow_mut().push((row, col.to_string()));
                })
                .on_row_activate(move |_row, _ctx| counter.set(counter.get() + 1)),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        (tree, id, seen, activated)
    }
}
