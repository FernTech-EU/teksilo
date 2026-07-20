// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `GridBodyPane<T>` — the virtualized tile pane.
//!
//! Like `TableView`'s `BodyPane`, this is a **sibling** of the scrollbar
//! rather than its ancestor, so rebuilds triggered by scroll-buffer exits
//! or column-count changes don't tear down the scrollbar mid-thumb-drag
//! (the framework's rebuild deferral only skips rebuilds targeting an
//! *ancestor* of the captured widget). `GridView` owns the pane, the
//! optional scrollbar, and the `GridOverlay` as three flat children.
//!
//! The pane realizes only the tiles in the strategy's visible range (plus
//! buffer), positions them at `tile_rect - scroll`, and — for
//! variable-height strategies — measures each realized tile and feeds the
//! heights back for scroll-anchoring (see [`GridLayoutStrategy::observe_measured`]).

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
use bastyde_data::{DragEligibility, RowState, SelectionModel};

use super::TileContext;
use super::a11y::TileA11y;
use super::layout::GridLayoutStrategy;
use crate::data_views::{RowSelection, ViewId, default_placeholder};

pub(crate) type LenFn = Rc<dyn Fn() -> usize>;
pub(crate) type WithItemFn<T> =
    Rc<dyn Fn(usize, &dyn Fn(&T) -> Box<dyn Widget>) -> Option<Box<dyn Widget>>>;
/// Read `&T` from the resident row at `index` via a side-effecting callback,
/// returning whether it ran (row present + loaded). Powers export
/// item-cloning (`.exportable(..)`) without the delegate's widget-building
/// path.
pub(crate) type ReadItemFn<T> = Rc<dyn Fn(usize, &mut dyn FnMut(&T)) -> bool>;
pub(crate) type TileDelegate<T> = Rc<dyn Fn(&TileContext<'_, T>) -> Box<dyn Widget>>;
/// Per-tile transferable gate (source-owned): may this tile begin a drag?
pub(crate) type DragFn = Rc<dyn Fn(usize) -> DragEligibility>;
/// Per-tile residency state (source-owned): is this tile loaded, or a
/// windowed placeholder?
pub(crate) type RowStateFn = Rc<dyn Fn(usize) -> RowState>;
/// Nudge the source to load a visible range / append the next page.
pub(crate) type RequestWindowFn = Rc<dyn Fn(std::ops::Range<usize>)>;
pub(crate) type CanFetchMoreFn = Rc<dyn Fn() -> bool>;
pub(crate) type FetchMoreFn = Rc<dyn Fn()>;

/// How close (in tiles) the realized window's end must come to the total
/// before an append-only source is asked to `fetch_more`.
const FETCH_BUFFER_TILES: usize = 24;

pub(crate) struct GridBodyPane<T: 'static> {
    pub(crate) len_fn: LenFn,
    pub(crate) with_item_fn: WithItemFn<T>,
    pub(crate) delegate: TileDelegate<T>,

    pub(crate) strategy: Rc<dyn GridLayoutStrategy>,
    pub(crate) viewport_width: Rc<Cell<f32>>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    /// The pane's absolute (window) origin, published each `place_children`
    /// pass so `GridView`'s keyboard handler can chase the focused tile into
    /// enclosing scroll areas. Shares the `GridView`'s `viewport_origin` cell
    /// (`None` until this pane lays out at least once).
    pub(crate) viewport_origin: Rc<Cell<Option<bastyde_canvas::Point>>>,
    /// Live column count, written by `GridView::place_children`. Drives a
    /// rebuild when the window resize changes how many columns fit.
    pub(crate) column_count: Signal<usize>,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) selection: Option<SelectionModel>,
    /// Read-only snapshot for `TileContext::is_focused`. The pane does NOT
    /// rebuild on focus change — the focus ring is painted by `GridOverlay`.
    pub(crate) focused_index: Signal<Option<usize>>,

    // Per-tile interaction callbacks (Phase 3).
    #[allow(clippy::type_complexity)]
    pub(crate) on_tile_activate: Option<Rc<dyn Fn(usize, &mut bastyde_core::widget::EventContext)>>,
    pub(crate) activate_on: crate::data_views::ActivateOn,
    #[allow(clippy::type_complexity)]
    pub(crate) tile_context_menu: Option<
        Rc<
            dyn Fn(
                usize,
                bastyde_canvas::Point,
                &mut bastyde_core::widget::EventContext,
            ) -> Option<Box<dyn Widget>>,
        >,
    >,
    /// Per-tile accessible name for the `GridCell` (see `GridView::tile_a11y_label`).
    #[allow(clippy::type_complexity)]
    pub(crate) tile_a11y_label: Option<Rc<dyn Fn(usize) -> String>>,
    pub(crate) reorderable: bool,
    pub(crate) model_id: ViewId,
    /// Cross-widget export / foreign-receive machinery, shared with
    /// `GridView` (the single source of truth for the setting) — the
    /// drag-start payload build and the move-out completion.
    pub(crate) export: crate::data_views::RowExport<T>,
    /// Read `&T` from the resident row at `index` (source-owned), used to
    /// clone dragged items for export.
    pub(crate) read_item_fn: ReadItemFn<T>,
    /// Stable-key removal thunk resolver for the default move-out
    /// (source-owned), invoked at drag-start via `RowExport::build_payload`.
    pub(crate) snapshot_out_fn: crate::data_views::SnapshotOutFn,
    /// The GridView root's focusable `WidgetId`. The focus scope this pane
    /// opens for its tiles is keyed on the root (where keyboard focus lands),
    /// not on the pane itself (a non-focusable child), so a `StandardItem`
    /// tile's focus-aware selection tracks the grid's real focus.
    pub(crate) scope_owner: WidgetId,
    /// Source-owned DnD + lazy capability closures (erased from the backing
    /// `ListDataSource`). `drag_fn` gates per-tile drag start; `row_state_fn`
    /// drives windowed placeholders; the lazy trio nudges the source to load
    /// the realized window / fetch the next page as the viewport advances.
    pub(crate) drag_fn: DragFn,
    pub(crate) row_state_fn: RowStateFn,
    pub(crate) request_window_fn: RequestWindowFn,
    pub(crate) can_fetch_more_fn: CanFetchMoreFn,
    pub(crate) fetch_more_fn: FetchMoreFn,
    /// Shared (flat index → tile wrapper id) map, written at the end of
    /// each build for the container's `active_descendant` roving focus.
    pub(crate) tile_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,

    // Section headers (Phase 4). When set, the pane realizes a header widget
    // per visible section alongside the tiles.
    #[allow(clippy::type_complexity)]
    pub(crate) header_factory: Option<Rc<dyn Fn(usize) -> Box<dyn Widget>>>,
    #[allow(clippy::type_complexity)]
    pub(crate) header_title: Option<Rc<dyn Fn(usize) -> String>>,

    /// Pane-local rebuild trigger. A persistent field (re-bound each
    /// build) so `place_children`'s post-measure realization re-check
    /// can request a rebuild of this pane.
    pub(crate) version: Signal<u64>,
    /// Bound at `Relayout` on the `GridView` ROOT — bumped when a
    /// measure pass changes the content total so the root re-places
    /// with the corrected `max_scroll_y` / thumb ratio next frame (the
    /// root computes them before this pane measures; without the poke
    /// they'd stay stale until the next scroll).
    pub(crate) total_refresh: Signal<u64>,
    /// Realized tile range from the latest build — consulted by both
    /// the scroll observer and the realization re-check.
    pub(crate) prev_built_start: Rc<Cell<usize>>,
    pub(crate) prev_built_end: Rc<Cell<usize>>,

    // Build state
    pub(crate) tile_entries: Vec<(usize, WidgetId)>,
    pub(crate) header_entries: Vec<(usize, WidgetId)>,

    /// Re-entrancy guard for `place_children`'s Relayout-bound signal
    /// writes (`scroll_y`, `version`, `total_refresh`) — see the comment
    /// at their call site. Not `Rc`-shared: purely internal bookkeeping
    /// for this one pane instance.
    pub(crate) in_place_children: Cell<bool>,
}

impl<T: 'static> GridBodyPane<T> {
    fn visible(&self) -> (usize, usize) {
        let count = (self.len_fn)();
        let vr = self.strategy.visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.viewport_width.get(),
            count,
        );
        (vr.start, vr.end)
    }
}

impl<T: 'static> std::fmt::Debug for GridBodyPane<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridBodyPane")
            .field("items", &(self.len_fn)())
            .field("realized", &self.tile_entries.len())
            .finish()
    }
}

impl<T: 'static> Widget for GridBodyPane<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Self-rebuild trigger. A persistent field (not `ctx.signal`)
        // so the realization re-check in `place_children` can bump it
        // after measurement.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll re-places tiles without rebuilding (within buffer).
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Buffer-exit detection → rebuild THIS pane (sibling of scrollbar).
        let strategy = self.strategy.clone();
        let len = self.len_fn.clone();
        let vp_h = self.viewport_height.clone();
        let vp_w = self.viewport_width.clone();
        let (initial_start, initial_end) = self.visible();
        self.prev_built_start.set(initial_start);
        self.prev_built_end.set(initial_end);
        let v_scroll = version.clone();
        let scroll_handle = self.scroll_y.observe({
            let ps = self.prev_built_start.clone();
            let pe = self.prev_built_end.clone();
            move |y| {
                let vr = strategy.visible_range(*y, vp_h.get(), vp_w.get(), (len)());
                if vr.start < ps.get() || vr.end > pe.get() {
                    ps.set(vr.start);
                    pe.set(vr.end);
                    v_scroll.set(v_scroll.get() + 1);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // Column-count change (window resize reflow) → rebuild.
        {
            let v = version.clone();
            let counter = Rc::new(Cell::new(0_u64));
            ctx.effect(&self.column_count, move |_| {
                counter.set(counter.get() + 1);
                v.set(counter.get());
            });
        }

        // Selection change → re-render visible tiles (refresh `is_selected`).
        if let Some(ref sel) = self.selection {
            let v = version.clone();
            let counter = Rc::new(Cell::new(0_u64));
            ctx.effect(&sel.selection_signal(), move |_| {
                counter.set(counter.get() + 1);
                v.set(counter.get());
            });
        }

        // Export completion (move-out): fires on the drag source — THIS
        // pane's own id, the stable id `start_drag` is anchored on below. A
        // same-view reorder is applied by `GridView`'s root `on_drop`, which
        // calls `note_self_reorder`, so it is skipped here (already applied).
        ctx.apply_self_handlers(self.export.install_completion(HandlerSet::new()));

        // Realize the visible tiles.
        self.tile_entries.clear();
        let total = (self.len_fn)();
        let (start, end) = self.visible();
        // Lazy: nudge the source to load the realized window, and fetch more
        // as the viewport nears the end (append-only sources). Fires on every
        // pane build — i.e. on each scroll-buffer exit — matching ListView.
        (self.request_window_fn)(start..end);
        if (self.can_fetch_more_fn)() && end + FETCH_BUFFER_TILES >= total {
            (self.fetch_more_fn)();
        }
        let focused = self.focused_index.get();
        // Built ONCE per pane build (not per tile) and cheaply `Clone`d
        // per-tile below — the facade's `Rc<dyn Fn>` closures would be
        // real allocations if constructed inside the realize loop.
        let sel_facade = self
            .selection
            .as_ref()
            .map(|s| RowSelection::from_index(s.clone()));

        ctx.begin_view_focus_for(self.scope_owner);
        for i in start..end {
            // Per-strategy: global row-major math for uniform/variable-row/
            // waterfall, section-local for a sectioned grid (each section
            // starts its own row band — see `SectionedGrid::tile_row_col`).
            let (row, col) = self.strategy.tile_row_col(i, self.viewport_width.get());
            let selected = self
                .selection
                .as_ref()
                .map(|s| s.is_selected(i))
                .unwrap_or(false);
            let is_focused = focused == Some(i);
            let delegate = self.delegate.clone();
            // A `Loading` tile (data not yet resident) renders a placeholder
            // skeleton instead of being skipped, so the scrollbar and layout
            // stay stable while the window loads.
            let widget = (self.with_item_fn)(i, &|item| {
                let tc = TileContext {
                    index: i,
                    row,
                    col,
                    item,
                    is_selected: selected,
                    is_focused,
                };
                delegate(&tc)
            })
            .or_else(|| ((self.row_state_fn)(i) == RowState::Loading).then(default_placeholder));
            let Some(widget) = widget else { continue };

            let inner_id = ctx.add_boxed(widget);
            let a11y_name = self.tile_a11y_label.as_ref().map(|f| f(i));
            let tile_id = ctx.add(TileA11y::new(
                inner_id,
                row + 1,
                col + 1,
                i + 1,
                total,
                selected,
                a11y_name,
            ));

            // Selection click. Returns Ignored so the gesture arena still
            // sees the PointerDown (drag-to-reorder / marquee). Deferred
            // collapse: pressing an already-selected tile (no modifiers)
            // keeps the whole (multi-)selection so it can be dragged; the
            // collapse-to-single happens on release WITHOUT a drag. The
            // press-claimed guard, Ctrl/Shift handling, and the defer rule
            // itself live in the shared `deferred_select` helper (mirrors
            // `ListView` / `TreeView`); only the focus-follows-selection
            // step is grid-specific, gated on `on_down`'s return.
            if let Some(ref sel) = sel_facade {
                let sel_click = sel.clone();
                let focused_set = self.focused_index.clone();
                let idx = i;
                let pending_collapse = Rc::new(Cell::new(false));
                ctx.apply_handlers(
                    tile_id,
                    HandlerSet::new().on_pointer_event(move |event, ctx| match event {
                        WidgetEvent::PointerDown {
                            button: PointerButton::Primary,
                            modifiers,
                            ..
                        } => {
                            if crate::data_views::deferred_select::on_down(
                                &sel_click,
                                idx,
                                *modifiers,
                                &pending_collapse,
                                ctx,
                            ) {
                                focused_set.set(Some(idx));
                            }
                            EventResponse::Ignored
                        }
                        WidgetEvent::PointerUp {
                            button: PointerButton::Primary,
                            ..
                        } => {
                            crate::data_views::deferred_select::on_up(
                                &sel_click,
                                idx,
                                &pending_collapse,
                                ctx,
                            );
                            EventResponse::Ignored
                        }
                        _ => EventResponse::Ignored,
                    }),
                );
            }

            // Activation (double-tap), context menu, drag-to-reorder, and
            // the AT click. `extra` is always applied — every tile carries
            // the access-action handler below.
            let mut extra = HandlerSet::new();
            if let Some(cb) = &self.on_tile_activate {
                let cb = cb.clone();
                let idx = i;
                extra = match self.activate_on {
                    crate::data_views::ActivateOn::SingleClick => {
                        extra.on_tap(move |_tap, ctx| cb(idx, ctx))
                    }
                    crate::data_views::ActivateOn::DoubleClick => {
                        extra.on_double_tap(move |_tap, ctx| cb(idx, ctx))
                    }
                };
            }
            if let Some(factory) = &self.tile_context_menu {
                let factory = factory.clone();
                let idx = i;
                extra = extra.context_menu(move |pos, ctx| factory(idx, pos, ctx));
            }
            if self.export.is_drag_source(self.reorderable) {
                let idx = i;
                let model_id = self.model_id;
                let anchor = ctx.self_id();
                let delegate = self.delegate.clone();
                let with_item = self.with_item_fn.clone();
                let strategy = self.strategy.clone();
                let vp_w = self.viewport_width.clone();
                let drag_gate = self.drag_fn.clone();
                // Export capture: the dragged set is selection-aware; the
                // shared `RowExport` builds the payload (clones / MIME /
                // Loading-filter / stash) when the view opted in.
                let sel_for_drag = self.selection.clone();
                let export_for_drag = self.export.clone();
                let read_for_drag = self.read_item_fn.clone();
                let snapshot_for_drag = self.snapshot_out_fn.clone();
                extra = extra.on_drag(move |phase, ctx| {
                    if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                        // The source's per-tile transferable gate.
                        if (drag_gate)(idx) == DragEligibility::NoDrag {
                            return;
                        }
                        // Selection-aware dragged set: the whole selection
                        // when the pressed tile is part of a
                        // multi-selection, else just the pressed tile.
                        let rows: Vec<usize> = match sel_for_drag.as_ref() {
                            Some(s) if s.is_selected(idx) => {
                                let mut v = s.selected_indices();
                                v.sort_unstable();
                                if v.len() <= 1 { vec![idx] } else { v }
                            }
                            _ => vec![idx],
                        };
                        let Some(payload) = export_for_drag.build_payload(
                            model_id,
                            rows,
                            &*read_for_drag,
                            &snapshot_for_drag,
                        ) else {
                            return;
                        };
                        let r = strategy.tile_rect(idx, vp_w.get());
                        let (w, h) = (r.width.max(40.0), r.height.max(40.0));
                        let delegate = delegate.clone();
                        let (row, col) = strategy.tile_row_col(idx, vp_w.get());
                        let preview = (with_item)(idx, &|item| {
                            let tc = TileContext {
                                index: idx,
                                row,
                                col,
                                item,
                                is_selected: false,
                                is_focused: false,
                            };
                            Box::new(crate::drag_preview::DragPreview::new(w, h, delegate(&tc)))
                                as Box<dyn Widget>
                        });
                        if let Some(preview) = preview {
                            ctx.start_drag_with_preview(anchor, payload, preview);
                        } else {
                            ctx.start_drag(anchor, payload);
                        }
                    }
                });
            }
            // AT / automation `Action::Click`. `TileA11y` advertises it,
            // but every pointer handler above is `on_pointer_event` /
            // `on_tap` — and the dispatcher never synthesizes a tap from
            // an access action, so a tile is otherwise undriveable by
            // assistive tech. AccessKit defines `Click` as "the
            // equivalent of a single click or tap", and the Windows /
            // macOS adapters also map AT *select-this-item* on a
            // selectable node (this one calls `set_selected`) to `Click`
            // — so select, and activate only when a single click would.
            {
                let sel = self.selection.clone();
                let focused_set = self.focused_index.clone();
                let activate = self.on_tile_activate.clone();
                let activate_on = self.activate_on;
                let idx = i;
                extra = extra.on_access_action(move |action, ctx| {
                    if action != bastyde_core::accesskit::Action::Click {
                        return EventResponse::Ignored;
                    }
                    focused_set.set(Some(idx));
                    if let Some(sel) = sel.as_ref() {
                        sel.select(idx);
                    }
                    if activate_on == crate::data_views::ActivateOn::SingleClick
                        && let Some(cb) = activate.as_ref()
                    {
                        cb(idx, ctx);
                    }
                    EventResponse::Handled
                });
            }

            ctx.apply_handlers(tile_id, extra);

            self.tile_entries.push((i, tile_id));
        }
        ctx.end_view_focus();

        // Realize the visible section headers.
        self.header_entries.clear();
        if let Some(factory) = &self.header_factory {
            let headers = self.strategy.headers_in_range(
                self.scroll_y.get(),
                self.viewport_height.get(),
                self.viewport_width.get(),
            );
            for (section, _rect) in headers {
                let body = factory(section);
                let inner = ctx.add_boxed(body);
                let title = self
                    .header_title
                    .as_ref()
                    .map(|f| f(section))
                    .unwrap_or_default();
                let hid = ctx.add(super::a11y::SectionHeaderA11y::new(inner, title));
                self.header_entries.push((section, hid));
            }
        }

        *self.tile_map.borrow_mut() = self.tile_entries.clone();
        let mut ids: Vec<WidgetId> = self.tile_entries.iter().map(|(_, id)| *id).collect();
        ids.extend(self.header_entries.iter().map(|(_, id)| *id));
        ids
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
        // Debug-only re-entrancy guard for the Relayout-bound signal writes
        // below (`scroll_y`'s anchor correction, `version`, `total_refresh`
        // — see the comments at each call site). Their safety rests on an
        // invariant this function cannot observe directly: `WidgetTree`
        // flushes every pending relayout-dirty mark exactly ONCE per
        // `layout()` / `layout_with_ops()` call, *before* the recursive
        // `place_children` walk begins — so a `Signal::set` here only
        // marks a consumer dirty for the NEXT pass, never triggers a
        // synchronous nested layout within this one. If that ever stopped
        // holding, this pane would be re-entered before this call
        // returns, and the flag below turns that into a diagnostic panic
        // instead of a silent infinite bounce or a stack overflow.
        //
        // This only catches a *synchronous* re-entry on this exact pane
        // instance — there's no pass/frame id on `LayoutContext` for
        // `place_children` to compare against, so a same-frame-but-not-
        // nested double call (were the walk order ever restructured to
        // revisit a dirtied subtree before returning to the caller) would
        // slip past it. `Signal::try_set`'s own debug-only
        // `NotifyDepthGuard` remains the general backstop for a runaway
        // observer feedback loop through these signals.
        debug_assert!(
            !self.in_place_children.get(),
            "GridBodyPane::place_children was re-entered — the single- \
             flush-per-pass invariant its scroll-anchor/total-refresh \
             signal writes rely on no longer holds"
        );
        self.in_place_children.set(true);

        // Publish our absolute origin so GridView's keyboard handler can build
        // the focused tile's window rect for the outer-scroll chase.
        self.viewport_origin
            .set(Some(bastyde_canvas::Point::new(bounds.x, bounds.y)));

        let scroll_y = self.scroll_y.get();
        let vp_w = bounds.width;
        let measures = self.strategy.measures_tiles();

        // Lookups: tile id → model index, header id → section.
        let tile_of: std::collections::HashMap<WidgetId, usize> = self
            .tile_entries
            .iter()
            .map(|(idx, id)| (*id, *idx))
            .collect();
        let header_of: std::collections::HashMap<WidgetId, usize> = self
            .header_entries
            .iter()
            .map(|(s, id)| (*id, *s))
            .collect();

        // Pass A — measure realized tiles (variable-height strategies only).
        let pre_total = if measures {
            self.strategy.total_content_height((self.len_fn)(), vp_w)
        } else {
            0.0
        };
        let mut measured: Vec<(usize, f32)> = Vec::new();
        if measures {
            measured.reserve(self.tile_entries.len());
            for child in children.iter() {
                if let Some(&model_index) = tile_of.get(&child.id) {
                    let r = self.strategy.tile_rect(model_index, vp_w);
                    let h = ctx
                        .child_size(child.id, SizeProposal::with_width(r.width))
                        .map(|s| s.height)
                        .unwrap_or(r.height);
                    measured.push((model_index, h));
                }
            }
        }

        // Feed measurements back; the strategy updates its height cache and
        // returns the scroll-anchor correction.
        let anchor_delta = if measures {
            self.strategy.observe_measured(&measured, scroll_y, vp_w)
        } else {
            0.0
        };
        // O(1) per-tile height lookup for Pass B (place_children runs every
        // scroll frame — a linear scan here would be O(tiles²)).
        let measured_h: std::collections::HashMap<usize, f32> = measured.into_iter().collect();

        // Pass B — place tiles and headers using the (possibly updated) rects.
        for child in children.iter_mut() {
            if let Some(&model_index) = tile_of.get(&child.id) {
                let r = self.strategy.tile_rect(model_index, vp_w);
                let h = if measures {
                    measured_h.get(&model_index).copied().unwrap_or(r.height)
                } else {
                    r.height
                };
                child.origin = Point::new(bounds.x + r.x, bounds.y + r.y - scroll_y);
                child.size = Size::new(r.width, h);
            } else if let Some(&section) = header_of.get(&child.id) {
                if let Some(r) = self.strategy.header_rect(section, vp_w) {
                    child.origin = Point::new(bounds.x + r.x, bounds.y + r.y - scroll_y);
                    child.size = Size::new(r.width, r.height);
                }
            }
        }

        // Apply scroll-anchor correction — see the invariant documented
        // on the re-entrancy guard at the top of this function.
        if anchor_delta.abs() > 0.01 {
            let new_scroll = (self.scroll_y.get() + anchor_delta).max(0.0);
            self.scroll_y.set(new_scroll);
        }

        // Realization re-check: corrected offsets may reveal viewport
        // tiles the estimated offsets never realized (tiles measured
        // shorter than the estimate previously left a gap at the bottom
        // until the next scroll). Request a pane rebuild for next
        // frame; the strategies' sub-pixel measurement epsilon
        // guarantees convergence.
        if measures {
            let len = (self.len_fn)();
            let vr = self.strategy.visible_range(
                self.scroll_y.get(),
                self.viewport_height.get(),
                vp_w,
                len,
            );
            if vr.start < self.prev_built_start.get() || vr.end > self.prev_built_end.get() {
                self.prev_built_start.set(vr.start);
                self.prev_built_end.set(vr.end);
                self.version.set(self.version.get() + 1);
            }

            // Total-refresh poke: the root computed `max_scroll_y` /
            // thumb ratio BEFORE this measure pass. Re-place it next
            // frame when the total changed — otherwise content past the
            // estimated total stays unreachable until the next scroll.
            let post_total = self.strategy.total_content_height(len, vp_w);
            if (post_total - pre_total).abs() > 0.01 {
                self.total_refresh.set(self.total_refresh.get() + 1);
            }
        }

        self.in_place_children.set(false);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A non-hidden generic group between the `Role::Grid` container and
        // the `Role::GridCell` tiles keeps the AT path well-formed.
        builder.set_role(bastyde_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids: Vec<WidgetId> = self.tile_entries.iter().map(|(_, id)| *id).collect();
        ids.extend(self.header_entries.iter().map(|(_, id)| *id));
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}
