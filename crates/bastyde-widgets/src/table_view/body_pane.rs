//! `BodyPane<T>` — the virtualized row pane underneath the header.
//!
//! Splitting this out of `TableView`'s root widget is a deliberate
//! architectural choice: `TableView` owns three direct children — the
//! header, the body pane, and the scrollbar. Rebuilds triggered by
//! scroll-buffer exits, selection changes, or row-edit toggles target
//! the body pane only, *not* the table root.
//!
//! Why it matters: when the user drags the scrollbar thumb, the
//! framework holds an implicit pointer capture on the scrollbar widget
//! for the entire Down→Up sequence. The rebuild deferral in
//! `process_pending_rebuilds` skips rebuilds that target any *ancestor*
//! of the captured widget — otherwise the rebuild would destroy the
//! scrollbar mid-drag and the recogniser would lose the press state.
//! With the row-rebuild target moved off `TableView` (an ancestor of
//! the scrollbar) and onto `BodyPane` (a sibling of the scrollbar),
//! mid-drag rebuilds become safe and the body keeps materializing
//! visible rows as `scroll_y` advances.
//!
//! `BodyPane` is `pub(crate)` — applications still talk to `TableView`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{SelectionMode, SelectionModel};

use super::a11y::CellA11y;
use super::body::{BodyRow, SharedColumnWidths};
use super::column::{CellContext, Column};
use super::selection::{CellSelectionModel, TableSelectionMode};
use crate::common::row_metrics::SharedRowMetrics;
use crate::table_view::RowReorderDragData;

const BUFFER_ROWS: usize = 5;

pub(crate) type LenFn = Rc<dyn Fn() -> usize>;
pub(crate) type WithItemFn<T> = Rc<dyn Fn(usize, &dyn Fn(&T))>;
pub(crate) type MoveItemFn = Rc<dyn Fn(usize, usize)>;

/// The row-virtualization pane. Owns the visible row widgets and
/// handles their per-row click + drag handlers. Sized to fill the
/// caller's proposal; lays each row at `flat_index * row_height -
/// scroll_y` in pane-local coordinates.
pub(crate) struct BodyPane<T: 'static> {
    pub(crate) len_fn: LenFn,
    pub(crate) with_item_fn: WithItemFn<T>,
    pub(crate) move_item_fn: Option<MoveItemFn>,

    pub(crate) columns: Vec<Column<T>>,
    pub(crate) display_indices: Rc<RefCell<Vec<usize>>>,
    pub(crate) column_widths: SharedColumnWidths,

    /// Row geometry shared with the `TableView` root (one handle, two
    /// holders — the root drives scrollbar totals / paint / keyboard,
    /// the pane drives realization, placement, and measurement).
    pub(crate) row_metrics: SharedRowMetrics,
    pub(crate) selection_mode: TableSelectionMode,
    pub(crate) selection: Option<SelectionModel>,
    pub(crate) cell_selection: Option<CellSelectionModel>,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    pub(crate) editing_cell: Signal<Option<(usize, usize)>>,
    pub(crate) focused_cell: Signal<Option<(usize, usize)>>,

    pub(crate) reorderable_rows: bool,
    pub(crate) table_id: usize,
    /// Anchor used by row drag-start to identify the source. Captured
    /// at construction so the closure stays `'static`.
    pub(crate) drag_anchor: WidgetId,

    /// Pane-local rebuild trigger. A persistent field (re-bound each
    /// build) so `place_children`'s post-measure realization re-check
    /// can request a rebuild of this pane.
    pub(crate) version: Signal<u64>,
    /// Bound at `Relayout` on the `TableView` ROOT. The root computes
    /// scrollbar totals (`max_scroll_y`, thumb ratio) before this pane
    /// measures (parent-before-child layout order); when a measure pass
    /// changes the content total, the pane bumps this so the root
    /// re-places next frame with the corrected total — otherwise the
    /// stale totals would persist forever (content beyond the estimated
    /// total would be unreachable). A dedicated signal rather than a
    /// `scroll_y` self-set so an in-flight scroll animation is never
    /// cancelled.
    pub(crate) total_refresh: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    pub(crate) prev_built_start: Rc<Cell<usize>>,
    pub(crate) prev_built_end: Rc<Cell<usize>>,

    // Build state
    pub(crate) row_entries: Vec<(usize, WidgetId)>,
}

impl<T: 'static> BodyPane<T> {
    fn visible_range(&self) -> (usize, usize) {
        self.row_metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            (self.len_fn)(),
            BUFFER_ROWS,
        )
    }
}

impl<T: 'static> std::fmt::Debug for BodyPane<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BodyPane")
            .field("rows", &(self.len_fn)())
            .field("columns", &self.columns.len())
            .finish()
    }
}

impl<T: 'static> Widget for BodyPane<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Self-rebuild trigger. A persistent field (not `ctx.signal`)
        // so the realization re-check in `place_children` can bump it
        // after measurement.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll position re-places rows without rebuilding (within buffer).
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Buffer-exit detection. Bumps version → rebuild THIS pane.
        // Critical: because BodyPane is a sibling of the scrollbar
        // (not its ancestor), the rebuild deferral logic doesn't
        // skip this rebuild during a thumb drag. Without this split
        // (when the rebuild was rooted on TableView, an ancestor of
        // the scrollbar), dragging the thumb past the buffer left
        // the body empty until the user released the thumb.
        let len = self.len_fn.clone();
        let vp_h = self.viewport_height.clone();
        let (initial_start, initial_end) = self.visible_range();
        self.prev_built_start.set(initial_start);
        self.prev_built_end.set(initial_end);
        let v_for_scroll = version.clone();
        let scroll_handle = self.scroll_y.observe({
            let pbs = self.prev_built_start.clone();
            let pbe = self.prev_built_end.clone();
            let metrics = self.row_metrics.clone();
            move |y| {
                let count = (len)();
                let (visible_start, visible_end) =
                    metrics
                        .borrow_mut()
                        .visible_range(*y, vp_h.get(), count, 0);
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ROWS);
                    let new_end = (visible_end + BUFFER_ROWS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    v_for_scroll.set(v_for_scroll.get() + 1);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // Selection / editing changes — refresh `is_selected` /
        // `is_editing` flags fed into the cell delegate.
        if let Some(ref sel) = self.selection {
            let v = version.clone();
            let counter = Rc::new(Cell::new(0_u64));
            ctx.effect(&sel.selection_signal(), move |_| {
                counter.set(counter.get() + 1);
                v.set(counter.get());
            });
        }
        if let Some(ref cs) = self.cell_selection {
            let v = version.clone();
            let counter = Rc::new(Cell::new(0_u64));
            ctx.effect(&cs.selection_signal(), move |_| {
                counter.set(counter.get() + 1);
                v.set(counter.get());
            });
        }
        let v_for_edit = version.clone();
        let edit_counter = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.editing_cell, move |_| {
            edit_counter.set(edit_counter.get() + 1);
            v_for_edit.set(edit_counter.get());
        });

        // Build the visible row range.
        self.row_entries.clear();
        let (start, end) = self.visible_range();
        let columns = self.columns.clone();
        let with_item_fn = self.with_item_fn.clone();
        let display_indices = self.display_indices.borrow().clone();
        let editing_state = self.editing_cell.get();
        let row_widths_handle = self.column_widths.clone();
        let selection_mode = self.selection_mode;

        for row_idx in start..end {
            let row_selected_for_a11y = match (selection_mode, &self.selection) {
                (TableSelectionMode::SingleRow | TableSelectionMode::MultiRow, Some(s)) => {
                    s.is_selected(row_idx)
                }
                _ => false,
            };

            let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
            for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                let col = &columns[col_idx];
                let is_editing = editing_state == Some((row_idx, display_pos));
                let is_selected = match (selection_mode, &self.selection, &self.cell_selection) {
                    (TableSelectionMode::SingleRow | TableSelectionMode::MultiRow, Some(s), _) => {
                        s.is_selected(row_idx)
                    }
                    (
                        TableSelectionMode::SingleCell | TableSelectionMode::MultiCell,
                        _,
                        Some(cs),
                    ) => cs.is_selected(row_idx, display_pos),
                    _ => false,
                };
                let is_focused = self.focused_cell.get() == Some((row_idx, display_pos));
                let cell_ctx = CellContext {
                    row_index: row_idx,
                    col_id: col.id.clone(),
                    col_index: display_pos,
                    is_selected,
                    is_focused,
                    is_hovered: false,
                    is_editing,
                    depth: None,
                    is_tree_column: false,
                };
                let cell_widget =
                    read_item_local(&with_item_fn, row_idx, |item| (col.cell)(item, &cell_ctx));
                if let Some(widget) = cell_widget {
                    let inner_id = ctx.add_boxed(widget);
                    // When the cell delegate just swapped in an editor
                    // (because `is_editing` flipped to true), the
                    // delegate's child subtree is built fresh — focus
                    // is still on whatever it was before the rebuild,
                    // which is now stale. Walk the editing cell's
                    // subtree for the first focusable descendant and
                    // hand keyboard focus to it. Without this, F2 puts
                    // a `TextInput` on screen but the user has to
                    // click it before they can type.
                    if is_editing
                        && let Some(focus_target) = ctx.first_focusable_descendant(inner_id)
                    {
                        ctx.focus(focus_target);
                    }
                    let cell_a11y = CellA11y::new(
                        inner_id,
                        row_idx + 2, // header is row 1
                        display_pos + 1,
                        is_selected,
                    );
                    let cell_id = ctx.add(cell_a11y);

                    // Per-cell pointer handler: a click on the cell
                    // sets `focused_cell` to (row, col) so the focus
                    // ring follows the mouse. Also mirrors the click
                    // into `cell_selection` when the table is in a
                    // cell-selection mode (Ctrl/Shift modifiers extend
                    // the rectangular selection just like the keyboard
                    // handler). Skipped while editing — the click
                    // belongs to the inner editor.
                    let focused_for_cell = self.focused_cell.clone();
                    let editing_for_cell = self.editing_cell.clone();
                    let cell_sel_for_click = self.cell_selection.clone();
                    let row_for_cell = row_idx;
                    let col_for_cell = display_pos;
                    let mode_for_cell = selection_mode;
                    let cell_handlers =
                        HandlerSet::new().on_pointer_event(move |event, _ctx| match event {
                            bastyde_core::event::WidgetEvent::PointerDown {
                                button: bastyde_core::event::PointerButton::Primary,
                                modifiers,
                                ..
                            } => {
                                if editing_for_cell.get().is_some() {
                                    return bastyde_core::event::EventResponse::Ignored;
                                }
                                focused_for_cell.set(Some((row_for_cell, col_for_cell)));
                                if let Some(ref cs) = cell_sel_for_click {
                                    match mode_for_cell {
                                        TableSelectionMode::SingleCell => {
                                            cs.select(row_for_cell, col_for_cell);
                                        }
                                        TableSelectionMode::MultiCell => {
                                            if modifiers.shift() {
                                                cs.extend_to(row_for_cell, col_for_cell);
                                            } else if modifiers.ctrl() {
                                                cs.toggle(row_for_cell, col_for_cell);
                                            } else {
                                                cs.select(row_for_cell, col_for_cell);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                bastyde_core::event::EventResponse::Ignored
                            }
                            _ => bastyde_core::event::EventResponse::Ignored,
                        });
                    ctx.apply_handlers(cell_id, cell_handlers);

                    cell_ids.push(cell_id);
                }
            }

            // Auto-measure mode hands the row a `None` height so its
            // `layout_response` measures the tallest cell; fixed modes
            // pass the per-row height from the metrics. (Two separate
            // borrows — a borrow inside an `if` condition would live to
            // the end of the statement and collide with `borrow_mut`.)
            let needs_measure = self.row_metrics.borrow().needs_measure();
            let row_height = if needs_measure {
                None
            } else {
                Some(self.row_metrics.borrow_mut().row_height(row_idx))
            };
            let row_widget = BodyRow::new(
                cell_ids,
                row_idx + 2,
                row_selected_for_a11y,
                row_height,
                row_widths_handle.clone(),
            );
            let row_id = ctx.add(row_widget);

            // Selection click on the row. Skipped while a cell is in
            // edit mode so clicks landing inside the editor (e.g. on
            // the cell's `TextInput`) don't change the selection — a
            // selection change would re-emit the row, destroying the
            // editor and dropping focus mid-click.
            let mut row_handlers = HandlerSet::new();
            if let Some(ref sel) = self.selection {
                let row_index_for_click = row_idx;
                let sel_for_click = sel.clone();
                let editing_for_click = self.editing_cell.clone();
                if matches!(
                    selection_mode,
                    TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
                ) {
                    row_handlers = row_handlers.on_pointer_event(move |event, _ctx| match event {
                        bastyde_core::event::WidgetEvent::PointerDown {
                            button: bastyde_core::event::PointerButton::Primary,
                            modifiers,
                            ..
                        } => {
                            if editing_for_click.get().is_some() {
                                return bastyde_core::event::EventResponse::Ignored;
                            }
                            if modifiers.ctrl() && sel_for_click.mode() == SelectionMode::Multi {
                                sel_for_click.toggle(row_index_for_click);
                            } else if modifiers.shift()
                                && sel_for_click.mode() == SelectionMode::Multi
                            {
                                sel_for_click.extend_to(row_index_for_click);
                            } else {
                                sel_for_click.select(row_index_for_click);
                            }
                            bastyde_core::event::EventResponse::Ignored
                        }
                        _ => bastyde_core::event::EventResponse::Ignored,
                    });
                }
            }
            if self.reorderable_rows && self.move_item_fn.is_some() {
                let drag_row = row_idx;
                let drag_table_id = self.table_id;
                let anchor = self.drag_anchor;
                row_handlers = row_handlers.on_drag(move |phase, ctx| {
                    if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                        let payload =
                            bastyde_core::drag_payload::DragPayload::typed(RowReorderDragData {
                                source_row: drag_row,
                                source_table_id: drag_table_id,
                            });
                        ctx.start_drag(anchor, payload);
                    }
                });
            }
            ctx.apply_handlers(row_id, row_handlers);

            self.row_entries.push((row_idx, row_id));
        }

        self.row_entries.iter().map(|(_, id)| *id).collect()
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
        // Auto-measure pass: measure every realized row at the pane
        // width (BodyRow reports its tallest cell, height-for-width),
        // feed the heights back, and apply the scroll-anchor delta so
        // content above the viewport stays put. Measurements are
        // collected with NO metrics borrow held.
        if self.row_metrics.borrow().needs_measure() {
            let count = (self.len_fn)();
            let pre_total = self.row_metrics.borrow_mut().total_height(count);
            let mut measured = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                if let Some(size) = ctx.child_size(child.id, SizeProposal::with_width(bounds.width))
                {
                    let (model_index, _) = self.row_entries[i];
                    measured.push((model_index, size.height));
                }
            }
            let anchor = self
                .row_metrics
                .borrow_mut()
                .observe_measured(&measured, self.scroll_y.get());
            if anchor.abs() > 0.01 {
                // Safe from place_children: the dirty flag is set but the
                // binding flush already ran this pass — lands next frame.
                self.scroll_y
                    .set((self.scroll_y.get() + anchor).max(0.0));
            }

            // Realization re-check: corrected offsets may reveal viewport
            // rows the estimated offsets never realized. Request a pane
            // rebuild for next frame; the 0.01 measurement epsilon
            // guarantees convergence.
            let (vs, ve) = self.row_metrics.borrow_mut().visible_range(
                self.scroll_y.get(),
                self.viewport_height.get(),
                count,
                0,
            );
            if vs < self.prev_built_start.get() || ve > self.prev_built_end.get() {
                self.prev_built_start.set(vs.saturating_sub(BUFFER_ROWS));
                self.prev_built_end.set((ve + BUFFER_ROWS).min(count));
                self.version.set(self.version.get() + 1);
            }

            // Total-refresh poke: the root computed `max_scroll_y` /
            // thumb ratio BEFORE this measure pass (parent-first
            // ordering). If the content total changed, re-place the
            // root next frame so the corrected total lands — without
            // this, content past the estimated total stays unreachable
            // forever. Terminates: a re-measure of settled rows yields
            // zero deltas (sub-pixel epsilon), leaving the total fixed.
            let post_total = self.row_metrics.borrow_mut().total_height(count);
            if (post_total - pre_total).abs() > 0.01 {
                self.total_refresh.set(self.total_refresh.get() + 1);
            }
        }

        let scroll_y = self.scroll_y.get();
        for (i, child) in children.iter_mut().enumerate() {
            let (model_index, _) = self.row_entries[i];
            let (top, height) = {
                let mut m = self.row_metrics.borrow_mut();
                (m.row_top(model_index), m.row_height(model_index))
            };
            let y = bounds.y + top - scroll_y;
            child.origin = Point::new(bounds.x, y);
            child.size = Size::new(bounds.width, height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Body pane stands in as the table's `Role::RowGroup` — the
        // ARIA-blessed intermediate between `Role::Table` and
        // `Role::Row`. Without a non-hidden role here, AT clients
        // that walk `Table > Row` directly would balk at a hidden
        // generic container in the path.
        builder.set_role(bastyde_core::accesskit::Role::RowGroup);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.row_entries.iter().map(|(_, id)| *id).collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}

fn read_item_local<T, R>(
    with_item_fn: &WithItemFn<T>,
    idx: usize,
    f: impl FnOnce(&T) -> R,
) -> Option<R> {
    let f_cell: Cell<Option<_>> = Cell::new(Some(f));
    let slot: Cell<Option<R>> = Cell::new(None);
    (with_item_fn)(idx, &|item: &T| {
        if let Some(f) = f_cell.take() {
            slot.set(Some(f(item)));
        }
    });
    slot.into_inner()
}
