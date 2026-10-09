// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ListBodyPane<T>` — the virtualized row pane underneath `ListView`.
//!
//! Splitting this out of `ListView`'s root widget is a deliberate
//! architectural choice, and the same one `TableView`, `TreeTableView` and
//! `GridView` already made: `ListView`'s direct children are this pane, the
//! scrollbar and, when asked for, the pinned section header. Rebuilds
//! triggered by scroll-buffer exits, data changes or selection changes target
//! the pane only, *never* the `ListView` root.
//!
//! Why it matters: while the user drags the scrollbar thumb, the framework
//! holds an implicit pointer capture on the scrollbar widget for the whole
//! Down→Up sequence (`GestureEvent::DragStarted` → `ctx.capture_pointer()`,
//! released on `DragEnded`). The rebuild deferral in
//! `WidgetTree::process_pending_rebuilds` skips any rebuild targeting an
//! *ancestor* of the captured widget — it has to, since rebuilding an
//! ancestor destroys the scrollbar's arena node and its gesture recognizer
//! along with it, and the fresh `ScrollBar` built in its place would carry
//! fresh drag-origin signals. With the row-rebuild target moved off
//! `ListView` (an ancestor of the scrollbar) and onto `ListBodyPane` (a
//! sibling of it), the deferral no longer applies and the body keeps
//! materializing rows as `scroll_y` advances. Before the split, dragging the
//! thumb past the 5-row buffer left the list blank until release.
//!
//! Everything the pane touches is `Rc`/`Signal`-shared with the root, which
//! is exactly what makes the pane disposable: it can be destroyed and rebuilt
//! at will without the root losing scroll position, metrics or selection.
//!
//! `ListBodyPane` is `pub(crate)` — applications still talk to `ListView`.

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

use super::BUFFER_ITEMS;
use super::sections::{HeaderFactory, ListGeometry, SectionHeaderRow};
use crate::data_views::{RowSelection, ViewId, default_placeholder};
use crate::list_source::ListSource;

/// What one of the pane's children is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneChild {
    /// The row of this model index.
    Row(usize),
    /// The header of this section.
    Header(usize),
}

/// The row-virtualization pane. Owns the realized row widgets and their
/// per-row selection / activation / drag handlers, and the section headers
/// between them. Sized to fill the rect the root allocates it; lays each row
/// at its content top minus `scroll_y` in pane-local coordinates.
pub(crate) struct ListBodyPane<T: 'static> {
    /// The erased data source — the same handle the root holds, so the pane
    /// reads rows, anchors and the DnD/lazy protocol without the root having
    /// to thread nine separate closures through.
    pub(crate) source: ListSource<T>,
    pub(crate) delegate: Rc<dyn Fn(usize, &T, bool) -> Box<dyn Widget>>,
    pub(crate) row_tooltips: crate::data_views::RowTooltips<T>,

    /// Row and header geometry shared with the `ListView` root (one set of
    /// handles, two holders — the root drives scrollbar totals, paint and
    /// keyboard, the pane drives realization, placement and measurement).
    pub(crate) geometry: ListGeometry,
    /// Builds a section's header widget; `None` when the view has no
    /// sections.
    pub(crate) header_factory: Option<HeaderFactory>,
    pub(crate) row_selection: Option<RowSelection>,
    /// Keyboard cursor, shared with the root's key handler — the per-row
    /// pointer handler moves it so arrows step from the clicked row.
    pub(crate) focused_index: Rc<Cell<Option<usize>>>,

    pub(crate) reorderable: bool,
    /// The non-drag reorder, bound once by the root. `Some` exactly when the
    /// view is reorderable; each realized row binds its own index into it and
    /// gets the menu rows and custom actions SC 2.5.7 asks for.
    pub(crate) reorder_perform: Option<crate::common::ordered_move::MoveRow>,
    /// Cross-widget export / foreign-receive machinery, cloned in from the
    /// owning `ListView` — builds the drag-start payload here; the
    /// self-reorder flag and removal-thunk stash are `Rc`-backed, so
    /// mutations made through this clone are visible to the root's
    /// `on_drag_ended` completion (installed on the root's own clone).
    pub(crate) export: crate::data_views::RowExport<T>,

    pub(crate) on_activate: Option<Rc<dyn Fn(usize, &mut teksilo_core::widget::EventContext)>>,
    pub(crate) activate_on: crate::data_views::ActivateOn,
    /// Stable, kind-tagged id of the owning `ListView` instance — stamped
    /// into the `RowDragData` payload so the source can tell a same-view
    /// reorder from a foreign drop.
    pub(crate) model_id: ViewId,
    /// The owning `ListView`'s arena id. Two jobs: it is the drag source
    /// `start_drag` is given (the root is where `install_completion` put the
    /// move-out handler), and it keys the row focus scope, since keyboard
    /// focus lands on the root and a `StandardItem`'s focus-aware styling
    /// must track *its* focus rather than this pane's.
    pub(crate) root_id: WidgetId,

    pub(crate) scroll_y: Signal<f32>,
    pub(crate) viewport_height: Rc<Cell<f32>>,
    /// Row width the root last placed, used for the drag preview's footprint.
    pub(crate) placed_content_width: Rc<Cell<f32>>,

    /// Pane-local rebuild trigger. A persistent field (re-bound each build)
    /// so `place_children`'s post-measure realization re-check can request a
    /// rebuild of this pane.
    pub(crate) version: Signal<u64>,
    /// Bound at `Relayout` on the `ListView` ROOT. The root computes
    /// scrollbar totals (`max_scroll_y`, thumb ratio) before this pane
    /// measures (parent-before-child layout order); when a measure pass
    /// changes the content total, the pane bumps this so the root re-places
    /// next frame with the corrected total — otherwise the stale totals would
    /// persist forever and content past the estimate would be unreachable. A
    /// dedicated signal rather than a `scroll_y` self-set, so an in-flight
    /// scroll animation is never cancelled.
    pub(crate) total_refresh: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    pub(crate) prev_built_start: Rc<Cell<usize>>,
    pub(crate) prev_built_end: Rc<Cell<usize>>,
    /// Section headers materialized by the latest build.
    pub(crate) prev_built_headers: Rc<Cell<(usize, usize)>>,

    // Build state
    pub(crate) item_entries: Vec<(usize, WidgetId)>,
    /// The ids this pane actually hands the arena as its children, in
    /// display order: each section header just before the first row of its
    /// section. That order is the reading order assistive technology walks,
    /// so a heading is read before the rows it names.
    ///
    /// A row that is a drag source is wrapped in a
    /// [`DragSurface`](crate::data_views::DragSurface), so its layout child is
    /// the wrapper rather than the row node itself; every other row's entry is
    /// the row node. `item_entries` (and so `row_map`) stays on the row node,
    /// which is the one carrying the accessibility properties and the
    /// keyboard-toggle target.
    pub(crate) child_roots: Vec<WidgetId>,
    /// What each of [`Self::child_roots`] is, positionally aligned with it.
    pub(crate) child_kinds: Vec<PaneChild>,
    /// Each realized header's Tab-stop state, which `place_children` keeps
    /// in step with whether the header is on screen.
    pub(crate) header_tab_stops: Vec<(usize, Signal<bool>)>,
    /// Shared mirror of [`Self::item_entries`], published at the end of each
    /// build so the `ListView` root — and anything the app hands the handle to
    /// — can resolve a model index back to the realized row's wrapper id. The
    /// wrapper is the node carrying `Role::ListBoxOption`, so this is what an
    /// `active_descendant` must point at. Same shape as `GridView`'s
    /// `tile_map`. Only realized rows appear; a row outside the virtualization
    /// window has no widget and therefore no id.
    pub(crate) row_map: Rc<RefCell<Vec<(usize, WidgetId)>>>,
    /// The owning view is [`presentational`](super::ListView::presentational):
    /// this pane and its row wrappers publish nothing.
    pub(crate) presentational: bool,
}

impl<T: 'static> ListBodyPane<T> {
    fn visible_range(&self) -> (usize, usize) {
        self.geometry.visible_range(
            self.scroll_y.get(),
            self.viewport_height.get(),
            self.source.len(),
            BUFFER_ITEMS,
        )
    }

    /// Build the header row of `section`.
    ///
    /// A control in it is a Tab stop while the header is on screen, unlike a
    /// control in a row. The rows' controls are kept out because there is a
    /// keyboard route to them through the list's own cursor, and a header has
    /// none: the cursor steps over it. Only while on screen, so that Tab
    /// reaches what the user can see and nothing the virtualization keeps
    /// realized out of view (a header far above, in a long section).
    fn build_header(
        &self,
        ctx: &mut BuildContext,
        section: usize,
    ) -> Option<(WidgetId, Signal<bool>)> {
        let factory = self.header_factory.as_ref()?;
        let title = self.geometry.section_table()?.title(section);
        let inner = ctx.add_boxed(factory(section));
        let header =
            ctx.add(SectionHeaderRow::new(inner, title).presentational(self.presentational));
        let on_screen = Signal::new(self.header_on_screen(section));
        ctx.set_tab_stop(header, on_screen.clone());
        Some((header, on_screen))
    }

    /// Append a built header to the children, in display order.
    fn push_header(&mut self, section: usize, (header, on_screen): (WidgetId, Signal<bool>)) {
        self.child_roots.push(header);
        self.child_kinds.push(PaneChild::Header(section));
        self.header_tab_stops.push((section, on_screen));
    }

    /// Whether `section`'s header shows in the viewport: inside it, and not
    /// under the pinned copy, which stands in for it.
    fn header_on_screen(&self, section: usize) -> bool {
        let count = self.source.len();
        let scroll = self.scroll_y.get().max(0.0);
        let Some(top) = self.geometry.header_top(section, count) else {
            return false;
        };
        let top = top - scroll;
        let bottom = top + self.geometry.header_height();
        bottom > 0.0
            && top < self.viewport_height.get()
            && self.geometry.covered_by_pinned_header(scroll, count) != Some(section)
    }
}

impl<T: 'static> std::fmt::Debug for ListBodyPane<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListBodyPane")
            .field("rows", &self.item_entries.len())
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

impl<T: 'static> Widget for ListBodyPane<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Self-rebuild trigger. A persistent field (not `ctx.signal`) so the
        // realization re-check in `place_children` can bump it after
        // measurement, and so the root's data / selection observers can drive
        // it without rebuilding themselves.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll position re-places rows without rebuilding (within buffer).
        // Deliberately NOT `register_animated_signal`: the root owns that
        // registration. The scheduler keys an animation to the widget that
        // registered the signal last and cancels it on that widget's rebuild
        // — registering here would make every buffer-exit rebuild kill an
        // in-flight smooth-scroll fling, which is precisely the scroll this
        // pane exists to keep serving.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Buffer-exit detection. Bumps version → rebuilds THIS pane. Because
        // the pane is a sibling of the scrollbar rather than its ancestor,
        // the rebuild deferral does not skip this one during a thumb drag —
        // see the module docs.
        let (initial_start, initial_end) = self.visible_range();
        self.prev_built_start.set(initial_start);
        self.prev_built_end.set(initial_end);
        let v_for_scroll = version.clone();
        let vp_h = self.viewport_height.clone();
        let len_for_scroll = self.source.len_fn.clone();
        let scroll_handle = self.scroll_y.observe({
            let pbs = self.prev_built_start.clone();
            let pbe = self.prev_built_end.clone();
            let pbh = self.prev_built_headers.clone();
            let geometry = self.geometry.clone();
            move |y| {
                let count = (len_for_scroll)();
                let (visible_start, visible_end) = geometry.visible_range(*y, vp_h.get(), count, 0);
                // Headers are realized with the rows around them, so leaving
                // the realized rows covers them too — except where there are
                // no rows to leave: a run of empty sections.
                let headers = geometry.visible_headers(*y, vp_h.get(), count);
                let (built_from, built_to) = pbh.get();
                let header_missing =
                    !headers.is_empty() && (headers.start < built_from || headers.end > built_to);
                if visible_start < pbs.get() || visible_end > pbe.get() || header_missing {
                    let new_start = visible_start.saturating_sub(BUFFER_ITEMS);
                    // Clamp to `count`: build realizes a `min(end, count)`
                    // window, so an unclamped `pbe` past the end would leave
                    // the dirty-check believing rows were built that never
                    // were, and the last rows of a long list would never
                    // realize on a fast scroll.
                    let new_end = (visible_end + BUFFER_ITEMS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    v_for_scroll.set(v_for_scroll.get() + 1);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // A selection change is deliberately NOT observed here. Each row
        // watches its own selectedness from inside its `ListItemWrapper` and
        // rebuilds only itself, so an arrow press replaces the two rows that
        // actually changed instead of every realized row in the pane. See
        // `ListItemWrapper`'s docs for what that identity is load-bearing
        // for. (The root separately repaints its container focus ring — see
        // `ListView::build`.)

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        self.child_roots.clear();
        self.child_kinds.clear();
        self.header_tab_stops.clear();
        // The headers of the realized rows' sections and those near the
        // viewport, each emitted just before the first row of its section.
        let count = self.source.len();
        let headers = self.geometry.headers_to_realize(
            (start, end),
            self.scroll_y.get(),
            self.viewport_height.get(),
            count,
            BUFFER_ITEMS,
        );
        self.prev_built_headers
            .set((headers.near_viewport.start, headers.near_viewport.end));
        let section_firsts = if headers.sections.is_empty() {
            Vec::new()
        } else {
            self.geometry.section_firsts(count)
        };
        let mut pending_headers = headers.sections.into_iter().peekable();
        // Lazy: nudge the source to load the realized window, and fetch more
        // as the viewport nears the end (append-only sources). This lives in
        // the pane, not the root, because the pane is what decides the
        // realization window — and it is the only one of the two that still
        // rebuilds while the thumb is captured.
        (self.source.dnd.request_window_fn)(start..end);
        if (self.source.dnd.can_fetch_more_fn)() && end + BUFFER_ITEMS >= self.source.len() {
            (self.source.dnd.fetch_more_fn)();
        }
        let selection = self.row_selection.clone();
        let is_drag_source = self.export.is_drag_source(self.reorderable);
        let model_id = self.model_id;
        let root_id = self.root_id;
        let row_state_fn = self.source.dnd.row_state_fn.clone();
        // Key the row focus scope on the ListView root (which is the
        // focusable node), not this pane — see `root_id`'s docs.
        ctx.begin_view_focus_for(root_id);
        for i in start..end {
            while let Some(&section) = pending_headers.peek()
                && section_firsts[section] <= i
            {
                pending_headers.next();
                if let Some(header) = self.build_header(ctx, section) {
                    self.push_header(section, header);
                }
            }

            // Has this row anything to show? The wrapper builds the row widget
            // itself, so the answer is needed before the wrapper exists.
            // `read_item_fn` is the cheap probe: it reports presence without
            // building anything. A `Loading` row (data not yet resident)
            // renders a placeholder skeleton rather than being skipped, so the
            // scrollbar and the layout stay stable while the window loads.
            let has_item = (self.source.read_item_fn)(i, &mut |_| {});
            if !has_item && (row_state_fn)(i) != RowState::Loading {
                continue;
            }

            // Everything the row needs to draw itself, closed over once. The
            // wrapper calls this on its first build and again whenever this
            // row's own selectedness flips, which is the only thing that can
            // change the delegate's output without the data changing.
            let body: crate::list_item_a11y::RowBody = {
                let with_item = self.source.with_item_fn.clone();
                let delegate = self.delegate.clone();
                let tips = self.row_tooltips.clone();
                let row_state = row_state_fn.clone();
                Rc::new(move |ctx: &mut BuildContext, selected: bool| {
                    // Resolve the row's tooltip inside the same borrow that
                    // builds the row: it is the only place the item is
                    // reachable, and attaching needs a `WidgetId` that does
                    // not exist until afterwards.
                    let pending_tip: RefCell<Option<crate::data_views::ResolvedRowTooltip>> =
                        RefCell::new(None);
                    let widget = (with_item)(i, &|item| {
                        if tips.is_set() {
                            *pending_tip.borrow_mut() = tips.resolve(i, item);
                        }
                        (delegate)(i, item, selected)
                    })
                    .or_else(|| ((row_state)(i) == RowState::Loading).then(default_placeholder))?;
                    let inner_id = ctx.add_boxed(widget);
                    // The app cannot reach this widget to hang a tooltip on
                    // it — the view built it — so the view attaches the
                    // resolved tip.
                    if let Some(tip) = pending_tip.into_inner() {
                        tips.attach_resolved(ctx, inner_id, tip);
                    }
                    Some(inner_id)
                })
            };

            {
                let child_id = ctx.add(
                    crate::list_item_a11y::ListItemWrapper::new(body, selection.clone(), i)
                        .presentational(self.presentational),
                );

                // A listbox is *one* Tab stop, with a cursor moving inside it.
                // Any focusable the delegate put in a row — the checkbox
                // `StandardListItem` embeds, most often — would otherwise be a
                // Tab stop of its own, which makes the Tab order track the
                // virtualization window: how many stops there are, and which,
                // then depends on the scroll position. Suppressing the row
                // subtree is honoured up the ancestor chain by
                // `tab_stop_effective`, so one call covers whatever the
                // delegate built. The control stays clickable, and `Space`
                // reaches it through the row's keyboard-toggle target.
                ctx.set_tab_stop(child_id, false);

                // Selection click handling: plain click selects,
                // Ctrl+click toggles, Shift+click extends range.
                if let Some(ref sel) = self.row_selection {
                    let sel_click = sel.clone();
                    let click_anchor = self.source.anchor(i);
                    let fi_click = self.focused_index.clone();
                    let fi_up = self.focused_index.clone();
                    // Deferred collapse: pressing an already-selected row keeps
                    // the whole (multi-)selection so it can be dragged; the
                    // collapse-to-single happens on release WITHOUT a drag,
                    // and only on a release the row still owns (see
                    // `release_completes_the_press`).
                    let pending_collapse = crate::data_views::deferred_select::pending_cell();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_pointer_event(move |event, ctx| match event {
                            teksilo_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: teksilo_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // The shared deferred-select helper owns the
                                // press-claimed guard, Ctrl/Shift handling, and
                                // the defer-collapse-on-already-selected rule; it
                                // returns false (skip the nav-cursor move) when an
                                // interactive child claimed the press.
                                // Resolve the row's CURRENT position: rows above
                                // may have shifted since this handler was built,
                                // and a deleted row must not hand its click on.
                                let Some(row) = click_anchor.index() else {
                                    return teksilo_core::event::EventResponse::Ignored;
                                };
                                if crate::data_views::deferred_select::on_down(
                                    &sel_click,
                                    row,
                                    *modifiers,
                                    &pending_collapse,
                                    ctx,
                                ) {
                                    fi_click.set(Some(row));
                                }
                                // Ignored so the gesture arena still arms the
                                // DragRecognizer for drag-to-reorder.
                                teksilo_core::event::EventResponse::Ignored
                            }
                            teksilo_core::event::WidgetEvent::PointerUp {
                                button: teksilo_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // `on_up` owns clearing `pending_collapse`, so
                                // a vanished row must still clear it — leaving
                                // it set would collapse the selection on an
                                // unrelated later release — but must not select
                                // an index that no longer exists.
                                match click_anchor.index() {
                                    Some(row) => {
                                        // The nav cursor travels with the
                                        // applied selection, not with the
                                        // press: for a direct pointer the whole
                                        // decision — cursor included — is made
                                        // here.
                                        if crate::data_views::deferred_select::on_up(
                                            &sel_click,
                                            row,
                                            &pending_collapse,
                                            ctx,
                                        ) {
                                            fi_up.set(Some(row));
                                        }
                                    }
                                    None => pending_collapse.set(None),
                                }
                                teksilo_core::event::EventResponse::Ignored
                            }
                            _ => teksilo_core::event::EventResponse::Ignored,
                        }),
                    );
                }

                // Row activation (open/commit) — a gesture, so it arbitrates
                // against the reorder drag via the gesture arena (a click
                // activates, a drag does not). `SingleClick` → `on_tap`,
                // `DoubleClick` → `on_double_tap`; Enter/Space activates too.
                if let Some(ref cb) = self.on_activate {
                    let cb = cb.clone();
                    // Anchored: a row that moved (or vanished) between build and
                    // click must not activate whoever took its slot.
                    let a = self.source.anchor(i);
                    let handlers = match self.activate_on {
                        crate::data_views::ActivateOn::SingleClick => {
                            let a = a.clone();
                            HandlerSet::new().on_tap(move |_tap, ctx| {
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

                // AT-action driving — the `Click` / `ScrollIntoView` pair
                // `ListItemWrapper::accessibility` advertises. See
                // `TreeViewBodyPane::build`'s equivalent block for why a
                // synthetic pointer click at the row's bounds is not a
                // substitute for a virtualized row.
                {
                    let sel_a11y = self.row_selection.clone();
                    let anchor_a11y = self.source.anchor(i);
                    let fi_a11y = self.focused_index.clone();
                    let geometry_a11y = self.geometry.clone();
                    let scroll_a11y = self.scroll_y.clone();
                    let vh_a11y = self.viewport_height.clone();
                    let len_a11y = self.source.len_fn.clone();
                    let activate_a11y = self.on_activate.clone();
                    ctx.apply_handlers(
                        child_id,
                        HandlerSet::new().on_access_action(move |action, ctx| {
                            use teksilo_core::accesskit::Action;
                            use teksilo_core::event::EventResponse;
                            let Some(row) = anchor_a11y.index() else {
                                return EventResponse::Ignored;
                            };
                            match action {
                                Action::Click => {
                                    // Default action: select AND activate — an
                                    // AT client has no double-click to give.
                                    if let Some(ref sel) = sel_a11y {
                                        sel.select(row);
                                    }
                                    fi_a11y.set(Some(row));
                                    if let Some(ref cb) = activate_a11y {
                                        cb(row, ctx);
                                    }
                                    EventResponse::Handled
                                }
                                Action::ScrollIntoView => {
                                    let viewport = vh_a11y.get();
                                    let scroll = scroll_a11y.get();
                                    let new_scroll = {
                                        let count = (len_a11y)();
                                        let total = geometry_a11y.total_height(count);
                                        let max = (total - viewport).max(0.0);
                                        geometry_a11y.scroll_for_ensure_visible(
                                            row, scroll, viewport, max, count,
                                        )
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

                // The non-drag alternative to that drag: the four Move
                // commands as AccessKit custom actions plus a context menu
                // carrying the same rows, both calling the root's own commit
                // closure. See `common::ordered_move`.
                if let Some(ref perform) = self.reorder_perform {
                    let anchor = self.source.anchor(i);
                    crate::common::ordered_move::RowCommands {
                        perform: crate::common::ordered_move::bind_row(
                            perform,
                            Rc::new(move || anchor.index()),
                        ),
                        from: i,
                        count: self.source.len(),
                        axis: crate::common::ordered_move::MoveAxis::Vertical,
                        extra: Vec::new(),
                    }
                    .install(ctx, child_id);
                }

                // When reorderable OR exportable, attach an on_drag handler to
                // start the drag. The preview is a fresh copy of the delegate's
                // widget for the pressed item, wrapped in a sized+raised
                // `DragPreview` so the floating widget has a stable footprint
                // and reads as "picked up" against the window surface. Uses
                // `start_drag_with_preview` so the framework overlays the
                // preview at the pointer.
                let mut row_root = child_id;
                if is_drag_source {
                    // The reorder drag goes on a wrapper that STRICTLY encloses
                    // this row, never on the row itself: a drag on the node
                    // that captures the press is driven by the capture
                    // dispatch, which runs before the arbitration advances, so
                    // it latches at `drag_slop` and decides the sequence before
                    // the list's own `PanClaim` can win at `pan_slop` — a
                    // reorderable list would not scroll under a finger at all.
                    // Enclosing it is also the only shape the tree arms
                    // `DragActivation` for: `Immediate` for a mouse (the same
                    // 5 dp it always was), `AfterLongPress` for a finger.
                    //
                    // A row with no activation hook has no gesture handler and
                    // so no arena; nothing would capture the press, the
                    // enrolment walk would never start, and the drag would
                    // compete by no path at all. The absorber is what makes the
                    // structure real — applied unconditionally rather than
                    // gated on the hooks this pane happens to know about, so a
                    // handler added here later cannot silently un-guarantee it.
                    ctx.apply_handlers(child_id, crate::data_views::press_absorber());
                    row_root = ctx.add(crate::data_views::DragSurface::new(child_id));
                    let drag_index = i;
                    let drag_model_id = model_id;
                    let drag_self_id = root_id;
                    let delegate_for_preview = self.delegate.clone();
                    let with_item_for_preview = self.source.with_item_fn.clone();
                    let metrics_for_preview = self.geometry.metrics().clone();
                    let width_for_preview = self.placed_content_width.clone();
                    let drag_gate = self.source.dnd.drag_fn.clone();
                    // Export capture: the dragged set is selection-aware; the
                    // shared `RowExport` builds the payload (clones / MIME /
                    // Loading-filter / stash) when the view opted in.
                    let sel_for_drag = self.row_selection.clone();
                    let export_for_drag = self.export.clone();
                    let read_for_drag = self.source.read_item_fn.clone();
                    let snapshot_for_drag = self.source.dnd.snapshot_out_fn.clone();
                    ctx.apply_handlers(
                        row_root,
                        crate::data_views::row_grab_surface().on_drag(move |phase, ctx| {
                            if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                                // The source's per-row transferable gate.
                                if (drag_gate)(drag_index) == DragEligibility::NoDrag {
                                    return;
                                }
                                // Selection-aware dragged set: the whole
                                // selection when the pressed row is part of a
                                // multi-selection, else just the pressed row.
                                let rows: Vec<usize> = match sel_for_drag.as_ref() {
                                    Some(s) if s.is_selected(drag_index) => {
                                        let mut v = s.selected_indices();
                                        v.sort_unstable();
                                        if v.len() <= 1 { vec![drag_index] } else { v }
                                    }
                                    _ => vec![drag_index],
                                };
                                let Some(payload) = export_for_drag.build_payload(
                                    drag_model_id,
                                    rows,
                                    &*read_for_drag,
                                    &snapshot_for_drag,
                                ) else {
                                    return;
                                };
                                let delegate = delegate_for_preview.clone();
                                let w = width_for_preview.get().max(120.0);
                                let h = metrics_for_preview.borrow_mut().row_height(drag_index);
                                let preview_opt = (with_item_for_preview)(drag_index, &|item| {
                                    Box::new(crate::drag_preview::DragPreview::new(
                                        w,
                                        h,
                                        delegate(drag_index, item, false),
                                    )) as Box<dyn Widget>
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
                self.child_roots.push(row_root);
                self.child_kinds.push(PaneChild::Row(i));
            }
        }
        // Headers past the last realized row: trailing empty sections, or a
        // viewport holding headers and no row at all.
        for section in pending_headers {
            if let Some(header) = self.build_header(ctx, section) {
                self.push_header(section, header);
            }
        }
        ctx.end_view_focus();

        // Publish the realized (index → wrapper id) map for the root's a11y.
        *self.row_map.borrow_mut() = self.item_entries.clone();

        self.child_roots.clone()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Only an allocation may seed the cached viewport — a measurement's
        // fallback would desync `build`'s realization window
        // (`common::viewport`).
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

        if self.child_kinds.is_empty() {
            return;
        }
        let count = self.source.len();

        // Auto-measure pass: measure every realized row at the pane width
        // (height-for-width), feed the heights back, and apply the
        // scroll-anchor delta so content above the viewport stays put.
        // Measurements are collected with NO metrics borrow held. Headers
        // are not measured: they are exactly `section_header_height` tall.
        if self.geometry.metrics().borrow().needs_measure() {
            let pre_total = self.geometry.total_height(count);
            let mut measured = Vec::with_capacity(self.item_entries.len());
            for (child, kind) in children.iter().zip(&self.child_kinds) {
                if let PaneChild::Row(model_index) = *kind
                    && let Some(size) =
                        ctx.child_size(child.id, SizeProposal::with_width(bounds.width))
                {
                    measured.push((model_index, size.height));
                }
            }
            let anchor = self
                .geometry
                .observe_measured(&measured, self.scroll_y.get(), count);
            if anchor.abs() > 0.01 {
                // Safe from place_children: the dirty flag is set but the
                // binding flush already ran this pass — lands next frame.
                self.scroll_y.set((self.scroll_y.get() + anchor).max(0.0));
            }

            // Realization re-check: corrected offsets may reveal viewport
            // rows that the estimated offsets never realized (rows measured
            // shorter than the estimate leave a gap at the bottom
            // otherwise). Request a pane rebuild for next frame; the 0.01
            // measurement epsilon guarantees convergence.
            let (vs, ve) = self.geometry.visible_range(
                self.scroll_y.get(),
                self.viewport_height.get(),
                count,
                0,
            );
            let headers = self.geometry.visible_headers(
                self.scroll_y.get(),
                self.viewport_height.get(),
                count,
            );
            let (built_from, built_to) = self.prev_built_headers.get();
            let header_missing =
                !headers.is_empty() && (headers.start < built_from || headers.end > built_to);
            if vs < self.prev_built_start.get() || ve > self.prev_built_end.get() || header_missing
            {
                self.prev_built_start.set(vs.saturating_sub(BUFFER_ITEMS));
                self.prev_built_end.set((ve + BUFFER_ITEMS).min(count));
                self.version.set(self.version.get() + 1);
            }

            // Total-refresh poke: the root computed `max_scroll_y` / the
            // thumb ratio BEFORE this measure pass (parent-first ordering).
            // If the content total changed, re-place the root next frame so
            // the corrected total lands — without this, content past the
            // estimated total stays unreachable forever. Terminates: a
            // re-measure of settled rows yields zero deltas (sub-pixel
            // epsilon), leaving the total fixed.
            let post_total = self.geometry.total_height(count);
            if (post_total - pre_total).abs() > 0.01 {
                self.total_refresh.set(self.total_refresh.get() + 1);
            }
        }

        let scroll_y = self.scroll_y.get();
        for (child, kind) in children.iter_mut().zip(&self.child_kinds) {
            let (top, height) = match *kind {
                PaneChild::Row(model_index) => self.geometry.item_span(model_index, count),
                PaneChild::Header(section) => {
                    let Some(top) = self.geometry.header_top(section, count) else {
                        continue;
                    };
                    (top, self.geometry.header_height())
                }
            };
            child.origin = Point::new(bounds.x, bounds.y + top - scroll_y);
            child.size = Size::new(bounds.width, height);
        }

        // Kept current for the next Tab press. Written only on a change:
        // every write repaints the header through the tab-stop binding.
        for (section, on_screen) in &self.header_tab_stops {
            let now = self.header_on_screen(*section);
            if on_screen.get() != now {
                on_screen.set(now);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.presentational {
            builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
            return;
        }
        // The pane stands in as the listbox's `Role::Group` — the
        // ARIA-blessed intermediate between `Role::ListBox` and its
        // `Role::ListBoxOption` children (`listbox` permits `group`
        // children, which is how option groups are expressed). Without a
        // non-hidden role here, AT clients that walk ListBox → option
        // directly would balk at a hidden generic container in the path.
        builder.set_role(teksilo_core::accesskit::Role::Group);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_roots.clone()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
