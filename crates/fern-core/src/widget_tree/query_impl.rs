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

    /// Borrow the widget at `id` as `&dyn Any` for concrete-type
    /// introspection. Uses the `Widget::as_any` hook — widgets that
    /// haven't opted in return `None`. Primarily for tests that need
    /// to inspect a widget's private Signal state.
    pub fn widget_as_any(&self, id: WidgetId) -> Option<&dyn std::any::Any> {
        self.arena.get(id).and_then(|node| node.widget.as_any())
    }

    pub fn children(&self, id: WidgetId) -> Vec<WidgetId> {
        self.arena.children(id).to_vec()
    }

    /// Parent widget id in the arena graph, or `None` for roots.
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.arena.parent(id)
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

    pub fn has_pending_modal_dismissal(&self) -> bool {
        self.pending_modal_dismissal
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
    use crate::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;
    use crate::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
    use fern_canvas::Size;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct FixedWidget(f32, f32);

    impl Widget for FixedWidget {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

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
        let widget = tree.add(FillWidget::new().on_tap(move |_pos, ctx| {
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
    fn set_locale_from_event_handler_is_parked_not_applied() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_tap(|_pos, ctx| {
            ctx.set_locale("fr-FR");
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.locale().is_none());

        tree.click(widget);

        // The tree's own locale signal must NOT have been flipped — the
        // app layer is responsible for routing the switch through
        // `WindowManager::set_locale` so the `I18nManager`'s active
        // locale, version signal, and RTL direction stay in sync.
        assert_eq!(tree.locale(), None);
        // The request is parked for the app layer to drain.
        assert_eq!(
            tree.take_pending_locale_request(),
            Some("fr-FR".to_string())
        );
        // Drained exactly once.
        assert_eq!(tree.take_pending_locale_request(), None);
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
        let trigger = tree.add(FillWidget::new().on_tap(move |_pos, ctx| {
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
        assert_eq!(
            requests[0].request.close_behavior,
            ModalCloseBehavior::Manual
        );
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
        let trigger = tree.add(FillWidget::new().on_tap(move |_pos, ctx| {
            ctx.present_modal(ModalRequest::in_tree(content));
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(trigger);
        assert_eq!(tree.drain_pending_modal_requests().len(), 1);
        assert!(tree.drain_pending_modal_requests().is_empty());
    }

    #[test]
    fn dismiss_modal_closes_centered_overlay_for_source_widget() {
        let mut tree = WidgetTree::new();
        let trigger = tree.add(FillWidget::new().label("Trigger"));
        let modal_content = tree.add(FixedWidget(120.0, 48.0).on_tap(|_pos, ctx| {
            ctx.dismiss_modal();
        }));
        tree.layout(SizeProposal::exact(320.0, 200.0));

        tree.show_overlay(OverlayRequest {
            content_id: modal_content,
            anchor: trigger,
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
        });
        tree.layout(SizeProposal::exact(320.0, 200.0));

        assert_eq!(tree.active_overlays().len(), 1);

        let center = tree
            .overlay_manager()
            .topmost_centered()
            .expect("expected centered modal overlay")
            .bounds
            .center();
        tree.pointer_down_button(center, PointerButton::Primary);
        tree.pointer_up_button(center, PointerButton::Primary);

        assert!(tree.active_overlays().is_empty());
        assert!(!tree.has_pending_modal_dismissal());
    }

    #[test]
    fn dismiss_modal_without_in_tree_modal_queues_window_dismissal() {
        let mut tree = WidgetTree::new();
        let trigger = tree.add(FillWidget::new().on_tap(|_pos, ctx| {
            ctx.dismiss_modal();
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(!tree.has_pending_modal_dismissal());

        tree.click(trigger);

        assert!(tree.has_pending_modal_dismissal());
        assert!(tree.drain_pending_modal_dismissal());
        assert!(!tree.has_pending_modal_dismissal());
    }
}
