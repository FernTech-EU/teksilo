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

    pub fn has_pending_modal_requests(&self) -> bool {
        !self.pending_modal_requests.is_empty()
    }

    pub fn current_cursor(&self) -> crate::widget::CursorIcon {
        self.current_cursor
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
    use crate::widget_builder::WidgetBuilder;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn destroy_removes_from_arena() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Gone"));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.find_by_label("Gone").is_some());

        tree.arena.destroy(widget);
        assert!(tree.find_by_label("Gone").is_none());
    }

    #[test]
    fn idle_callback_requested_from_event_handler() {
        let called = Rc::new(Cell::new(false));
        let called_flag = called.clone();
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_tap(move |ctx| {
            let called = called_flag.clone();
            ctx.request_idle_callback(move |_deadline| {
                called.set(true);
            });
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(!tree.has_idle_work());

        tree.click(widget);

        assert!(tree.has_idle_work());
        assert!(!called.get());

        tree.run_idle_callbacks(std::time::Duration::from_millis(16));

        assert!(called.get());
        assert!(!tree.has_idle_work());
    }

    #[test]
    fn idle_deadline_provides_time_budget() {
        let deadline = crate::idle::IdleDeadline::new(std::time::Duration::from_millis(100));
        assert!(!deadline.did_timeout());
        assert!(deadline.time_remaining() > std::time::Duration::ZERO);
    }

    #[test]
    fn modal_request_requested_from_event_handler() {
        let mut tree = WidgetTree::new();
        let content = tree.add(FillWidget::new().label("Modal content"));
        let trigger = tree.add(FillWidget::new().on_tap(move |ctx| {
            ctx.present_modal(
                ModalRequest::in_tree(content)
                    .presentation(ModalPresentation::InTree)
                    .close_behavior(ModalCloseBehavior::Manual),
            );
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(!tree.has_pending_modal_requests());

        tree.click(trigger);

        assert!(tree.has_pending_modal_requests());
        let requests = tree.drain_pending_modal_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].source_widget, trigger);
        assert_eq!(requests[0].request.presentation, ModalPresentation::InTree);
        assert_eq!(requests[0].request.close_behavior, ModalCloseBehavior::Manual);
        match requests[0].request.content {
            ModalContent::ExistingWidget(id) => assert_eq!(id, content),
            ModalContent::Deferred(_) => panic!("expected ExistingWidget content"),
        }
        assert!(!tree.has_pending_modal_requests());
    }

    #[test]
    fn draining_modal_requests_clears_queue() {
        let mut tree = WidgetTree::new();
        let content = tree.add(FillWidget::new());
        let trigger = tree.add(FillWidget::new().on_tap(move |ctx| {
            ctx.present_modal(ModalRequest::in_tree(content));
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(trigger);
        assert_eq!(tree.drain_pending_modal_requests().len(), 1);
        assert!(tree.drain_pending_modal_requests().is_empty());
    }
}
