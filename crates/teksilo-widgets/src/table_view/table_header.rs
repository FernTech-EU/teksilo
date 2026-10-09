// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TableHeader`] — the column header strip of [`TableView`](crate::TableView)
//! and [`TreeTableView`](crate::TreeTableView), and a widget of its own for an
//! application that lays out its own rows under columns: a section list with
//! full-width group rows, an album card beside its tracks.
//!
//! Both views compose this widget, so there is one header implementation: the
//! `HeaderCell`s (label, sort indicator, filter popover, the resize grip on
//! each divider, the column-reorder drag, `Role::ColumnHeader` with its sort
//! direction and AccessKit `Increment` / `Decrement`), the strip that lays them
//! out in pinned and scrolling bands and paints their separators, and the drop
//! target that turns a reorder drag into a new column order.
//!
//! ## State
//!
//! The header's state is six signals — the width overrides, the sort, the
//! column order, the pinning overrides, the filters and the horizontal scroll
//! offset. Each builder ([`widths`](TableHeader::widths),
//! [`sort`](TableHeader::sort), [`order`](TableHeader::order),
//! [`pinning`](TableHeader::pinning), [`filters`](TableHeader::filters),
//! [`scroll_x`](TableHeader::scroll_x)) adopts an application's signal; one
//! that is not given is the header's own, read back through its getter. The
//! rules are the views' ("Column state an application owns" in the
//! [`table_view`](crate::table_view) module docs): the header writes nothing
//! into these signals until the user acts on it, ignores ids it does not
//! declare, and keeps them when it writes. Hand the same signals to a
//! `TableView` and the two stay in step.
//!
//! The width map holds **overrides**, not the layout: a column with no entry
//! takes its declared [`ColumnWidth`]. What the header actually laid each
//! column out at is [`resolved_widths_signal`](TableHeader::resolved_widths_signal),
//! which is what rows of an application's own read to line their cells up.
//!
//! ## Hosted
//!
//! Inside a view the header is *hosted* (`TableHeader::hosted`): the view
//! resolves the widths and the display order (its body needs both before the
//! header is placed), paints the `OnRelease` resize guide across the whole
//! table, rebuilds the header whenever its state changes, and keeps the drag
//! state across those rebuilds. A standalone header does all of that itself,
//! within its own bounds, which it clips to.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::SortDirection;
use teksilo_tokens::{BorderRole, SurfaceRole};

use super::body::{RowBand, SharedColumnWidths, has_pinning};
use super::column::{ColumnResizePolicy, ColumnSpec, ColumnWidth, PinnedSide};
use super::header::{
    ColumnResizeInfo, ColumnResizeTable, HeaderCell, HeaderCellSpec, ResizeStateHandle,
};
use super::imperative;
use super::layout::{self, ColumnSolver, band_rects, insertion_slot_at_x};
use super::{ColumnReorderDragData, PaneBoundaries};
use crate::styles::recipe_table_style as cp;

/// A process-unique id for one header's column-reorder drags. Every header
/// draws from this one counter — a `TableView`'s, a `TreeTableView`'s and a
/// standalone one — so a drop never takes another header's drag for one of
/// its own.
pub(crate) fn next_header_id() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The handles a header shares with whatever lays it out — all of them outlive
/// one build of the header.
///
/// A standalone header owns a set ([`HeaderLink::new`]) and fills it itself; a
/// view composing the header hands over its own (see [`TableHeader::hosted`]).
#[derive(Clone)]
pub(crate) struct HeaderLink {
    /// Disambiguates this header's reorder drags; see [`next_header_id`].
    pub table_id: usize,
    /// Resolved widths in display order — shared with a view's body rows.
    pub widths: SharedColumnWidths,
    /// Display order, as indices into the columns.
    pub display_indices: Rc<RefCell<Vec<usize>>>,
    pub pane_boundaries: Rc<RefCell<PaneBoundaries>>,
    /// The strip's width: the reorder drop mirrors its x against it under RTL,
    /// where the columns are anchored to the strip's right edge.
    pub strip_width: Rc<Cell<f32>>,
    pub resize_state: ResizeStateHandle,
    pub resize_target: Signal<Option<usize>>,
    pub resize_preview_x: Signal<Option<f32>>,
}

impl HeaderLink {
    pub(crate) fn new() -> Self {
        Self {
            table_id: next_header_id(),
            widths: Rc::new(RefCell::new(Vec::new())),
            display_indices: Rc::new(RefCell::new(Vec::new())),
            pane_boundaries: Rc::new(RefCell::new(PaneBoundaries::default())),
            strip_width: Rc::new(Cell::new(0.0)),
            resize_state: Rc::new(RefCell::new(None)),
            resize_target: Signal::new(None),
            resize_preview_x: Signal::new(None),
        }
    }
}

/// A table's column header strip: one `Role::ColumnHeader` cell per column,
/// with click-to-sort, drag-to-resize on every divider, drag-to-reorder,
/// pinned columns, the filter popover and a horizontal scroll offset to follow
/// a scrolled body.
///
/// ```ignore
/// let widths = Signal::new(HashMap::new());
/// let sort = Signal::new(None);
/// let header = TableHeader::new(vec![
///     ColumnSpec::new("title", lit!("Title")).sortable(true),
///     ColumnSpec::new("length", lit!("Length"))
///         .width(ColumnWidth::Fixed(80.0))
///         .sortable(true),
/// ])
/// .widths(widths.clone())
/// .sort(sort.clone());
/// let layout = header.resolved_widths_signal().clone(); // line rows up with this
/// ```
///
/// A header announces itself as the `Role::Row` with row index 1, its cells as
/// `Role::ColumnHeader`s, so place it inside the container that announces the
/// rows under it as a table or a grid.
///
/// See the [module docs](crate::table_view::table_header) for how its state is
/// shared and for how a view hosts it.
pub struct TableHeader {
    columns: Vec<ColumnSpec>,
    widths: Signal<HashMap<String, f32>>,
    sort: Signal<Option<(String, SortDirection)>>,
    order: Signal<Vec<String>>,
    pinning: Signal<HashMap<String, PinnedSide>>,
    filters: Signal<HashMap<String, String>>,
    scroll_x: Signal<f32>,
    resize_policy: ColumnResizePolicy,
    stretch_last_column: bool,
    /// `(id, width)` per displayed column, published by a standalone header
    /// after each layout.
    resolved: Signal<Vec<(String, f32)>>,
    link: HeaderLink,
    /// `true` when a view composes this header; see the module docs.
    hosted: bool,

    // Build state.
    cells: Vec<WidgetId>,
    /// Pane bands, built only while a column is pinned (see `BodyRow`'s
    /// module docs for why the split exists).
    bands: Option<[Option<WidgetId>; 3]>,
    boundaries: PaneBoundaries,
}

impl TableHeader {
    /// A header over `columns`, in declaration order. Every piece of state it
    /// is not handed is its own.
    pub fn new(columns: Vec<ColumnSpec>) -> Self {
        Self {
            columns,
            widths: Signal::new(HashMap::new()),
            sort: Signal::new(None),
            order: Signal::new(Vec::new()),
            pinning: Signal::new(HashMap::new()),
            filters: Signal::new(HashMap::new()),
            scroll_x: Signal::new(0.0),
            resize_policy: ColumnResizePolicy::default(),
            stretch_last_column: false,
            resolved: Signal::new(Vec::new()),
            link: HeaderLink::new(),
            hosted: false,
            cells: Vec::new(),
            bands: None,
            boundaries: PaneBoundaries::default(),
        }
    }

    /// Adopt `widths` as the map of column id → width override. A resize drag
    /// writes the resized column, and freezes each `Flex` column before it at
    /// its current width; a column with no entry takes its declared
    /// [`ColumnWidth`].
    pub fn widths(mut self, widths: Signal<HashMap<String, f32>>) -> Self {
        self.widths = widths;
        self
    }

    /// Adopt `sort` as the active sort. A click on a sortable column cycles it
    /// None → ascending → descending → None.
    pub fn sort(mut self, sort: Signal<Option<(String, SortDirection)>>) -> Self {
        self.sort = sort;
        self
    }

    /// Adopt `order` as the column order, a list of column ids; columns it
    /// does not name follow in declaration order. A reorder drop writes it.
    pub fn order(mut self, order: Signal<Vec<String>>) -> Self {
        self.order = order;
        self
    }

    /// Adopt `pinning` as the per-column pinning overrides, which win over
    /// each [`ColumnSpec::pinned`]. A reorder drop into a pinned pane writes
    /// it.
    pub fn pinning(mut self, pinning: Signal<HashMap<String, PinnedSide>>) -> Self {
        self.pinning = pinning;
        self
    }

    /// Adopt `filters` as the per-column filter text the filter popover of a
    /// [`filterable`](ColumnSpec::filterable) column edits.
    pub fn filters(mut self, filters: Signal<HashMap<String, String>>) -> Self {
        self.filters = filters;
        self
    }

    /// Follow `scroll_x`, the horizontal offset of the body under the header:
    /// the unpinned columns shift left by it, the pinned ones stay put.
    pub fn scroll_x(mut self, scroll_x: Signal<f32>) -> Self {
        self.scroll_x = scroll_x;
        self
    }

    /// Whether a resize writes its width on every pointer move (`Live`, the
    /// default) or on release (`OnRelease`, with a guide line meanwhile).
    pub fn resize_policy(mut self, policy: ColumnResizePolicy) -> Self {
        self.resize_policy = policy;
        self
    }

    /// Let the last column in display order take the width the others leave
    /// — see [`TableView::stretch_last_column`](crate::TableView::stretch_last_column).
    /// Default off.
    pub fn stretch_last_column(mut self, on: bool) -> Self {
        self.stretch_last_column = on;
        self
    }

    /// Compose this header inside a view, which owns the layout and the drag
    /// state `link` carries; see the module docs.
    pub(crate) fn hosted(mut self, link: HeaderLink) -> Self {
        self.link = link;
        self.hosted = true;
        self
    }

    /// The width overrides — the signal [`widths`](Self::widths) adopted, or
    /// the header's own.
    pub fn widths_signal(&self) -> &Signal<HashMap<String, f32>> {
        &self.widths
    }

    /// The active sort — the signal [`sort`](Self::sort) adopted, or the
    /// header's own.
    pub fn sort_signal(&self) -> &Signal<Option<(String, SortDirection)>> {
        &self.sort
    }

    /// The column order — the signal [`order`](Self::order) adopted, or the
    /// header's own.
    pub fn order_signal(&self) -> &Signal<Vec<String>> {
        &self.order
    }

    /// The pinning overrides — the signal [`pinning`](Self::pinning)
    /// adopted, or the header's own.
    pub fn pinning_signal(&self) -> &Signal<HashMap<String, PinnedSide>> {
        &self.pinning
    }

    /// The filters — the signal [`filters`](Self::filters) adopted, or the
    /// header's own.
    pub fn filters_signal(&self) -> &Signal<HashMap<String, String>> {
        &self.filters
    }

    /// The horizontal offset the header follows — the signal
    /// [`scroll_x`](Self::scroll_x) adopted, or the header's own (which
    /// stays at 0).
    pub fn scroll_x_signal(&self) -> &Signal<f32> {
        &self.scroll_x
    }

    /// `(column id, width)` for each displayed column, in display order: the
    /// widths the header laid its cells out at, after the overrides, the
    /// declared widths, the `Flex` share and the min / max clamps. Written
    /// after each layout of the header, and only when it changed; bind a row's
    /// layout to it to line the row's cells up with the columns. Leading-pinned
    /// columns come first and do not move with `scroll_x`, nor do the
    /// trailing-pinned ones at the end.
    ///
    /// Published by a standalone header only; a view hosting the header lays
    /// its rows out from the same widths directly.
    pub fn resolved_widths_signal(&self) -> &Signal<Vec<(String, f32)>> {
        &self.resolved
    }

    fn has_pinning(&self) -> bool {
        has_pinning(self.boundaries, self.cells.len())
    }

    /// The display order, from the order and pinning signals — a standalone
    /// header's own; a hosted one reads the view's.
    fn compute_display_order(&self) {
        let (display, boundaries) =
            layout::display_order(&self.columns, &self.order.get(), &self.pinning.get());
        *self.link.display_indices.borrow_mut() = display;
        *self.link.pane_boundaries.borrow_mut() = boundaries;
    }

    fn resolve_widths(&self, available: f32) -> Vec<f32> {
        ColumnSolver::resolve_in_order(
            &self.columns,
            &self.link.display_indices.borrow(),
            available,
            cp::MIN_COLUMN_WIDTH_DEFAULT,
            &self.widths.get(),
            self.stretch_last_column,
        )
    }

    /// Wire the strip as the drop target of its own cells' reorder drags:
    /// classify the drop into a pane and an insertion slot, and write the
    /// pinning and the order.
    fn reorder_drop_handlers(&self) -> HandlerSet {
        let link = self.link.clone();
        let order = self.order.clone();
        let pinning = self.pinning.clone();
        let scroll_x = self.scroll_x.clone();
        let ids: Vec<String> = self.columns.iter().map(|c| c.id.clone()).collect();
        let table_id = self.link.table_id;

        HandlerSet::new()
            .on_drag_hover(|payload, _position, _ctx| {
                if payload.has_typed::<ColumnReorderDragData>() {
                    teksilo_core::DropFeedback::HighlightRect {
                        rect: Rect::ZERO,
                        color: teksilo_tokens::Color::TRANSPARENT,
                    }
                } else {
                    teksilo_core::DropFeedback::NoFeedback
                }
            })
            .on_drop(move |mut payload, position, ctx| {
                let drag = match payload.take_typed::<ColumnReorderDragData>() {
                    Some(d) => d,
                    None => return false,
                };
                if drag.source_table_id != table_id {
                    return false;
                }
                let widths = link.widths.borrow().clone();
                let display = link.display_indices.borrow().clone();
                let panes = *link.pane_boundaries.borrow();
                let total = display.len();
                if total == 0 {
                    return false;
                }

                // `position` is local to the strip (origin at its physical-left
                // edge). Under RTL the columns are placed in display order from
                // the strip's right edge leftward, so mirror the drop x against
                // the strip width before running the left-to-right scan. (A drop
                // in any non-content dead space then maps past the last column →
                // append, matching LTR's trailing-end behaviour.)
                let drop_x = if ctx.is_rtl() {
                    link.strip_width.get() - position.x
                } else {
                    position.x
                };

                // Insertion index in display order: the first column whose
                // midpoint exceeds the (mirrored) x — pane- and scroll-aware, so
                // a drop under a nonzero `scroll_x` resolves against the columns
                // actually under the pointer, not their unscrolled positions.
                let insertion_display_idx = insertion_slot_at_x(
                    &widths,
                    panes,
                    scroll_x.get(),
                    link.strip_width.get(),
                    drop_x,
                );

                // Classify the drop position into a pane. A pane only exists
                // while a column is pinned to it: with nothing pinned the leading
                // pane is empty and the middle pane ends at the strip's end, so a
                // drop at either end of the strip is a plain move to the first /
                // last slot, not a pin — without the guards it would pin the
                // column to a pane the user never saw.
                let new_pinning = if panes.leading_count > 0
                    && insertion_display_idx <= panes.leading_count
                {
                    PinnedSide::Leading
                } else if panes.middle_end < total && insertion_display_idx >= panes.middle_end {
                    PinnedSide::Trailing
                } else {
                    PinnedSide::None
                };

                // Update the pinning override (recorded only when it deviates
                // from None, which is the framework default).
                let mut pin_map = pinning.get();
                match new_pinning {
                    PinnedSide::None => {
                        pin_map.remove(&drag.col_id);
                    }
                    other => {
                        pin_map.insert(drag.col_id.clone(), other);
                    }
                }
                pinning.set(pin_map);

                // Rebuild the column-order list to reflect the drop.
                let mut new_order: Vec<String> = display.iter().map(|&i| ids[i].clone()).collect();
                let from_pos = new_order.iter().position(|id| id == &drag.col_id);
                if let Some(from) = from_pos {
                    let item = new_order.remove(from);
                    let to = if from < insertion_display_idx {
                        insertion_display_idx.saturating_sub(1)
                    } else {
                        insertion_display_idx
                    };
                    let to = to.min(new_order.len());
                    new_order.insert(to, item);
                    order.set(merge_reordered(&order.get(), &ids, new_order));
                }
                true
            })
    }
}

impl std::fmt::Debug for TableHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableHeader")
            .field("columns", &self.columns.len())
            .field("hosted", &self.hosted)
            .field("sort", &self.sort.get())
            .finish()
    }
}

impl Widget for TableHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // `paint` draws the column separators from the widths and `scroll_x`,
        // but neither a column resize nor a horizontal scroll changes the
        // strip's own bounds — and the render walker replays a clean node's
        // cached paint. The cells move with their new bounds; only the
        // separators would stay behind. Repaint-only: geometry is a relayout's
        // business, below for a standalone header and the view's for a hosted
        // one.
        self.widths
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        self.scroll_x
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        if !self.hosted {
            // A view relays out and rebuilds a hosted header on all of these
            // itself, and owns the display order it reads.
            self.widths
                .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
            self.scroll_x
                .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
            // Each cell captures its sort direction, its filter state and its
            // display position at build, and the bands follow the pinning.
            self.sort
                .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
            self.order
                .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
            self.pinning
                .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
            self.filters
                .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
            self.link.resize_preview_x.bind_to(
                self_id,
                ctx.binding_registry(),
                BindingLevel::RepaintOnly,
            );
            self.compute_display_order();
        }

        // A resize drag that loses the window never gets its PointerUp: the
        // user Alt-Tabs (or a native dialog steals focus) with the button
        // down, releases it over another window, and the OS delivers the Up
        // nowhere. Abandon the gesture on deactivation, or the state outlives
        // it and the next bare PointerMove drags the column with no button
        // held. Nothing is committed — an interrupted drag leaves the column
        // wherever the last delivered move put it, which is what the user last
        // saw.
        {
            let resize_state = self.link.resize_state.clone();
            let resize_target = self.link.resize_target.clone();
            let resize_preview_x = self.link.resize_preview_x.clone();
            ctx.effect(&ctx.window_active_signal(), move |active| {
                if !*active && resize_state.borrow().is_some() {
                    *resize_state.borrow_mut() = None;
                    resize_target.set(None);
                    resize_preview_x.set(None);
                }
            });
        }

        // A build destroys (and re-creates) every header cell, which drops the
        // pointer capture an in-flight resize depends on. Clear the shared drag
        // state with it: a `ResizeState` that outlived its anchor would
        // otherwise let the next bare PointerMove over the same column resize
        // it with no button held.
        *self.link.resize_state.borrow_mut() = None;
        self.link.resize_target.set(None);
        self.link.resize_preview_x.set(None);

        let display = self.link.display_indices.borrow().clone();
        self.boundaries = *self.link.pane_boundaries.borrow();

        // A stretched last column has no size of its own to drag: its trailing
        // grip (and the AT step actions behind the same flag) is off. The grip
        // on its *leading* edge still resizes its predecessor.
        let stretched_slot = self
            .stretch_last_column
            .then(|| display.len().saturating_sub(1));
        let resize_columns: ColumnResizeTable = Rc::new(
            display
                .iter()
                .enumerate()
                .map(|(slot, &i)| {
                    let c = &self.columns[i];
                    ColumnResizeInfo {
                        id: c.id.clone(),
                        min_width: c.min_width.unwrap_or(cp::MIN_COLUMN_WIDTH_DEFAULT),
                        max_width: c.max_width,
                        resizable: c.resizable && stretched_slot != Some(slot),
                        flex: matches!(c.width, ColumnWidth::Flex(_)),
                    }
                })
                .collect(),
        );

        let active_sort = self.sort.get();
        let cell_padding_horizontal = crate::styles::recipe_table_style::resolve_table_style(ctx)
            .cell_padding_horizontal(&ctx.theme().input);
        // Filter zone width: indicator glyph + a small horizontal padding for
        // tap tolerance. Mirrors the layout of the HStack inside
        // `HeaderCell::build` — including its gutter, which comes from the
        // active `TableStyle` rather than from `cp::CELL_PADDING_HORIZONTAL`, so
        // the padding the cell applies and the zone the press handler measures
        // cannot name different cells.
        //
        // At every shipped gutter this is currently inert:
        // `header_cell_zones` hands the figure to `partition_targets` with a
        // floor of `target_size`, and 12 + 8 (IntUI) and 12 + 12 (Fluent) are
        // both under the 24 dp Compact floor, so the solved zone is 24 either
        // way. It stops being inert the moment a preset's gutter takes the sum
        // past the floor, which is exactly when the two numbers disagreeing
        // would show.
        let filter_zone_width = cp::FILTER_INDICATOR_SIZE + cell_padding_horizontal;
        self.cells = display
            .iter()
            .enumerate()
            .map(|(display_pos, &col_idx)| {
                let col = &self.columns[col_idx];
                let current_sort = active_sort
                    .as_ref()
                    .and_then(|(id, dir)| (id == &col.id).then_some(*dir));
                ctx.add(HeaderCell::new(HeaderCellSpec {
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
                    pane_boundaries: self.boundaries,
                    resize_columns: resize_columns.clone(),
                    resize_policy: self.resize_policy,
                    resize_state: self.link.resize_state.clone(),
                    resize_target: self.link.resize_target.clone(),
                    resize_preview_x: self.link.resize_preview_x.clone(),
                    table_id: self.link.table_id,
                    sort_signal: self.sort.clone(),
                    column_widths_signal: self.widths.clone(),
                    column_widths: self.link.widths.clone(),
                    filters_signal: self.filters.clone(),
                }))
            })
            .collect();

        ctx.apply_self_handlers(self.reorder_drop_handlers());

        if !self.has_pinning() {
            self.bands = None;
            return self.cells.clone();
        }
        let b = self.boundaries;
        let leading_end = b.leading_count.min(self.cells.len());
        let middle_end = b.middle_end.min(self.cells.len()).max(leading_end);
        let leading: Vec<WidgetId> = self.cells[..leading_end].to_vec();
        let middle: Vec<WidgetId> = self.cells[leading_end..middle_end].to_vec();
        let trailing: Vec<WidgetId> = self.cells[middle_end..].to_vec();

        let mut bands: [Option<WidgetId>; 3] = [None, None, None];
        if !leading.is_empty() {
            bands[0] = Some(ctx.add(RowBand::new(leading, self.link.widths.clone(), 0)));
        }
        if !middle.is_empty() {
            bands[1] = Some(
                ctx.add(
                    RowBand::new(middle, self.link.widths.clone(), leading_end)
                        .scrollable(self.scroll_x.clone()),
                ),
            );
        }
        if !trailing.is_empty() {
            bands[2] = Some(ctx.add(RowBand::new(trailing, self.link.widths.clone(), middle_end)));
        }
        self.bands = Some(bands);
        bands.iter().copied().flatten().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let width = proposal.width.unwrap_or_else(|| {
            if self.hosted {
                self.link.widths.borrow().iter().sum()
            } else {
                // With no width to fill, every `Flex` column sits at its floor.
                self.resolve_widths(0.0).iter().sum()
            }
        });
        let height = proposal.height.unwrap_or(cp::HEADER_HEIGHT);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if !self.hosted {
            let widths = self.resolve_widths(bounds.width);
            let resolved: Vec<(String, f32)> = self
                .link
                .display_indices
                .borrow()
                .iter()
                .zip(&widths)
                .map(|(&i, &w)| (self.columns[i].id.clone(), w))
                .collect();
            *self.link.widths.borrow_mut() = widths;
            self.link.strip_width.set(bounds.width);
            // Equality-guarded: a layout that changed nothing must not wake a
            // row bound to this.
            imperative::set_if_changed(&self.resolved, resolved);
        }

        if let Some(bands) = self.bands {
            let widths = self.link.widths.borrow();
            let rtl = ctx.is_rtl();
            let (leading_rect, middle_rect, trailing_rect) =
                band_rects(bounds, &widths, self.boundaries, rtl);
            let rects = [leading_rect, middle_rect, trailing_rect];
            let mut next = 0;
            for (band, rect) in bands.iter().zip(rects.iter()) {
                if band.is_some() {
                    if let Some(child) = children.get_mut(next) {
                        child.origin = rect.origin();
                        child.size = rect.size();
                    }
                    next += 1;
                }
            }
            return;
        }

        let widths = self.link.widths.borrow();
        let total_children = children.len();
        let fallback_w = if total_children == 0 {
            0.0
        } else {
            bounds.width / total_children as f32
        };
        let scroll = self.scroll_x.get();
        // Mirror the body: preserve display order, reverse physical x in RTL.
        if ctx.is_rtl() {
            let mut x = bounds.right() + scroll;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(i).copied().unwrap_or(fallback_w);
                x -= w;
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
            }
        } else {
            let mut x = bounds.x - scroll;
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(i).copied().unwrap_or(fallback_w);
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
                x += w;
            }
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let bg = SurfaceRole::Raised.resolve(&ctx.theme.colors);
        canvas.fill_rect(bounds, bg);

        let line = BorderRole::DividerStrong.resolve(&ctx.theme.colors);
        let dw = cp::GRID_LINE_THICKNESS.max(1.0);
        canvas.fill_rect(
            Rect::new(bounds.x, bounds.y + bounds.height - dw, bounds.width, dw),
            line,
        );

        // Column separators. Unlike the body's vertical grid lines these are
        // NOT gated on `GridLines` — in the header the separator *is* the
        // resize affordance (it is the only thing showing where the grip is),
        // so a table with `GridLines::None`/`Horizontal` — the default, and
        // what both shipped demos use — would otherwise ask the user to grab
        // an invisible divider. Every desktop table (QHeaderView, GtkTreeView,
        // NSTableHeaderView) draws them unconditionally for the same reason.
        let widths = self.link.widths.borrow();
        if widths.len() > 1 {
            let rtl =
                ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;
            let sep = BorderRole::Divider.resolve(&ctx.theme.colors);
            let (leading_rect, middle_rect, trailing_rect) =
                band_rects(bounds, &widths, self.boundaries, rtl);
            let b = self.boundaries;
            let leading_end = b.leading_count.min(widths.len());
            let middle_end = b.middle_end.min(widths.len()).max(leading_end);
            // Within-pane dividers (the Middle pane's are scroll-shifted and
            // bounded to its viewport, exactly like the body's).
            draw_band_separators(
                canvas,
                leading_rect,
                &widths[..leading_end],
                0.0,
                rtl,
                sep,
                dw,
            );
            draw_band_separators(
                canvas,
                middle_rect,
                &widths[leading_end..middle_end],
                self.scroll_x.get(),
                rtl,
                sep,
                dw,
            );
            draw_band_separators(
                canvas,
                trailing_rect,
                &widths[middle_end..],
                0.0,
                rtl,
                sep,
                dw,
            );
            // Pane seams — the boundary between the last pinned column and
            // the scrolling region. `draw_band_separators` only draws a band's
            // *internal* boundaries, so these two would otherwise be the only
            // column edges in the strip with no line.
            let mut seam = |x: f32| {
                canvas.fill_rect(Rect::new(x, bounds.y, dw, bounds.height), sep);
            };
            if leading_end > 0 {
                seam(if rtl {
                    leading_rect.x
                } else {
                    leading_rect.right() - dw
                });
            }
            if middle_end < widths.len() {
                seam(if rtl {
                    trailing_rect.right() - dw
                } else {
                    trailing_rect.x
                });
            }
        }
    }

    fn wants_post_paint(&self) -> bool {
        // A hosting view paints the guide itself, across its rows too.
        !self.hosted
    }

    /// The `OnRelease` resize guide. Under that policy no column moves until
    /// the button comes up, so this line is the *only* feedback the gesture
    /// has. Drawn over the cells, within the strip.
    fn post_paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if let Some(x) = self.link.resize_preview_x.get() {
            let thickness = cp::GRID_LINE_THICKNESS.max(1.5);
            canvas.fill_rect(
                Rect::new(x - thickness * 0.5, bounds.y, thickness, bounds.height),
                BorderRole::Focused.resolve(&ctx.theme.colors),
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Row);
        builder.set_row_index(1);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.bands {
            Some(bands) => bands.iter().copied().flatten().collect(),
            None => self.cells.clone(),
        }
    }

    fn clips_children(&self) -> bool {
        // A hosting view clips the strip with the rest of the table.
        !self.hosted
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Draw one pane band's internal column separators inside the header strip.
///
/// Deliberately *not* [`super::draw_pane_dividers`], which scissors each band:
/// the strip paints inside its view's `clips_children` scope, and
/// `Canvas::clear_clip` resets the scissor outright rather than popping a
/// stack — so borrowing that helper here would drop the table's clip for every
/// header cell painted afterwards (the walker emits the enclosing `SetClip`
/// before the children, not around each one). Separators are `line_w` wide, so
/// range-testing each against the band is equivalent to scissoring it, and
/// leaves the clip state untouched.
fn draw_band_separators(
    canvas: &mut Canvas,
    rect: Rect,
    slice: &[f32],
    scroll: f32,
    rtl: bool,
    color: teksilo_tokens::Color,
    line_w: f32,
) {
    if slice.len() < 2 || rect.width <= 0.0 {
        return;
    }
    let mut emit = |x: f32| {
        if x >= rect.x && x + line_w <= rect.right() {
            canvas.fill_rect(Rect::new(x, rect.y, line_w, rect.height), color);
        }
    };
    // Same walk (and same which-side-of-the-boundary convention) as the body's
    // vertical grid lines, so header and body seams land on the same x.
    if rtl {
        let mut x = rect.right() + scroll;
        for &w in &slice[..slice.len() - 1] {
            x -= w;
            emit(x);
        }
    } else {
        let mut x = rect.x - scroll;
        for &w in &slice[..slice.len() - 1] {
            x += w;
            emit(x - line_w);
        }
    }
}

/// Write `reordered` — this header's own columns, in their new order — back
/// over `existing`, the order signal's current list, which may also name
/// columns this header does not have.
///
/// An order signal several views share (`bind_column_order`) carries every
/// view's columns, so replacing it with the dropped-on view's own list would
/// throw the other views' arrangement away. Instead every slot that held one
/// of the `own` columns takes the next column of `reordered`, the other ids stay
/// where they were, and own columns the list did not mention yet go at the end.
/// The own columns then read in `reordered`'s order, which is all
/// `display_order` looks at.
pub(crate) fn merge_reordered(
    existing: &[String],
    own: &[String],
    reordered: Vec<String>,
) -> Vec<String> {
    let mut next = reordered.into_iter();
    let mut out = Vec::with_capacity(existing.len() + own.len());
    for id in existing {
        if own.contains(id) {
            out.extend(next.next());
        } else {
            out.push(id.clone());
        }
    }
    out.extend(next);
    out
}
