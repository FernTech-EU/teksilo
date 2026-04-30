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
        self.attach_tooltip_inner(anchor_id, content_id, delay, None, None);
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
        self.attach_tooltip_inner(anchor_id, content_id, delay, sticky_after, None);
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
        );
    }

    fn attach_tooltip_inner(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
        shown_at_sink: Option<std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>>,
    ) {
        self.arena.set_dormant(content_id);
        self.tooltips.push(TooltipEntry {
            anchor_id,
            content_id,
            delay,
            hover_start: None,
            real_hover_start: None,
            overlay_id: None,
            sticky_after,
            is_sticky: false,
            shown_at_sim: None,
            shown_at_real: None,
            shown_at_sink,
            promoted_by_focus: false,
        });
    }

    pub(super) fn process_tooltips(&mut self) {
        let sim_now = self.sim_clock;
        self.process_tooltips_impl(|entry| {
            entry
                .hover_start
                .map(|start| sim_now.saturating_duration_since(start))
        });
    }

    pub(super) fn process_tooltips_real(&mut self) {
        let real_now = std::time::Instant::now();
        self.process_tooltips_impl(|entry| {
            entry
                .real_hover_start
                .map(|start| real_now.saturating_duration_since(start))
        });
    }

    fn process_tooltips_impl(
        &mut self,
        elapsed_fn: impl Fn(&TooltipEntry) -> Option<std::time::Duration>,
    ) {
        let mut to_show = Vec::new();
        for entry in &mut self.tooltips {
            if entry.overlay_id.is_some() {
                continue;
            }
            if !self.arena.is_active(entry.anchor_id) {
                continue;
            }
            if let Some(elapsed) = elapsed_fn(entry)
                && elapsed >= entry.delay
            {
                to_show.push((entry.anchor_id, entry.content_id));
                entry.hover_start = None;
                entry.real_hover_start = None;
            }
        }
        let sim_now = self.sim_clock;
        let real_now = std::time::Instant::now();
        // Tooltips fade in over `duration_fast` (~120 ms) — matches the
        // MotionTokens recommendation for "tooltip fade, popup fade".
        // Reduced-motion users get an instant snap.
        let fade_duration = if self.prefers_reduced_motion {
            None
        } else {
            Some(self.theme.motion.duration_fast)
        };
        for (anchor_id, content_id) in to_show {
            self.arena.activate(content_id);
            let oid = self.show_overlay(crate::overlay::OverlayRequest {
                content_id,
                anchor: anchor_id,
                placement: crate::overlay::OverlayPlacement::NearAnchor {
                    offset: fern_canvas::Vec2::new(0.0, 8.0),
                },
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

    pub(super) fn process_delayed_overlays_real(
        &mut self,
        ops: &mut dyn crate::window::WindowOps,
    ) {
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

        for pending in ready {
            let content_id = pending.request.content_id;
            self.arena.activate(content_id);
            let current_focus = self.focused;
            self.overlay_manager.show(pending.request);
            if let Some(focus_id) = current_focus {
                self.overlay_manager.set_top_focus_restore(focus_id);
            }
            self.arena.mark_needs_paint(content_id);
            if let Some(focus_target) = pending.focus_target {
                if self.arena.is_active(focus_target) {
                    self.focus_ops(focus_target, &mut *ops);
                }
            }
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
            if matches!(overlay.placement, crate::overlay::OverlayPlacement::Centered) {
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
            if let Some(restore_id) = focus_restore {
                if self.arena.is_active(restore_id) {
                    self.focus_ops(restore_id, &mut *ops);
                }
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
            if let Some(restore_id) = focus_restore {
                if self.arena.is_active(restore_id) {
                    self.focus_ops(restore_id, &mut *ops);
                }
            }
        }
    }

    pub(super) fn dismiss_modal_for_source(
        &mut self,
        source_widget: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let Some(modal_overlay) = self.modal_overlay_for_widget(source_widget) else {
            return false;
        };

        let (dismissed, focus_restore) = self.overlay_manager.dismiss_with_focus_restore(modal_overlay);
        self.dormant_dismissed_content(&dismissed, &mut *ops);
        if let Some(restore_id) = focus_restore {
            if self.arena.is_active(restore_id) {
                self.focus_ops(restore_id, &mut *ops);
            }
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
        // Composite widgets (e.g. `Button`) attach their tooltip to an
        // *inner* subtree root, then keep focus on the outer widget
        // itself. Accept either direction of the ancestor relationship
        // so "focus landed anywhere inside the anchor's scope" fires
        // correctly regardless of whether the anchor is the focusable
        // node or one of its descendants.
        let to_show: Vec<(WidgetId, WidgetId)> = self
            .tooltips
            .iter()
            .filter(|e| {
                e.sticky_after.is_some()
                    && e.overlay_id.is_none()
                    && (self.is_descendant_of(widget_id, e.anchor_id)
                        || self.is_descendant_of(e.anchor_id, widget_id))
            })
            .map(|e| (e.anchor_id, e.content_id))
            .collect();

        let sim_now = self.sim_clock;
        let real_now = std::time::Instant::now();
        // Sticky tooltips share the standard tooltip fade.
        let fade_duration = if self.prefers_reduced_motion {
            None
        } else {
            Some(self.theme.motion.duration_fast)
        };
        for (anchor_id, content_id) in to_show {
            self.arena.activate(content_id);
            let oid = self.show_overlay(crate::overlay::OverlayRequest {
                content_id,
                anchor: anchor_id,
                placement: crate::overlay::OverlayPlacement::NearAnchor {
                    offset: fern_canvas::Vec2::new(0.0, 8.0),
                },
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

    pub(super) fn tooltip_pointer_enter(&mut self, widget_id: WidgetId) {
        let matching: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.is_descendant_of(widget_id, entry.anchor_id))
            .map(|(index, _)| index)
            .collect();
        let now = self.sim_clock;
        let real_now = std::time::Instant::now();
        for index in matching {
            self.tooltips[index].hover_start = Some(now);
            self.tooltips[index].real_hover_start = Some(real_now);
            self.arena.mark_needs_paint(self.tooltips[index].anchor_id);
        }
    }

    /// Promote a shown tooltip from "ephemeral hover" to "sticky".
    ///
    /// - Flags the tooltip entry as sticky so
    ///   [`tooltip_pointer_leave`](Self::tooltip_pointer_leave)
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
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let matching: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.is_descendant_of(widget_id, entry.anchor_id))
            .map(|(index, _)| index)
            .collect();
        let mut to_dismiss = Vec::new();
        for index in matching {
            self.tooltips[index].hover_start = None;
            self.tooltips[index].real_hover_start = None;
            // Sticky tooltips (post-dwell-promotion) survive
            // pointer-leave — the user dismisses them via Escape
            // or click-outside via the overlay's dismiss behavior.
            if self.tooltips[index].is_sticky {
                continue;
            }
            if let Some(id) = self.tooltips[index].overlay_id.take() {
                if let Some(sink) = self.tooltips[index].shown_at_sink.as_ref() {
                    sink.set(None);
                }
                self.tooltips[index].shown_at_sim = None;
                self.tooltips[index].shown_at_real = None;
                to_dismiss.push((id, self.tooltips[index].content_id));
            }
        }
        for (overlay_id, _content_id) in to_dismiss {
            let dismissed = self.overlay_manager.dismiss(overlay_id);
            self.dormant_dismissed_content(&dismissed, &mut *ops);
        }
    }

    /// Returns the earliest deadline for a pending tooltip or delayed overlay (if any).
    pub fn next_timer_deadline(&self) -> Option<std::time::Instant> {
        let tooltip_deadline = self
            .tooltips
            .iter()
            .filter(|entry| entry.overlay_id.is_none())
            .filter_map(|entry| entry.real_hover_start.map(|start| start + entry.delay))
            .min();

        // Sticky-on-dwell wake-ups: once a rich tooltip has been
        // shown, the dwell-promotion timer needs to keep ticking
        // every 500 ms so the visible step indicator advances and the
        // 2 s promotion eventually fires. Without these deadlines the
        // framework would only wake on user input, leaving the dwell
        // counter stuck at 0.
        let now = std::time::Instant::now();
        let dwell_step = std::time::Duration::from_millis(500);
        let dwell_tooltip_deadline = self
            .tooltips
            .iter()
            .filter_map(|entry| {
                let sticky_after = entry.sticky_after?;
                let shown_at = entry.shown_at_real?;
                if entry.overlay_id.is_none() || entry.is_sticky {
                    return None;
                }
                let elapsed = now.saturating_duration_since(shown_at);
                if elapsed >= sticky_after {
                    return None;
                }
                // Round up to the next step boundary so each
                // wake-up lands on a 500 ms / 1 s / 1.5 s / 2 s mark.
                let steps_passed = (elapsed.as_millis() / dwell_step.as_millis()) as u32;
                let next_step_at =
                    shown_at + dwell_step * (steps_passed + 1);
                Some(next_step_at.min(shown_at + sticky_after))
            })
            .min();
        let delayed_overlay_deadline = self
            .pending_delayed_overlays
            .iter()
            .map(|pending| pending.real_requested_at + pending.delay)
            .min();
        let auto_dismiss_deadline = self.overlay_manager.next_auto_dismiss_deadline();
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

        [
            tooltip_deadline,
            dwell_tooltip_deadline,
            delayed_overlay_deadline,
            auto_dismiss_deadline,
            animation_deadline,
            animated_quad_deadline,
            gesture_deadline,
            wake_at_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn overlay_manager(&self) -> &crate::overlay::OverlayManager {
        &self.overlay_manager
    }

    pub fn active_overlays(&self) -> Vec<crate::overlay::OverlayId> {
        self.overlay_manager.active_ids()
    }

    pub fn show_overlay(
        &mut self,
        request: crate::overlay::OverlayRequest,
    ) -> crate::overlay::OverlayId {
        let fade_duration = request.fade_duration;
        let content_id = request.content_id;
        let id = self.overlay_manager.show(request);
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
        let _ = opacity.try_animate_with_options(crate::animation::AnimationRequest {
            target: 1.0,
            duration,
            easing: fern_tokens::Easing::EaseOut,
            frame_interval: None,
            looping: false,
            epsilon: 0.0,
            max_duration: None,
        });
        self.overlay_manager.attach_fade(overlay_id, opacity, duration);
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
        for &id in content_ids {
            if let Some(entry) = self
                .tooltips
                .iter_mut()
                .find(|e| e.content_id == id)
            {
                entry.overlay_id = None;
                entry.is_sticky = false;
                entry.hover_start = None;
                entry.real_hover_start = None;
                entry.shown_at_sim = None;
                entry.shown_at_real = None;
                entry.promoted_by_focus = false;
                if let Some(sink) = entry.shown_at_sink.as_ref() {
                    sink.set(None);
                }
            }
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
                    self.focused = None;
                    self.focus_origin = None;
                }
            }

            self.arena.set_dormant(id);

            if hovered_in_subtree.is_some() {
                self.hovered = None;
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
            .set_content_bounds(overlay, fern_canvas::Size::new(100.0, 50.0));

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
    fn tooltip_dismissed_on_pointer_leave() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(600));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.pointer_move(Point::new(500.0, 500.0));
        assert!(tree.active_overlays().is_empty());
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
        assert!(tree.is_visible(content), "content stays active during fade-out");

        tree.advance_time(std::time::Duration::from_millis(150));
        assert!(
            !tree.is_visible(content),
            "after sim-time past the tween window, deferred removal fires"
        );
    }
}
