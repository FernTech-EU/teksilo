//! Accordion — a collapsible section with clickable header.
//!
//! Content visibility is animated via `MaxSize::bind_max_height()` with an
//! animated `State<f32>`. When collapsed, max_height animates to 0; when
//! expanded, it animates to a large value (content sizes naturally within).

use std::time::Duration;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, IntoWidgetTree, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::Easing;

use fern_tokens::{Color, CornerRadius};

use crate::primitives::{HStack, IconWidget, MaxSize, RectWidget, Spacer, TextWidget, VStack, ZStack};

/// Large enough to never clip content when fully expanded.
const EXPANDED_MAX_HEIGHT: f32 = 10000.0;

/// A collapsible section with a clickable header that toggles content visibility.
///
/// Content must be pre-registered via `set_content(id)`.
pub struct Accordion {
    title: String,
    expanded: Signal<bool>,
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn IntoWidgetTree>>,
    content_height: Option<Signal<f32>>,
    keyboard_focused: Option<Signal<bool>>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
    root_child_id: Option<WidgetId>,
}

impl Accordion {
    pub fn new(title: impl Into<String>, expanded: Signal<bool>) -> Self {
        Self {
            title: title.into(),
            expanded,
            content_id: None,
            pending_content: None,
            content_height: None,
            keyboard_focused: None,
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            root_child_id: None,
        }
    }

    /// Set the content widget by pre-registered ID.
    pub fn set_content(mut self, id: WidgetId) -> Self {
        self.content_id = Some(id);
        self
    }

    /// Set an inline content widget (deferred insertion).
    pub fn content(mut self, widget: impl IntoWidgetTree) -> Self {
        self.pending_content = Some(Box::new(widget));
        self
    }

    fn toggle_expanded(&self, _ctx: &mut EventContext) {
        let new_expanded = !self.expanded.get();
        self.expanded.set(new_expanded);

        // Animate the content height
        if let Some(ref height) = self.content_height {
            let target = if new_expanded { EXPANDED_MAX_HEIGHT } else { 0.0 };
            height.animate_to(target, Duration::from_millis(200), Easing::EaseInOut);
        }
    }
}

impl std::fmt::Debug for Accordion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accordion")
            .field("title", &self.title)
            .finish()
    }
}

impl Widget for Accordion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve deferred content if provided
        if let Some(pending) = self.pending_content.take() {
            self.content_id = Some(ctx.add_boxed(pending));
        }

        let theme = ctx.theme().clone();
        let expanded = self.expanded.clone();
        let is_expanded = expanded.get();

        // Keyboard focus state for focus ring
        let kb_focused = ctx.signal(false);
        self.keyboard_focused = Some(kb_focused.clone());

        // Header: title + spacer + chevron icon
        // Use two chevrons with visible_when so the icon updates reactively
        let chevron_down_id = ctx.add(
            IconWidget::chevron_down(16.0).color(theme.colors.on_surface),
        );
        let chevron_right_id = ctx.add(
            IconWidget::chevron_right(16.0).color(theme.colors.on_surface),
        );
        ctx.visible_when(chevron_down_id, expanded.clone());
        ctx.visible_when(chevron_right_id, expanded.map(|v| !*v));

        let title_widget = TextWidget::new(&self.title)
            .style(theme.typography.body.clone())
            .color(theme.colors.on_surface);
        let title_id = ctx.add(title_widget);
        let spacer_id = ctx.add(Spacer::new());

        let header = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(title_id)
                .add_child(spacer_id)
                .add_child(chevron_down_id)
                .add_child(chevron_right_id),
        );

        // Focus ring border around the header
        let focus_ring_color = {
            let colors = theme.colors.clone();
            kb_focused.map(move |focused| {
                if *focused { colors.focus_ring } else { Color::TRANSPARENT }
            })
        };
        let focus_ring_width = kb_focused.map(|focused| if *focused { 2.0_f32 } else { 0.0 });
        let focus_rect = RectWidget::new()
            .bind_border_color(focus_ring_color)
            .bind_border_width(focus_ring_width)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm));
        let focus_rect_id = ctx.add(focus_rect);
        let header_with_ring = ctx.add(ZStack::new().add_child(focus_rect_id).add_child(header));

        let mut vstack = VStack::new().spacing(theme.spacing.xs).add_child(header_with_ring);
        if let Some(content_id) = self.content_id {
            // Wrap content in MaxSize with animated height for smooth expand/collapse
            let initial_height = if is_expanded { EXPANDED_MAX_HEIGHT } else { 0.0 };
            let height_state = ctx.animated_signal(initial_height);
            self.content_height = Some(height_state.clone());

            let wrapper = ctx.add(
                MaxSize::new(f32::MAX, EXPANDED_MAX_HEIGHT)
                    .bind_max_height(height_state)
                    .set_child(content_id),
            );
            vstack = vstack.add_child(wrapper);
        }

        let root = ctx.add(vstack);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return size;
        }
        proposal.resolve(0.0, 0.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = fern_canvas::Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        match event {
            WidgetEvent::PointerDown { position, button } => {
                self.tap_recognizer.process(&RawPointerEvent::Down {
                    position: *position,
                    button: *button,
                });
                EventResponse::Handled
            }
            WidgetEvent::PointerUp { position, button } => {
                let result = self.tap_recognizer.process(&RawPointerEvent::Up {
                    position: *position,
                    button: *button,
                });
                if matches!(result, GestureResult::Recognized(GestureEvent::Tap { .. })) {
                    self.toggle_expanded(ctx);
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position } => {
                self.tap_recognizer.process(&RawPointerEvent::Move { position: *position });
                EventResponse::Ignored
            }
            WidgetEvent::PointerEnter => {
                ctx.set_cursor(CursorIcon::Pointer);
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.tap_recognizer.reset();
                ctx.set_cursor(CursorIcon::Default);
                EventResponse::Handled
            }
            WidgetEvent::KeyDown { key: Key::Space | Key::Enter, .. } => {
                EventResponse::Handled
            }
            WidgetEvent::KeyUp { key: Key::Space | Key::Enter, .. } => {
                self.toggle_expanded(ctx);
                EventResponse::Handled
            }
            WidgetEvent::FocusGained { origin } => {
                self.focus_origin = Some(*origin);
                if let Some(ref s) = self.keyboard_focused {
                    s.set(*origin == FocusOrigin::Keyboard);
                }
                EventResponse::Handled
            }
            WidgetEvent::FocusLost => {
                self.focus_origin = None;
                if let Some(ref s) = self.keyboard_focused {
                    s.set(false);
                }
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Button);
        builder.set_name(&self.title);
        builder.set_expanded(self.expanded.get());
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn accordion_builds_collapsed() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let acc = tree.add(Accordion::new("Section", expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.width > 0.0);
    }

    #[test]
    fn click_toggles_expanded_state() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let acc = tree.add(Accordion::new("Section", expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        tree.click(acc);
        assert!(expanded.get());
        tree.click(acc);
        assert!(!expanded.get());
    }

    #[test]
    fn accordion_with_content() {
        use crate::primitives::TextWidget;
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let content = tree.add(TextWidget::new("Content text"));
        let acc = tree.add(
            Accordion::new("Details", expanded.clone()).set_content(content),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.height > 0.0);
    }

    #[test]
    fn accessibility() {
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let acc = tree.add(Accordion::new("Details", expanded));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let info = tree.accessibility_node(acc);
        assert_eq!(info.name(), Some("Details"));
        assert!(info.is_expanded());
    }

    #[test]
    fn content_dormant_when_collapsed() {
        use crate::primitives::TextWidget;
        use std::time::Duration;

        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let content = tree.add(TextWidget::new("Some content text here"));
        let acc = tree.add(
            Accordion::new("Section", expanded.clone()).set_content(content),
        );
        tree.layout(SizeProposal { width: Some(300.0), height: None });
        let collapsed_height = tree.bounds(acc).height;

        // Click to expand
        tree.click(acc);
        assert!(expanded.get(), "should be expanded after click");

        // Tick animation to completion (accordion uses 200ms animation)
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal { width: Some(300.0), height: None });
        let expanded_height = tree.bounds(acc).height;

        assert!(
            expanded_height > collapsed_height,
            "expanded height ({}) should be greater than collapsed height ({})",
            expanded_height,
            collapsed_height
        );
    }
}
