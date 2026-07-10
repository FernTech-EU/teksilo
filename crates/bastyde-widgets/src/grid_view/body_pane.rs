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
use bastyde_core::drag_payload::{DragPayload, DropOutcome};
use bastyde_core::event::{EventResponse, PointerButton, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{DragEligibility, RowState, SelectionModel};

use super::TileContext;
use super::a11y::TileA11y;
use super::layout::GridLayoutStrategy;
use crate::data_views::{DragTransferMode, RowDragData, ViewId, default_placeholder};

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
    pub(crate) reorderable: bool,
    pub(crate) model_id: ViewId,
    /// Export: when `Some`, dragged tiles carry clones of their items in the
    /// payload (source-shared with `GridView`, which is the single source of
    /// truth for the setting).
    pub(crate) export_mode: Option<DragTransferMode>,
    /// Clones a `&T` into an owned `T` for the export payload.
    #[allow(clippy::type_complexity)]
    pub(crate) clone_item_fn: Option<Rc<dyn Fn(&T) -> T>>,
    /// Builds MIME representations of the dragged items for OS / `DropZone`
    /// export.
    #[allow(clippy::type_complexity)]
    pub(crate) export_mime_fn: Option<Rc<dyn Fn(&[T]) -> Vec<(String, Vec<u8>)>>>,
    /// Read `&T` from the resident row at `index` (source-owned), used to
    /// clone dragged items for export.
    pub(crate) read_item_fn: ReadItemFn<T>,
    /// App override for removing rows moved out to a foreign target; default
    /// is the source's `on_drag_out`.
    #[allow(clippy::type_complexity)]
    pub(crate) on_rows_transferred_out: Option<Rc<dyn Fn(&[usize], &mut EventContext)>>,
    /// Source-side completion for a plain (non-custom) move-out.
    pub(crate) on_drag_out_fn: Rc<dyn Fn(&[usize])>,
    /// Shared with `GridView`'s root `on_drop`: set when a drop was applied as
    /// a same-view reorder, so `on_drag_ended` skips the move-out (already
    /// applied).
    pub(crate) self_reorder_flag: Rc<Cell<bool>>,
    /// The rows carried by the in-flight drag (shared with `GridView`, which
    /// captures them at drag-start since `on_drag_ended` gets only a
    /// `DropOutcome`).
    pub(crate) dragged_rows: Rc<RefCell<Vec<usize>>>,
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

        // Export completion: remove rows moved out to a FOREIGN target. The
        // handler fires on the drag source — THIS pane's own id, the stable
        // id `start_drag` is anchored on below. A same-view reorder is
        // applied by `GridView`'s root `on_drop`, which sets
        // `self_reorder_flag`, so it is skipped here (already applied).
        if let Some(mode) = self.export_mode {
            let flag = self.self_reorder_flag.clone();
            let stash = self.dragged_rows.clone();
            let on_out = self.on_rows_transferred_out.clone();
            let on_drag_out_fn = self.on_drag_out_fn.clone();
            ctx.apply_self_handlers(HandlerSet::new().on_drag_ended(move |outcome, ctx| {
                if flag.replace(false) {
                    stash.borrow_mut().clear();
                    return;
                }
                let accepted_elsewhere = matches!(
                    outcome,
                    DropOutcome::InApp { accepted: true } | DropOutcome::OsMove
                );
                let rows = std::mem::take(&mut *stash.borrow_mut());
                if mode != DragTransferMode::Move || !accepted_elsewhere || rows.is_empty() {
                    return;
                }
                if let Some(cb) = on_out.as_ref() {
                    // Deliver descending so a caller that removes by index
                    // one at a time stays valid across the batch.
                    let mut desc = rows;
                    desc.sort_unstable();
                    desc.reverse();
                    cb(&desc, ctx);
                } else {
                    // The erasure resolves stable keys before mutating and
                    // removes in a key-safe order.
                    (on_drag_out_fn)(&rows);
                }
            }));
        }

        // Realize the visible tiles.
        self.tile_entries.clear();
        let total = (self.len_fn)();
        let cols = self.strategy.column_count(self.viewport_width.get()).max(1);
        let (start, end) = self.visible();
        // Lazy: nudge the source to load the realized window, and fetch more
        // as the viewport nears the end (append-only sources). Fires on every
        // pane build — i.e. on each scroll-buffer exit — matching ListView.
        (self.request_window_fn)(start..end);
        if (self.can_fetch_more_fn)() && end + FETCH_BUFFER_TILES >= total {
            (self.fetch_more_fn)();
        }
        let focused = self.focused_index.get();

        ctx.begin_view_focus_for(self.scope_owner);
        for i in start..end {
            let row = i / cols;
            let col = i % cols;
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
            let tile_id = ctx.add(TileA11y::new(
                inner_id,
                row + 1,
                col + 1,
                i + 1,
                total,
                selected,
            ));

            // Selection click. Returns Ignored so the gesture arena still
            // sees the PointerDown (drag-to-reorder / marquee). Deferred
            // collapse: pressing an already-selected tile (no modifiers)
            // keeps the whole (multi-)selection so it can be dragged; the
            // collapse-to-single happens on release WITHOUT a drag (mirrors
            // `ListView`).
            if let Some(ref sel) = self.selection {
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
                            // The press belongs to an interactive child (an
                            // embedded checkbox, button, …) — let it handle
                            // the tap; don't also select the tile. Clear any
                            // stale deferred-collapse (left by a prior drag
                            // whose PointerUp the drag machinery consumed) so
                            // it can't fire on this unrelated interaction.
                            if ctx.press_claimed_by_interactive_child() {
                                pending_collapse.set(false);
                                return EventResponse::Ignored;
                            }
                            focused_set.set(Some(idx));
                            if modifiers.ctrl() {
                                sel_click.toggle(idx);
                                pending_collapse.set(false);
                            } else if modifiers.shift() {
                                sel_click.extend_to(idx);
                                pending_collapse.set(false);
                            } else if sel_click.is_selected(idx) {
                                // Defer: a following drag preserves the whole
                                // selection; a plain click collapses on
                                // release.
                                pending_collapse.set(true);
                            } else {
                                sel_click.select(idx);
                                pending_collapse.set(false);
                            }
                            EventResponse::Ignored
                        }
                        WidgetEvent::PointerUp {
                            button: PointerButton::Primary,
                            ..
                        } => {
                            // A release on an interactive child is that
                            // child's tap — never collapse the tile from it
                            // (guards against a `pending_collapse` a prior
                            // drag left stuck true).
                            if ctx.press_claimed_by_interactive_child() {
                                return EventResponse::Ignored;
                            }
                            // Reached only on a click WITHOUT a drag (an
                            // active drag consumes PointerUp). Collapse the
                            // deferred multi-selection to the clicked tile.
                            if pending_collapse.replace(false) {
                                sel_click.select(idx);
                            }
                            EventResponse::Ignored
                        }
                        _ => EventResponse::Ignored,
                    }),
                );
            }

            // Activation (double-tap), context menu, and drag-to-reorder.
            let mut extra = HandlerSet::new();
            let mut has_extra = false;
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
                has_extra = true;
            }
            if let Some(factory) = &self.tile_context_menu {
                let factory = factory.clone();
                let idx = i;
                extra = extra.context_menu(move |pos, ctx| factory(idx, pos, ctx));
                has_extra = true;
            }
            if self.reorderable || self.export_mode.is_some() {
                let idx = i;
                let model_id = self.model_id;
                let anchor = ctx.self_id();
                let delegate = self.delegate.clone();
                let with_item = self.with_item_fn.clone();
                let strategy = self.strategy.clone();
                let vp_w = self.viewport_width.clone();
                let drag_gate = self.drag_fn.clone();
                // Export capture: the dragged set is selection-aware, and
                // clones/MIME are built only when the view opted in.
                let sel_for_drag = self.selection.clone();
                let clone_for_drag = self.clone_item_fn.clone();
                let mime_for_drag = self.export_mime_fn.clone();
                let read_for_drag = self.read_item_fn.clone();
                let stash_for_drag = self.dragged_rows.clone();
                extra = extra.on_drag(move |phase, ctx| {
                    if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
                        // The source's per-tile transferable gate.
                        if (drag_gate)(idx) == DragEligibility::NoDrag {
                            return;
                        }
                        // Selection-aware dragged set: the whole selection
                        // when the pressed tile is part of a
                        // multi-selection, else just the pressed tile.
                        let mut rows: Vec<usize> = match sel_for_drag.as_ref() {
                            Some(s) if s.is_selected(idx) => {
                                let mut v = s.selected_indices();
                                v.sort_unstable();
                                if v.len() <= 1 { vec![idx] } else { v }
                            }
                            _ => vec![idx],
                        };
                        // Export clones, if opted in. Drop any row whose
                        // item isn't resident (a lazy `Loading` tile) so
                        // `rows` and `items` stay index-aligned and a Move
                        // never removes a tile whose data was never
                        // transferred.
                        let items: Option<Vec<T>> = if let Some(cf) = clone_for_drag.as_ref() {
                            let mut out = Vec::with_capacity(rows.len());
                            rows.retain(|&r| {
                                let mut got = None;
                                (read_for_drag)(r, &mut |t| got = Some(cf(t)));
                                match got {
                                    Some(v) => {
                                        out.push(v);
                                        true
                                    }
                                    None => false,
                                }
                            });
                            Some(out)
                        } else {
                            None
                        };
                        // MIME reps for OS / DropZone export.
                        let mime_pairs: Vec<(String, Vec<u8>)> =
                            match (mime_for_drag.as_ref(), items.as_ref()) {
                                (Some(mf), Some(its)) => mf(its),
                                _ => Vec::new(),
                            };
                        let mut payload = DragPayload::typed(RowDragData::<T> {
                            source: model_id,
                            rows: rows.clone(),
                            items,
                        });
                        let has_mime = !mime_pairs.is_empty();
                        for (mime, bytes) in mime_pairs {
                            payload = payload.with_mime(&mime, bytes);
                        }
                        if has_mime {
                            payload.enrich_external_from_mime();
                        }
                        *stash_for_drag.borrow_mut() = rows;
                        let r = strategy.tile_rect(idx, vp_w.get());
                        let (w, h) = (r.width.max(40.0), r.height.max(40.0));
                        let delegate = delegate.clone();
                        let cols = strategy.column_count(vp_w.get()).max(1);
                        let preview = (with_item)(idx, &|item| {
                            let tc = TileContext {
                                index: idx,
                                row: idx / cols,
                                col: idx % cols,
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
                has_extra = true;
            }
            if has_extra {
                ctx.apply_handlers(tile_id, extra);
            }

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

        // Apply scroll-anchor correction. Safe from place_children: the
        // dirty flag is set but no second layout sweep runs this frame
        // (flush_all_dirty already ran at the start of the pass), so this
        // lands next frame with no loop.
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
