// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use crate::accessibility::AccessNodeBuilder;

impl WidgetTree {
    /// Attach a tooltip to a widget. The tooltip content widget must already
    /// be in the tree (typically added as a dormant widget during build).
    pub fn attach_tooltip(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
    ) {
        self.attach_tooltip_inner(
            anchor_id,
            content_id,
            delay,
            None,
            None,
            crate::overlay::TooltipPlacement::Below,
        );
    }

    /// Variant of [`attach_tooltip`](Self::attach_tooltip) that opens the
    /// tooltip at the given [`TooltipPlacement`](crate::overlay::TooltipPlacement)
    /// — `Side` for anchors stacked vertically (menu items, a vertical tab
    /// strip, list/tree rows) where `Below` would cover the next sibling.
    pub fn attach_tooltip_with_placement(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        placement: crate::overlay::TooltipPlacement,
    ) {
        self.attach_tooltip_inner(anchor_id, content_id, delay, None, None, placement);
    }

    /// Attach a tooltip that auto-promotes to "sticky" after
    /// `sticky_after` elapses post-show. Used by rich tooltips
    /// implementing the sticky-on-dwell UX (typically 2 seconds).
    ///
    /// The tooltip is shown normally after `delay`, then each
    /// subsequent layout pass checks whether `sticky_after` has
    /// elapsed since the overlay was shown. When it has, the tree
    /// calls [`promote_tooltip_to_sticky`](Self::promote_tooltip_to_sticky)
    /// — the entry is flagged sticky (so pointer-leave no longer
    /// auto-dismisses) and the overlay's dismiss behavior is
    /// swapped to `EscapeOrClickOutside`.
    pub fn attach_tooltip_with_sticky(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
    ) {
        self.attach_tooltip_inner(
            anchor_id,
            content_id,
            delay,
            sticky_after,
            None,
            crate::overlay::TooltipPlacement::Below,
        );
    }

    /// Variant of [`attach_tooltip_with_sticky`](Self::attach_tooltip_with_sticky)
    /// that also takes a shared `Rc<Cell<Option<Instant>>>` "sink"
    /// the tree updates whenever the tooltip is shown or dismissed.
    /// The rich tooltip widget reads from this sink to drive its
    /// dwell indicator without needing a paint-gap heuristic.
    pub fn attach_tooltip_with_sticky_sink(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
        shown_at_sink: std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>,
    ) {
        self.attach_tooltip_inner(
            anchor_id,
            content_id,
            delay,
            sticky_after,
            Some(shown_at_sink),
            crate::overlay::TooltipPlacement::Below,
        );
    }

    /// Variant of [`attach_tooltip_with_sticky_sink`](Self::attach_tooltip_with_sticky_sink)
    /// that also carries a [`TooltipPlacement`](crate::overlay::TooltipPlacement)
    /// — the full-featured path used by `MenuItem` / `TabHeader` /
    /// `StandardItem` rich + composite tooltips that want `Side` placement.
    pub fn attach_tooltip_with_sticky_sink_placement(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
        shown_at_sink: std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>,
        placement: crate::overlay::TooltipPlacement,
    ) {
        self.attach_tooltip_inner(
            anchor_id,
            content_id,
            delay,
            sticky_after,
            Some(shown_at_sink),
            placement,
        );
    }

    /// Drop every tooltip previously attached to `anchor_id` whose content is
    /// not `keep_content_id`: dismiss a live overlay, destroy the orphaned
    /// content subtree, and remove the entry.
    ///
    /// **An anchor owns at most one tooltip.** `attach_tooltip*` is called from
    /// `build()`, which re-runs on every rebuild with a freshly-`ctx.add`ed
    /// content widget — and `ctx.add` creates a *parentless* node, so the
    /// rebuild teardown (which walks `old_children`) never reaches it. Without
    /// this retirement the entry table would gain one dead row plus one
    /// orphaned arena node per rebuild, forever. That table is not cold
    /// storage: it is scanned on every pointer move, four times per layout
    /// pass, on every event-loop wake (`next_timer_deadline`) and — worst —
    /// once *per widget* during the accessibility walk, so the leak is O(n)
    /// on the hottest paths in the tree.
    fn retire_tooltips_for_anchor(&mut self, anchor_id: WidgetId, keep_content_id: WidgetId) {
        let stale: Vec<(WidgetId, Option<crate::overlay::OverlayId>)> = self
            .tooltips
            .iter()
            .filter(|entry| entry.anchor_id == anchor_id && entry.content_id != keep_content_id)
            .map(|entry| (entry.content_id, entry.overlay_id))
            .collect();
        if stale.is_empty() {
            return;
        }
        self.tooltips
            .retain(|entry| entry.anchor_id != anchor_id || entry.content_id == keep_content_id);
        for (content_id, overlay_id) in stale {
            // Retire the overlay before the widget: `dismiss_overlay` walks the
            // content subtree for focus/hover restoration, which needs the
            // nodes to still exist.
            if let Some(overlay_id) = overlay_id {
                self.dismiss_overlay(overlay_id);
            }
            self.destroy_subtree(content_id);
        }
    }

    /// Reap the tooltip owned by a widget that is being destroyed.
    ///
    /// The anchor's own teardown never reaches the tooltip content — it is a
    /// parentless node (see [`retire_tooltips_for_anchor`]) — so a destroyed
    /// widget would otherwise leave its entry and content node behind for the
    /// lifetime of the tree. Called from `destroy_subtree_inner`.
    pub(super) fn retire_tooltips_of_destroyed_anchor(&mut self, anchor_id: WidgetId) {
        let stale: Vec<(WidgetId, Option<crate::overlay::OverlayId>)> = self
            .tooltips
            .iter()
            .filter(|entry| entry.anchor_id == anchor_id)
            .map(|entry| (entry.content_id, entry.overlay_id))
            .collect();
        if stale.is_empty() {
            return;
        }
        self.tooltips.retain(|entry| entry.anchor_id != anchor_id);
        for (content_id, overlay_id) in stale {
            if let Some(overlay_id) = overlay_id {
                self.dismiss_overlay(overlay_id);
            }
            self.destroy_subtree(content_id);
        }
    }

    fn attach_tooltip_inner(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
        shown_at_sink: Option<std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>>,
        placement: crate::overlay::TooltipPlacement,
    ) {
        self.retire_tooltips_for_anchor(anchor_id, content_id);
        // A re-attach with the *same* content id (a widget whose build reuses
        // its tooltip node) updates in place rather than stacking a duplicate.
        if let Some(entry) = self
            .tooltips
            .iter_mut()
            .find(|entry| entry.anchor_id == anchor_id && entry.content_id == content_id)
        {
            entry.delay = delay;
            entry.sticky_after = sticky_after;
            entry.placement = placement;
            if shown_at_sink.is_some() {
                entry.shown_at_sink = shown_at_sink;
            }
            return;
        }
        self.arena.set_dormant(content_id);
        self.tooltips.push(TooltipEntry {
            anchor_id,
            content_id,
            delay,
            hover_start: None,
            real_hover_start: None,
            hover_origin: None,
            overlay_id: None,
            sticky_after,
            is_sticky: false,
            shown_at_sim: None,
            shown_at_real: None,
            shown_at_sink,
            promoted_by_focus: false,
            placement,
        });
    }

    /// Resolve a [`TooltipPlacement`](crate::overlay::TooltipPlacement) to
    /// the concrete [`OverlayPlacement`](crate::overlay::OverlayPlacement) used to position the tooltip
    /// overlay. `Below` keeps the historic tooltip offset; `Side` reuses
    /// the submenu-style `TrailingEdge` (RTL-aware, leading fallback,
    /// viewport-clamped) so the tooltip opens beside — not over — the next
    /// vertically-stacked sibling.
    fn tooltip_overlay_placement(
        placement: crate::overlay::TooltipPlacement,
    ) -> crate::overlay::OverlayPlacement {
        match placement {
            crate::overlay::TooltipPlacement::Below => {
                crate::overlay::OverlayPlacement::NearAnchor {
                    offset: bastyde_canvas::Vec2::new(0.0, 8.0),
                }
            }
            crate::overlay::TooltipPlacement::Side => {
                crate::overlay::OverlayPlacement::TrailingEdge
            }
        }
    }

    pub(super) fn process_tooltips(&mut self) {
        let sim_now = self.sim_clock;
        let session_active = self.tooltip_session_active_sim(sim_now);
        self.process_tooltips_impl(
            |entry| {
                entry
                    .hover_start
                    .map(|start| sim_now.saturating_duration_since(start))
            },
            session_active,
            true,
        );
    }

    pub(super) fn process_tooltips_real(&mut self) {
        let real_now = std::time::Instant::now();
        let session_active = self.tooltip_session_active_real(real_now);
        self.process_tooltips_impl(
            |entry| {
                entry
                    .real_hover_start
                    .map(|start| real_now.saturating_duration_since(start))
            },
            session_active,
            false,
        );
    }

    /// Whether any tooltip is visible *and still part of an active hover
    /// session*.
    ///
    /// A pointer-dwelled **sticky** tooltip is deliberately excluded. It
    /// survives pointer-leave and stays up until Escape or a click outside, so
    /// counting it would keep the reshow session warm for as long as it is
    /// pinned — every other anchor in the window would then fire on the 100 ms
    /// path indefinitely, which is not a "session", it is a stuck state.
    fn any_tooltip_in_session(&self) -> bool {
        self.tooltips
            .iter()
            .any(|e| e.overlay_id.is_some() && !e.is_sticky)
    }

    /// Whether the shortened reshow delay applies right now (sim clock).
    fn tooltip_session_active_sim(&self, now: std::time::Instant) -> bool {
        self.any_tooltip_in_session()
            || self
                .tooltip_session_until_sim
                .is_some_and(|until| now < until)
    }

    /// Whether the shortened reshow delay applies right now (real clock).
    fn tooltip_session_active_real(&self, now: std::time::Instant) -> bool {
        self.any_tooltip_in_session()
            || self
                .tooltip_session_until_real
                .is_some_and(|until| now < until)
    }

    /// Resolve the delay for a pending tooltip: full initial delay, or the
    /// shortened reshow delay while a tooltip session is active.
    ///
    /// The shortening is **proportional**, not a flat floor. Windows derives
    /// `TTDT_RESHOW` as `TTDT_INITIAL / 5` rather than pinning an absolute
    /// number, and that ratio is what the two theme tokens encode (100 ms of
    /// 500 ms). Clamping every entry to the absolute `tooltip_reshow_delay`
    /// instead collapsed `tooltip_delay_heavy` to the light tier's 100 ms
    /// whenever a session happened to be warm — so a composite surface or a
    /// scene-item tip, which exists precisely because heavier content needs a
    /// longer statement of intent, popped after an incidental 100 ms brush.
    /// Scaling keeps the light tier at exactly 100 ms while a 700 ms heavy
    /// entry reshows at 140 ms.
    fn effective_tooltip_delay(
        &self,
        entry_delay: std::time::Duration,
        session_active: bool,
    ) -> std::time::Duration {
        if !session_active {
            return entry_delay;
        }
        let motion = &self.theme.motion;
        let base = motion.tooltip_delay.as_secs_f64();
        if base <= 0.0 {
            // Degenerate theme (no initial delay to take a ratio of) — fall
            // back to the absolute token.
            return entry_delay.min(motion.tooltip_reshow_delay);
        }
        let ratio = motion.tooltip_reshow_delay.as_secs_f64() / base;
        // Never *lengthen* a delay on the warm path, whatever the theme says.
        entry_delay.mul_f64(ratio).min(entry_delay)
    }

    fn process_tooltips_impl(
        &mut self,
        elapsed_fn: impl Fn(&TooltipEntry) -> Option<std::time::Duration>,
        session_active: bool,
        use_sim_clock: bool,
    ) {
        // Reconcile externally-dismissed overlays (audit G12): a shown tooltip's
        // overlay may have been removed by the overlay stack's PointerLeave
        // machinery (pointer left BOTH anchor and tooltip for 100ms). Clear the
        // now-stale overlay_id + shown state so the tooltip can re-show on the
        // next dwell, and reset its "shown at" sink.
        let dismissed_indices: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e.overlay_id {
                Some(oid) if !self.overlay_manager.stack.iter().any(|o| o.id == oid) => Some(i),
                _ => None,
            })
            .collect();
        let any_dismissed = !dismissed_indices.is_empty();
        for i in dismissed_indices {
            let e = &mut self.tooltips[i];
            e.overlay_id = None;
            e.is_sticky = false;
            e.promoted_by_focus = false;
            e.shown_at_sim = None;
            e.shown_at_real = None;
            e.hover_origin = None;
            if let Some(sink) = e.shown_at_sink.as_ref() {
                sink.set(None);
            }
        }
        // Keep the reshow session warm for a short grace after dismiss so
        // moving to the next toolbar icon does not pay the full initial delay.
        if any_dismissed {
            let grace = super::TOOLTIP_SESSION_GRACE;
            if use_sim_clock {
                self.tooltip_session_until_sim = Some(self.sim_clock + grace);
            } else {
                self.tooltip_session_until_real = Some(std::time::Instant::now() + grace);
            }
        }

        // Re-evaluate session after dismiss bookkeeping: a still-visible
        // sibling tip, or the grace we just opened, both count. Uses the same
        // sticky-excluding predicate as the two `tooltip_session_active_*`
        // helpers — a pinned tip is not an active session.
        let session_active = session_active
            || self.any_tooltip_in_session()
            || if use_sim_clock {
                self.tooltip_session_active_sim(self.sim_clock)
            } else {
                self.tooltip_session_active_real(std::time::Instant::now())
            };

        let mut to_show = Vec::new();
        // Collect show candidates first so we don't hold a mutable borrow
        // across `arena.is_active` (which needs `&self`).
        let pending: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.overlay_id.is_none())
            .filter(|(_, entry)| self.arena.is_active(entry.anchor_id))
            .filter_map(|(i, entry)| {
                let delay = self.effective_tooltip_delay(entry.delay, session_active);
                let elapsed = elapsed_fn(entry)?;
                (elapsed >= delay).then_some(i)
            })
            .collect();
        for index in pending {
            // A tooltip with nothing to say must not open. An unresolved or
            // blank i18n key would otherwise pop an empty chromed bubble,
            // which reads as a rendering fault rather than as "no tip here".
            // The content widget decides — see `Widget::tooltip_has_content`,
            // which defaults to `true` so arbitrary bodies are never
            // suppressed by a check they did not opt into.
            if !self
                .arena
                .get(self.tooltips[index].content_id)
                .is_none_or(|node| node.widget.tooltip_has_content())
            {
                let entry = &mut self.tooltips[index];
                entry.hover_start = None;
                entry.real_hover_start = None;
                entry.hover_origin = None;
                continue;
            }
            let entry = &mut self.tooltips[index];
            to_show.push((entry.anchor_id, entry.content_id, entry.placement));
            entry.hover_start = None;
            entry.real_hover_start = None;
            entry.hover_origin = None;
        }
        let sim_now = self.sim_clock;
        let real_now = std::time::Instant::now();
        // Tooltips fade in over `duration_fast` (~120 ms) — matches the
        // MotionTokens recommendation for "tooltip fade, popup fade".
        // Reduced-motion users get an instant snap, and so does the warm
        // reshow path: on a toolbar sweep the whole point of the ~100 ms
        // reshow is that the next tip is *already there*, and a 120 ms fade
        // on top of it costs more than the delay it just saved.
        let fade_duration = if self.prefers_reduced_motion || session_active {
            None
        } else {
            Some(self.theme.motion.duration_fast)
        };
        for (anchor_id, content_id, placement) in to_show {
            self.arena.activate(content_id);
            let oid = self.show_overlay(crate::overlay::OverlayRequest {
                content_id,
                anchor: anchor_id,
                placement: Self::tooltip_overlay_placement(placement),
                dismiss: crate::overlay::DismissBehavior::PointerLeave {
                    delay: std::time::Duration::from_millis(100),
                },
                layer: crate::overlay::OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration,
            });
            if let Some(entry) = self
                .tooltips
                .iter_mut()
                .find(|e| e.content_id == content_id)
            {
                entry.overlay_id = Some(oid);
                entry.shown_at_sim = Some(sim_now);
                entry.shown_at_real = Some(real_now);
                if let Some(sink) = entry.shown_at_sink.as_ref() {
                    sink.set(Some(real_now));
                }
            }
        }

        // Sticky-on-dwell sweep:
        //   1. Mark every shown rich tooltip's **subtree** needs_paint
        //      so its `paint()` re-runs each layout pass during the
        //      dwell window AND its children (notably the
        //      `DwellIndicator`) repaint with the freshly-set step.
        //      Marking only the root would leave the indicator's
        //      cached_paint in place and the visible wedge stale.
        //   2. Auto-promote any tooltip whose dwell window has
        //      elapsed: flag the entry sticky and swap the overlay's
        //      dismiss behavior to `EscapeOrClickOutside`. Marking
        //      runs even on the promoting frame so `tick_dwell` can
        //      observe `elapsed >= sticky_after` and flip the
        //      indicator to its pin variant.
        let mut to_mark_paint: Vec<WidgetId> = Vec::new();
        let mut to_promote: Vec<WidgetId> = Vec::new();
        for entry in &self.tooltips {
            let Some(sticky_after) = entry.sticky_after else {
                continue;
            };
            if entry.overlay_id.is_none() || entry.is_sticky {
                continue;
            }
            let elapsed = entry
                .shown_at_real
                .map(|t| real_now.saturating_duration_since(t));
            let elapsed = match elapsed {
                Some(e) => e,
                None => continue,
            };
            // Always mark needs_paint on the dwell window — the
            // promoting frame still needs the widget to repaint so
            // tick_dwell can flip the indicator to its pin variant.
            to_mark_paint.push(entry.content_id);
            if elapsed >= sticky_after {
                to_promote.push(entry.content_id);
            }
        }
        for id in to_mark_paint {
            self.arena.mark_subtree_needs_paint(id);
        }
        for content_id in to_promote {
            self.promote_tooltip_to_sticky(content_id);
        }
    }

    pub(super) fn process_delayed_overlays(&mut self) {
        let sim_now = self.sim_clock;
        let mut noop = crate::window::NoopWindowOps;
        self.process_delayed_overlays_impl(
            |p| sim_now.saturating_duration_since(p.sim_requested_at),
            &mut noop,
        );
    }

    pub(super) fn process_delayed_overlays_real(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let real_now = std::time::Instant::now();
        self.process_delayed_overlays_impl(
            |p| real_now.saturating_duration_since(p.real_requested_at),
            &mut *ops,
        );
    }

    fn process_delayed_overlays_impl(
        &mut self,
        elapsed_fn: impl Fn(&PendingDelayedOverlay) -> std::time::Duration,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let mut ready_indices = Vec::new();
        for (index, pending) in self.pending_delayed_overlays.iter().enumerate() {
            if elapsed_fn(pending) >= pending.delay {
                ready_indices.push(index);
            }
        }

        let mut ready = Vec::new();
        for &index in ready_indices.iter().rev() {
            ready.push(self.pending_delayed_overlays.remove(index));
        }
        ready.reverse();

        let any_shown = !ready.is_empty();
        for pending in ready {
            let content_id = pending.request.content_id;
            self.arena.activate(content_id);
            let current_focus = self.focused;
            self.overlay_manager.show(pending.request);
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
            self.arena.mark_needs_paint(content_id);
            if let Some(focus_target) = pending.focus_target
                && self.arena.is_active(focus_target)
            {
                self.focus_ops(focus_target, &mut *ops);
            }
        }
        if any_shown {
            self.a11y_dirty = true;
        }
    }

    pub(super) fn overlay_ancestor_for_widget(
        &self,
        widget_id: WidgetId,
    ) -> Option<crate::overlay::OverlayId> {
        self.overlay_manager
            .stack
            .iter()
            .rev()
            .find(|overlay| self.is_descendant_of(widget_id, overlay.content_id))
            .map(|overlay| overlay.id)
    }

    pub(super) fn modal_overlay_for_widget(
        &self,
        widget_id: WidgetId,
    ) -> Option<crate::overlay::OverlayId> {
        let mut current = self.overlay_ancestor_for_widget(widget_id);

        while let Some(overlay_id) = current {
            let overlay = self.overlay_manager.overlay(overlay_id)?;
            if matches!(
                overlay.placement,
                crate::overlay::OverlayPlacement::Centered
            ) {
                return Some(overlay_id);
            }
            current = overlay.parent_overlay;
        }

        None
    }

    fn menu_ancestor_for_widget(&self, widget_id: WidgetId) -> Option<WidgetId> {
        let mut current = Some(widget_id);
        while let Some(id) = current {
            if let Some(node) = self.arena.get(id) {
                let mut builder = AccessNodeBuilder::new();
                node.widget.accessibility(&mut builder);
                if builder.role() == accesskit::Role::Menu {
                    return Some(id);
                }
            }
            current = self.arena.parent(id);
        }
        None
    }

    pub(super) fn dismiss_child_overlays_for_source(
        &mut self,
        source_widget: WidgetId,
        preserve_content: Option<WidgetId>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if let Some(parent_overlay) = self.overlay_ancestor_for_widget(source_widget) {
            let preserve_overlay = preserve_content
                .and_then(|content_id| self.overlay_manager.find_by_content(content_id));
            let (dismissed, focus_restore) = self
                .overlay_manager
                .dismiss_descendants_of(parent_overlay, preserve_overlay);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            if let Some(restore_id) = focus_restore
                && self.arena.is_active(restore_id)
            {
                self.focus_ops(restore_id, &mut *ops);
            }
            return;
        }

        let Some(menu_root) = self.menu_ancestor_for_widget(source_widget) else {
            return;
        };
        let preserve_overlay = preserve_content
            .and_then(|content_id| self.overlay_manager.find_by_content(content_id));
        let overlay_ids: Vec<crate::overlay::OverlayId> = self
            .overlay_manager
            .stack
            .iter()
            .filter(|overlay| {
                overlay.parent_overlay.is_none()
                    && self.is_descendant_of(overlay.anchor, menu_root)
                    && !preserve_overlay.is_some_and(|keep| {
                        overlay.id == keep
                            || self.overlay_manager.is_descendant_of(overlay.id, keep)
                    })
            })
            .map(|overlay| overlay.id)
            .collect();

        for overlay_id in overlay_ids {
            let (dismissed, focus_restore) =
                self.overlay_manager.dismiss_with_focus_restore(overlay_id);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            if let Some(restore_id) = focus_restore
                && self.arena.is_active(restore_id)
            {
                self.focus_ops(restore_id, &mut *ops);
            }
        }
    }

    /// Dismiss the menu chain anchored to `source_widget`. Walks from
    /// the source's containing overlay up the `parent_overlay` chain,
    /// collecting every overlay whose content role is *not* a "stop"
    /// role (`Tooltip`, `Dialog`, `AlertDialog`). Dismisses the
    /// topmost collected overlay — `OverlayManager::dismiss` cascades
    /// to descendants, so the entire menu/popover cascade closes in
    /// one go while a hosting tooltip / dialog is preserved.
    ///
    /// Replacement for `dismiss_all_overlays` in menu / dropdown item
    /// activation handlers, where the popover may be hosted inside a
    /// composite tooltip and "all overlays" is too broad.
    pub(super) fn dismiss_self_overlay_chain_for_source(
        &mut self,
        source_widget: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let Some(start) = self.overlay_ancestor_for_widget(source_widget) else {
            return;
        };

        let mut topmost_to_dismiss: Option<crate::overlay::OverlayId> = None;
        let mut current = Some(start);
        while let Some(overlay_id) = current {
            let Some(overlay) = self.overlay_manager.overlay(overlay_id) else {
                break;
            };
            if self.overlay_is_host_surface(overlay_id) {
                break;
            }
            topmost_to_dismiss = Some(overlay_id);
            current = overlay.parent_overlay;
        }

        if let Some(target) = topmost_to_dismiss {
            let (dismissed, focus_restore) =
                self.overlay_manager.dismiss_with_focus_restore(target);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            if let Some(restore_id) = focus_restore
                && self.arena.is_active(restore_id)
            {
                self.focus_ops(restore_id, &mut *ops);
            }
        }
    }

    /// Dismiss every overlay whose content's role is *not* a host
    /// surface (`Tooltip` / `Dialog` / `AlertDialog`). Targets stay
    /// stable across the loop because we resolve ids first, then
    /// dismiss — `OverlayManager::dismiss` cascades to descendants,
    /// so an already-cascaded id is a no-op.
    ///
    /// Replacement for `dismiss_all_overlays` in popover triggers and
    /// pre-show cleanup paths, where the broad "dismiss everything"
    /// semantics also closed an outer composite tooltip or modal
    /// hosting the trigger.
    pub(super) fn dismiss_all_overlays_except_hosts(
        &mut self,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let to_dismiss: Vec<crate::overlay::OverlayId> = self
            .overlay_manager
            .stack
            .iter()
            .map(|overlay| overlay.id)
            .filter(|&id| !self.overlay_is_host_surface(id))
            .collect();

        for overlay_id in to_dismiss {
            let dismissed = self.overlay_manager.dismiss(overlay_id);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
        }
    }

    /// Whether `overlay_id` is a "host" surface — a tooltip, dialog,
    /// or alert dialog. Host overlays survive `dismiss_all_except_hosts`
    /// and stop the upward walk in `dismiss_self_overlay_chain_for_source`,
    /// so a popover hosted inside a composite tooltip can dismiss
    /// itself (or its menu cascade) without taking the host with it.
    pub(super) fn overlay_is_host_surface(&self, overlay_id: crate::overlay::OverlayId) -> bool {
        let Some(overlay) = self.overlay_manager.overlay(overlay_id) else {
            return false;
        };
        // A modal (a `Centered` overlay — see `modal_overlay_for_widget`) is
        // always a host surface: a dropdown / popover / menu opened *inside* a
        // modal must dismiss only its own cascade, never tear down the hosting
        // modal. Without this, a `ComboBox`/menu inside a modal that closes via
        // `dismiss_all_except_hosts` / `dismiss_self_overlay_chain_for_source`
        // walks past the modal (whose content is not a `Dialog`-role widget) and
        // dismisses it too. Same fix that stopped a `TabWidget` overflow menu
        // from closing its hosting composite tooltip, extended to modals.
        if matches!(
            overlay.placement,
            crate::overlay::OverlayPlacement::Centered
        ) {
            return true;
        }
        let Some(node) = self.arena.get(overlay.content_id) else {
            return false;
        };
        // Prefer an `.access_role(...)` override (e.g. a collapsible
        // `MenuBar`'s bar-content node marked `Role::MenuBar`) over the
        // widget's own role — the override is what the AccessKit walker
        // surfaces, so it must also decide host-ness here. Without this,
        // the revealed bar would not be recognised as a host and
        // `dismiss_all_except_hosts` (called when a menu opens) would
        // tear it down mid-navigation.
        let role = node
            .access_overrides
            .as_ref()
            .and_then(|o| o.role)
            .unwrap_or_else(|| {
                let mut builder = AccessNodeBuilder::new();
                node.widget.accessibility(&mut builder);
                builder.role()
            });
        matches!(
            role,
            accesskit::Role::Tooltip
                | accesskit::Role::Dialog
                | accesskit::Role::AlertDialog
                | accesskit::Role::MenuBar
        )
    }

    pub(super) fn dismiss_modal_for_source(
        &mut self,
        source_widget: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let Some(modal_overlay) = self.modal_overlay_for_widget(source_widget) else {
            return false;
        };

        let (dismissed, focus_restore) = self
            .overlay_manager
            .dismiss_with_focus_restore(modal_overlay);
        self.dormant_dismissed_content(&dismissed, &mut *ops);
        if let Some(restore_id) = focus_restore
            && self.arena.is_active(restore_id)
        {
            self.focus_ops(restore_id, &mut *ops);
        }
        true
    }

    pub fn is_descendant_of(&self, widget_id: WidgetId, ancestor: WidgetId) -> bool {
        if widget_id == ancestor {
            return true;
        }
        let mut current = self.arena.parent(widget_id);
        while let Some(parent_id) = current {
            if parent_id == ancestor {
                return true;
            }
            current = self.arena.parent(parent_id);
        }
        false
    }

    /// Whether `widget_id` is the root content node of an active overlay
    /// (an open menu, popover, dialog, …). Menus and popovers move keyboard
    /// focus onto their whole content container while navigating items via
    /// an internal highlight index, so the focus-tooltip path must not treat
    /// that container's focus as a per-item trigger (it would surface every
    /// descendant's rich tooltip at once).
    fn is_overlay_content_root(&self, widget_id: WidgetId) -> bool {
        self.overlay_manager.find_by_content(widget_id).is_some()
    }

    /// Called when a widget gains keyboard focus. For any *rich*
    /// tooltip (one with `sticky_after` set) whose anchor contains
    /// `widget_id`, show the tooltip immediately and promote it to
    /// sticky. This gives keyboard and screen-reader users the same
    /// access to rich-tooltip interactive content as pointer users,
    /// who reach it via the 2 s hover dwell.
    ///
    /// Plain tooltips (no `sticky_after`) are deliberately NOT
    /// auto-shown on focus — their text reaches assistive tech via
    /// the anchor's `aria-describedby` relationship wired in the
    /// a11y tree pass, which is the W3C-recommended pattern for
    /// supplementary hints.
    pub(super) fn tooltip_focus_enter(&mut self, widget_id: WidgetId) {
        // Two ways a registered rich/composite tooltip can relate to the
        // focus target:
        //   • direct  — focus landed ON the anchor or somewhere inside it
        //     (an ordinary focusable control, a self-anchored focusable
        //     widget, and composites whose focus sinks into an inner field).
        //     Always promote.
        //   • reverse — the anchor sits strictly *inside* the focused widget.
        //     This exists for composing controls (e.g. `Button`) that keep
        //     focus on their outer node but anchor the tooltip on an inner
        //     body root. Promote ONLY when the focused widget is a single
        //     such control — never when it is a *container* that merely
        //     happens to be focusable and owns many tooltip-bearing
        //     descendants (an open `MenuList`, whose whole panel receives
        //     focus while items are navigated by an internal highlight
        //     index), or every descendant's rich tooltip fires at once — the
        //     "wall of tooltips".
        //
        // The two arms are mutually exclusive (`if` / `else if`): a widget
        // that anchors its own tooltip to its `self_id` (`TabHeader`,
        // `ColorSwatch`) satisfies BOTH predicates because `is_descendant_of`
        // is reflexive — routing it to `direct` only avoids a duplicate
        // `show_overlay` (which would leak the first overlay).
        type ToShow = (WidgetId, WidgetId, crate::overlay::TooltipPlacement);
        let mut direct: Vec<ToShow> = Vec::new();
        let mut reverse: Vec<ToShow> = Vec::new();
        for e in &self.tooltips {
            if e.sticky_after.is_none() || e.overlay_id.is_some() {
                continue;
            }
            if self.is_descendant_of(widget_id, e.anchor_id) {
                direct.push((e.anchor_id, e.content_id, e.placement));
            } else if self.is_descendant_of(e.anchor_id, widget_id) {
                reverse.push((e.anchor_id, e.content_id, e.placement));
            }
        }
        let mut to_show = direct;
        // A reverse match is a single composing control only when it resolves
        // to exactly one anchor and the focused node is not itself a menu /
        // popover content root. Otherwise it is a container fan-out — skip it.
        if reverse.len() == 1 && !self.is_overlay_content_root(widget_id) {
            to_show.extend(reverse);
        }

        let sim_now = self.sim_clock;
        let real_now = std::time::Instant::now();
        // Sticky tooltips share the standard tooltip fade.
        let fade_duration = if self.prefers_reduced_motion {
            None
        } else {
            Some(self.theme.motion.duration_fast)
        };
        for (anchor_id, content_id, placement) in to_show {
            self.arena.activate(content_id);
            let oid = self.show_overlay(crate::overlay::OverlayRequest {
                content_id,
                anchor: anchor_id,
                placement: Self::tooltip_overlay_placement(placement),
                dismiss: crate::overlay::DismissBehavior::EscapeOrClickOutside,
                layer: crate::overlay::OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration,
            });
            if let Some(entry) = self
                .tooltips
                .iter_mut()
                .find(|e| e.content_id == content_id)
            {
                entry.overlay_id = Some(oid);
                entry.shown_at_sim = Some(sim_now);
                entry.shown_at_real = Some(real_now);
                entry.promoted_by_focus = true;
                if let Some(sink) = entry.shown_at_sink.as_ref() {
                    sink.set(Some(real_now));
                }
            }
            self.promote_tooltip_to_sticky(content_id);
        }
    }

    /// Surface the tooltip of a keyboard-highlighted menu item immediately
    /// (no dwell), positioned per its own `TooltipPlacement`, and dismiss the
    /// previously-highlighted item's tooltip. Real keyboard focus stays on the
    /// enclosing `MenuList` (for key handling); this is keyed on `item_id`.
    ///
    /// The tooltip is shown as a `Manual`-dismiss **child overlay of the
    /// enclosing menu**, so closing the menu (Escape / click-outside / the
    /// opener) cascades the tooltip away automatically, and a single Escape
    /// reaches the menu rather than only clearing the tooltip.
    pub(super) fn show_highlight_tooltip(
        &mut self,
        item_id: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Clear (and reconcile) any currently-shown highlight tooltip first —
        // moving the highlight replaces it; a tooltip-less target clears it.
        self.clear_highlight_tooltip(&mut *ops);

        // Find the single tooltip anchored within the highlighted item's
        // subtree (a `MenuItem` anchors to its inner body root). Skip if it is
        // already shown or the item carries no tooltip.
        let found = self.tooltips.iter().find_map(|e| {
            if e.overlay_id.is_none() && self.is_descendant_of(e.anchor_id, item_id) {
                Some((e.anchor_id, e.content_id, e.placement))
            } else {
                None
            }
        });
        let Some((anchor_id, content_id, placement)) = found else {
            return;
        };

        let parent_overlay = self.overlay_ancestor_for_widget(item_id);
        self.arena.activate(content_id);
        let fade_duration = if self.prefers_reduced_motion {
            None
        } else {
            Some(self.theme.motion.duration_fast)
        };
        let oid = self.show_overlay(crate::overlay::OverlayRequest {
            content_id,
            anchor: anchor_id,
            placement: Self::tooltip_overlay_placement(placement),
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay,
            on_dismiss: None,
            fade_duration,
        });
        let real_now = std::time::Instant::now();
        let sim_now = self.sim_clock;
        if let Some(entry) = self
            .tooltips
            .iter_mut()
            .find(|e| e.content_id == content_id)
        {
            entry.overlay_id = Some(oid);
            entry.shown_at_sim = Some(sim_now);
            entry.shown_at_real = Some(real_now);
            if let Some(sink) = entry.shown_at_sink.as_ref() {
                sink.set(Some(real_now));
            }
        }
        self.highlight_tooltip = Some((oid, content_id));
    }

    /// Dismiss the keyboard-highlight tooltip if one is showing, resetting its
    /// entry so it can re-show later. Safe to call when none is active or when
    /// the overlay was already cascade-dismissed by a menu close (the tracked
    /// id is simply no longer in the stack).
    pub(super) fn clear_highlight_tooltip(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let Some((oid, content_id)) = self.highlight_tooltip.take() else {
            return;
        };
        if self.overlay_manager.stack.iter().any(|o| o.id == oid) {
            let dismissed = self.overlay_manager.dismiss(oid);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
        }
        if let Some(entry) = self
            .tooltips
            .iter_mut()
            .find(|e| e.content_id == content_id)
        {
            entry.overlay_id = None;
            entry.shown_at_sim = None;
            entry.shown_at_real = None;
            if let Some(sink) = entry.shown_at_sink.as_ref() {
                sink.set(None);
            }
        }
    }

    /// Called when focus moves to a new widget. Dismisses every
    /// focus-promoted rich tooltip whose anchor- and tooltip-content
    /// subtrees both fail to contain the new focus — so Tab'ing INTO
    /// a sticky tooltip to click a link keeps it up, but Tab'ing
    /// past it onto unrelated controls closes it (preventing sticky
    /// accumulation as the user navigates through a form).
    ///
    /// Pointer-dwelled stickies (`promoted_by_focus == false`)
    /// survive focus changes intact — they're dismissed only via
    /// Escape or click-outside, matching the existing mouse UX.
    pub(super) fn tooltip_focus_leave_outside(
        &mut self,
        new_focus: Option<WidgetId>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let to_dismiss: Vec<crate::overlay::OverlayId> = self
            .tooltips
            .iter()
            .filter(|e| e.promoted_by_focus && e.overlay_id.is_some())
            .filter(|e| {
                let in_scope = new_focus
                    .map(|nf| {
                        // In scope when the new focus lands in either
                        // the anchor's subtree or the tooltip content's
                        // subtree — covers Tab-to-anchor, Tab-deeper-
                        // inside-anchor, and Tab-into-tooltip.
                        self.is_descendant_of(nf, e.anchor_id)
                            || self.is_descendant_of(e.anchor_id, nf)
                            || self.is_descendant_of(nf, e.content_id)
                    })
                    .unwrap_or(false);
                !in_scope
            })
            .filter_map(|e| e.overlay_id)
            .collect();

        for oid in to_dismiss {
            let dismissed = self.overlay_manager.dismiss(oid);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
        }
    }

    /// Whether a pointer hover over `widget_id` should count as a hover
    /// of `anchor_id` for tooltip purposes.
    ///
    /// `widget_id` must be a descendant-or-self of `anchor_id`, AND no
    /// active-overlay boundary may separate them. The second clause is
    /// what stops a tooltip leaking onto overlay content that merely
    /// *remains* an arena child of its anchor: a `ComboBox`'s dropdown
    /// panel, a `PopoverButton`'s popover, a menu — all are kept under
    /// their trigger in the arena (for hit-test / a11y / teardown) but
    /// are shown as overlays. Hovering one of their rows is a hover of
    /// the *overlay*, not of the anchor's own chrome, so the anchor's
    /// (or any ancestor's) tooltip must not fire.
    ///
    /// Concretely: if the hovered widget lives inside an active overlay
    /// whose content subtree does **not** contain the anchor, the hover
    /// is on the overlay and we return `false`. A tooltip attached to a
    /// widget *inside* the overlay (e.g. a dropdown row's own tooltip)
    /// still fires, because that anchor is itself within the overlay's
    /// content subtree.
    fn tooltip_hover_targets_anchor(&self, widget_id: WidgetId, anchor_id: WidgetId) -> bool {
        if !self.is_descendant_of(widget_id, anchor_id) {
            return false;
        }
        if let Some(overlay_id) = self.overlay_ancestor_for_widget(widget_id)
            && let Some(content_id) = self
                .overlay_manager
                .overlay(overlay_id)
                .map(|o| o.content_id)
            && !self.is_descendant_of(anchor_id, content_id)
        {
            return false;
        }
        true
    }

    /// Arm the dwell for the tooltip that owns this hover.
    ///
    /// A hover target can sit inside several tooltip anchors at once (a row
    /// with its own tip inside a panel with a tip). Only the **innermost**
    /// anchor arms: it is the most specific description of what the pointer is
    /// actually over, and arming the outer ones too would let two tooltips
    /// mature and open simultaneously, one on top of the other.
    ///
    /// "Innermost" is measured by arena depth from the hovered widget, so
    /// nesting order — not the order the anchors happened to be attached in —
    /// decides the winner.
    pub(super) fn tooltip_pointer_enter(&mut self, widget_id: WidgetId) {
        let innermost: Option<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.tooltip_hover_targets_anchor(widget_id, entry.anchor_id))
            .filter_map(|(index, entry)| {
                self.ancestor_distance(widget_id, entry.anchor_id)
                    .map(|depth| (depth, index))
            })
            .min()
            .map(|(_, index)| index);
        let Some(index) = innermost else {
            return;
        };
        // Don't restart a timer for a tip that is already showing.
        if self.tooltips[index].overlay_id.is_some() {
            return;
        }
        self.tooltips[index].hover_start = Some(self.sim_clock);
        self.tooltips[index].real_hover_start = Some(std::time::Instant::now());
        self.tooltips[index].hover_origin = self.last_pointer_position;
        self.arena.mark_needs_paint(self.tooltips[index].anchor_id);
    }

    /// Number of parent hops from `widget_id` up to `ancestor`, or `None` if
    /// `ancestor` is not on the chain. `0` when they are the same widget.
    fn ancestor_distance(&self, widget_id: WidgetId, ancestor: WidgetId) -> Option<usize> {
        let mut current = widget_id;
        let mut depth = 0usize;
        loop {
            if current == ancestor {
                return Some(depth);
            }
            current = self.arena.parent(current)?;
            depth += 1;
        }
    }

    /// Cancel tooltip activity on a pointer press.
    ///
    /// A press is a statement that the user already knows what the control
    /// does: a pending dwell is cancelled (so a tooltip does not pop *after*
    /// the click that answered it), and a shown non-sticky tooltip is
    /// dismissed (so it stops covering what was just clicked). Re-hovering
    /// restarts the delay from scratch, matching Windows and GTK.
    ///
    /// Pointer-dwelled **sticky** tooltips are left alone — they are an
    /// interactive surface the user deliberately pinned, and they own their
    /// own Escape / click-outside dismissal.
    /// `at` is the press position, when there is one. A press landing *inside*
    /// a tooltip's own surface is the user reaching into it (a rich tooltip's
    /// inline link), not dismissing it, so that tooltip is spared.
    pub(super) fn tooltip_pointer_press(&mut self, at: Option<bastyde_canvas::Point>) {
        let mut to_dismiss = Vec::new();
        let candidates: Vec<(usize, Option<crate::overlay::OverlayId>)> = self
            .tooltips
            .iter()
            .enumerate()
            .map(|(i, e)| (i, if e.is_sticky { None } else { e.overlay_id }))
            .collect();
        for (index, overlay_id) in candidates {
            // Content rect only — deliberately NOT `pointer_inside_overlay_region`,
            // which also counts the anchor (it exists to keep the tip alive
            // while the pointer crosses the gap). A press on the *anchor* is
            // exactly the case that must dismiss.
            let inside = match (overlay_id, at) {
                (Some(oid), Some(pos)) => self
                    .overlay_content_bounds(oid)
                    .is_some_and(|rect| rect.contains(pos)),
                _ => false,
            };
            if inside {
                continue;
            }
            let entry = &mut self.tooltips[index];
            entry.hover_start = None;
            entry.real_hover_start = None;
            entry.hover_origin = None;
            if let Some(oid) = overlay_id {
                to_dismiss.push(oid);
            }
        }
        for overlay_id in to_dismiss {
            self.dismiss_overlay(overlay_id);
        }
    }

    /// Retire hover tooltips when the window stops being active.
    ///
    /// Same shape as [`tooltip_pointer_press`](Self::tooltip_pointer_press) —
    /// pending dwells cancelled, shown non-sticky tips dismissed, pinned
    /// stickies preserved — but triggered by the window losing focus rather
    /// than by a click.
    pub(super) fn tooltip_window_deactivated(&mut self) {
        // No position: the window is going away, so every non-sticky tip goes
        // with it regardless of where the pointer happened to be.
        self.tooltip_pointer_press(None);
    }

    /// Cancel every pending (not-yet-shown) tooltip dwell.
    ///
    /// Called when a drag session starts: the pointer is now carrying
    /// something, and a tooltip that pops mid-drag is a stray overlay in the
    /// user's way. `process_tooltips_real` runs from the layout pass, which a
    /// drag keeps driving, so the dwell would otherwise mature and show even
    /// though the pointer-move path is short-circuited for the drag.
    pub(crate) fn tooltip_cancel_pending_dwell(&mut self) {
        for entry in &mut self.tooltips {
            if entry.overlay_id.is_none() {
                entry.hover_start = None;
                entry.real_hover_start = None;
                entry.hover_origin = None;
            }
        }
    }

    /// Restart a pending tooltip's delay when the pointer keeps moving
    /// inside the same anchor beyond the stationary slop. Called from
    /// pointer-move when the hover target has not changed.
    pub(super) fn tooltip_pointer_moved(
        &mut self,
        widget_id: WidgetId,
        position: bastyde_canvas::Point,
    ) {
        let matching: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.overlay_id.is_none()
                    && entry.hover_start.is_some()
                    && self.tooltip_hover_targets_anchor(widget_id, entry.anchor_id)
            })
            .map(|(index, _)| index)
            .collect();
        if matching.is_empty() {
            return;
        }
        let now = self.sim_clock;
        let real_now = std::time::Instant::now();
        let slop = super::TOOLTIP_STATIONARY_SLOP;
        let slop_sq = slop * slop;
        for index in matching {
            let origin = match self.tooltips[index].hover_origin {
                Some(o) => o,
                None => {
                    // Enter happened without a recorded position (tests that
                    // call `tooltip_pointer_enter` directly) — adopt this
                    // position as the origin without restarting the timer.
                    self.tooltips[index].hover_origin = Some(position);
                    continue;
                }
            };
            let dx = position.x - origin.x;
            let dy = position.y - origin.y;
            if dx * dx + dy * dy <= slop_sq {
                continue;
            }
            // Pointer has moved meaningfully since hover began — restart.
            self.tooltips[index].hover_start = Some(now);
            self.tooltips[index].real_hover_start = Some(real_now);
            self.tooltips[index].hover_origin = Some(position);
        }
    }

    /// Promote a shown tooltip from "ephemeral hover" to "sticky".
    ///
    /// - Flags the tooltip entry as sticky so
    ///   `tooltip_pointer_leave`
    ///   no longer auto-dismisses it,
    /// - Swaps the overlay's dismiss behavior to
    ///   `EscapeOrClickOutside` so clicking anywhere off the tooltip
    ///   (or pressing Escape) closes it.
    ///
    /// The entry is **not** removed: when the user later dismisses
    /// the sticky overlay, `dormant_dismissed_content` resets the
    /// entry back to its initial state so a future hover re-shows
    /// the tooltip from scratch.
    ///
    /// Called from `RichTooltipWidget` (or by the auto-promote
    /// sweep) once the dwell timer reaches its threshold.
    pub fn promote_tooltip_to_sticky(&mut self, content_id: WidgetId) {
        let Some(entry) = self
            .tooltips
            .iter_mut()
            .find(|entry| entry.content_id == content_id)
        else {
            return;
        };
        if entry.is_sticky {
            return;
        }
        entry.is_sticky = true;
        let overlay_id = entry.overlay_id;
        if let Some(overlay_id) = overlay_id {
            self.overlay_manager.set_dismiss(
                overlay_id,
                crate::overlay::DismissBehavior::EscapeOrClickOutside,
            );
        }
    }

    pub(super) fn tooltip_pointer_leave(
        &mut self,
        widget_id: WidgetId,
        _ops: &mut dyn crate::window::WindowOps,
    ) {
        let matching: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.is_descendant_of(widget_id, entry.anchor_id))
            .map(|(index, _)| index)
            .collect();
        for index in matching {
            // Cancel any pending (not-yet-shown) dwell so re-hovering restarts
            // the delay timer.
            self.tooltips[index].hover_start = None;
            self.tooltips[index].real_hover_start = None;
            self.tooltips[index].hover_origin = None;
            // Audit G12 (WCAG 1.4.13 Hoverable): do NOT dismiss a *shown*
            // tooltip here on anchor-leave — that killed it the instant the
            // pointer crossed the 8px gap toward the tooltip. Dismissal of a
            // shown (non-sticky) tooltip is owned by the overlay stack's
            // `PointerLeave { 100ms }` machinery (process_pointer_leave_overlays_real,
            // run every frame), whose `pointer_inside_overlay_region` keeps the
            // overlay alive while the pointer is over EITHER the anchor or the
            // tooltip and dismisses only after 100ms outside both. Sticky
            // tooltips are dismissed via Escape / click-outside. The stale
            // overlay_id left when that machinery dismisses the overlay is
            // reconciled at the top of `process_tooltips_impl`.
        }
    }

    /// Returns the earliest deadline for a pending tooltip or delayed overlay (if any).
    pub fn next_timer_deadline(&self) -> Option<std::time::Instant> {
        let now = std::time::Instant::now();
        let session_active = self.tooltip_session_active_real(now);
        let tooltip_deadline = self
            .tooltips
            .iter()
            .filter(|entry| entry.overlay_id.is_none())
            .filter_map(|entry| {
                let start = entry.real_hover_start?;
                let delay = self.effective_tooltip_delay(entry.delay, session_active);
                Some(start + delay)
            })
            .min();

        // Sticky-on-dwell wake-ups: once a rich tooltip is shown, wake once per
        // indicator step (500 ms for the default 2 s promotion) so the step
        // indicator advances and the promotion fires, even with the pointer
        // held still. Without these deadlines the loop would only wake on user
        // input, freezing the dwell counter. The step is derived per entry from
        // its own `sticky_after` rather than hardcoded, so a caller with a
        // non-default promotion window still gets evenly-spaced wake-ups.
        //
        // The step boundary is rounded off `last_frame_time` (the last rendered
        // frame) — NOT `Instant::now()`. The app's `request_redraw_due`
        // re-derives this deadline at each timer wake and only redraws windows
        // whose `deadline <= now`. If we rounded off `now`, then at the instant
        // a 500 ms boundary's wake fires, `elapsed` has just crossed it and the
        // boundary would already have rolled forward to the NEXT step (a future
        // instant) — so `deadline <= now` would never hold and the window would
        // never redraw. The dwell then only advanced when some unrelated input
        // event happened to redraw the window (the "only updates on mouse move"
        // bug). Pinning the boundary to `last_frame_time` keeps the deadline
        // `<= now` at its own wake until a render actually advances the frame
        // time to the next step — one redraw per 500 ms boundary, no free-run.
        let ref_time = self.last_frame_time.unwrap_or_else(std::time::Instant::now);
        let dwell_tooltip_deadline = self
            .tooltips
            .iter()
            .filter_map(|entry| {
                let sticky_after = entry.sticky_after?;
                let shown_at = entry.shown_at_real?;
                if entry.overlay_id.is_none() || entry.is_sticky {
                    return None;
                }
                let dwell_step = sticky_after / super::TOOLTIP_DWELL_STEPS;
                if dwell_step.is_zero() {
                    return None;
                }
                let elapsed = ref_time.saturating_duration_since(shown_at);
                if elapsed >= sticky_after {
                    return None;
                }
                // Round up to the next step boundary so each wake-up lands on a
                // 500 ms / 1 s / 1.5 s / 2 s mark (measured at the last frame).
                let steps_passed = (elapsed.as_millis() / dwell_step.as_millis()) as u32;
                let next_step_at = shown_at + dwell_step * (steps_passed + 1);
                Some(next_step_at.min(shown_at + sticky_after))
            })
            .min();
        let delayed_overlay_deadline = self
            .pending_delayed_overlays
            .iter()
            .map(|pending| pending.real_requested_at + pending.delay)
            .min();
        let auto_dismiss_deadline = self.overlay_manager.next_auto_dismiss_deadline();
        // Hover-opened overlays (every shown tooltip, hover submenus) dismiss
        // on a `PointerLeave { delay }` grace that only advances when a frame
        // runs. The pointer's last motion event is the *start* of that grace,
        // not a reason to wake at its end — so without this term a tooltip the
        // user has walked away from stays on screen for as long as the app
        // stays idle.
        let pointer_leave_deadline = self.overlay_manager.next_pointer_leave_deadline();
        let animation_deadline = self
            .animation_scheduler
            .next_deadline(&self.arena, self.paint_epoch);
        // Same pattern for the shader-driven animated-quad registry —
        // without this the event loop sleeps between frame intervals
        // and shader-driven animations only advance on unrelated
        // wakes (mouse move, scroll), producing a visible staircase.
        let animated_quad_deadline = self
            .animated_quads
            .next_deadline(&self.arena, self.paint_epoch);
        let gesture_deadline = self.next_gesture_deadline();
        let wake_at_deadline = self.pending_wake_at.get();
        // Per-frame-effect path (Pulse / Cycle / caret blink / drag
        // auto-scroll): a fixed 60 Hz deadline instead of the old
        // `ControlFlow::Poll` free-run, so continuous animations render
        // at 60 Hz regardless of the display's refresh rate. See
        // `frame_tick_deadline`.
        let frame_tick_deadline = self.frame_tick_deadline();

        [
            tooltip_deadline,
            dwell_tooltip_deadline,
            delayed_overlay_deadline,
            auto_dismiss_deadline,
            pointer_leave_deadline,
            animation_deadline,
            animated_quad_deadline,
            gesture_deadline,
            wake_at_deadline,
            frame_tick_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn overlay_manager(&self) -> &crate::overlay::OverlayManager {
        &self.overlay_manager
    }

    /// Mutable access to the overlay manager. Used by the
    /// modal-presentation pipeline to wire up cascade-dismissal
    /// between paired overlays (e.g. the dialog scrim and the modal
    /// panel) via `OverlayManager::set_parent_overlay`.
    pub fn overlay_manager_mut(&mut self) -> &mut crate::overlay::OverlayManager {
        &mut self.overlay_manager
    }

    pub fn active_overlays(&self) -> Vec<crate::overlay::OverlayId> {
        self.overlay_manager.active_ids()
    }

    /// Laid-out bounds of an open overlay's content surface.
    ///
    /// This is the size the overlay pass actually measured — taken with an
    /// *unbounded* proposal, independent of the host tree's own proposal — so
    /// it is the right thing to assert against for content that must cap or
    /// wrap itself (tooltips against `TOOLTIP_MAX_WIDTH`, popovers against
    /// their max height). Reading `bounds(content_id)` instead would report
    /// whatever the surrounding layout handed the widget.
    pub fn overlay_content_bounds(
        &self,
        id: crate::overlay::OverlayId,
    ) -> Option<bastyde_canvas::Rect> {
        self.overlay_manager
            .stack
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.bounds)
    }

    pub fn show_overlay(
        &mut self,
        request: crate::overlay::OverlayRequest,
    ) -> crate::overlay::OverlayId {
        let fade_duration = request.fade_duration;
        let content_id = request.content_id;
        let id = self.overlay_manager.show(request);
        // The overlay's content subtree just entered the active set;
        // the AT tree shape changed and the cached snapshot must be
        // rebuilt. The dismiss path already flips this; we must mirror
        // it here, otherwise a popup show + read AT sequence returns
        // the pre-popup snapshot. The unconditional `a11y_dirty = true`
        // in `layout()` previously masked this gap; now this explicit
        // set is required.
        self.a11y_dirty = true;
        if let Some(duration) = fade_duration {
            self.attach_overlay_fade(id, content_id, duration);
        }
        id
    }

    /// Show an overlay relative to a source widget, inheriting the source
    /// overlay ancestry and focus-restore behavior used during event dispatch.
    pub fn show_overlay_from_source(
        &mut self,
        source_widget: WidgetId,
        mut request: crate::overlay::OverlayRequest,
    ) -> crate::overlay::OverlayId {
        if request.parent_overlay.is_none() {
            request.parent_overlay = self.overlay_ancestor_for_widget(source_widget);
        }
        if let Some(existing) = self.overlay_manager.find_by_content(request.content_id) {
            return existing;
        }

        let fade_duration = request.fade_duration;
        let content_id = request.content_id;
        let current_focus = self.focused;
        let id = self.overlay_manager.show(request);
        self.a11y_dirty = true;
        if let Some(focus_id) = current_focus {
            self.overlay_manager.set_top_focus_restore(focus_id);
        }
        if let Some(duration) = fade_duration {
            self.attach_overlay_fade(id, content_id, duration);
        }
        id
    }

    pub fn show_overlay_for(
        &mut self,
        request: crate::overlay::OverlayRequest,
        duration: std::time::Duration,
    ) -> crate::overlay::OverlayId {
        let fade_duration = request.fade_duration;
        let content_id = request.content_id;
        let id = self.overlay_manager.show_for(request, duration);
        self.overlay_manager.set_shown_at_sim(id, self.sim_clock);
        self.a11y_dirty = true;
        if let Some(fade) = fade_duration {
            self.attach_overlay_fade(id, content_id, fade);
        }
        id
    }

    /// Internal: install an animated opacity scope on `content_id`,
    /// kick off the 0→1 fade-in tween, and register the signal with
    /// the overlay manager so the matching fade-out plays on
    /// dismiss. Owner of the animated signal is `content_id` itself
    /// — the visibility-gate fix from the scheduler ensures the
    /// fade-in still ticks even when the content is freshly inserted
    /// and not yet stamped with a paint epoch.
    fn attach_overlay_fade(
        &mut self,
        overlay_id: crate::overlay::OverlayId,
        content_id: WidgetId,
        duration: std::time::Duration,
    ) {
        let opacity = crate::signal::Signal::<f32>::new_animated(0.0);
        self.register_animated_signal(&opacity, content_id);
        self.set_opacity(content_id, opacity.clone());
        // Audit G16 (WCAG 2.3.3 / EN 301 549 11.7): honour reduced motion —
        // snap the overlay to fully visible with no fade-in tween, and register
        // a zero-duration fade so dismissal snaps to 0 as well.
        if self.prefers_reduced_motion() {
            opacity.set(1.0);
            self.overlay_manager
                .attach_fade(overlay_id, opacity, std::time::Duration::ZERO);
            return;
        }
        let _ = opacity.try_animate_with_options(crate::animation::AnimationRequest {
            target: 1.0,
            duration,
            easing: bastyde_tokens::Easing::EaseOut,
            frame_interval: None,
            looping: false,
            epsilon: 0.0,
            max_duration: None,
        });
        self.overlay_manager
            .attach_fade(overlay_id, opacity, duration);
    }

    /// Dismiss an overlay programmatically. Uses
    /// [`NoopWindowOps`](crate::window::NoopWindowOps) for any
    /// focus-loss handlers it triggers — user code fires these from
    /// outside a dispatch.
    pub fn dismiss_overlay(&mut self, id: crate::overlay::OverlayId) {
        let mut noop = crate::window::NoopWindowOps;
        self.dismiss_overlay_with_ops(id, &mut noop);
    }

    /// Dispatch-path variant that threads `ops` through to the
    /// focus-loss handler fired during dismissal.
    pub fn dismiss_overlay_with_ops(
        &mut self,
        id: crate::overlay::OverlayId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let dismissed = self.overlay_manager.dismiss(id);
        self.dormant_dismissed_content(&dismissed, &mut *ops);
    }

    pub(super) fn dormant_dismissed_content(
        &mut self,
        content_ids: &[WidgetId],
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Reset any tooltip entries that match a dismissed content
        // id so the next hover starts fresh — without this, sticky
        // tooltips dismissed via Escape/click-outside would keep
        // their `is_sticky` flag and stale `overlay_id`, and the
        // next hover would never re-show them.
        let mut any_tooltip_dismissed = false;
        for &id in content_ids {
            if let Some(entry) = self.tooltips.iter_mut().find(|e| e.content_id == id) {
                entry.overlay_id = None;
                entry.is_sticky = false;
                entry.hover_start = None;
                entry.real_hover_start = None;
                entry.hover_origin = None;
                entry.shown_at_sim = None;
                entry.shown_at_real = None;
                entry.promoted_by_focus = false;
                if let Some(sink) = entry.shown_at_sink.as_ref() {
                    sink.set(None);
                }
                any_tooltip_dismissed = true;
            }
        }
        if any_tooltip_dismissed {
            let grace = super::TOOLTIP_SESSION_GRACE;
            self.tooltip_session_until_sim = Some(self.sim_clock + grace);
            self.tooltip_session_until_real = Some(std::time::Instant::now() + grace);
        }
        for &id in content_ids {
            let focused_in_subtree = self
                .focused
                .filter(|focused| self.is_descendant_of(*focused, id));
            let hovered_in_subtree = self
                .hovered
                .filter(|hovered| self.is_descendant_of(*hovered, id));

            if let Some(focused) = focused_in_subtree {
                self.dispatch_to_widget(focused, &WidgetEvent::FocusLost, &mut *ops);
                if self
                    .focused
                    .is_some_and(|current| self.is_descendant_of(current, id))
                {
                    let old = self.focused;
                    self.set_focused(None);
                    self.focus_origin = None;
                    self.update_focus_within_signals(old, None);
                    self.update_view_focus_signals(old, None);
                }
            }

            self.arena.set_dormant(id);

            if hovered_in_subtree.is_some() {
                let old = self.hovered;
                self.set_hovered(None);
                self.update_hover_within_signals(old, None);
            }
        }
        if !content_ids.is_empty() {
            self.cached_frame = None;
            self.a11y_dirty = true;
        }
    }

    pub fn is_visible(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;

    #[test]
    fn is_visible_reflects_dormancy() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(tree.is_visible(widget));
        tree.set_dormant(widget);
        assert!(!tree.is_visible(widget));
        tree.activate(widget);
        assert!(tree.is_visible(widget));
    }

    #[test]
    fn frame_tick_deadline_caps_per_frame_effects_at_60hz() {
        let mut tree = WidgetTree::new();
        // Not armed → no per-frame-effect deadline.
        assert!(tree.frame_tick_deadline().is_none());

        // Arm the per-frame-effect path (what Pulse / Cycle / caret blink
        // / drag auto-scroll do via `request_frame` or the render re-arm).
        tree.request_frame();
        // Pace from a known frame time so the interval is assertable.
        let t0 = std::time::Instant::now();
        tree.last_frame_time = Some(t0);

        let deadline = tree
            .frame_tick_deadline()
            .expect("an armed per-frame effect must publish a deadline");
        let interval = deadline.saturating_duration_since(t0);
        // 60 Hz == 16.667 ms; the cap replaces the old ControlFlow::Poll
        // free-run (which rendered at the display's full refresh rate).
        assert!(
            interval >= std::time::Duration::from_micros(16_000)
                && interval <= std::time::Duration::from_micros(17_500),
            "per-frame effects must pace at ~60 Hz (got {interval:?})"
        );

        // It must flow through next_timer_deadline so the event loop uses
        // WaitUntil rather than the removed Poll free-run.
        assert_eq!(
            tree.next_timer_deadline(),
            Some(deadline),
            "frame-tick deadline must be surfaced by next_timer_deadline"
        );
    }

    #[test]
    fn throttled_subscriber_stretches_deadline_but_per_frame_wins_min() {
        use crate::test_widgets::FillWidget;
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        // Cycle-style throttled subscription: wake at most once per 1.5 s.
        let _throttled =
            tree.subscribe_frame_tick_throttled(w, std::time::Duration::from_millis(1500));
        tree.request_frame();
        let t0 = std::time::Instant::now();
        tree.last_frame_time = Some(t0);

        // paint_epoch == 0 (never rendered) → the sentinel treats the
        // subscriber as visible, so its throttled interval governs.
        let d = tree.frame_tick_deadline().expect("armed");
        let dt = d.saturating_duration_since(t0);
        assert!(
            dt >= std::time::Duration::from_millis(1490)
                && dt <= std::time::Duration::from_millis(1510),
            "a lone throttled subscriber must pace at its interval (~1.5 s), got {dt:?}"
        );

        // A per-frame subscriber pulls the *shared* deadline back to 60 Hz:
        // the deadline is the minimum interval across visible subscribers,
        // so a Cycle sharing a tree with a Pulse rides the Pulse's cadence.
        let w2 = tree.add(FillWidget::new());
        let _per_frame = tree.subscribe_frame_tick(w2);
        let d2 = tree.frame_tick_deadline().expect("armed");
        let dt2 = d2.saturating_duration_since(t0);
        assert!(
            dt2 <= std::time::Duration::from_millis(20),
            "a per-frame subscriber must pull the shared deadline to 60 Hz, got {dt2:?}"
        );
    }

    #[test]
    fn show_and_dismiss_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new().label("Overlay"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!(tree.active_overlays().is_empty());

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);

        tree.dismiss_overlay(id);
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content));
    }

    /// A modal (a `Centered` overlay) must count as a host surface so a
    /// `ComboBox` / menu / popover opened *inside* it — which closes via
    /// `dismiss_all_except_hosts` / `dismiss_self_overlay_chain_for_source` —
    /// dismisses only its own cascade and never tears down the hosting modal.
    #[test]
    fn modal_centered_overlay_is_a_host_surface() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let modal_content = tree.add(FillWidget::new().label("Modal"));
        let dropdown_content = tree.add(FillWidget::new().label("Dropdown"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let modal = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: modal_content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Centered,
            dismiss: crate::overlay::DismissBehavior::EscapeOrClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        let dropdown = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: dropdown_content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::EscapeOrClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: Some(modal),
            on_dismiss: None,
            fade_duration: None,
        });

        assert!(
            tree.overlay_is_host_surface(modal),
            "a Centered modal overlay must be treated as a host surface"
        );
        assert!(
            !tree.overlay_is_host_surface(dropdown),
            "a plain dropdown overlay is not a host surface"
        );
    }

    #[test]
    fn escape_dismisses_topmost_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(anchor);

        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::EscapeOrClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Escape, Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content));
    }

    /// Build two stacked overlays (a submenu over its parent menu) so the
    /// nested-overlay "back" key path (`overlay_manager.len() > 1`) is live.
    fn show_two_nested_overlays(tree: &mut WidgetTree) -> (WidgetId, WidgetId) {
        let anchor = tree.add(FillWidget::new());
        let c1 = tree.add(FillWidget::new());
        let c2 = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let o1 = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: c1,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: c2,
            anchor: c1,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: Some(o1),
            on_dismiss: None,
            fade_duration: None,
        });
        (c1, c2)
    }

    #[test]
    fn nested_overlay_back_key_dismisses_with_arrow_left_under_ltr() {
        let mut tree = WidgetTree::new();
        let _ = show_two_nested_overlays(&mut tree);
        assert_eq!(tree.active_overlays().len(), 2);

        // Wrong-direction arrow under LTR leaves both overlays open.
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(tree.active_overlays().len(), 2);

        // ArrowLeft (inline-start under LTR) closes the top nested overlay.
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn nested_overlay_back_key_flips_to_arrow_right_under_rtl() {
        let mut tree = WidgetTree::new();
        tree.set_layout_direction(crate::environment::LayoutDirection::RightToLeft);
        let _ = show_two_nested_overlays(&mut tree);
        assert_eq!(tree.active_overlays().len(), 2);

        // Under RTL, ArrowLeft navigates *into* a submenu — it must NOT
        // dismiss the top overlay.
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(tree.active_overlays().len(), 2);

        // ArrowRight is the inline-start ("back toward parent") key in RTL.
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(tree.active_overlays().len(), 1);
    }

    /// The back key navigates *menu* cascades only — it must never close a
    /// dialog/alert/modal on top. A modal is a scrim+panel overlay pair, so two
    /// stacked modals put two (non-host) scrims in the stack, which inflates the
    /// "nested menu" count; guard on the *topmost* overlay being back-navigable.
    #[test]
    fn back_key_does_not_dismiss_a_dialog_on_top_of_a_modal() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let scrim1 = tree.add(FillWidget::new());
        let scrim2 = tree.add(FillWidget::new());
        let dialog = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        // Two non-host "scrim" overlays (as the two modals' scrims would be)…
        for c in [scrim1, scrim2] {
            tree.show_overlay(crate::overlay::OverlayRequest {
                content_id: c,
                anchor,
                placement: crate::overlay::OverlayPlacement::Below,
                dismiss: crate::overlay::DismissBehavior::Manual,
                layer: crate::overlay::OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            });
        }
        // …with a Centered (host) dialog panel on top.
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: dialog,
            anchor,
            placement: crate::overlay::OverlayPlacement::Centered,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(tree.active_overlays().len(), 3);

        // The nested-menu count is 2 (the scrims), but the top is a host dialog,
        // so the back key must leave it alone (Escape / its buttons dismiss it).
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            3,
            "the back key must not close a dialog sitting on top of a modal"
        );
    }

    #[test]
    fn escape_does_not_dismiss_manual_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(anchor);

        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Escape, Modifiers::NONE);
        // Manual overlays should NOT be dismissed by Escape
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn click_outside_dismisses_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let overlay = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::ClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        tree.overlay_manager
            .set_content_bounds(overlay, bastyde_canvas::Size::new(100.0, 50.0));

        assert_eq!(tree.active_overlays().len(), 1);

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(500.0, 500.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content));
    }

    #[test]
    fn cascade_dismissal() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content_a = tree.add(FillWidget::new());
        let content_b = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let parent = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content_a,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content_b,
            anchor: content_a,
            placement: crate::overlay::OverlayPlacement::TrailingEdge,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: Some(parent),
            on_dismiss: None,
            fade_duration: None,
        });

        assert_eq!(tree.active_overlays().len(), 2);

        tree.dismiss_overlay(parent);
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content_a));
        assert!(!tree.is_visible(content_b));
    }

    #[test]
    fn dismissed_overlay_content_is_dormant_and_invisible() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.dismiss_overlay(id);
        assert!(!tree.is_visible(content));

        tree.layout(SizeProposal::exact(800.0, 600.0));

        let center = tree.bounds(content).center();
        let hit = tree.hit_test(center);
        assert_ne!(hit, Some(content));

        let _frame = tree.render();
        assert!(!tree.is_visible(content));
    }

    #[test]
    fn tooltip_appears_after_delay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tooltip text"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        let center = tree.bounds(anchor).center();
        tree.pointer_move(center);
        assert!(tree.active_overlays().is_empty());

        tree.advance_time(std::time::Duration::from_millis(600));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(tree.find_by_label("Tooltip text").is_some());
    }

    #[test]
    fn tooltip_survives_theme_switch() {
        // Regression: switching themes used to wipe the tooltip
        // registry, so subsequent hovers found nothing to show. Theme
        // changes don't rebuild widgets (they only update the theme
        // signal), so the registry must be preserved.
        let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        tree.set_theme(crate::presets::intui::dark());

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(600));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(tree.find_by_label("Tip").is_some());
    }

    #[test]
    fn tooltip_suppressed_when_hovering_anchor_owned_overlay_content() {
        // Regression: a tooltip attached to an anchor (a ComboBox, a
        // PopoverButton, a menu trigger, …) must not re-trigger while
        // the pointer is over content the anchor opened as an overlay.
        // Those overlays keep their content as an arena child of the
        // anchor (for hit-test / a11y / teardown), so a plain
        // descendant walk would treat hovering a dropdown row as
        // hovering the anchor's own chrome.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        // Overlay content + a row inside it, both arena children of the
        // anchor — exactly the ComboBox dropdown shape.
        let panel = tree.add_child(anchor, FillWidget::new());
        let row = tree.add_child(panel, FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let delay = std::time::Duration::from_millis(100);
        tree.attach_tooltip(anchor, tip, delay);

        // Sanity: hovering the anchor's own chrome starts the timer and
        // the tooltip appears.
        tree.tooltip_pointer_enter(anchor);
        tree.advance_time(delay + std::time::Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear when hovering the anchor itself"
        );
        // Dismiss the first tooltip before the next scenario: move the pointer
        // away and let the 100ms hoverable grace (audit G12) expire.
        tree.pointer_move(Point::new(500.0, 500.0));
        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(tree.active_overlays().is_empty());

        // Open the panel as an overlay anchored to the anchor.
        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: panel,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(tree.active_overlays().len(), 1);

        // Hovering a row inside the overlay must NOT start the anchor's
        // tooltip: the hover lands on overlay content, not anchor chrome.
        tree.tooltip_pointer_enter(row);
        tree.advance_time(delay + std::time::Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "anchor tooltip must not leak onto its own overlay's rows"
        );
    }

    #[test]
    fn tooltip_inside_overlay_still_fires() {
        // The overlay gate must not over-reach: a tooltip whose anchor
        // is *itself* inside the overlay (a dropdown row with its own
        // tooltip) still fires when that row is hovered.
        let mut tree = WidgetTree::new();
        let host = tree.add(FillWidget::new());
        let panel = tree.add_child(host, FillWidget::new());
        let row = tree.add_child(panel, FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Row tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let delay = std::time::Duration::from_millis(100);
        // Anchor is the row, which lives inside the overlay's content.
        tree.attach_tooltip(row, tip, delay);

        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: panel,
            anchor: host,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(tree.active_overlays().len(), 1);

        tree.tooltip_pointer_enter(row);
        tree.advance_time(delay + std::time::Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            2,
            "a row's own tooltip should still fire inside an overlay"
        );
    }

    #[test]
    fn tooltip_dismissed_on_pointer_leave() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(600));
        assert_eq!(tree.active_overlays().len(), 1);

        // WCAG 1.4.13 (Hoverable, audit G12): leaving the anchor no longer
        // dismisses instantly — the pointer might be heading toward the
        // tooltip. The overlay stack's 100ms PointerLeave grace owns dismissal
        // once the pointer is outside BOTH the anchor and the tooltip.
        tree.pointer_move(Point::new(500.0, 500.0));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip persists briefly after anchor-leave (hoverable grace)"
        );
        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(
            tree.active_overlays().is_empty(),
            "tooltip dismissed after the 100ms grace outside anchor+overlay"
        );
    }

    #[test]
    fn tooltip_not_shown_if_pointer_leaves_before_delay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(200));
        tree.pointer_move(Point::new(500.0, 500.0));

        tree.advance_time(std::time::Duration::from_millis(500));
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn tooltip_reshow_uses_short_delay_after_prior_tip() {
        // Windows TTDT_RESHOW: while a tip is open (or just dismissed), the
        // next anchor pays the short reshow delay, not the full initial delay.
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let tip_a = tree.add(FillWidget::new().label("Tip A"));
        let tip_b = tree.add(FillWidget::new().label("Tip B"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let delay = std::time::Duration::from_millis(500);
        tree.attach_tooltip(a, tip_a, delay);
        tree.attach_tooltip(b, tip_b, delay);

        // First tip: full initial delay (drive via enter so anchors may
        // share layout bounds without hit-test ambiguity).
        tree.tooltip_pointer_enter(a);
        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(
            tree.active_overlays().is_empty(),
            "must not appear before full initial delay"
        );
        tree.advance_time(std::time::Duration::from_millis(400));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "first tip after initial delay"
        );
        assert!(tree.find_by_label("Tip A").is_some());

        // While A is still shown the reshow session is active — B uses 100 ms.
        let mut noop = crate::window::NoopWindowOps;
        tree.tooltip_pointer_leave(a, &mut noop);
        tree.tooltip_pointer_enter(b);
        tree.advance_time(std::time::Duration::from_millis(120));
        assert!(
            tree.find_by_label("Tip B").is_some(),
            "second tip should use the short reshow delay while session is warm"
        );
    }

    #[test]
    fn reshow_delay_reverts_to_full_after_the_session_grace_expires() {
        // The reshow session is a *session*: once the last tip has been gone
        // for TOOLTIP_SESSION_GRACE, a fresh hover is a new deliberate act and
        // pays the full delay again. Regression guard for a grace that is
        // never cleared (permanent 100 ms flash on every control) or cleared
        // too eagerly (the toolbar sweep loses its snappiness).
        let mut tree = WidgetTree::new();
        // Reduced motion removes the fade-out, so the dismissal — and with it
        // the start of the grace window — lands on the pass that dismisses
        // rather than on the one that finishes the tween.
        tree.set_accessibility_preferences(false, true, 1.0);
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let tip_a = tree.add(FillWidget::new().label("Tip A"));
        let tip_b = tree.add(FillWidget::new().label("Tip B"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let delay = std::time::Duration::from_millis(500);
        tree.attach_tooltip(a, tip_a, delay);
        tree.attach_tooltip(b, tip_b, delay);

        // Warm the session, then close A and let the grace run out.
        tree.tooltip_pointer_enter(a);
        tree.advance_time(std::time::Duration::from_millis(550));
        assert!(tree.find_by_label("Tip A").is_some(), "first tip shown");

        tree.pointer_move(Point::new(900.0, 900.0));
        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(tree.active_overlays().is_empty(), "A dismissed on leave");

        // Past the 1 s grace with nothing shown, the session is cold.
        tree.advance_time(super::TOOLTIP_SESSION_GRACE + std::time::Duration::from_millis(50));

        tree.tooltip_pointer_enter(b);
        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(
            tree.active_overlays().is_empty(),
            "session went cold — B must pay the FULL delay, not the 100 ms reshow"
        );
        tree.advance_time(std::time::Duration::from_millis(400));
        assert!(
            tree.find_by_label("Tip B").is_some(),
            "B still appears once its full delay elapses"
        );
    }

    #[test]
    fn warm_reshow_scales_the_delay_rather_than_flattening_every_tier() {
        // The reshow shortcut is proportional (Windows TTDT_RESHOW =
        // TTDT_INITIAL / 5), not an absolute floor. A *heavy* 700 ms entry
        // exists because its content needs a longer statement of intent, so on
        // the warm path it must reshow at 140 ms — not collapse to the light
        // tier's 100 ms.
        let mut tree = WidgetTree::new();
        let light = tree.add(FillWidget::new());
        let heavy = tree.add(FillWidget::new());
        let tip_light = tree.add(FillWidget::new().label("Light"));
        let tip_heavy = tree.add(FillWidget::new().label("Heavy"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.attach_tooltip(light, tip_light, std::time::Duration::from_millis(500));
        tree.attach_tooltip(heavy, tip_heavy, std::time::Duration::from_millis(700));

        // Warm the session with the light tip and leave it open.
        tree.tooltip_pointer_enter(light);
        tree.advance_time(std::time::Duration::from_millis(550));
        assert!(tree.find_by_label("Light").is_some(), "session warm");

        let mut noop = crate::window::NoopWindowOps;
        tree.tooltip_pointer_leave(light, &mut noop);
        tree.tooltip_pointer_enter(heavy);

        // 120 ms would have been enough under the old flat 100 ms clamp.
        tree.advance_time(std::time::Duration::from_millis(120));
        assert!(
            tree.find_by_label("Heavy").is_none(),
            "a heavy tooltip must not fire at the light tier's reshow delay"
        );
        // 700 * (100/500) = 140 ms.
        tree.advance_time(std::time::Duration::from_millis(40));
        assert!(
            tree.find_by_label("Heavy").is_some(),
            "heavy reshow is the scaled 140 ms"
        );
    }

    #[test]
    fn a_pinned_sticky_tooltip_does_not_hold_the_session_warm() {
        // A sticky tip survives pointer-leave and stays up until Escape or a
        // click outside. Counting it as an active session would put every
        // other anchor on the 100 ms path for as long as it is pinned.
        let mut tree = WidgetTree::new();
        tree.set_accessibility_preferences(false, true, 1.0); // no fade deferral
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let tip_a = tree.add(FillWidget::new().label("Pinned"));
        let tip_b = tree.add(FillWidget::new().label("Other"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.attach_tooltip_with_sticky(
            a,
            tip_a,
            std::time::Duration::from_millis(500),
            Some(std::time::Duration::from_millis(100)),
        );
        tree.attach_tooltip(b, tip_b, std::time::Duration::from_millis(500));

        tree.tooltip_pointer_enter(a);
        tree.advance_time(std::time::Duration::from_millis(550));
        assert!(tree.find_by_label("Pinned").is_some());
        tree.promote_tooltip_to_sticky(tip_a);

        // Let the dismiss-grace from nothing elapse, then hover B.
        tree.advance_time(super::TOOLTIP_SESSION_GRACE + std::time::Duration::from_millis(50));
        tree.tooltip_pointer_enter(b);
        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(
            tree.find_by_label("Other").is_none(),
            "a pinned sticky must not keep every other anchor on the reshow path"
        );
    }

    #[test]
    fn reattaching_a_tooltip_does_not_grow_the_entry_table() {
        // `attach_tooltip*` is called from `build()`, so it re-runs on every
        // rebuild. An anchor owns at most one tooltip: without retirement the
        // table gains a dead row (and an orphaned, parentless content node) per
        // rebuild, forever — and it is scanned on every pointer move, four
        // times per layout pass, and once per widget in the a11y walk.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let delay = std::time::Duration::from_millis(100);
        for _ in 0..25 {
            // Each "rebuild" mints a fresh content widget, as `ctx.add` does.
            let tip = tree.add(FillWidget::new().label("Tip"));
            tree.attach_tooltip(anchor, tip, delay);
            assert_eq!(
                tree.tooltip_entry_count(),
                1,
                "an anchor must own exactly one tooltip entry across rebuilds"
            );
        }

        // The surviving entry is the newest one and still works.
        tree.tooltip_pointer_enter(anchor);
        tree.advance_time(delay + std::time::Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "latest tooltip still shows"
        );
    }

    #[test]
    fn destroying_an_anchor_reaps_its_tooltip_entry() {
        // The content widget is parentless (`ctx.add`), so the anchor's own
        // subtree teardown never reaches it.
        let mut tree = WidgetTree::new();
        let host = tree.add(FillWidget::new());
        let anchor = tree.add_child(host, FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(100));
        assert_eq!(tree.tooltip_entry_count(), 1);

        tree.destroy_subtree(anchor);
        assert_eq!(
            tree.tooltip_entry_count(),
            0,
            "destroying the anchor must reap its entry and content node"
        );
    }

    #[test]
    fn a_leaving_tooltip_schedules_a_wake_for_its_dismissal() {
        // The pointer's last motion event only *starts* the PointerLeave
        // grace. Without a deadline for its end the loop parks in
        // `ControlFlow::Wait` and the tooltip hangs on screen until unrelated
        // input redraws the window.
        let mut tree = WidgetTree::new();
        tree.set_accessibility_preferences(false, true, 1.0); // no fade deadline
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(100));
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(150));
        assert_eq!(tree.active_overlays().len(), 1, "tooltip shown");

        // Shown, pointer still inside: nothing pending, so no deadline.
        assert!(
            tree.next_timer_deadline().is_none(),
            "a settled tooltip under the pointer schedules nothing"
        );

        // Pointer leaves — now the 100 ms grace is running and MUST be a wake
        // source, or nothing will ever dismiss the tooltip on an idle app.
        tree.pointer_move(Point::new(900.0, 900.0));
        assert!(
            tree.next_timer_deadline().is_some(),
            "the PointerLeave grace must contribute a wake deadline"
        );
    }

    #[test]
    fn pressing_cancels_a_pending_dwell_and_dismisses_a_shown_tooltip() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let delay = std::time::Duration::from_millis(500);
        tree.attach_tooltip(anchor, tip, delay);

        // Press partway through the dwell: the user has answered their own
        // question, so the tip must not arrive afterwards.
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(300));
        tree.tooltip_pointer_press(None);
        tree.advance_time(std::time::Duration::from_millis(400));
        assert!(
            tree.active_overlays().is_empty(),
            "a press must cancel the pending dwell, not merely delay it"
        );

        // And a press while one is shown retires it rather than leaving it
        // covering the control that was just clicked.
        tree.tooltip_pointer_enter(anchor);
        tree.advance_time(delay + std::time::Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1, "tooltip shown again");
        tree.tooltip_pointer_press(Some(tree.bounds(anchor).center()));
        assert!(
            tree.active_overlays().is_empty(),
            "a press must dismiss the shown tooltip"
        );
    }

    #[test]
    fn window_deactivation_retires_hover_tooltips() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(100));
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(150));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.set_window_active(false);
        assert!(
            tree.active_overlays().is_empty(),
            "a tooltip must not float over another window's chrome"
        );
    }

    #[test]
    fn only_the_innermost_anchor_arms_its_dwell() {
        // A row inside a panel, both with tooltips. Arming both would mature
        // two tips and stack them on top of each other.
        let mut tree = WidgetTree::new();
        let panel = tree.add(FillWidget::new());
        let row = tree.add_child(panel, FillWidget::new());
        let panel_tip = tree.add(FillWidget::new().label("Panel"));
        let row_tip = tree.add(FillWidget::new().label("Row"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let delay = std::time::Duration::from_millis(100);
        tree.attach_tooltip(panel, panel_tip, delay);
        tree.attach_tooltip(row, row_tip, delay);

        tree.tooltip_pointer_enter(row);
        tree.advance_time(delay + std::time::Duration::from_millis(50));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "exactly one tooltip may open for a hover"
        );
        assert!(
            tree.find_by_label("Row").is_some(),
            "the innermost anchor wins"
        );
    }

    #[test]
    fn escape_dismisses_a_hover_tooltip_and_falls_through_to_the_menu_below() {
        // WCAG 2.2 SC 1.4.13(a): hover content must be dismissible without
        // moving the pointer. And a tooltip raised over an open menu must not
        // swallow the Escape meant for the menu underneath.
        let mut tree = WidgetTree::new();
        tree.set_accessibility_preferences(false, true, 1.0); // no fade deferral
        let anchor = tree.add(FillWidget::new());
        let menu = tree.add(FillWidget::new());
        let tip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let menu_overlay = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: menu,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::EscapeOrClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(tree.active_overlays().len(), 1);

        // Raise a tooltip on top of the menu.
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(100));
        tree.tooltip_pointer_enter(anchor);
        tree.advance_time(std::time::Duration::from_millis(150));
        assert_eq!(
            tree.active_overlays().len(),
            2,
            "tooltip sits above the menu"
        );

        // First Escape takes the tooltip...
        let dismissed = tree.overlay_manager.try_dismiss_top_on_escape();
        assert!(dismissed.is_some(), "Escape must dismiss the hover tooltip");
        assert_eq!(tree.active_overlays().len(), 1);

        // ...the second reaches the menu, which was previously unreachable.
        let dismissed = tree.overlay_manager.try_dismiss_top_on_escape();
        assert_eq!(
            dismissed.map(|(id, _, _)| id),
            Some(menu_overlay),
            "Escape must then reach the menu underneath"
        );
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn tooltip_timer_restarts_when_pointer_keeps_moving() {
        // Stationary-pointer filter: travel beyond ~4 px from hover origin
        // restarts the delay so a sweeping cursor does not pop tips.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        let start = tree.bounds(anchor).center();
        tree.pointer_move(start);
        tree.advance_time(std::time::Duration::from_millis(300));
        // Still pending; move well past the 4 px slop inside the same anchor.
        tree.pointer_move(Point::new(start.x + 20.0, start.y));
        // Another 300 ms would have completed the *original* 500 ms timer,
        // but the restart means we need a full 500 ms from the move.
        tree.advance_time(std::time::Duration::from_millis(300));
        assert!(
            tree.active_overlays().is_empty(),
            "moving past stationary slop must restart the delay"
        );
        tree.advance_time(std::time::Duration::from_millis(250));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip appears after a full delay of stillness"
        );
    }

    #[test]
    fn timed_overlay_auto_dismisses_after_duration() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new().label("Toast"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.show_overlay_for(
            crate::overlay::OverlayRequest {
                content_id: content,
                anchor,
                placement: crate::overlay::OverlayPlacement::Below,
                dismiss: crate::overlay::DismissBehavior::Manual,
                layer: crate::overlay::OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            },
            std::time::Duration::from_millis(300),
        );

        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(std::time::Duration::from_millis(200));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content));
    }

    #[test]
    fn fade_dismiss_keeps_overlay_off_active_list_immediately() {
        // The user-facing `active_overlays()` accessor reports a
        // dismissing-with-fade overlay as gone the moment dismiss is
        // requested — even though the fade-out tween is still
        // playing under the hood. Caller code asking "is this
        // overlay still up?" gets the expected answer; the framework
        // reaps the actual content on the next layout pass past the
        // tween deadline.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new().label("Faded"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: Some(std::time::Duration::from_millis(100)),
        });
        assert_eq!(tree.active_overlays().len(), 1);

        tree.dismiss_overlay(id);
        // Reported as gone immediately, even though the content
        // widget is still active and painting the fade-out tween —
        // the deferred removal happens later in
        // process_overlay_fade_dismissals_real.
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn fade_dismiss_defers_content_dormancy_until_sim_tween_completes() {
        // Sim-clock variant of the fade-defer contract: dismiss kicks
        // off the fade-out tween and stamps both real- and sim-time
        // start markers. `advance_time` (sim-clock) past the tween
        // duration flushes the deferred removal via
        // `process_overlay_fade_dismissals_sim`.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new().label("Faded"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: Some(std::time::Duration::from_millis(100)),
        });
        tree.dismiss_overlay(id);
        assert!(
            tree.is_visible(content),
            "content stays active during fade-out"
        );

        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(
            !tree.is_visible(content),
            "after sim-time past the tween window, deferred removal fires"
        );
    }
}
