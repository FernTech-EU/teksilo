use super::*;

impl WidgetTree {
    /// Get an immutable reference to a widget node (for internal use).
    #[allow(dead_code)]
    pub(crate) fn arena_get(&self, id: WidgetId) -> Option<&crate::arena::WidgetNode> {
        self.arena.get(id)
    }

    pub fn bounds(&self, id: WidgetId) -> Rect {
        self.arena.bounds(id)
    }

    pub fn children(&self, id: WidgetId) -> Vec<WidgetId> {
        self.arena.children(id).to_vec()
    }

    pub fn needs_layout(&self) -> bool {
        self.arena.any_needs_layout()
    }

    pub fn needs_paint(&self) -> bool {
        self.arena.any_needs_paint()
    }

    pub fn active_animation_count(&self) -> usize {
        self.animation_scheduler.active_count()
    }

    pub fn pending_tooltip_count(&self) -> usize {
        self.tooltips
            .iter()
            .filter(|entry| entry.overlay_id.is_none() && entry.real_hover_start.is_some())
            .count()
    }

    /// Whether there are pending idle callbacks to run.
    pub fn has_idle_work(&self) -> bool {
        !self.idle_queue.is_empty()
    }

    /// Drain and run all pending idle callbacks with the given time budget.
    /// Called by the event loop during idle periods between frames.
    pub fn run_idle_callbacks(&mut self, budget: std::time::Duration) {
        let callbacks = self.idle_queue.drain();
        for callback in callbacks {
            callback(crate::idle::IdleDeadline::new(budget));
        }
    }
}