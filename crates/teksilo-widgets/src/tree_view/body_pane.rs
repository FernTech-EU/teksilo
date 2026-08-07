// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TreeViewBodyPane<T>` — the virtualized row pane underneath `TreeView`.
//!
//! Like `ListView`'s `ListBodyPane` (and the table / grid panes before both),
//! this is a **sibling** of the scrollbar rather than its ancestor, so
//! rebuilds triggered by scroll-buffer exits, source-version bumps
//! (expand/collapse included) or selection changes don't tear the scrollbar
//! down mid-thumb-drag.
//!
//! The framework's rebuild deferral in
//! `WidgetTree::process_pending_rebuilds` skips any rebuild targeting an
//! *ancestor* of the pointer-captured widget, and a thumb drag holds that
//! capture on the scrollbar for the whole Down→Up sequence. With row
//! realization rooted on `TreeView` itself — an ancestor of its own scrollbar
//! — dragging the thumb past the 5-row buffer left the tree blank until the
//! user released it. `TreeView` now owns exactly two children, this pane and
//! the scrollbar, and only the pane rebuilds.
//!
//! `TreeViewBodyPane` is `pub(crate)` — applications still talk to `TreeView`.

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
use teksilo_data::{DragEligibility, RowState};

use super::{BUFFER_ITEMS, RowDelegate};
use crate::common::row_metrics::SharedRowMetrics;
use crate::data_views::{RowSelection, ViewId};
use crate::tree_source::TreeSource;

/// The row-virtualization pane. Owns the realized row widgets and their
/// per-row selection / expand / activation / drag handlers. Sized to fill the
/// rect the root allocates it; lays each row at `row_top(index) - scroll_y`
/// in pane-local coordinates.
pub(crate) struct TreeViewBodyPane<T: 'static> {
    /// The erased, index-keyed tree source — the same handle the root holds.
    pub(crate) source: Rc<TreeSource<T>>,
    pub(crate) row_delegate: Rc<RowDelegate<T>>,
    pub(crate) row_tooltips: crate::data_views::RowTooltips<T>,

    /// Row geometry shared with the `TreeView` root (one handle, two holders
    /// — the root drives scrollbar totals, paint and keyboard, the pane
    /// drives realization, placement and measurement).
    pub(crate) metrics: SharedRowMetrics,
    pub(crate) row_selection: Option<RowSelection>,
    /// Keyboard cursor + the identity it resolves through, shared with the
    /// root's key handler: the per-row pointer handler moves both, so arrows
    /// step from the clicked row and keep following it across reflattens.
    pub(crate) focused_index: Rc<Cell<Option<usize>>>,
    pub(crate) focused_anchor: Rc<RefCell<Option<crate::data_views::RowAnchor>>>,

    pub(crate) reorderable: bool,
    /// Whether a row-body release on a branch row auto-toggles its expansion.
    pub(crate) row_click_expands: bool,
    /// Cross-widget export / foreign-receive machinery, cloned in from the
    /// owning `TreeView` — builds the drag-start payload here; the
    /// self-reorder flag and removal-thunk stash are `Rc`-backed, so mutations
    /// made through this clone are visible to the root's `on_drag_ended`
    /// completion (installed on the root's own clone).
    pub(crate) export: crate::data_views::RowExport<T>,

    pub(crate) on_activate: Option<Rc<dyn Fn(usize, &mut teksilo_core::widget::EventContext)>>,
    pub(crate) activate_on: crate::data_views::ActivateOn,
    /// Stable, kind-tagged id of the owning `TreeView` instance — stamped into
    /// the `RowDragData` payload so the source can tell a same-view reorder
    /// from a foreign drop.
    pub(crate) tree_id: ViewId,
    /// The owning `TreeView`'s arena id: the drag source `start_drag` is given
    /// (the root is where `install_completion` put the move-out handler), and
    /// the key for the row focus scope, since keyboard focus lands on the root
    /// and a `StandardTreeItem`'s focus-aware styling must track *its* focus.
    pub(crate) root_id: WidgetId,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,

    /// Pane-local rebuild trigger — see `ListBodyPane::version`.
    pub(crate) version: Signal<u64>,
    /// Bound at `Relayout` on the `TreeView` ROOT: the root computes scrollbar
    /// totals before this pane measures (parent-before-child ordering), so a
    /// measurement that moves the content total pokes this to re-place the
    /// root next frame with the corrected value.
    pub(crate) total_refresh: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    pub(crate) prev_built_start: Rc<Cell<usize>>,
    pub(crate) prev_built_end: Rc<Cell<usize>>,

    // Build state
    pub(crate) item_entries: Vec<(usize, WidgetId)>,
}

impl<T: 'static> TreeViewBodyPane<T> {
    fn visible_range(&self) -> (usize, usize) {
        self.metrics.borrow_mut().visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.visible_count(),
            BUFFER_ITEMS,
        )
    }
}

impl<T: 'static> std::fmt::Debug for TreeViewBodyPane<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeViewBodyPane")
            .field("rows", &self.item_entries.len())
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for TreeViewBodyPane<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Self-rebuild trigger. A persistent field (not `ctx.signal`) so the
        // realization re-check in `place_children` can bump it after
        // measurement, and so the root's source-version observer can drive it
        // without rebuilding itself.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll position re-places rows without rebuilding (within buffer).
        // Deliberately NOT `register_animated_signal`: the root owns that
        // registration, and the scheduler cancels an animation when the widget
        // that last registered its signal rebuilds — registering here would
        // make every buffer-exit rebuild abort an in-flight smooth-scroll.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Buffer-exit detection → rebuild THIS pane. Because the pane is a
        // sibling of the scrollbar rather than its ancestor, the rebuild
        // deferral does not skip this one during a thumb drag — see the
        // module docs.
        let (initial_start, initial_end) = self.visible_range();
        self.prev_built_start.set(initial_start);
        self.prev_built_end.set(initial_end);
        let v_for_scroll = version.clone();
        let vp_h = self.viewport_height.clone();
        let scroll_handle = self.scroll_y.observe({
            let pbs = self.prev_built_start.clone();
            let pbe = self.prev_built_end.clone();
            let metrics = self.metrics.clone();
            let source = self.source.clone();
            move |y| {
                let count = source.visible_count();
                let (visible_start, visible_end) =
                    metrics.borrow_mut().visible_range(*y, vp_h.get(), count, 0);
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ITEMS);
                    // Clamp to `count` — build realizes a `min(end, count)`
                    // window, so an unclamped `pbe` past the end leaves the
                    // dirty-check believing rows were built that never were,
                    // and the bottom rows of a large tree never realize on a
                    // fast scroll.
                    let new_end = (visible_end + BUFFER_ITEMS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    v_for_scroll.set(v_for_scroll.get() + 1);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // Selection changes refresh the `selected` argument handed to the row
        // delegate, so they rebuild the pane. (The root separately repaints
        // its container focus ring — see `TreeView::build`.)
        if let Some(ref rs) = self.row_selection {
            let v = version.clone();
            let counter = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                counter.set(counter.get() + 1);
                v.set(counter.get());
            });
            ctx.own_handle(handle);
        }

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        // Lazy: nudge the source to load the realized window, and fetch more
        // as the viewport nears the end (append-only sources). This lives in
        // the pane, not the root, because the pane decides the realization
        // window — and it is the only one of the two that still rebuilds
        // while the thumb is captured.
        (self.source.dnd.request_window_fn)(start..end);
        if (self.source.dnd.can_fetch_more_fn)()
            && end + BUFFER_ITEMS >= self.source.visible_count()
        {
            (self.source.dnd.fetch_more_fn)();
        }
        let is_drag_source = self.export.is_drag_source(self.reorderable);
        let tree_id = self.tree_id;
        let root_id = self.root_id;
        let row_state_fn = self.source.dnd.row_state_fn.clone();
        // Key the row focus scope on the TreeView root (the focusable node),
        // not this pane, so rows' `StandardTreeItem`s read *its* keyboard
        // focus deterministically.
        ctx.begin_view_focus_for(root_id);
        for i in start..end {
            let selected = self
                .row_selection
                .as_ref()
                .map(|s| s.is_selected(i))
                .unwrap_or(false);
            // Row metadata (a11y level / expand state) from the source.
            let meta = self.source.meta(i);
            let item_has_children = meta.as_ref().is_some_and(|m| m.has_children);
            // A `Loading` row (data not yet resident) renders a placeholder
            // skeleton instead of being skipped, so the scrollbar and layout
            // stay stable while the window loads. A placeholder reports no
            // metadata, so the expand/drag wiring below is gated off.
            // Resolve the row's tooltip inside the same borrow that builds the
            // row: it is the only place the item is reachable, and attaching
            // needs a `WidgetId` that does not exist until afterwards.
            let pending_tip: RefCell<Option<crate::data_views::ResolvedRowTooltip>> =
                RefCell::new(None);
            let tips = &self.row_tooltips;
            let row_widget = self
                .source
                .with_row(i, &|item, m| {
                    if tips.is_set() {
                        *pending_tip.borrow_mut() = tips.resolve(i, item);
                    }
                    (self.row_delegate)(i, item, m, selected)
                })
                .or_else(|| {
                    ((row_state_fn)(i) == RowState::Loading)
                        .then(crate::data_views::default_placeholder)
                });
            if let Some(widget) = row_widget {
                let inner_id = ctx.add_boxed(widget);
                // The app cannot reach this widget to hang a tooltip on it —
                // the view built it — so the view attaches the resolved tip.
                if let Some(tip) = pending_tip.into_inner() {
                    self.row_tooltips.attach_resolved(ctx, inner_id, tip);
                }
                let (level, position_1based, total_siblings, expanded_opt) =
                    if let Some(ref m) = meta {
                        let exp = if m.has_children {
                            Some(m.is_expanded)
                        } else {
                            None
                        };
                        let (pos, total) = self.source.sibling_pos(i);
                        (m.depth + 1, pos, total, exp)
                    } else {
                        (1, 1, 1, None)
                    };
                let child_id = ctx.add(crate::list_item_a11y::TreeItemWrapper::new(
                    inner_id,
                    level,
                    position_1based,
                    total_siblings,
                    expanded_opt,
                    selected,
                ));

                // Click handling: selection + expand/collapse for branch rows.
                {
                    let sel_click = self.row_selection.clone();
                    let click_index = i;
                    let source_click = self.source.clone();
                    let click_anchor = self.source.anchor(i);
                    let fi_click = self.focused_index.clone();
                    let fi_anchor_click = self.focused_anchor.clone();
                    let has_children = item_has_children && self.row_click_expands;
                    // Deferred collapse: pressing an already-selected row keeps
                    // the whole (multi-)selection so it can be dragged; the
                    // collapse-to-single happens on release WITHOUT a drag.
                    let pending_collapse = Rc::new(Cell::new(false));

                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_pointer_event(move |event, ctx| match event {
                            teksilo_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: teksilo_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // The press belongs to an interactive child (the
                                // chevron, or an inline control) — toggling/acting
                                // is its job; don't also select the row. Clear any
                                // stale deferred-collapse (left by a prior drag
                                // whose PointerUp the drag machinery consumed) so
                                // it can't fire on this unrelated interaction. (This
                                // guards the no-selection-model branch below — the
                                // shared helper does the equivalent for its own
                                // branch.)
                                if ctx.press_claimed_by_interactive_child() {
                                    pending_collapse.set(false);
                                    return teksilo_core::event::EventResponse::Ignored;
                                }
                                // The shared deferred-select helper owns the
                                // press-claimed guard, Ctrl/Shift handling, and
                                // the defer-collapse-on-already-selected rule; it
                                // returns false (skip the nav-cursor move) when an
                                // interactive child claimed the press. Without a
                                // selection model there's nothing to defer — a
                                // plain click still moves the nav cursor.
                                let moved = match sel_click.as_ref() {
                                    Some(sel) => crate::data_views::deferred_select::on_down(
                                        sel,
                                        click_index,
                                        *modifiers,
                                        &pending_collapse,
                                        ctx,
                                    ),
                                    None => true,
                                };
                                if moved {
                                    // Move the keyboard-navigation cursor to the
                                    // clicked row so a subsequent Arrow keypress
                                    // steps from here — `focused_index` is the
                                    // arrow-nav origin and is otherwise only
                                    // written by the keyboard handler. Refresh
                                    // the anchor alongside it — see `set_focus`
                                    // in the keyboard handler.
                                    fi_click.set(Some(click_index));
                                    *fi_anchor_click.borrow_mut() =
                                        Some(source_click.anchor(click_index));
                                }
                                // Ignored lets the gesture arena also see the
                                // PointerDown so DragRecognizer can capture the
                                // press position and enable drag-to-reorder.
                                teksilo_core::event::EventResponse::Ignored
                            }
                            teksilo_core::event::WidgetEvent::PointerUp {
                                button: teksilo_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // A release on the chevron (or another interactive
                                // child) is handled by that child's own tap — don't
                                // also toggle from the row body.
                                if ctx.press_claimed_by_interactive_child() {
                                    return teksilo_core::event::EventResponse::Ignored;
                                }
                                // Reached only on a click WITHOUT a drag (an
                                // active drag consumes PointerUp). Collapse the
                                // deferred multi-selection to the clicked row.
                                if let Some(ref sel) = sel_click {
                                    crate::data_views::deferred_select::on_up(
                                        sel,
                                        click_index,
                                        &pending_collapse,
                                        ctx,
                                    );
                                }
                                // Expand/collapse fires on release so a drag
                                // gesture pre-empts it (once active_drag is
                                // set, PointerUp is routed to handle_drag_drop
                                // and never reaches this widget).
                                // Anchored: rows above may have shifted since
                                // this handler was built, so resolve the row's
                                // current position rather than trusting the
                                // captured index.
                                if has_children && let Some(cur) = click_anchor.index() {
                                    source_click.toggle_at(cur);
                                }
                                teksilo_core::event::EventResponse::Ignored
                            }
                            _ => teksilo_core::event::EventResponse::Ignored,
                        }),
                    );

                    // Row activation (open/commit) — a gesture, so it arbitrates
                    // against the reorder drag via the gesture arena (a click
                    // activates, a drag does not). `SingleClick` → `on_tap`,
                    // `DoubleClick` → `on_double_tap`; Enter/Space activates too
                    // (keyboard handler). Distinct from selection, which also
                    // moves on arrow navigation.
                    if let Some(ref cb) = self.on_activate {
                        let cb = cb.clone();
                        let a = self.source.anchor(i);
                        let handlers = match self.activate_on {
                            crate::data_views::ActivateOn::SingleClick => {
                                HandlerSet::new().on_tap(move |tap, ctx| {
                                    // A Ctrl/Shift click is a selection-extension
                                    // gesture (applied on PointerDown), not an
                                    // activation — suppress open/commit so a
                                    // multi-select click doesn't also fire the
                                    // activate callback. Mirrors the PointerDown
                                    // selection condition (`ctrl` toggles, `shift`
                                    // extends) so the two stay in lock-step.
                                    if tap.modifiers.ctrl() || tap.modifiers.shift() {
                                        return;
                                    }
                                    if let Some(cur) = a.index() {
                                        cb(cur, ctx)
                                    }
                                })
                            }
                            crate::data_views::ActivateOn::DoubleClick => HandlerSet::new()
                                .on_double_tap(move |_tap, ctx| {
                                    if let Some(cur) = a.index() {
                                        cb(cur, ctx)
                                    }
                                }),
                        };
                        ctx.apply_handlers(child_id, handlers);
                    }
                }

                // AT-action driving. `TreeItemWrapper::accessibility`
                // advertises `Click`, `ScrollIntoView` and — on a branch —
                // `Expand` / `Collapse`; this is where they land. A screen
                // reader's activate gesture and the automation bridge's
                // `invoke_action` / `expand` / `collapse` both arrive as
                // `WidgetEvent::AccessAction`.
                //
                // Not optional garnish: without it the only way to drive a row
                // is a synthetic pointer click at its reported bounds, and for
                // any row the virtualizer has parked outside the viewport those
                // bounds are *content* coordinates that can sit below the
                // window entirely — the click goes nowhere, or somewhere else.
                {
                    let sel_a11y = self.row_selection.clone();
                    let source_a11y = self.source.clone();
                    let anchor_a11y = self.source.anchor(i);
                    let fi_a11y = self.focused_index.clone();
                    let fi_anchor_a11y = self.focused_anchor.clone();
                    let metrics_a11y = self.metrics.clone();
                    let scroll_a11y = self.scroll_y.clone();
                    let vh_a11y = self.viewport_height.clone();
                    let activate_a11y = self.on_activate.clone();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_access_action(move |action, ctx| {
                            use teksilo_core::accesskit::Action;
                            use teksilo_core::event::EventResponse;
                            // Rows above may have shifted since this handler was
                            // built (an expand elsewhere, a model edit), so
                            // resolve the row's CURRENT flat index rather than
                            // trusting the captured one — same rule as the
                            // pointer path above.
                            let Some(row) = anchor_a11y.index() else {
                                return EventResponse::Ignored;
                            };
                            match action {
                                Action::Click => {
                                    // `Click` is the node's DEFAULT action and
                                    // an AT client has no double-click, so it
                                    // both selects and activates regardless of
                                    // what `activate_on` asks of a mouse.
                                    if let Some(ref sel) = sel_a11y {
                                        sel.select(row);
                                    }
                                    // Move the arrow-nav origin with it, exactly
                                    // as a pointer click does, so a following
                                    // ArrowDown steps from here.
                                    fi_a11y.set(Some(row));
                                    *fi_anchor_a11y.borrow_mut() = Some(source_a11y.anchor(row));
                                    if let Some(ref cb) = activate_a11y {
                                        cb(row, ctx);
                                    }
                                    EventResponse::Handled
                                }
                                Action::Expand | Action::Collapse => match source_a11y.meta(row) {
                                    // Idempotent: asking a branch for the state
                                    // it is already in succeeds. Asking a *leaf*
                                    // does not — that is a caller error, and
                                    // reporting it is the whole point of having
                                    // the reply distinguish the two.
                                    Some(m) if m.has_children => {
                                        let want = action == Action::Expand;
                                        if m.is_expanded != want {
                                            source_a11y.set_expanded_at(row, want);
                                        }
                                        EventResponse::Handled
                                    }
                                    _ => EventResponse::Ignored,
                                },
                                Action::ScrollIntoView => {
                                    let viewport = vh_a11y.get();
                                    let scroll = scroll_a11y.get();
                                    let new_scroll = {
                                        let mut m = metrics_a11y.borrow_mut();
                                        let total = m.total_height(source_a11y.visible_count());
                                        let max = (total - viewport).max(0.0);
                                        m.scroll_for_ensure_visible(row, scroll, viewport, max)
                                    };
                                    if (new_scroll - scroll).abs() > f32::EPSILON {
                                        scroll_a11y.set(new_scroll);
                                    }
                                    EventResponse::Handled
                                }
                                _ => EventResponse::Ignored,
                            }
                        }),
                    );
                }

                // Drag handler when reorderable OR exportable, gated by the
                // source's transferable verdict (`drag`). Emits the public
                // `RowDragData<T>`; the source recovers the key + validates at
                // hover/drop. The floating preview re-invokes the row delegate.
                if is_drag_source && (self.source.dnd.drag_fn)(i) == DragEligibility::CanDrag {
                    let drag_view_id = tree_id;
                    let drag_self_id = root_id;
                    let row_delegate = self.row_delegate.clone();
                    let source_for_preview = self.source.clone();
                    let flat_idx = i;
                    let metrics_for_preview = self.metrics.clone();
                    // Export capture: the dragged set is selection-aware; the
                    // shared `RowExport` builds the payload (clones / MIME /
                    // Loading-filter / stash) when the view opted in.
                    let sel_for_drag = self.row_selection.clone();
                    let export_for_drag = self.export.clone();
                    let read_for_drag = self.source.read_item_fn.clone();
                    let snapshot_for_drag = self.source.dnd.snapshot_out_fn.clone();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_drag(move |phase, ctx| {
                            if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                                // Selection-aware dragged set: the whole
                                // selection when the pressed row is part of a
                                // multi-selection, else just the pressed row.
                                let rows: Vec<usize> = match sel_for_drag.as_ref() {
                                    Some(s) if s.is_selected(flat_idx) => {
                                        let mut v = s.selected_indices();
                                        v.sort_unstable();
                                        if v.len() <= 1 { vec![flat_idx] } else { v }
                                    }
                                    _ => vec![flat_idx],
                                };
                                let Some(payload) = export_for_drag.build_payload(
                                    drag_view_id,
                                    rows,
                                    &*read_for_drag,
                                    &snapshot_for_drag,
                                ) else {
                                    return;
                                };
                                const PREVIEW_WIDTH: f32 = 240.0;
                                let h = metrics_for_preview.borrow_mut().row_height(flat_idx);
                                let rd = row_delegate.clone();
                                let preview_opt =
                                    source_for_preview.with_row(flat_idx, &move |item, m| {
                                        Box::new(crate::drag_preview::DragPreview::new(
                                            PREVIEW_WIDTH,
                                            h,
                                            rd(flat_idx, item, m, false),
                                        ))
                                            as Box<dyn Widget>
                                    });
                                if let Some(preview) = preview_opt {
                                    ctx.start_drag_with_preview(drag_self_id, payload, preview);
                                } else {
                                    ctx.start_drag(drag_self_id, payload);
                                }
                            }
                        }),
                    );
                }

                self.item_entries.push((i, child_id));
            }
        }
        ctx.end_view_focus();

        self.item_entries.iter().map(|(_, id)| *id).collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Only an allocation may seed the cached viewport — see
        // `common::viewport` for what a measurement pass would otherwise do to
        // `build`'s realization window.
        crate::common::viewport::viewport_size(
            proposal,
            &self.viewport_height,
            Size::new(300.0, 200.0),
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
        // The allocated height is the authoritative viewport: `build` sizes
        // its realization window from this, and a stale value there costs a
        // permanent rebuild loop (`common::viewport`).
        crate::common::viewport::record_viewport_height(&self.viewport_height, bounds.height);

        let item_count = self.item_entries.len();
        if item_count == 0 {
            return;
        }
        let count = self.source.visible_count();

        // Auto-measure pass: measure every realized row at the pane width
        // (height-for-width), feed the heights back, and apply the
        // scroll-anchor delta so content above the viewport stays put.
        // Measurements are collected with NO metrics borrow held.
        if self.metrics.borrow().needs_measure() {
            let pre_total = self.metrics.borrow_mut().total_height(count);
            let mut measured = Vec::with_capacity(item_count);
            for (idx, child) in children.iter().enumerate() {
                if idx < item_count
                    && let Some(size) =
                        ctx.child_size(child.id, SizeProposal::with_width(bounds.width))
                {
                    let (flat_index, _) = self.item_entries[idx];
                    measured.push((flat_index, size.height));
                }
            }
            let anchor = self
                .metrics
                .borrow_mut()
                .observe_measured(&measured, self.scroll_y.get());
            if anchor.abs() > 0.01 {
                // Safe from place_children: the dirty flag is set but the
                // binding flush already ran this pass — lands next frame.
                self.scroll_y.set((self.scroll_y.get() + anchor).max(0.0));
            }

            // Realization re-check: corrected offsets may reveal viewport
            // rows the estimated offsets never realized. Request a pane
            // rebuild for next frame; the 0.01 measurement epsilon guarantees
            // convergence.
            let (vs, ve) = self.metrics.borrow_mut().visible_range(
                self.scroll_y.get(),
                self.viewport_height.get(),
                count,
                0,
            );
            if vs < self.prev_built_start.get() || ve > self.prev_built_end.get() {
                self.prev_built_start.set(vs.saturating_sub(BUFFER_ITEMS));
                self.prev_built_end.set((ve + BUFFER_ITEMS).min(count));
                self.version.set(self.version.get() + 1);
            }

            // Total-refresh poke: the root computed `max_scroll_y` / the thumb
            // ratio BEFORE this measure pass (parent-first ordering). If the
            // content total changed, re-place the root next frame so the
            // corrected total lands — otherwise content past the estimated
            // total stays unreachable forever. Terminates: a re-measure of
            // settled rows yields zero deltas (sub-pixel epsilon).
            let post_total = self.metrics.borrow_mut().total_height(count);
            if (post_total - pre_total).abs() > 0.01 {
                self.total_refresh.set(self.total_refresh.get() + 1);
            }
        }

        let scroll_y = self.scroll_y.get();
        for (idx, child) in children.iter_mut().enumerate() {
            if idx < item_count {
                let (flat_index, _) = self.item_entries[idx];
                let (top, height) = {
                    let mut m = self.metrics.borrow_mut();
                    (m.row_top(flat_index), m.row_height(flat_index))
                };
                let y = bounds.y + top - scroll_y;
                child.origin = Point::new(bounds.x, y);
                child.size = Size::new(bounds.width, height);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The pane stands in as the tree's `Role::Group` — the ARIA-blessed
        // intermediate between `Role::Tree` and `Role::TreeItem` (`group` is
        // exactly how a tree wraps a set of items). Without a non-hidden role
        // here, AT clients that walk Tree → TreeItem directly would balk at a
        // hidden generic container in the path.
        builder.set_role(teksilo_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.item_entries.iter().map(|(_, id)| *id).collect()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
