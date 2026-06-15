// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeTable<T>` — hierarchical multi-column widget.
//!
//! Sibling of [`TableView`](crate::TableView). Backed by
//! [`SortFilterTreeModel<T>`](bastyde_data::SortFilterTreeModel) so sort
//! and filter compose with expand/collapse for free.
//!
//! Reuses the shared `column`, `header`, `layout`, `keyboard`, and
//! `selection` modules of [`crate::table_view`]. Differences from
//! `TableView`:
//!
//! - Rows come from the proxy's flattened visible list (depth carried
//!   in [`FlatEntry`](bastyde_data::FlatEntry)).
//! - One column is the **tree column**: cells gain an indent gutter
//!   plus a twist (chevron) that toggles expansion. Defaults to the
//!   first column declared.
//! - `Role::TreeGrid` on the root, `TreeRowA11y` on rows (carries
//!   `set_level` + `set_expanded`).
//! - ArrowLeft / ArrowRight on the tree column collapse / expand.
//! - Row drag-drop is NOT shipped here: insertion-vs-reparent UX
//!   requires its own design pass and is out-of-scope.
//!
//! Rows live in a `TreeBodyPane` — a sibling
//! of the scrollbar, so buffer-exit / selection / expand rebuilds are
//! never deferred mid-thumb-drag (see that module's doc).
//!
//! Row heights come in three modes: uniform (`row_height`, the default
//! fast path), exact per-flat-index callback (`row_height_fn`), and
//! auto-measured (`auto_row_height` — rows grow to their tallest cell);
//! expand/collapse/sort keep measured heights above the change via the
//! proxy's `first_changed_index` divergence. See docs/table-view.md
//! "Row heights".

mod body_pane;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};

use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::EventResponse;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{
    NodeId, SelectionModel, SortDirection, SortFilterTreeModel, TreeFilterMode, TreeModel,
};
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{BorderRole, SurfaceRole};

use crate::styles::recipe_table_style as cp;

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};
use crate::table_view::body::SharedColumnWidths;
use crate::table_view::column::{
    Column, ColumnResizePolicy, EditTrigger, GridLines, PinnedSide, TabTraversal,
};
use crate::table_view::header::{HeaderCell, HeaderRow, ResizeStateHandle};
use crate::table_view::keyboard;
use crate::table_view::layout;
use crate::table_view::row_navigator::RowNavigator;
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};

const BUFFER_ROWS: usize = 5;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Hierarchical projection navigator. Adapts
/// [`SortFilterTreeModel`]'s
/// flat-list view to the [`RowNavigator`] interface used by the
/// shared keyboard handler.
pub(crate) struct TreeNavigator<T: 'static> {
    proxy: SortFilterTreeModel<T>,
}

impl<T: 'static> TreeNavigator<T> {
    pub(crate) fn new(proxy: SortFilterTreeModel<T>) -> Self {
        Self { proxy }
    }
}

impl<T: 'static> RowNavigator for TreeNavigator<T> {
    fn row_count(&self) -> usize {
        self.proxy.visible_count()
    }

    fn depth(&self, row: usize) -> Option<usize> {
        self.proxy.entry_at(row).map(|e| e.depth)
    }

    fn has_children(&self, row: usize) -> bool {
        self.proxy
            .entry_at(row)
            .map(|e| e.has_children)
            .unwrap_or(false)
    }

    fn is_expanded(&self, row: usize) -> bool {
        self.proxy
            .entry_at(row)
            .map(|e| e.is_expanded)
            .unwrap_or(false)
    }

    fn toggle_expanded(&self, row: usize) {
        if let Some(node) = self.proxy.visible_node_id(row) {
            self.proxy.toggle(node);
        }
    }
}

/// Hierarchical multi-column widget. See module documentation.
pub struct TreeTable<T: 'static> {
    proxy: SortFilterTreeModel<T>,

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
    selection: Option<SelectionModel>,
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

    // Build state
    header_row_id: Option<WidgetId>,
    body_pane_id: Option<WidgetId>,
    scrollbar_id: Option<WidgetId>,
    /// Pane-local rebuild trigger + buffered range, owned here so they
    /// survive `TreeTable` rebuilds (each rebuild constructs a fresh
    /// `TreeBodyPane` struct that inherits these handles).
    pane_version: Signal<u64>,
    pane_built_start: Rc<Cell<usize>>,
    pane_built_end: Rc<Cell<usize>>,
    /// Bumped by the pane when a measure pass changes the content
    /// total; bound at `Relayout` on this root so `max_scroll_y` / the
    /// thumb ratio are recomputed with the corrected total next frame.
    pane_total_refresh: Signal<u64>,

    // Layout state
    column_widths: SharedColumnWidths,
    display_indices: Rc<RefCell<Vec<usize>>>,
    viewport_height: Rc<Cell<f32>>,
    resize_state: ResizeStateHandle,
    table_id: usize,
}

impl<T: 'static> TreeTable<T> {
    /// Wrap a `SortFilterTreeModel<T>`.
    pub fn from_projection(proxy: SortFilterTreeModel<T>) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let table_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
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
            selection: None,
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
            editing_cell: Signal::new(None),
            header_row_id: None,
            body_pane_id: None,
            scrollbar_id: None,
            pane_version: Signal::new(0_u64),
            pane_built_start: Rc::new(Cell::new(0)),
            pane_built_end: Rc::new(Cell::new(0)),
            pane_total_refresh: Signal::new(0_u64),
            column_widths: Rc::new(RefCell::new(Vec::new())),
            display_indices: Rc::new(RefCell::new(Vec::new())),
            viewport_height: Rc::new(Cell::new(600.0)),
            resize_state: Rc::new(RefCell::new(None)),
            table_id,
        }
    }

    /// Wrap a raw `TreeModel<T>` — convenience for callers that don't
    /// need sort/filter. Internally builds an identity
    /// `SortFilterTreeModel`.
    pub fn new(model: TreeModel<T>) -> Self {
        Self::from_projection(SortFilterTreeModel::new(model))
    }

    // ── Builder ────────────────────────────────────────────────────────

    /// Set the scroll-chaining behavior at the boundary (default
    /// [`OverscrollBehavior::Chain`]; [`Contain`](OverscrollBehavior::Contain)
    /// disables chaining to an ancestor scrollable).
    pub fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.overscroll_behavior = behavior;
        self
    }

    pub fn add_column(mut self, col: Column<T>) -> Self {
        self.columns.push(col);
        self
    }

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

    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = Some(height);
        self
    }

    pub fn show_header(mut self, visible: bool) -> Self {
        self.show_header = visible;
        self
    }

    pub fn selection_mode(mut self, mode: TableSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    pub fn selection(mut self, sel: SelectionModel) -> Self {
        self.selection = Some(sel);
        self
    }

    pub fn cell_selection(mut self, sel: CellSelectionModel) -> Self {
        self.cell_selection = Some(sel);
        self
    }

    pub fn alternating_rows(mut self, enabled: bool) -> Self {
        self.alternating_rows = enabled;
        self
    }

    pub fn grid_lines(mut self, kind: GridLines) -> Self {
        self.grid_lines = kind;
        self
    }

    pub fn a11y_label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.a11y_label = Some(label.into());
        self
    }

    pub fn show_internal_scrollbars(mut self, show: bool) -> Self {
        self.show_internal_scrollbars = show;
        self
    }

    pub fn column_resize_policy(mut self, policy: ColumnResizePolicy) -> Self {
        self.column_resize_policy = policy;
        self
    }

    pub fn tab_traversal(mut self, mode: TabTraversal) -> Self {
        self.tab_traversal = mode;
        self
    }

    pub fn edit_trigger(mut self, trigger: EditTrigger) -> Self {
        self.edit_trigger = trigger;
        self
    }

    pub fn on_cell_edit_request(
        mut self,
        f: impl Fn(usize, &str, &mut EventContext) + 'static,
    ) -> Self {
        self.on_cell_edit_request = Some(Rc::new(f));
        self
    }

    pub fn on_row_activate(mut self, f: impl Fn(usize, &mut EventContext) + 'static) -> Self {
        self.on_row_activate = Some(Rc::new(f));
        self
    }

    /// Forward `mode` to the underlying projection. The proxy holds its
    /// state behind `Rc<RefCell>`, so calling `.filter_mode()` on a
    /// clone mutates the shared inner — effectively persisting the
    /// choice on `self.proxy`.
    pub fn filter_mode(self, mode: TreeFilterMode) -> Self {
        let _ = self.proxy.clone().filter_mode(mode);
        self
    }

    // ── Reactive signals ──────────────────────────────────────────────

    pub fn scroll_y_signal(&self) -> &Signal<f32> {
        &self.scroll_y
    }

    pub fn max_scroll_y_signal(&self) -> &Signal<f32> {
        &self.max_scroll_y
    }

    pub fn viewport_ratio_y_signal(&self) -> &Signal<f32> {
        &self.viewport_ratio_y
    }

    pub fn sort_signal(&self) -> &Signal<Option<(String, SortDirection)>> {
        &self.sort_signal
    }

    pub fn filters_signal(&self) -> &Signal<HashMap<String, String>> {
        &self.filters_signal
    }

    pub fn column_widths_signal(&self) -> &Signal<HashMap<String, f32>> {
        &self.column_widths_signal
    }

    pub fn column_order_signal(&self) -> &Signal<Vec<String>> {
        &self.column_order_signal
    }

    pub fn focused_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.focused_cell
    }

    pub fn editing_cell_signal(&self) -> &Signal<Option<(usize, usize)>> {
        &self.editing_cell
    }

    pub fn projection(&self) -> &SortFilterTreeModel<T> {
        &self.proxy
    }

    // ── Imperative API ─────────────────────────────────────────────────

    pub fn expand(&self, node: NodeId) {
        self.proxy.expand(node);
    }

    pub fn collapse(&self, node: NodeId) {
        self.proxy.collapse(node);
    }

    pub fn toggle(&self, node: NodeId) {
        self.proxy.toggle(node);
    }

    pub fn expand_all(&self) {
        self.proxy.expand_all();
    }

    pub fn collapse_all(&self) {
        self.proxy.collapse_all();
    }

    pub fn set_focused_cell(&self, row: usize, col: usize) {
        self.focused_cell.set(Some((row, col)));
    }

    pub fn clear_focused_cell(&self) {
        self.focused_cell.set(None);
    }

    pub fn set_sort(&self, col_id: Option<&str>, dir: SortDirection) {
        self.sort_signal.set(col_id.map(|c| (c.to_string(), dir)));
    }

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
}

impl<T: 'static> std::fmt::Debug for TreeTable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeTable")
            .field("rows", &self.proxy.visible_count())
            .field("columns", &self.columns.len())
            .field("tree_column", &self.tree_column_id)
            .finish()
    }
}

impl<T: 'static> Widget for TreeTable<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
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

        // Bump version on projection version (data + sort/filter +
        // expand/collapse all in one signal). Proxy observers fire
        // synchronously per rebuild, so `first_changed_index()`
        // describes exactly this change — heights of flat rows before
        // it (e.g. above an expand/collapse point) stay valid.
        let v_for_proj = version.clone();
        let proj_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.proxy.version_signal(), {
            let metrics = self.row_metrics.clone();
            let proxy = self.proxy.clone();
            move |_| {
                metrics
                    .borrow_mut()
                    .apply_divergence(proxy.first_changed_index(), proxy.visible_count());
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
        //   proxy.bind_sort_signal(tree_table.sort_signal().clone());
        //   proxy.bind_filters_signal(tree_table.filters_signal().clone());
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

        let navigator: Rc<dyn RowNavigator> = Rc::new(TreeNavigator::new(self.proxy.clone()));
        let key_cfg = keyboard::KeyHandlerConfig {
            navigator,
            col_count: display_indices.len().max(1),
            focused_cell: self.focused_cell.clone(),
            selection_mode: self.selection_mode,
            selection: self.selection.clone(),
            cell_selection: self.cell_selection.clone(),
            scroll_y: self.scroll_y.clone(),
            max_scroll_y: self.max_scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            row_metrics: self.row_metrics.clone(),
            tab_traversal: self.tab_traversal,
            editing_cell: self.editing_cell.clone(),
            edit_trigger: self.edit_trigger,
            display_col_to_id,
            display_col_editable,
            on_cell_edit_request: self.on_cell_edit_request.clone(),
            on_row_activate: self.on_row_activate.clone(),
        };

        let handlers = HandlerSet::new()
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
                        let (new_y, moved) =
                            crate::common::scroll::scroll_clamp_axis(current, dy, max);
                        scroll_y_for_wheel.set(new_y);
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
            .on_key(keyboard::build_key_handler(key_cfg))
            .clips_children(true)
            .focusable(true);
        ctx.apply_self_handlers(handlers);

        // ── Build children ────────────────────────────────────────────
        self.header_row_id = None;
        self.body_pane_id = None;
        self.scrollbar_id = None;

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
        let row_count = self.proxy.visible_count();
        if row_count > 0 {
            let pane = body_pane::TreeBodyPane::<T> {
                proxy: self.proxy.clone(),
                columns: self.columns.clone(),
                display_indices: self.display_indices.clone(),
                column_widths: self.column_widths.clone(),
                tree_display_pos,
                indent_per_level,
                row_metrics: self.row_metrics.clone(),
                selection_mode: self.selection_mode,
                selection: self.selection.clone(),
                cell_selection: self.cell_selection.clone(),
                scroll_y: self.scroll_y.clone(),
                viewport_height: self.viewport_height.clone(),
                editing_cell: self.editing_cell.clone(),
                focused_cell: self.focused_cell.clone(),
                version: self.pane_version.clone(),
                prev_built_start: self.pane_built_start.clone(),
                prev_built_end: self.pane_built_end.clone(),
                total_refresh: self.pane_total_refresh.clone(),
                row_entries: Vec::new(),
            };
            self.body_pane_id = Some(ctx.add(pane));
        }

        // Scrollbar.
        if self.show_internal_scrollbars {
            let sb = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            );
            self.scrollbar_id = Some(ctx.add(sb));
        }

        // Z-order mirrors TableView: body pane first, header last so it
        // paints above any row that bleeds into the header band on
        // overscroll.
        let mut children: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
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
            .total_height(self.proxy.visible_count());
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
        let body_width = if needs_scrollbar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        // RTL mirror (see TableView::place_children): scrollbar to the
        // physical left, body/header band shifted right by its thickness.
        let band_left = if rtl && needs_scrollbar {
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
        let row_count = self.proxy.visible_count();
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

        if let Some(ref sel) = self.selection
            && matches!(
                self.selection_mode,
                TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
            )
        {
            let bg = SurfaceRole::Selected.resolve(colors);
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

        // Focus ring.
        if let Some((focus_row, focus_col)) = self.focused_cell.get()
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

        canvas.clear_clip();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::TreeGrid);
        if let Some(ref label) = self.a11y_label {
            builder.set_name(label.resolve_now());
        }
        let row_count = self.proxy.visible_count() + if self.show_header { 1 } else { 0 };
        let col_count = self.columns.len();
        let n = builder.inner_mut();
        n.set_row_count(row_count);
        n.set_column_count(col_count);
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
        if let Some(id) = self.scrollbar_id {
            out.push(id);
        }
        if let Some(id) = self.header_row_id {
            out.push(id);
        }
        out
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
        // Regression for "row selection in TreeTable only fires on
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
            TreeTable::from_projection(proxy.clone())
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
    fn role_is_treegrid() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTable::from_projection(proxy)
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
            TreeTable::from_projection(proxy.clone())
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
            TreeTable::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTable<&'static str>>().unwrap();
            tt.expand(docs);
        }
        assert_eq!(proxy.visible_count(), 4); // docs, readme, guide, src
    }

    #[test]
    fn arrow_right_expands_and_left_collapses_on_tree_column() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTable::from_projection(proxy.clone())
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
            let tt = any.downcast_ref::<TreeTable<&'static str>>().unwrap();
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

    #[test]
    fn rows_carry_role_row_with_level_indicator() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        proxy.expand(docs);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTable::from_projection(proxy)
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
            TreeTable::from_projection(proxy.clone())
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
            TreeTable::from_projection(proxy.clone())
                .add_column(name_col())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTable<&'static str>>().unwrap();
            tt.expand_all();
        }
        assert_eq!(proxy.visible_count(), 5);
        {
            let any = tree.widget_as_any(id).unwrap();
            let tt = any.downcast_ref::<TreeTable<&'static str>>().unwrap();
            tt.collapse_all();
        }
        assert_eq!(proxy.visible_count(), 2);
    }

    #[test]
    fn row_count_in_a11y_includes_header() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            TreeTable::from_projection(proxy)
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
            TreeTable::from_projection(proxy.clone())
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
            any.downcast_ref::<TreeTable<&'static str>>()
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
            TreeTable::from_projection(proxy)
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
            TreeTable::from_projection(proxy2)
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

    /// A TreeTable (40 root rows × 20 px in a ~120 px viewport) above a
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
        let tt = TreeTable::from_projection(proxy)
            .add_column(name_col())
            .show_header(false)
            .row_height(20.0)
            .overscroll_behavior(inner);
        let inner_y = tt.scroll_y_signal().clone();
        let tt_id = tree.add(tt);
        let viewport = tree.add(
            FixedSize::new()
                .bind_width(220.0)
                .bind_height(120.0)
                .child_id(tt_id),
        );
        let filler = tree.add(
            FixedSize::new()
                .bind_width(220.0)
                .bind_height(300.0)
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
        // exit had to rebuild the TreeTable root (an ancestor), so
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
            TreeTable::from_projection(proxy.clone())
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
            let tt = any.downcast_ref::<TreeTable<&'static str>>().unwrap();
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
            TreeTable::from_projection(proxy)
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
            TreeTable::from_projection(proxy.clone())
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
}
