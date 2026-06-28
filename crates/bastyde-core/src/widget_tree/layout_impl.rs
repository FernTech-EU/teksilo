// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

impl WidgetTree {
    /// Process dirty state bindings: mark bound widgets for repaint, relayout,
    /// or rebuild. Called automatically at the start of layout().
    pub(super) fn process_state_changes(&mut self, ops: &mut dyn crate::window::WindowOps) {
        // One unified flush: both visual buckets and the a11y flag
        // are drained from the same walk, so a signal bound at both
        // a visual level and `AccessibilityOnly` (e.g. a Button's
        // `bind_label` re-registers the same Signal at RepaintOnly
        // *and* AccessibilityOnly) flips both. Two separate flushes
        // would race on the shared per-Signal dirty flag and the
        // second flush would always see it cleared.
        let (dirty_widgets, a11y_binding_dirty) = self.binding_registry.flush_all_dirty();
        for (id, level) in &dirty_widgets {
            match level {
                crate::binding::BindingLevel::RepaintOnly => {
                    self.arena.mark_needs_paint(*id);
                }
                crate::binding::BindingLevel::SubtreeRepaint => {
                    // Used by `enabled_when` so the leaves in the
                    // disabled subtree re-resolve their role colors
                    // via the paint walker's `effective_enabled`.
                    // No layout work — geometry is unchanged.
                    self.arena.mark_subtree_needs_paint(*id);
                }
                crate::binding::BindingLevel::Relayout => {
                    self.arena.mark_needs_layout(*id);
                    self.arena.mark_ancestors_need_layout(*id);
                }
                crate::binding::BindingLevel::Rebuild => {
                    self.arena.mark_needs_rebuild(*id);
                    self.arena.mark_ancestors_need_layout(*id);
                }
                crate::binding::BindingLevel::AccessibilityOnly => {
                    // Drained into the boolean below — never appears in
                    // the visual map, but kept in the match so a future
                    // variant addition is a compile-time reminder.
                }
            }
        }

        // Orthogonal to the visual dirty pass: if any signal bound at
        // `BindingLevel::AccessibilityOnly` fired, flip the tree-wide
        // `a11y_dirty` flag so the next `sync_accessibility` rebuilds
        // the AccessKit tree. Decoupled from layout / paint so a text
        // edit that changes no visual geometry still reaches screen
        // readers within one frame.
        if a11y_binding_dirty {
            self.a11y_dirty = true;
        }

        // Rebuild data-driven widgets whose data model changed.
        self.process_pending_rebuilds(&mut *ops);

        let mut to_dormant = Vec::new();
        let mut to_activate = Vec::new();
        for (id, is_active, should_be_visible) in self.arena.visibility_checks_iter() {
            if is_active && !should_be_visible {
                to_dormant.push(id);
            } else if !is_active && should_be_visible {
                // Only wake a `visible_when(true)` node whose parent is active.
                // A gated node inside a dormant ancestor (e.g. a row in a
                // closed popover / overflow menu) must NOT escape that
                // ancestor's dormancy and render on its own. When the ancestor
                // is later activated, `arena.activate` wakes this node via the
                // cascade (its gate is true). The dormancy invariant — an
                // active node has an active parent — makes the immediate-parent
                // check sufficient.
                let parent_active = self
                    .arena
                    .parent(id)
                    .map(|p| self.arena.is_active(p))
                    .unwrap_or(true);
                if parent_active {
                    to_activate.push(id);
                }
            }
        }
        // The accessibility walk skips dormant nodes, so any
        // active↔dormant transition changes the AccessKit tree shape
        // and must dirty the cached snapshot. Other Relayout-causing
        // signal flips (e.g. a Switcher visibility binding that doesn't
        // straddle activation, an opacity change, a text-width change)
        // do not change the AT tree — the unconditional `a11y_dirty = true`
        // was removed from `layout()` and is now set only by events that
        // actually change the AT tree shape.
        if !to_dormant.is_empty() || !to_activate.is_empty() {
            self.a11y_dirty = true;
        }
        for id in to_dormant {
            self.arena.set_dormant(id);
        }
        for id in to_activate {
            self.arena.activate(id);
        }
        // Fire activation_signal observers (e.g. a WebView's set_visible
        // bridge) after the whole visibility pass has committed — not from
        // inside the set_dormant/activate recursion above.
        self.flush_activation_signals();
    }

    /// Dismiss any active overlay whose content widget is no longer
    /// alive in the arena. An overlay's owner can be torn down
    /// out-of-band: a data-driven rebuild destroys the widget that
    /// showed it (clicking "mark all read" inside a notification popover
    /// rebuilds the bell that owns the overlay; closing a document tears
    /// down a still-open inline popover). The content then disappears
    /// visually, but the overlay ENTRY survives in the manager and keeps
    /// intercepting clicks (the click-outside scrim) until the user
    /// clicks elsewhere. This GC removes such orphans immediately (no
    /// fade — the content is already gone). A normally-open overlay's
    /// content stays active (gated `true`), so it is never touched.
    pub(super) fn gc_orphaned_overlays(&mut self) {
        let orphaned: Vec<crate::overlay::OverlayId> = self
            .overlay_manager
            .active_ids()
            .into_iter()
            .filter(|&id| {
                self.overlay_manager
                    .overlay(id)
                    .map(|o| !self.arena.is_active(o.content_id))
                    .unwrap_or(false)
            })
            .collect();
        for id in orphaned {
            self.overlay_manager.dismiss_immediate(id);
        }
    }

    /// Drain any widgets flagged `needs_rebuild` that are currently
    /// active + have built children. Called from
    /// `process_state_changes` after dirty bindings have been
    /// flushed, and again after overlay / tooltip activation so that
    /// widgets transitioning from dormant → active in the same
    /// layout pass get rebuilt *this* frame rather than the next.
    pub(super) fn process_pending_rebuilds(&mut self, ops: &mut dyn crate::window::WindowOps) {
        // Defer *selected* rebuilds during the gesture-arena latch
        // window: from `PointerDown` (which stores the press position
        // in the captured widget's arena) until either `PointerUp` or
        // the arena fires `DragStarted`. Rebuilding the captured
        // widget, or any of its ancestors, would destroy that arena
        // and lose the press state — the recognizer would never fire.
        //
        // Rebuilds targeting widgets *outside* the captured widget's
        // ancestor chain are safe: destroying sibling subtrees leaves
        // the captured widget intact, so ongoing drags keep routing
        // correctly. This matters for e.g. a virtualized `ListView`
        // inside a ComboBox panel whose scrollbar sibling is capturing
        // the pointer — the list must rebuild when scroll crosses its
        // buffer, but the scrollbar itself (held elsewhere in the
        // tree) must not be torn down mid-drag.
        //
        // Once `active_drag` is set, the framework routes PointerMove /
        // PointerUp via `handle_drag_move` / `handle_drag_drop` keyed
        // on the `DragSession`, not on the captured widget's arena —
        // so a mid-drag rebuild is safe regardless of topology. Post-
        // rebuild, `revalidate_interaction_state` clears a now-stale
        // `pointer_captured_by`; subsequent events hit-test normally.
        let to_rebuild_all = self.arena.collect_needs_rebuild();
        if to_rebuild_all.is_empty() {
            self.revalidate_interaction_state(&mut *ops);
            return;
        }
        let captured_ancestors: Option<Vec<WidgetId>> = if self.active_drag.is_none() {
            self.pointer_captured_by.map(|cap| {
                let mut ids = vec![cap];
                let mut cur = self.arena.parent(cap);
                while let Some(id) = cur {
                    ids.push(id);
                    cur = self.arena.parent(id);
                }
                ids
            })
        } else {
            None
        };
        let to_rebuild: Vec<WidgetId> = match &captured_ancestors {
            Some(chain) => to_rebuild_all
                .into_iter()
                .filter(|id| !chain.contains(id))
                .collect(),
            None => to_rebuild_all,
        };
        if to_rebuild.is_empty() {
            self.revalidate_interaction_state(&mut *ops);
            return;
        }
        for widget_id in to_rebuild {
            self.rebuild_single_widget(widget_id);
        }
        // Rebuild destroys old child subtrees and allocates fresh WidgetIds;
        // drop any focus/hover state whose target is no longer valid so we
        // don't dispatch to dead widgets on the next event.
        self.revalidate_interaction_state(&mut *ops);
        // A rebuild's `build()` may arm new animations (looping or
        // one-shot) by calling `signal.animate_to(...)` /
        // `animate_looping(...)` — these set `pending` on the signal
        // but don't enter the scheduler until `process_pending_animations`
        // runs again. The early-frame `process_pending_animations`
        // (`layout_impl::layout_with_ops`) already ran *before* this
        // rebuild, so without this second drain the animation would
        // wait for the next frame; if the rebuild also cancelled
        // existing scheduler entries (`cancel_by_widget` is called by
        // `rebuild_single_widget`), the scheduler ends up empty, no
        // frame deadline is set, and the freshly-armed animation
        // *never* gets picked up — the user sees animations freeze
        // after any state-driven rebuild that re-arms them
        // (e.g. SceneView's drag-end rebuild re-arming PulsingDot
        // loopers via `register_bindings`).
        self.process_pending_animations();
    }

    /// Run the layout pass with the given size proposal, using
    /// [`NoopWindowOps`](crate::window::NoopWindowOps). Handlers
    /// triggered from drag_tick / tooltip activation / etc cannot
    /// call `ctx.open_window(...)` from this path.
    ///
    /// `bastyde-app` calls [`layout_with_ops`](Self::layout_with_ops)
    /// with a real sink so those handlers can open windows.
    pub fn layout(&mut self, proposal: SizeProposal) {
        let mut noop = crate::window::NoopWindowOps;
        self.layout_with_ops(proposal, &mut noop);
    }

    /// Run the layout pass with the given size proposal, threading
    /// the app's [`WindowOps`](crate::window::WindowOps) sink
    /// through to drag_tick / tooltip / delayed-overlay handlers.
    pub fn layout_with_ops(
        &mut self,
        proposal: SizeProposal,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        self.process_pending_animations();

        let now = std::time::Instant::now();
        // Deadline-driven wake-up: if a widget requested a future
        // frame via `wake_at_handle()` and that deadline is now past,
        // arm the frame tick so its effect runs on this layout pass.
        // Used by the rich text editor's caret blink to avoid
        // keeping winit in Poll mode.
        if let Some(deadline) = self.pending_wake_at.get()
            && deadline <= now
        {
            self.pending_wake_at.set(None);
            self.frame_tick_requested.set(true);
        }
        self.advance_frame_tick(now);
        self.animation_scheduler
            .tick(now, &self.arena, self.paint_epoch);

        // Fire on_drag_tick on the current drop target, if any. Runs once
        // per layout pass so widgets can implement per-frame behaviours
        // (viewport-edge auto-scroll, spring-loaded folders) without
        // depending on pointer events — crucial when the user holds the
        // cursor still at the edge or over a collapsed branch.
        self.process_drag_tick(&mut *ops);

        self.process_state_changes(&mut *ops);
        self.process_tooltips_real();
        self.process_delayed_overlays_real(&mut *ops);
        self.process_pointer_leave_overlays_real(&mut *ops);
        self.process_auto_dismiss_overlays_real(&mut *ops);
        self.process_overlay_fade_dismissals_real(&mut *ops);
        // The show paths above may arm a fade animation via
        // `attach_overlay_fade` (plain tooltips, delayed overlays).
        // That sets `pending` on the opacity signal but does NOT
        // register the animation with the scheduler — registration
        // happens via `process_pending_animations`, which already ran
        // earlier in this layout pass. Without a second drain here,
        // the fade only enters the scheduler on the *next* layout
        // pass, and for surfaces with no further wake source (plain
        // tooltips, no dwell timer) `next_deadline` returns `None`
        // and the event loop sleeps with the fade stuck at opacity 0
        // — the tooltip is "shown" but invisible until an unrelated
        // input event forces another layout pass.
        self.process_pending_animations();
        // Overlay / tooltip activation may have flipped widgets from
        // dormant → active; if any of those had `needs_rebuild`
        // pending (e.g. a shortcut rebind happened while the tooltip
        // was hidden), drain them now so the freshly-visible surface
        // shows fresh content in the *same* layout pass rather than
        // waiting for another paint-triggering event.
        self.process_pending_rebuilds(&mut *ops);

        // Now that any data-driven rebuilds have torn down their old
        // subtrees, drop any overlay whose content was destroyed out-of-
        // band (e.g. clicking "mark all read" inside a notification
        // popover rebuilds the bell that owns it). Without this the
        // overlay lingers as an invisible click-blocker. Runs before the
        // early-return so it takes effect even on otherwise-idle passes.
        self.gc_orphaned_overlays();

        self.arena.refresh_roots();

        let proposal_changed = self.last_proposal != proposal;
        self.last_proposal = proposal;

        if !proposal_changed && !self.arena.any_needs_layout() {
            return;
        }

        // Per-pass layout memoization: a widget's `layout_response` is a pure
        // function of (state, proposal) within a pass, so memoizing across the
        // main-then-cross queries that height-for-width negotiation issues keeps
        // the pass O(n). Cleared here — once, dominating both the main-tree and
        // overlay root recursions below — because geometry may change between
        // passes. See `WidgetArena::cached_layout_response`.
        self.arena.clear_layout_cache();

        // `effective_theme` carries the user/OS text-scale multiplier baked into
        // its typography, so every text widget measures at the scaled size.
        let base_theme = self.effective_theme.clone();

        let overlay_content_ids = self.overlay_manager.active_content_ids();
        let roots: Vec<WidgetId> = self.arena.roots();
        let focused = self.focused;
        for root_id in roots {
            if overlay_content_ids.contains(&root_id) {
                continue;
            }
            let extras = crate::widget::LayoutExtras {
                focused,
                shortcut_registry: Some(&self.shortcut_registry),
                overlay_manager: Some(&self.overlay_manager),
            };
            layout_widget_recursive(
                &mut self.arena,
                root_id,
                Rect::from_origin_size(Point::ZERO, proposal.resolve(0.0, 0.0)),
                proposal,
                &base_theme,
                self.layout_direction,
                self.device_scale_factor,
                self.effective_text_scale,
                self.text_backend.as_ref(),
                Some(extras),
            );
        }

        let anchor_bounds = |id: WidgetId| -> Option<Rect> {
            self.arena.is_active(id).then(|| self.arena.bounds(id))
        };
        let viewport = (
            proposal.width.unwrap_or(800.0),
            proposal.height.unwrap_or(600.0),
        );
        self.overlay_manager
            .position_overlays(anchor_bounds, viewport, self.layout_direction);
        for content_id in &overlay_content_ids {
            if !self.arena.is_active(*content_id) {
                continue;
            }
            let overlay_id = self.overlay_manager.find_by_content(*content_id);
            let intrinsic = {
                let resolved_theme = self.arena.resolve_theme(*content_id, &base_theme);
                let extras = crate::widget::LayoutExtras {
                    focused: self.focused,
                    shortcut_registry: Some(&self.shortcut_registry),
                    overlay_manager: Some(&self.overlay_manager),
                };
                let ctx = LayoutContext {
                    theme: &resolved_theme,
                    layout_direction: self.layout_direction,
                    scale_factor: self.device_scale_factor,
                    text_scale: self.effective_text_scale,
                    text_backend: self.text_backend.as_ref(),
                    arena: Some(&self.arena),
                    extras: Some(extras),
                    stack_main_axis: None,
                };
                let node = self
                    .arena
                    .get(*content_id)
                    .expect("content_id from active arena children");
                node.widget
                    .layout_response(
                        SizeProposal {
                            width: None,
                            height: None,
                        },
                        &ctx,
                    )
                    .size
            };
            if let Some(overlay_id) = overlay_id {
                self.overlay_manager
                    .set_content_bounds(overlay_id, intrinsic);
                let anchor_bounds = |id: WidgetId| -> Option<Rect> {
                    self.arena.is_active(id).then(|| self.arena.bounds(id))
                };
                self.overlay_manager.position_overlays(
                    anchor_bounds,
                    viewport,
                    self.layout_direction,
                );
            }
            let overlay_bounds = overlay_id
                .and_then(|overlay_id| {
                    self.overlay_manager
                        .stack
                        .iter()
                        .find(|overlay| overlay.id == overlay_id)
                        .map(|overlay| overlay.bounds)
                })
                .unwrap_or(Rect::ZERO);
            // Use the positioned overlay_bounds for layout, not the intrinsic
            // size. For `BelowPreferred` (and any future placement that
            // inflates the overlay rect beyond the content's intrinsic size
            // to match an anchor, e.g. a combo-box dropdown that must be at
            // least as wide as its trigger), this lets the content widget
            // actually fill the overlay rather than sitting as a narrow
            // strip inside it. All other placements return
            // overlay_bounds.size() == intrinsic, so this is a no-op there.
            let content_proposal = SizeProposal::exact(overlay_bounds.width, overlay_bounds.height);
            let extras = crate::widget::LayoutExtras {
                focused: self.focused,
                shortcut_registry: Some(&self.shortcut_registry),
                overlay_manager: Some(&self.overlay_manager),
            };
            layout_widget_recursive(
                &mut self.arena,
                *content_id,
                overlay_bounds,
                content_proposal,
                &base_theme,
                self.layout_direction,
                self.device_scale_factor,
                self.effective_text_scale,
                self.text_backend.as_ref(),
                Some(extras),
            );
        }

        // Clear `needs_layout` for every active widget — layout just
        // ran. `needs_rebuild` is NOT cleared here: `rebuild_single_widget`
        // clears it for widgets it processes, and widgets whose rebuild
        // was deferred (captured-pointer window) must keep the flag set
        // so the next layout pass picks them up. Wiping it here caused
        // a regression where a scroll-driven ListView rebuild, deferred
        // during a scrollbar thumb drag, was silently dropped — the
        // user saw the thumb move but the list view stayed frozen.
        // Clear `needs_layout` on every active node. Mutation during
        // iter — pull the snapshot via the reusable scratch.
        self.arena.fill_active_ids(&mut self.active_ids_scratch);
        let ids = std::mem::take(&mut self.active_ids_scratch);
        for &id in &ids {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_layout = false;
            }
        }
        self.active_ids_scratch = ids;

        // Post-layout hover refresh. When a rebuild destroyed the
        // hovered widget, `revalidate_interaction_state` cleared
        // `hovered` to `None`. Now that widgets have fresh bounds
        // from this layout pass, re-hit-test at the cached pointer
        // position so the next wheel/pointer event routes to the
        // widget the cursor is actually over. Without this, a
        // virtualized list that materializes new rows under a
        // stationary cursor would see the next `Scroll` fall through
        // to `focused` and bubble to an ancestor scrollable.
        if self.hovered.is_none()
            && let Some(pos) = self.last_pointer_position
        {
            let new_target = self.hit_test(pos);
            if new_target.is_some() {
                if let Some(new) = new_target {
                    self.dispatch_to_widget(new, &WidgetEvent::PointerEnter, &mut *ops);
                }
                self.set_hovered(new_target);
            }
        }
    }
}

/// Recursive layout pass operating on the arena directly (avoids borrow conflicts).
#[allow(clippy::too_many_arguments)]
fn layout_widget_recursive(
    arena: &mut WidgetArena,
    id: WidgetId,
    parent_bounds: Rect,
    proposal: SizeProposal,
    base_theme: &crate::styles::Theme,
    layout_direction: crate::environment::LayoutDirection,
    scale_factor: f32,
    text_scale: f32,
    text_backend: Option<&std::rc::Rc<std::cell::RefCell<dyn bastyde_canvas::TextBackend>>>,
    extras: Option<crate::widget::LayoutExtras<'_>>,
) {
    if !arena.is_active(id) {
        return;
    }

    let resolved_theme = arena.resolve_theme(id, base_theme);

    let desired_size = {
        let ctx = LayoutContext {
            theme: &resolved_theme,
            layout_direction,
            scale_factor,
            text_scale,
            text_backend,
            arena: Some(arena),
            extras,
            stack_main_axis: None,
        };
        arena
            .cached_layout_response(id, proposal, &ctx)
            .map(|r| r.size)
            .unwrap_or(bastyde_canvas::Size::ZERO)
    };

    let bounds = Rect::new(
        parent_bounds.x,
        parent_bounds.y,
        proposal.width.unwrap_or(desired_size.width),
        proposal.height.unwrap_or(desired_size.height),
    );
    if let Some(node) = arena.get_mut(id) {
        if node.bounds != bounds {
            node.cached_paint = None;
            node.dirty.needs_paint = true;
        }
        node.bounds = bounds;
    }

    let child_ids: Vec<WidgetId> = arena.children(id).to_vec();
    if !child_ids.is_empty() {
        let active_child_ids: Vec<WidgetId> = child_ids
            .iter()
            .copied()
            .filter(|&child_id| arena.is_active(child_id))
            .collect();

        let mut placements: Vec<WidgetPlacement> = active_child_ids
            .iter()
            .map(|&child_id| WidgetPlacement {
                id: child_id,
                origin: bounds.origin(),
                size: bounds.size(),
            })
            .collect();

        {
            let ctx = LayoutContext {
                theme: &resolved_theme,
                layout_direction,
                scale_factor,
                text_scale,
                text_backend,
                arena: Some(arena),
                extras,
                stack_main_axis: None,
            };
            let node = arena.get(id).expect("widget id is active in arena");
            node.widget
                .place_children(bounds, proposal, &mut placements, &ctx);
        }

        for placement in &placements {
            let child_bounds = Rect::from_origin_size(placement.origin, placement.size);
            if let Some(child_node) = arena.get_mut(placement.id) {
                if child_node.bounds != child_bounds {
                    child_node.cached_paint = None;
                    child_node.dirty.needs_paint = true;
                }
                child_node.bounds = child_bounds;
            }

            let child_proposal = SizeProposal::exact(placement.size.width, placement.size.height);
            let grandchild_ids: Vec<WidgetId> = arena.children(placement.id).to_vec();
            if !grandchild_ids.is_empty() {
                layout_widget_recursive(
                    arena,
                    placement.id,
                    child_bounds,
                    child_proposal,
                    base_theme,
                    layout_direction,
                    scale_factor,
                    text_scale,
                    text_backend,
                    extras,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, InsetWidget, StackWidget};
    use bastyde_canvas::Size;
    use bastyde_tokens::Color;

    #[derive(Debug)]
    struct ShrinkWrapContainer {
        child: WidgetId,
        inset: f32,
    }

    impl Widget for ShrinkWrapContainer {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            let child_size = ctx
                .child_size(self.child, SizeProposal::unspecified())
                .unwrap_or(Size::ZERO);
            Size::new(
                child_size.width + self.inset * 2.0,
                child_size.height + self.inset * 2.0,
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
            for child in children.iter_mut() {
                child.origin = Point::new(bounds.x + self.inset, bounds.y + self.inset);
                child.size = Size::new(
                    (bounds.width - self.inset * 2.0).max(0.0),
                    (bounds.height - self.inset * 2.0).max(0.0),
                );
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            vec![self.child]
        }
    }

    // ── Per-pass layout memoization cache (Part C) ──────────────────────────

    /// A childless leaf that counts how many times `layout_response` runs and
    /// can opt out of caching. The driver does not recurse into a childless
    /// leaf's placement, so the only calls come from a parent's `child_size`
    /// queries — making the count a precise probe of the cache.
    #[derive(Debug)]
    struct CountingLeaf {
        calls: std::rc::Rc<std::cell::Cell<u32>>,
        cacheable: bool,
    }

    impl Widget for CountingLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            self.calls.set(self.calls.get() + 1);
            Size::new(50.0, 20.0).into()
        }
        fn cacheable_layout(&self) -> bool {
            self.cacheable
        }
    }

    /// Queries its single child with the *same* proposal in both
    /// `layout_response` and `place_children` — the pattern real stacks use
    /// for height-for-width. With caching the child computes once; without it,
    /// twice.
    #[derive(Debug)]
    struct DoubleQueryContainer {
        child: WidgetId,
    }

    impl Widget for DoubleQueryContainer {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            ctx.child_size(self.child, SizeProposal::exact(50.0, 20.0))
                .unwrap_or(Size::ZERO)
                .into()
        }
        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [WidgetPlacement],
            ctx: &LayoutContext,
        ) {
            // Second query with the identical proposal.
            let _ = ctx.child_size(self.child, SizeProposal::exact(50.0, 20.0));
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }
        fn children(&self) -> Vec<WidgetId> {
            vec![self.child]
        }
    }

    #[test]
    fn cache_dedupes_identical_child_queries_within_a_pass() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(CountingLeaf {
            calls: calls.clone(),
            cacheable: true,
        });
        let _root = tree.add(DoubleQueryContainer { child: leaf });
        tree.layout(SizeProposal::exact(100.0, 50.0));
        // Two identical `exact(50,20)` queries (layout_response + place_children)
        // collapse to one real call; the driver does not recurse into the
        // childless leaf.
        assert_eq!(calls.get(), 1, "cacheable leaf should be computed once");
    }

    #[test]
    fn cache_opt_out_recomputes_every_query() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(CountingLeaf {
            calls: calls.clone(),
            cacheable: false,
        });
        let _root = tree.add(DoubleQueryContainer { child: leaf });
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(
            calls.get(),
            2,
            "opt-out leaf must run on every query (side effects preserved)"
        );
    }

    #[test]
    fn cache_is_cleared_between_passes() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(CountingLeaf {
            calls: calls.clone(),
            cacheable: true,
        });
        let _root = tree.add(DoubleQueryContainer { child: leaf });
        tree.layout(SizeProposal::exact(100.0, 50.0));
        // A second pass with a different proposal must re-run layout — proving
        // the cache is per-pass, not stale across passes (the `exact(50,20)`
        // child key is identical between passes).
        tree.layout(SizeProposal::exact(120.0, 60.0));
        assert_eq!(
            calls.get(),
            2,
            "each pass recomputes; cache cleared per pass"
        );
    }

    // ── measure_intrinsic (Primitive 2) ─────────────────────────────────────

    /// Probe: from its own `layout_response`, measures `target` two ways and
    /// stashes the results — the normal (activation-gated) query and the
    /// intrinsic (activation-ignoring) query.
    #[derive(Debug)]
    struct MeasureProbe {
        target: WidgetId,
        active_w: std::rc::Rc<std::cell::Cell<f32>>, // -1.0 == None
        intrinsic_w: std::rc::Rc<std::cell::Cell<f32>>,
    }
    impl Widget for MeasureProbe {
        fn layout_response(
            &self,
            p: SizeProposal,
            ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            // Measure intrinsic FIRST, then the normal gated query: if the
            // measure had polluted the cache, the gated query could wrongly
            // return a size for the dormant target. `exact` because FillWidget
            // fills its proposal (it has no intrinsic size of its own).
            let probe = SizeProposal::exact(120.0, 30.0);
            let intrinsic = ctx
                .measure_intrinsic(self.target, probe)
                .map(|s| s.width)
                .unwrap_or(-1.0);
            let active = ctx
                .child_size(self.target, probe)
                .map(|s| s.width)
                .unwrap_or(-1.0);
            self.intrinsic_w.set(intrinsic);
            self.active_w.set(active);
            p.resolve(0.0, 0.0).into()
        }
        fn cacheable_layout(&self) -> bool {
            false
        }
    }

    #[test]
    fn measure_intrinsic_sees_a_dormant_widget_normal_query_does_not() {
        let active = std::rc::Rc::new(std::cell::Cell::new(0.0));
        let intrinsic = std::rc::Rc::new(std::cell::Cell::new(0.0));
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        tree.set_dormant(leaf);
        let _probe = tree.add(MeasureProbe {
            target: leaf,
            active_w: active.clone(),
            intrinsic_w: intrinsic.clone(),
        });
        tree.layout(SizeProposal::exact(200.0, 50.0));

        // measure_intrinsic measures the dormant widget (FillWidget fills the
        // 120px probe)…
        assert!(
            (intrinsic.get() - 120.0).abs() < 0.01,
            "measure_intrinsic should size the dormant widget, got {}",
            intrinsic.get()
        );
        // …and the normal gated query (run AFTER) still returns None — proving
        // the measure bypassed, and did not seed, the per-pass cache.
        assert_eq!(
            active.get(),
            -1.0,
            "child_size must stay None for a dormant widget (no cache pollution)"
        );
    }

    #[test]
    fn single_widget_fills_proposal() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        let bounds = tree.bounds(widget);
        assert_eq!(bounds.width, 200.0);
        assert_eq!(bounds.height, 40.0);
    }

    #[test]
    fn stack_children_overlap() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let stack = tree.add(StackWidget::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let children = tree.children(stack);
        assert_eq!(children.len(), 2);
        let a_bounds = tree.bounds(children[0]);
        let b_bounds = tree.bounds(children[1]);
        assert_eq!(a_bounds.origin(), b_bounds.origin());
        assert_eq!(a_bounds.size(), b_bounds.size());
    }

    #[test]
    fn inset_widget_insets_child() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(InsetWidget::new(10.0).set_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let children = tree.children(parent);
        let child_bounds = tree.bounds(children[0]);
        assert_eq!(child_bounds.x, 10.0);
        assert_eq!(child_bounds.y, 10.0);
        assert_eq!(child_bounds.width, 80.0);
        assert_eq!(child_bounds.height, 30.0);
    }

    #[test]
    fn recursive_layout_preserves_exact_parent_placement_for_containers() {
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let shrink = tree.add(ShrinkWrapContainer {
            child: leaf,
            inset: 8.0,
        });
        let root = tree.add(StackWidget::new().add_child(shrink));

        tree.layout(SizeProposal::exact(120.0, 80.0));

        assert_eq!(tree.bounds(root), Rect::new(0.0, 0.0, 120.0, 80.0));
        assert_eq!(
            tree.bounds(shrink),
            Rect::new(0.0, 0.0, 120.0, 80.0),
            "child container should keep the exact size assigned by its parent"
        );
        assert_eq!(tree.bounds(leaf), Rect::new(8.0, 8.0, 104.0, 64.0));
    }

    #[test]
    fn needs_paint_after_layout() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        assert!(tree.needs_layout());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(!tree.needs_layout());
    }

    #[test]
    fn signal_binding_marks_widget_dirty_on_layout() {
        use crate::signal::Signal;

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.render();

        assert!(!tree.needs_paint());

        let visible = Signal::new(true);
        visible.bind_to(
            widget,
            tree.binding_registry(),
            crate::binding::BindingLevel::RepaintOnly,
        );

        visible.set(false);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.needs_paint());
    }
}
