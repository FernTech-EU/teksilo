// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`TableView`]: build (row
//! realization, pane assembly, pointer and drag wiring), layout, placement,
//! paint and accessibility.

use super::*;
impl<T: 'static> Widget for TableView<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        let row_h = self.effective_row_height();
        let header_h = self.effective_header_height();

        // Version signal — bumps drive a rebuild.
        let version = ctx.signal(0_u64);
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Scroll-y at Relayout: place_children re-runs without rebuild.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_y);

        // Scroll-x mirrors scroll-y: Relayout re-places the header + body
        // bands (and any pane-aware root decorations) without a rebuild.
        self.scroll_x.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        ctx.register_animated_signal(&self.scroll_x);

        // Row-drop insertion indicator at RepaintOnly so on_drag_hover /
        // on_drag_leave `set(...)` calls dirty paint without a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Pane → root total refresh (auto-measure mode): re-place this
        // root when the body pane's measurements changed the content
        // total, so `max_scroll_y` / the thumb ratio pick up the
        // corrected value.
        self.pane_total_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Column width overrides: any change re-runs place_children
        // (which calls ColumnSolver with the latest map). No rebuild
        // needed — widths flow through `column_widths` Rc into rows.
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

        // A resize drag that loses the window never gets its PointerUp: the
        // user Alt-Tabs (or a native dialog steals focus) with the button
        // down, releases it over another window, and the OS delivers the Up
        // nowhere. Abandon the gesture on deactivation, or the state outlives
        // it and the next bare PointerMove drags the column with no button
        // held. Nothing is committed — an interrupted drag leaves the column
        // wherever the last delivered move put it, which is what the user last
        // saw.
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

        // Column order + pinning: changes require a rebuild because the
        // header cells and row cells must be re-emitted in the new order
        // (each cell captures its display-position-based 1-based index).
        let v_for_order = version.clone();
        let order_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_order_signal, move |_| {
            let next = order_ver.get() + 1;
            order_ver.set(next);
            v_for_order.set(next);
        });
        let v_for_pin = version.clone();
        let pin_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.column_pinning_signal, move |_| {
            let next = pin_ver.get() + 1;
            pin_ver.set(next);
            v_for_pin.set(next);
        });
        let v_for_edit = version.clone();
        let edit_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.editing_cell, move |_| {
            let next = edit_ver.get() + 1;
            edit_ver.set(next);
            v_for_edit.set(next);
        });
        let v_for_filter = version.clone();
        let filter_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.filters_signal, move |_| {
            let next = filter_ver.get() + 1;
            filter_ver.set(next);
            v_for_filter.set(next);
        });

        // Sort signal: a change requires a rebuild because each header
        // cell's chevron child is added/removed conditionally and the
        // AccessKit `set_sort_direction` is captured at build time.
        let v_for_sort = version.clone();
        let sort_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&self.sort_signal, move |_| {
            let next = sort_ver.get() + 1;
            sort_ver.set(next);
            v_for_sort.set(next);
        });

        // Observe model changes -> bump version.
        let v_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        let upstream = (self.observe_fn)(Box::new({
            let dv = data_ver.clone();
            let sel_for_adjust = self.row_selection.clone();
            let cell_sel_for_adjust = self.cell_selection.clone();
            let metrics_for_data = self.row_metrics.clone();
            let len_for_data = self.len_fn.clone();
            let first_changed = self.first_changed_fn.clone();
            move |change| {
                // Keep row metrics in step with the data: rows before
                // the first changed index keep their heights, the rest
                // re-derive. A `SortFilterListModel` source collapses
                // everything to `Reset` — its real divergence comes
                // through the side-channel, which is what lets an
                // append keep the measured prefix.
                let divergence = match change {
                    DataChange::ItemsInserted { range } | DataChange::ItemsRemoved { range } => {
                        Some(range.start)
                    }
                    DataChange::ItemUpdated { index } => Some(*index),
                    DataChange::ItemsMoved { from, to, .. } => Some((*from).min(*to)),
                    DataChange::WindowLoaded { range } => Some(range.start),
                    DataChange::Reset => (first_changed)(),
                };
                metrics_for_data
                    .borrow_mut()
                    .apply_divergence(divergence, (len_for_data)());
                // Keep row selection in step: index-shift (index model) or
                // prune orphaned keys (keyed model). Cell selection (always
                // index-based) is adjusted separately below.
                if let Some(ref rs) = sel_for_adjust {
                    rs.on_data_change(change);
                }
                if let Some(ref s) = cell_sel_for_adjust {
                    match change {
                        DataChange::ItemsInserted { range } => {
                            s.adjust_for_row_insert(range.start, range.end - range.start);
                        }
                        DataChange::ItemsRemoved { range } => {
                            s.adjust_for_row_remove(range.start, range.end - range.start);
                        }
                        DataChange::ItemsMoved { from, to, count } => {
                            s.adjust_for_row_move(*from, *to, *count);
                        }
                        DataChange::Reset => s.clear(),
                        _ => {}
                    }
                }
                let next = dv.get() + 1;
                dv.set(next);
                v_for_data.set(next);
            }
        }));
        ctx.own_handle(upstream);

        // Observe selection changes -> bump version (rebuild updates the
        // `is_selected` arg passed to cell delegates).
        if let Some(ref rs) = self.row_selection {
            let v_for_sel = version.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                v_for_sel.set(next);
            });
            ctx.own_handle(handle);
        }
        if let Some(ref cs) = self.cell_selection {
            let v_for_csel = version.clone();
            let csel_ver = Rc::new(Cell::new(0_u64));
            ctx.effect(&cs.selection_signal(), move |_| {
                let next = csel_ver.get() + 1;
                csel_ver.set(next);
                v_for_csel.set(next);
            });
        }

        // Observe scroll position — only rebuild when visible range exits
        // the buffered window. The Relayout binding above handles
        // intra-buffer scrolls without a rebuild.
        let vp_h = self.viewport_height.clone();
        let len_for_scroll = self.len_fn.clone();
        let (built_start, built_end) = self.visible_range();
        let prev_built_start = Rc::new(Cell::new(built_start));
        let prev_built_end = Rc::new(Cell::new(built_end));
        let v_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let pbs = prev_built_start.clone();
            let pbe = prev_built_end.clone();
            let sv = scroll_ver.clone();
            let metrics = self.row_metrics.clone();
            move |y| {
                let count = (len_for_scroll)();
                let (visible_start, visible_end) =
                    metrics.borrow_mut().visible_range(*y, vp_h.get(), count, 0);
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ROWS);
                    let new_end = (visible_end + BUFFER_ROWS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    let next = sv.get() + 1;
                    sv.set(next);
                    v_for_scroll.set(next);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // Compute display order eagerly — the keyboard handler needs
        // the column count, and the header / body builds below also
        // need it. We re-write `self.display_indices` here; later
        // build steps read it.
        let display_indices_now = self.display_order();

        // Remap any `(row, display_pos)` pairs the *previous* order left in
        // `focused_cell` / `editing_cell` / `cell_selection` onto their
        // column's position under the order just computed, before it
        // overwrites `self.display_indices` below. A column reorder drag or
        // a pin toggle only bumps `version` (see the `column_order_signal` /
        // `column_pinning_signal` effects above) — display position is
        // recomputed here on every rebuild regardless of cause, so this map
        // is the identity (a no-op) unless THIS rebuild's cause was an
        // order/pinning change.
        {
            let old_display = self.display_indices.borrow();
            if !old_display.is_empty() {
                let old_to_new: Vec<Option<usize>> = old_display
                    .iter()
                    .map(|&decl_idx| {
                        let id = &self.columns[decl_idx].id;
                        display_indices_now
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
        *self.display_indices.borrow_mut() = display_indices_now.clone();

        // Self handlers: scroll wheel + keyboard + clip + focusable.
        let scroll_y_for_wheel = self.scroll_y.clone();
        let max_scroll_for_wheel = self.max_scroll_y.clone();
        let scroll_x_for_wheel = self.scroll_x.clone();
        let max_scroll_x_for_wheel = self.max_scroll_x.clone();
        let line_height = row_h;
        let overscroll_behavior = self.overscroll_behavior;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;

        // Bind focused_cell at RepaintOnly — its update redraws the
        // focus ring without rebuilding the row tree. Also at
        // AccessibilityOnly (orthogonal — see `BindingLevel`) so a
        // keyboard focus move re-walks the AT tree and re-resolves
        // `active_descendant` in `accessibility()` below, even though
        // nothing about the cell's own node changed.
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        self.focused_cell.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Focus-aware selection + modality-gated focus ring. `begin_view_focus`
        // keys the scope signal on this root id directly — the same id the body
        // pane uses for its row scope (`drag_anchor = ctx.self_id()`), and
        // independent of the arena focusable flag (not yet wired here). A plain
        // `view_focus_active()` here would find no focusable ancestor and fall
        // back to the constant-`true` "outside any scope" signal — `true`
        // whenever ANY widget holds focus, lighting every table's ring at once.
        // The signal is `true` whenever the table or any descendant holds focus,
        // so the selection band dims to `SelectedInactive` on focus-out. Pop
        // straight back; the body pane re-pushes the same cached signal.
        // `focus_visible` gates the cell ring to keyboard navigation. Both bound
        // `RepaintOnly`: a focus/modality change redraws without a rebuild.
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

        // Build the navigator + key handler. The keyboard module is
        // generic over RowNavigator so TreeTableView can plug in its own
        // tree-aware navigator.
        let navigator: Rc<dyn row_navigator::RowNavigator> =
            Rc::new(row_navigator::FlatNavigator::new(self.len_fn.clone()));
        // display_col_to_id resolves a display position back to its
        // column id, so the keyboard module doesn't need a `Column<T>`
        // reference. Snapshotted at build; rebuilds re-issue this.
        let column_ids_in_display_order: Vec<String> = display_indices_now
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
            let per_display_column: Vec<EditTriggers> = display_indices_now
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

        // Type-ahead label resolver (row -> Some(text)) built from the user's
        // `Fn(&T) -> String` + the side-effect source read: the closure only
        // fires for a resident row, so unloaded (lazy) rows resolve to `None`
        // and the search skips them.
        let type_ahead_label: Option<Rc<dyn Fn(usize) -> Option<String>>> =
            self.type_ahead_label.clone().map(|user| {
                let with_item = self.with_item_fn.clone();
                Rc::new(move |i: usize| {
                    let out = std::cell::RefCell::new(None);
                    (with_item)(i, &|item| {
                        *out.borrow_mut() = Some(user(item));
                    });
                    out.into_inner()
                }) as Rc<dyn Fn(usize) -> Option<String>>
            });

        let key_cfg = keyboard::KeyHandlerConfig {
            navigator,
            col_count: display_indices_now.len().max(1),
            // Flat table: no tree column exists. `FlatNavigator` reports no
            // children and never expands, so this value is inert — it only has
            // to be a position the cursor can actually occupy.
            tree_column_display_pos: 0,
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

        // Row DnD is owned by the backing source. The view computes the
        // geometric (target_row, position) and asks the source: `can_accept`
        // on hover gates the insertion line (forbidden → no affordance),
        // `accept_drop` on release commits the move (in-place for a
        // `ListModel`, routed for an external source). Same-view reorders and
        // foreign / cross-table drops both flow through `accept_drop` — the
        // erased closures recover SameView-vs-Foreign from the payload.
        let view_id = self.model_id;
        let can_accept_hover = self.dnd.can_accept_fn.clone();
        let scroll_for_hover = self.scroll_y.clone();
        let metrics_for_hover = self.row_metrics.clone();
        let len_for_hover = self.len_fn.clone();
        let header_h_for_hover = header_h;
        let band_width_for_hover = self.header_strip_width.clone();
        let feedback_for_hover = self.drop_feedback.clone();
        let export_for_hover = self.export.clone();

        let accept_drop_for_drop = self.dnd.accept_drop_fn.clone();
        let scroll_y_for_drop = self.scroll_y.clone();
        let header_h_for_drop = header_h;
        let metrics_for_drop = self.row_metrics.clone();
        let len_fn_for_drop = self.len_fn.clone();
        let feedback_for_drop = self.drop_feedback.clone();
        let export_for_drop = self.export.clone();
        let reorderable_for_drop = self.reorderable;

        let feedback_for_leave = self.drop_feedback.clone();
        let scroll_for_tick = self.scroll_y.clone();
        let max_scroll_for_tick = self.max_scroll_y.clone();
        let viewport_for_tick = self.viewport_height.clone();
        let header_h_for_tick = header_h;

        // Alt+Arrow reorder wraps the shared key handler: the move is a
        // synthetic same-view `RowDragData` through the source's
        // `accept_drop`, so it travels exactly the pointer-drop path. Every
        // other key falls through to the shared navigator (cell/row
        // movement, edit, etc.).
        let mut shared_key = keyboard::build_key_handler(key_cfg);
        let reorderable_kbd = self.reorderable;
        let accept_drop_kbd = self.dnd.accept_drop_fn.clone();
        let stash_kbd = self.dnd.stash_drag_keys_fn.clone();
        let focused_kbd = self.focused_cell.clone();
        let sel_kbd = self.row_selection.clone();
        let len_kbd = self.len_fn.clone();
        let key_handler = move |event: &teksilo_core::event::WidgetEvent,
                                ctx: &mut teksilo_core::widget::EventContext|
              -> teksilo_core::event::EventResponse {
            use teksilo_core::event::{EventResponse, Key, WidgetEvent};
            if reorderable_kbd
                && let WidgetEvent::KeyDown { key, modifiers, .. } = event
                && modifiers.alt()
            {
                let count = (len_kbd)();
                if count > 0 {
                    let cur = focused_kbd.get().map(|(r, _)| r).or_else(|| {
                        sel_kbd
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied())
                    });
                    if let Some(idx) = cur {
                        let mv = match key {
                            Key::ArrowUp if idx > 0 => {
                                Some((idx - 1, DropPosition::Before, idx - 1))
                            }
                            Key::ArrowDown if idx + 1 < count => {
                                Some((idx + 1, DropPosition::After, idx + 1))
                            }
                            _ => None,
                        };
                        if let Some((target, position, dest)) = mv {
                            // Synthetic same-view payloads must stash the
                            // dragged row's key at construction — the accept
                            // path resolves identity from the stash, never
                            // from `rows`.
                            (stash_kbd)(&[idx]);
                            let payload =
                                teksilo_core::drag_payload::DragPayload::typed(RowDragData::<T> {
                                    source: view_id,
                                    rows: vec![idx],
                                    items: None,
                                });
                            if (accept_drop_kbd)(&payload, target, position, view_id) {
                                if let Some(ref s) = sel_kbd {
                                    s.select(dest);
                                }
                                let col = focused_kbd.get().map(|(_, c)| c).unwrap_or(0);
                                focused_kbd.set(Some((dest, col)));
                            }
                            return EventResponse::Handled;
                        }
                    }
                }
            }
            shared_key(event, ctx)
        };

        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                teksilo_core::event::WidgetEvent::Scroll {
                    delta, modifiers, ..
                } => {
                    let (raw_dx, raw_dy) = match delta {
                        teksilo_core::event::ScrollDelta::Lines { x, y } => {
                            (x * line_height, y * line_height)
                        }
                        teksilo_core::event::ScrollDelta::Pixels { x, y } => (*x, *y),
                    };
                    // Shift+wheel remaps a vertical-only wheel to horizontal
                    // scroll (the `TabBar` precedent) — a genuine two-axis
                    // trackpad delta (both native `dx` and `dy` nonzero)
                    // passes through unremapped either way.
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
                    // Chain to an ancestor scrollable when fully clamped on
                    // every axis touched (unless Contain), otherwise consume.
                    crate::common::scroll::scroll_response(
                        moved_any,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                _ => teksilo_core::event::EventResponse::Ignored,
            })
            .clips_children(true)
            .focusable(true);

        handlers = handlers.on_key(key_handler);

        // Row-level drop target: registered only when this table can
        // reorder its own rows or accept foreign ones (mirrors ListView).
        // Column reorder lives entirely on the header strip
        // (`attach_header_reorder_handlers`) and is untouched by this gate.
        if self.export.is_drop_target(self.reorderable) {
            handlers = handlers
                .on_drag_hover(move |payload, position, _ctx| {
                    // Column reorder is handled by the header strip; only
                    // row-level drops (same-view `RowDragData` or a foreign
                    // payload the source accepts) get an insertion line here.
                    if payload.has_typed::<ColumnReorderDragData>() {
                        feedback_for_hover.set(None);
                        return teksilo_core::DropFeedback::NoFeedback;
                    }
                    let body_y = position.y - header_h_for_hover;
                    let scroll = scroll_for_hover.get();
                    let content_y = body_y + scroll;
                    let len = (len_for_hover)();
                    let (ins, line_y) = {
                        let mut m = metrics_for_hover.borrow_mut();
                        m.resize(len);
                        let ins = m.insertion_index(content_y);
                        (ins, m.row_top(ins) - scroll)
                    };
                    let width = band_width_for_hover.get();
                    // Source-owned validation: paint the line only when the
                    // source does not reject the hovered position. A foreign
                    // exported row is allowed when `accept_foreign_rows` is on
                    // even though a bare `ListModel`'s `can_accept` rejects
                    // the `Foreign` branch.
                    let allowed = flat_insertion_target(ins, len).is_some_and(|(target, pos)| {
                        !matches!(
                            (can_accept_hover)(payload, target, pos, view_id),
                            DropResponse::Reject
                        ) || export_for_hover.accepts_foreign_export(payload, view_id)
                    });
                    if allowed {
                        feedback_for_hover.set(Some((line_y, width)));
                        teksilo_core::DropFeedback::InsertionLine { y: line_y, width }
                    } else {
                        feedback_for_hover.set(None);
                        teksilo_core::DropFeedback::NoFeedback
                    }
                })
                .on_drop(move |mut payload, position, ctx| {
                    feedback_for_drop.set(None);
                    if payload.has_typed::<ColumnReorderDragData>() {
                        return false;
                    }
                    let body_y = position.y - header_h_for_drop;
                    let scroll = scroll_y_for_drop.get();
                    let content_y = body_y + scroll;
                    let len = (len_fn_for_drop)();
                    let ins = {
                        let mut m = metrics_for_drop.borrow_mut();
                        m.resize(len);
                        m.insertion_index(content_y)
                    };
                    let is_same_view = payload
                        .get_typed::<RowDragData<T>>()
                        .is_some_and(|rd| rd.source == view_id);
                    // Route the drop to the source's accept_drop first. A
                    // same-view reorder only happens when the table is
                    // `reorderable`; a foreign payload is the source's
                    // call (a bare ListModel rejects it).
                    if (reorderable_for_drop || !is_same_view)
                        && let Some((target, position_kind)) = flat_insertion_target(ins, len)
                        && (accept_drop_for_drop)(&payload, target, position_kind, view_id)
                    {
                        // Only suppress our OWN move-out for a genuine
                        // same-view drop.
                        if is_same_view {
                            export_for_drop.note_self_reorder();
                        }
                        return true;
                    }
                    // Otherwise, the shared foreign-receive sugar
                    // (peek-before-take).
                    export_for_drop.foreign_receive(&mut payload, view_id, ins, ctx)
                })
                .on_drag_leave(move |_ctx| {
                    feedback_for_leave.set(None);
                })
                .on_drag_tick(move |pos, _ctx| {
                    // Auto-scroll when the pointer lingers within 32 px of the
                    // body band's top/bottom edge during a drag (body-relative
                    // so the header doesn't count as the top edge).
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
        // table's root id, the stable id `start_drag` was given.
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // ── Build children ────────────────────────────────────────────
        self.header_row_id = None;
        self.body_pane_id = None;
        self.scrollbar_id = None;
        self.h_scrollbar_id = None;
        self.empty_id = None;

        // Display order was already computed above (before the
        // keyboard handler was wired); pull it back into a local for
        // the header / body loops.
        let display_indices = display_indices_now;

        // Header strip: build first so it sits above the body in the
        // child order (place_children iterates in this order).
        if self.show_header {
            // A rebuild destroys (and re-creates) every header cell, which
            // drops the pointer capture an in-flight resize depends on. Clear
            // the shared drag state with it: a `ResizeState` that outlived its
            // anchor would otherwise let the next bare PointerMove over the
            // same column resize it with no button held.
            *self.resize_state.borrow_mut() = None;
            self.resize_target.set(None);
            self.resize_preview_x.set(None);

            let boundaries = *self.pane_boundaries.borrow();
            // A stretched last column has no size of its own to drag: its
            // trailing grip (and the AT step actions behind the same flag)
            // is off. The grip on its *leading* edge still resizes its
            // predecessor.
            let stretched_slot = self
                .stretch_last_column
                .then(|| display_indices.len().saturating_sub(1));
            let resize_columns: header::ColumnResizeTable = Rc::new(
                display_indices
                    .iter()
                    .enumerate()
                    .map(|(slot, &i)| {
                        let c = &self.columns[i];
                        header::ColumnResizeInfo {
                            id: c.id.clone(),
                            min_width: c.min_width.unwrap_or(cp::MIN_COLUMN_WIDTH_DEFAULT),
                            max_width: c.max_width,
                            resizable: c.resizable && stretched_slot != Some(slot),
                            flex: matches!(c.width, ColumnWidth::Flex(_)),
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
                // Filter zone width: indicator glyph + a small horizontal
                // padding for tap tolerance. Mirrors the layout of the
                // HStack inside HeaderCell::build.
                let filter_zone_width = cp::FILTER_INDICATOR_SIZE + cp::CELL_PADDING_HORIZONTAL;
                let cell = header::HeaderCell::new(header::HeaderCellSpec {
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
            let header_row = header::HeaderRow::new(
                cell_ids,
                self.column_widths.clone(),
                cp::GRID_LINE_THICKNESS,
                *self.pane_boundaries.borrow(),
                self.scroll_x.clone(),
                self.column_widths_signal.clone(),
            );
            // Wire reorder drag-target handlers on the header strip.
            let header_row_id = ctx.add(header_row);
            header::attach_header_reorder_handlers(
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

        let row_count = (self.len_fn)();

        // Lazy: nudge the source to load the realized window, and fetch the
        // next page as the viewport nears the end (append-only sources). A
        // fully-resident source leaves these inert.
        let (vis_start, vis_end) = self.visible_range();
        (self.dnd.request_window_fn)(vis_start..vis_end);
        if (self.dnd.can_fetch_more_fn)() && vis_end + BUFFER_ROWS >= row_count {
            (self.dnd.fetch_more_fn)();
        }

        if row_count == 0 {
            // Empty state.
            if let Some(ref f) = self.empty_view {
                let id = ctx.add_boxed(f());
                self.empty_id = Some(id);
            }
        } else {
            // Hoist the row pane into its own widget so that
            // scroll-buffer-exit rebuilds (which happen mid-thumb-drag
            // when the user scrolls past the buffered range) target a
            // sibling of the scrollbar rather than the scrollbar's
            // ancestor. Rebuilding the ancestor would be deferred by
            // the framework (to preserve the captured drag), leaving
            // the body empty until the user released the thumb.
            let pane = body_pane::BodyPane::<T> {
                len_fn: self.len_fn.clone(),
                with_item_fn: self.with_item_fn.clone(),
                drag_fn: self.dnd.drag_fn.clone(),
                row_state_fn: self.dnd.row_state_fn.clone(),
                columns: self.columns.clone(),
                display_indices: self.display_indices.clone(),
                column_widths: self.column_widths.clone(),
                pane_boundaries: *self.pane_boundaries.borrow(),
                scroll_x: self.scroll_x.clone(),
                row_metrics: self.row_metrics.clone(),
                selection_mode: self.selection_mode,
                selection: self.row_selection.clone(),
                cell_selection: self.cell_selection.clone(),
                scroll_y: self.scroll_y.clone(),
                viewport_height: self.viewport_height.clone(),
                editing_cell: self.editing_cell.clone(),
                focused_cell: self.focused_cell.clone(),
                reorderable: self.reorderable,
                export: self.export.clone(),
                snapshot_out_fn: self.dnd.snapshot_out_fn.clone(),
                anchor_fn: self.anchor_fn.clone(),
                editing_anchor: self.editing_anchor.clone(),
                view_id: self.model_id,
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
            if let Some(handlers) = body_pane::root_edit_dismiss_handler(
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
        }

        // Scrollbar (single internal vertical bar).
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

            // Horizontal bar — the Middle pane only. Visibility (max_scroll_x
            // > 0) and geometry (band_left + pinned-pane offsets) are decided
            // in `place_children`, same as the vertical bar's `needs_scrollbar`
            // gate; here we just build it unconditionally so it exists to be
            // placed (zero-sized and skipped when not needed).
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

        // Z-order: body rows first, then empty/scrollbar, then header
        // last. The header band overlaps the top of the body region
        // when `scroll_y > 0` (rows positioned at `body_origin_y +
        // row_idx * row_h - scroll_y` can extend above
        // `body_origin_y` on overscroll). Painting the header last
        // means it sits on top of any row that bleeds into the
        // header band — without this fix, scrolled-out rows would
        // visibly draw over the header label.
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
        // Suppress the unused-binding warning on header_h while the
        // value is consumed by `place_children` via the same helper.
        let _ = header_h;
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
        // Provisional — the vertical scrollbar's own need is decided
        // against this (a possible tiny inaccuracy if reserving room for
        // the horizontal bar below would itself flip that decision; not
        // worth a fixed-point iteration for a dual-scrollbar corner case).
        let body_height_provisional = (bounds.height - header_h).max(0.0);

        // Parent-before-child layout order means this runs before the
        // body pane's measure pass — in auto-measure mode the scrollbar
        // totals settle one frame after a measurement change.
        let total_height = self.total_content_height();
        let needs_v_scrollbar =
            self.show_internal_scrollbars && total_height > body_height_provisional + 0.5;
        // Permanent reserves a column for the bar; Overlay / Thin float
        // over the content, so rows span the full width.
        let reserves_v_bar = needs_v_scrollbar && self.scroll_bar_style == ScrollBarMode::Permanent;
        let body_width = if reserves_v_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        // Under RTL the vertical scrollbar moves to the physical left
        // (matching `ScrollArea`), so the body/header band shifts right
        // by its thickness. `band_left` is the shared origin for the
        // body pane, empty state, and header; `scrollbar_x` is the
        // scrollbar's own physical x. The paint pass derives the same
        // content region from these conventions so the two never drift.
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
        // reorder-drop handler's RTL mirror.
        self.header_strip_width.set(body_width);

        // Resolve column widths in display order, honoring any
        // user-resize overrides from `column_widths_signal`.
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

        // Pane geometry: the Middle pane's viewport (`body_width` minus the
        // pinned panes) and the horizontal scroll headroom it implies.
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
        // Clamp scroll_x — a pane shrink (window narrowed, a column grew)
        // must not leave scroll_x stranded past the new max (mirrors
        // `clamp_scroll` for scroll_y).
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

        // Vertical scrollbar totals, against the FINAL body_height (after
        // any horizontal-bar reservation) so the range stays accurate when
        // both bars show at once.
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

        // BodyPane fills the body region. It positions its rows
        // internally using its own scroll signal and clips them to
        // its own bounds.
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

        // Scrollbar — alongside the body, below the header. Physical
        // left under RTL, physical right under LTR.
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

        // Horizontal scrollbar — the Middle pane's own band, below the
        // body, never overlapping a pinned pane.
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
        // Physical left edge of the column content. Under RTL the band is
        // right-aligned within `bounds` (the scrollbar took the left), so
        // content runs from `bounds.right() - body_width` leftward —
        // exactly where `place_children` reverse-placed the cells.
        let rtl = ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;
        let content_left = if rtl {
            bounds.x + bounds.width - body_width_for_paint
        } else {
            bounds.x
        };

        // Visible row window for the paint passes — offset-table-driven
        // so variable heights paint correctly. One metrics borrow per
        // pass; nothing inside re-enters the metrics.
        let row_count = (self.len_fn)();
        let (first_visible, last_visible) =
            self.row_metrics
                .borrow_mut()
                .visible_range(scroll_y, body_height, row_count, 0);

        // Clip the root-painted row decorations (alt-row stripes,
        // selection bands, grid lines, focus ring) to the body band.
        // `clips_children` only clips child WIDGETS — this widget's own
        // paint would otherwise bleed past the table's bottom edge for
        // the partially visible last row (its stripe/grid-line rect
        // spans the full row height).
        canvas.set_clip(Rect::new(
            content_left,
            body_origin_y,
            body_width_for_paint,
            body_height,
        ));

        // Alt-row backgrounds — paint odd visible rows. Parity keys on
        // the row index, not on y, so stripes stay stable under
        // variable heights.
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

        // Selection highlights — row selection modes only.
        if let Some(ref sel) = self.row_selection
            && matches!(
                self.selection_mode,
                TableSelectionMode::SingleRow | TableSelectionMode::MultiRow
            )
        {
            // Focus- and window-aware: vivid `Selected` while the table holds
            // keyboard focus AND the host window is active; muted
            // `SelectedInactive` once focus moves elsewhere or the window goes
            // inactive (the same desaturation serves both states).
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

        // Grid lines.
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
        // below (vertical grid lines, the cell focus ring): both must clip
        // to the target column's OWN pane, or a scrolled Middle-pane
        // decoration could paint over a pinned Leading/Trailing column
        // within the same row band (the outer body clip above only bounds
        // the row's outer edges, not the seam between panes).
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
            draw_pane_dividers(
                canvas,
                leading_rect,
                &widths[..leading_end],
                0.0,
                rtl,
                line_color,
                line_w,
            );
            draw_pane_dividers(
                canvas,
                middle_rect,
                &widths[leading_end..middle_end],
                scroll_x,
                rtl,
                line_color,
                line_w,
            );
            draw_pane_dividers(
                canvas,
                trailing_rect,
                &widths[middle_end..],
                0.0,
                rtl,
                line_color,
                line_w,
            );
        }

        // Focus ring on the currently-focused cell — keyboard-only
        // (`:focus-visible`) and only while the table itself holds focus, so a
        // mouse click never leaves a ring and an unfocused table shows none.
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
                // `x_off` is the leading-side offset (sum of widths before
                // the focused column). Under RTL that offset is measured
                // from the right edge of the content band.
                let rx = if rtl {
                    content_left + body_width_for_paint - x_off - cell_w + inset
                } else {
                    content_left + x_off + inset
                };
                let ry = y + inset;
                let rw = (cell_w - inset * 2.0).max(0.0);
                let rh = (focus_h - inset * 2.0).max(0.0);
                // Top
                canvas.fill_rect(Rect::new(rx, ry, rw, stroke), ring_color);
                // Bottom
                canvas.fill_rect(Rect::new(rx, ry + rh - stroke, rw, stroke), ring_color);
                // Left
                canvas.fill_rect(Rect::new(rx, ry, stroke, rh), ring_color);
                // Right
                canvas.fill_rect(Rect::new(rx + rw - stroke, ry, stroke, rh), ring_color);
                canvas.clear_clip();
            }
        }

        // Row-drop insertion indicator (source-accepted positions only —
        // a forbidden hover clears the signal, so no line shows). `y` is
        // stored body-local; the band clip is already active.
        if let Some((y, _width)) = self.drop_feedback.get() {
            let line_color = BorderRole::Focused.resolve(colors);
            let thickness = 2.0_f32;
            let line_y = body_origin_y + y - thickness * 0.5;
            canvas.fill_rect(
                Rect::new(content_left, line_y, body_width_for_paint, thickness),
                line_color,
            );
        }

        canvas.clear_clip();

        // Container focus ring — the table holds keyboard focus but nothing
        // indicates where: no current cell (no cell ring) and no selection (no
        // band). Outline the whole view so Tab has a visible landing point
        // before the user navigates (mirrors TreeView / ListView).
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

        // `OnRelease` column-resize guide. Under that policy no column moves
        // until the button comes up, so this line is the *only* feedback the
        // gesture has — the same full-height rubber band Qt / Excel draw.
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
    /// A `TableView` is focusable and its rows deliberately are not — the
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
        // `Role::Grid` for a table the keyboard can drive, `Role::Table` for
        // one that is pure structure.
        //
        // Not a semantic nicety: `accesskit_consumer`'s
        // `is_container_with_selectable_children` — the sole gate on UIA's
        // `ISelectionProvider` — lists `Grid`, `ListBox`, `ListGrid`, `Tree`
        // and `TreeGrid`, and **not** `Table`. Announced as a table, a
        // multi-select `TableView` exposed no `CanSelectMultiple` and no
        // `GetSelection` to Narrator, NVDA or JAWS at all. The two roles map
        // identically on macOS (`NSAccessibilityTableRole`) and AT-SPI
        // (`AtspiRole::Table`), so this is a Windows fix that costs nothing
        // elsewhere.
        //
        // `Role::ListGrid` looks like the closer match — its own doc says it
        // exists for Chromium's `TableView` — but `accesskit_macos` maps it to
        // `NSAccessibilityUnknownRole`, which is worse than either.
        builder.set_role(if self.selection_mode == TableSelectionMode::None {
            teksilo_core::accesskit::Role::Table
        } else {
            teksilo_core::accesskit::Role::Grid
        });
        if let Some(ref label) = self.a11y_label {
            builder.set_name(label.resolve_now());
        }
        // AccessKit's `row_count` includes the header row when present —
        // matches ARIA `aria-rowcount` semantics.
        let row_count = (self.len_fn)() + if self.show_header { 1 } else { 0 };
        let col_count = self.columns.len();
        let n = builder.inner_mut();
        n.set_row_count(row_count);
        n.set_column_count(col_count);

        // Roving focus: point active_descendant at the focused cell's own
        // AT node so a screen reader follows arrow-key cell navigation
        // (only the table root is otherwise focusable — the ring is
        // visual-only). `cell_map` is a snapshot of the body pane's last
        // realized cells; a focused cell that scrolled out of the
        // realized buffer simply isn't in it, so no stale id is emitted.
        if let Some(target) = self.focused_cell.get() {
            let map = self.cell_map.borrow();
            if let Some(&(_, cell_id)) = map.iter().find(|&&(pos, _)| pos == target) {
                builder.set_active_descendant(widget_id_to_node_id(cell_id));
            }
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        // Same order as `build()` — body pane first, header last so
        // it paints on top of any overscrolled rows.
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
