// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`TreeView`]: build,
//! layout, placement, paint, and accessibility.

use super::*;

impl<T: 'static> Widget for TreeView<T> {
    fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        // --- Version signal for rebuild triggering ---
        // A persistent field (not `ctx.signal`) so the realization
        // re-check in `place_children` can bump it after measurement.
        let version = self.version.clone();
        version.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (repositions items) without a full rebuild.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        ctx.register_animated_signal(&self.scroll_y);

        // Bind drop_feedback at RepaintOnly so `set(...)` calls from
        // on_drag_hover / on_drag_leave dirty the TreeView's paint cache
        // without triggering a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Focus signals for the container ring. `begin_view_focus` keys the
        // scope signal on this root id directly (independent of the arena
        // focusable flag, not yet wired here): a plain `view_focus_active()`
        // would find no focusable ancestor and fall back to the constant-`true`
        // "outside any scope" signal — lighting the ring whenever ANY other
        // widget takes keyboard focus. Pop straight back; the real row scope
        // below resolves the same cached signal. `focus_visible` is the
        // keyboard/pointer modality. Bound `RepaintOnly` so focus-in/out
        // redraws the ring. (Selection-emptiness changes already rebuild via
        // `version`, so paint re-reads the selection without extra binding.)
        self.view_focused = ctx.begin_view_focus();
        ctx.end_view_focus();
        self.focus_visible = ctx.focus_visible();
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

        // --- Observe source version (covers both data mutations and expand/collapse) ---
        let source_version = self.source.version_signal();
        let version_for_data = version.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&source_version, {
            let dv = data_ver.clone();
            let ver = version_for_data.clone();
            let metrics = self.metrics.clone();
            let source = self.source.clone();
            let row_sel = self.row_selection.clone();
            let focused = self.focused_index.clone();
            let focused_anchor = self.focused_anchor.clone();
            move |_| {
                // Source version observers fire synchronously per reflatten, so
                // `first_changed_index()` describes exactly this change:
                // heights of flat rows before it (e.g. above an
                // expand/collapse point) stay valid.
                metrics
                    .borrow_mut()
                    .apply_divergence(source.first_changed_index(), source.visible_count());
                // Drop any keyed selection whose node was deleted (no-op for
                // the index model). A collapse does not delete, so a collapsed
                // node's selection survives.
                if let Some(ref rs) = row_sel {
                    rs.prune();
                    // Index-based selection has no identity to track by, so
                    // it cannot follow a moved row — but it must not keep
                    // pointing past the shrunk end either.
                    rs.prune_out_of_range(source.visible_count());
                }
                // The keyboard cursor: a version bump carries no `DataChange`
                // delta to shift it by (it covers expand/collapse too, which
                // has none), so it is tracked by identity instead. The anchor
                // captured the last time `focused_index` moved is resolved
                // against the now-current source and the cursor rewritten to
                // wherever that row landed, or dropped if the row is gone —
                // the same dance `reconcile_editing_row` runs for
                // `TableView`'s `editing_cell`.
                // Snapshot-then-drop the borrow before the `None` arm below
                // takes it mutably — an `if let focused_anchor.borrow()...`
                // scrutinee keeps the immutable `Ref` alive for the whole
                // block (temporary lifetime extension), which would panic
                // on that `borrow_mut()`.
                let anchor_snapshot = focused_anchor.borrow().clone();
                if let Some(anchor) = anchor_snapshot {
                    match anchor.index() {
                        Some(idx) => {
                            if focused.get() != Some(idx) {
                                focused.set(Some(idx));
                            }
                        }
                        None => {
                            focused.set(None);
                            *focused_anchor.borrow_mut() = None;
                        }
                    }
                }
                let next = dv.get() + 1;
                dv.set(next);
                ver.set(next);
            }
        });

        // --- Observe selection changes (rebuild to update delegate's `selected` param) ---
        if let Some(ref rs) = self.row_selection {
            let version_for_sel = version.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                version_for_sel.set(next);
            });
            ctx.own_handle(handle);
        }

        // --- Observe scroll position changes (rebuild only when items leave/enter buffer) ---
        let viewport_h = self.viewport_height.clone();
        // Track the buffered range from this build. Only trigger a rebuild
        // when the visible range exceeds the buffer — most scrolls just need
        // a relayout (handled by scroll_y's Relayout binding above).
        let (built_start, built_end) = self.visible_range();
        self.prev_built_start.set(built_start);
        self.prev_built_end.set(built_end);
        let version_for_scroll = version.clone();
        let scroll_ver = Rc::new(Cell::new(0_u64));
        let scroll_handle = self.scroll_y.observe({
            let pbs = self.prev_built_start.clone();
            let pbe = self.prev_built_end.clone();
            let sv = scroll_ver.clone();
            let metrics = self.metrics.clone();
            let source = self.source.clone();
            move |y| {
                let count = source.visible_count();
                let (visible_start, visible_end) =
                    metrics
                        .borrow_mut()
                        .visible_range(*y, viewport_h.get(), count, 0);
                // Only rebuild when visible items fall outside the currently-built range
                if visible_start < pbs.get() || visible_end > pbe.get() {
                    let new_start = visible_start.saturating_sub(BUFFER_ITEMS);
                    // Clamp to `count` — build() realizes a `min(end, count)`
                    // window, so an unclamped `pbe` past the end leaves the
                    // dirty-check believing rows are built that never were,
                    // so the bottom rows of a large tree never realize on a
                    // fast scroll. Mirrors TableView's BodyPane.
                    let new_end = (visible_end + BUFFER_ITEMS).min(count);
                    pbs.set(new_start);
                    pbe.set(new_end);
                    let next = sv.get() + 1;
                    sv.set(next);
                    version_for_scroll.set(next);
                }
            }
        });
        ctx.own_handle(scroll_handle);

        // --- Scroll event handler + DnD ---
        let scroll_y = self.scroll_y.clone();
        let max_scroll = self.max_scroll_y.clone();
        let line_height = self.item_height;
        let overscroll_behavior = self.overscroll_behavior;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;
        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                bastyde_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        bastyde_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        bastyde_core::event::ScrollDelta::Pixels { y, .. } => *y,
                    };
                    let current = scroll_y.get();
                    let max = max_scroll.get();
                    // Base off the animation target (not the rendered offset)
                    // so a mid-fling boundary correctly chains and successive
                    // notches accumulate instead of restarting from the
                    // partway-animated position.
                    let base = scroll_y.animation_target().unwrap_or(current);
                    let (new_y, moved) = crate::common::scroll::scroll_clamp_axis(base, dy, max);
                    if moved {
                        if smooth_scrolling {
                            scroll_y.animate_to(new_y, smooth_scroll_duration, Easing::EaseOut);
                        } else {
                            scroll_y.set(new_y);
                        }
                    }
                    // Chain to an ancestor scrollable when fully clamped
                    // (unless Contain), otherwise consume.
                    crate::common::scroll::scroll_response(
                        moved,
                        overscroll_behavior == OverscrollBehavior::Contain,
                    )
                }
                _ => bastyde_core::event::EventResponse::Ignored,
            })
            .clips_children(true)
            .focusable(true);

        // --- Keyboard navigation + expand/collapse + Alt+Arrow reorder ---
        {
            let source = self.source.clone();
            let sel_for_key = self.row_selection.clone();
            let activate_key = self.on_activate.clone();
            let fi = self.focused_index.clone();
            let fi_anchor = self.focused_anchor.clone();
            let reorderable = self.reorderable;
            let scroll_for_nav = self.scroll_y.clone();
            let metrics_for_nav = self.metrics.clone();
            let max_for_nav = self.max_scroll_y.clone();
            let vh_for_nav = self.viewport_height.clone();
            let vb_for_nav = self.viewport_bounds.clone();
            let ta_state = self.type_ahead.clone();
            let ta_label = self.type_ahead_label.clone();
            let ta_timeout = self.type_ahead_timeout;

            handlers = handlers.on_key(move |event, ctx| {
                if let bastyde_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    use bastyde_core::event::Key;
                    let visible_count = source.visible_count();
                    if visible_count == 0 {
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // The keyboard cursor: `focused_index` once the user has
                    // navigated or clicked, else the current selection (a tree
                    // can be handed a selected row before it is ever focused).
                    // `None` = "no cursor yet", which is NOT "cursor on row 0" —
                    // see the arrow keys below.
                    let cursor = fi
                        .get()
                        .or_else(|| {
                            sel_for_key
                                .as_ref()
                                .and_then(|s| s.selected_indices().first().copied())
                        })
                        .map(|i| i.min(visible_count - 1));
                    // Anchor for the keys that compute *from* a row (expand /
                    // collapse / paging / activation) rather than step in a
                    // direction.
                    let current = cursor.unwrap_or(0);

                    // Move the keyboard cursor AND refresh the `RowAnchor` it
                    // resolves through on the next structural change — every
                    // site below that moves `fi` must go through this, or the
                    // cursor silently stops following its row (see the
                    // `source_version` effect in `build`).
                    let set_focus = |idx: usize| {
                        fi.set(Some(idx));
                        *fi_anchor.borrow_mut() = Some(source.anchor(idx));
                    };

                    // Helper: scroll so flat row `idx` is visible in the tree's
                    // OWN viewport; returns the resulting scroll offset so the
                    // caller can chain the reveal to enclosing scroll areas.
                    let ensure_visible = |idx: usize| -> f32 {
                        let scroll = scroll_for_nav.get();
                        let new_scroll = metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                            idx,
                            scroll,
                            vh_for_nav.get(),
                            max_for_nav.get(),
                        );
                        if (new_scroll - scroll).abs() > f32::EPSILON {
                            scroll_for_nav.set(new_scroll);
                        }
                        new_scroll
                    };

                    // Ctrl+A: select all visible rows (Multi only).
                    if modifiers.ctrl() && matches!(key, Key::A) {
                        if let Some(ref sel) = sel_for_key
                            && sel.mode() == bastyde_data::SelectionMode::Multi
                        {
                            sel.select_all(visible_count);
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // Type-ahead: a printable char (no Ctrl/Alt/Super) jumps the
                    // selection to the next visible row whose label starts with
                    // the accumulated term. Opt-in via `type_ahead_label`.
                    if ta_label.is_some()
                        && !modifiers.ctrl()
                        && !modifiers.alt()
                        && !modifiers.super_key()
                        && let Some(c) = key.to_char()
                    {
                        let label = ta_label.as_ref().unwrap();
                        let source_ref = &source;
                        if let Some(idx) =
                            ta_state.search(c, current, visible_count, ta_timeout, |i| {
                                source_ref.with_row_str(i, &|item| label(item))
                            })
                        {
                            set_focus(idx);
                            if let Some(ref sel) = sel_for_key {
                                sel.select(idx);
                            }
                            let new_scroll = ensure_visible(idx);
                            crate::common::row_metrics::chase_row_into_outer_view(
                                ctx,
                                &metrics_for_nav,
                                vb_for_nav.get(),
                                idx,
                                new_scroll,
                            );
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // Alt+Arrow: sibling reorder (when reorderable). Routed
                    // through the source's own `accept_drop` (cycle-guarded),
                    // which returns the moved row's new flat index.
                    if modifiers.alt() && reorderable {
                        let flat_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied())
                            .or(fi.get())
                            .unwrap_or(current);
                        let down = match key {
                            bastyde_core::event::Key::ArrowUp => false,
                            bastyde_core::event::Key::ArrowDown => true,
                            _ => return bastyde_core::event::EventResponse::Ignored,
                        };
                        if let Some(new_flat) = source.keyboard_reorder(flat_idx, down) {
                            set_focus(new_flat);
                            if let Some(ref sel) = sel_for_key {
                                sel.select(new_flat);
                            }
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        return bastyde_core::event::EventResponse::Ignored;
                    }

                    // ArrowRight: expand / ArrowLeft: collapse or move to parent
                    match key {
                        bastyde_core::event::Key::ArrowRight => {
                            if let Some(meta) = source.meta(current)
                                && meta.has_children
                                && !meta.is_expanded
                            {
                                source.set_expanded_at(current, true);
                                return bastyde_core::event::EventResponse::Handled;
                            }
                        }
                        bastyde_core::event::Key::ArrowLeft => {
                            if let Some(meta) = source.meta(current) {
                                if meta.is_expanded {
                                    source.set_expanded_at(current, false);
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                                // If leaf or collapsed, move to parent.
                                if let Some(parent_idx) = source.parent_index(current) {
                                    set_focus(parent_idx);
                                    if let Some(ref sel) = sel_for_key {
                                        sel.select(parent_idx);
                                    }
                                    // Reveal the parent row (own viewport, then
                                    // any enclosing scroll area) like every
                                    // other focus-moving key.
                                    let new_scroll = ensure_visible(parent_idx);
                                    crate::common::row_metrics::chase_row_into_outer_view(
                                        ctx,
                                        &metrics_for_nav,
                                        vb_for_nav.get(),
                                        parent_idx,
                                        new_scroll,
                                    );
                                    return bastyde_core::event::EventResponse::Handled;
                                }
                            }
                        }
                        _ => {}
                    }

                    // Navigation keys. With no cursor yet, the first Down lands ON
                    // the first row and the first Up on the last one — stepping
                    // to row 1 would silently skip the row the user is looking at
                    // (see `ListView`, same rule).
                    let new_idx = match key {
                        Key::ArrowDown => Some(match cursor {
                            None => 0,
                            Some(c) => (c + 1).min(visible_count - 1),
                        }),
                        Key::ArrowUp => Some(match cursor {
                            None => visible_count - 1,
                            Some(c) => c.saturating_sub(1),
                        }),
                        Key::Home => Some(0),
                        Key::End => Some(visible_count - 1),
                        // Page keys: jump one viewport of rows by visual distance
                        // (variable heights honored), then ensure-visible scrolls.
                        Key::PageDown => {
                            let vh = vh_for_nav.get();
                            let r = {
                                let mut m = metrics_for_nav.borrow_mut();
                                m.resize(visible_count);
                                let target = m.row_top(current) + vh;
                                m.row_at(target)
                            };
                            Some(if r == current {
                                (current + 1).min(visible_count - 1)
                            } else {
                                r.min(visible_count - 1)
                            })
                        }
                        Key::PageUp => {
                            let vh = vh_for_nav.get();
                            let r = {
                                let mut m = metrics_for_nav.borrow_mut();
                                m.resize(visible_count);
                                let target = (m.row_top(current) - vh).max(0.0);
                                m.row_at(target)
                            };
                            Some(if r == current {
                                current.saturating_sub(1)
                            } else {
                                r
                            })
                        }
                        Key::Enter => {
                            // Enter activates the focused row (open / commit).
                            if let Some(ref sel) = sel_for_key {
                                sel.select(current);
                            }
                            if let Some(ref cb) = activate_key {
                                cb(current);
                            }
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        Key::Space => {
                            // Space moves/toggles the selection but does NOT
                            // activate (Enter is the activator). Multi: toggle;
                            // Single: select.
                            if let Some(ref sel) = sel_for_key {
                                if sel.mode() == bastyde_data::SelectionMode::Multi {
                                    sel.toggle(current);
                                } else {
                                    sel.select(current);
                                }
                            }
                            set_focus(current);
                            return bastyde_core::event::EventResponse::Handled;
                        }
                        _ => None,
                    };

                    if let Some(idx) = new_idx {
                        set_focus(idx);
                        if let Some(ref sel) = sel_for_key {
                            if modifiers.shift() {
                                sel.extend_to(idx);
                            } else {
                                sel.select(idx);
                            }
                        }
                        let new_scroll = ensure_visible(idx);
                        crate::common::row_metrics::chase_row_into_outer_view(
                            ctx,
                            &metrics_for_nav,
                            vb_for_nav.get(),
                            idx,
                            new_scroll,
                        );
                        return bastyde_core::event::EventResponse::Handled;
                    }
                }
                bastyde_core::event::EventResponse::Ignored
            });
        }

        // --- DnD: register as drop target when reorderable OR accept foreign
        // rows. The source's `can_accept` decides per-hover whether the drop is
        // allowed (and a forbidden verdict shows no insertion line / highlight);
        // a foreign exported row that the source itself rejects can still be
        // accepted via the `accept_foreign_rows` sugar (shown as a plain
        // between-rows insertion — a foreign source has no Into/reparent
        // semantics). ---
        if self.export.is_drop_target(self.reorderable) {
            let my_view_id = self.tree_id;

            // Shared across hover / tick / leave: the visible row index under the
            // pointer + when first seen, for spring-loaded folder expansion.
            // Reset whenever the hovered row changes or the drag leaves.
            let hovered_row: Rc<Cell<Option<(usize, std::time::Instant)>>> =
                Rc::new(Cell::new(None));

            // ----- hover: geometry → (target, position) → source.can_accept -----
            let metrics_for_hover = self.metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let source_for_hover = self.source.clone();
            let feedback_for_hover = self.drop_feedback.clone();
            let width_for_hover = self.placed_content_width.clone();
            let hr_for_hover = hovered_row.clone();
            let export_for_hover = self.export.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                let line_width = width_for_hover.get();
                let vc = source_for_hover.visible_count();
                if vc == 0 {
                    feedback_for_hover.set(None);
                    hr_for_hover.set(None);
                    return DropFeedback::NoFeedback;
                }
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y + scroll;
                let (insertion_top, row_idx, row_top, row_h) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(vc);
                    let ins = m.insertion_index(content_y);
                    let r = m.row_at(content_y);
                    let insertion_top = m.row_top(ins);
                    let row_top = m.row_top(r);
                    let row_h = m.row_height(r);
                    (insertion_top, r, row_top, row_h)
                };
                // Spring-load tracking (dwell-to-expand the hovered branch).
                match hr_for_hover.get() {
                    Some((p, t)) if p == row_idx => hr_for_hover.set(Some((row_idx, t))),
                    _ => hr_for_hover.set(Some((row_idx, std::time::Instant::now()))),
                }
                // Drop position from Y within the row (top third Before / middle
                // Into / bottom After). The source's `can_accept` is the verdict
                // — a Reject shows NO line (the pre-commit forbidden affordance).
                let y_in_row = content_y - row_top;
                let third = (row_h / 3.0).max(f32::EPSILON);
                let drop_pos = if y_in_row < third {
                    DropPosition::Before
                } else if y_in_row > 2.0 * third {
                    DropPosition::After
                } else {
                    DropPosition::Into
                };
                // The source's verdict decides the *effective* position: a
                // `Redirect` (e.g. Into-a-leaf → After) overrides the raw zone.
                let effective = match (source_for_hover.dnd.can_accept_fn)(
                    payload, row_idx, drop_pos, my_view_id,
                ) {
                    DropResponse::Reject => {
                        // The source itself won't take this drop — fall back to
                        // the foreign-export sugar, shown as a plain between-rows
                        // insertion (a foreign source has no Into/reparent
                        // semantics to honor).
                        let foreign_ok =
                            export_for_hover.accepts_foreign_export(payload, my_view_id);
                        if !foreign_ok {
                            feedback_for_hover.set(None);
                            return DropFeedback::NoFeedback;
                        }
                        DropPosition::Before
                    }
                    DropResponse::Accept => drop_pos,
                    DropResponse::Redirect(p) => p,
                };
                if effective == DropPosition::Into {
                    // Drop *into* the hovered container → highlight its whole row.
                    let top = row_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Rect {
                        top,
                        height: row_h,
                        width: line_width,
                    }));
                    DropFeedback::HighlightRect {
                        rect: Rect::new(0.0, top, line_width, row_h),
                        color: bastyde_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25),
                    }
                } else {
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Line {
                        y: insertion_y,
                        width: line_width,
                    }));
                    DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: line_width,
                    }
                }
            });

            // ----- drop: re-derive (target, position), route to accept_drop -----
            let metrics_for_drop = self.metrics.clone();
            let scroll_for_drop = self.scroll_y.clone();
            let source_for_drop = self.source.clone();
            let feedback_for_drop = self.drop_feedback.clone();
            let export_for_drop = self.export.clone();
            let reorderable_for_drop = self.reorderable;
            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                feedback_for_drop.set(None);
                let vc = source_for_drop.visible_count();
                if vc == 0 {
                    return false;
                }
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y + scroll;
                let (row_idx, row_top, row_h, ins) = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(vc);
                    let r = m.row_at(content_y);
                    let ins = m.insertion_index(content_y);
                    (r, m.row_top(r), m.row_height(r), ins)
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
                    .is_some_and(|rd| rd.source == my_view_id);
                // Route the drop to the source's accept_drop first. A same-view
                // reorder/reparent only happens when the view is `reorderable`;
                // a foreign payload the source itself recognises is the
                // source's call.
                if (reorderable_for_drop || !is_same_view)
                    && (source_for_drop.dnd.accept_drop_fn)(&payload, row_idx, drop_pos, my_view_id)
                {
                    // Only suppress our OWN move-out for a genuine same-view drop.
                    if is_same_view {
                        export_for_drop.note_self_reorder();
                    }
                    return true;
                }
                // Otherwise, the shared foreign-receive sugar (peek-before-take):
                // accept exported rows from a different view/source without a
                // custom TreeDataSource, at the flat insertion index.
                export_for_drop.foreign_receive(&mut payload, my_view_id, ins, ctx)
            });

            // Clear insertion line + spring-load timer whenever the drag leaves.
            let feedback_for_leave = self.drop_feedback.clone();
            let hr_for_leave = hovered_row.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
                hr_for_leave.set(None);
            });

            // Per-frame tick: viewport-edge auto-scroll plus spring-loaded
            // folders. The tick fires regardless of pointer movement, so
            // edge-scroll and spring-open still progress when the hand is
            // stationary.
            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            let hr_for_tick = hovered_row.clone();
            let source_for_tick = self.source.clone();
            const SPRING_DELAY_MS: u64 = 700;
            handlers = handlers.on_drag_tick(move |pos, _ctx| {
                // --- 1. Edge auto-scroll ---
                const EDGE: f32 = 32.0;
                const MAX_VELOCITY: f32 = 12.0;
                let h = viewport_for_tick.get();
                let above = (EDGE - pos.y).max(0.0);
                let below = (pos.y - (h - EDGE)).max(0.0);
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

                // --- 2. Spring-loaded folders ---
                if let Some((row_idx, first_seen)) = hr_for_tick.get() {
                    let elapsed_ms = first_seen.elapsed().as_millis() as u64;
                    let has_children = source_for_tick
                        .meta(row_idx)
                        .map(|m| m.has_children)
                        .unwrap_or(false);
                    if elapsed_ms >= SPRING_DELAY_MS
                        && has_children
                        && !source_for_tick.is_expanded_at(row_idx)
                    {
                        source_for_tick.set_expanded_at(row_idx, true);
                        // Reset so we don't keep re-firing on the same row.
                        hr_for_tick.set(None);
                    }
                }
            });
        }

        // --- Export completion: remove rows moved out to a FOREIGN target. The
        // handler fires on the drag source (this view's root id, the stable id
        // start_drag was given). A same-view reorder called
        // `export.note_self_reorder()`, so it is skipped here (already applied).
        //
        // FIXED (was a known limitation): move-out no longer resolves the
        // dragged rows from flat indices at completion time. `build_payload`
        // captures a stable-key removal thunk via `source.dnd.snapshot_out_fn`
        // at drag-start, so a Move that dwelled over a collapsing/expanding
        // folder mid-drag (spring-load auto-expand reshuffling flat indices)
        // still removes the correct node.
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // --- Create visible item widgets ---
        let (start, end) = self.visible_range();
        self.item_entries.clear();
        // Lazy: nudge the source to load the realized window, and fetch more
        // as the viewport nears the end (append-only sources).
        (self.source.dnd.request_window_fn)(start..end);
        if (self.source.dnd.can_fetch_more_fn)()
            && end + BUFFER_ITEMS >= self.source.visible_count()
        {
            (self.source.dnd.fetch_more_fn)();
        }
        let is_drag_source = self.export.is_drag_source(self.reorderable);
        let tree_id = self.tree_id;
        let self_id = ctx.self_id();
        let row_state_fn = self.source.dnd.row_state_fn.clone();
        // Establish this TreeView as the focus scope for the rows it builds, so
        // their `StandardItem`s read *its* keyboard focus deterministically
        // (rows may build before arena parenting is wired).
        ctx.begin_view_focus();
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
            let row_widget = self
                .source
                .with_row(i, &|item, m| (self.row_delegate)(i, item, m, selected))
                .or_else(|| {
                    ((row_state_fn)(i) == RowState::Loading)
                        .then(crate::data_views::default_placeholder)
                });
            if let Some(widget) = row_widget {
                let inner_id = ctx.add_boxed(widget);
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
                            bastyde_core::event::WidgetEvent::PointerDown {
                                modifiers,
                                button: bastyde_core::event::PointerButton::Primary,
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
                                    return bastyde_core::event::EventResponse::Ignored;
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
                                    // arrow-nav origin (`fi.get().unwrap_or(0)`)
                                    // and is otherwise only written by the
                                    // keyboard handler, so without this a click
                                    // would select a row yet leave arrows
                                    // stepping from the stale keyboard cursor.
                                    // Refresh the anchor alongside it — see
                                    // `set_focus` in the keyboard handler.
                                    fi_click.set(Some(click_index));
                                    *fi_anchor_click.borrow_mut() =
                                        Some(source_click.anchor(click_index));
                                }
                                // Ignored lets the gesture arena also see the
                                // PointerDown so DragRecognizer can capture the
                                // press position and enable drag-to-reorder.
                                bastyde_core::event::EventResponse::Ignored
                            }
                            bastyde_core::event::WidgetEvent::PointerUp {
                                button: bastyde_core::event::PointerButton::Primary,
                                ..
                            } => {
                                // A release on the chevron (or another interactive
                                // child) is handled by that child's own tap — don't
                                // also toggle from the row body.
                                if ctx.press_claimed_by_interactive_child() {
                                    return bastyde_core::event::EventResponse::Ignored;
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
                                bastyde_core::event::EventResponse::Ignored
                            }
                            _ => bastyde_core::event::EventResponse::Ignored,
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
                                HandlerSet::new().on_tap(move |tap, _ctx| {
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
                                        cb(cur)
                                    }
                                })
                            }
                            crate::data_views::ActivateOn::DoubleClick => HandlerSet::new()
                                .on_double_tap(move |_tap, _ctx| {
                                    if let Some(cur) = a.index() {
                                        cb(cur)
                                    }
                                }),
                        };
                        ctx.apply_handlers(child_id, handlers);
                    }
                }

                // Drag handler when reorderable OR exportable, gated by the
                // source's transferable verdict (`drag`). Emits the public
                // `RowDragData<T>`; the source recovers the key + validates at
                // hover/drop. The floating preview re-invokes the row delegate.
                if is_drag_source && (self.source.dnd.drag_fn)(i) == DragEligibility::CanDrag {
                    let drag_view_id = tree_id;
                    let drag_self_id = self_id;
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
                            if let bastyde_core::gesture::DragPhase::Started { .. } = phase {
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

        // --- Scrollbar ---
        let scrollbar = ScrollBar::new(
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
        let sb_id = ctx.add(scrollbar);
        self.scrollbar_id = Some(sb_id);

        let mut children: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        children.push(sb_id);
        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(300.0);
        let height = proposal.height.unwrap_or(200.0);
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
        // Cache our own absolute bounds for the keyboard handler's
        // outer-scroll chase (`ensure_visible`), before the empty-children bail.
        self.viewport_bounds.set(bounds);

        if children.is_empty() {
            return;
        }

        let viewport_height = bounds.height;
        let count = self.source.visible_count();
        let item_count = self.item_entries.len();
        // Permanent reserves a column for the bar; Overlay / Thin float
        // over the content, so rows span the full width.
        let reserves_bar = self.scroll_bar_style == ScrollBarMode::Permanent;
        let content_width = if reserves_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        self.placed_content_width.set(content_width);

        // Auto-measure pass: measure every realized row at the content
        // width (height-for-width), feed the heights back, and apply the
        // scroll-anchor delta so content above the viewport stays put.
        // Measurements are collected with NO metrics borrow held.
        if self.metrics.borrow().needs_measure() {
            let mut measured = Vec::with_capacity(item_count);
            for (idx, child) in children.iter().enumerate() {
                if idx < item_count
                    && let Some(size) =
                        ctx.child_size(child.id, SizeProposal::with_width(content_width))
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
            // rows the estimated offsets never realized. Request a
            // rebuild for next frame; the 0.01 measurement epsilon
            // guarantees convergence.
            let (vs, ve) = self.metrics.borrow_mut().visible_range(
                self.scroll_y.get(),
                viewport_height,
                count,
                0,
            );
            if vs < self.prev_built_start.get() || ve > self.prev_built_end.get() {
                self.prev_built_start.set(vs.saturating_sub(BUFFER_ITEMS));
                self.prev_built_end.set((ve + BUFFER_ITEMS).min(count));
                self.version.set(self.version.get() + 1);
            }
        }

        // Post-measure totals so even frame 1's scrollbar reflects the
        // measured window.
        let total_height = self.total_content_height();
        let max_y = (total_height - viewport_height).max(0.0);
        self.max_scroll_y.set(max_y);
        let ratio = if total_height > 0.0 {
            (viewport_height / total_height).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.viewport_ratio_y.set(ratio);
        self.clamp_scroll();

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
                child.size = Size::new(content_width, height);
            }
        }

        // Scrollbar
        if let Some(sb_child) = children.last_mut() {
            let needs_scrollbar = total_height > viewport_height + 0.5;
            if needs_scrollbar {
                sb_child.origin =
                    Point::new(bounds.x + bounds.width - SCROLLBAR_THICKNESS, bounds.y);
                sb_child.size = Size::new(SCROLLBAR_THICKNESS, bounds.height);
            } else {
                sb_child.origin = bounds.origin();
                sb_child.size = Size::ZERO;
            }
        }
    }

    fn paint(
        &self,
        bounds: Rect,
        canvas: &mut bastyde_canvas::Canvas,
        ctx: &bastyde_core::widget::PaintContext,
    ) {
        // Draw insertion line during drag hover — recipe-driven role +
        // thickness via `ListContainerStyle::insertion()`.
        if let Some(viz) = self.drop_feedback.get() {
            let recipe = ctx
                .theme
                .style_slots
                .list_container
                .as_ref()
                .map(|s| s.insertion())
                .unwrap_or_default();
            let color = recipe.role.resolve(&ctx.theme.colors);
            // Own paint isn't covered by `clips_children` — clip so feedback at
            // the after-last boundary can't bleed past the widget's bottom edge.
            canvas.set_clip(bounds);
            match viz {
                DropViz::Line { y, width } => {
                    let line_y = bounds.y + y;
                    let half = recipe.thickness * 0.5;
                    canvas.fill_rect(
                        Rect::new(bounds.x, line_y - half, width, recipe.thickness),
                        color,
                    );
                }
                DropViz::Rect { top, height, width } => {
                    // Into-container highlight: a translucent fill plus a solid
                    // outline at the insertion role's color.
                    let rect = Rect::new(bounds.x, bounds.y + top, width, height);
                    canvas.fill_rect(rect, color.with_alpha(0.18));
                    canvas.stroke_rect(rect, color, recipe.thickness.max(1.5));
                }
            }
            canvas.clear_clip();
        }

        // Container focus ring. When the view is Tab-focused (keyboard modality)
        // but nothing is selected, no row paints a ring — so outline the whole
        // view, giving the user a visible focus landing point before they arrow.
        // Once a row is selected its own ring takes over and this clears.
        let has_selection = self
            .row_selection
            .as_ref()
            .is_some_and(|s| s.has_selection());
        if self.view_focused.get() && self.focus_visible.get() && !has_selection {
            let color = BorderRole::Focused.resolve(&ctx.theme.colors);
            let inset = 1.0_f32;
            let rect = Rect::new(
                bounds.x + inset,
                bounds.y + inset,
                (bounds.width - inset * 2.0).max(0.0),
                (bounds.height - inset * 2.0).max(0.0),
            );
            canvas.stroke_rect(rect, color, 1.5);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Tree);
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids: Vec<WidgetId> = self.item_entries.iter().map(|(_, id)| *id).collect();
        if let Some(sb) = self.scrollbar_id {
            ids.push(sb);
        }
        ids
    }

    fn clips_children(&self) -> bool {
        true
    }
}
