use super::*;

impl WidgetTree {
    /// Simulate a click at the center of a widget.
    pub fn click(&mut self, id: WidgetId) {
        let center = self.arena.bounds(id).center();
        self.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
        });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: center,
            button: PointerButton::Primary,
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
        self.dispatch_event(WidgetEvent::PointerDown { position, button });
    }

    /// Simulate a pointer up at a specific position with a specific button.
    pub fn pointer_up_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::PointerUp { position, button });
    }

    /// Simulate a drag from one position to another.
    pub fn drag(&mut self, from: Point, to: Point) {
        self.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
        });
        self.dispatch_event(WidgetEvent::PointerMove { position: to });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
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