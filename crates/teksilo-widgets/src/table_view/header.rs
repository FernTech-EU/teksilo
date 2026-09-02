// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Sticky header strip + per-column header cells.
//!
//! `HeaderCell` lays out as
//! `Padding → HStack { TextWidget(label), Spacer, SortIndicator? }` and
//! handles click-to-sort plus drag-to-resize on the grip that straddles
//! *either* of the two column dividers it touches (see the type's docs).
//! `HeaderRow` lays its cells horizontally using the same shared
//! `column_widths` handle that body rows consume — so a resize commits
//! in one place and reflows everywhere — and paints the column separators
//! that make those grips findable.
//!
//! Supports sort, resize, reuse across pinned panes, per-column filter
//! popovers, and column-reorder drag.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::drag_payload::DragPayload;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    SharedTableStyle, SortDirection as StyleSortDirection, TableHeaderCellConfig,
};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::SortDirection;
use teksilo_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

/// Convert the data-layer `SortDirection` (teksilo-data) to the
/// styles-layer one (teksilo-core::styles). They share the same shape
/// but are distinct types because teksilo-core cannot depend on
/// teksilo-data.
fn style_sort(d: SortDirection) -> StyleSortDirection {
    match d {
        SortDirection::Ascending => StyleSortDirection::Ascending,
        SortDirection::Descending => StyleSortDirection::Descending,
    }
}

use crate::primitives::{HStack, Padding, Spacer, TextWidget};

use super::ColumnReorderDragData;
use super::PaneBoundaries;
use super::body::{RowBand, SharedColumnWidths};
use super::column::{ColumnResizePolicy, PinnedSide};
use super::filter::FilterIndicator;
use super::filter::FilterPopoverContent;
use super::layout::{band_rects, insertion_slot_at_x};
use crate::overlay_trigger::OverlayTrigger;
use crate::popover_widget::PopoverWidget;
use teksilo_core::overlay::OverlayPlacement;

const DRAG_REORDER_THRESHOLD: f32 = 5.0;

/// Per-column resize metadata, in **display order** — the table a
/// [`HeaderCell`] consults to resolve a grabbed divider into the column it
/// actually resizes, and to clamp the committed width to exactly the bounds
/// [`ColumnSolver::resolve_in_order`](super::layout::ColumnSolver::resolve_in_order)
/// will re-apply to the override.
///
/// A cell owns *two* dividers (the one at its trailing edge and the one at
/// its leading edge, shared with its predecessor), so it cannot resize from
/// its own column's declaration alone — hence a shared table rather than
/// per-cell scalars.
#[derive(Debug, Clone)]
pub(crate) struct ColumnResizeInfo {
    /// Stable column id — the key `column_widths_signal` is written under.
    pub id: String,
    /// Floor a resize can't push this column below — the column's own
    /// `min_width` if declared, else the table's `min_column_width_default`.
    pub min_width: f32,
    /// Ceiling, if the column declared one.
    pub max_width: Option<f32>,
    /// Whether the column opted into drag-resize at all.
    pub resizable: bool,
}

/// Shared, display-ordered [`ColumnResizeInfo`] table. Rebuilt (and re-shared
/// into every `HeaderCell`) on each view rebuild, which is also when the
/// display order can change — so an index into it is always current.
pub(crate) type ColumnResizeTable = Rc<Vec<ColumnResizeInfo>>;

/// Which pane a display slot belongs to (0 = Leading-pinned, 1 = Middle /
/// scrollable, 2 = Trailing-pinned). Only used to decide whether two adjacent
/// slots share a *column* divider or a *pane seam* — a seam is not a resize
/// boundary, because under a nonzero `scroll_x` the column on its far side is
/// not the one visually adjacent to it.
fn pane_of(slot: usize, b: PaneBoundaries) -> u8 {
    if slot < b.leading_count {
        0
    } else if slot < b.middle_end {
        1
    } else {
        2
    }
}

/// Draw one pane band's internal column separators inside the header strip.
///
/// Deliberately *not* [`super::draw_pane_dividers`], which scissors each band:
/// `HeaderRow` paints inside the table's own `clips_children` scope, and
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

/// Clamp a dragged width to the same `[min, max]` window the column solver
/// re-applies to the stored override. Keeping the two in lock-step is what
/// stops `column_widths_signal` — a *public* handle apps read back and
/// persist — from holding a width the table never renders.
fn clamp_width(w: f32, min: f32, max: Option<f32>) -> f32 {
    w.max(min).min(max.unwrap_or(f32::INFINITY))
}

/// Active resize state for one column. Anchored at PointerDown,
/// advanced on PointerMove, committed (and cleared) on PointerUp.
#[derive(Debug, Clone)]
pub(crate) struct ResizeState {
    /// Id of the column being resized — not necessarily the cell that owns
    /// the gesture, since grabbing a cell's *leading* edge resizes its
    /// predecessor (see [`HeaderCell`]'s grip docs).
    pub col_id: String,
    /// Display slot of the cell that took the pointer capture. Only that
    /// cell may advance or commit the drag; a mismatch means the event
    /// reached the wrong cell (capture lost) and must not move a column.
    pub anchor_index: usize,
    /// Pointer x at PointerDown, in **window** coordinates. Window-space
    /// (not cell-local) so the delta stays stable across the relayouts a
    /// Live-policy resize triggers: under RTL a widening column's
    /// physical-left edge — and thus the cell's local origin — moves
    /// mid-drag, so a cell-local anchor would drift. Window x doesn't.
    pub start_pointer_x: f32,
    /// Target column's width at PointerDown (in pixels).
    pub start_width: f32,
    /// Window x of the grabbed divider at PointerDown — the anchor the
    /// `OnRelease` preview line is offset from.
    pub start_divider_x: f32,
    /// Resolved floor / ceiling of the target column, snapshotted so the
    /// commit path needs no second lookup.
    pub min_width: f32,
    pub max_width: Option<f32>,
}

pub(crate) type ResizeStateHandle = Rc<RefCell<Option<ResizeState>>>;

/// Press state recorded on PointerDown in the label region of a
/// HeaderCell. If the pointer moves past `DRAG_REORDER_THRESHOLD`
/// before PointerUp, the cell starts a reorder drag; otherwise the
/// cell cycles its sort on PointerUp.
#[derive(Debug, Clone, Copy)]
struct PressState {
    pointer_x: f32,
    pointer_y: f32,
}

/// Everything one [`HeaderCell`] needs, gathered into a struct because the
/// positional constructor had outgrown readability (and clippy's
/// `too_many_arguments`) long before the resize grip needed three more
/// fields. Both `TableView` and `TreeTableView` fill this in the same shape.
pub(crate) struct HeaderCellSpec {
    pub col_id: String,
    pub label: String,
    pub col_index_1based: usize,
    pub sortable: bool,
    pub reorderable: bool,
    pub filterable: bool,
    /// Half-width of the resize grip: the grabbable band extends this far on
    /// **each** side of a column divider (the Qt `PM_HeaderGripMargin`
    /// convention), not just inside the cell that owns the divider.
    pub resize_grip: f32,
    /// Width of the trailing region reserved for the filter popover trigger
    /// (glyph + padding). Ignored when `filterable` is false.
    pub filter_zone_width: f32,
    pub current_sort: Option<SortDirection>,
    pub width_index: usize,
    pub pane_boundaries: PaneBoundaries,
    pub resize_columns: ColumnResizeTable,
    pub resize_policy: ColumnResizePolicy,
    pub resize_state: ResizeStateHandle,
    /// Display slot of the column under an active resize, or `None`. Shared
    /// view-wide: the *target* cell derives its `is_resizing` chrome from it,
    /// which is not always the cell holding the capture.
    pub resize_target: Signal<Option<usize>>,
    /// Window x of the prospective divider while an `OnRelease` drag is in
    /// flight, or `None`. The view paints it as a guide line — without it
    /// `OnRelease` gives the user no feedback at all until the button comes up.
    pub resize_preview_x: Signal<Option<f32>>,
    pub table_id: usize,
    pub sort_signal: Signal<Option<(String, SortDirection)>>,
    pub column_widths_signal: Signal<HashMap<String, f32>>,
    pub column_widths: SharedColumnWidths,
    pub filters_signal: Signal<HashMap<String, String>>,
}

/// One header cell — label, optional sort indicator, click-to-sort,
/// drag-to-resize on either of the two dividers it touches.
///
/// ## The resize grip
///
/// A column divider is a boundary *between* two cells, so the grabbable band
/// straddles it: `resize_grip` px inside the cell on each side. Consequently
/// a cell claims two zones — its reading-order **trailing** edge (which
/// resizes *this* column) and its **leading** edge (which resizes its
/// **predecessor**, whose trailing edge that same divider is). Claiming only
/// the former, as this widget originally did, left the outer half of every
/// divider owned by the next cell's label region: a grab that missed by one
/// pixel cycled the sort or started a column-reorder drag instead of
/// resizing.
///
/// Under RTL the display order runs right-to-left, so "reading-order
/// trailing" is the cell's physical-**left** edge and the two zones swap
/// sides; the drag sign inverts with them (see `on_pointer_event`).
///
/// The leading zone is suppressed when the predecessor sits in a different
/// pane: that boundary is a pane *seam*, and under a nonzero `scroll_x` the
/// column on its far side is not the one visually adjacent to it.
pub(crate) struct HeaderCell {
    col_id: String,
    label: String,
    col_index_1based: usize,
    sortable: bool,
    reorderable: bool,
    /// Half-width of the resize grip — see the type docs.
    resize_grip: f32,
    /// Sort direction for *this* column, or `None` if it isn't the
    /// active sort column. Captured at build time and reflected in the
    /// AccessKit node + the chevron child.
    current_sort: Option<SortDirection>,
    sort_signal: Signal<Option<(String, SortDirection)>>,
    column_widths_signal: Signal<HashMap<String, f32>>,
    /// Live resolved widths, shared with the row layout. Read at
    /// PointerDown to record the starting width of whichever column the
    /// grabbed divider belongs to.
    column_widths: SharedColumnWidths,
    /// Index of this column in the resolved-widths vector.
    width_index: usize,
    /// Pane partition, so the leading grip can be suppressed across a seam.
    pane_boundaries: PaneBoundaries,
    /// Display-ordered resize metadata for **all** columns — this cell needs
    /// its predecessor's floor/ceiling too.
    resize_columns: ColumnResizeTable,
    resize_policy: ColumnResizePolicy,
    resize_state: ResizeStateHandle,
    resize_target: Signal<Option<usize>>,
    resize_preview_x: Signal<Option<f32>>,
    /// Stable id of the owning TableView, propagated into the reorder
    /// drag payload so inter-table drops are rejected.
    table_id: usize,
    /// This cell's window-space leading edge, written by `place_children`
    /// and read by `on_pointer_event` to translate the window-coord
    /// pointer position into cell-local coords. Without this, the
    /// trailing-edge resize-zone test (`local_x > cell_w - resize_zone`)
    /// would compare against a window x that's always huge for any
    /// column past the first one, firing resize from the wrong region.
    cell_window_x: Rc<Cell<f32>>,
    /// This cell's placed width, written by `place_children`. The grip test
    /// runs against the geometry actually on screen rather than against the
    /// shared widths vector, so the two can never disagree (they do when the
    /// vector is shorter than the cell list and `HeaderRow` falls back to an
    /// even split).
    cell_window_w: Rc<Cell<f32>>,
    /// This cell's resolved height, written by `place_children` and read
    /// by `on_pointer_event` to reject pointer events that bubble up from
    /// the in-tree filter popover (a descendant overlay anchored below the
    /// cell). Without it the x-only resize-zone test paints a `ColResize`
    /// cursor across the whole popover.
    cell_window_h: Rc<Cell<f32>>,
    /// `true` when the column's `filterable` flag is set. Drives the
    /// filter popover affordance and a "leave-this-region-alone" zone
    /// in the cell's pointer handler so PointerDown over the popover
    /// trigger reaches the trigger instead of being eaten by sort.
    filterable: bool,
    /// Width of the trailing region reserved for the filter popover
    /// trigger (glyph + padding). Zero when `filterable` is false.
    filter_zone_width: f32,
    /// Live per-column filter map. The popover edits this signal in
    /// place via `set_filter` semantics — empty string removes the
    /// entry, non-empty string inserts/replaces it.
    filters_signal: Signal<HashMap<String, String>>,
    /// `true` while the pointer is inside the cell — drives the
    /// `Hover` overlay supplied by `TableStyle::make_header_cell`.
    /// Toggled by the cell's `on_hover` handler.
    is_hovered: Signal<bool>,
    /// `true` while **this column** is the one being resized — drives the
    /// `Pressed` overlay supplied by `TableStyle::make_header_cell`. Derived
    /// from the shared `resize_target` rather than set locally, so the
    /// highlight follows the column that moves even when the gesture is
    /// anchored on its neighbour's leading grip.
    is_resizing: Signal<bool>,

    // Build state
    root_child_id: Option<WidgetId>,
}

impl HeaderCell {
    pub(crate) fn new(spec: HeaderCellSpec) -> Self {
        let width_index = spec.width_index;
        let is_resizing = spec.resize_target.map(move |t| *t == Some(width_index));
        Self {
            col_id: spec.col_id,
            label: spec.label,
            col_index_1based: spec.col_index_1based,
            sortable: spec.sortable,
            reorderable: spec.reorderable,
            resize_grip: spec.resize_grip,
            current_sort: spec.current_sort,
            sort_signal: spec.sort_signal,
            column_widths_signal: spec.column_widths_signal,
            column_widths: spec.column_widths,
            width_index,
            pane_boundaries: spec.pane_boundaries,
            resize_columns: spec.resize_columns,
            resize_policy: spec.resize_policy,
            resize_state: spec.resize_state,
            resize_target: spec.resize_target,
            resize_preview_x: spec.resize_preview_x,
            table_id: spec.table_id,
            cell_window_x: Rc::new(Cell::new(0.0)),
            cell_window_w: Rc::new(Cell::new(0.0)),
            cell_window_h: Rc::new(Cell::new(0.0)),
            filterable: spec.filterable,
            filter_zone_width: if spec.filterable {
                spec.filter_zone_width
            } else {
                0.0
            },
            filters_signal: spec.filters_signal,
            is_hovered: Signal::new(false),
            is_resizing,
            root_child_id: None,
        }
    }
}

impl std::fmt::Debug for HeaderCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderCell")
            .field("col_id", &self.col_id)
            .field("label", &self.label)
            .field("sortable", &self.sortable)
            .field(
                "resizable",
                &self
                    .resize_columns
                    .get(self.width_index)
                    .map(|c| c.resizable)
                    .unwrap_or(false),
            )
            .field("current_sort", &self.current_sort)
            .finish()
    }
}

impl Widget for HeaderCell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_table_style as cp;

        let label_id = ctx.add(
            TextWidget::new(lit!(self.label.clone()))
                .style(TextStyleRole::Body)
                .color(ColorProp::from(TextRole::Primary))
                .single_line()
                .a11y_hidden(),
        );

        let mut row = HStack::new()
            .spacing(4.0)
            .add_child(label_id)
            .add_child(ctx.add(Spacer::new()));

        if self.current_sort.is_some() {
            let chevron = ctx.add(SortIndicator::new(
                self.current_sort,
                cp::SORT_INDICATOR_SIZE,
            ));
            row = row.add_child(chevron);
        }

        // Filter popover trigger — appears at the trailing end of the
        // cell after the sort indicator. The Popover content is a
        // `TextInput` with a trailing clear `IconButton` bound back
        // into `filters_signal`. Callers can also mutate
        // `filters_signal` programmatically.
        if self.filterable {
            let filters_signal = self.filters_signal.clone();
            let col_id = self.col_id.clone();
            let initial = self
                .filters_signal
                .get()
                .get(&self.col_id)
                .cloned()
                .unwrap_or_default();
            let active = !initial.is_empty();
            let glyph = FilterIndicator::new(cp::FILTER_INDICATOR_SIZE, active);
            let on_change = {
                let filters_signal = filters_signal.clone();
                let col_id = col_id.clone();
                move |s: &str| {
                    let mut m = filters_signal.get();
                    if s.is_empty() {
                        m.remove(&col_id);
                    } else {
                        m.insert(col_id.clone(), s.to_string());
                    }
                    filters_signal.set(m);
                }
            };
            // A custom (non-button) trigger, so `PopoverWidget` takes it through
            // `OverlayTrigger`. No `focus_on_show` slot: the popover asks for
            // focus by *panel* id and the framework walks to the first focusable
            // descendant, which is this panel's filter field.
            let popover = PopoverWidget::new(
                OverlayTrigger::around(glyph).named(lit!("Filter").resolve_now()),
            )
            .content(FilterPopoverContent::new(initial).on_change(on_change))
            .placement(OverlayPlacement::BelowPreferred)
            .show_disclosure_caret(false);
            let popover_id = ctx.add(popover);
            row = row.add_child(popover_id);
        }
        let row_id = ctx.add(row);
        let padded = ctx.add(
            Padding::symmetric(cp::CELL_PADDING_VERTICAL, cp::CELL_PADDING_HORIZONTAL)
                .child_id(row_id),
        );

        // Route the header cell's chrome through `TableStyle::make_header_cell`.
        // The default `RecipeTableStyle` returns a `ZStack` that overlays
        // a hover/resize background behind the label — apps install a
        // theme-wide `style_slots.table` or pass their own when wrapping
        // the table to swap the chrome wholesale.
        let style: SharedTableStyle = ctx
            .theme()
            .style_slots
            .table
            .clone()
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTableStyle::default()));
        let cell_cfg = TableHeaderCellConfig {
            label: padded,
            sort: self.current_sort.map(style_sort),
            is_hovered: self.is_hovered.clone(),
            is_resizing: self.is_resizing.clone(),
        };
        let cell_root = style.make_header_cell(&cell_cfg, ctx);
        self.root_child_id = Some(cell_root);

        // Build a single pointer-event handler covering: cursor hint,
        // resize start (PointerDown in trailing zone), resize advance
        // (PointerMove with active state), sort cycle (PointerDown
        // outside the resize zone), and resize commit + capture release
        // (PointerUp).
        let sort_signal = self.sort_signal.clone();
        let widths_signal = self.column_widths_signal.clone();
        let widths_handle = self.column_widths.clone();
        let resize_state = self.resize_state.clone();
        let resize_target = self.resize_target.clone();
        let resize_preview_x = self.resize_preview_x.clone();
        let resize_columns = self.resize_columns.clone();
        let boundaries = self.pane_boundaries;
        let col_id = self.col_id.clone();
        let sortable = self.sortable;
        let reorderable = self.reorderable;
        let grip_base = self.resize_grip;
        let policy = self.resize_policy;
        let width_index = self.width_index;
        let table_id = self.table_id;
        let press_state: Rc<Cell<Option<PressState>>> = Rc::new(Cell::new(None));
        let self_id = ctx.self_id();
        let cell_window_x = self.cell_window_x.clone();
        let cell_window_w = self.cell_window_w.clone();
        let cell_window_h = self.cell_window_h.clone();
        let filter_zone_w = self.filter_zone_width;
        let is_hovered = self.is_hovered.clone();

        let handlers = HandlerSet::new()
            .on_hover({
                let is_hovered = is_hovered.clone();
                move |entered, _ctx| {
                    is_hovered.set(entered);
                }
            })
            .on_pointer_event(move |event, ctx: &mut EventContext| {
                // Pointer events now deliver `position` in cell-local
                // coords (the framework converts once at dispatch), so
                // `local_x` is `position.x` directly. The resize math,
                // however, wants a *window-space* x that stays stable
                // across the relayouts a Live resize triggers (the cell's
                // own leading edge moves under RTL) — reconstruct it as
                // `position.x + cell_x0`, where `cell_x0` is the cell's
                // window-space leading edge written by `place_children`
                // (== this cell node's bounds origin).
                let cell_x0 = cell_window_x.get();
                // This cell's height, used to reject pointer events that
                // bubble up from the in-tree filter popover (a descendant
                // overlay anchored *below* the cell). Those arrive with a
                // local y outside `[0, cell_h]`; the resize/cursor logic is
                // x-only, so without this gate it paints a `ColResize`
                // cursor across the entire popover.
                let cell_h = cell_window_h.get();
                // Prefer the width `place_children` actually laid this cell
                // out at; fall back to the shared widths vector only before
                // the first layout has run.
                let cell_w = {
                    let placed = cell_window_w.get();
                    if placed > 0.0 {
                        placed
                    } else {
                        widths_handle
                            .borrow()
                            .get(width_index)
                            .copied()
                            .unwrap_or(0.0)
                    }
                };
                // Under RTL columns run right-to-left, so a cell's
                // reading-order trailing edge is its physical-*left* one and
                // the drag sign inverts. Read direction live.
                let rtl = ctx.is_rtl();

                // Resolve a cell-local x into the display slot of the column
                // whose divider is being grabbed, if any. See the type docs
                // for why a cell owns the grip on *both* of its edges.
                let target_at = |local_x: f32| -> Option<usize> {
                    if cell_w <= 0.0 {
                        return None;
                    }
                    // Never let the two half-grips eat more than half the
                    // cell: a column dragged down to a tiny `min_width` must
                    // keep a central band for sort / reorder, or it becomes
                    // permanently un-sortable and un-draggable.
                    let grip = grip_base.min(cell_w * 0.25);
                    if grip <= 0.0 {
                        return None;
                    }
                    let near_physical_leading = local_x <= grip;
                    let near_physical_trailing = local_x >= cell_w - grip;
                    let (on_own_edge, on_predecessor_edge) = if rtl {
                        (near_physical_leading, near_physical_trailing)
                    } else {
                        (near_physical_trailing, near_physical_leading)
                    };
                    if on_own_edge && resize_columns.get(width_index).is_some_and(|c| c.resizable) {
                        return Some(width_index);
                    }
                    if on_predecessor_edge && width_index > 0 {
                        let prev = width_index - 1;
                        // A pane seam is not a column divider: with the
                        // Middle pane scrolled, the column on its far side
                        // is not the one visually adjacent to it.
                        if pane_of(prev, boundaries) == pane_of(width_index, boundaries)
                            && resize_columns.get(prev).is_some_and(|c| c.resizable)
                        {
                            return Some(prev);
                        }
                    }
                    None
                };

                match event {
                    WidgetEvent::PointerMove { position } => {
                        let local_x = position.x;
                        // 1. Active resize: advance regardless of pointer
                        //    location (the pointer is captured). Only the
                        //    cell that anchored the gesture may advance it —
                        //    an event reaching any other cell means the
                        //    capture was lost, and moving a column then would
                        //    look like the table resizing itself with no
                        //    button held.
                        let active = resize_state.borrow().clone();
                        if let Some(state) = active
                            && state.anchor_index == width_index
                        {
                            // Window-space delta — stable across the
                            // relayouts a Live resize triggers (see
                            // ResizeState::start_pointer_x). Reconstruct
                            // window x from the now cell-local position.
                            let delta = position.x + cell_x0 - state.start_pointer_x;
                            let signed = if rtl { -delta } else { delta };
                            let new_w = clamp_width(
                                state.start_width + signed,
                                state.min_width,
                                state.max_width,
                            );
                            match policy {
                                ColumnResizePolicy::Live => {
                                    write_width(&widths_signal, &state.col_id, new_w);
                                }
                                ColumnResizePolicy::OnRelease => {
                                    // Nothing moves until release, so show
                                    // where the divider would land.
                                    let dir = if rtl { -1.0 } else { 1.0 };
                                    resize_preview_x.set(Some(
                                        state.start_divider_x + (new_w - state.start_width) * dir,
                                    ));
                                }
                            }
                            // Hold the resize shape for the whole drag: the
                            // pointer is captured, so it can travel far
                            // outside the grip (and outside the header) and
                            // must not look like it stopped resizing.
                            ctx.set_cursor(CursorIcon::ColResize);
                            return EventResponse::Handled;
                        }
                        // 2. Press state set, no resize: if movement
                        //    crosses the threshold, escalate to a
                        //    reorder drag.
                        if reorderable && let Some(p) = press_state.get() {
                            let dx = local_x - p.pointer_x;
                            let dy = position.y - p.pointer_y;
                            if (dx * dx + dy * dy).sqrt() > DRAG_REORDER_THRESHOLD {
                                press_state.set(None);
                                let payload = DragPayload::typed(ColumnReorderDragData {
                                    col_id: col_id.clone(),
                                    source_table_id: table_id,
                                });
                                ctx.start_drag(self_id, payload);
                                return EventResponse::Handled;
                            }
                        }
                        // 3. Cursor hint over either grip. Outside them we
                        //    explicitly reset to Default so the cursor shape
                        //    doesn't stay stuck on `ColResize` after the
                        //    pointer moves off the handle (PointerLeave alone
                        //    can't rescue this — the cell has no node-level
                        //    cursor for the framework to revert to).
                        // Only manage the cursor for moves that are
                        // genuinely over this cell. Moves bubbling up from
                        // the filter popover (out-of-cell y) must leave the
                        // cursor alone, or the x-only grip test below paints
                        // `ColResize` across the popover.
                        let in_cell_y =
                            cell_h <= 0.0 || (position.y >= 0.0 && position.y <= cell_h);
                        if in_cell_y {
                            if target_at(local_x).is_some() {
                                ctx.set_cursor(CursorIcon::ColResize);
                                return EventResponse::Handled;
                            }
                            ctx.set_cursor(CursorIcon::Default);
                        }
                        EventResponse::Ignored
                    }
                    WidgetEvent::PointerDown {
                        position,
                        button: PointerButton::Primary,
                        ..
                    } => {
                        // Ignore presses bubbling up from the filter popover
                        // (out-of-cell y) — they must not record a header
                        // press / sort cycle.
                        if cell_h > 0.0 && (position.y < 0.0 || position.y > cell_h) {
                            return EventResponse::Ignored;
                        }
                        let local_x = position.x;
                        if let Some(target) = target_at(local_x) {
                            let info = &resize_columns[target];
                            let start_width =
                                widths_handle.borrow().get(target).copied().unwrap_or(0.0);
                            // Window x of the divider under the pointer: this
                            // cell's own trailing edge when resizing itself,
                            // its leading edge when resizing the predecessor
                            // — mirrored under RTL.
                            let resizing_self = target == width_index;
                            let own_edge_is_physical_leading = rtl;
                            let on_physical_leading = if resizing_self {
                                own_edge_is_physical_leading
                            } else {
                                !own_edge_is_physical_leading
                            };
                            let start_divider_x = if on_physical_leading {
                                cell_x0
                            } else {
                                cell_x0 + cell_w
                            };
                            *resize_state.borrow_mut() = Some(ResizeState {
                                col_id: info.id.clone(),
                                anchor_index: width_index,
                                start_pointer_x: position.x + cell_x0,
                                start_width,
                                start_divider_x,
                                min_width: info.min_width,
                                max_width: info.max_width,
                            });
                            resize_target.set(Some(target));
                            ctx.set_cursor(CursorIcon::ColResize);
                            ctx.capture_pointer();
                            return EventResponse::Handled;
                        }
                        // Filter-popover trigger zone — leave it alone
                        // so the Popover's gesture-based tap handler
                        // running in the bubble pass can fire. Without
                        // this carve-out, the preview-pass handler
                        // would consume PointerDown and the popover
                        // would never open. The filter glyph sits at the
                        // trailing inner edge — physical-right under LTR,
                        // physical-left under RTL (the header HStack
                        // reverses), just inside the resize handle.
                        let in_filter_zone = if rtl {
                            local_x < grip_base + filter_zone_w
                        } else {
                            local_x > cell_w - grip_base - filter_zone_w
                        };
                        if filter_zone_w > 0.0 && cell_w > 0.0 && in_filter_zone {
                            return EventResponse::Ignored;
                        }
                        // Record press: PointerUp without movement →
                        // sort cycle; PointerMove past threshold →
                        // reorder drag.
                        press_state.set(Some(PressState {
                            pointer_x: local_x,
                            pointer_y: position.y,
                        }));
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { position, .. } => {
                        // Resize commit / release. Delta is window-space
                        // (reconstruct window x from cell-local position).
                        let taken = resize_state.borrow_mut().take();
                        if let Some(state) = taken {
                            // Only the anchoring cell commits. A mismatch
                            // means the capture was lost mid-gesture and this
                            // Up landed on a bystander by hit-test — clear the
                            // orphaned state so the next press starts clean,
                            // but never write a width from the wrong origin.
                            if state.anchor_index == width_index {
                                if policy == ColumnResizePolicy::OnRelease {
                                    let delta = position.x + cell_x0 - state.start_pointer_x;
                                    let signed = if rtl { -delta } else { delta };
                                    let new_w = clamp_width(
                                        state.start_width + signed,
                                        state.min_width,
                                        state.max_width,
                                    );
                                    write_width(&widths_signal, &state.col_id, new_w);
                                }
                                resize_target.set(None);
                                resize_preview_x.set(None);
                                ctx.release_pointer();
                                return EventResponse::Handled;
                            }
                            resize_target.set(None);
                            resize_preview_x.set(None);
                        }
                        // Click without significant movement → sort
                        // cycle.
                        if press_state.replace(None).is_some() && sortable {
                            let next = match sort_signal.get() {
                                None => Some((col_id.clone(), SortDirection::Ascending)),
                                Some((id, SortDirection::Ascending)) if id == col_id => {
                                    Some((col_id.clone(), SortDirection::Descending))
                                }
                                Some((id, SortDirection::Descending)) if id == col_id => None,
                                Some(_) => Some((col_id.clone(), SortDirection::Ascending)),
                            };
                            sort_signal.set(next);
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                }
            })
            // Assistive-technology path to the resize grip. Pointer-only
            // resize is unreachable by a screen reader, switch access, or the
            // automation MCP (whose action tools route through AccessKit), so
            // a resizable column advertises Increment / Decrement and steps
            // its width by `COLUMN_RESIZE_STEP` — clamped exactly like a drag,
            // so the two paths cannot disagree about the stored override.
            .on_access_action({
                let resize_columns = self.resize_columns.clone();
                let widths_handle = self.column_widths.clone();
                let widths_signal = self.column_widths_signal.clone();
                move |action, _ctx| {
                    use teksilo_core::accesskit::Action;
                    if !matches!(action, Action::Increment | Action::Decrement) {
                        return EventResponse::Ignored;
                    }
                    let Some(info) = resize_columns.get(width_index) else {
                        return EventResponse::Ignored;
                    };
                    if !info.resizable {
                        return EventResponse::Ignored;
                    }
                    let current = widths_handle
                        .borrow()
                        .get(width_index)
                        .copied()
                        .unwrap_or(0.0);
                    if current <= 0.0 {
                        return EventResponse::Ignored;
                    }
                    let step = if matches!(action, Action::Increment) {
                        crate::styles::recipe_table_style::COLUMN_RESIZE_STEP
                    } else {
                        -crate::styles::recipe_table_style::COLUMN_RESIZE_STEP
                    };
                    let next = clamp_width(current + step, info.min_width, info.max_width);
                    write_width(&widths_signal, &info.id, next);
                    EventResponse::Handled
                }
            })
            // Default node cursor — the framework restores this on
            // PointerLeave, which guarantees the resize cursor doesn't
            // bleed across cells when the pointer exits HeaderCell from
            // inside the resize zone (where on_pointer_event last set
            // it to `ColResize`).
            .cursor(CursorIcon::Default)
            .focusable(false);
        ctx.apply_self_handlers(handlers);

        vec![cell_root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Snapshot the cell's window-space physical-left edge so the
        // pointer-event handler can convert window x to cell-local x
        // (`local_x ∈ [0, cell_w]` in both directions), plus the placed
        // size the grip test runs against.
        self.cell_window_x.set(bounds.x);
        self.cell_window_w.set(bounds.width);
        self.cell_window_h.set(bounds.height);
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    // No `paint()` — the cell's visual chrome is composed via
    // `TableStyle::make_header_cell`, layered behind the label inside
    // a `ZStack`. The outer `HeaderRow` paints the shared `Raised`
    // background for the whole strip, so a transparent cell default
    // (`SurfaceRole::Transparent`) lets the row chrome show through
    // while hover / resize overlays come from the composed body.

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::ColumnHeader);
        builder.set_name(self.label.clone());
        // Advertise the resize grip to assistive technology. Deliberately
        // actions only, with no `numeric_value` / range: a value on a
        // `ColumnHeader` would be read out on every ordinary pass over the
        // table, trading a noisier common case for a rarer one. The actions
        // are invocable without being announced.
        if self
            .resize_columns
            .get(self.width_index)
            .is_some_and(|c| c.resizable)
        {
            builder.add_action(teksilo_core::accesskit::Action::Increment);
            builder.add_action(teksilo_core::accesskit::Action::Decrement);
        }
        // Through the typed wrapper, which converts the ARIA 1-based index to
        // AccessKit's zero-based `column_index`. `inner_mut()` would not.
        builder.set_column_index(self.col_index_1based);
        let n = builder.inner_mut();
        if let Some(dir) = self.current_sort {
            let ak_dir = match dir {
                SortDirection::Ascending => teksilo_core::accesskit::SortDirection::Ascending,
                SortDirection::Descending => teksilo_core::accesskit::SortDirection::Descending,
            };
            n.set_sort_direction(ak_dir);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn write_width(signal: &Signal<HashMap<String, f32>>, col_id: &str, new_w: f32) {
    let mut m = signal.get();
    m.insert(col_id.to_string(), new_w);
    signal.set(m);
}

/// Tiny chevron drawn as a triangle path.
#[derive(Debug)]
struct SortIndicator {
    direction: Option<SortDirection>,
    size: f32,
}

impl SortIndicator {
    fn new(direction: Option<SortDirection>, size: f32) -> Self {
        Self { direction, size }
    }
}

impl Widget for SortIndicator {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(self.size, self.size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(dir) = self.direction else {
            return;
        };
        let color = TextRole::Accent.resolve(&ctx.theme.colors);
        let cx = bounds.x + bounds.width / 2.0;
        let pad = bounds.height * 0.15;
        let top_y = bounds.y + pad;
        let bot_y = bounds.y + bounds.height - pad;
        let half_w = bounds.width / 2.0 - pad;
        let mut path = Path::new();
        match dir {
            SortDirection::Ascending => {
                path.move_to(Point::new(cx, top_y));
                path.line_to(Point::new(cx + half_w, bot_y));
                path.line_to(Point::new(cx - half_w, bot_y));
                path.close();
            }
            SortDirection::Descending => {
                path.move_to(Point::new(cx - half_w, top_y));
                path.line_to(Point::new(cx + half_w, top_y));
                path.line_to(Point::new(cx, bot_y));
                path.close();
            }
        }
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// Header strip — `Role::Row` (row index 1), N HeaderCell widgets laid
/// out horizontally using the same shared widths handle as body rows.
///
/// Splits into pane bands under column pinning, exactly like `BodyRow` — see
/// that type's module docs for the full rationale (this is the header-side
/// half of the same mechanism, sharing `RowBand`).
#[derive(Debug)]
pub(crate) struct HeaderRow {
    cells: Vec<WidgetId>,
    widths: SharedColumnWidths,
    divider_width: f32,
    pane_boundaries: PaneBoundaries,
    scroll_x: Signal<f32>,

    // Build state.
    bands: Option<[Option<WidgetId>; 3]>,
}

impl HeaderRow {
    pub(crate) fn new(
        cells: Vec<WidgetId>,
        widths: SharedColumnWidths,
        divider_width: f32,
        pane_boundaries: PaneBoundaries,
        scroll_x: Signal<f32>,
    ) -> Self {
        Self {
            cells,
            widths,
            divider_width,
            pane_boundaries,
            scroll_x,
            bands: None,
        }
    }

    fn has_pinning(&self) -> bool {
        self.pane_boundaries.leading_count > 0 || self.pane_boundaries.middle_end < self.cells.len()
    }
}

impl Widget for HeaderRow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if !self.has_pinning() {
            return Vec::new();
        }
        let b = self.pane_boundaries;
        let leading_end = b.leading_count.min(self.cells.len());
        let middle_end = b.middle_end.min(self.cells.len()).max(leading_end);
        let leading: Vec<WidgetId> = self.cells[..leading_end].to_vec();
        let middle: Vec<WidgetId> = self.cells[leading_end..middle_end].to_vec();
        let trailing: Vec<WidgetId> = self.cells[middle_end..].to_vec();

        let mut bands: [Option<WidgetId>; 3] = [None, None, None];
        if !leading.is_empty() {
            bands[0] = Some(ctx.add(RowBand::new(leading, self.widths.clone(), 0)));
        }
        if !middle.is_empty() {
            bands[1] = Some(
                ctx.add(
                    RowBand::new(middle, self.widths.clone(), leading_end)
                        .scrollable(self.scroll_x.clone()),
                ),
            );
        }
        if !trailing.is_empty() {
            bands[2] = Some(ctx.add(RowBand::new(trailing, self.widths.clone(), middle_end)));
        }
        let out: Vec<WidgetId> = bands.iter().copied().flatten().collect();
        self.bands = Some(bands);
        out
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let width = proposal
            .width
            .unwrap_or_else(|| self.widths.borrow().iter().sum());
        let height = proposal.height.unwrap_or(32.0);
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if let Some(bands) = self.bands {
            let widths = self.widths.borrow();
            let rtl = ctx.is_rtl();
            let (leading_rect, middle_rect, trailing_rect) =
                band_rects(bounds, &widths, self.pane_boundaries, rtl);
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

        let widths = self.widths.borrow();
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
        let dw = self.divider_width.max(1.0);
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
        let widths = self.widths.borrow();
        if widths.len() > 1 {
            let rtl =
                ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;
            let sep = BorderRole::Divider.resolve(&ctx.theme.colors);
            let (leading_rect, middle_rect, trailing_rect) =
                band_rects(bounds, &widths, self.pane_boundaries, rtl);
            let b = self.pane_boundaries;
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
            // the scrolling region. `draw_pane_dividers` only draws a band's
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
}

// ── Reorder drag-target plumbing ───────────────────────────────────────────

/// Attach `on_drag_hover` and `on_drop` to a header strip so reorder drags
/// from any cell of *this* table/tree-table can be classified into a pane
/// (Leading / None / Trailing) and an insertion index.
///
/// Shared by `TableView` and `TreeTableView` — both build the header out of
/// the same `HeaderCell`/`HeaderRow` pair and carry an identically-shaped
/// bundle of order/pinning/geometry state, so the drop-target half lives
/// here once rather than twice. `source_table_id` is each view's own
/// `table_id` (a `TableView` and a `TreeTableView` mint theirs from separate
/// counters, so the values can collide across the two widget kinds — this
/// is fine, since the collision only matters if a `ColumnReorderDragData`
/// somehow reached a header of the wrong *kind*, which the header cell's
/// `col_id` domain already prevents in practice; a same-kind, different-id
/// pairing is what this guard exists to reject).
///
/// Inter-table drops are rejected by matching `source_table_id`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attach_header_reorder_handlers(
    ctx: &mut BuildContext,
    header_row_id: WidgetId,
    source_table_id: usize,
    column_widths: Rc<RefCell<Vec<f32>>>,
    display_indices: Rc<RefCell<Vec<usize>>>,
    pane_boundaries: Rc<RefCell<PaneBoundaries>>,
    column_order_signal: Signal<Vec<String>>,
    column_pinning_signal: Signal<HashMap<String, PinnedSide>>,
    column_ids: Vec<String>,
    header_strip_width: Rc<Cell<f32>>,
    scroll_x: Signal<f32>,
) {
    let widths_for_drop = column_widths.clone();
    let display_for_drop = display_indices.clone();
    let panes_for_drop = pane_boundaries.clone();
    let order_for_drop = column_order_signal.clone();
    let pinning_for_drop = column_pinning_signal.clone();
    let ids_for_drop = column_ids;
    let strip_width_for_drop = header_strip_width;
    let scroll_x_for_drop = scroll_x;

    ctx.apply_handlers(
        header_row_id,
        HandlerSet::new()
            .on_drag_hover(|payload, _position, _ctx| {
                if payload.has_typed::<ColumnReorderDragData>() {
                    teksilo_core::DropFeedback::HighlightRect {
                        rect: teksilo_canvas::Rect::ZERO,
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
                if drag.source_table_id != source_table_id {
                    return false;
                }
                let widths = widths_for_drop.borrow().clone();
                let display = display_for_drop.borrow().clone();
                let panes = *panes_for_drop.borrow();
                let total = display.len();
                if total == 0 {
                    return false;
                }

                // `position` is local to the header strip (origin at its
                // physical-left edge). Under RTL the columns are placed in
                // display order from the strip's right edge leftward, so
                // mirror the drop x against the strip width before running
                // the left-to-right scan. (A drop in any non-content dead
                // space then maps past the last column → append, matching
                // LTR's trailing-end behaviour.)
                let drop_x = if ctx.is_rtl() {
                    strip_width_for_drop.get() - position.x
                } else {
                    position.x
                };

                // Compute insertion index in display order: find the
                // first column whose midpoint exceeds the (mirrored) x —
                // pane- and scroll-aware, so a drop under a nonzero
                // `scroll_x` resolves against the columns actually under
                // the pointer, not their unscrolled positions.
                let insertion_display_idx = insertion_slot_at_x(
                    &widths,
                    panes,
                    scroll_x_for_drop.get(),
                    strip_width_for_drop.get(),
                    drop_x,
                );

                // Classify the drop position into a pane.
                let new_pinning = if insertion_display_idx <= panes.leading_count {
                    PinnedSide::Leading
                } else if insertion_display_idx >= panes.middle_end {
                    PinnedSide::Trailing
                } else {
                    PinnedSide::None
                };

                // Update pinning override (record only when it deviates
                // from None, which is the framework default).
                let mut pin_map = pinning_for_drop.get();
                match new_pinning {
                    PinnedSide::None => {
                        pin_map.remove(&drag.col_id);
                    }
                    other => {
                        pin_map.insert(drag.col_id.clone(), other);
                    }
                }
                pinning_for_drop.set(pin_map);

                // Rebuild the column-order list to reflect the drop.
                let mut new_order: Vec<String> =
                    display.iter().map(|&i| ids_for_drop[i].clone()).collect();
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
                    order_for_drop.set(new_order);
                }
                true
            }),
    );
}
