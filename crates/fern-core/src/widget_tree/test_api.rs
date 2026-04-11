use super::*;

impl WidgetTree {
    /// Simulate a click at the center of a widget.
    pub fn click(&mut self, id: WidgetId) {
        let center = self.arena.bounds(id).center();
        self.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// Simulate pointer movement to a position.
    pub fn pointer_move(&mut self, position: Point) {
        self.dispatch_event(WidgetEvent::PointerMove { position });
    }

    /// Simulate a key press (down + up).
    pub fn press_key(&mut self, key: Key, modifiers: Modifiers) {
        self.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
        self.dispatch_event(WidgetEvent::KeyUp { key, modifiers });
    }

    /// Simulate typing text into the focused widget.
    pub fn type_text(&mut self, _widget: WidgetId, text: &str) {
        for ch in text.chars() {
            self.dispatch_event(WidgetEvent::KeyDown {
                key: Key::Character(ch),
                modifiers: Modifiers::NONE,
                text: Some(ch.to_string()),
            });
        }
    }

    /// Simulate a pointer down at a specific position with a specific button.
    pub fn pointer_down_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::PointerDown {
            position,
            button,
            modifiers: Modifiers::NONE,
        });
    }

    /// Simulate a pointer up at a specific position with a specific button.
    pub fn pointer_up_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::PointerUp {
            position,
            button,
            modifiers: Modifiers::NONE,
        });
    }

    /// Simulate a drag from one position to another.
    pub fn drag(&mut self, from: Point, to: Point) {
        self.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        self.dispatch_event(WidgetEvent::PointerMove { position: to });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// Get bounds of a child by index.
    pub fn child_bounds(&self, parent: WidgetId, index: usize) -> Rect {
        let children = self.children(parent);
        self.bounds(children[index])
    }

    /// Get a child widget ID by index.
    pub fn child_widget(&self, parent: WidgetId, index: usize) -> WidgetId {
        self.children(parent)[index]
    }

    /// Advance the simulated clock by the given duration.
    /// Triggers time-dependent behavior such as long-press gesture recognition
    /// and tooltip timers. Enables deterministic testing without real delays.
    pub fn advance_time(&mut self, duration: std::time::Duration) {
        self.sim_clock += duration;
        self.process_tooltips();
        self.process_delayed_overlays();
        self.process_pointer_leave_overlays();
        self.process_auto_dismiss_overlays();
    }

    /// Get the current simulated clock value.
    pub fn simulated_now(&self) -> std::time::Instant {
        self.sim_clock
    }

    /// Mark a widget as needing repaint.
    pub fn mark_needs_paint(&mut self, id: WidgetId) {
        self.arena.mark_needs_paint(id);
    }

    /// Set a widget subtree as dormant.
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.arena.set_dormant(id);
        self.arena.mark_ancestors_need_layout(id);
        self.cached_frame = None;
        self.a11y_dirty = true;
    }

    /// Activate a dormant widget subtree.
    pub fn activate(&mut self, id: WidgetId) {
        self.arena.activate(id);
        self.arena.mark_ancestors_need_layout(id);
        self.cached_frame = None;
        self.a11y_dirty = true;
    }

    /// Invalidate all per-widget paint caches and the assembled frame cache.
    /// Forces every widget to repaint on the next `render()` call.
    pub fn invalidate_all_paints(&mut self) {
        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = true;
                node.cached_paint = None;
            }
        }
        self.cached_frame = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use crate::test_widgets::{FillWidget, InsetWidget};

    #[test]
    fn child_bounds_helper() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(InsetWidget::new(5.0).set_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let child_bounds = tree.child_bounds(parent, 0);
        assert_eq!(child_bounds.x, 5.0);
    }

    #[test]
    fn state_get_set_and_derived() {
        let text = State::new(String::new());
        let is_empty = text.map(|value| value.is_empty());
        assert!(is_empty.get());
        text.set("hello".to_string());
        assert!(!is_empty.get());
    }

    #[test]
    fn advance_time_updates_simulated_clock() {
        let mut tree = WidgetTree::new();
        let start = tree.simulated_now();

        tree.advance_time(std::time::Duration::from_millis(500));
        let end = tree.simulated_now();

        assert_eq!(
            end.duration_since(start),
            std::time::Duration::from_millis(500)
        );
    }

    #[test]
    fn set_animated_interpolates_over_time() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::Linear,
        );

        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (*state.get() - 50.0).abs() < 2.0,
            "at 50%: {}",
            *state.get()
        );

        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (*state.get() - 100.0).abs() < 0.1,
            "at 100%: {}",
            *state.get()
        );

        assert!(!tree.has_active_animations());
    }

    #[test]
    fn set_animated_with_easing() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::EaseIn,
        );

        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (*state.get() - 25.0).abs() < 2.0,
            "ease-in at 50%: {}",
            *state.get()
        );
    }

    #[test]
    fn set_animated_replaces_in_flight() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::Linear,
        );
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!((*state.get() - 50.0).abs() < 2.0);

        state.set_animated(
            0.0,
            std::time::Duration::from_millis(100),
            fern_tokens::Easing::Linear,
        );
        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(
            (*state.get() - 25.0).abs() < 3.0,
            "mid-replace: {}",
            *state.get()
        );

        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(
            (*state.get() - 0.0).abs() < 0.5,
            "end-replace: {}",
            *state.get()
        );
    }

    #[test]
    fn animation_marks_widgets_dirty() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(100.0_f32);
        tree.register_animated_state(&state);

        let widget = tree.add(FillWidget::new());
        state.bind_to(
            widget,
            tree.binding_registry(),
            crate::state::BindingLevel::Relayout,
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        state.set_animated(
            0.0,
            std::time::Duration::from_millis(100),
            fern_tokens::Easing::Linear,
        );

        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(tree.needs_redraw());
    }
}
