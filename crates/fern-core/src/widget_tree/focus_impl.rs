use super::*;

impl WidgetTree {
    /// Set focus to a specific widget with the given origin.
    pub fn focus_with_origin(&mut self, id: WidgetId, origin: crate::focus::FocusOrigin) {
        if self.focused == Some(id) {
            return;
        }
        if let Some(old) = self.focused {
            let old_overlay = self.overlay_ancestor_for_widget(old);
            let new_overlay = self.overlay_ancestor_for_widget(id);
            let moving_into_descendant_overlay = match (old_overlay, new_overlay) {
                (Some(old_overlay), Some(new_overlay)) => {
                    self.overlay_manager.is_descendant_of(new_overlay, old_overlay)
                }
                _ => false,
            };

            if moving_into_descendant_overlay {
                self.dispatch_to_widget_direct(old, &WidgetEvent::FocusLost);
            } else {
                self.dispatch_to_widget(old, &WidgetEvent::FocusLost);
            }
        }
        self.focused = Some(id);
        self.focus_origin = Some(origin);
        self.a11y_dirty = true;
        self.dispatch_to_widget(id, &WidgetEvent::FocusGained { origin });
        self.scroll_focused_into_view(id);
        self.flush_commands();
    }

    /// After setting focus, ensure the focused widget is visible inside
    /// any ancestor scroll area (clips_children container).
    fn scroll_focused_into_view(&mut self, focused_id: WidgetId) {
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
                    );
                }
                break;
            }
            current = self.arena.parent(ancestor_id);
        }
    }

    /// Set focus to a specific widget (programmatic origin).
    pub fn focus(&mut self, id: WidgetId) {
        self.focus_with_origin(id, crate::focus::FocusOrigin::Programmatic);
    }

    /// Get the currently focused widget.
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// How the currently focused widget gained focus.
    pub fn focus_origin(&self) -> Option<crate::focus::FocusOrigin> {
        self.focus_origin
    }

    /// Cycle focus to the next/previous focusable widget (Tab/Shift-Tab).
    /// Traverses in document order (depth-first tree traversal).
    pub(super) fn cycle_focus(&mut self, reverse: bool) {
        let mut focusable = Vec::new();
        let roots = self.arena.roots();
        for root in roots {
            self.collect_focusable_tree_order(root, &mut focusable);
        }

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

        self.focus_with_origin(focusable[next_idx], crate::focus::FocusOrigin::Keyboard);
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
            if self.is_node_focusable(node) {
                out.push(id);
            }
            for &child in &node.children {
                self.collect_focusable_tree_order(child, out);
            }
        }
    }
}