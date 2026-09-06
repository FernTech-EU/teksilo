// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`TreeTableView`]: build (row
//! realization, pane assembly, pointer and drag wiring), layout, placement,
//! paint and accessibility.

use super::*;
impl<T: 'static> Widget for TreeTableView<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        let row_h = self.effective_row_height();
        let header_h = self.effective_header_height();
        let indent_per_level = self.effective_indent();

        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        self.scroll_x.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_x);

        // Pane → root total refresh (auto-measure mode): re-place this
        // root when the body pane's measurements changed the content
        // total, so `max_scroll_y` / the thumb ratio pick up the
        // corrected value.
        self.pane_total_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        self.column_widths_signal.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        // `OnRelease` resize guide line — paint-only, nothing moves until the
        // button comes up.
        self.resize_preview_x.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Abandon an in-flight resize when the window goes inactive — see
        // `TableView::build` for why the missing PointerUp would otherwise
        // leave the column dragging with no button held.
        {
            let resize_state = self.resize_state.clone();
            let resize_target = self.resize_target.clone();
            let resize_preview_x = self.resize_preview_x.clone();
            ctx.effect(&ctx.window_active_signal(), move |active| {
                if !*active && resize_state.borrow().is_some() {
                    *resize_state.borrow_mut() = None;
                    resize_target.set(None);
                    resize_preview_x.set(None);
                }
            });
        }
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // Also at AccessibilityOnly (orthogonal — see `BindingLevel`) so a
        // keyboard focus move re-walks the AT tree and re-resolves
        // `active_descendant` in `accessibility()` below, even though
        // nothing about the cell's own node changed.
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Focus-aware selection + modality-gated focus ring (mirrors TableView).
        // `begin_view_focus` keys the scope signal on this root id directly —
        // the same id the body pane uses for its row scope, and independent of
        // the arena focusable flag (not yet wired here). A plain
        // `view_focus_active()` would find no focusable ancestor and fall back
        // to the constant-`true` "outside any scope" signal, lighting the ring
        // whenever ANY widget takes focus. Pop straight back; the body pane
        // re-pushes the same cached signal. `focus_visible` is the
        // keyboard/pointer modality. Both `RepaintOnly`.
        self.view_focused = ctx.begin_view_focus();
        ctx.end_view_focus();
        self.focus_visible = ctx.focus_visible();
        self.reveal_current_row_on_focus(ctx);
        self.view_focused.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.focus_visible.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        // Row-drop insertion indicator at RepaintOnly so on_drag_hover /
        // on_drag_leave `set(...)` calls dirty paint without a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Bump version on projection version (data + sort/filter +
        // expand/collapse all in one signal). Proxy observers fire
        // synchronously per rebuild, so `first_changed_index()`
        // describes exactly this change — heights of flat rows before
        // it (e.g. above an expand/collapse point) stay valid.
        let v_for_proj = version.clone();
        let proj_ver = Rc::new(Cell::new(0_u64));
        let prev_visible_count = Rc::new(Cell::new(self.source.visible_count()));
        ctx.effect(&self.source.version_signal(), {
            let metrics = self.row_metrics.clone();
            let src = self.source.clone();
            let row_sel = self.row_selection.clone();
            let cell_sel = self.cell_selection.clone();
            let prev_visible_count = prev_visible_count.clone();
            move |_| {
                metrics
                    .borrow_mut()
                    .apply_divergence(src.first_changed_index(), src.visible_count());
                // Drop any keyed selection whose node was deleted (no-op for
                // the index model). Cheap; runs on every projection change.
                if let Some(ref rs) = row_sel {
                    rs.prune();
                }
                // Cell selection is index-based (unlike the keyed row
                // selection above), and a `TreeDataSource`'s flattening
                // collapses every structural change — expand/collapse,
                // insert/remove, a re-sort — into one version bump with no
                // per-change delta to follow, unlike `TableView`'s
                // `ListModel` `DataChange` granularity. A changed visible
                // row count is a structural signal we CAN act on
                // honestly: clear the selection rather than let it point
                // at whatever node now occupies that flat index. Leave it
                // alone when the count is unchanged — a content-only
                // update (e.g. an in-place item edit) never moves a row,
                // and clearing on every projection bump would drop the
                // selection on a plain data refresh.
                let new_visible_count = src.visible_count();
                if let Some(ref cs) = cell_sel
                    && new_visible_count != prev_visible_count.get()
                {
                    cs.clear();
                }
                prev_visible_count.set(new_visible_count);
                let next = proj_ver.get() + 1;
                proj_ver.set(next);
                v_for_proj.set(next);
            }
        });

        // Sort + filter signals are NOT auto-bound onto the proxy.
        // The proxy may already carry preset comparators/predicates
        // and a custom filter mode; auto-binding would clobber them.
        // Callers wire the proxy explicitly:
        //
        //   proxy.sort_signal(tree_table.sort_signal().clone());
        //   proxy.filters_signal(tree_table.filters_signal().clone());
        //
        // Documented in the module-level comment.

        let v_for_sort = version.clone();
        let sv = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.sort_signal, move |_| {
            let next = sv.get() + 1;
            sv.set(next);
            v_for_sort.set(next);
        });
        let v_for_order = version.clone();
        let ov = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_order_signal, move |_| {
            let next = ov.get() + 1;
            ov.set(next);
            v_for_order.set(next);
        });
        let v_for_pin = version.clone();
        let pv = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_pinning_signal, move |_| {
            let next = pv.get() + 1;
            pv.set(next);
            v_for_pin.set(next);
        });
        // Selection / focus / editing effects live on the TreeBodyPane
        // (they only affect row content) — rebuilding the pane instead
        // of the root keeps those rebuilds out of the scrollbar's
        // ancestor chain during a thumb drag.

        // Display order.
        let display_indices = self.display_order();

        // Remap any `(row, display_pos)` pairs the *previous* order left in
        // `focused_cell` / `editing_cell` / `cell_selection` onto their
        // column's position under the order just computed, before it
        // overwrites `self.display_indices` below. See the identical block
        // in `TableView::build` for why this is a no-op unless THIS
        // rebuild's cause was a column reorder/pinning change.
        {
            let old_display = self.display_indices.borrow();
            if !old_display.is_empty() {
                let old_to_new: Vec<Option<usize>> = old_display
                    .iter()
                    .map(|&decl_idx| {
                        let id = &self.columns[decl_idx].id;
                        display_indices
                            .iter()
                            .position(|&new_decl_idx| self.columns[new_decl_idx].id == *id)
                    })
                    .collect();
                drop(old_display);
                imperative::remap_cell_state(
                    &self.focused_cell,
                    &self.editing_cell,
                    self.cell_selection.as_ref(),
                    &old_to_new,
                );
            }
        }
        *self.display_indices.borrow_mut() = display_indices.clone();
        let tree_decl = self.tree_column_decl_index();
        let tree_display_pos = display_indices
            .iter()
            .position(|&i| i == tree_decl)
            .unwrap_or(0);

        // Self handlers: scroll wheel + keyboard.
        let scroll_y_for_wheel = self.scroll_y.clone();
        let max_scroll_for_wheel = self.max_scroll_y.clone();
        let scroll_x_for_wheel = self.scroll_x.clone();
        let max_scroll_x_for_wheel = self.max_scroll_x.clone();
        let line_height = row_h;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;

        let column_ids_in_display_order: Vec<String> = display_indices
            .iter()
            .map(|&i| self.columns[i].id.clone())
            .collect();
        let display_col_to_id: Rc<dyn Fn(usize) -> Option<String>> = {
            let ids = column_ids_in_display_order;
            Rc::new(move |pos| ids.get(pos).cloned())
        };
        // The effective trigger set per display column: the view's, overridden
        // by the column's own, and `NONE` for a non-editable one. Resolved here
        // so the keyboard handler never has to reach a `Column<T>`.
        let display_col_triggers: Rc<dyn Fn(usize) -> EditTriggers> = {
            let view_triggers = self.edit_triggers;
            let per_display_column: Vec<EditTriggers> = display_indices
                .iter()
                .map(|&i| self.columns[i].effective_edit_triggers(view_triggers))
                .collect();
            Rc::new(move |pos| {
                per_display_column
                    .get(pos)
                    .copied()
                    .unwrap_or(EditTriggers::NONE)
            })
        };

        let navigator: Rc<dyn RowNavigator> = Rc::new(TreeNavigator::new(self.source.clone()));
        // Type-ahead resolver: read the visible row's item text through the
        // projection (`None` if the flat index isn't currently visible).
        let type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>> =
            self.type_ahead_label.clone().map(|user| {
                let src = self.source.clone();
                Rc::new(move |i: usize| src.with_row_str(i, &|item| user(item)))
                    as Rc<dyn Fn(usize) -> Option<String>>
            });

        let key_cfg = keyboard::KeyHandlerConfig {
            navigator,
            col_count: display_indices.len().max(1),
            // The same resolved position the twist and indent gutter render at
            // (see `tree_display_pos` above), so the arrow keys keep following
            // the chevron when `.tree_column()` or a user column-reorder moves
            // it off the leading position.
            tree_column_display_pos: tree_display_pos,
            focused_cell: self.focused_cell.clone(),
            cell_map: self.cell_map.clone(),
            selection_mode: self.selection_mode,
            selection: self.row_selection.clone(),
            cell_selection: self.cell_selection.clone(),
            scroll_y: self.scroll_y.clone(),
            max_scroll_y: self.max_scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            body_bounds: self.body_bounds.clone(),
            row_metrics: self.row_metrics.clone(),
            tab_traversal: self.tab_traversal,
            editing_cell: self.editing_cell.clone(),
            display_col_to_id,
            display_col_triggers,
            on_cell_edit_request: self.on_cell_edit_request.clone(),
            on_row_activate: self.on_row_activate.clone(),
            type_ahead: self.type_ahead.clone(),
            type_ahead_label,
            type_ahead_timeout: self.type_ahead_timeout,
            column_widths: self.column_widths.clone(),
            pane_boundaries: *self.pane_boundaries.borrow(),
            scroll_x: self.scroll_x.clone(),
            max_scroll_x: self.max_scroll_x.clone(),
            middle_viewport_width: self.middle_viewport_width.clone(),
        };

        // Alt+Arrow tree sibling reorder wraps the shared key handler: a move
        // among the node's siblings in the underlying `TreeModel` (cycle-free
        // by construction). Suppressed while sorted. Every other key falls
        // through to the navigator (cell/row movement, expand/collapse, edit).
        let mut shared_key = keyboard::build_key_handler(key_cfg);
        let reorderable_kbd = self.reorderable;
        let source_kbd = self.source.clone();
        let focused_kbd = self.focused_cell.clone();
        let sel_kbd = self.row_selection.clone();
        let sort_kbd = self.sort_signal.clone();
        let key_handler = move |event: &teksilo_core::event::WidgetEvent,
                                ctx: &mut EventContext|
              -> EventResponse {
            use teksilo_core::event::{Key, WidgetEvent};
            if reorderable_kbd
                && sort_kbd.get().is_none()
                && let WidgetEvent::KeyDown { key, modifiers, .. } = event
                && modifiers.alt()
                && matches!(key, Key::ArrowUp | Key::ArrowDown)
            {
                let row = focused_kbd.get().map(|(r, _)| r).or_else(|| {
                    sel_kbd
                        .as_ref()
                        .and_then(|s| s.selected_indices().first().copied())
                });
                // Sibling reorder + the "follow the moved row" bookkeeping live
                // in the source (key-typed there, so it works for an external
                // store too) and hand back the row's new flat index.
                if let Some(flat_idx) = row
                    && let Some(new_flat) =
                        source_kbd.keyboard_reorder(flat_idx, matches!(key, Key::ArrowDown))
                {
                    let col = focused_kbd.get().map(|(_, c)| c).unwrap_or(0);
                    focused_kbd.set(Some((new_flat, col)));
                    if let Some(ref s) = sel_kbd {
                        s.select(new_flat);
                    }
                    return EventResponse::Handled;
                }
            }
            shared_key(event, ctx)
        };

        let mut handlers = HandlerSet::new()
            .on_scroll({
                let overscroll_behavior = self.overscroll_behavior;
                move |event, _ctx| match event {
                    teksilo_core::event::WidgetEvent::Scroll {
                        delta, modifiers, ..
                    } => {
                        let (raw_dx, raw_dy) = match delta {
                            teksilo_core::event::ScrollDelta::Lines { x, y } => {
                                (x * line_height, y * line_height)
                            }
                            teksilo_core::event::ScrollDelta::Pixels { x, y } => (*x, *y),
                        };
                        // Shift+wheel remaps a vertical-only wheel to
                        // horizontal scroll (the `TabBar` precedent).
                        let (dx, dy) = if modifiers.shift() && raw_dx.abs() < f32::EPSILON {
                            (raw_dy, 0.0)
                        } else {
                            (raw_dx, raw_dy)
                        };

                        let mut moved_any = false;
                        if dy.abs() > 0.0 {
                            let current = scroll_y_for_wheel.get();
                            let max = max_scroll_for_wheel.get();
                            // Base off the animation target (not the rendered
                            // offset) so a mid-fling boundary correctly chains
                            // and successive notches accumulate instead of
                            // restarting from the partway-animated position.
                            let base = scroll_y_for_wheel.animation_target().unwrap_or(current);
                            let (new_y, moved) =
                                crate::common::scroll::scroll_clamp_axis(base, dy, max);
                            if moved {
                                if smooth_scrolling {
                                    scroll_y_for_wheel.animate_to(
                                        new_y,
                                        smooth_scroll_duration,
                                        Easing::EaseOut,
                                    );
                                } else {
                                    scroll_y_for_wheel.set(new_y);
                                }
                            }
                            moved_any |= moved;
                        }
                        if dx.abs() > 0.0 {
                            let current = scroll_x_for_wheel.get();
                            let max = max_scroll_x_for_wheel.get();
                            let base = scroll_x_for_wheel.animation_target().unwrap_or(current);
                            let (new_x, moved) =
                                crate::common::scroll::scroll_clamp_axis(base, dx, max);
                            if moved {
                                if smooth_scrolling {
                                    scroll_x_for_wheel.animate_to(
                                        new_x,
                                        smooth_scroll_duration,
                                        Easing::EaseOut,
                                    );
                                } else {
                                    scroll_x_for_wheel.set(new_x);
                                }
                            }
                            moved_any |= moved;
                        }
                        // Chain to an ancestor scrollable when fully
                        // clamped (unless Contain), otherwise consume —
                        // same contract as ListView/TreeView/TableView.
                        crate::common::scroll::scroll_response(
                            moved_any,
                            overscroll_behavior == OverscrollBehavior::Contain,
                        )
                    }
                    _ => EventResponse::Ignored,
                }
            })
            .on_key(key_handler)
            .clips_children(true)
            .focusable(true);

        // Row DnD: same-view reorder (reorderable) reparents/reorders the
        // dragged node(s) in the underlying `TreeModel`, cycle-guarded and
        // suppressed while sorted; plus optional foreign receive
        // (accept_foreign_rows / on_foreign_drop). Registered whenever ANY
        // of the three capabilities is enabled — a foreign-receive-only view
        // (reorderable == false) still needs to be a drop target.
        // NOTE: row DnD is still `NodeId`-typed, so it is registered only on the
        // projection path. A source-backed view (`from_source`) gets every other
        // capability but no built-in row drag yet — routing this through
        // `source.dnd.{can_accept,accept_drop}_fn` (as `TreeView` already does)
        // is a follow-up, because those closures also carry Into/Before/After
        // redirect semantics this widget does not model yet.
        // Row DnD: same-view reorder/reparent plus foreign receive, both routed
        // through the source's `can_accept` / `accept_drop` capability closures
        // — so this works over a `TreeModel`-backed projection AND an external
        // `TreeDataSource`, exactly like `TreeView`. Drop zones are the row's
        // thirds (Before / Into / After); the source's verdict decides the
        // effective position and may `Redirect` (e.g. Into-a-leaf becomes
        // After). Suppressed while sorted, where a manual order has no meaning.
        if self.export.is_drop_target(self.reorderable) || self.on_foreign_drop.is_some() {
            let my_model_id = self.model_id;
            let source_for_hover = self.source.clone();
            let metrics_for_hover = self.row_metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let header_h_for_hover = header_h;
            let feedback_for_hover = self.drop_feedback.clone();
            let sort_for_hover = self.sort_signal.clone();
            let reorderable_hover = self.reorderable;
            let export_for_hover = self.export.clone();
            let has_foreign_hook_hover = self.on_foreign_drop.is_some();
            let bounds_for_hover = self.body_bounds.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                // Column reorder is handled by the header strip
                // (`attach_header_reorder_handlers`); only row-level drops
                // get an insertion/into affordance here. Without this bail,
                // a `ColumnReorderDragData` dragged past the header into the
                // body would fall through to `on_foreign_drop` (which
                // accepts any payload type) and paint a row-drop visual for
                // a drag the header strip is already handling.
                if payload.has_typed::<ColumnReorderDragData>() {
                    feedback_for_hover.set(None);
                    return teksilo_core::DropFeedback::NoFeedback;
                }
                // Real body width, so the affordance spans the actual row area
                // rather than a placeholder.
                let viz_width = bounds_for_hover.get().width.max(1.0);
                let count = source_for_hover.visible_count();
                if count == 0 {
                    feedback_for_hover.set(None);
                    return teksilo_core::DropFeedback::NoFeedback;
                }
                let rd = payload.get_typed::<RowDragData<T>>();
                let is_same_view = rd.is_some_and(|r| r.source == my_model_id);
                let reorder_ok =
                    is_same_view && reorderable_hover && sort_for_hover.get().is_none();
                // The typed `accept_foreign_rows`/`on_rows_received` path can
                // only consume an EXPORT payload (items present); the raw
                // `on_foreign_drop` hook takes any foreign payload.
                let foreign_ok = !is_same_view
                    && (has_foreign_hook_hover
                        || export_for_hover.accepts_foreign_export(payload, my_model_id));
                if !reorder_ok && !foreign_ok {
                    feedback_for_hover.set(None);
                    return teksilo_core::DropFeedback::NoFeedback;
                }
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y - header_h_for_hover + scroll;
                let (insertion_top, row_idx, row_top, row_h) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(count);
                    let ins = m.insertion_index(content_y);
                    let r = m.row_at(content_y);
                    (m.row_top(ins), r, m.row_top(r), m.row_height(r))
                };
                let y_in_row = content_y - row_top;
                let third = (row_h / 3.0).max(f32::EPSILON);
                let drop_pos = if y_in_row < third {
                    DropPosition::Before
                } else if y_in_row > 2.0 * third {
                    DropPosition::After
                } else {
                    DropPosition::Into
                };
                // The source owns the structural verdict — including the cycle
                // guard (a node may not land inside its own subtree), which used
                // to be re-derived here against the `TreeModel`.
                // `depth` rides along so `paint` can indent the affordance to
                // the level the dropped row lands at — see `TreeView`'s twin of
                // this block. A foreign drop lands at a flat index the view
                // cannot promise a nesting for, so it claims none: depth 0.
                let (effective, depth) = if reorder_ok {
                    match (source_for_hover.dnd.can_accept_fn)(
                        payload,
                        row_idx,
                        drop_pos,
                        my_model_id,
                    ) {
                        DropResponse::Reject => {
                            if !foreign_ok {
                                feedback_for_hover.set(None);
                                return teksilo_core::DropFeedback::NoFeedback;
                            }
                            (DropPosition::Before, 0)
                        }
                        DropResponse::Accept => (drop_pos, source_for_hover.depth(row_idx)),
                        DropResponse::Redirect(p) => (p, source_for_hover.depth(row_idx)),
                    }
                } else {
                    // A foreign source has no Into/reparent semantics to honor.
                    (DropPosition::Before, 0)
                };
                if effective == DropPosition::Into {
                    let top = row_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Rect {
                        top,
                        height: row_h,
                        width: viz_width,
                        depth,
                    }));
                    teksilo_core::DropFeedback::HighlightRect {
                        rect: Rect::new(0.0, top, viz_width, row_h),
                        color: drop_into_tint(),
                    }
                } else {
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Line {
                        y: insertion_y,
                        width: viz_width,
                        depth,
                    }));
                    teksilo_core::DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: viz_width,
                    }
                }
            });

            let drop_model_id = self.model_id;
            let source_for_drop = self.source.clone();
            let metrics_for_drop = self.row_metrics.clone();
            let scroll_for_drop = self.scroll_y.clone();
            let header_h_for_drop = header_h;
            let feedback_for_drop = self.drop_feedback.clone();
            let sort_for_drop = self.sort_signal.clone();
            let reorderable_drop = self.reorderable;
            let on_foreign_for_drop = self.on_foreign_drop.clone();
            let proxy_for_foreign_hook = self.proxy.clone();
            let export_for_drop = self.export.clone();
            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                feedback_for_drop.set(None);
                // See the matching bail in `on_drag_hover` above — a column
                // reorder drop is the header strip's, never the body's
                // (`on_foreign_drop` would otherwise swallow it).
                if payload.has_typed::<ColumnReorderDragData>() {
                    return false;
                }
                let count = source_for_drop.visible_count();
                if count == 0 {
                    return false;
                }
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y - header_h_for_drop + scroll;
                let (flat_idx, row_top, row_h, ins) = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(count);
                    let idx = m.row_at(content_y);
                    let ins = m.insertion_index(content_y);
                    (idx, m.row_top(idx), m.row_height(idx), ins)
                };
                let y_in_row = content_y - row_top;
                let third = (row_h / 3.0).max(f32::EPSILON);
                let drop_pos = if y_in_row < third {
                    DropPosition::Before
                } else if y_in_row > 2.0 * third {
                    DropPosition::After
                } else {
                    DropPosition::Into
                };
                let is_same_view = payload
                    .get_typed::<RowDragData<T>>()
                    .is_some_and(|rd| rd.source == drop_model_id);
                if is_same_view && (!reorderable_drop || sort_for_drop.get().is_some()) {
                    return false;
                }
                // The source applies the move (cycle-guarded, undo-aware for an
                // external store) and reports whether it took. Gated exactly as
                // `TreeView` does, so a foreign payload the source does NOT
                // recognise still reaches the `on_rows_received` sugar below.
                if (reorderable_drop || !is_same_view)
                    && (source_for_drop.dnd.accept_drop_fn)(
                        &payload,
                        flat_idx,
                        drop_pos,
                        drop_model_id,
                    )
                {
                    if is_same_view {
                        export_for_drop.note_self_reorder();
                    }
                    return true;
                }
                // Foreign payload: the typed receive sugar first, then the raw
                // escape hatch.
                if export_for_drop.foreign_receive(&mut payload, drop_model_id, ins, ctx) {
                    return true;
                }
                // `on_foreign_drop` predates the source path and is
                // `NodeId`-typed, so it only fires when there is a projection to
                // resolve the target node through.
                if let Some(ref hook) = on_foreign_for_drop
                    && let Some(ref p) = proxy_for_foreign_hook
                    && let Some(node) = p.visible_node_id(flat_idx)
                {
                    return hook(&payload, node, drop_pos, ctx);
                }
                false
            });

            let feedback_for_leave = self.drop_feedback.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
            });

            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            let header_h_for_tick = header_h;
            handlers = handlers.on_drag_tick(move |pos, _ctx| {
                // Auto-scroll near the body band's top/bottom edge during a
                // drag (body-relative so the header doesn't count as the top).
                const EDGE: f32 = 32.0;
                const MAX_VELOCITY: f32 = 12.0;
                let body_h = (viewport_for_tick.get() - header_h_for_tick).max(0.0);
                let y = pos.y - header_h_for_tick;
                let above = (EDGE - y).max(0.0);
                let below = (y - (body_h - EDGE)).max(0.0);
                let delta = if above > 0.0 {
                    -(above / EDGE) * MAX_VELOCITY
                } else if below > 0.0 {
                    (below / EDGE) * MAX_VELOCITY
                } else {
                    0.0
                };
                if delta.abs() > 0.01 {
                    let max = max_scroll_for_tick.get();
                    let new_y = (scroll_for_tick.get() + delta).clamp(0.0, max);
                    scroll_for_tick.set(new_y);
                }
            });
        }

        // Export completion (move-out): fires on the drag source — this
        // view's root id, the stable id `start_drag` is given via the body
        // pane's `drag_anchor`. A same-view reorder called
        // `self.export.note_self_reorder()` in `on_drop` above, so it is
        // skipped here (already applied). Absent an
        // `on_rows_transferred_out` override, the default move-out runs the
        // stable-`NodeId` removal thunk `TreeBodyPane::build`'s `on_drag`
        // resolved at drag-start (ascending pre-order, so an already-removed
        // descendant of another dragged node is safely skipped).
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // ── Build children ────────────────────────────────────────────

        self.header_row_id = None;
        self.body_pane_id = None;
        self.scrollbar_id = None;
        self.h_scrollbar_id = None;
        self.empty_id = None;

        // Header strip.
        if self.show_header {
            // See `TableView::build`: a rebuild drops the pointer capture an
            // in-flight resize rides on, so the shared drag state must go with
            // it or a later bare PointerMove would resize with no button held.
            *self.resize_state.borrow_mut() = None;
            self.resize_target.set(None);
            self.resize_preview_x.set(None);

            let boundaries = *self.pane_boundaries.borrow();
            // A stretched last column has no size of its own to drag — see
            // `TableView::build`.
            let stretched_slot = self
                .stretch_last_column
                .then(|| display_indices.len().saturating_sub(1));
            let resize_columns: ColumnResizeTable = Rc::new(
                display_indices
                    .iter()
                    .enumerate()
                    .map(|(slot, &i)| {
                        let c = &self.columns[i];
                        ColumnResizeInfo {
                            id: c.id.clone(),
                            min_width: c.min_width.unwrap_or(cp::MIN_COLUMN_WIDTH_DEFAULT),
                            max_width: c.max_width,
                            resizable: c.resizable && stretched_slot != Some(slot),
                            flex: matches!(c.width, crate::ColumnWidth::Flex(_)),
                        }
                    })
                    .collect(),
            );
            let mut cell_ids: Vec<WidgetId> = Vec::with_capacity(display_indices.len());
            let active_sort = self.sort_signal.get();
            for (display_pos, &col_idx) in display_indices.iter().enumerate() {
                let col = &self.columns[col_idx];
                let current_sort = active_sort
                    .as_ref()
                    .and_then(|(id, dir)| if id == &col.id { Some(*dir) } else { None });
                let filter_zone_width = cp::FILTER_INDICATOR_SIZE + cp::CELL_PADDING_HORIZONTAL;
                let cell = HeaderCell::new(HeaderCellSpec {
                    col_id: col.id.clone(),
                    label: col.header_label.resolve_now(),
                    col_index_1based: display_pos + 1,
                    sortable: col.sortable,
                    reorderable: col.reorderable,
                    filterable: col.filterable,
                    resize_grip: cp::RESIZE_HANDLE_WIDTH,
                    filter_zone_width,
                    current_sort,
                    width_index: display_pos,
                    pane_boundaries: boundaries,
                    resize_columns: resize_columns.clone(),
                    resize_policy: self.column_resize_policy,
                    resize_state: self.resize_state.clone(),
                    resize_target: self.resize_target.clone(),
                    resize_preview_x: self.resize_preview_x.clone(),
                    table_id: self.table_id,
                    sort_signal: self.sort_signal.clone(),
                    column_widths_signal: self.column_widths_signal.clone(),
                    column_widths: self.column_widths.clone(),
                    filters_signal: self.filters_signal.clone(),
                });
                cell_ids.push(ctx.add(cell));
            }
            let header_row = HeaderRow::new(
                cell_ids,
                self.column_widths.clone(),
                cp::GRID_LINE_THICKNESS,
                *self.pane_boundaries.borrow(),
                self.scroll_x.clone(),
                self.column_widths_signal.clone(),
            );
            // Wire reorder drag-target handlers on the header strip — the
            // shared drop-target half of the mechanism `HeaderCell` already
            // escalates a press into (see `table_view::header`). The tree
            // column reorders like any other column: it carries no special
            // case here, since `tree_display_pos` (re-resolved from
            // `display_indices` on every rebuild — see below) is what makes
            // the indent/twist gutter and Left/Right expand-collapse follow
            // it wherever the drop lands, including into the leading- or
            // trailing-pinned pane.
            let header_row_id = ctx.add(header_row);
            attach_header_reorder_handlers(
                ctx,
                header_row_id,
                self.table_id,
                self.column_widths.clone(),
                self.display_indices.clone(),
                self.pane_boundaries.clone(),
                self.column_order_signal.clone(),
                self.column_pinning_signal.clone(),
                self.columns.iter().map(|c| c.id.clone()).collect(),
                self.header_strip_width.clone(),
                self.scroll_x.clone(),
            );
            self.header_row_id = Some(header_row_id);
        }

        // Body rows live in a TreeBodyPane — a sibling of the
        // scrollbar, so buffer-exit / selection / editing / expand
        // rebuilds target the pane and are never deferred by the
        // gesture-capture protection during a thumb drag.
        let row_count = self.source.visible_count();

        // Lazy: nudge the source to load the realized window, and fetch
        // the next page as the viewport nears the end (append-only
        // sources). `TreeSource` already erases a `TreeDataSource`'s
        // `row_state`/`request_window`/`can_fetch_more`/`fetch_more`
        // into `self.source.dnd` (mirrors `list_source::DndLazy` — see
        // `TableView::build`); a fully-resident source's default (inert)
        // impls leave this a no-op.
        let (vis_start, vis_end) = self.visible_range();
        (self.source.dnd.request_window_fn)(vis_start..vis_end);
        if (self.source.dnd.can_fetch_more_fn)() && vis_end + BUFFER_ROWS >= row_count {
            (self.source.dnd.fetch_more_fn)();
        }

        if row_count > 0 {
            let pane = body_pane::TreeBodyPane::<T> {
                source: self.source.clone(),
                editing_anchor: self.editing_anchor.clone(),
                columns: self.columns.clone(),
                display_indices: self.display_indices.clone(),
                column_widths: self.column_widths.clone(),
                pane_boundaries: *self.pane_boundaries.borrow(),
                scroll_x: self.scroll_x.clone(),
                tree_display_pos,
                indent_per_level,
                row_metrics: self.row_metrics.clone(),
                selection_mode: self.selection_mode,
                selection: self.row_selection.clone(),
                cell_selection: self.cell_selection.clone(),
                scroll_y: self.scroll_y.clone(),
                viewport_height: self.viewport_height.clone(),
                editing_cell: self.editing_cell.clone(),
                focused_cell: self.focused_cell.clone(),
                reorderable: self.reorderable,
                model_id: self.model_id,
                export: self.export.clone(),
                drag_anchor: ctx.self_id(),
                on_row_activate: self.on_row_activate.clone(),
                activate_on: self.activate_on,
                edit_triggers: self.edit_triggers,
                on_cell_edit_request: self.on_cell_edit_request.clone(),
                on_cell_edit_dismissed: self.on_cell_edit_dismissed.clone(),
                version: self.pane_version.clone(),
                prev_built_start: self.pane_built_start.clone(),
                prev_built_end: self.pane_built_end.clone(),
                total_refresh: self.pane_total_refresh.clone(),
                row_entries: Vec::new(),
                row_map: self.row_map.clone(),
                cell_map: self.cell_map.clone(),
            };
            self.body_pane_id = Some(ctx.add(pane));
            // An open cell editor also ends on a press that lands on no cell at
            // all — the empty band under the last row. Mounted here rather than
            // on the pane because the pane is not the hit target there.
            if let Some(handlers) = crate::table_view::body_pane::root_edit_dismiss_handler(
                &self.on_cell_edit_dismissed,
                &self.editing_cell,
                &Rc::new(
                    display_indices
                        .iter()
                        .map(|&i| self.columns[i].id.clone())
                        .collect::<Vec<_>>(),
                ),
            ) {
                ctx.apply_self_handlers(handlers);
            }
        } else if let Some(ref f) = self.empty_view {
            // Empty state — an empty tree, or a filter that matched nothing.
            self.empty_id = Some(ctx.add_boxed(f()));
        }

        // Scrollbar.
        if self.show_internal_scrollbars {
            let sb = ScrollBar::new(
                ScrollBarOrientation::Vertical,
                self.scroll_y.clone(),
                self.max_scroll_y.clone(),
                self.viewport_ratio_y.clone(),
            )
            .visual(match self.scroll_bar_style {
                ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
                ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
                ScrollBarMode::Thin => ScrollBarVisual::Thin,
            });
            self.scrollbar_id = Some(ctx.add(sb));

            // Horizontal bar — the Middle pane only, mirrors `TableView`.
            let hsb = ScrollBar::new(
                ScrollBarOrientation::Horizontal,
                self.scroll_x.clone(),
                self.max_scroll_x.clone(),
                self.viewport_ratio_x.clone(),
            )
            .visual(match self.scroll_bar_style {
                ScrollBarMode::Permanent => ScrollBarVisual::Permanent,
                ScrollBarMode::Overlay => ScrollBarVisual::Overlay,
                ScrollBarMode::Thin => ScrollBarVisual::Thin,
            });
            self.h_scrollbar_id = Some(ctx.add(hsb));
        }

        // Z-order mirrors TableView: body pane first, header last so it
        // paints above any row that bleeds into the header band on
        // overscroll.
        let mut children: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
            children.push(id);
        }
        if let Some(id) = self.empty_id {
            children.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            children.push(id);
        }
        if let Some(id) = self.h_scrollbar_id {
            children.push(id);
        }
        if let Some(id) = self.header_row_id {
            children.push(id);
        }
        let _ = (header_h, row_h);
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Only an allocation may seed the cached viewport (`common::viewport`);
        // the body pane shares this very cell, so a measurement's fallback
        // would desync its realization window.
        let size = crate::common::viewport::viewport_size(
            proposal,
            &self.viewport_height,
            Size::new(400.0, 300.0),
        );
        if proposal.height.is_some() {
            // Viewport-relative imperatives are meaningful from here on — but
            // only once a real height has landed, for the reason `laid_out`
            // exists at all.
            self.laid_out.set(true);
        }
        size.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }
        let rtl = ctx.is_rtl();
        let header_h = self.effective_header_height();
        let body_height_provisional = (bounds.height - header_h).max(0.0);

        // Parent-before-child layout order means this runs before the
        // body pane's measure pass — in auto-measure mode the scrollbar
        // totals settle one frame after a measurement change.
        let total_height = self
            .row_metrics
            .borrow_mut()
            .total_height(self.source.visible_count());
        let needs_v_scrollbar =
            self.show_internal_scrollbars && total_height > body_height_provisional + 0.5;
        // Permanent reserves a layout column for the bar; Overlay / Thin
        // float over the content, so the body spans the full width.
        let reserves_v_bar = needs_v_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_width = if reserves_v_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        // RTL mirror (see TableView::place_children): scrollbar to the
        // physical left, body/header band shifted right by its thickness.
        // Only shift when the bar actually reserves a column (Permanent).
        let band_left = if rtl && reserves_v_bar {
            bounds.x + SCROLLBAR_THICKNESS
        } else {
            bounds.x
        };
        let scrollbar_x = if rtl {
            bounds.x
        } else {
            bounds.x + bounds.width - SCROLLBAR_THICKNESS
        };
        // The header strip spans the band; snapshot its width for the
        // reorder-drop handler's RTL mirror (see `TableView::place_children`).
        self.header_strip_width.set(body_width);

        let overrides = self.column_widths_signal.get();
        let display = self.display_indices.borrow().clone();
        let widths = layout::ColumnSolver::resolve_in_order(
            &self.columns,
            &display,
            body_width,
            cp::MIN_COLUMN_WIDTH_DEFAULT,
            &overrides,
            self.stretch_last_column,
        );

        // Pane geometry (see `TableView::place_children`).
        let boundaries = *self.pane_boundaries.borrow();
        let (leading_w, middle_content_w, trailing_w) = layout::pane_widths(&widths, boundaries);
        let middle_viewport_w = (body_width - leading_w - trailing_w).max(0.0);
        let max_x = (middle_content_w - middle_viewport_w).max(0.0);
        self.max_scroll_x.set(max_x);
        self.middle_viewport_width.set(middle_viewport_w);
        let x_ratio = if middle_content_w > 0.0 {
            (middle_viewport_w / middle_content_w).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_x.set(x_ratio);
        {
            let current = self.scroll_x.get();
            let clamped = current.clamp(0.0, max_x);
            if (clamped - current).abs() > 0.001 {
                self.scroll_x.set(clamped);
            }
        }

        *self.column_widths.borrow_mut() = widths;

        let needs_h_scrollbar = self.show_internal_scrollbars && max_x > 0.5;
        let reserves_h_bar = needs_h_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_height = if reserves_h_bar {
            (body_height_provisional - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            body_height_provisional
        };

        let max_y = (total_height - body_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let y_ratio = if total_height > 0.0 {
            (body_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(y_ratio);
        self.clamp_scroll();

        let body_origin_y = bounds.y + header_h;
        // Cache the row-area rect for the keyboard handler's outer-scroll chase.
        self.body_bounds
            .set(Rect::new(band_left, body_origin_y, body_width, body_height));

        let mut next = 0;

        // Body pane fills the body region; it positions its rows
        // internally and clips them to its own bounds.
        if self.body_pane_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(band_left, body_origin_y);
                child.size = Size::new(body_width, body_height);
            }
            next += 1;
        }

        // Empty-state child fills the body region (below the header).
        if self.empty_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = Point::new(band_left, body_origin_y);
                child.size = Size::new(body_width, body_height);
            }
            next += 1;
        }

        // Scrollbar — alongside the body, below the header.
        if self.scrollbar_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                if needs_v_scrollbar {
                    child.origin = Point::new(scrollbar_x, body_origin_y);
                    child.size = Size::new(SCROLLBAR_THICKNESS, body_height);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
            next += 1;
        }

        // Horizontal scrollbar — the Middle pane's own band, below the body.
        if self.h_scrollbar_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                if needs_h_scrollbar {
                    let h_x = if rtl {
                        band_left + trailing_w
                    } else {
                        band_left + leading_w
                    };
                    child.origin = Point::new(h_x, body_origin_y + body_height);
                    child.size = Size::new(middle_viewport_w, SCROLLBAR_THICKNESS);
                } else {
                    child.origin = bounds.origin();
                    child.size = Size::ZERO;
                }
            }
            next += 1;
        }

        // Header strip last — placed at top y but emitted last so paint
        // z-order draws it above any overscrolled body rows.
        if self.header_row_id.is_some()
            && let Some(child) = children.get_mut(next)
        {
            child.origin = Point::new(band_left, bounds.y);
            child.size = Size::new(body_width, header_h);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let header_h = self.effective_header_height();
        let colors = &ctx.theme.colors;
        let scroll_y = self.scroll_y.get();
        let body_origin_y = bounds.y + header_h;
        let body_height = (bounds.height - header_h).max(0.0);
        let widths = self.column_widths.borrow();
        let body_width = widths.iter().sum::<f32>();
        let body_width_for_paint = if body_width > 0.0 {
            body_width.min(bounds.width)
        } else {
            bounds.width
        };
        // Physical left edge of the column content (see TableView::paint).
        let rtl = ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;
        let content_left = if rtl {
            bounds.x + bounds.width - body_width_for_paint
        } else {
            bounds.x
        };

        // Visible row window for the paint passes — offset-table-driven
        // so variable heights paint correctly.
        let row_count = self.source.visible_count();
        let (first_visible, last_visible) =
            self.row_metrics
                .borrow_mut()
                .visible_range(scroll_y, body_height, row_count, 0);

        // Clip the root-painted row decorations (alt-row stripes,
        // selection bands, grid lines, focus ring) to the body band —
        // `clips_children` only clips child widgets, not this widget's
        // own paint, which would otherwise bleed past the bottom edge
        // for the partially visible last row.
        canvas.set_clip(Rect::new(
            content_left,
            body_origin_y,
            body_width_for_paint,
            body_height,
        ));

        if self.alternating_rows {
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in first_visible..last_visible {
                if row_idx % 2 == 1 {
                    let y = body_origin_y + m.row_top(row_idx) - scroll_y;
                    let h = m.row_height(row_idx);
                    let rect = Rect::new(content_left, y, body_width_for_paint, h);
                    canvas.fill_rect(rect, SurfaceRole::AltRow.resolve(colors));
                }
            }
        }

        if let Some(ref sel) = self.row_selection
            && matches!(
                self.selection_mode,
                TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
            )
        {
            // Focus- and window-aware: vivid while the view holds keyboard
            // focus AND the host window is active, muted otherwise (the same
            // `SelectedInactive` serves view-unfocused and window-inactive).
            let bg = if self.view_focused.get() && ctx.window_active {
                SurfaceRole::Selected.resolve(colors)
            } else {
                SurfaceRole::SelectedInactive.resolve(colors)
            };
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in sel.selected_indices() {
                let y = body_origin_y + m.row_top(row_idx) - scroll_y;
                let h = m.row_height(row_idx);
                if y + h < body_origin_y || y > body_origin_y + body_height {
                    continue;
                }
                let rect = Rect::new(content_left, y, body_width_for_paint, h);
                canvas.fill_rect(rect, bg);
            }
        }

        let line_color = BorderRole::Divider.resolve(colors);
        let line_w = cp::GRID_LINE_THICKNESS.max(1.0);
        if matches!(self.grid_lines, GridLines::Horizontal | GridLines::Both) {
            let mut m = self.row_metrics.borrow_mut();
            for row_idx in first_visible..last_visible {
                let bottom = m.row_top(row_idx) + m.row_height(row_idx);
                let y = body_origin_y + bottom - scroll_y - line_w;
                let rect = Rect::new(content_left, y, body_width_for_paint, line_w);
                canvas.fill_rect(rect, line_color);
            }
        }

        // Pane geometry for the two column-position-dependent decorations
        // below — see `TableView::paint`.
        let boundaries = *self.pane_boundaries.borrow();
        let scroll_x = self.scroll_x.get();
        let content_bounds = Rect::new(
            content_left,
            body_origin_y,
            body_width_for_paint,
            body_height,
        );
        let (leading_rect, middle_rect, trailing_rect) =
            layout::band_rects(content_bounds, &widths, boundaries, rtl);

        if matches!(self.grid_lines, GridLines::Vertical | GridLines::Both) {
            let leading_end = boundaries.leading_count.min(widths.len());
            let middle_end = boundaries.middle_end.min(widths.len()).max(leading_end);
            crate::table_view::draw_pane_dividers(
                canvas,
                leading_rect,
                &widths[..leading_end],
                0.0,
                rtl,
                line_color,
                line_w,
            );
            crate::table_view::draw_pane_dividers(
                canvas,
                middle_rect,
                &widths[leading_end..middle_end],
                scroll_x,
                rtl,
                line_color,
                line_w,
            );
            crate::table_view::draw_pane_dividers(
                canvas,
                trailing_rect,
                &widths[middle_end..],
                0.0,
                rtl,
                line_color,
                line_w,
            );
        }

        // Focus ring — keyboard-only (`:focus-visible`) and only while the
        // view holds focus, so a mouse click never leaves a ring.
        if self.view_focused.get()
            && self.focus_visible.get()
            && let Some((focus_row, focus_col)) = self.focused_cell.get()
            && focus_col < widths.len()
            && let Some(x_off) = layout::column_logical_x(
                &widths,
                boundaries,
                scroll_x,
                body_width_for_paint,
                focus_col,
            )
        {
            let cell_w = widths[focus_col];
            let (focus_top, focus_h) = {
                let mut m = self.row_metrics.borrow_mut();
                (m.row_top(focus_row), m.row_height(focus_row))
            };
            let y = body_origin_y + focus_top - scroll_y;
            if y + focus_h >= body_origin_y && y <= body_origin_y + body_height {
                let pane_rect = if focus_col < boundaries.leading_count {
                    leading_rect
                } else if focus_col >= boundaries.middle_end {
                    trailing_rect
                } else {
                    middle_rect
                };
                canvas.set_clip(pane_rect);
                let inset = cp::FOCUS_RING_INSET;
                let stroke = cp::GRID_LINE_THICKNESS.max(1.5);
                let ring_color = BorderRole::Focused.resolve(colors);
                let rx = if rtl {
                    content_left + body_width_for_paint - x_off - cell_w + inset
                } else {
                    content_left + x_off + inset
                };
                let ry = y + inset;
                let rw = (cell_w - inset * 2.0).max(0.0);
                let rh = (focus_h - inset * 2.0).max(0.0);
                canvas.fill_rect(Rect::new(rx, ry, rw, stroke), ring_color);
                canvas.fill_rect(Rect::new(rx, ry + rh - stroke, rw, stroke), ring_color);
                canvas.fill_rect(Rect::new(rx, ry, stroke, rh), ring_color);
                canvas.fill_rect(Rect::new(rx + rw - stroke, ry, stroke, rh), ring_color);
                canvas.clear_clip();
            }
        }

        // Row-drop insertion indicator (source-accepted positions only — a
        // forbidden hover clears the signal). `y` is stored body-local.
        //
        // Both affordances are indented to the level the dropped row lands at,
        // measured from the **tree column's** own leading edge rather than the
        // body's: `.tree_column()` and a user column-reorder can move the
        // twist/indent gutter off the leading slot, and an indent measured from
        // the wrong origin points at nothing. The per-level step is this view's
        // `effective_indent()` — the very value its indent gutter renders with
        // — not the container recipe's, which describes `StandardTreeItem`.
        let drop_indent_origin = |depth: usize| -> f32 {
            let step = self.effective_indent();
            let tree_decl = self.tree_column_decl_index();
            let tree_slot = self
                .display_indices
                .borrow()
                .iter()
                .position(|&i| i == tree_decl)
                .unwrap_or(0);
            let col_x = layout::column_logical_x(
                &widths,
                boundaries,
                scroll_x,
                body_width_for_paint,
                tree_slot,
            )
            .unwrap_or(0.0);
            (col_x + depth as f32 * step).clamp(0.0, body_width_for_paint)
        };
        match self.drop_feedback.get() {
            Some(DropViz::Line { y, depth, .. }) => {
                let recipe = ctx
                    .theme
                    .style_slots
                    .list_container
                    .as_ref()
                    .map(|s| s.insertion())
                    .unwrap_or_default();
                let line_color = recipe.role.resolve(colors);
                let thickness = recipe.thickness;
                let line_y = body_origin_y + y - thickness * 0.5;
                let indent = drop_indent_origin(depth);
                // RTL mirrors the row, so the indent eats into the *right* edge
                // and the line still runs away from the row's leading side.
                let x = if rtl {
                    content_left
                } else {
                    content_left + indent
                };
                canvas.fill_rect(
                    Rect::new(x, line_y, body_width_for_paint - indent, thickness),
                    line_color,
                );
            }
            // "Drop into this container" — a box round the target row, inset on
            // every side so its horizontal edges can never be mistaken for the
            // Before / After line. Same affordance `TreeView` paints for an
            // `Into` verdict; see `ListDropIntoRecipe`.
            Some(DropViz::Rect {
                top, height, depth, ..
            }) => {
                let into = ctx
                    .theme
                    .style_slots
                    .list_container
                    .as_ref()
                    .map(|s| s.drop_into())
                    .unwrap_or_default();
                let color = into.role.resolve(colors);
                let indent = drop_indent_origin(depth);
                let x = if rtl {
                    content_left
                } else {
                    content_left + indent
                };
                let rect = Rect::new(
                    x + into.inset,
                    body_origin_y + top + into.inset,
                    (body_width_for_paint - indent - into.inset * 2.0).max(0.0),
                    (height - into.inset * 2.0).max(0.0),
                );
                let radius = teksilo_tokens::CornerRadius::uniform(into.corner_radius);
                canvas.fill_rounded_rect(rect, radius, color.with_alpha(into.fill_alpha));
                canvas.stroke_rounded_rect(rect, radius, color, into.thickness);
            }
            None => {}
        }

        canvas.clear_clip();

        // Container focus ring — keyboard focus on the view but no current cell
        // and no selection, so nothing else marks the focus. Outline the whole
        // view (see TableView / TreeView).
        let nothing_indicated = self.focused_cell.get().is_none()
            && self
                .row_selection
                .as_ref()
                .is_none_or(|s| s.selected_indices().is_empty())
            && self.cell_selection.as_ref().is_none_or(|s| s.count() == 0);
        if self.view_focused.get() && self.focus_visible.get() && nothing_indicated {
            let inset = 1.0_f32;
            let rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rect(rect, BorderRole::Focused.resolve(colors), 1.5);
        }

        // `OnRelease` column-resize guide — see `TableView::paint`.
        if let Some(x) = self.resize_preview_x.get() {
            let thickness = cp::GRID_LINE_THICKNESS.max(1.5);
            canvas.fill_rect(
                Rect::new(x - thickness * 0.5, bounds.y, thickness, bounds.height),
                BorderRole::Focused.resolve(colors),
            );
        }
    }

    /// The context-menu key opens the *current row's* menu, not the view's.
    ///
    /// A `TreeTableView` is focusable and its rows deliberately are not — the
    /// container owns focus and `set_selected` is what tells assistive
    /// technology which row is current. So the dispatcher's default of "the
    /// focused widget" would open the view's own menu, in the widget family
    /// where a per-row menu matters most.
    ///
    /// The row the user means is the focused cell's row if they have navigated,
    /// else the first selected row. Only realized rows have a widget, so a
    /// cursor scrolled outside the virtualization window resolves to nothing
    /// and the menu falls back to the view — right, because there is no row on
    /// screen for it to be about.
    fn context_menu_key_target(&self) -> Option<WidgetId> {
        let index = self.focused_cell.get().map(|(row, _col)| row).or_else(|| {
            self.row_selection
                .as_ref()
                .and_then(|s| s.selected_indices().first().copied())
        })?;
        let map = self.row_map.borrow();
        map.iter().find(|(i, _)| *i == index).map(|(_, id)| *id)
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::TreeGrid);
        // Whether the selection takes more than one row. A real property on
        // both platforms that have one: UIA's `SelectionCanSelectMultiple`
        // and AT-SPI's multiselectable state. Left unset it reads false, so a
        // multi-select view was telling every screen reader that one row was
        // the most it would ever hold.
        //
        // Gated on the mode, and the gate matters beyond tidiness:
        // `accesskit_windows` picks the event it raises on a selection change
        // from this property (`adapter.rs:189-199`), firing
        // `ElementAddedToSelection` when it is true and `ElementSelected` when
        // it is false. A single-select view publishing `true` would trade the
        // right event for the wrong one.
        if self
            .row_selection
            .as_ref()
            .is_some_and(|selection| selection.mode() == teksilo_data::SelectionMode::Multi)
        {
            builder.set_multiselectable(true);
        }

        if let Some(ref label) = self.a11y_label {
            builder.set_name(label.resolve_now());
        }
        let row_count = self.source.visible_count() + if self.show_header { 1 } else { 0 };
        let col_count = self.columns.len();
        let n = builder.inner_mut();
        n.set_row_count(row_count);
        n.set_column_count(col_count);

        // Roving focus: point active_descendant at the focused cell's own
        // AT node so a screen reader follows arrow-key cell navigation
        // and ArrowLeft/Right expand/collapse. `cell_map` is a snapshot
        // of the body pane's last realized cells; a focused cell that
        // scrolled (or collapsed) out of the realized buffer simply
        // isn't in it, so no stale id is emitted.
        if let Some((row, col)) = self.focused_cell.get()
            && let Some(cell_id) = self.realized_cell(row, col)
        {
            builder.set_active_descendant(widget_id_to_node_id(cell_id));
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        // Same order as `build()` — body pane first, header last so it
        // paints on top of any overscrolled rows.
        let mut out: Vec<WidgetId> = Vec::new();
        if let Some(id) = self.body_pane_id {
            out.push(id);
        }
        if let Some(id) = self.empty_id {
            out.push(id);
        }
        if let Some(id) = self.scrollbar_id {
            out.push(id);
        }
        if let Some(id) = self.h_scrollbar_id {
            out.push(id);
        }
        if let Some(id) = self.header_row_id {
            out.push(id);
        }
        out
    }

    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        // WCAG 1.3.2 (audit G17): read the column-header row FIRST, then the
        // body, even though `build()` / `children()` list the body first so it
        // paints beneath the header. Same id set as `children()`, reordered.
        let out: Vec<WidgetId> = [
            self.header_row_id,
            self.body_pane_id,
            self.empty_id,
            self.scrollbar_id,
            self.h_scrollbar_id,
        ]
        .into_iter()
        .flatten()
        .collect();
        if out.is_empty() { None } else { Some(out) }
    }

    fn clips_children(&self) -> bool {
        true
    }
}
