// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeBodyPane<T>` — the virtualized row pane underneath `TreeTableView`'s
//! header.
//!
//! Mirrors `table_view::body_pane::BodyPane` (see its module doc for the
//! full rationale): the framework's rebuild deferral in
//! `process_pending_rebuilds` skips rebuilds that target any *ancestor*
//! of a pointer-captured widget, so row rebuilds rooted on the
//! `TreeTableView` (an ancestor of the scrollbar) stall during a
//! thumb drag. With the rows owned by this pane — a *sibling* of the
//! scrollbar — buffer-exit, selection, editing, and expand/collapse
//! rebuilds keep materializing rows mid-drag.
//!
//! Not parameterized into `BodyPane<T>` deliberately: the tree case
//! needs `SortFilterTreeModel` access (`entry_at` / `with_entry` with
//! `FlatEntry` metadata), the tree-column indent + `TwistArrow`
//! wrapping, `CellContext::depth`, and the `BodyRow::a11y_hidden()` +
//! `TreeRowA11y` row shape — injecting all that as hooks would bloat
//! the flat pane's clean closure surface.
//!
//! `TreeBodyPane` is `pub(crate)` — applications still talk to
//! `TreeTableView`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, PointerButton, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{SelectionMode, SelectionModel, SortFilterTreeModel};

use super::TreeTableRowDragData;
use crate::common::row_metrics::SharedRowMetrics;
use crate::primitives::{HStack, Padding, TwistArrow};
use crate::styles::recipe_table_style as cp;
use crate::table_view::a11y::{CellA11y, TreeRowA11y};
use crate::table_view::body::{BodyRow, SharedColumnWidths};
use crate::table_view::body_pane::CellRowPreview;
use crate::table_view::column::{CellContext, Column};
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};

const BUFFER_ROWS: usize = 5;

/// The tree-row virtualization pane. Owns the visible row widgets
/// (indent + twist + cells, wrapped in `BodyRow` + `TreeRowA11y`) and
/// their per-row click handlers.
pub(crate) struct TreeBodyPane<T: 'static> {
    pub(crate) proxy: SortFilterTreeModel<T>,

    pub(crate) columns: Vec<Column<T>>,
    pub(crate) display_indices: Rc<RefCell<Vec<usize>>>,
    pub(crate) column_widths: SharedColumnWidths,
    /// Display position of the tree column (indent + twist host).
    pub(crate) tree_display_pos: usize,
    pub(crate) indent_per_level: f32,

    /// Row geometry shared with the `TreeTableView` root (the root drives
    /// scrollbar totals / paint / keyboard, the pane drives
    /// realization, placement, and measurement).
    pub(crate) row_metrics: SharedRowMetrics,
    pub(crate) selection_mode: TableSelectionMode,
    pub(crate) selection: Option<SelectionModel>,
    pub(crate) cell_selection: Option<CellSelectionModel>,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    pub(crate) editing_cell: Signal<Option<(usize, usize)>>,
    pub(crate) focused_cell: Signal<Option<(usize, usize)>>,

    /// Enable per-row drag-to-reorder (the root owns the drop validation +
    /// commit + sort gate; the pane only emits the drag).
    pub(crate) reorderable: bool,
    /// Owning `TreeTableView` id — stamped into the drag payload so a drop
    /// into a sibling table is rejected.
    pub(crate) table_id: usize,
    /// Drag-start anchor (the root id), captured so the drag closure stays
    /// `'static`.
    pub(crate) drag_anchor: WidgetId,

    /// Pane-local rebuild trigger. A persistent handle owned by the
    /// root (so it survives root rebuilds); also bumped by
    /// `place_children`'s post-measure realization re-check.
    pub(crate) version: Signal<u64>,
    /// Bound at `Relayout` on the `TreeTableView` ROOT — bumped when a
    /// measure pass changes the content total so the root re-places
    /// with the corrected `max_scroll_y` / thumb ratio next frame (the
    /// root computes them before this pane measures). See
    /// `table_view::body_pane::BodyPane::total_refresh`.
    pub(crate) total_refresh: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    pub(crate) prev_built_start: Rc<Cell<usize>>,
    pub(crate) prev_built_end: Rc<Cell<usize>>,

    // Build state
    pub(crate) row_entries: Vec<(usize, WidgetId)>,
}

impl<T: 'static> TreeBodyPane<T> {
    fn visible_range(&self) -> (usize, usize) {
        self.row_metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.proxy.visible_count(),
            BUFFER_ROWS,
        )
    }
}

impl<T: 'static> std::fmt::Debug for TreeBodyPane<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeBodyPane")
            .field("rows", &self.proxy.visible_count())
            .field("columns", &self.columns.len())
            .finish()
    }
}

impl<T: 'static> Widget for TreeBodyPane<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Self-rebuild trigger (persistent handle — see field doc).
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll position re-places rows without rebuilding (within buffer).
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Buffer-exit detection. Bumps version → rebuild THIS pane,
        // which the deferral logic never skips during a scrollbar thumb
        // drag (the pane is a sibling of the scrollbar, not an
        // ancestor). The pre-split TreeTableView had no buffer-exit
        // observer at all — scrolling past the buffer left stale rows
        // until the next unrelated rebuild.
        let proxy_for_scroll = self.proxy.clone();
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
                let count = proxy_for_scroll.visible_count();
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

        // Selection / focus / editing changes — refresh the
        // `is_selected` / `is_focused` / `is_editing` flags fed into
        // the cell delegates.
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
        let v_for_focus = version.clone();
        let focus_counter = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.focused_cell, move |_| {
            focus_counter.set(focus_counter.get() + 1);
            v_for_focus.set(focus_counter.get());
        });
        let v_for_edit = version.clone();
        let edit_counter = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.editing_cell, move |_| {
            edit_counter.set(edit_counter.get() + 1);
            v_for_edit.set(edit_counter.get());
        });

        // Build the visible row range.
        self.row_entries.clear();
        let (start, end) = self.visible_range();
        let display_indices = self.display_indices.borrow().clone();
        let columns = self.columns.clone();
        let proxy = self.proxy.clone();
        let selection = self.selection.clone();
        let cell_selection = self.cell_selection.clone();
        let selection_mode = self.selection_mode;
        let editing_state = self.editing_cell.get();
        let indent_per_level = self.indent_per_level;
        let tree_display_pos = self.tree_display_pos;

        for flat_idx in start..end {
            let entry = match proxy.entry_at(flat_idx) {
                Some(e) => e,
                None => continue,
            };
            let row_selected = match (selection_mode, &selection) {
                (TableSelectionMode::SingleRow | TableSelectionMode::MultiRow, Some(s)) => {
                    s.is_selected(flat_idx)
                }
                _ => false,
            };

            let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
            for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                let col = &columns[col_idx];
                let is_tree_column = display_pos == tree_display_pos;
                let is_selected = match (selection_mode, &selection, &cell_selection) {
                    (TableSelectionMode::SingleRow | TableSelectionMode::MultiRow, Some(s), _) => {
                        s.is_selected(flat_idx)
                    }
                    (
                        TableSelectionMode::SingleCell | TableSelectionMode::MultiCell,
                        _,
                        Some(cs),
                    ) => cs.is_selected(flat_idx, display_pos),
                    _ => false,
                };
                let is_focused = self.focused_cell.get() == Some((flat_idx, display_pos));
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
                let inner_widget =
                    proxy.with_entry(flat_idx, |item, _| (col.cell)(item, &cell_ctx));
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
                        TwistArrow::new(cp::TREE_TWIST_SIZE, entry.has_children, entry.is_expanded)
                            .on_click(move |_ctx| {
                                proxy_for_twist.toggle(twist_node_id);
                            }),
                    );
                    // Build inside-out so each `ctx.add` happens
                    // outside the mutable borrow chain.
                    let twist_and_label = HStack::new()
                        .spacing(cp::TREE_TWIST_LABEL_GAP)
                        .add_child(twist)
                        .add_child(inner_id);
                    let twist_label_id = ctx.add(twist_and_label);
                    ctx.add(
                        Padding::new(0.0_f32, 0.0_f32, 0.0_f32, indent_px).child_id(twist_label_id),
                    )
                } else {
                    inner_id
                };

                let cell_a11y =
                    CellA11y::new(leading_id, flat_idx + 2, display_pos + 1, is_selected);
                cell_ids.push(ctx.add(cell_a11y));
            }

            // Wrap cells in BodyRow (Role::Row by default), but add the
            // level/expanded annotations via TreeRowA11y so screen
            // readers announce the depth. Auto-measure mode hands the
            // row a `None` height so its layout_response measures the
            // tallest cell.
            let needs_measure = self.row_metrics.borrow().needs_measure();
            let row_height = if needs_measure {
                None
            } else {
                Some(self.row_metrics.borrow_mut().row_height(flat_idx))
            };
            let row_widget = BodyRow::new(
                cell_ids,
                flat_idx + 2,
                row_selected,
                row_height,
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

            // Row handlers: selection click + optional drag-to-reorder
            // (combined so a row carries both, applied once).
            let mut row_handlers = HandlerSet::new();
            if let Some(ref sel) = selection
                && matches!(
                    selection_mode,
                    TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
                )
            {
                let sel_for_click = sel.clone();
                let row_index_for_click = flat_idx;
                row_handlers = row_handlers.on_pointer_event(move |event, _ctx| {
                    if let WidgetEvent::PointerDown {
                        button: PointerButton::Primary,
                        modifiers,
                        ..
                    } = event
                    {
                        if modifiers.ctrl() && sel_for_click.mode() == SelectionMode::Multi {
                            sel_for_click.toggle(row_index_for_click);
                        } else if modifiers.shift() && sel_for_click.mode() == SelectionMode::Multi {
                            sel_for_click.extend_to(row_index_for_click);
                        } else {
                            sel_for_click.select(row_index_for_click);
                        }
                    }
                    EventResponse::Ignored
                });
            }
            if self.reorderable {
                let source_node = entry.node_id;
                let table_id = self.table_id;
                let anchor = self.drag_anchor;
                let preview_flat = flat_idx;
                let proxy_for_preview = proxy.clone();
                let columns_for_preview = columns.clone();
                let display_for_preview = display_indices.clone();
                let widths_for_preview = self.column_widths.clone();
                let metrics_for_preview = self.row_metrics.clone();
                let tree_pos_for_preview = tree_display_pos;
                row_handlers = row_handlers.on_drag(move |phase, ctx| {
                    if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                        let payload = bastyde_core::drag_payload::DragPayload::typed(
                            TreeTableRowDragData {
                                source_node,
                                source_table_id: table_id,
                            },
                        );
                        // Flat multi-cell preview of the dragged row (indent is
                        // dropped in the floating preview — it reads as the
                        // row's content picked up).
                        let widths = widths_for_preview.borrow().clone();
                        let h = metrics_for_preview.borrow_mut().row_height(preview_flat);
                        let total_w = widths.iter().sum::<f32>().max(120.0);
                        let cells: Vec<Box<dyn Widget>> = proxy_for_preview
                            .with_entry(preview_flat, |item, e| {
                                display_for_preview
                                    .iter()
                                    .enumerate()
                                    .map(|(display_pos, &col_idx)| {
                                        let col = &columns_for_preview[col_idx];
                                        let cell_ctx = CellContext {
                                            row_index: preview_flat,
                                            col_id: col.id.clone(),
                                            col_index: display_pos,
                                            is_selected: false,
                                            is_focused: false,
                                            is_hovered: false,
                                            is_editing: false,
                                            depth: Some(e.depth),
                                            is_tree_column: display_pos == tree_pos_for_preview,
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
                        let preview = Box::new(crate::drag_preview::DragPreview::new(
                            total_w,
                            h,
                            Box::new(CellRowPreview::new(cells, widths, h)),
                        )) as Box<dyn Widget>;
                        ctx.start_drag_with_preview(anchor, payload, preview);
                    }
                });
            }
            ctx.apply_handlers(tree_row_id, row_handlers);
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
        // width (the row reports its tallest cell, height-for-width),
        // feed the heights back, and apply the scroll-anchor delta so
        // content above the viewport stays put. Measurements are
        // collected with NO metrics borrow held.
        if self.row_metrics.borrow().needs_measure() {
            let count = self.proxy.visible_count();
            let pre_total = self.row_metrics.borrow_mut().total_height(count);
            let mut measured = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                if let Some(size) = ctx.child_size(child.id, SizeProposal::with_width(bounds.width))
                {
                    let (flat_idx, _) = self.row_entries[i];
                    measured.push((flat_idx, size.height));
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

            // Total-refresh poke: re-place the root next frame when the
            // measure pass changed the content total (see the field doc).
            let post_total = self.row_metrics.borrow_mut().total_height(count);
            if (post_total - pre_total).abs() > 0.01 {
                self.total_refresh.set(self.total_refresh.get() + 1);
            }
        }

        let scroll_y = self.scroll_y.get();
        for (i, child) in children.iter_mut().enumerate() {
            let (flat_idx, _) = self.row_entries[i];
            let (top, height) = {
                let mut m = self.row_metrics.borrow_mut();
                (m.row_top(flat_idx), m.row_height(flat_idx))
            };
            let y = bounds.y + top - scroll_y;
            child.origin = Point::new(bounds.x, y);
            child.size = Size::new(bounds.width, height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The ARIA-blessed intermediate between `Role::TreeGrid` and
        // `Role::Row` (same shape as `BodyPane` under `Role::Table`).
        builder.set_role(bastyde_core::accesskit::Role::RowGroup);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.row_entries.iter().map(|(_, id)| *id).collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
