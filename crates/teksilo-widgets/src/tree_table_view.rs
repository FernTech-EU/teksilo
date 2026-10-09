// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeTableView<T>` — hierarchical multi-column data table with expand/collapse.
//!
//! Sibling of [`TableView`](crate::TableView) for tree-shaped data. Each row carries
//! a depth level; one designated column (the *tree column*, defaulting to the first)
//! shows a twist (chevron) and an indent gutter that toggles the row's children.
//! Backed by a [`SortFilterTreeModel<T>`] so sort, filter, and expand state compose
//! without extra bookkeeping. Shares the header (a hosted
//! [`TableHeader`]), column, keyboard, and selection
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
//! **Group rows that span the table, and the group kept in sight.** A tree
//! whose upper levels are groups — songs under albums under artists — draws
//! those rows as one band across every column with
//! [`full_width_row`](TreeTableView::full_width_row) and
//! [`full_width_row_delegate`](TreeTableView::full_width_row_delegate), and
//! keeps the groups being scrolled through pinned under the header with
//! [`pinned_ancestors`](TreeTableView::pinned_ancestors):
//!
//! ```ignore
//! let view = TreeTableView::from_projection(proxy)
//!     .columns(columns)
//!     .full_width_row(|row: &Row| !row.is_song())
//!     .full_width_row_delegate(|row, _cx: &CellContext| {
//!         Box::new(TextWidget::new(lit!(row.summary()))) as Box<dyn Widget>
//!     })
//!     .pinned_ancestors(2); // the artist, then the album
//! ```
//!
//! A band is still a row of the tree — it selects, drags, expands from its
//! chevron and answers type-ahead — and one cell to the keyboard and to
//! assistive technology. The pinned rows are copies, pushed up as the next
//! group arrives, hidden from assistive technology, and every reveal stops
//! below them. The two builders say the rest.
//!
//! ## Accessibility
//!
//! Root emits `Role::TreeGrid`; rows carry `set_level` + `set_expanded`.
//! ArrowLeft / ArrowRight on the tree column collapse / expand. A full-width
//! row holds one cell, spanning every column.
//!
//! ```ignore
//! // Column delegates capture closures — use ignore.
//! use teksilo_widgets::TreeTableView;
//! use teksilo_data::TreeModel;
//! # struct File { name: String }
//! # let model: TreeModel<File> = TreeModel::new();
//! let _view = TreeTableView::new(model).row_height(28.0);
//! ```
//!
//! ## Pan to scroll
//!
//! The view installs [`common::scrollable::ScrollableBehavior`](crate::common::scrollable::ScrollableBehavior),
//! which gives it the shared wheel arithmetic, a finger's pan and the
//! `PanClaim` that puts it on a pan's claimant chain. A pan scrolls it, the
//! release coasts, and a pan it cannot absorb hands the **whole** event to the
//! container outside — never a residual. A pan that starts on a row scrolls
//! rather than activating it or collapsing a multi-selection onto it. Both axes
//! are claimed. Shift+wheel still
//! scrolls the columns, and a finger's pan is never remapped by a held Shift:
//! the remap is a wheel convention, and turning a drag sideways is not what
//! the hand asked for.

mod body_pane;
mod full_width;
mod pinned;
mod widget_impl;

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
use teksilo_core::kinetic::KineticScroller;
use teksilo_core::pointer::touch_action::PanAxes;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{
    DropPosition, KeyedSelectionModel, NodeId, SelectionModel, SortDirection, SortFilterTreeModel,
    TreeFilterMode, TreeModel,
};
use teksilo_i18n::LocalizedString;
use teksilo_tokens::{BorderRole, OverscrollStyle, SurfaceRole};

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
    CellContext, Column, ColumnResizePolicy, EditTriggers, GridLines, PinnedSide, TabTraversal,
};
use crate::table_view::header::{ResizeStateHandle, SharedFrozenWidths, width_overrides};
use crate::table_view::imperative;
use crate::table_view::keyboard;
use crate::table_view::layout;
use crate::table_view::row_navigator::RowNavigator;
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};
use crate::table_view::{HeaderLink, TableHeader};
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
    /// Which rows are full-width ones, when the view has any.
    full_width: Option<Rc<dyn Fn(usize) -> bool>>,
    /// The reveal below the pinned ancestors, while the view pins any.
    pinned: Option<pinned::PinnedReveal>,
}

impl<T: 'static> TreeNavigator<T> {
    pub(crate) fn new(source: Rc<TreeSource<T>>) -> Self {
        Self {
            source,
            full_width: None,
            pinned: None,
        }
    }

    pub(crate) fn with_full_width(mut self, at: Option<Rc<dyn Fn(usize) -> bool>>) -> Self {
        self.full_width = at;
        self
    }

    pub(crate) fn with_pinned(mut self, reveal: Option<pinned::PinnedReveal>) -> Self {
        self.pinned = reveal;
        self
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

    fn spans_all_columns(&self, row: usize) -> bool {
        self.full_width.as_ref().is_some_and(|at| at(row))
    }

    fn reveal_scroll(
        &self,
        row: usize,
        scroll: f32,
        viewport: f32,
        max_scroll: f32,
    ) -> Option<f32> {
        self.pinned
            .as_ref()
            .map(|pinned| pinned.scroll_for(row, scroll, viewport, max_scroll))
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
    /// See [`Self::stretch_last_column`].
    stretch_last_column: bool,
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
    /// This surface's pan physics: the range a finger's pan is clamped to and
    /// the offset it is currently holding. Owned by the view rather than by
    /// the [`ScrollableBehavior`](crate::common::scrollable::ScrollableBehavior)
    /// so it survives a rebuild, and so `place_children` — the only pass that
    /// knows the viewport extent — can publish into it.
    scroller: Rc<RefCell<KineticScroller>>,
    sort_signal: Signal<Option<(String, SortDirection)>>,
    column_widths_signal: Signal<HashMap<String, f32>>,
    /// `Some` once `bind_column_widths` adopted the width map — see the
    /// field of the same name on `TableView`.
    column_widths_frozen: Option<SharedFrozenWidths>,
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
    /// Suppressed while the view is sorted by one of its own columns (the
    /// visible order then differs from the tree order, so a manual reorder
    /// would be meaningless). A shared sort on a column it lacks leaves it on.
    reorderable: bool,
    /// Active row-drop insertion indicator `(body_local_y, width)`. Set by
    /// `on_drag_hover`, cleared on leave / drop, read by `paint`.
    drop_feedback: Signal<Option<DropViz>>,

    /// Whether activation is a single or double click (default `DoubleClick`).
    activate_on: crate::data_views::ActivateOn,

    /// `true` while this view — its root or any descendant — holds keyboard
    /// focus. Captured at build from [`BuildContext::begin_view_focus`], bound
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

    /// Which items are drawn as one band across every column — see
    /// [`full_width_row`](Self::full_width_row).
    #[allow(clippy::type_complexity)]
    full_width_row: Option<Rc<dyn Fn(&T) -> bool>>,
    /// What draws that band — see
    /// [`full_width_row_delegate`](Self::full_width_row_delegate).
    full_width_delegate: Option<full_width::BandDelegate<T>>,
    /// How many levels of ancestors stay pinned under the header; `0` pins
    /// none. See [`pinned_ancestors`](Self::pinned_ancestors).
    pinned_ancestors: usize,
    /// The rows the pinned stack copies, outermost first, written by
    /// `place_children` and bound `Rebuild` on the stack. Owned here so it
    /// survives the root's rebuilds, as `ListView::pinned_section` does.
    pinned_rows: Signal<Vec<usize>>,
    /// The rows the stack last built. See `pinned::PinnedStack::built`.
    pinned_built: Rc<RefCell<Vec<usize>>>,
    /// Where each line of the stack goes this frame, from `place_children`.
    pinned_layout: Rc<RefCell<Vec<pinned::StackLine>>>,
    pinned_stack_id: Option<WidgetId>,
}

impl<T: 'static> TreeTableView<T> {
    /// Wrap a `SortFilterTreeModel<T>`.
    /// Wrap a `SortFilterTreeModel<T>`.
    pub fn from_projection(proxy: SortFilterTreeModel<T>) -> Self {
        let source = Rc::new(TreeSource::from_data_source(Rc::new(proxy.clone())));
        Self::assemble(source, Some(proxy))
    }

    /// Build a tree table over any [`TreeDataSource`] — an external source of
    /// truth (an entity store, a database, a virtual filesystem) carrying
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
        let table_id = crate::table_view::next_header_id();
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
            stretch_last_column: false,
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
            scroller: Rc::new(RefCell::new(KineticScroller::new(OverscrollStyle::Clamp))),
            sort_signal: Signal::new(None),
            column_widths_signal: Signal::new(HashMap::new()),
            column_widths_frozen: None,
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
            full_width_row: None,
            full_width_delegate: None,
            pinned_ancestors: 0,
            pinned_rows: Signal::new(Vec::new()),
            pinned_built: Rc::new(RefCell::new(Vec::new())),
            pinned_layout: Rc::new(RefCell::new(Vec::new())),
            pinned_stack_id: None,
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
    /// is **suppressed while the view is sorted** by one of its own columns:
    /// with the visible order driven by the sort, a manual reorder would have
    /// no visible effect. A sort a shared [`bind_sort`](Self::bind_sort)
    /// holds for a column this view lacks does not count.
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
    /// [`on_rows_received`](Self::on_rows_received), which are source-agnostic.
    /// A source-backed view expresses foreign-accept through its source's own
    /// capability closures (`can_accept` / `accept_drop`), like `ListView` /
    /// `TableView`; this hook is what a **projection**-backed view has instead,
    /// since a `SortFilterTreeModel` carries no such closures.
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

    /// Draw the rows whose item satisfies `is_full_width` as **one cell across
    /// every column** instead of a cell per column: a group row ("an artist ·
    /// 12 songs | 3 albums | 52:10") over rows that fill the columns. What the
    /// band shows comes from
    /// [`full_width_row_delegate`](Self::full_width_row_delegate); without one
    /// it shows the tree column's own cell, laid across the row.
    ///
    /// Asked of the item, like the cell delegates; a row whose item is still
    /// loading is never full-width. The predicate is read on every build and
    /// on every key press, so keep it to a field test.
    ///
    /// The row stays a row of the tree in every other respect: it selects,
    /// activates, drags, takes drops and answers type-ahead as any row does,
    /// its chevron (indent and twist) sits in the band, and ←/→ act on it as
    /// on the tree column from any column: they collapse and expand it, and ←
    /// on a collapsed band moves up to its parent. In cell navigation the band
    /// is one cell: Home and End stay on it, Tab passes it as one stop, and no
    /// column of it opens an editor. The cursor keeps the column it arrived
    /// with, so stepping on to an ordinary row lands back in that column.
    ///
    /// The band is as wide as the columns together, or the row when they are
    /// wider, so beside columns narrower than the view it ends where a row's
    /// selection band, alternating tint and focus ring do. It stays in the
    /// viewport while the columns
    /// scroll sideways, so a group row stays readable; its chevron sits where
    /// the tree column's does with the columns unscrolled. Its height comes
    /// from the view's height mode like any row's: the uniform height, the
    /// [`row_height_fn`](Self::row_height_fn) callback, or, under
    /// [`auto_row_height`](Self::auto_row_height), the band measured at its
    /// width.
    ///
    /// To assistive technology the row is unchanged (`Role::Row` with its
    /// level, expanded state and position) and holds one `Role::Cell`
    /// (`Role::GridCell` in the cell modes) at column 1, with a column span of
    /// every column and the band as its content.
    pub fn full_width_row(mut self, is_full_width: impl Fn(&T) -> bool + 'static) -> Self {
        self.full_width_row = Some(Rc::new(is_full_width));
        self
    }

    /// Build the band of a [`full_width_row`](Self::full_width_row) from its
    /// item, as a [`Column`]'s delegate builds a cell. The context describes
    /// the band as the tree column, whose chevron it carries: `col_id` and
    /// `col_index` are the tree column's and `is_tree_column` is `true`;
    /// `is_focused` holds while the cursor is on the row, in any column, and
    /// `is_selected` while the row is selected (in a cell mode, while any of
    /// its cells is). The indent and the chevron are drawn before the
    /// delegate's widget, as in the tree column.
    ///
    /// Does nothing without [`full_width_row`](Self::full_width_row), which
    /// says which rows get it.
    pub fn full_width_row_delegate(
        mut self,
        delegate: impl Fn(&T, &CellContext) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.full_width_delegate = Some(Rc::new(delegate));
        self
    }

    /// Keep the ancestor rows of the first visible row pinned under the
    /// header while the view scrolls, outermost first and at most `depth`
    /// levels of them: the "sticky scroll" of code editors, the artist and
    /// then the album over the songs. `0` (the default) pins nothing.
    ///
    /// A pinned row is pushed up by the next row at its level as that row
    /// arrives, as a pinned section header is, so it never covers a row it is
    /// not an ancestor of. It is a copy, drawn with the real row's cells (its
    /// band, for a [`full_width_row`](Self::full_width_row)) at the real row's
    /// height, on the header's surface. The copy shows the row's content, not
    /// its selection or its focus, and its chevron is a picture. Under
    /// [`auto_row_height`](Self::auto_row_height) the copy measures the row,
    /// as the row does once laid out: an ancestor the view scrolled past
    /// without laying it out takes its height a frame after it is pinned, and
    /// a reveal made before then counts it at the estimate.
    ///
    /// A press on a pinned row does what a plain click on the real row would:
    /// it ends an open cell edit, selects the row and puts the cursor on it,
    /// activates it under [`ActivateOn::SingleClick`](crate::data_views::ActivateOn::SingleClick),
    /// and scrolls it back into view, right under its own pinned ancestors. A
    /// double click on a copy does not activate it, and a press never reaches
    /// a control drawn inside the copy. A drop on a pinned row lands on the
    /// row it shows, and its indicator is drawn over the copy.
    ///
    /// Every reveal the view makes (the keyboard moving the cursor,
    /// [`ensure_row_visible`](Self::ensure_row_visible),
    /// [`scroll_to_row`](Self::scroll_to_row), taking focus, the
    /// ScrollIntoView action) stops below the ancestors that would be pinned
    /// over the row, so a copy never covers the row the cursor is on.
    ///
    /// The copies are hidden from assistive technology and are not focusable;
    /// the real rows stay where they are in the tree, scrolled under the
    /// copies like any row off the top. The stack is re-derived on every
    /// layout, so an expand, a collapse, an insert or a re-source above or
    /// inside it shows on the next frame.
    pub fn pinned_ancestors(mut self, depth: usize) -> Self {
        self.pinned_ancestors = depth;
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

    /// Let the **last column in display order** take up whatever width the
    /// other columns leave, so the table never ends in a bare strip at its
    /// trailing edge — Qt's `stretchLastSection`, NSTableView's
    /// `lastColumnOnlyAutoresizingStyle`. Default: off.
    ///
    /// Positional, not a property of a column: after a reorder it is the
    /// *new* last column that stretches and the previous one goes back to
    /// its own width. While a column stretches, its declared width is the
    /// floor it grows from, its user-resize override is ignored, and its
    /// trailing grip is disabled (no AccessKit Increment/Decrement either):
    /// any size the user gave it, the stretch would take straight back.
    /// Resizing any *other* column reflows it. Once the other columns
    /// exceed the viewport there is nothing left to stretch into — the
    /// last column sits at its own width and the pane scrolls, as in Qt.
    ///
    /// `Flex` columns already share every spare pixel among themselves, so
    /// a table of `Flex` columns looks the same either way: this is for
    /// pixel-sized tables (`Fixed` / `Auto`, or widths the user has set)
    /// that would otherwise end in a gap.
    pub fn stretch_last_column(mut self, on: bool) -> Self {
        self.stretch_last_column = on;
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

    /// Set the keyboard Tab traversal direction inside the table (default `CellsThenRows`).
    pub fn tab_traversal(mut self, mode: TabTraversal) -> Self {
        self.tab_traversal = mode;
        self
    }

    /// Set which user gestures open an in-place cell editor — a set, composed
    /// with `|` (default `F2 | ANY_KEY | DOUBLE_CLICK`). See [`EditTriggers`].
    pub fn edit_triggers(mut self, trigger: EditTriggers) -> Self {
        self.edit_triggers = trigger;
        self
    }

    /// Callback invoked when the user requests an in-place cell edit (e.g.
    /// double-click when `edit_triggers` contains `DOUBLE_CLICK`). Receives the flat row
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

    // ── Adopted column state ──────────────────────────────────────────
    //
    // The same contract as `TableView`'s `bind_*` builders: see
    // "Column state an application owns" in `table_view`'s module docs.

    /// Use `sort` as this view's sort state instead of a signal of its own:
    /// header clicks and [`set_sort`](Self::set_sort) write it, a write from
    /// anywhere else updates the header, and
    /// [`sort_signal`](Self::sort_signal) returns it. Bind the same signal to
    /// the [`SortFilterTreeModel`] to re-sort the rows; one signal shared by
    /// both is what lets a projection keep its preset comparators while the
    /// header drives it.
    ///
    /// The view writes nothing into it until the user sorts; see
    /// [`TableView`](crate::TableView)'s module docs, "Column state an
    /// application owns".
    pub fn bind_sort(mut self, sort: Signal<Option<(String, SortDirection)>>) -> Self {
        self.sort_signal = sort;
        self
    }

    /// Use `widths` as this view's map of column id → width instead of a
    /// signal of its own: a resize drag and
    /// [`set_column_width`](Self::set_column_width) write it, and a write from
    /// anywhere else resizes the columns. A column with no entry takes its
    /// declared width; an entry for a column the view lacks is ignored. A
    /// resize writes the resized column's entry alone; the `Flex` columns
    /// before it keep their widths in this view, not in the map.
    /// [`column_widths_signal`](Self::column_widths_signal) returns it.
    pub fn bind_column_widths(mut self, widths: Signal<HashMap<String, f32>>) -> Self {
        self.column_widths_signal = widths;
        self.column_widths_frozen = Some(Rc::default());
        self
    }

    /// Use `order` as this view's column order instead of a signal of its
    /// own: a reorder drop and [`set_column_order`](Self::set_column_order)
    /// write it, and a write from anywhere else reorders the columns. Ids the
    /// view lacks are skipped when it lays out and kept in place when it
    /// writes. [`column_order_signal`](Self::column_order_signal) returns it.
    pub fn bind_column_order(mut self, order: Signal<Vec<String>>) -> Self {
        self.column_order_signal = order;
        self
    }

    /// Use `filters` as this view's per-column filter text instead of a
    /// signal of its own: the filter popover and
    /// [`set_filter`](Self::set_filter) write it, and
    /// [`filters_signal`](Self::filters_signal) returns it. Bind the same
    /// signal to the [`SortFilterTreeModel`] to filter the rows.
    pub fn bind_filters(mut self, filters: Signal<HashMap<String, String>>) -> Self {
        self.filters_signal = filters;
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
    /// the view's empty signal at construction would clobber them. To share
    /// one signal between the two from the start, hand it to
    /// [`bind_sort`](Self::bind_sort); this returns the adopted signal then.
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
    ///
    /// The signal adopted by [`bind_filters`](Self::bind_filters), if any.
    pub fn filters_signal(&self) -> &Signal<HashMap<String, String>> {
        &self.filters_signal
    }

    /// User-resized column widths in logical pixels, keyed by column id. The
    /// view writes entries only for a resize: the resized column's, and —
    /// when the map is its own — one for each `Flex` column before it, frozen
    /// at the width it had. A missing key means the declared width. The
    /// signal adopted by [`bind_column_widths`](Self::bind_column_widths), if
    /// any.
    pub fn column_widths_signal(&self) -> &Signal<HashMap<String, f32>> {
        &self.column_widths_signal
    }

    /// Current column display order as a list of column ids. The signal
    /// adopted by [`bind_column_order`](Self::bind_column_order), if any.
    pub fn column_order_signal(&self) -> &Signal<Vec<String>> {
        &self.column_order_signal
    }

    /// Keyboard-focused cell as `(row, display_column_index)`, or `None`.
    ///
    /// In a tree table whose selection holds one row or cell, a selection
    /// change that leaves an existing cursor off the selection moves the
    /// cursor onto it.
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
    ///
    /// With [`pinned_ancestors`](Self::pinned_ancestors), the top it is
    /// aligned to is the bottom of its own pinned ancestors.
    pub fn scroll_to_row(&self, row: usize) {
        if !self.laid_out.get() {
            return;
        }
        match self.pinned_reveal() {
            Some(pinned) => {
                if let Some(target) = pinned.top_for(row) {
                    self.scroll_y
                        .set(target.clamp(0.0, self.max_scroll_y.get()));
                }
            }
            None => imperative::scroll_to_row(
                row,
                &self.row_metrics,
                &self.scroll_y,
                &self.max_scroll_y,
            ),
        }
    }

    /// Scroll the minimum distance needed to make `row` visible. A no-op
    /// before the first layout pass, when the viewport height is not yet known.
    ///
    /// With [`pinned_ancestors`](Self::pinned_ancestors), "visible" means
    /// below the ancestors that would be pinned over it.
    pub fn ensure_row_visible(&self, row: usize) {
        Self::reveal_row(
            row,
            self.pinned_reveal().as_ref(),
            &self.row_metrics,
            &self.scroll_y,
            &self.max_scroll_y,
            self.viewport_height.get(),
            self.laid_out.get(),
        );
    }

    /// The one reveal every path shares: below the pinned ancestors when
    /// there are any, the plain row arithmetic otherwise.
    fn reveal_row(
        row: usize,
        pinned: Option<&pinned::PinnedReveal>,
        row_metrics: &SharedRowMetrics,
        scroll_y: &Signal<f32>,
        max_scroll_y: &Signal<f32>,
        viewport_height: f32,
        laid_out: bool,
    ) {
        let Some(pinned) = pinned else {
            imperative::ensure_row_visible(
                row,
                row_metrics,
                scroll_y,
                max_scroll_y,
                viewport_height,
                laid_out,
            );
            return;
        };
        if !laid_out {
            return;
        }
        let scroll = scroll_y.get();
        let new_scroll = pinned.scroll_for(row, scroll, viewport_height, max_scroll_y.get());
        if (new_scroll - scroll).abs() > f32::EPSILON {
            scroll_y.set(new_scroll);
        }
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
    /// (`tree_table_view.rs:145-147`), and the keyed selection facade builds
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
    /// (`table_view/keyboard.rs:672`) adds on top is chasing the row into an
    /// *enclosing* scroll area, and that needs an `EventContext`, which an
    /// effect does not have. Nothing is lost: the same keyboard or programmatic
    /// focus change makes the framework reveal the newly focused widget in
    /// every ancestor scroll area itself (`focus_impl.rs:109`,
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
        let pinned = self.pinned_reveal();

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
            Self::reveal_row(
                row,
                pinned.as_ref(),
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

    /// Put a single column on `side`: [`PinnedSide::Leading`] or
    /// [`PinnedSide::Trailing`] pins it, [`PinnedSide::None`] unpins it, a
    /// column declared pinned ([`Column::pinned`]) included.
    ///
    /// Writes [`column_pinning_signal`](Self::column_pinning_signal) the way a
    /// header drag does: an entry for a column moved off its declared side,
    /// and none for a column put back on it.
    pub fn set_column_pinning(&self, col_id: &str, side: PinnedSide) {
        let declared = self
            .columns
            .iter()
            .find(|c| c.spec.id == col_id)
            .map_or(PinnedSide::None, |c| c.spec.pinned);
        imperative::set_column_pinning(&self.column_pinning_signal, col_id, side, declared);
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
    ///
    /// A [`full_width_row`](Self::full_width_row) has no cell of any column
    /// to edit, so a row that is one is a no-op too.
    pub fn begin_edit(&self, row: usize, col_id: &str) {
        if self.full_width_rows().is_some_and(|fw| (fw.at)(row)) {
            return;
        }
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

    /// The full-width rows and the delegate that draws them, or `None` when
    /// the view has none. The delegate falls back to the tree column's cell.
    fn full_width_rows(&self) -> Option<full_width::FullWidthRows<T>> {
        let is_full_width = self.full_width_row.clone()?;
        let delegate = match &self.full_width_delegate {
            Some(delegate) => delegate.clone(),
            None => self
                .columns
                .get(self.tree_column_decl_index())?
                .cell
                .clone(),
        };
        let source = self.source.clone();
        Some(full_width::FullWidthRows {
            at: Rc::new(move |row| {
                let mut hit = false;
                (source.read_item_fn)(row, &mut |item| hit = is_full_width(item));
                hit
            }),
            delegate,
        })
    }

    /// The reveal below the pinned ancestors, while the view pins any.
    fn pinned_reveal(&self) -> Option<pinned::PinnedReveal> {
        pinned::PinnedReveal::new(
            self.source.clone(),
            self.row_metrics.clone(),
            self.pinned_ancestors,
        )
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
                if &col.spec.id == id {
                    return i;
                }
            }
        }
        0
    }

    /// Display order as indices into `self.columns` — see
    /// [`layout::display_order`], shared with `TableView`.
    fn display_order(&self) -> Vec<usize> {
        let (out, boundaries) = layout::display_order(
            &self.columns,
            &self.column_order_signal.get(),
            &self.column_pinning_signal.get(),
        );
        // Stash the boundaries so paint / place_children / the keyboard
        // handler's ensure-column-visible can read them.
        *self.pane_boundaries.borrow_mut() = boundaries;
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

#[cfg(test)]
mod tests;
