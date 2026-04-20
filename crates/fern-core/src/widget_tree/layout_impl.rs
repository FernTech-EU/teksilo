use super::*;

impl WidgetTree {
    /// Process dirty state bindings: mark bound widgets for repaint, relayout,
    /// or rebuild. Called automatically at the start of layout().
    pub(super) fn process_state_changes(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let dirty_widgets = self.binding_registry.flush_dirty();
        for (id, level) in &dirty_widgets {
            match level {
                crate::binding::BindingLevel::RepaintOnly => {
                    self.arena.mark_needs_paint(*id);
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
                    // Drained separately below; see `flush_accessibility_dirty`.
                    // Kept in the match so a future variant addition is a
                    // compile-time reminder.
                }
            }
        }

        // Orthogonal to the visual dirty pass: if any signal bound at
        // `BindingLevel::AccessibilityOnly` fired, flip the tree-wide
        // `a11y_dirty` flag so the next `sync_accessibility` rebuilds
        // the AccessKit tree. Decoupled from layout / paint so a text
        // edit that changes no visual geometry still reaches screen
        // readers within one frame.
        if self.binding_registry.flush_accessibility_dirty() {
            self.a11y_dirty = true;
        }

        // Rebuild data-driven widgets whose data model changed.
        self.process_pending_rebuilds(&mut *ops);

        let mut to_dormant = Vec::new();
        let mut to_activate = Vec::new();
        for (id, is_active, should_be_visible) in self.arena.visibility_checks() {
            if is_active && !should_be_visible {
                to_dormant.push(id);
            } else if !is_active && should_be_visible {
                to_activate.push(id);
            }
        }
        for id in to_dormant {
            self.arena.set_dormant(id);
        }
        for id in to_activate {
            self.arena.activate(id);
        }
    }

    /// Drain any widgets flagged `needs_rebuild` that are currently
    /// active + have built children. Called from
    /// [`process_state_changes`] after dirty bindings have been
    /// flushed, and again after overlay / tooltip activation so that
    /// widgets transitioning from dormant → active in the same
    /// layout pass get rebuilt *this* frame rather than the next.
    pub(super) fn process_pending_rebuilds(
        &mut self,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Defer rebuilds during the gesture-arena latch window: from
        // `PointerDown` (which stores the press position in the captured
        // widget's arena) until either `PointerUp` or the arena fires
        // `DragStarted`. Rebuilding inside that window would destroy
        // the arena and the press state with it, so the recognizer would
        // never fire — the user clicks and nothing happens.
        //
        // Once `active_drag` is set, the framework routes PointerMove /
        // PointerUp via `handle_drag_move` / `handle_drag_drop` keyed
        // on the `DragSession`, not on the captured widget's arena —
        // so a mid-drag rebuild (spring-loaded folders, data model
        // mutations) is safe. Post-rebuild, `revalidate_interaction_state`
        // clears the now-stale `pointer_captured_by`; subsequent events
        // hit-test normally and still reach `handle_drag_move`.
        let defer = self.pointer_captured_by.is_some() && self.active_drag.is_none();
        if defer {
            self.revalidate_interaction_state(&mut *ops);
            return;
        }
        let to_rebuild = self.arena.collect_needs_rebuild();
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
    }

    /// Run the layout pass with the given size proposal, using
    /// [`NoopWindowOps`](crate::window::NoopWindowOps). Handlers
    /// triggered from drag_tick / tooltip activation / etc cannot
    /// call `ctx.open_window(...)` from this path.
    ///
    /// `fern-app` calls [`layout_with_ops`](Self::layout_with_ops)
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
        // Overlay / tooltip activation may have flipped widgets from
        // dormant → active; if any of those had `needs_rebuild`
        // pending (e.g. a shortcut rebind happened while the tooltip
        // was hidden), drain them now so the freshly-visible surface
        // shows fresh content in the *same* layout pass rather than
        // waiting for another paint-triggering event.
        self.process_pending_rebuilds(&mut *ops);

        self.arena.refresh_roots();

        let proposal_changed = self.last_proposal != proposal;
        self.last_proposal = proposal;

        if !proposal_changed && !self.arena.any_needs_layout() {
            return;
        }

        self.a11y_dirty = true;

        let base_theme = self.theme.clone();

        let overlay_content_ids = self.overlay_manager.active_content_ids();
        let roots: Vec<WidgetId> = self.arena.roots();
        for root_id in roots {
            if overlay_content_ids.contains(&root_id) {
                continue;
            }
            layout_widget_recursive(
                &mut self.arena,
                root_id,
                Rect::from_origin_size(Point::ZERO, proposal.resolve(0.0, 0.0)),
                proposal,
                &base_theme,
                self.layout_direction,
                self.text_backend.as_ref(),
            );
        }

        let anchor_bounds = |id: WidgetId| -> Rect { self.arena.bounds(id) };
        let viewport = (
            proposal.width.unwrap_or(800.0),
            proposal.height.unwrap_or(600.0),
        );
        self.overlay_manager
            .position_overlays(anchor_bounds, viewport);
        for content_id in &overlay_content_ids {
            if !self.arena.is_active(*content_id) {
                continue;
            }
            let overlay_id = self.overlay_manager.find_by_content(*content_id);
            let intrinsic = {
                let resolved_theme = self.arena.resolve_theme(*content_id, &base_theme);
                let ctx = LayoutContext {
                    theme: &resolved_theme,
                    layout_direction: self.layout_direction,
                    text_backend: self.text_backend.as_ref(),
                    arena: Some(&self.arena),
                };
                let node = self.arena.get(*content_id).unwrap();
                node.widget.size_that_fits(
                    SizeProposal {
                        width: None,
                        height: None,
                    },
                    &ctx,
                )
            };
            if let Some(overlay_id) = overlay_id {
                self.overlay_manager
                    .set_content_bounds(overlay_id, intrinsic);
                let anchor_bounds = |id: WidgetId| -> Rect { self.arena.bounds(id) };
                self.overlay_manager
                    .position_overlays(anchor_bounds, viewport);
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
            layout_widget_recursive(
                &mut self.arena,
                *content_id,
                overlay_bounds,
                content_proposal,
                &base_theme,
                self.layout_direction,
                self.text_backend.as_ref(),
            );
        }

        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_layout = false;
                node.dirty.needs_rebuild = false;
            }
        }
    }
}

/// Recursive layout pass operating on the arena directly (avoids borrow conflicts).
fn layout_widget_recursive(
    arena: &mut WidgetArena,
    id: WidgetId,
    parent_bounds: Rect,
    proposal: SizeProposal,
    base_theme: &fern_tokens::Theme,
    layout_direction: crate::environment::LayoutDirection,
    text_backend: Option<&std::rc::Rc<std::cell::RefCell<dyn fern_canvas::TextBackend>>>,
) {
    if !arena.is_active(id) {
        return;
    }

    let resolved_theme = arena.resolve_theme(id, base_theme);

    let desired_size = {
        let ctx = LayoutContext {
            theme: &resolved_theme,
            layout_direction,
            text_backend,
            arena: Some(arena),
        };
        let node = arena.get(id).unwrap();
        node.widget.size_that_fits(proposal, &ctx)
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
                text_backend,
                arena: Some(arena),
            };
            let node = arena.get(id).unwrap();
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
                    text_backend,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, InsetWidget, StackWidget};
    use fern_canvas::Size;
    use fern_tokens::Color;

    #[derive(Debug)]
    struct ShrinkWrapContainer {
        child: WidgetId,
        inset: f32,
    }

    impl Widget for ShrinkWrapContainer {
        fn size_that_fits(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> Size {
            let child_size = ctx
                .child_size(self.child, SizeProposal::unspecified())
                .unwrap_or(Size::ZERO);
            Size::new(
                child_size.width + self.inset * 2.0,
                child_size.height + self.inset * 2.0,
            )
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
