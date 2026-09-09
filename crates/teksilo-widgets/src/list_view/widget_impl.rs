// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`ListView`]: build (body pane
//! and scrollbar assembly, row realization, pointer and drag wiring), layout,
//! placement, paint and accessibility.

use super::*;
impl<T: 'static> Widget for ListView<T> {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        // The root builds exactly two children — the body pane and the
        // scrollbar — and neither depends on the data, the selection or the
        // scroll offset. So it declares no `Rebuild`-level binding at all:
        // row realization is the pane's job (see `body_pane`'s module docs
        // for why that separation is load-bearing and not just tidy), and
        // what the root still owns resolves at `Relayout` / `RepaintOnly`.
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        // Scrollbar totals + the content-width decision live in the root's
        // `place_children`; a data change or a pane measurement that moves
        // the content total re-places the root through this.
        self.layout_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        // Container focus ring: painted only while nothing is selected, so a
        // selection change has to reach the root's paint — without rebuilding
        // it and taking the scrollbar down with it.
        self.paint_refresh.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Bind scroll_y at Relayout so place_children runs on every scroll
        // position change (re-clamps and refreshes the thumb) without a
        // rebuild. The pane holds the matching binding for its rows.
        self.scroll_y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Register animated signal for smooth scrolling. Deliberately the
        // ROOT and only the root: the scheduler keys an animation to the
        // widget that registered its signal last and cancels it when that
        // widget rebuilds, so registering from the pane too would make every
        // buffer-exit rebuild abort an in-flight fling.
        ctx.register_animated_signal(&self.scroll_y);

        // Bind drop_feedback at RepaintOnly so `set(...)` calls from
        // on_drag_hover / on_drag_leave dirty the ListView's paint cache
        // without triggering a rebuild.
        self.drop_feedback.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );

        // Focus signals for the container ring (see TreeView). `RepaintOnly` so
        // focus-in/out redraws; selection-emptiness changes arrive on
        // `paint_refresh`. `begin_view_focus` keys the scope signal on this root id directly,
        // independent of the arena focusable flag (not yet wired at this point):
        // a plain `view_focus_active()` would `find_focusable_at_or_above`
        // nothing and fall back to the constant-`true` "outside any scope"
        // signal — lighting the ring whenever ANY other widget takes keyboard
        // focus. Pop straight back; the real row scope below resolves the same
        // cached signal.
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

        // --- Observe model changes ---
        // One observer, root-owned, doing the bookkeeping the pane can't
        // (metrics divergence, selection shift, keyboard cursor) and then
        // fanning out: rebuild the pane (row content changed) and re-place
        // the root (the content total, hence the thumb, changed).
        let pane_version_for_data = self.pane_version.clone();
        let layout_refresh_for_data = self.layout_refresh.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        let data_handle = (self.source.observe_fn)(Box::new({
            let dv = data_ver.clone();
            let metrics = self.metrics.clone();
            let len_fn = self.source.len_fn.clone();
            let first_changed = self.source.first_changed_fn.clone();
            let row_sel = self.row_selection.clone();
            let focused = self.focused_index.clone();
            move |change| {
                // Keep row metrics in step with the data: rows before
                // the first changed index keep their (seeded or
                // measured) heights, the rest re-derive.
                let divergence = match change {
                    DataChange::ItemsInserted { range } | DataChange::ItemsRemoved { range } => {
                        Some(range.start)
                    }
                    DataChange::ItemUpdated { index } => Some(*index),
                    DataChange::ItemsMoved { from, to, .. } => Some((*from).min(*to)),
                    // A lazy window load makes rows from range.start onward differ.
                    DataChange::WindowLoaded { range } => Some(range.start),
                    // Reset-emitting proxies (SortFilterListModel) expose
                    // their real divergence through the side-channel.
                    DataChange::Reset => (first_changed)(),
                };
                metrics
                    .borrow_mut()
                    .apply_divergence(divergence, (len_fn)());
                // Keep selection in step: index-shift (index model) or prune
                // orphaned keys (keyed model).
                if let Some(ref rs) = row_sel {
                    rs.on_data_change(change);
                }
                // Keep the keyboard-navigation anchor in step too — otherwise
                // it silently points at the wrong row after any insert /
                // remove / move (reachable not just from local edits but
                // from a live watcher pushing in a peer process's write).
                if let Some(current) = focused.get() {
                    focused.set(teksilo_data::data_change::adjust_single_index_for_change(
                        current, change,
                    ));
                }
                let next = dv.get() + 1;
                dv.set(next);
                pane_version_for_data.set(next);
                layout_refresh_for_data.set(next);
            }
        }));
        ctx.own_handle(data_handle);

        // --- Observe selection changes ---
        // The pane runs its own selection observer for the delegate's
        // `selected` argument; the root only needs its container focus ring
        // repainted, since that ring is suppressed once anything is selected.
        if let Some(ref rs) = self.row_selection {
            let paint_refresh_for_sel = self.paint_refresh.clone();
            let sel_ver = Rc::new(Cell::new(0_u64));
            let handle = rs.observe_for_rebuild(move || {
                let next = sel_ver.get() + 1;
                sel_ver.set(next);
                paint_refresh_for_sel.set(next);
            });
            ctx.own_handle(handle);
        }

        // Scroll-buffer exit is deliberately NOT observed here. It rebuilds
        // the body pane and nothing else — the root's own children are
        // unaffected by which rows are realized, and a root rebuild during a
        // scrollbar thumb drag is exactly the one the framework defers.

        // --- Set up scroll event handler + DnD handlers on self ---
        // The wheel arithmetic, the pan and the claim that puts this node on a
        // finger's claimant chain all come from `common::scrollable`. A wheel
        // still takes the path it always did — `handle_scroll_event` branches
        // on the scroll *source*, not the phase.
        let mut handlers = HandlerSet::new().clips_children(true).focusable(true);
        {
            let behavior = crate::common::scrollable::ScrollableBehavior::new(
                crate::common::scrollable::ScrollableAxes::vertical(
                    self.scroll_y.clone(),
                    self.max_scroll_y.clone(),
                ),
            )
            .with_scroller(self.scroller.clone())
            // Vertical only: this view owns no horizontal offset, so a
            // horizontal pan is declined and chains outward.
            .axes(PanAxes::Y)
            .overscroll(self.overscroll_behavior)
            .smooth(self.smooth_scrolling)
            .smooth_duration(self.smooth_scroll_duration)
            .line_height(self.item_height)
            .reduced_motion(ctx.prefers_reduced_motion())
            .physics(ctx.theme().input.scroll_physics);
            handlers = behavior.install(handlers);
        }

        // --- Keyboard navigation + Alt+Arrow reorder ---
        {
            let len_for_key = self.source.len_fn.clone();
            let accept_drop_for_key = self.source.dnd.accept_drop_fn.clone();
            let stash_for_key = self.source.dnd.stash_drag_keys_fn.clone();
            let view_id_for_key = self.model_id;
            let sel_for_key = self.row_selection.clone();
            let activate_key = self.on_activate.clone();
            let fi = self.focused_index.clone();
            let reorderable = self.reorderable;
            let scroll_for_nav = self.scroll_y.clone();
            let metrics_for_nav = self.metrics.clone();
            let max_for_nav = self.max_scroll_y.clone();
            let vh_for_nav = self.viewport_height.clone();
            let vb_for_nav = self.viewport_bounds.clone();
            // Type-ahead state + label resolver (reads row text via the
            // source's string accessor, so lazy/unloaded rows are skipped).
            let ta_state = self.type_ahead.clone();
            // Index → realized row id, so `Space` can ask the row whether it
            // publishes a keyboard toggle (a checkbox) before falling back to
            // the selection.
            let row_map_for_key = self.row_map.clone();
            let ta_label = self.type_ahead_label.clone();
            let ta_timeout = self.type_ahead_timeout;
            let with_item_str = self.source.with_item_str_fn.clone();

            handlers = handlers.on_key(move |event, ctx| {
                if let teksilo_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    use teksilo_core::event::Key;
                    let count = (len_for_key)();
                    if count == 0 {
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // Select all — Ctrl+A, ⌘A on macOS (Multi selection only;
                    // a no-op for Single / None, matching every list control).
                    // With Shift it deselects instead: GTK is the only toolkit
                    // that *mandates* Ctrl+Shift+A, but the ARIA listbox and
                    // tree patterns both sanction an unselect-all, Windows and
                    // Qt simply have none, and adding it takes nothing away.
                    if modifiers.command() && matches!(key, Key::A) {
                        if let Some(ref sel) = sel_for_key
                            && sel.mode() == teksilo_data::SelectionMode::Multi
                        {
                            if modifiers.shift() {
                                sel.clear();
                            } else {
                                sel.select_all(count);
                            }
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // macOS reads a couple of chords in a list that the other
                    // desktops spend elsewhere, and both are dead here
                    // otherwise. ⌘↓ opens the row — Finder's "Command–Down
                    // Arrow: Open the selected item", and what VS Code binds as
                    // `list.select`'s macOS secondary. (⌘↑ ascends to the
                    // parent, which a flat list has none of; `TreeView` claims
                    // it.) Off macOS this resolves to `None` and costs nothing.
                    if let Some(alias) = list_nav::mac_alias(*key, *modifiers, ctx.is_rtl()) {
                        if alias == list_nav::MacAlias::Activate {
                            let row = fi
                                .get()
                                .or_else(|| {
                                    sel_for_key
                                        .as_ref()
                                        .and_then(|s| s.selected_indices().first().copied())
                                })
                                .unwrap_or(0)
                                .min(count - 1);
                            if let Some(ref sel) = sel_for_key {
                                sel.select(row);
                            }
                            if let Some(ref cb) = activate_key {
                                cb(row, ctx);
                            }
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // Type-ahead: a printable char (no Ctrl/Alt/Super) jumps the
                    // selection to the next row whose label starts with the
                    // accumulated term. Opt-in via `type_ahead_label`.
                    if ta_label.is_some()
                        && !modifiers.ctrl()
                        && !modifiers.alt()
                        && !modifiers.super_key()
                        && let Some(c) = key.to_char()
                    {
                        let current = fi.get().unwrap_or(0).min(count - 1);
                        let label = ta_label.as_ref().unwrap();
                        if let Some(idx) = ta_state.search(c, current, count, ta_timeout, |i| {
                            (with_item_str)(i, &|item| label(item))
                        }) {
                            fi.set(Some(idx));
                            if let Some(ref sel) = sel_for_key {
                                sel.select(idx);
                            }
                            let scroll = scroll_for_nav.get();
                            let new_scroll =
                                metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                                    idx,
                                    scroll,
                                    vh_for_nav.get(),
                                    max_for_nav.get(),
                                );
                            if (new_scroll - scroll).abs() > f32::EPSILON {
                                scroll_for_nav.set(new_scroll);
                            }
                            crate::common::row_metrics::chase_row_into_outer_view(
                                ctx,
                                &metrics_for_nav,
                                vb_for_nav.get(),
                                idx,
                                new_scroll,
                            );
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // Alt+Arrow: reorder via the source's accept_drop (when
                    // reorderable). The move is expressed as a synthetic
                    // same-view RowDragData so it travels exactly the same
                    // source-owned path as a pointer drop.
                    if modifiers.alt() && reorderable {
                        let selected_idx = sel_for_key
                            .as_ref()
                            .and_then(|s| s.selected_indices().first().copied());
                        if let Some(idx) = selected_idx {
                            let mv = match key {
                                teksilo_core::event::Key::ArrowUp if idx > 0 => {
                                    Some((idx - 1, DropPosition::Before, idx - 1))
                                }
                                teksilo_core::event::Key::ArrowDown if idx + 1 < count => {
                                    Some((idx + 1, DropPosition::After, idx + 1))
                                }
                                _ => None,
                            };
                            if let Some((target, position, dest)) = mv {
                                // Synthetic same-view payloads must stash the
                                // dragged row's key at construction — the
                                // accept path resolves identity from the
                                // stash, never from `rows`.
                                (stash_for_key)(&[idx]);
                                let payload = DragPayload::typed(RowDragData::<T> {
                                    source: view_id_for_key,
                                    rows: vec![idx],
                                    items: None,
                                });
                                if (accept_drop_for_key)(
                                    &payload,
                                    target,
                                    position,
                                    view_id_for_key,
                                ) {
                                    if let Some(ref sel) = sel_for_key {
                                        sel.select(dest);
                                    }
                                    fi.set(Some(dest));
                                    // Reveal the moved row (own viewport first,
                                    // then chain to any enclosing scroll area).
                                    let scroll = scroll_for_nav.get();
                                    let new_scroll =
                                        metrics_for_nav.borrow_mut().scroll_for_ensure_visible(
                                            dest,
                                            scroll,
                                            vh_for_nav.get(),
                                            max_for_nav.get(),
                                        );
                                    if (new_scroll - scroll).abs() > f32::EPSILON {
                                        scroll_for_nav.set(new_scroll);
                                    }
                                    crate::common::row_metrics::chase_row_into_outer_view(
                                        ctx,
                                        &metrics_for_nav,
                                        vb_for_nav.get(),
                                        dest,
                                        new_scroll,
                                    );
                                }
                                return teksilo_core::event::EventResponse::Handled;
                            }
                        }
                    }

                    // Navigation keys (no modifiers or with Shift for extend)
                    //
                    // The cursor is `focused_index` once the user has navigated
                    // or clicked; failing that it is the current selection — a
                    // view can be handed a selected row before it is ever
                    // focused (a launcher preselecting the top entry, a dialog
                    // restoring the last choice), and the keyboard must continue
                    // from what the user can see, not from an invisible zero.
                    //
                    // `None` ("no cursor yet") is deliberately NOT the same as
                    // `Some(0)`: from nothing, Down must land ON the first row
                    // and Up on the last one. Stepping to row 1 instead would
                    // silently skip row 0 — the row the user was looking at —
                    // which is what every toolkit (GTK, Qt, macOS, the ARIA
                    // listbox pattern) explicitly avoids.
                    let cursor = fi
                        .get()
                        .or_else(|| {
                            sel_for_key
                                .as_ref()
                                .and_then(|s| s.selected_indices().first().copied())
                        })
                        .map(|i| i.min(count - 1));
                    // Anchor for the keys that need a row to compute *from*
                    // (paging, activation) rather than a direction to step in.
                    let current = cursor.unwrap_or(0);
                    // The edge-and-page family is resolved once, in
                    // `common::list_nav`, so the five data views cannot drift
                    // apart on it again. A flat list has no row to be scoped
                    // to, so `RowFirst` / `RowLast` never arrive here — but a
                    // list's row *is* the collection, so they read the same way
                    // if the view kind is ever widened.
                    let nav = list_nav::nav_chord(*key, *modifiers, list_nav::ViewKind::Linear);
                    let new_idx = if let Some(chord) = nav {
                        Some(match chord.movement {
                            list_nav::NavMove::First | list_nav::NavMove::RowFirst => 0,
                            list_nav::NavMove::Last | list_nav::NavMove::RowLast => count - 1,
                            // Geometry-driven, so variable and auto-measured
                            // heights page by visual distance rather than by a
                            // fixed row count; the ensure-visible below then
                            // scrolls to follow.
                            list_nav::NavMove::Page { down } => {
                                let vh = vh_for_nav.get();
                                let r = {
                                    let mut m = metrics_for_nav.borrow_mut();
                                    m.resize(count);
                                    let target = if down {
                                        m.row_top(current) + vh
                                    } else {
                                        (m.row_top(current) - vh).max(0.0)
                                    };
                                    m.row_at(target)
                                };
                                // Guarantee progress even when one row is
                                // taller than the whole viewport.
                                if r == current && down {
                                    (current + 1).min(count - 1)
                                } else if r == current {
                                    current.saturating_sub(1)
                                } else {
                                    r.min(count - 1)
                                }
                            }
                        })
                    } else {
                        match key {
                            Key::ArrowDown => Some(match cursor {
                                None => 0,
                                Some(c) => (c + 1).min(count - 1),
                            }),
                            Key::ArrowUp => Some(match cursor {
                                None => count - 1,
                                Some(c) => c.saturating_sub(1),
                            }),
                            Key::Enter => {
                                // Enter activates the focused row (open / commit).
                                if let Some(ref sel) = sel_for_key {
                                    sel.select(current);
                                }
                                if let Some(ref cb) = activate_key {
                                    cb(current, ctx);
                                }
                                return teksilo_core::event::EventResponse::Handled;
                            }
                            Key::Space if modifiers.ctrl() => {
                                // Ctrl+Space toggles the focused row's selection —
                                // the keyboard equivalent of Ctrl+click. Distinct
                                // from plain Space below: it always toggles (even
                                // in Single mode, via `SelectionModel::toggle`'s
                                // own Single-mode fallback to `select`), pairing
                                // with Ctrl+Arrow's cursor-only move so a user can
                                // walk the cursor without disturbing the existing
                                // selection, then Ctrl+Space to add rows one at a
                                // time.
                                //
                                // Both halves stay on literal `ctrl()`, macOS
                                // included: ⌘Space is Spotlight and never reaches
                                // an app, and ⌘↑/⌘↓ already mean something else in
                                // a Finder list. This Explorer-style cursor pair
                                // has no ⌘ counterpart, so Control keeps it
                                // reachable and out of the platform's way.
                                if let Some(ref sel) = sel_for_key {
                                    sel.toggle(current);
                                }
                                fi.set(Some(current));
                                return teksilo_core::event::EventResponse::Handled;
                            }
                            Key::Space => {
                                // A row carrying a checkbox reads Space as
                                // "check this" — what Windows does for a
                                // checkbox list view, and what a visible
                                // checkbox looks like it should answer to. The
                                // row's control is out of the Tab order, so
                                // this is its only keyboard route; Ctrl+Space
                                // above keeps toggling the *selection*.
                                //
                                // Rows without a checkbox are unaffected:
                                // there is no published toggle, so Space falls
                                // through to the selection as before.
                                if let Some(row_id) = row_map_for_key
                                    .borrow()
                                    .iter()
                                    .find(|(i, _)| *i == current)
                                    .map(|(_, id)| *id)
                                {
                                    let sel_fallback = sel_for_key.clone();
                                    ctx.row_space_activate(
                                        row_id,
                                        std::rc::Rc::new(move || {
                                            if let Some(ref sel) = sel_fallback {
                                                if sel.mode() == teksilo_data::SelectionMode::Multi
                                                {
                                                    sel.toggle(current);
                                                } else {
                                                    sel.select(current);
                                                }
                                            }
                                        }),
                                    );
                                    fi.set(Some(current));
                                    return teksilo_core::event::EventResponse::Handled;
                                }
                                // Otherwise Space moves/toggles the selection but
                                // does NOT activate — the platform convention
                                // (Enter is the activator). Multi: toggle the
                                // focused row; Single: select it.
                                if let Some(ref sel) = sel_for_key {
                                    if sel.mode() == teksilo_data::SelectionMode::Multi {
                                        sel.toggle(current);
                                    } else {
                                        sel.select(current);
                                    }
                                }
                                fi.set(Some(current));
                                return teksilo_core::event::EventResponse::Handled;
                            }
                            _ => None,
                        }
                    };

                    if let Some(idx) = new_idx {
                        fi.set(Some(idx));
                        // What the chord does to the selection. The edge-and-page
                        // keys carry their own answer from `list_nav`, where the
                        // accelerator means "move the cursor, leave the selection
                        // alone" — the rule GTK4 and Qt both apply to *every*
                        // navigation key.
                        //
                        // The arrows keep reading literal `ctrl()` instead: ⌘↑/⌘↓
                        // already mean something else in a Finder list (see the
                        // Ctrl+Space arm above), so this pair has no ⌘ counterpart
                        // to move to. That asymmetry is deliberate — it is also
                        // what leaves ⌘↑/⌘↓ free for the macOS aliases.
                        let op = match nav {
                            Some(chord) => chord.selection,
                            None if modifiers.ctrl()
                                && !modifiers.shift()
                                && matches!(key, Key::ArrowUp | Key::ArrowDown) =>
                            {
                                list_nav::SelectionOp::Suppress
                            }
                            None if modifiers.shift() => list_nav::SelectionOp::Extend,
                            None => list_nav::SelectionOp::Replace,
                        };
                        if let Some(ref sel) = sel_for_key {
                            match op {
                                list_nav::SelectionOp::Replace => sel.select(idx),
                                list_nav::SelectionOp::Suppress => {}
                                list_nav::SelectionOp::Extend => sel.extend_to(idx),
                                list_nav::SelectionOp::ExtendAdditive => {
                                    sel.extend_to_additive(idx)
                                }
                            }
                        }
                        // Scroll into view — the ListView's own viewport first,
                        // then chain to any enclosing scroll area.
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
                        crate::common::row_metrics::chase_row_into_outer_view(
                            ctx,
                            &metrics_for_nav,
                            vb_for_nav.get(),
                            idx,
                            new_scroll,
                        );
                        return teksilo_core::event::EventResponse::Handled;
                    }
                }
                teksilo_core::event::EventResponse::Ignored
            });
        }

        // --- DnD: register self as a drop target when it can reorder OR accept
        // foreign rows. The source's `can_accept` decides per-hover whether the
        // drop is allowed (and a forbidden verdict shows no insertion line). ---
        if self.export.is_drop_target(self.reorderable) {
            let metrics_for_hover = self.metrics.clone();
            let scroll_for_hover = self.scroll_y.clone();
            let len_for_hover = self.source.len_fn.clone();
            let can_accept_for_hover = self.source.dnd.can_accept_fn.clone();
            let my_view_id = self.model_id;

            let feedback_for_hover = self.drop_feedback.clone();
            let width_for_hover = self.placed_content_width.clone();
            let export_for_hover = self.export.clone();
            handlers = handlers.on_drag_hover(move |payload, position, _ctx| {
                let scroll = scroll_for_hover.get().max(0.0);
                let content_y = position.y + scroll;
                let len = (len_for_hover)();
                let (insertion_y, ins) = {
                    let mut m = metrics_for_hover.borrow_mut();
                    m.resize(len);
                    let ins = m.insertion_index(content_y);
                    (m.row_top(ins) - scroll, ins)
                };
                let line_width = width_for_hover.get();
                // Ask the source whether a drop here is allowed; paint the
                // insertion line only when it is. A foreign exported row is
                // allowed when `accept_foreign_rows` is on even though a bare
                // `ListModel`'s `can_accept` rejects the `Foreign` branch.
                let allowed = flat_insertion_target(ins, len).is_some_and(|(target, pos)| {
                    !matches!(
                        (can_accept_for_hover)(payload, target, pos, my_view_id),
                        DropResponse::Reject
                    ) || export_for_hover.accepts_foreign_export(payload, my_view_id)
                });
                if allowed {
                    feedback_for_hover.set(Some((insertion_y, line_width)));
                    DropFeedback::InsertionLine {
                        y: insertion_y,
                        width: line_width,
                    }
                } else {
                    feedback_for_hover.set(None);
                    DropFeedback::NoFeedback
                }
            });

            let len_for_drop = self.source.len_fn.clone();
            let accept_drop_for_drop = self.source.dnd.accept_drop_fn.clone();
            let drop_view_id = self.model_id;
            let scroll_for_drop = self.scroll_y.clone();
            let metrics_for_drop = self.metrics.clone();
            let export_for_drop = self.export.clone();
            let reorderable_for_drop = self.reorderable;

            handlers = handlers.on_drop(move |mut payload, position, ctx| {
                let scroll = scroll_for_drop.get().max(0.0);
                let content_y = position.y + scroll;
                let len = (len_for_drop)();
                let ins = {
                    let mut m = metrics_for_drop.borrow_mut();
                    m.resize(len);
                    m.insertion_index(content_y)
                };
                let is_same_view = payload
                    .get_typed::<RowDragData<T>>()
                    .is_some_and(|rd| rd.source == drop_view_id);
                // A same-view reorder only happens when the view is
                // `reorderable`; a foreign payload is the source's call (a bare
                // ListModel rejects it).
                if (reorderable_for_drop || !is_same_view)
                    && let Some((target, position_kind)) = flat_insertion_target(ins, len)
                    && (accept_drop_for_drop)(&payload, target, position_kind, drop_view_id)
                {
                    if is_same_view {
                        export_for_drop.note_self_reorder();
                    }
                    return true;
                }
                // Otherwise, the shared foreign-receive sugar (peek-before-take).
                export_for_drop.foreign_receive(&mut payload, drop_view_id, ins, ctx)
            });

            // Clear the insertion line whenever the drag leaves this
            // widget — pointer moves to another target, drop completes,
            // Escape cancels, or the source is destroyed.
            let feedback_for_leave = self.drop_feedback.clone();
            handlers = handlers.on_drag_leave(move |_ctx| {
                feedback_for_leave.set(None);
            });

            // Per-frame auto-scroll when the pointer lingers within
            // 32 px of the viewport top or bottom edge during a drag.
            // Linear ramp inside the edge zone, capped at ~12 px/frame
            // so fast-moving fingers still feel responsive but don't
            // rocket past the content.
            let scroll_for_tick = self.scroll_y.clone();
            let max_scroll_for_tick = self.max_scroll_y.clone();
            let viewport_for_tick = self.viewport_height.clone();
            handlers = handlers.on_drag_tick(move |pos, ctx| {
                let h = viewport_for_tick.get();
                let band = crate::common::drag_autoscroll::band_for(ctx.pointer_kind());
                let delta = crate::common::drag_autoscroll::step(pos.y, h, band);
                if delta.abs() > 0.01 {
                    let max = max_scroll_for_tick.get();
                    let new_y = (scroll_for_tick.get() + delta).clamp(0.0, max);
                    scroll_for_tick.set(new_y);
                }
            });
        }

        // Export completion (move-out): fires on the drag source — this view's
        // root id, the stable id start_drag was given.
        handlers = self.export.install_completion(handlers);

        ctx.apply_self_handlers(handlers);

        // --- Body pane ---
        // Hoisted into its own widget so that scroll-buffer-exit rebuilds
        // (which happen mid-thumb-drag once the user scrolls past the
        // buffered range) target a SIBLING of the scrollbar rather than the
        // scrollbar's ancestor. Rebuilding the ancestor would be deferred by
        // the framework to preserve the captured drag, leaving the list blank
        // until the user released the thumb. See `body_pane`'s module docs.
        let pane = body_pane::ListBodyPane::<T> {
            source: self.source.clone(),
            delegate: self.delegate.clone(),
            row_tooltips: self.row_tooltips.clone(),
            metrics: self.metrics.clone(),
            row_selection: self.row_selection.clone(),
            focused_index: self.focused_index.clone(),
            row_map: self.row_map.clone(),
            reorderable: self.reorderable,
            export: self.export.clone(),
            on_activate: self.on_activate.clone(),
            activate_on: self.activate_on,
            model_id: self.model_id,
            root_id: self_id,
            scroll_y: self.scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            placed_content_width: self.placed_content_width.clone(),
            version: self.pane_version.clone(),
            total_refresh: self.layout_refresh.clone(),
            prev_built_start: self.pane_built_start.clone(),
            prev_built_end: self.pane_built_end.clone(),
            item_entries: Vec::new(),
            row_roots: Vec::new(),
        };
        self.body_pane_id = Some(ctx.add(pane));

        // --- Create scrollbar ---
        // Skipped when the caller opted out via `show_scrollbar(false)`
        // — they're expected to mount their own, wired through the
        // exposed signal accessors.
        if self.show_scrollbar {
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
        } else {
            self.scrollbar_id = None;
        }

        self.child_ids()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // The viewport takes whatever the parent offers — but only an
        // allocation is cached for the visible-range computation; a
        // measurement's fallback is not a viewport (`common::viewport`).
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
        _ctx: &LayoutContext,
    ) {
        // Cache our own absolute bounds for the keyboard handler's
        // outer-scroll chase (`ensure_visible`). Done before the empty-children
        // bail so the rect stays fresh even for an empty list that later fills.
        self.viewport_bounds.set(bounds);
        // The allocated height is the authoritative viewport: `build` sizes its
        // realization window from this, and a stale value there costs a
        // permanent rebuild loop (`common::viewport`).
        crate::common::viewport::record_viewport_height(&self.viewport_height, bounds.height);
        // The rubber band's resistance is a fraction of the viewport. This
        // view does not band, but the scroller reads the extent either way and
        // this is the only pass that knows it.
        self.scroller
            .borrow_mut()
            .set_viewport(teksilo_canvas::Vec2::new(bounds.width, bounds.height));

        if children.is_empty() {
            return;
        }

        let viewport_height = bounds.height;

        // The scrollbar decision uses the pre-measure total: the content
        // width must be known before rows can be measured at it. If a
        // measurement flips the decision, the next frame corrects it.
        let provisional_total = self.total_content_height();
        let needs_internal_scrollbar =
            self.show_scrollbar && provisional_total > viewport_height + 0.5;
        let reserves_bar = self.scroll_bar_style == ScrollBarMode::Permanent;
        let content_width = if needs_internal_scrollbar && reserves_bar {
            (bounds.width - SCROLLBAR_THICKNESS).max(0.0)
        } else {
            bounds.width
        };
        self.placed_content_width.set(content_width);

        // Totals for the scrollbar. In auto-measure mode these are computed
        // BEFORE the pane measures its rows (parent-before-child ordering), so
        // the pane pokes `layout_refresh` when a measurement moves the total
        // and we re-place next frame with the corrected value.
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

        // Two children in a fixed order (see `child_ids`): the body pane
        // fills the content column and positions its own rows; the scrollbar
        // sits alongside it.
        let mut next = 0;
        if self.body_pane_id.is_some() {
            if let Some(child) = children.get_mut(next) {
                child.origin = bounds.origin();
                child.size = Size::new(content_width, bounds.height);
            }
            next += 1;
        }
        if self.scrollbar_id.is_some()
            && let Some(sb_child) = children.get_mut(next)
        {
            if needs_internal_scrollbar {
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
        canvas: &mut teksilo_canvas::Canvas,
        ctx: &teksilo_core::widget::PaintContext,
    ) {
        // Draw insertion line during drag hover. Recipe-driven role +
        // thickness — defaults to BorderRole::Accent / 2 dp; a custom
        // `ListContainerStyle` installed via the theme slot overrides.
        if let Some((y, width)) = self.drop_feedback.get() {
            let recipe = ctx
                .theme
                .style_slots
                .list_container
                .as_ref()
                .map(|s| s.insertion())
                .unwrap_or_default();
            let color = recipe.role.resolve(&ctx.theme.colors);
            let line_y = bounds.y + y;
            let line_x = bounds.x;
            let half = recipe.thickness * 0.5;
            // Own paint isn't covered by `clips_children` — clip so an
            // insertion line at the after-last boundary can't bleed
            // past the widget's bottom edge.
            canvas.set_clip(bounds);
            canvas.fill_rect(
                Rect::new(line_x, line_y - half, width, recipe.thickness),
                color,
            );
            canvas.clear_clip();
        }

        // Container focus ring — keyboard focus landed but nothing is selected,
        // so no row ring shows; outline the whole view (see TreeView).
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

    /// The context-menu key opens the *current row's* menu, not the list's.
    ///
    /// A `ListView` is focusable and its rows deliberately are not — the
    /// container owns focus and `set_selected` is what tells assistive
    /// technology which row is current (see `list_item_a11y`). So the
    /// dispatcher's default of "the focused widget" would open the list's own
    /// menu, in the widget family where a per-row menu matters most.
    ///
    /// The row the user means is the keyboard cursor if they have navigated
    /// (`focused_index`), else the first selected row. Both are indices into
    /// the model, and only realized rows have a widget, so a cursor scrolled
    /// outside the virtualization window resolves to nothing and the menu falls
    /// back to the list — which is the right answer, since there is no row on
    /// screen for it to be about.
    fn context_menu_key_target(&self) -> Option<WidgetId> {
        self.current_row_widget()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::ListBox);
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

        // The logical row count, not the realized virtualization window: a
        // 200-row list announces "of 200" even while twenty rows exist as
        // widgets. It belongs here rather than on each row, because
        // `size_of_set_from_container` resolves an item's set size by walking
        // *up* from it — a size written on a row is read by no adapter.
        builder.set_size_of_set(self.source.len());

        // The current row, as the container's active descendant.
        //
        // Keyboard focus stays here, on the list, and the row is marked
        // `selected`. On AT-SPI that is the whole story: Orca announces the
        // selection change. On Windows it is not, because UIA has no
        // active-descendant property at all — what it has is a focused
        // element, and for a list box that element is the item.
        //
        // AccessKit bridges the two in the consumer rather than in each
        // adapter: `accesskit_consumer` resolves the focused node as
        // `focused.active_descendant().unwrap_or(focused)`
        // (`tree.rs:541`) and `accesskit_windows::focus_moved`
        // (`adapter.rs:341-345`) raises `UIA_AutomationFocusChangedEventId` on
        // whatever comes out. So this one property turns every arrow press
        // into the focus change a screen reader announces, and `is_focused`
        // (`consumer node.rs:89-105`) moves from this container to the row,
        // which is what the ARIA listbox pattern says should happen.
        //
        // Without it, arrowing through any Teksilo list is silent to NVDA:
        // there is no focus change to announce, and the selection event that
        // is raised names a row node the pane rebuilt a moment earlier. The
        // mouse still reads rows correctly, because hit-testing does not go
        // through events at all, which is exactly how this hid for so long.
        // Only while this view actually holds focus. A container that does not
        // have focus has no active descendant to speak of, and publishing one
        // anyway puts a second relation in the tree for a client to follow: the
        // combobox pattern (`CommandPalette`) keeps focus on a text field that
        // points at a row in *this* list, and two publishers of the same row is
        // an ambiguity nobody needs to resolve.
        if self.view_focused.get()
            && let Some(row) = self.current_row_widget()
        {
            builder.set_active_descendant(teksilo_core::accessibility::widget_id_to_node_id(row));
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids()
    }

    fn clips_children(&self) -> bool {
        true
    }
}
