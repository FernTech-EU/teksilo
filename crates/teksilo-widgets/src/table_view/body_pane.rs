// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{DragEligibility, RowState, SelectionMode};

use super::a11y::CellA11y;
use super::body::{BodyRow, SharedColumnWidths};
use super::column::{CellContext, Column};
use super::selection::{CellSelectionModel, TableSelectionMode};
use crate::common::row_metrics::SharedRowMetrics;
use crate::data_views::{RowSelection, ViewId, default_placeholder};
use crate::table_view::column::EditTriggers;

const BUFFER_ROWS: usize = 5;

pub(crate) type LenFn = Rc<dyn Fn() -> usize>;
pub(crate) type WithItemFn<T> = Rc<dyn Fn(usize, &dyn Fn(&T))>;

/// The row-virtualization pane. Owns the visible row widgets and
/// handles their per-row click + drag handlers. Sized to fill the
/// caller's proposal; lays each row at `flat_index * row_height -
/// scroll_y` in pane-local coordinates.
pub(crate) struct BodyPane<T: 'static> {
    pub(crate) len_fn: LenFn,
    pub(crate) with_item_fn: WithItemFn<T>,
    /// Source per-row drag gate — `NoDrag` suppresses the drag gesture.
    pub(crate) drag_fn: Rc<dyn Fn(usize) -> DragEligibility>,
    /// Source per-row load state — a `Loading` row renders a placeholder
    /// skeleton instead of its cells.
    pub(crate) row_state_fn: Rc<dyn Fn(usize) -> RowState>,

    pub(crate) columns: Vec<Column<T>>,
    pub(crate) display_indices: Rc<RefCell<Vec<usize>>>,
    pub(crate) column_widths: SharedColumnWidths,
    /// Pane partition (Leading/Middle/Trailing), snapshotted at build —
    /// forwarded to each `BodyRow` for the pane-band split. See
    /// `body::BodyRow`'s module docs.
    pub(crate) pane_boundaries: super::PaneBoundaries,
    /// Middle-pane horizontal scroll offset, forwarded to each `BodyRow`.
    pub(crate) scroll_x: Signal<f32>,

    /// Row geometry shared with the `TableView` root (one handle, two
    /// holders — the root drives scrollbar totals / paint / keyboard,
    /// the pane drives realization, placement, and measurement).
    pub(crate) row_metrics: SharedRowMetrics,
    pub(crate) selection_mode: TableSelectionMode,
    pub(crate) selection: Option<RowSelection>,
    pub(crate) cell_selection: Option<CellSelectionModel>,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    pub(crate) editing_cell: Signal<Option<(usize, usize)>>,
    pub(crate) focused_cell: Signal<Option<(usize, usize)>>,

    pub(crate) reorderable: bool,
    /// Cross-widget export / foreign-receive machinery, cloned in from the
    /// owning `TableView` — builds the drag-start payload here; the
    /// self-reorder flag and removal-thunk stash are Rc-backed, so mutations
    /// made through this clone are visible to the root's `on_drag_ended`
    /// completion (installed on the owning `TableView`'s own clone).
    pub(crate) export: crate::data_views::RowExport<T>,
    /// Source-side move-out completion: resolves stable keys at drag-start
    /// and returns a removal thunk. Threaded straight from the owning
    /// `TableView`'s `dnd` bundle.
    pub(crate) snapshot_out_fn: crate::data_views::SnapshotOutFn,
    /// Resolve a row index to a movement-proof handle. Threaded from the
    /// owning `TableView`'s source, like `snapshot_out_fn`, because the pane
    /// gets erased closures rather than the source itself.
    pub(crate) anchor_fn: Rc<dyn Fn(usize) -> crate::data_views::RowAnchor>,
    /// Anchor slot for the row with an open cell editor (Rc-shared with the
    /// owning `TableView`, so it survives this pane being rebuilt).
    pub(crate) editing_anchor: Rc<RefCell<Option<crate::data_views::RowAnchor>>>,
    /// Stable, kind-tagged id of the owning `TableView` instance — stamped
    /// into the `RowDragData` payload so the source can tell a same-view
    /// reorder from a foreign drop.
    pub(crate) view_id: ViewId,

    /// Optional row-activation callback (a click per `activate_on`, or
    /// Enter/Space on the focused row) — distinct from *selection*, which also
    /// moves on arrow navigation.
    pub(crate) on_row_activate: Option<Rc<dyn Fn(usize, &mut teksilo_core::widget::EventContext)>>,
    /// Whether activation is a single or double click (default `DoubleClick`).
    pub(crate) activate_on: crate::data_views::ActivateOn,
    /// Which gestures open a cell editor. The pane implements the
    /// **double-click** arm; F2 and type-to-edit live in the shared key
    /// handler (`table_view::keyboard`), which is the root's business.
    pub(crate) edit_triggers: crate::table_view::EditTriggers,
    /// Fired with `(flat row, column id)` when a double-click opens an editor,
    /// so the owner can seed its buffer — the same callback the keyboard
    /// routes use.
    pub(crate) on_cell_edit_request:
        Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    /// Fired when a press lands outside the cell currently being edited, so the
    /// owner can end that edit. See `TableView::on_cell_edit_dismissed`.
    pub(crate) on_cell_edit_dismissed:
        Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
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
    /// The realized `(row index -> row wrapper id)` map, shared with the owning
    /// view so its `&self` methods can resolve a row index to a widget without
    /// reaching into this pane.
    pub(crate) row_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,
    /// `(row, display_pos) -> WidgetId` for every realized cell, shared
    /// with the `TableView` root (the GridView `tile_map` pattern).
    /// Overwritten wholesale at the end of every `build()`; the root's
    /// `accessibility()` reads it to resolve `active_descendant` for the
    /// keyboard-focused cell.
    pub(crate) cell_map: Rc<RefCell<Vec<((usize, usize), WidgetId)>>>,
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
        // The pane rebuilds on BOTH an editing change and a data change, so
        // this is the one place that sees every transition an open editor has
        // to survive.
        let anchor_fn = self.anchor_fn.clone();
        crate::data_views::reconcile_editing_row(&self.editing_cell, &self.editing_anchor, &|i| {
            anchor_fn(i)
        });
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
                    metrics.borrow_mut().visible_range(*y, vp_h.get(), count, 0);
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
            let handle = sel.observe_for_rebuild(move || {
                counter.set(counter.get() + 1);
                v.set(counter.get());
            });
            ctx.own_handle(handle);
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
        let mut cell_entries: Vec<((usize, usize), WidgetId)> = Vec::new();
        let (start, end) = self.visible_range();
        let columns = self.columns.clone();
        let with_item_fn = self.with_item_fn.clone();
        let display_indices = self.display_indices.borrow().clone();
        // Column ids in display order, so a dismissal can name the column whose
        // editor it is ending rather than the one that was pressed.
        let display_col_ids: Rc<Vec<String>> = Rc::new(
            display_indices
                .iter()
                .map(|&i| columns[i].id.clone())
                .collect(),
        );
        let editing_state = self.editing_cell.get();
        let row_widths_handle = self.column_widths.clone();
        let selection_mode = self.selection_mode;
        // Rows become a drag source when reorderable OR exportable — the
        // export path makes a row draggable-out even when same-view
        // reordering is disabled.
        let is_drag_source = self.export.is_drag_source(self.reorderable);

        // Key the row focus scope on the table's focusable root (`drag_anchor`),
        // not this pane — keyboard focus lands on the root, so a `StandardItem`
        // cell's focus-aware selection must track the root's focus.
        ctx.begin_view_focus_for(self.drag_anchor);
        for row_idx in start..end {
            // One anchor per row, cloned into every handler that addresses the
            // row — the cells' double-click-to-edit as well as the row's own
            // selection / activation / drag. Resolved once here, above the cell
            // loop, so all of them agree on which row this is.
            let row_anchor = (self.anchor_fn)(row_idx);
            let row_selected_for_a11y = match (selection_mode, &self.selection) {
                (TableSelectionMode::SingleRow | TableSelectionMode::MultiRow, Some(s)) => {
                    s.is_selected(row_idx)
                }
                _ => false,
            };

            // A non-resident row (data not yet loaded) whose source reports
            // `Loading` renders placeholder cells instead of being skipped,
            // so the scrollbar and layout stay stable while the window loads.
            let resident = read_item_local(&with_item_fn, row_idx, |_| ()).is_some();
            let loading = !resident && (self.row_state_fn)(row_idx) == RowState::Loading;

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
                let cell_widget = if loading {
                    Some(default_placeholder())
                } else {
                    read_item_local(&with_item_fn, row_idx, |item| (col.cell)(item, &cell_ctx))
                };
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
                    //
                    // `focus_into` rather than a bare `focus` on the
                    // first focusable descendant: it is a no-op while
                    // focus is already inside the cell, so the rebuild
                    // storm a table lives in (selection, filtering,
                    // scroll, the edit signal itself) cannot yank the
                    // caret back to the field's start mid-edit.
                    if is_editing {
                        ctx.focus_into(inner_id);
                    }
                    let cell_a11y = CellA11y::new(
                        inner_id,
                        row_idx + 2, // header is row 1
                        display_pos + 1,
                        is_selected,
                    )
                    // `Role::GridCell` only where the cell really is the
                    // selectable unit; see `CellA11y::is_grid_cell`.
                    .with_grid_cell_role(selection_mode.is_cell_mode());
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
                            teksilo_core::event::WidgetEvent::PointerDown {
                                button: teksilo_core::event::PointerButton::Primary,
                                modifiers,
                                ..
                            } => {
                                if editing_for_cell.get().is_some() {
                                    return teksilo_core::event::EventResponse::Ignored;
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
                                            } else if modifiers.command() {
                                                cs.toggle(row_for_cell, col_for_cell);
                                            } else {
                                                cs.select(row_for_cell, col_for_cell);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                teksilo_core::event::EventResponse::Ignored
                            }
                            _ => teksilo_core::event::EventResponse::Ignored,
                        });
                    ctx.apply_handlers(cell_id, cell_handlers);
                    // Click-to-edit, from this column's `EditTriggers`. A second
                    // `apply_handlers` on the same node *merges* into its
                    // external bucket, so the focus-ring / cell-selection
                    // handler above survives.
                    if let Some(edit_handlers) = cell_edit_handlers(
                        col.effective_edit_triggers(self.edit_triggers),
                        &self.on_cell_edit_request,
                        &self.editing_cell,
                        &row_anchor,
                        display_pos,
                        &col.id,
                    ) {
                        ctx.apply_handlers(cell_id, edit_handlers);
                    }
                    if let Some(dismiss) = cell_edit_dismiss_handler(
                        &self.on_cell_edit_dismissed,
                        &self.editing_cell,
                        &display_col_ids,
                        &row_anchor,
                        display_pos,
                    ) {
                        ctx.apply_handlers(cell_id, dismiss);
                    }

                    cell_entries.push(((row_idx, display_pos), cell_id));
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
                self.pane_boundaries,
                self.scroll_x.clone(),
            );
            let row_id = ctx.add(row_widget);

            // A grid is *one* Tab stop — the cell cursor is the navigation, and
            // a control the delegate put in a cell (a checkbox column, most
            // often) must not become a Tab stop of its own. Only realized rows
            // exist, so a per-cell stop would make the Tab order follow the
            // scroll position: 36 stops in a 200-row table showing 35, and a
            // different 36 after scrolling. Honoured up the ancestor chain by
            // `tab_stop_effective`, so one call covers every cell in the row.
            // `Space` reaches the control through the focused cell's published
            // keyboard toggle instead.
            ctx.set_tab_stop(row_id, false);

            // Selection click on the row. Skipped while a cell is in
            // edit mode so clicks landing inside the editor (e.g. on
            // the cell's `TextInput`) don't change the selection — a
            // selection change would re-emit the row, destroying the
            // editor and dropping focus mid-click.
            let mut row_handlers = HandlerSet::new();
            // ScrollIntoView is the one scroll action all three AccessKit
            // adapters actually consume (UIA `IScrollItemProvider`, AppKit
            // `accessibilityScrollToVisible`, AT-SPI `ScrollTo`). Rows are not
            // focusable nodes, so without it a screen reader has no way to
            // bring one into view. `ListView` and `TreeView` have answered it
            // since they shipped; the tables did not.
            {
                let metrics = self.row_metrics.clone();
                let scroll = self.scroll_y.clone();
                let viewport = self.viewport_height.clone();
                let len = self.len_fn.clone();
                row_handlers = row_handlers.on_access_action(move |action, _ctx| {
                    if action != teksilo_core::accesskit::Action::ScrollIntoView {
                        return teksilo_core::event::EventResponse::Ignored;
                    }
                    let vh = viewport.get();
                    let cur = scroll.get();
                    let new = {
                        let mut m = metrics.borrow_mut();
                        let total = m.total_height(len());
                        let max = (total - vh).max(0.0);
                        m.scroll_for_ensure_visible(row_idx, cur, vh, max)
                    };
                    if (new - cur).abs() > f32::EPSILON {
                        scroll.set(new);
                    }
                    teksilo_core::event::EventResponse::Handled
                });
            }
            if let Some(ref sel) = self.selection {
                let click_anchor = row_anchor.clone();
                let sel_for_click = sel.clone();
                let editing_for_click = self.editing_cell.clone();
                if matches!(
                    selection_mode,
                    TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
                ) {
                    // Deferred collapse: pressing an already-selected row
                    // keeps the whole (multi-)selection so it can be
                    // dragged; the collapse-to-single happens on release
                    // WITHOUT a drag.
                    let pending_collapse = Rc::new(Cell::new(false));
                    row_handlers = row_handlers.on_pointer_event(move |event, ctx| match event {
                        teksilo_core::event::WidgetEvent::PointerDown {
                            button: teksilo_core::event::PointerButton::Primary,
                            modifiers,
                            ..
                        } => {
                            if editing_for_click.get().is_some() {
                                return teksilo_core::event::EventResponse::Ignored;
                            }
                            // The press belongs to an interactive child (an
                            // embedded checkbox, button, …) — let it handle the
                            // tap; don't also select the row. Clear any stale
                            // deferred-collapse (left by a prior drag whose
                            // PointerUp the drag machinery consumed) so it can't
                            // fire on this unrelated interaction.
                            if ctx.press_claimed_by_interactive_child() {
                                pending_collapse.set(false);
                                return teksilo_core::event::EventResponse::Ignored;
                            }
                            // Resolve the row's CURRENT position only after the
                            // guards above have run — the interactive-child
                            // branch clears stale deferred-collapse state, and
                            // returning before it would strand that flag.
                            let Some(row_index_for_click) = click_anchor.index() else {
                                return teksilo_core::event::EventResponse::Ignored;
                            };
                            // Nav-cursor sync (`focused_cell`) is handled by the
                            // per-cell pointer handler above, which fires on any
                            // cell click in every mode — so a row click here already
                            // moves the arrow-nav origin. (TreeTableView has no such
                            // per-cell handler, so it syncs in its row handler.)
                            if modifiers.command() && sel_for_click.mode() == SelectionMode::Multi {
                                sel_for_click.toggle(row_index_for_click);
                                pending_collapse.set(false);
                            } else if modifiers.shift()
                                && sel_for_click.mode() == SelectionMode::Multi
                            {
                                sel_for_click.extend_to(row_index_for_click);
                                pending_collapse.set(false);
                            } else if sel_for_click.is_selected(row_index_for_click) {
                                // Defer: a following drag preserves the whole
                                // selection; a plain click collapses on release.
                                pending_collapse.set(true);
                            } else {
                                sel_for_click.select(row_index_for_click);
                                pending_collapse.set(false);
                            }
                            // Ignored so the gesture arena on this widget
                            // still sees the PointerDown and can arm the
                            // DragRecognizer for drag-to-reorder/export
                            // alongside selection.
                            teksilo_core::event::EventResponse::Ignored
                        }
                        teksilo_core::event::WidgetEvent::PointerUp {
                            button: teksilo_core::event::PointerButton::Primary,
                            ..
                        } => {
                            // A release on an interactive child is that
                            // child's tap — never collapse the row from it
                            // (guards against a `pending_collapse` a prior
                            // drag left stuck true).
                            if ctx.press_claimed_by_interactive_child() {
                                return teksilo_core::event::EventResponse::Ignored;
                            }
                            // Reached only on a click WITHOUT a drag (an
                            // active drag consumes PointerUp). Collapse the
                            // deferred multi-selection to the clicked row.
                            if pending_collapse.replace(false)
                                && let Some(row) = click_anchor.index()
                            {
                                sel_for_click.select(row);
                            }
                            teksilo_core::event::EventResponse::Ignored
                        }
                        _ => teksilo_core::event::EventResponse::Ignored,
                    });
                }
            }
            if is_drag_source {
                let drag_row = row_idx;
                let view_id = self.view_id;
                let anchor = self.drag_anchor;
                let drag_gate = self.drag_fn.clone();
                let with_item_for_preview = self.with_item_fn.clone();
                let columns_for_preview = self.columns.clone();
                let display_for_preview = self.display_indices.clone();
                let widths_for_preview = self.column_widths.clone();
                let metrics_for_preview = self.row_metrics.clone();
                // Export capture: the dragged set is selection-aware; the
                // shared `RowExport` builds the payload (clones / MIME /
                // Loading-filter / stash) when the view opted in.
                let sel_for_drag = self.selection.clone();
                let export_for_drag = self.export.clone();
                let with_item_for_drag = self.with_item_fn.clone();
                let snapshot_for_drag = self.snapshot_out_fn.clone();
                row_handlers = row_handlers.on_drag(move |phase, ctx| {
                    if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                        // The source's per-row transferable gate.
                        if (drag_gate)(drag_row) == DragEligibility::NoDrag {
                            return;
                        }
                        // Selection-aware dragged set: the whole selection
                        // when the pressed row is part of a multi-selection,
                        // else just the pressed row.
                        let rows: Vec<usize> = match sel_for_drag.as_ref() {
                            Some(s) if s.is_selected(drag_row) => {
                                let mut v = s.selected_indices();
                                v.sort_unstable();
                                if v.len() <= 1 { vec![drag_row] } else { v }
                            }
                            _ => vec![drag_row],
                        };
                        // Adapt the side-effect `with_item_fn` reader to the
                        // `RowExport::build_payload` signature (which needs a
                        // bool-returning "did it resolve" reader).
                        let read = |i: usize, f: &mut dyn FnMut(&T)| -> bool {
                            read_item_local(&with_item_for_drag, i, |t| f(t)).is_some()
                        };
                        let Some(payload) =
                            export_for_drag.build_payload(view_id, rows, &read, &snapshot_for_drag)
                        else {
                            return;
                        };
                        // Build a full-width preview from the PRESSED row's
                        // cells so the floating widget reads as the picked-up
                        // row. Cells are built eagerly here (no arena), then a
                        // self-contained `CellRowPreview` lays them out.
                        let display = display_for_preview.borrow().clone();
                        let cells: Vec<Box<dyn Widget>> =
                            read_item_local(&with_item_for_preview, drag_row, |item| {
                                display
                                    .iter()
                                    .enumerate()
                                    .map(|(display_pos, &col_idx)| {
                                        let col = &columns_for_preview[col_idx];
                                        let cell_ctx = CellContext {
                                            row_index: drag_row,
                                            col_id: col.id.clone(),
                                            col_index: display_pos,
                                            is_selected: false,
                                            is_focused: false,
                                            is_hovered: false,
                                            is_editing: false,
                                            depth: None,
                                            is_tree_column: false,
                                        };
                                        (col.cell)(item, &cell_ctx)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        if cells.is_empty() {
                            ctx.start_drag(anchor, payload);
                            return;
                        }
                        let widths = widths_for_preview.borrow().clone();
                        let h = metrics_for_preview.borrow_mut().row_height(drag_row);
                        let total_w = widths.iter().sum::<f32>().max(120.0);
                        let preview = Box::new(crate::drag_preview::DragPreview::new(
                            total_w,
                            h,
                            Box::new(CellRowPreview::new(cells, widths, h)),
                        )) as Box<dyn Widget>;
                        ctx.start_drag_with_preview(anchor, payload, preview);
                    }
                });
            }
            // Row activation (open/commit) — a gesture, so it arbitrates
            // against the reorder drag via the gesture arena (a click
            // activates, a drag does not). `SingleClick` → `on_tap`,
            // `DoubleClick` → `on_double_tap`; Enter/Space activates too.
            if let Some(ref cb) = self.on_row_activate {
                let cb = cb.clone();
                // Anchored: a row that moved (or vanished) between build and
                // click must not activate whoever took its slot.
                let a = row_anchor.clone();
                let handlers = match self.activate_on {
                    crate::data_views::ActivateOn::SingleClick => {
                        let a = a.clone();
                        HandlerSet::new().on_tap(move |_tap, ctx| {
                            if let Some(cur) = a.index() {
                                cb(cur, ctx)
                            }
                        })
                    }
                    crate::data_views::ActivateOn::DoubleClick => {
                        // **Editing wins over activation, and the framework
                        // arbitrates it — there is nothing to guard here.** A
                        // node carrying a gesture arena answers `Handled` to
                        // the press and the bubble stops there, so once a cell
                        // takes a click trigger its row's activation no longer
                        // sees clicks on *that column*. Which is the wanted
                        // reading: a column that edits on double-click must not
                        // also open the row on the same gesture. Every other
                        // column still activates.
                        HandlerSet::new().on_double_tap(move |_tap, ctx| {
                            if let Some(cur) = a.index() {
                                cb(cur, ctx)
                            }
                        })
                    }
                };
                ctx.apply_handlers(row_id, handlers);
            }
            ctx.apply_handlers(row_id, row_handlers);

            self.row_entries.push((row_idx, row_id));
        }
        ctx.end_view_focus();

        *self.cell_map.borrow_mut() = cell_entries;

        // Publish the realized (index -> row wrapper id) map for the view.
        *self.row_map.borrow_mut() = self.row_entries.clone();

        self.row_entries.iter().map(|(_, id)| *id).collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Only an allocation may seed the cached viewport — a measurement's
        // fallback would desync `build`'s realization window (`common::viewport`).
        crate::common::viewport::viewport_size(
            proposal,
            &self.viewport_height,
            Size::new(400.0, 300.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // The allocated height is the authoritative viewport: `build` sizes its
        // realization window from this, and a stale value there costs a
        // permanent rebuild loop (`common::viewport`).
        crate::common::viewport::record_viewport_height(&self.viewport_height, bounds.height);

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
                self.scroll_y.set((self.scroll_y.get() + anchor).max(0.0));
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
        builder.set_role(teksilo_core::accesskit::Role::RowGroup);
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

/// Self-contained, arena-free row preview for the drag floating widget: it
/// owns its (already-built) boxed cells and lays them out horizontally at the
/// dragged row's column widths. Built once at drag-start and mounted by the
/// framework's drag-overlay build pass, so it can't reuse `BodyRow` (which
/// addresses cells by arena id). Shared with `TreeTableView`'s body pane.
pub(crate) struct CellRowPreview {
    /// Cells to mount, drained in `build`.
    cells: Vec<Box<dyn Widget>>,
    /// Display-order column widths, parallel to the mounted children.
    widths: Vec<f32>,
    height: f32,
    ids: Vec<WidgetId>,
}

impl CellRowPreview {
    pub(crate) fn new(cells: Vec<Box<dyn Widget>>, widths: Vec<f32>, height: f32) -> Self {
        Self {
            cells,
            widths,
            height,
            ids: Vec::new(),
        }
    }
}

impl std::fmt::Debug for CellRowPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CellRowPreview")
            .field("cells", &self.ids.len())
            .finish()
    }
}

impl Widget for CellRowPreview {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.ids = std::mem::take(&mut self.cells)
            .into_iter()
            .map(|w| ctx.add_boxed(w))
            .collect();
        self.ids.clone()
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        Size::new(self.widths.iter().sum::<f32>().max(1.0), self.height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let mut x = bounds.x;
        for (i, child) in children.iter_mut().enumerate() {
            let w = self.widths.get(i).copied().unwrap_or(0.0);
            child.origin = Point::new(x, bounds.y);
            child.size = Size::new(w, bounds.height);
            x += w;
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.ids.clone()
    }
}

/// The click half of [`EditTriggers`], as handlers for one cell.
///
/// Shared by both body panes so `TableView` and `TreeTableView` cannot drift
/// about what a click on an editable cell means — the drift that left the tree
/// table with no keyboard focus in its editors for as long as it has existed.
///
/// Returns `None` when this column opens no editor by click, so a cell that
/// wants neither trigger gets no handler at all — and stays out of the
/// gesture-arena bookkeeping entirely.
///
/// Nothing here has to suppress row activation: a node carrying a gesture arena
/// answers `Handled` to the press, so the bubble stops at the cell and the row's
/// activation never sees a click on a column that took a click trigger.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cell_edit_handlers(
    triggers: EditTriggers,
    request: &Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    editing_cell: &Signal<Option<(usize, usize)>>,
    row_anchor: &crate::data_views::RowAnchor,
    display_pos: usize,
    col_id: &str,
) -> Option<HandlerSet> {
    let single = triggers.contains(EditTriggers::SINGLE_CLICK);
    let double = triggers.contains(EditTriggers::DOUBLE_CLICK);
    if !(single || double) {
        return None;
    }
    let request = request.clone()?;

    // Anchored like every other row-addressed handler in a body pane: a row
    // that moved between build and click must not hand its edit to whoever
    // took its slot.
    let open = {
        let editing_cell = editing_cell.clone();
        let anchor = row_anchor.clone();
        let col_id = col_id.to_string();
        move |ctx: &mut teksilo_core::widget::EventContext| {
            if let Some(row) = anchor.index() {
                editing_cell.set(Some((row, display_pos)));
                request(row, &col_id, ctx);
            }
        }
    };
    let open = Rc::new(open);

    let mut handlers = HandlerSet::new();
    if single {
        let open = open.clone();
        handlers = handlers.on_tap(move |_tap, ctx| open(ctx));
    }
    if double {
        // Only when `SINGLE_CLICK` is off: with both set the first click has
        // already opened the editor, and a second one arriving as a `DoubleTap`
        // would re-seed the buffer from the model — silently discarding what
        // the writer typed between the two clicks. (The gesture arena would not
        // deliver it anyway: `TapRecognizer` is skipped whenever a multi-tap
        // recognizer is present, so wiring both would cost the single click
        // instead.)
        if !single {
            handlers = handlers.on_double_tap(move |_tap, ctx| open(ctx));
        }
    }
    Some(handlers)
}

/// Ends an open edit when a press lands on **some other cell**.
///
/// Shared by both body panes, next to [`cell_edit_handlers`], so the two tables
/// cannot disagree about when an edit stops.
///
/// `on_pointer_event` rather than a tap: it is not a gesture, so this adds no
/// recognizer to the cell and does not make it a tap owner — a cell carrying
/// this still selects its row on a press exactly as before. It also fires on
/// the press, not the release, which is what makes "click away and the value is
/// kept" feel immediate.
///
/// **A press inside the edited cell is not a dismissal.** Clicking into the open
/// field to move the caret, select a word or drag over text all land there, and
/// every one of them would otherwise close the editor under the pointer.
///
/// Deliberately reports the **editing** cell, not the pressed one: the owner is
/// being told which edit ended, and it has to be able to write that row's value
/// back. `EventResponse::Ignored` throughout — ending an edit is a side effect
/// of the press, never a reason to swallow it, so the press goes on to select
/// its row (or open its own editor) in the same gesture.
pub(crate) fn cell_edit_dismiss_handler(
    dismissed: &Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    editing_cell: &Signal<Option<(usize, usize)>>,
    display_col_ids: &Rc<Vec<String>>,
    row_anchor: &crate::data_views::RowAnchor,
    display_pos: usize,
) -> Option<HandlerSet> {
    let dismissed = dismissed.clone()?;
    let editing_cell = editing_cell.clone();
    let col_ids = display_col_ids.clone();
    let anchor = row_anchor.clone();
    Some(HandlerSet::new().on_pointer_event(move |event, ctx| {
        if let teksilo_core::event::WidgetEvent::PointerDown {
            button: teksilo_core::event::PointerButton::Primary,
            ..
        } = event
            && let Some((edit_row, edit_col)) = editing_cell.get()
            && (edit_col != display_pos || anchor.index() != Some(edit_row))
            && let Some(col_id) = col_ids.get(edit_col)
        {
            dismissed(edit_row, col_id, ctx);
        }
        teksilo_core::event::EventResponse::Ignored
    }))
}

/// The same dismissal, mounted on the **table root** so it also catches a press
/// that lands on no cell at all — the empty band below the last row, the gutter
/// beside a short column set, the header strip.
///
/// Guarded on `press_claimed_by_interactive_child`, which here is exactly the
/// question "did this press belong to a control inside the table": the open
/// editor is a `TextInput` and owns its taps, so clicking into the field to move
/// the caret or drag over a word is claimed, and is not a dismissal. A press on
/// a plain cell is not claimed and so dismisses here as well as through its own
/// handler — the second call is a no-op, there being no open edit left to end.
pub(crate) fn root_edit_dismiss_handler(
    dismissed: &Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
    editing_cell: &Signal<Option<(usize, usize)>>,
    display_col_ids: &Rc<Vec<String>>,
) -> Option<HandlerSet> {
    let dismissed = dismissed.clone()?;
    let editing_cell = editing_cell.clone();
    let col_ids = display_col_ids.clone();
    Some(HandlerSet::new().on_pointer_event(move |event, ctx| {
        if let teksilo_core::event::WidgetEvent::PointerDown {
            button: teksilo_core::event::PointerButton::Primary,
            ..
        } = event
            && !ctx.press_claimed_by_interactive_child()
            && let Some((edit_row, edit_col)) = editing_cell.get()
            && let Some(col_id) = col_ids.get(edit_col)
        {
            dismissed(edit_row, col_id, ctx);
        }
        teksilo_core::event::EventResponse::Ignored
    }))
}
