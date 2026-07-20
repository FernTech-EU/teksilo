// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Sticky header strip + per-column header cells.
//!
//! `HeaderCell` lays out as
//! `Padding → HStack { TextWidget(label), Spacer, SortIndicator? }` and
//! handles click-to-sort plus drag-to-resize on its trailing edge.
//! `HeaderRow` lays its cells horizontally using the same shared
//! `column_widths` handle that body rows consume — so a resize commits
//! in one place and reflows everywhere.
//!
//! Supports sort, resize, reuse across pinned panes, per-column filter
//! popovers, and column-reorder drag.

use bastyde_i18n::lit;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::drag_payload::DragPayload;
use bastyde_core::event::{EventResponse, PointerButton, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    SharedTableStyle, SortDirection as StyleSortDirection, TableHeaderCellConfig,
};
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::SortDirection;
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

/// Convert the data-layer `SortDirection` (bastyde-data) to the
/// styles-layer one (bastyde-core::styles). They share the same shape
/// but are distinct types because bastyde-core cannot depend on
/// bastyde-data.
fn style_sort(d: SortDirection) -> StyleSortDirection {
    match d {
        SortDirection::Ascending => StyleSortDirection::Ascending,
        SortDirection::Descending => StyleSortDirection::Descending,
    }
}

use crate::primitives::{HStack, Padding, Spacer, TextWidget};

use super::ColumnReorderDragData;
use super::body::SharedColumnWidths;
use super::column::ColumnResizePolicy;
use super::filter::FilterIndicator;
use super::filter::FilterPopoverContent;
use crate::popover::Popover;
use bastyde_core::overlay::OverlayPlacement;

const DRAG_REORDER_THRESHOLD: f32 = 5.0;

/// Active resize state for one column. Anchored at PointerDown,
/// advanced on PointerMove, committed (and cleared) on PointerUp.
#[derive(Debug, Clone)]
pub(crate) struct ResizeState {
    pub col_id: String,
    /// Pointer x at PointerDown, in **window** coordinates. Window-space
    /// (not cell-local) so the delta stays stable across the relayouts a
    /// Live-policy resize triggers: under RTL a widening column's
    /// physical-left edge — and thus the cell's local origin — moves
    /// mid-drag, so a cell-local anchor would drift. Window x doesn't.
    pub start_pointer_x: f32,
    /// Column width at PointerDown (in pixels).
    pub start_width: f32,
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

/// One header cell — label, optional sort indicator, click-to-sort,
/// drag-to-resize on the trailing edge.
pub(crate) struct HeaderCell {
    col_id: String,
    label: String,
    col_index_1based: usize,
    sortable: bool,
    resizable: bool,
    reorderable: bool,
    resize_handle_width: f32,
    /// Sort direction for *this* column, or `None` if it isn't the
    /// active sort column. Captured at build time and reflected in the
    /// AccessKit node + the chevron child.
    current_sort: Option<SortDirection>,
    sort_signal: Signal<Option<(String, SortDirection)>>,
    column_widths_signal: Signal<HashMap<String, f32>>,
    /// Live resolved widths, shared with the row layout. Read at
    /// PointerDown to record the starting width and at PointerMove to
    /// translate cell-local x into a delta.
    column_widths: SharedColumnWidths,
    /// Index of this column in the resolved-widths vector.
    width_index: usize,
    /// The floor a resize drag can't push this column below — the
    /// column's own `min_width` if declared, else the table's
    /// `min_column_width_default`. Must match the floor
    /// `ColumnSolver::resolve_in_order` clamps to (`layout.rs`), or a
    /// drag below it creates a dead zone (the override commits smaller
    /// than the solver ever renders).
    min_width_floor: f32,
    resize_policy: ColumnResizePolicy,
    resize_state: ResizeStateHandle,
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
    /// `true` while an active column-resize drag is anchored on
    /// this cell — drives the `Pressed` overlay supplied by
    /// `TableStyle::make_header_cell`. Toggled in lockstep with
    /// the `resize_state` writes in `on_pointer_event`.
    is_resizing: Signal<bool>,

    // Build state
    root_child_id: Option<WidgetId>,
}

impl HeaderCell {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        col_id: impl Into<String>,
        label: impl Into<String>,
        col_index_1based: usize,
        sortable: bool,
        resizable: bool,
        reorderable: bool,
        resize_handle_width: f32,
        current_sort: Option<SortDirection>,
        sort_signal: Signal<Option<(String, SortDirection)>>,
        column_widths_signal: Signal<HashMap<String, f32>>,
        column_widths: SharedColumnWidths,
        width_index: usize,
        min_width_floor: f32,
        resize_policy: ColumnResizePolicy,
        resize_state: ResizeStateHandle,
        table_id: usize,
        filterable: bool,
        filter_zone_width: f32,
        filters_signal: Signal<HashMap<String, String>>,
    ) -> Self {
        Self {
            col_id: col_id.into(),
            label: label.into(),
            col_index_1based,
            sortable,
            resizable,
            reorderable,
            resize_handle_width,
            current_sort,
            sort_signal,
            column_widths_signal,
            column_widths,
            width_index,
            min_width_floor,
            resize_policy,
            resize_state,
            table_id,
            cell_window_x: Rc::new(Cell::new(0.0)),
            cell_window_h: Rc::new(Cell::new(0.0)),
            filterable,
            filter_zone_width: if filterable { filter_zone_width } else { 0.0 },
            filters_signal,
            is_hovered: Signal::new(false),
            is_resizing: Signal::new(false),
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
            .field("resizable", &self.resizable)
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
            let focus_slot = Rc::new(Cell::new(None));
            let popover = Popover::new(lit!("Filter"))
                .content(
                    FilterPopoverContent::new(initial)
                        .on_change(on_change)
                        .focus_slot(focus_slot.clone()),
                )
                .trigger(glyph)
                .placement(OverlayPlacement::BelowPreferred)
                .caret(false)
                .focus_on_show(focus_slot);
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
        let col_id = self.col_id.clone();
        let sortable = self.sortable;
        let resizable = self.resizable;
        let reorderable = self.reorderable;
        let resize_zone = self.resize_handle_width;
        let min_width_floor = self.min_width_floor;
        let policy = self.resize_policy;
        let width_index = self.width_index;
        let table_id = self.table_id;
        let press_state: Rc<Cell<Option<PressState>>> = Rc::new(Cell::new(None));
        let self_id = ctx.self_id();
        let cell_window_x = self.cell_window_x.clone();
        let cell_window_h = self.cell_window_h.clone();
        let filter_zone_w = self.filter_zone_width;
        let is_hovered = self.is_hovered.clone();
        let is_resizing = self.is_resizing.clone();

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
                // The column-resize boundary is shared with the *visual*
                // neighbour: under LTR that's the column to the right
                // (handle at the cell's physical-right edge, drag-right =
                // wider). Under RTL columns run right-to-left, so the
                // neighbour — and the handle — are at the physical-left
                // edge and the drag sign inverts. Read direction live.
                let rtl = ctx.is_rtl();
                match event {
                    WidgetEvent::PointerMove { position } => {
                        let local_x = position.x;
                        // 1. Active resize: advance regardless of pointer
                        //    location (the pointer is captured). `delta`
                        //    is computed against the press's local x so
                        //    the sign matches the visible drag.
                        let active = resize_state.borrow().clone();
                        if let Some(state) = active
                            && state.col_id == col_id
                        {
                            // Window-space delta — stable across the
                            // relayouts a Live resize triggers (see
                            // ResizeState::start_pointer_x). Reconstruct
                            // window x from the now cell-local position.
                            let delta = position.x + cell_x0 - state.start_pointer_x;
                            let signed = if rtl { -delta } else { delta };
                            let new_w = (state.start_width + signed).max(min_width_floor);
                            if policy == ColumnResizePolicy::Live {
                                write_width(&widths_signal, &state.col_id, new_w);
                            }
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
                        // 3. Cursor hint near the trailing edge. Outside
                        //    the zone we explicitly reset to Default so
                        //    the cursor shape doesn't stay stuck on
                        //    `ColResize` after the pointer moves off
                        //    the handle (PointerLeave alone can't
                        //    rescue this — the cell has no node-level
                        //    cursor for the framework to revert to).
                        // Only manage the cursor for moves that are
                        // genuinely over this cell. Moves bubbling up from
                        // the filter popover (out-of-cell y) must leave the
                        // cursor alone, or the x-only resize-zone test below
                        // paints `ColResize` across the popover.
                        let in_cell_y =
                            cell_h <= 0.0 || (position.y >= 0.0 && position.y <= cell_h);
                        if resizable && in_cell_y {
                            let w = widths_handle
                                .borrow()
                                .get(width_index)
                                .copied()
                                .unwrap_or(0.0);
                            let in_zone = if rtl {
                                local_x < resize_zone
                            } else {
                                local_x > w - resize_zone
                            };
                            if w > 0.0 && in_zone {
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
                        let cell_w = widths_handle
                            .borrow()
                            .get(width_index)
                            .copied()
                            .unwrap_or(0.0);
                        let in_resize_zone = if rtl {
                            local_x < resize_zone
                        } else {
                            local_x > cell_w - resize_zone
                        };
                        let in_resize = resizable && cell_w > 0.0 && in_resize_zone;
                        if in_resize {
                            *resize_state.borrow_mut() = Some(ResizeState {
                                col_id: col_id.clone(),
                                start_pointer_x: position.x + cell_x0,
                                start_width: cell_w,
                            });
                            is_resizing.set(true);
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
                            local_x < resize_zone + filter_zone_w
                        } else {
                            local_x > cell_w - resize_zone - filter_zone_w
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
                            if policy == ColumnResizePolicy::OnRelease {
                                let delta = position.x + cell_x0 - state.start_pointer_x;
                                let signed = if rtl { -delta } else { delta };
                                let new_w = (state.start_width + signed).max(min_width_floor);
                                write_width(&widths_signal, &state.col_id, new_w);
                            }
                            is_resizing.set(false);
                            ctx.release_pointer();
                            return EventResponse::Handled;
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
    ) -> bastyde_core::widget::LayoutResponse {
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
        // (`local_x ∈ [0, cell_w]` in both directions).
        self.cell_window_x.set(bounds.x);
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
        builder.set_role(bastyde_core::accesskit::Role::ColumnHeader);
        builder.set_name(self.label.clone());
        let n = builder.inner_mut();
        n.set_column_index(self.col_index_1based);
        if let Some(dir) = self.current_sort {
            let ak_dir = match dir {
                SortDirection::Ascending => bastyde_core::accesskit::SortDirection::Ascending,
                SortDirection::Descending => bastyde_core::accesskit::SortDirection::Descending,
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
    ) -> bastyde_core::widget::LayoutResponse {
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
#[derive(Debug)]
pub(crate) struct HeaderRow {
    cells: Vec<WidgetId>,
    widths: SharedColumnWidths,
    divider_width: f32,
}

impl HeaderRow {
    pub(crate) fn new(
        cells: Vec<WidgetId>,
        widths: SharedColumnWidths,
        divider_width: f32,
    ) -> Self {
        Self {
            cells,
            widths,
            divider_width,
        }
    }
}

impl Widget for HeaderRow {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        let widths = self.widths.borrow();
        let total_children = children.len();
        let fallback_w = if total_children == 0 {
            0.0
        } else {
            bounds.width / total_children as f32
        };
        // Mirror the body: preserve display order, reverse physical x in RTL.
        if ctx.is_rtl() {
            let mut x = bounds.right();
            for (i, child) in children.iter_mut().enumerate() {
                let w = widths.get(i).copied().unwrap_or(fallback_w);
                x -= w;
                child.origin = Point::new(x, bounds.y);
                child.size = Size::new(w, bounds.height);
            }
        } else {
            let mut x = bounds.x;
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
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Row);
        builder.inner_mut().set_row_index(1);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.cells.clone()
    }
}
