use super::*;

impl WidgetTree {
    /// Set focus to a specific widget with the given origin, invoking
    /// `on_focus_lost` / `on_focus_gained` handlers through the
    /// caller-supplied [`WindowOps`](crate::window::WindowOps) sink.
    ///
    /// `bastyde-app` drives in-dispatch focus changes through this method
    /// so that focus-triggered handlers can synchronously call
    /// `ctx.open_window(...)`. Standalone callers (programmatic
    /// focus from framework code paths, tests) use
    /// [`focus_with_origin`](Self::focus_with_origin) which wraps with
    /// [`NoopWindowOps`](crate::window::NoopWindowOps).
    pub fn focus_with_origin_ops(
        &mut self,
        id: WidgetId,
        origin: crate::focus::FocusOrigin,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if self.focused == Some(id) {
            return;
        }
        let previously_focused = self.focused;
        if let Some(old) = self.focused {
            let old_overlay = self.overlay_ancestor_for_widget(old);
            let new_overlay = self.overlay_ancestor_for_widget(id);
            let moving_into_descendant_overlay = match (old_overlay, new_overlay) {
                (Some(old_overlay), Some(new_overlay)) => self
                    .overlay_manager
                    .is_descendant_of(new_overlay, old_overlay),
                _ => false,
            };

            if moving_into_descendant_overlay {
                self.dispatch_to_widget_direct(old, &WidgetEvent::FocusLost, &mut *ops);
            } else {
                self.dispatch_to_widget(old, &WidgetEvent::FocusLost, &mut *ops);
            }
        }
        self.set_focused(Some(id));
        self.focus_origin = Some(origin);
        self.a11y_dirty = true;
        self.update_focus_within_signals(previously_focused, Some(id));
        self.dispatch_to_widget(id, &WidgetEvent::FocusGained { origin }, &mut *ops);
        // Focus-driven tooltip machinery: close any previously-shown
        // focus-promoted rich tooltip whose scope no longer contains
        // the focus target, then immediately surface+sticky the rich
        // tooltip (if any) attached to the new focus target. See
        // `tooltip_focus_enter` / `tooltip_focus_leave_outside` for
        // the full rationale.
        self.tooltip_focus_leave_outside(Some(id), &mut *ops);
        self.tooltip_focus_enter(id);
        self.scroll_focused_into_view(id, &mut *ops);
    }

    /// Set focus using [`NoopWindowOps`](crate::window::NoopWindowOps).
    /// Programmatic / framework-internal callers. Handlers triggered
    /// from this path cannot `ctx.open_window(...)`.
    pub fn focus_with_origin(&mut self, id: WidgetId, origin: crate::focus::FocusOrigin) {
        let mut noop = crate::window::NoopWindowOps;
        self.focus_with_origin_ops(id, origin, &mut noop);
    }

    /// After setting focus, ensure the focused widget is visible inside
    /// all ancestor scroll areas (clips_children containers).
    fn scroll_focused_into_view(
        &mut self,
        focused_id: WidgetId,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let focused_bounds = self.arena.bounds(focused_id);

        let mut current = self.arena.parent(focused_id);
        while let Some(ancestor_id) = current {
            if let Some(node) = self.arena.get(ancestor_id)
                && node.clips_children
            {
                let viewport = node.bounds;
                let needs_scroll = focused_bounds.y < viewport.y
                    || focused_bounds.bottom() > viewport.bottom()
                    || focused_bounds.x < viewport.x
                    || focused_bounds.right() > viewport.right();

                if needs_scroll {
                    self.dispatch_to_widget(
                        ancestor_id,
                        &WidgetEvent::ScrollIntoView {
                            target_bounds: focused_bounds,
                            margin: 0.0,
                        },
                        &mut *ops,
                    );
                }
            }
            current = self.arena.parent(ancestor_id);
        }
    }

    /// Set focus to a specific widget (programmatic origin, no ops).
    pub fn focus(&mut self, id: WidgetId) {
        self.focus_with_origin(id, crate::focus::FocusOrigin::Programmatic);
    }

    /// Set focus — the dispatch-path variant that threads `ops` through
    /// to any on_focus_lost / on_focus_gained handlers.
    pub fn focus_ops(&mut self, id: WidgetId, ops: &mut dyn crate::window::WindowOps) {
        self.focus_with_origin_ops(id, crate::focus::FocusOrigin::Programmatic, ops);
    }

    /// Get the currently focused widget.
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// Find the first focusable widget in depth-first order within a subtree.
    pub fn first_focusable_descendant(&self, root: WidgetId) -> Option<WidgetId> {
        if !self.arena.is_active(root) {
            return None;
        }

        let mut focusable = Vec::new();
        self.collect_focusable_tree_order(root, &mut focusable);
        focusable.into_iter().next()
    }

    /// Whether the given widget id currently exists and is active in the
    /// tree (not dormant, not destroyed). Callers that need to validate a
    /// user-supplied `WidgetId` before acting on it — e.g. the modal
    /// presentation path validating `ModalRequest::focus_target` — use
    /// this.
    pub fn is_active(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
    }

    /// Walk the subtree rooted at `id` in depth-first order, returning
    /// the first widget-reported `initial_focus_hint` that resolves to
    /// an active descendant of `id`.
    ///
    /// Used by the modal presentation pipeline to let a deferred-built
    /// content widget (e.g. `MessageBox`) direct focus to a specific
    /// descendant after build — even when wrapped in a surface widget
    /// like `ModalContainer` that doesn't itself know the default
    /// button's id. The framework walks in to find the first hint
    /// under the content root, which is tighter than falling all the
    /// way back to `first_focusable_descendant`.
    ///
    /// Hints pointing at inactive or out-of-subtree ids are ignored;
    /// the walk continues so a shallow wrapper's stale hint doesn't
    /// hide a deeper child's valid one.
    pub fn widget_initial_focus_hint(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.arena.is_active(id) {
            return None;
        }
        if let Some(node) = self.arena.get(id) {
            if let Some(target) = node.widget.initial_focus_hint()
                && self.arena.is_active(target)
                && self.is_descendant_of(target, id)
            {
                return Some(target);
            }
            for &child in &node.children {
                if let Some(found) = self.widget_initial_focus_hint(child) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// How the currently focused widget gained focus.
    pub fn focus_origin(&self) -> Option<crate::focus::FocusOrigin> {
        self.focus_origin
    }

    /// Cycle focus to the next/previous focusable widget (Tab/Shift-Tab).
    /// Traverses in document order (depth-first tree traversal).
    pub(super) fn cycle_focus(&mut self, reverse: bool, ops: &mut dyn crate::window::WindowOps) {
        let mut focusable = Vec::new();
        if let Some(modal_overlay) = self.overlay_manager.topmost_centered() {
            self.collect_focusable_tree_order(modal_overlay.content_id, &mut focusable);
        } else {
            let roots = self.arena.roots();
            for root in roots {
                self.collect_focusable_tree_order(root, &mut focusable);
            }
        }

        // Filter out widgets with a `tab_stop` binding that
        // evaluates to `false` — they're focusable via
        // `request_focus` but excluded from Tab traversal. Implements
        // the ARIA roving-tabindex pattern (HTML `tabindex="-1"`).
        focusable.retain(|&id| {
            self.arena
                .get(id)
                .and_then(|node| node.tab_stop.as_ref())
                .map(|prop| prop.get())
                .unwrap_or(true)
        });

        if focusable.is_empty() {
            return;
        }

        focusable.sort_by(|&a, &b| {
            let ta = self.arena.get(a).and_then(|node| node.node_tab_index);
            let tb = self.arena.get(b).and_then(|node| node.node_tab_index);
            match (ta, tb) {
                (Some(ia), Some(ib)) => ia.cmp(&ib),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        let current_idx = self
            .focused
            .and_then(|focused| focusable.iter().position(|&id| id == focused));

        let next_idx = match current_idx {
            Some(idx) => {
                if reverse {
                    if idx == 0 {
                        focusable.len() - 1
                    } else {
                        idx - 1
                    }
                } else {
                    (idx + 1) % focusable.len()
                }
            }
            None => 0,
        };

        self.focus_with_origin_ops(
            focusable[next_idx],
            crate::focus::FocusOrigin::Keyboard,
            &mut *ops,
        );
    }

    /// Check if a node is focusable (set via HandlerSet `.focusable(true)` in build).
    fn is_node_focusable(&self, node: &crate::arena::WidgetNode) -> bool {
        node.node_focusable.unwrap_or(false)
    }

    /// Find the nearest focusable widget at or above the given ID.
    pub(super) fn find_focusable_at_or_above(&self, id: WidgetId) -> Option<WidgetId> {
        let mut current = Some(id);
        while let Some(current_id) = current {
            if let Some(node) = self.arena.get(current_id)
                && self.is_node_focusable(node)
            {
                return Some(current_id);
            }
            current = self.arena.parent(current_id);
        }
        None
    }

    /// Collect focusable widgets in depth-first (document) order.
    fn collect_focusable_tree_order(&self, id: WidgetId, out: &mut Vec<WidgetId>) {
        if !self.arena.is_active(id) {
            return;
        }
        if let Some(node) = self.arena.get(id) {
            if node
                .enabled_state
                .as_ref()
                .map(|s| !s.get())
                .unwrap_or(false)
            {
                return;
            }
            if self.is_node_focusable(node) {
                out.push(id);
            }
            for &child in &node.children {
                self.collect_focusable_tree_order(child, out);
            }
        }
    }

    /// Build the chain of strict ancestors of `id` (i.e. starting at
    /// the parent of `id`, walking up to a root). Returns an empty
    /// vector when `id` is `None` or has no parent. Used by the
    /// `focus_within` / `hover_within` chain-diff helpers.
    pub(super) fn strict_ancestors_of(&self, id: Option<WidgetId>) -> Vec<WidgetId> {
        let mut chain = Vec::new();
        if let Some(start) = id {
            let mut current = self.arena.parent(start);
            while let Some(parent) = current {
                chain.push(parent);
                current = self.arena.parent(parent);
            }
        }
        chain
    }

    /// Update every `focus_within_signal` whose owning node moved
    /// in or out of the focused widget's strict-ancestor chain
    /// between `old` and `new`. Strict ancestors only — the
    /// focused widget's own signal (if any) is never written.
    pub(crate) fn update_focus_within_signals(
        &mut self,
        old: Option<WidgetId>,
        new: Option<WidgetId>,
    ) {
        let old_chain = self.strict_ancestors_of(old);
        let new_chain = self.strict_ancestors_of(new);
        // Nodes leaving the chain → false.
        for &id in &old_chain {
            if !new_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.focus_within_signal.clone()
            {
                sig.set(false);
            }
        }
        // Nodes entering the chain → true.
        for &id in &new_chain {
            if !old_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.focus_within_signal.clone()
            {
                sig.set(true);
            }
        }
    }

    /// Single point of mutation for `self.hovered`. Updates both the
    /// internal field and the externally-observable Signal so debug
    /// tooling (the inspector's hover tooltip) doesn't have to poll.
    /// Does **not** call [`update_hover_within_signals`] — call sites
    /// remain in charge of dispatching enter/leave because some sites
    /// (e.g. post-layout hover recovery) intentionally skip it.
    pub(crate) fn set_hovered(&mut self, value: Option<WidgetId>) {
        self.hovered = value;
        if self.hovered_signal.get() != value {
            self.hovered_signal.set(value);
        }
    }

    /// Single point of mutation for `self.focused`. Mirror of
    /// [`set_hovered`] for the focused chain. Drives the inspector's
    /// Focus tab without requiring the tab to poll. Does not touch
    /// `focus_origin` or focus-within signals — call sites remain
    /// responsible for those (the bookkeeping varies by mutation
    /// path, e.g. focus loss vs. arena destruction).
    pub(crate) fn set_focused(&mut self, value: Option<WidgetId>) {
        self.focused = value;
        if self.focused_signal.get() != value {
            self.focused_signal.set(value);
        }
    }

    /// Mirror of [`update_focus_within_signals`](Self::update_focus_within_signals)
    /// for the hovered chain.
    pub(crate) fn update_hover_within_signals(
        &mut self,
        old: Option<WidgetId>,
        new: Option<WidgetId>,
    ) {
        let old_chain = self.strict_ancestors_of(old);
        let new_chain = self.strict_ancestors_of(new);
        for &id in &old_chain {
            if !new_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.hover_within_signal.clone()
            {
                sig.set(false);
            }
        }
        for &id in &new_chain {
            if !old_chain.contains(&id)
                && let Some(node) = self.arena.get(id)
                && let Some(sig) = node.hover_within_signal.clone()
            {
                sig.set(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;

    #[test]
    fn focus_widget() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);
        assert_eq!(tree.focused(), Some(widget));
    }

    #[test]
    fn focus_change() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));
        tree.focus(b);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn first_focusable_descendant_prefers_first_focusable_child() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let _not_focusable = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new().focusable());
        let root = tree.add(
            crate::test_widgets::StackWidget::new()
                .add_child(a)
                .add_child(b),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.first_focusable_descendant(root), Some(a));
    }

    #[test]
    fn tab_cycles_through_focusable_widgets() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.focused(), None);

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(c));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn shift_tab_cycles_backwards() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(c));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn tab_skips_non_focusable_widgets() {
        let mut tree = WidgetTree::new();
        let _not_focusable = tree.add(FillWidget::new());
        let a = tree.add(FillWidget::new().focusable());
        let _also_not = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn tab_focus_has_keyboard_origin() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focus_origin(),
            Some(crate::focus::FocusOrigin::Keyboard)
        );
    }

    #[test]
    fn tab_skips_focusable_inside_disabled_ancestor() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;

        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let inner = tree.add(FillWidget::new().focusable());
        let disabled_container = tree.add(StackWidget::new().add_child(inner));
        let c = tree.add(FillWidget::new().focusable());
        tree.enabled_when(disabled_container, Signal::new(false));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focused(),
            Some(c),
            "tab should skip the focusable widget nested inside the disabled container"
        );
    }

    #[test]
    fn dormant_widget_not_in_focus_cycle() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(a);
        tree.set_dormant(b);

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(c));
    }

    #[test]
    fn tab_cycles_focus_in_tree_order() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn focus_survives_theme_switch() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let _b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));

        tree.set_theme(crate::presets::intui::dark());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert_eq!(
            tree.focused(),
            Some(a),
            "theme switch must not clobber focus"
        );
    }

    #[test]
    fn focus_survives_locale_switch() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        tree.set_locale("fr-FR".to_string());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert_eq!(
            tree.focused(),
            Some(a),
            "locale switch must not clobber focus"
        );
    }

    // ── focus_within / hover_within ─────────────────────────────

    #[test]
    fn focus_within_flips_when_descendant_takes_focus() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let halo = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable());
        let mid = tree.add(StackWidget::new().add_child(leaf));
        let _outer = tree.add(StackWidget::new().add_child(mid).focus_within(halo.clone()));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(!halo.get(), "no focus yet → signal is false");

        tree.focus(leaf);
        assert!(halo.get(), "leaf now focused, outer is its strict ancestor");
    }

    #[test]
    fn focus_within_strict_ancestors_only() {
        // A widget that is itself focused must NOT see its own
        // focus_within signal flipped to true.
        use crate::signal::Signal;
        use crate::widget_builder::WidgetBuilder;

        let halo = Signal::new(false);

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().focus_within(halo.clone()));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        assert!(
            !halo.get(),
            "focusing the widget itself must not set its own focus_within"
        );
    }

    #[test]
    fn focus_within_diff_across_siblings() {
        // Tree: root → mid_a [sig_a] → leaf_a, root → mid_b [sig_b] → leaf_b.
        // Move focus from leaf_a to leaf_b: sig_a → false, sig_b → true.
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let sig_a = Signal::new(false);
        let sig_b = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf_a = tree.add(FillWidget::new().focusable());
        let leaf_b = tree.add(FillWidget::new().focusable());
        let mid_a = tree.add(
            StackWidget::new()
                .add_child(leaf_a)
                .focus_within(sig_a.clone()),
        );
        let mid_b = tree.add(
            StackWidget::new()
                .add_child(leaf_b)
                .focus_within(sig_b.clone()),
        );
        let _root = tree.add(StackWidget::new().add_child(mid_a).add_child(mid_b));

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf_a);
        assert!(sig_a.get(), "sig_a true after focusing leaf_a");
        assert!(!sig_b.get(), "sig_b false: leaf_a is not its descendant");

        tree.focus(leaf_b);
        assert!(!sig_a.get(), "sig_a flipped to false on focus move");
        assert!(sig_b.get(), "sig_b flipped to true on focus move");
    }

    #[test]
    fn focus_within_clears_when_focused_widget_destroyed() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;

        let halo = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().focusable());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(leaf)
                .focus_within(halo.clone()),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(leaf);
        assert!(halo.get());

        tree.destroy_subtree(leaf);
        assert!(
            !halo.get(),
            "destroying the focused widget must clear focus_within on its ancestors"
        );
    }

    #[test]
    fn hover_within_flips_via_pointer_move() {
        use crate::signal::Signal;
        use crate::test_widgets::StackWidget;
        use crate::widget_builder::WidgetBuilder;
        use bastyde_canvas::Point;

        let glow = Signal::new(false);

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let _outer = tree.add(
            StackWidget::new()
                .add_child(leaf)
                .hover_within(glow.clone()),
        );

        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(!glow.get());

        tree.pointer_move(Point::new(50.0, 25.0));
        assert!(glow.get(), "pointer over leaf → outer.hover_within = true");

        tree.pointer_move(Point::new(500.0, 500.0));
        assert!(
            !glow.get(),
            "pointer moves outside the tree → hover_within clears"
        );
    }

    #[test]
    fn hover_within_strict_ancestors_only() {
        use crate::signal::Signal;
        use crate::widget_builder::WidgetBuilder;
        use bastyde_canvas::Point;

        let glow = Signal::new(false);

        let mut tree = WidgetTree::new();
        let _w = tree.add(FillWidget::new().hover_within(glow.clone()));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.pointer_move(Point::new(50.0, 25.0));

        assert!(
            !glow.get(),
            "hovering the widget itself must not set its own hover_within"
        );
    }
}
