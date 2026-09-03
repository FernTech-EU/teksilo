// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Widget`] trait implementation for [`TreeView`]: build,
//! layout, placement, paint, and accessibility.

use super::*;

impl<T: 'static> TreeView<T> {
    /// The realized row the keyboard is on: the navigation cursor when there
    /// is one, else the first selected row.
    ///
    /// `None` when that row is outside the virtualization window, which is the
    /// honest answer: there is no widget for it, so there is no node to point
    /// at and nothing on screen for a menu or an announcement to be about.
    ///
    /// The index is the **flat** (visible) row index, the same coordinate
    /// `focused_index` and `row_map` are keyed on, so a collapsed branch's
    /// descendants simply are not in it.
    fn current_row_widget(&self) -> Option<WidgetId> {
        let index = self.focused_index.get().or_else(|| {
            self.row_selection
                .as_ref()
                .and_then(|s| s.selected_indices().first().copied())
        })?;
        let map = self.row_map.borrow();
        map.iter().find(|(i, _)| *i == index).map(|(_, id)| *id)
    }

    /// Scroll the row the keyboard is on into view when this tree takes focus.
    ///
    /// Only the rows near the viewport are realized, so on a tree taller than
    /// the window the current row frequently has no widget. Everything that
    /// speaks for it then has nothing to speak about: no node carries
    /// `selected`, [`Self::current_row_widget`] resolves to `None` so no active
    /// descendant is nominated, and a screen reader taking focus here is told
    /// nothing at all. Worse, the first arrow press steps *past* that row,
    /// because the cursor was somewhere the user was never shown.
    ///
    /// `ensure_index_visible` rather than `scroll_to_index`: a row already on
    /// screen must not jump under somebody who can see it.
    ///
    /// The handles are cloned into the effect rather than reaching through
    /// `self`, which the closure cannot borrow.
    fn reveal_current_row_on_focus(&self, ctx: &mut teksilo_core::build_context::BuildContext) {
        let metrics = self.metrics.clone();
        let scroll_y = self.scroll_y.clone();
        let viewport_height = self.viewport_height.clone();
        let max_scroll_y = self.max_scroll_y.clone();
        let focused_index = self.focused_index.clone();
        let selection = self.row_selection.clone();

        ctx.effect(&self.view_focused, move |focused| {
            if !*focused {
                return;
            }
            let Some(index) = focused_index.get().or_else(|| {
                selection
                    .as_ref()
                    .and_then(|s| s.selected_indices().first().copied())
            }) else {
                return;
            };
            let current = scroll_y.get();
            let target = metrics.borrow_mut().scroll_for_ensure_visible(
                index,
                current,
                viewport_height.get(),
                max_scroll_y.get(),
            );
            if (target - current).abs() > f32::EPSILON {
                scroll_y.set(target);
            }
        });
    }
}

impl<T: 'static> Widget for TreeView<T> {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.enabled.clone());

        // The root builds exactly two children — the body pane and the
        // scrollbar — and neither depends on the source, the selection or the
        // scroll offset. So it declares no `Rebuild`-level binding at all:
        // row realization is the pane's job (see `body_pane`'s module docs for
        // why that separation is load-bearing), and what the root still owns
        // resolves at `Relayout` / `RepaintOnly`.

        // Scrollbar totals + the content-width decision live in the root's
        // `place_children`; a source change or a pane measurement that moves
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

        // Register the animated signal for smooth scrolling on the ROOT and
        // only the root: the scheduler keys an animation to the widget that
        // registered its signal last and cancels it when that widget rebuilds,
        // so registering from the pane too would make every buffer-exit
        // rebuild abort an in-flight fling.
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

        // --- Observe source version (covers both data mutations and expand/collapse) ---
        // One observer, root-owned, doing the bookkeeping the pane can't
        // (metrics divergence, selection prune, keyboard cursor) and then
        // fanning out: rebuild the pane (row content changed) and re-place the
        // root (the content total, hence the thumb, changed).
        let source_version = self.source.version_signal();
        let pane_version_for_data = self.pane_version.clone();
        let layout_refresh_for_data = self.layout_refresh.clone();
        let data_ver = Rc::new(Cell::new(0_u64));
        ctx.effect(&source_version, {
            let dv = data_ver.clone();
            let ver = pane_version_for_data.clone();
            let layout = layout_refresh_for_data.clone();
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
                layout.set(next);
            }
        });

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

        // --- Scroll event handler + DnD ---
        let scroll_y = self.scroll_y.clone();
        let max_scroll = self.max_scroll_y.clone();
        let line_height = self.item_height;
        let overscroll_behavior = self.overscroll_behavior;
        let smooth_scrolling = self.smooth_scrolling;
        let smooth_scroll_duration = self.smooth_scroll_duration;
        let mut handlers = HandlerSet::new()
            .on_scroll(move |event, _ctx| match event {
                teksilo_core::event::WidgetEvent::Scroll { delta, .. } => {
                    let dy = match delta {
                        teksilo_core::event::ScrollDelta::Lines { y, .. } => y * line_height,
                        teksilo_core::event::ScrollDelta::Pixels { y, .. } => *y,
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
                _ => teksilo_core::event::EventResponse::Ignored,
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
                if let teksilo_core::event::WidgetEvent::KeyDown { key, modifiers, .. } = event {
                    use teksilo_core::event::Key;
                    let visible_count = source.visible_count();
                    if visible_count == 0 {
                        return teksilo_core::event::EventResponse::Ignored;
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

                    // Select all visible rows — Ctrl+A, ⌘A on macOS (Multi only).
                    if modifiers.command() && matches!(key, Key::A) {
                        if let Some(ref sel) = sel_for_key
                            && sel.mode() == teksilo_data::SelectionMode::Multi
                        {
                            sel.select_all(visible_count);
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
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
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
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
                            teksilo_core::event::Key::ArrowUp => false,
                            teksilo_core::event::Key::ArrowDown => true,
                            _ => return teksilo_core::event::EventResponse::Ignored,
                        };
                        if let Some(new_flat) = source.keyboard_reorder(flat_idx, down) {
                            set_focus(new_flat);
                            if let Some(ref sel) = sel_for_key {
                                sel.select(new_flat);
                            }
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        return teksilo_core::event::EventResponse::Ignored;
                    }

                    // ArrowRight: expand / ArrowLeft: collapse or move to parent
                    match key {
                        teksilo_core::event::Key::ArrowRight => {
                            if let Some(meta) = source.meta(current)
                                && meta.has_children
                                && !meta.is_expanded
                            {
                                source.set_expanded_at(current, true);
                                return teksilo_core::event::EventResponse::Handled;
                            }
                        }
                        teksilo_core::event::Key::ArrowLeft => {
                            if let Some(meta) = source.meta(current) {
                                if meta.is_expanded {
                                    source.set_expanded_at(current, false);
                                    return teksilo_core::event::EventResponse::Handled;
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
                                    return teksilo_core::event::EventResponse::Handled;
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
                                cb(current, ctx);
                            }
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        Key::Space if modifiers.ctrl() => {
                            // Ctrl+Space toggles the focused row's selection —
                            // the keyboard equivalent of Ctrl+click. Pairs
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
                            set_focus(current);
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        Key::Space => {
                            // Space moves/toggles the selection but does NOT
                            // activate (Enter is the activator). Multi: toggle;
                            // Single: select.
                            if let Some(ref sel) = sel_for_key {
                                if sel.mode() == teksilo_data::SelectionMode::Multi {
                                    sel.toggle(current);
                                } else {
                                    sel.select(current);
                                }
                            }
                            set_focus(current);
                            return teksilo_core::event::EventResponse::Handled;
                        }
                        _ => None,
                    };

                    if let Some(idx) = new_idx {
                        set_focus(idx);
                        // Ctrl+Arrow (no Shift) moves the keyboard cursor
                        // only, leaving the selection untouched — see the
                        // `ListView` sibling implementation for the full
                        // rationale. Only the arrows opt in; Home/End/
                        // PageUp/PageDown keep selecting under Ctrl. Literal
                        // `ctrl()` — see the Ctrl+Space arm above.
                        let cursor_only = modifiers.ctrl()
                            && !modifiers.shift()
                            && matches!(key, Key::ArrowUp | Key::ArrowDown);
                        if !cursor_only && let Some(ref sel) = sel_for_key {
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
                        return teksilo_core::event::EventResponse::Handled;
                    }
                }
                teksilo_core::event::EventResponse::Ignored
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
                // `depth` rides along so `paint` can indent the affordance to
                // the level the dropped row actually lands at — `Before` /
                // `After` are documented as *siblings* of the target, so both
                // take the target's own depth, and so does the `Into` box,
                // which frames that very row.
                let (effective, depth) = match (source_for_hover.dnd.can_accept_fn)(
                    payload, row_idx, drop_pos, my_view_id,
                ) {
                    DropResponse::Reject => {
                        // The source itself won't take this drop — fall back to
                        // the foreign-export sugar, shown as a plain between-rows
                        // insertion (a foreign source has no Into/reparent
                        // semantics to honor). It lands at a flat index with no
                        // nesting the view can promise, so it claims none:
                        // depth 0.
                        let foreign_ok =
                            export_for_hover.accepts_foreign_export(payload, my_view_id);
                        if !foreign_ok {
                            feedback_for_hover.set(None);
                            return DropFeedback::NoFeedback;
                        }
                        (DropPosition::Before, 0)
                    }
                    DropResponse::Accept => (drop_pos, source_for_hover.depth(row_idx)),
                    DropResponse::Redirect(p) => (p, source_for_hover.depth(row_idx)),
                };
                if effective == DropPosition::Into {
                    // Drop *into* the hovered container → highlight its whole row.
                    let top = row_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Rect {
                        top,
                        height: row_h,
                        width: line_width,
                        depth,
                    }));
                    DropFeedback::HighlightRect {
                        rect: Rect::new(0.0, top, line_width, row_h),
                        color: teksilo_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25),
                    }
                } else {
                    let insertion_y = insertion_top - scroll;
                    feedback_for_hover.set(Some(DropViz::Line {
                        y: insertion_y,
                        width: line_width,
                        depth,
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

        // --- Body pane ---
        // Hoisted into its own widget so that scroll-buffer-exit rebuilds
        // (which happen mid-thumb-drag once the user scrolls past the buffered
        // range) target a SIBLING of the scrollbar rather than the scrollbar's
        // ancestor. Rebuilding the ancestor would be deferred by the framework
        // to preserve the captured drag, leaving the tree blank until the user
        // released the thumb. See `body_pane`'s module docs.
        let pane = super::body_pane::TreeViewBodyPane::<T> {
            source: self.source.clone(),
            row_delegate: self.row_delegate.clone(),
            row_tooltips: self.row_tooltips.clone(),
            metrics: self.metrics.clone(),
            row_selection: self.row_selection.clone(),
            focused_index: self.focused_index.clone(),
            focused_anchor: self.focused_anchor.clone(),
            reorderable: self.reorderable,
            row_click_expands: self.row_click_expands,
            export: self.export.clone(),
            on_activate: self.on_activate.clone(),
            activate_on: self.activate_on,
            tree_id: self.tree_id,
            root_id: self_id,
            scroll_y: self.scroll_y.clone(),
            viewport_height: self.viewport_height.clone(),
            version: self.pane_version.clone(),
            total_refresh: self.layout_refresh.clone(),
            prev_built_start: self.pane_built_start.clone(),
            prev_built_end: self.pane_built_end.clone(),
            item_entries: Vec::new(),
            row_map: self.row_map.clone(),
        };
        self.body_pane_id = Some(ctx.add(pane));

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
        self.scrollbar_id = Some(ctx.add(scrollbar));

        self.child_ids()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Only an allocation may seed the cached viewport — see
        // `common::viewport` for what a measurement pass does to `build`'s
        // realization window otherwise.
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
        // outer-scroll chase (`ensure_visible`), before the empty-children bail.
        self.viewport_bounds.set(bounds);
        // The allocated height is the authoritative viewport: `build` sizes its
        // realization window from this, and a stale value there costs a
        // permanent rebuild loop (`common::viewport`).
        crate::common::viewport::record_viewport_height(&self.viewport_height, bounds.height);

        if children.is_empty() {
            return;
        }

        let viewport_height = bounds.height;
        // Permanent reserves a column for the bar; Overlay / Thin float
        // over the content, so rows span the full width.
        let reserves_bar = self.scroll_bar_style == ScrollBarMode::Permanent;
        let content_width = if reserves_bar {
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

        // Two children in a fixed order (see `child_ids`): the body pane fills
        // the content column and positions its own rows; the scrollbar sits
        // alongside it.
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
        canvas: &mut teksilo_canvas::Canvas,
        ctx: &teksilo_core::widget::PaintContext,
    ) {
        // Draw the drop affordance during drag hover — recipe-driven role +
        // thickness via `ListContainerStyle::insertion()` / `drop_into()`.
        if let Some(viz) = self.drop_feedback.get() {
            let slot = ctx.theme.style_slots.list_container.as_ref();
            let recipe = slot.map(|s| s.insertion()).unwrap_or_default();
            let color = recipe.role.resolve(&ctx.theme.colors);
            // Own paint isn't covered by `clips_children` — clip so feedback at
            // the after-last boundary can't bleed past the widget's bottom edge.
            canvas.set_clip(bounds);
            match viz {
                DropViz::Line { y, width, depth } => {
                    let line_y = bounds.y + y;
                    let half = recipe.thickness * 0.5;
                    let indent = (depth as f32 * recipe.indent_step).min(width);
                    canvas.fill_rect(
                        Rect::new(
                            bounds.x + indent,
                            line_y - half,
                            width - indent,
                            recipe.thickness,
                        ),
                        color,
                    );
                }
                DropViz::Rect {
                    top,
                    height,
                    width,
                    depth,
                } => {
                    // Into-container highlight. Inset on every side — see
                    // `ListDropIntoRecipe::inset`: flush to the row, its top and
                    // bottom edges would be the very pixels a Before / After
                    // line occupies, and the affordance would stop saying
                    // anything the line doesn't.
                    let into = slot.map(|s| s.drop_into()).unwrap_or_default();
                    let color = into.role.resolve(&ctx.theme.colors);
                    let indent = (depth as f32 * recipe.indent_step).min(width);
                    let rect = Rect::new(
                        bounds.x + indent + into.inset,
                        bounds.y + top + into.inset,
                        (width - indent - into.inset * 2.0).max(0.0),
                        (height - into.inset * 2.0).max(0.0),
                    );
                    let radius = teksilo_tokens::CornerRadius::uniform(into.corner_radius);
                    canvas.fill_rounded_rect(rect, radius, color.with_alpha(into.fill_alpha));
                    canvas.stroke_rounded_rect(rect, radius, color, into.thickness);
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

    /// The context-menu key opens the *current row's* menu, not the tree's.
    ///
    /// A `TreeView` is focusable and its rows deliberately are not — the
    /// container owns focus and `set_selected` is what tells assistive
    /// technology which row is current (see `list_item_a11y`, which says so
    /// explicitly). So the dispatcher's default of "the focused widget" would
    /// open the tree's own menu, in the widget family where a per-row menu
    /// matters most.
    ///
    /// The row the user means is the keyboard cursor if they have navigated,
    /// else the first selected row. Only realized rows have a widget, so a
    /// cursor scrolled outside the virtualization window resolves to nothing
    /// and the menu falls back to the tree — right, because there is no row on
    /// screen for it to be about.
    fn context_menu_key_target(&self) -> Option<WidgetId> {
        self.current_row_widget()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Tree);
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

        // The role above is left exactly as it was found. It is not what makes
        // the nomination below work: `focus_id` is the raw stored value
        // (`accesskit_consumer-0.39.0/src/tree.rs:534-536`) and neither it nor
        // the `active_descendant` resolution beneath it consults `common_filter`
        // at all, so a container the filter drops can still be the node whose
        // active descendant an adapter reads.
        //
        // No `size_of_set` here, deliberately. A flattened tree cannot express
        // "the 2nd of 5 siblings" from a single container value, and the reason
        // is argued in full at `list_item_a11y.rs:263-276`: AccessKit resolves
        // an item's set size by walking *up* from it, so the only number this
        // node could carry is one shared by every row at every depth. Doing it
        // correctly needs a real `Role::Group` per expanded branch. Writing the
        // number anyway would make a missing feature look like a working one.

        // The current row, as the container's active descendant.
        //
        // Keyboard focus stays here, on the tree, and the row is marked
        // `selected`. On AT-SPI that is the whole story: Orca announces the
        // selection change. On Windows it is not, because UIA has no
        // active-descendant property at all. What it has is a focused element,
        // and for a tree that element is the item.
        //
        // AccessKit bridges the two in the consumer rather than in each
        // adapter: `accesskit_consumer` resolves the focused node as
        // `focused.active_descendant().unwrap_or(focused)` (`tree.rs:541`) and
        // `accesskit_windows::focus_moved` (`adapter.rs:341-345`) raises
        // `UIA_AutomationFocusChangedEventId` on whatever comes out. So this
        // one property turns every arrow press into the focus change a screen
        // reader announces, and `is_focused` (`consumer node.rs:89-105`) moves
        // from this container to the row, which is what the ARIA tree pattern
        // says should happen.
        //
        // Without it, arrowing through any Teksilo tree is silent to NVDA:
        // there is no focus change to announce. The mouse still reads rows
        // correctly, because hit-testing does not go through events at all,
        // which is exactly how this hid for so long.
        //
        // Only while this view actually holds focus. A container that does not
        // have focus has no active descendant to speak of, and publishing one
        // anyway puts a second relation in the tree for a client to follow.
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
