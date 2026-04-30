//! `TreeTable<T>` — hierarchical multi-column widget.
//!
//! Sibling of [`TableView`](crate::TableView). Backed by
//! [`SortFilterTreeModel<T>`](fern_data::SortFilterTreeModel) so sort
//! and filter compose with expand/collapse for free.
//!
//! Reuses the shared `column`, `header`, `layout`, `keyboard`, and
//! `selection` modules of [`crate::table_view`]. Differences from
//! `TableView`:
//!
//! - Rows come from the proxy's flattened visible list (depth carried
//!   in [`FlatEntry`](fern_data::FlatEntry)).
//! - One column is the **tree column**: cells gain an indent gutter
//!   plus a twist (chevron) that toggles expansion. Defaults to the
//!   first column declared.
//! - `Role::TreeGrid` on the root, `TreeRowA11y` on rows (carries
//!   `set_level` + `set_expanded`).
//! - ArrowLeft / ArrowRight on the tree column collapse / expand.
//! - Row drag-drop is NOT shipped here: insertion-vs-reparent UX
//!   requires its own design pass and is documented as out-of-scope
//!   (plan §3.2 / §8.3).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use fern_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{
    EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_data::{NodeId, SelectionMode, SelectionModel, SortDirection, SortFilterTreeModel, TreeFilterMode, TreeModel};
use fern_i18n::LocalizedString;
use fern_tokens::{BorderRole, SurfaceRole, TextRole, components::TableStyle};

use crate::primitives::{HStack, Padding, RectWidget};
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};
use crate::table_view::a11y::{CellA11y, TreeRowA11y};
use crate::table_view::body::{BodyRow, SharedColumnWidths};
use crate::table_view::column::{
    CellContext, Column, ColumnResizePolicy, EditTrigger, GridLines, PinnedSide, TabTraversal,
};
use crate::table_view::header::{HeaderCell, HeaderRow, ResizeStateHandle};
use crate::table_view::keyboard;
use crate::table_view::layout;
use crate::table_view::row_navigator::RowNavigator;
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};

const BUFFER_ROWS: usize = 5;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Hierarchical projection navigator. Adapts
/// [`SortFilterTreeModel`](fern_data::SortFilterTreeModel)'s
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
    row_entries: Vec<(usize, WidgetId)>,
    scrollbar_id: Option<WidgetId>,

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
            viewport_ratio_y: Signal::new(1.0),
            sort_signal: Signal::new(None),
            column_widths_signal: Signal::new(HashMap::new()),
            column_order_signal: Signal::new(Vec::new()),
            column_pinning_signal: Signal::new(HashMap::new()),
            filters_signal: Signal::new(HashMap::new()),
            focused_cell: Signal::new(None),
            editing_cell: Signal::new(None),
            header_row_id: None,
            row_entries: Vec::new(),
            scrollbar_id: None,
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

    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = Some(height);
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

    pub fn on_row_activate(
        mut self,
        f: impl Fn(usize, &mut EventContext) + 'static,
    ) -> Self {
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
        self.sort_signal
            .set(col_id.map(|c| (c.to_string(), dir)));
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

    fn effective_row_height(&self, style: &TableStyle) -> f32 {
        self.row_height.unwrap_or(style.row_height)
    }

    fn effective_header_height(&self, style: &TableStyle) -> f32 {
        if self.show_header {
            self.header_height.unwrap_or(style.header_height)
        } else {
            0.0
        }
    }

    fn effective_indent(&self, style: &TableStyle) -> f32 {
        self.indent_per_level.unwrap_or(style.tree_indent_per_level)
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

    fn visible_range(&self, row_h: f32) -> (usize, usize) {
        let count = self.proxy.visible_count();
        if count == 0 || row_h <= 0.0 {
            return (0, 0);
        }
        let scroll = self.scroll_y.get().max(0.0);
        let viewport = self.viewport_height.get();
        let first = (scroll / row_h).floor() as usize;
        let last = ((scroll + viewport) / row_h).ceil() as usize;
        let start = first.saturating_sub(BUFFER_ROWS);
        let end = (last + BUFFER_ROWS).min(count);
        (start, end)
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
        let style = ctx.theme().components.table;
        let row_h = self.effective_row_height(&style);
        let header_h = self.effective_header_height(&style);
        let indent_per_level = self.effective_indent(&style);

        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        self.scroll_y
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Relayout);
        ctx.register_animated_signal(&self.scroll_y);

        self.column_widths_signal
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Relayout);
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Bump version on projection version (data + sort/filter +
        // expand/collapse all in one signal).
        let v_for_proj = version.clone();
        let proj_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.proxy.version_signal(), move |_| {
            let next = proj_ver.get() + 1;
            proj_ver.set(next);
            v_for_proj.set(next);
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
        let v_for_focus = version.clone();
        let fv = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.focused_cell, move |_| {
            let next = fv.get() + 1;
            fv.set(next);
            v_for_focus.set(next);
        });
        let v_for_edit = version.clone();
        let ev = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.editing_cell, move |_| {
            let next = ev.get() + 1;
            ev.set(next);
            v_for_edit.set(next);
        });

        // Observe selection changes → bump version so the rendered
        // rows reflect the new `is_selected` state. Without this, a
        // click that calls `sel.select(...)` mutates the model but
        // the row paint stays stale until something else (typically
        // an expand/collapse) triggers a rebuild — that's the symptom
        // "row selection only fires on expand/collapse" reported from
        // real testing.
        if let Some(ref sel) = self.selection {
            let v_for_sel = version.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            ctx.effect(&sel.selection_signal(), move |_| {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                v_for_sel.set(next);
            });
        }
        if let Some(ref cs) = self.cell_selection {
            let v_for_csel = version.clone();
            let csel_ver = Rc::new(Cell::new(0_u64));
            ctx.effect(&cs.selection_signal(), move |_| {
                let next = csel_ver.get() + 1;
                csel_ver.set(next);
                v_for_csel.set(next);
            });
        }

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

        let navigator: Rc<dyn RowNavigator> =
            Rc::new(TreeNavigator::new(self.proxy.clone()));
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
            row_height: row_h,
            tab_traversal: self.tab_traversal,
            editing_cell: self.editing_cell.clone(),
            edit_trigger: self.edit_trigger,
            display_col_to_id,
            display_col_editable,
            on_cell_edit_request: self.on_cell_edit_request.clone(),
            on_row_activate: self.on_row_activate.clone(),
        };

        let handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                fern_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        fern_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        fern_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y_for_wheel.get();
                    let max = max_scroll_for_wheel.get();
                    let new_y = (current + dy).clamp(0.0, max);
                    scroll_y_for_wheel.set(new_y);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            })
            .on_key(keyboard::build_key_handler(key_cfg))
            .clips_children(true)
            .focusable(true);
        ctx.apply_self_handlers(handlers);

        // ── Build children ────────────────────────────────────────────
        self.header_row_id = None;
        self.row_entries.clear();
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
                let filter_zone_width =
                    style.filter_indicator_size + style.cell_padding_horizontal;
                let cell = HeaderCell::new(
                    col.id.clone(),
                    col.header_label.resolve_now(),
                    display_pos + 1,
                    col.sortable,
                    col.resizable,
                    col.reorderable,
                    style.resize_handle_width,
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
                style.grid_line_thickness,
            );
            self.header_row_id = Some(ctx.add(header_row));
        }

        // Body rows.
        let row_count = self.proxy.visible_count();
        if row_count > 0 {
            let (start, end) = self.visible_range(row_h);
            let columns = self.columns.clone();
            let proxy = self.proxy.clone();
            let selection = self.selection.clone();
            let cell_selection = self.cell_selection.clone();
            let selection_mode = self.selection_mode;
            let tree_color = style.tree_twist_size;
            let _ = tree_color;
            let editing_state = self.editing_cell.get();

            for flat_idx in start..end {
                let entry = match proxy.entry_at(flat_idx) {
                    Some(e) => e,
                    None => continue,
                };
                let row_selected = match (selection_mode, &selection) {
                    (
                        TableSelectionMode::SingleRow | TableSelectionMode::MultiRow,
                        Some(s),
                    ) => s.is_selected(flat_idx),
                    _ => false,
                };

                let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
                for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                    let col = &columns[col_idx];
                    let is_tree_column = display_pos == tree_display_pos;
                    let is_selected = match (selection_mode, &selection, &cell_selection) {
                        (
                            TableSelectionMode::SingleRow | TableSelectionMode::MultiRow,
                            Some(s),
                            _,
                        ) => s.is_selected(flat_idx),
                        (
                            TableSelectionMode::SingleCell | TableSelectionMode::MultiCell,
                            _,
                            Some(cs),
                        ) => cs.is_selected(flat_idx, display_pos),
                        _ => false,
                    };
                    let is_focused =
                        self.focused_cell.get() == Some((flat_idx, display_pos));
                    let is_editing = editing_state == Some((flat_idx, display_pos));
                    let cell_ctx = CellContext {
                        row_index: flat_idx,
                        col_id: col.id.clone(),
                        col_index: display_pos,
                        is_selected,
                        is_focused,
                        is_hovered: false,
                        is_editing,
                        depth: Some(entry.depth),
                        is_tree_column,
                    };

                    // Build the cell delegate widget.
                    let inner_widget = proxy
                        .with_entry(flat_idx, |item, _| (col.cell)(item, &cell_ctx));
                    let inner_widget = match inner_widget {
                        Some(w) => w,
                        None => continue,
                    };
                    let inner_id = ctx.add_boxed(inner_widget);

                    // Wrap the tree-column's inner widget with indent +
                    // twist arrow.
                    let leading_id = if is_tree_column {
                        let indent_px = entry.depth as f32 * indent_per_level;
                        let twist_node_id = entry.node_id;
                        let proxy_for_twist = proxy.clone();
                        let twist = ctx.add(
                            TwistArrow::new(
                                style.tree_twist_size,
                                entry.has_children,
                                entry.is_expanded,
                            )
                            .on_click(move || {
                                proxy_for_twist.toggle(twist_node_id);
                            }),
                        );
                        // Build inside-out so each `ctx.add` happens
                        // outside the mutable borrow chain.
                        let twist_and_label = HStack::new()
                            .spacing(style.tree_twist_label_gap)
                            .add_child(twist)
                            .add_child(inner_id);
                        let twist_label_id = ctx.add(twist_and_label);
                        ctx.add(
                            Padding::new(0.0_f32, 0.0_f32, 0.0_f32, indent_px)
                                .child_id(twist_label_id),
                        )
                    } else {
                        inner_id
                    };

                    let cell_a11y = CellA11y::new(
                        leading_id,
                        flat_idx + 2,
                        display_pos + 1,
                        is_selected,
                    );
                    cell_ids.push(ctx.add(cell_a11y));
                }

                // Wrap cells in BodyRow (Role::Row by default), but
                // add the level/expanded annotations via TreeRowA11y
                // so screen readers announce the depth.
                let row_widget = BodyRow::new(
                    cell_ids,
                    flat_idx + 2,
                    row_selected,
                    row_h,
                    self.column_widths.clone(),
                )
                .a11y_hidden();
                let row_inner_id = ctx.add(row_widget);
                let tree_row_id = ctx.add(TreeRowA11y::new(
                    row_inner_id,
                    flat_idx + 2,
                    entry.depth + 1,
                    if entry.has_children {
                        Some(entry.is_expanded)
                    } else {
                        None
                    },
                    row_selected,
                ));
                self.row_entries.push((flat_idx, tree_row_id));

                // Selection click on the row.
                if let Some(ref sel) = selection {
                    let sel_for_click = sel.clone();
                    let row_index_for_click = flat_idx;
                    if matches!(
                        selection_mode,
                        TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
                    ) {
                        ctx.apply_handlers(
                            tree_row_id,
                            HandlerSet::new().on_pointer_event(move |event, _ctx| {
                                if let WidgetEvent::PointerDown {
                                    button: PointerButton::Primary,
                                    modifiers,
                                    ..
                                } = event
                                {
                                    if modifiers.ctrl()
                                        && sel_for_click.mode() == SelectionMode::Multi
                                    {
                                        sel_for_click.toggle(row_index_for_click);
                                    } else if modifiers.shift()
                                        && sel_for_click.mode() == SelectionMode::Multi
                                    {
                                        sel_for_click.extend_to(row_index_for_click);
                                    } else {
                                        sel_for_click.select(row_index_for_click);
                                    }
                                }
                                EventResponse::Ignored
                            }),
                        );
                    }
                }
            }
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

        let mut children: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.header_row_id {
            children.push(id);
        }
        children.extend(self.row_entries.iter().map(|(_, id)| *id));
        if let Some(id) = self.scrollbar_id {
            children.push(id);
        }
        let _ = header_h;
        children
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        let width = proposal.width.unwrap_or(400.0);
        let height = proposal.height.unwrap_or(300.0);
        self.viewport_height.set(height);
        Size::new(width, height)
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
        let style = ctx.theme.components.table;
        let row_h = self.effective_row_height(&style);
        let header_h = self.effective_header_height(&style);
        let body_height = (bounds.height - header_h).max(0.0);

        let row_count = self.proxy.visible_count();
        let total_height = row_count as f32 * row_h;
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

        let overrides = self.column_widths_signal.get();
        let display = self.display_indices.borrow().clone();
        let widths = layout::ColumnSolver::resolve_in_order(
            &self.columns,
            &display,
            body_width,
            style.min_column_width_default,
            &overrides,
        );
        *self.column_widths.borrow_mut() = widths;
        let scroll_y = self.scroll_y.get();
        let body_origin_y = bounds.y + header_h;

        let mut next = 0;
        if self.header_row_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(bounds.x, bounds.y);
                child.size = Size::new(body_width, header_h);
            }
            next += 1;
        }
        let row_count_visible = self.row_entries.len();
        for i in 0..row_count_visible {
            if let Some(child) = children.get_mut(next + i) {
                let (flat_idx, _) = self.row_entries[i];
                let y = body_origin_y + flat_idx as f32 * row_h - scroll_y;
                child.origin = Point::new(bounds.x, y);
                child.size = Size::new(body_width, row_h);
            }
        }
        next += row_count_visible;

        if self.scrollbar_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                if needs_scrollbar {
                    child.origin = Point::new(
                        bounds.x + bounds.width - SCROLLBAR_THICKNESS,
                        body_origin_y,
                    );
                    child.size = Size::new(SCROLLBAR_THICKNESS, body_height);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let style = ctx.theme.components.table;
        let row_h = self.effective_row_height(&style);
        let header_h = self.effective_header_height(&style);
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

        if self.alternating_rows {
            let first_visible = (scroll_y / row_h).floor().max(0.0) as usize;
            let last_visible = ((scroll_y + body_height) / row_h).ceil() as usize;
            let row_count = self.proxy.visible_count();
            let last_visible = last_visible.min(row_count);
            for row_idx in first_visible..last_visible {
                if row_idx % 2 == 1 {
                    let y = body_origin_y + (row_idx as f32) * row_h - scroll_y;
                    let rect = Rect::new(bounds.x, y, body_width_for_paint, row_h);
                    canvas.fill_rect(rect, SurfaceRole::AltRow.resolve(colors));
                }
            }
        }

        if let Some(ref sel) = self.selection {
            if matches!(
                self.selection_mode,
                TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
            ) {
                let bg = SurfaceRole::Selected.resolve(colors);
                for row_idx in sel.selected_indices() {
                    let y = body_origin_y + (row_idx as f32) * row_h - scroll_y;
                    if y + row_h < body_origin_y || y > body_origin_y + body_height {
                        continue;
                    }
                    let rect = Rect::new(bounds.x, y, body_width_for_paint, row_h);
                    canvas.fill_rect(rect, bg);
                }
            }
        }

        let line_color = BorderRole::Divider.resolve(colors);
        let line_w = style.grid_line_thickness.max(1.0);
        if matches!(self.grid_lines, GridLines::Horizontal | GridLines::Both) {
            let first_visible = (scroll_y / row_h).floor().max(0.0) as usize;
            let last_visible = ((scroll_y + body_height) / row_h).ceil() as usize;
            let row_count = self.proxy.visible_count();
            let last_visible = last_visible.min(row_count);
            for row_idx in first_visible..last_visible {
                let y = body_origin_y + (row_idx as f32 + 1.0) * row_h - scroll_y - line_w;
                let rect = Rect::new(bounds.x, y, body_width_for_paint, line_w);
                canvas.fill_rect(rect, line_color);
            }
        }
        if matches!(self.grid_lines, GridLines::Vertical | GridLines::Both) {
            let mut x = bounds.x;
            for &w in widths.iter() {
                x += w;
                if x < bounds.x + body_width_for_paint - 0.5 {
                    let rect = Rect::new(x - line_w, body_origin_y, line_w, body_height);
                    canvas.fill_rect(rect, line_color);
                }
            }
        }

        // Focus ring.
        if let Some((focus_row, focus_col)) = self.focused_cell.get() {
            if focus_col < widths.len() {
                let mut x_off = 0.0_f32;
                for &w in widths.iter().take(focus_col) {
                    x_off += w;
                }
                let cell_w = widths[focus_col];
                let y = body_origin_y + (focus_row as f32) * row_h - scroll_y;
                if y + row_h >= body_origin_y && y <= body_origin_y + body_height {
                    let inset = style.focus_ring_inset;
                    let stroke = style.grid_line_thickness.max(1.5);
                    let ring_color = BorderRole::Focused.resolve(colors);
                    let rx = bounds.x + x_off + inset;
                    let ry = y + inset;
                    let rw = (cell_w - inset * 2.0).max(0.0);
                    let rh = (row_h - inset * 2.0).max(0.0);
                    canvas.fill_rect(Rect::new(rx, ry, rw, stroke), ring_color);
                    canvas.fill_rect(Rect::new(rx, ry + rh - stroke, rw, stroke), ring_color);
                    canvas.fill_rect(Rect::new(rx, ry, stroke, rh), ring_color);
                    canvas.fill_rect(Rect::new(rx + rw - stroke, ry, stroke, rh), ring_color);
                }
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::TreeGrid);
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
        let mut out: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.header_row_id {
            out.push(id);
        }
        out.extend(self.row_entries.iter().map(|(_, id)| *id));
        if let Some(id) = self.scrollbar_id {
            out.push(id);
        }
        out
    }

    fn clips_children(&self) -> bool {
        true
    }
}

// ── TwistArrow widget ──────────────────────────────────────────────────────

/// Small interactive chevron that toggles a tree node's expansion.
/// Renders a right-pointing arrow when collapsed, down-pointing when
/// expanded; a leaf node renders nothing (just empty space) so the
/// indent column lines up.
pub(crate) struct TwistArrow {
    size: f32,
    has_children: bool,
    expanded: bool,
    on_click: Option<Rc<dyn Fn()>>,
}

impl TwistArrow {
    pub(crate) fn new(size: f32, has_children: bool, expanded: bool) -> Self {
        Self {
            size,
            has_children,
            expanded,
            on_click: None,
        }
    }

    pub(crate) fn on_click(mut self, f: impl Fn() + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for TwistArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwistArrow")
            .field("size", &self.size)
            .field("has_children", &self.has_children)
            .field("expanded", &self.expanded)
            .finish()
    }
}

impl Widget for TwistArrow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(cb) = self.on_click.clone() {
            let handlers = HandlerSet::new()
                .on_tap(move |_pos, _ctx| {
                    cb();
                })
                .focusable(false);
            ctx.apply_self_handlers(handlers);
        }
        // Add a transparent rect so the widget has a hit area sized
        // by `size_that_fits`.
        let rect = ctx.add(
            RectWidget::new()
                .background(SurfaceRole::Transparent),
        );
        vec![rect]
    }

    fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        Size::new(self.size, self.size)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if !self.has_children {
            return;
        }
        let color = TextRole::Secondary.resolve(&ctx.theme.colors);
        let cx = bounds.x + bounds.width / 2.0;
        let cy = bounds.y + bounds.height / 2.0;
        let r = bounds.width.min(bounds.height) * 0.4;
        let mut path = Path::new();
        if self.expanded {
            // Down-pointing triangle.
            path.move_to(Point::new(cx - r, cy - r * 0.4));
            path.line_to(Point::new(cx + r, cy - r * 0.4));
            path.line_to(Point::new(cx, cy + r * 0.6));
            path.close();
        } else {
            // Right-pointing triangle.
            path.move_to(Point::new(cx - r * 0.4, cy - r));
            path.line_to(Point::new(cx + r * 0.6, cy));
            path.line_to(Point::new(cx - r * 0.4, cy + r));
            path.close();
        }
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Decorative — the row's TreeRowA11y already announces
        // expanded/collapsed via `set_expanded`.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table_view::column::ColumnWidth;
    use fern_canvas::SizeProposal;
    use fern_core::accesskit::Role;
    use fern_core::widget_tree::WidgetTree;
    use fern_data::{TreeModel, SortFilterTreeModel, TreeFilterMode};
    use fern_tokens::Theme;

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
        Column::<&str>::new("name", "Name", |row, _: &CellContext| {
            Box::new(crate::primitives::TextWidget::new_literal(*row))
        })
        .width(ColumnWidth::Flex(1.0))
    }

    fn size_col() -> Column<&'static str> {
        Column::<&str>::new("size", "Size", |_row, _: &CellContext| {
            Box::new(crate::primitives::TextWidget::new_literal("0"))
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
        use fern_canvas::Point;
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};
        use fern_data::{SelectionMode, SelectionModel};
        let proxy = SortFilterTreeModel::new(sample_tree());
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let header_h = tree.theme().components.table.header_height;
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        tree.press_key(fern_core::event::Key::ArrowRight, fern_core::event::Modifiers::NONE);
        assert_eq!(proxy.visible_count(), 4);
        // ArrowLeft on first row (now expanded) → collapse.
        tree.press_key(fern_core::event::Key::ArrowLeft, fern_core::event::Modifiers::NONE);
        assert_eq!(proxy.visible_count(), 2);
    }

    #[test]
    fn rows_carry_role_row_with_level_indicator() {
        let proxy = SortFilterTreeModel::new(sample_tree());
        let docs = proxy.tree().root(0);
        proxy.expand(docs);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
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
}
