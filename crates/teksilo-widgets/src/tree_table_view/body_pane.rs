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

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{DragEligibility, RowState, SelectionMode};

use crate::common::row_metrics::SharedRowMetrics;
use crate::data_views::{RowSelection, ViewId, default_placeholder};
use crate::primitives::{HStack, Padding, TwistArrow};
use crate::styles::recipe_table_style as cp;
use crate::table_view::a11y::{CellA11y, TreeRowA11y};
use crate::table_view::body::{BodyRow, SharedColumnWidths};
use crate::table_view::body_pane::{CellRowPreview, cell_edit_dismiss_handler, cell_edit_handlers};
use crate::table_view::column::{CellContext, Column};
use crate::table_view::selection::{CellSelectionModel, TableSelectionMode};
use crate::tree_source::TreeSource;

const BUFFER_ROWS: usize = 5;

/// The tree-row virtualization pane. Owns the visible row widgets
/// (indent + twist + cells, wrapped in `BodyRow` + `TreeRowA11y`) and
/// their per-row click handlers.
pub(crate) struct TreeBodyPane<T: 'static> {
    /// Erased row access — index-keyed, so the pane works over any
    /// `TreeDataSource`, not just a `TreeModel`-backed projection.
    pub(crate) source: Rc<TreeSource<T>>,
    /// Anchor slot for the row with an open cell editor (Rc-shared with the
    /// owning `TreeTableView`, so it survives this pane being rebuilt).
    pub(crate) editing_anchor: Rc<RefCell<Option<crate::data_views::RowAnchor>>>,

    pub(crate) columns: Vec<Column<T>>,
    pub(crate) display_indices: Rc<RefCell<Vec<usize>>>,
    pub(crate) column_widths: SharedColumnWidths,
    /// Pane partition (Leading/Middle/Trailing), snapshotted at build —
    /// forwarded to each `BodyRow` for the pane-band split. See
    /// `table_view::body::BodyRow`'s module docs.
    pub(crate) pane_boundaries: crate::table_view::PaneBoundaries,
    /// Middle-pane horizontal scroll offset, forwarded to each `BodyRow`.
    pub(crate) scroll_x: Signal<f32>,
    /// Display position of the tree column (indent + twist host).
    pub(crate) tree_display_pos: usize,
    pub(crate) indent_per_level: f32,

    /// Row geometry shared with the `TreeTableView` root (the root drives
    /// scrollbar totals / paint / keyboard, the pane drives
    /// realization, placement, and measurement).
    pub(crate) row_metrics: SharedRowMetrics,
    pub(crate) selection_mode: TableSelectionMode,
    pub(crate) selection: Option<RowSelection>,
    pub(crate) cell_selection: Option<CellSelectionModel>,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    pub(crate) editing_cell: Signal<Option<(usize, usize)>>,
    pub(crate) focused_cell: Signal<Option<(usize, usize)>>,

    /// Enable per-row drag-to-reorder (the root owns the drop validation +
    /// commit + sort gate; the pane only emits the drag).
    pub(crate) reorderable: bool,
    /// Owning `TreeTableView`'s ROW-drag identity — stamped into the drag
    /// payload so same-view reorder vs. a foreign drop can be told apart.
    /// Distinct from the column-header `table_id: usize` (an unrelated
    /// resize/reorder identity `TreeTableView` keeps to itself).
    pub(crate) model_id: ViewId,

    /// Cross-widget export / foreign-receive machinery, shared with the
    /// root (`TreeTableView::export`). The pane builds the reader +
    /// stable-`NodeId` removal-thunk closures inline at drag-start (see
    /// `on_drag` below), since a `SortFilterTreeModel<T>`-backed view has no
    /// pluggable source to supply them.
    pub(crate) export: crate::data_views::RowExport<T>,

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
    /// owner can end that edit. See `TreeTableView::on_cell_edit_dismissed`.
    pub(crate) on_cell_edit_dismissed:
        Option<Rc<dyn Fn(usize, &str, &mut teksilo_core::widget::EventContext)>>,
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
    /// The realized `(row index -> row wrapper id)` map, shared with the owning
    /// view so its `&self` methods can resolve a row index to a widget without
    /// reaching into this pane.
    pub(crate) row_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,
    /// `(row, display_pos) -> WidgetId` for every realized cell, shared
    /// with the `TreeTableView` root (see `table_view::body_pane::BodyPane::cell_map`).
    /// Overwritten wholesale at the end of every `build()`.
    pub(crate) cell_map: Rc<RefCell<Vec<((usize, usize), WidgetId)>>>,
}

impl<T: 'static> TreeBodyPane<T> {
    fn visible_range(&self) -> (usize, usize) {
        self.row_metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.visible_count(),
            BUFFER_ROWS,
        )
    }
}

impl<T: 'static> std::fmt::Debug for TreeBodyPane<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeBodyPane")
            .field("rows", &self.source.visible_count())
            .field("columns", &self.columns.len())
            .finish()
    }
}

impl<T: 'static> Widget for TreeBodyPane<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The pane rebuilds on BOTH an editing change and a projection change,
        // so this is the one place that sees every transition an open editor
        // has to survive.
        let src = self.source.clone();
        crate::data_views::reconcile_editing_row(&self.editing_cell, &self.editing_anchor, &|i| {
            src.anchor(i)
        });

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
        let source_for_scroll = self.source.clone();
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
                let count = source_for_scroll.visible_count();
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
        let mut cell_entries: Vec<((usize, usize), WidgetId)> = Vec::new();
        let (start, end) = self.visible_range();
        let display_indices = self.display_indices.borrow().clone();
        let columns = self.columns.clone();
        // Column ids in display order, so a dismissal can name the column whose
        // editor it is ending rather than the one that was pressed.
        let display_col_ids: Rc<Vec<String>> = Rc::new(
            display_indices
                .iter()
                .map(|&i| columns[i].id.clone())
                .collect(),
        );
        let source = self.source.clone();
        let selection = self.selection.clone();
        let cell_selection = self.cell_selection.clone();
        let selection_mode = self.selection_mode;
        let editing_state = self.editing_cell.get();
        let indent_per_level = self.indent_per_level;
        let tree_display_pos = self.tree_display_pos;

        // Key the row focus scope on the view's focusable root (`drag_anchor`),
        // not this pane — see TableView's body pane for the rationale.
        ctx.begin_view_focus_for(self.drag_anchor);
        for flat_idx in start..end {
            // One anchor per row, cloned into each handler: building three
            // would allocate three closures (each capturing an Rc to the source
            // and a cloned key) on every rebuild, and rebuilds fire on every
            // filter keystroke.
            let row_anchor = source.anchor(flat_idx);
            let entry = source.meta(flat_idx);
            // A `TreeDataSource` returns `None` from `with_entry` (and so
            // from `meta`) both for a genuinely out-of-range index and for
            // an in-range-but-not-yet-loaded row — the trait gives no way
            // to tell those apart directly, so ask `row_state` (default
            // `Ready`, so a fully-resident source never takes this path).
            // A loading row renders placeholder cells instead of being
            // skipped, so the scrollbar and layout stay stable while the
            // window loads (mirrors `BodyPane`'s flat-table handling).
            let loading =
                entry.is_none() && (source.dnd.row_state_fn)(flat_idx) == RowState::Loading;
            if entry.is_none() && !loading {
                continue;
            }
            // Depth/has_children/is_expanded are unknowable for a loading
            // row (the trait can't hand back partial `FlatEntry` data
            // without the item) — render it as a depth-0 leaf; the real
            // values replace it once `with_entry` resolves and the pane
            // rebuilds on the source's version bump.
            let depth = entry.map(|e| e.depth).unwrap_or(0);
            let has_children = entry.map(|e| e.has_children).unwrap_or(false);
            let is_expanded = entry.map(|e| e.is_expanded).unwrap_or(false);
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
                    depth: (!loading).then_some(depth),
                    is_tree_column,
                };

                // Build the cell delegate widget — a placeholder while
                // loading, else the real delegate (which itself resolves
                // to `None` for a row that turned out not to be resident,
                // e.g. a race between `meta` and `with_row`).
                let cell_widget = if loading {
                    Some(default_placeholder())
                } else {
                    source.with_row(flat_idx, &|item, _meta| (col.cell)(item, &cell_ctx))
                };
                let inner_widget = match cell_widget {
                    Some(w) => w,
                    None => continue,
                };
                let inner_id = ctx.add_boxed(inner_widget);

                // The editor a cell delegate just swapped in has to *hold the
                // keyboard*, or the whole editing protocol is decorative: the
                // table keeps focus, so Escape, Enter and every character land
                // on the table's own key handler instead — Escape cancels
                // nothing, Enter activates the row, and typing runs type-ahead
                // over the value being edited. Nothing else focuses it: the
                // widget's `begin_edit` and the F2 / type-to-edit routes all
                // only write `editing_cell`.
                //
                // `TableView`'s body pane has always done this; the line was
                // simply left behind when the tree table was split out of it,
                // so `TreeTableView`'s inline editing has never been reachable
                // from the keyboard at all.
                //
                // `focus_into` and not `focus`: it is a no-op while focus is
                // already inside this cell, so the rebuild storm a table lives
                // in (selection, filtering, scroll, the edit signal itself)
                // cannot yank focus back mid-edit — while a rebuild that
                // *destroys and re-creates* the editor still restores it.
                if is_editing {
                    ctx.focus_into(inner_id);
                }

                // Wrap the tree-column's inner widget with indent +
                // twist arrow — skipped while loading, since has_children
                // isn't knowable yet (a placeholder twist would be a
                // guess, not information).
                let leading_id = if is_tree_column && !loading {
                    let indent_px = depth as f32 * indent_per_level;
                    let source_for_twist = source.clone();
                    let twist_anchor = row_anchor.clone();
                    let twist = ctx.add(
                        TwistArrow::new(cp::TREE_TWIST_SIZE, has_children, is_expanded).on_click(
                            move |_ctx| {
                                if let Some(i) = twist_anchor.index() {
                                    source_for_twist.toggle_at(i);
                                }
                            },
                        ),
                    );
                    // Build inside-out so each `ctx.add` happens
                    // outside the mutable borrow chain.
                    let twist_and_label = HStack::new()
                        .spacing(cp::TREE_TWIST_LABEL_GAP)
                        .add_child(twist)
                        .add_child(inner_id);
                    let twist_label_id = ctx.add(twist_and_label);
                    // Clip the indent + twist to the column. Both are rigid
                    // (a fixed indent per level, a fixed-size chevron), so a
                    // tree column dragged narrower than `depth * indent +
                    // twist + gap` cannot shrink to fit and would otherwise
                    // draw the chevron — and the whole label after it — on top
                    // of the next column. Cropping at the column edge is what
                    // Explorer / Finder / VS Code do, and it keeps the resize
                    // grip free to shrink the column all the way to its floor.
                    //
                    // Deliberately narrower than `TruncationPolicy`, which is
                    // about the *delegate's* content: `TruncationPolicy::None`
                    // documents that a cell may draw past its column edge, and
                    // that still holds — only this chrome wrapper clips.
                    let indent_id = ctx.add(
                        Padding::new(0.0_f32, 0.0_f32, 0.0_f32, indent_px).child_id(twist_label_id),
                    );
                    ctx.apply_handlers(
                        indent_id,
                        teksilo_core::widget_builder::HandlerSet::new().clips_children(true),
                    );
                    indent_id
                } else {
                    inner_id
                };

                let cell_a11y =
                    CellA11y::new(leading_id, flat_idx + 2, display_pos + 1, is_selected);
                let cell_id = ctx.add(cell_a11y);

                // Click-to-edit, from this column's `EditTriggers`. Per cell,
                // not per row: the row knows where the pointer was, the cell
                // knows which column it is — and which gestures that column
                // asked for. A click anywhere else still means whatever the
                // caller wired `on_row_activate` to.
                if let Some(handlers) = cell_edit_handlers(
                    col.effective_edit_triggers(self.edit_triggers),
                    &self.on_cell_edit_request,
                    &self.editing_cell,
                    &row_anchor,
                    display_pos,
                    &col.id,
                ) {
                    ctx.apply_handlers(cell_id, handlers);
                }
                if let Some(handlers) = cell_edit_dismiss_handler(
                    &self.on_cell_edit_dismissed,
                    &self.editing_cell,
                    &display_col_ids,
                    &row_anchor,
                    display_pos,
                ) {
                    ctx.apply_handlers(cell_id, handlers);
                }

                cell_entries.push(((flat_idx, display_pos), cell_id));
                cell_ids.push(cell_id);
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
                self.pane_boundaries,
                self.scroll_x.clone(),
            )
            .a11y_hidden();
            let row_inner_id = ctx.add(row_widget);
            // `(pos_in_set_1based, set_size)` among this row's siblings — a
            // loading row (no metadata resolved yet) reports (1, 1), the same
            // "unknowable yet" fallback already used for depth/has_children.
            let (position_in_set, size_of_set) = if loading {
                (1, 1)
            } else {
                self.source.sibling_pos(flat_idx)
            };
            let tree_row_id = ctx.add(TreeRowA11y::new(
                row_inner_id,
                flat_idx + 2,
                depth + 1,
                if has_children {
                    Some(is_expanded)
                } else {
                    None
                },
                row_selected,
                position_in_set,
                size_of_set,
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
                let click_anchor = row_anchor.clone();
                let focused_for_click = self.focused_cell.clone();
                // Deferred collapse: pressing an ALREADY-selected row (no
                // modifiers) keeps the whole (multi-)selection so it can be
                // dragged; the collapse-to-single happens on release WITHOUT
                // a drag (mirrors `ListView`).
                let pending_collapse = Rc::new(Cell::new(false));
                row_handlers = row_handlers.on_pointer_event(move |event, ctx| match event {
                    WidgetEvent::PointerDown {
                        button: PointerButton::Primary,
                        modifiers,
                        ..
                    } => {
                        // The press belongs to an interactive child (an
                        // embedded checkbox, twist arrow, …) — let it handle
                        // the tap; don't also select the row. Clear any stale
                        // deferred-collapse (left by a prior drag whose
                        // PointerUp the drag machinery consumed) so it can't
                        // fire on this unrelated interaction.
                        if ctx.press_claimed_by_interactive_child() {
                            pending_collapse.set(false);
                            return EventResponse::Ignored;
                        }
                        // Move the keyboard-navigation cursor to the clicked row so a
                        // subsequent Arrow steps from here — the row-nav origin is
                        // `focused_cell.get().unwrap_or((0,0))`, which nothing else
                        // writes on a click. Keep the existing column; the ring stays
                        // hidden (gated on the pointer/keyboard `focus_visible`
                        // modality).
                        // Resolve the row's CURRENT position once: rows above
                        // may have appeared or vanished since this handler was
                        // built, and a gone row must not hand its click to
                        // whoever took its slot.
                        let Some(row_index_for_click) = click_anchor.index() else {
                            return EventResponse::Ignored;
                        };
                        let col = focused_for_click.get().map(|(_, c)| c).unwrap_or(0);
                        focused_for_click.set(Some((row_index_for_click, col)));
                        if modifiers.command() && sel_for_click.mode() == SelectionMode::Multi {
                            sel_for_click.toggle(row_index_for_click);
                            pending_collapse.set(false);
                        } else if modifiers.shift() && sel_for_click.mode() == SelectionMode::Multi
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
                        EventResponse::Ignored
                    }
                    WidgetEvent::PointerUp {
                        button: PointerButton::Primary,
                        ..
                    } => {
                        // A release on an interactive child is that child's
                        // own tap — never collapse the row from it (guards
                        // against a `pending_collapse` a prior drag left
                        // stuck true).
                        if ctx.press_claimed_by_interactive_child() {
                            return EventResponse::Ignored;
                        }
                        // Reached only on a click WITHOUT a drag (an active
                        // drag consumes PointerUp). Collapse the deferred
                        // multi-selection to the clicked row.
                        if pending_collapse.replace(false)
                            && let Some(row) = click_anchor.index()
                        {
                            sel_for_click.select(row);
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                });
            }
            // When reorderable OR exportable, attach an on_drag handler to
            // start the drag. Selection-aware: the whole selection when the
            // pressed row is part of a multi-selection, else just the
            // pressed row. Export clones/MIME are built only when the view
            // opted in via `.exportable(..)` / `.export_external(..)`.
            let is_drag_source = self.export.is_drag_source(self.reorderable);
            // Per-row drag eligibility is the SOURCE's call (a locked/trashed row
            // may refuse to move), mirroring `TreeView`. Without this a source's
            // `set_drag_policy` would be silently overridden by the view.
            let drag_gate = self.source.dnd.drag_fn.clone();
            if is_drag_source && drag_gate(flat_idx) == DragEligibility::CanDrag {
                let drag_model_id = self.model_id;
                let anchor = self.drag_anchor;
                let preview_flat = flat_idx;
                let source_for_preview = source.clone();
                let columns_for_preview = columns.clone();
                let display_for_preview = display_indices.clone();
                let widths_for_preview = self.column_widths.clone();
                let metrics_for_preview = self.row_metrics.clone();
                let tree_pos_for_preview = tree_display_pos;
                let sel_for_drag = selection.clone();
                let export_for_drag = self.export.clone();
                let source_for_drag = source.clone();
                row_handlers = row_handlers.on_drag(move |phase, ctx| {
                    if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                        // Selection-aware dragged set: the whole selection
                        // when the pressed row is part of a
                        // multi-selection, else just the pressed row.
                        let rows: Vec<usize> = match sel_for_drag.as_ref() {
                            Some(s) if s.is_selected(preview_flat) => {
                                let mut v = s.selected_indices();
                                v.sort_unstable();
                                if v.len() <= 1 { vec![preview_flat] } else { v }
                            }
                            _ => vec![preview_flat],
                        };

                        // Reader: pulls the item at a flat visible index
                        // through the projection (skips a row that isn't
                        // currently resident — the shared `build_payload`
                        // drops it so `rows`/`items` stay index-aligned).
                        let src_r = source_for_drag.clone();
                        let read =
                            move |i: usize, f: &mut dyn FnMut(&T)| (src_r.read_item_fn)(i, f);

                        // Snapshot-out: resolve the dragged flat indices to
                        // stable `NodeId`s NOW, at drag-start, so the
                        // default move-out removal stays correct even if
                        // the tree reshuffles before the drag ends.
                        // Re-checks existence right before each removal, so
                        // a descendant already removed by its ancestor's
                        // subtree removal (`nodes` is in ascending
                        // pre-order — an ancestor always precedes its
                        // descendants) is safely skipped instead of the
                        // stale-key panic `TreeModel::remove` would raise.
                        let snapshot_out = source_for_drag.dnd.snapshot_out_fn.clone();

                        let Some(payload) = export_for_drag.build_payload(
                            drag_model_id,
                            rows,
                            &read,
                            &snapshot_out,
                        ) else {
                            return;
                        };

                        // Flat multi-cell preview of the PRESSED row (indent
                        // is dropped in the floating preview — it reads as
                        // the row's content picked up, even when a
                        // multi-row selection is being dragged).
                        let widths = widths_for_preview.borrow().clone();
                        let h = metrics_for_preview.borrow_mut().row_height(preview_flat);
                        let total_w = widths.iter().sum::<f32>().max(120.0);
                        let mut cells: Vec<Box<dyn Widget>> = Vec::new();
                        // Both halves must come from the same row: if the meta
                        // is missing the row is not really resident, so build no
                        // preview rather than one at a fabricated depth 0.
                        let preview_meta = source_for_preview.meta(preview_flat);
                        (source_for_preview.read_item_fn)(preview_flat, &mut |item| {
                            let Some(e) = preview_meta else {
                                return;
                            };
                            cells = {
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
                            };
                        });
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
            // Row activation (open/commit) — a gesture, so it arbitrates
            // against the reorder drag via the gesture arena (a click
            // activates, a drag does not). `SingleClick` → `on_tap`,
            // `DoubleClick` → `on_double_tap`; Enter/Space activates too.
            if let Some(ref cb) = self.on_row_activate {
                let cb = cb.clone();
                // Anchored: a row that moved (or vanished) between build and
                // click must not activate whichever row took its slot.
                let activate_anchor = row_anchor.clone();
                let handlers = match self.activate_on {
                    crate::data_views::ActivateOn::SingleClick => {
                        let a = activate_anchor;
                        HandlerSet::new().on_tap(move |_tap, ctx| {
                            if let Some(i) = a.index() {
                                cb(i, ctx);
                            }
                        })
                    }
                    crate::data_views::ActivateOn::DoubleClick => {
                        let a = activate_anchor;
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
                            if let Some(i) = a.index() {
                                cb(i, ctx);
                            }
                        })
                    }
                };
                ctx.apply_handlers(tree_row_id, handlers);
            }
            ctx.apply_handlers(tree_row_id, row_handlers);
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
        // width (the row reports its tallest cell, height-for-width),
        // feed the heights back, and apply the scroll-anchor delta so
        // content above the viewport stays put. Measurements are
        // collected with NO metrics borrow held.
        if self.row_metrics.borrow().needs_measure() {
            let count = self.source.visible_count();
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
        builder.set_role(teksilo_core::accesskit::Role::RowGroup);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.row_entries.iter().map(|(_, id)| *id).collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
