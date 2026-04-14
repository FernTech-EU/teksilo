//! Accordion — a collapsible section with clickable header.
//!
//! Content visibility is animated via `MaxSize::bind_max_height()` with an
//! animated `Signal<f32>`. When collapsed, max_height animates to 0; when
//! expanded, it animates to a large value (content sizes naturally within).
//! V2 attached handlers — no event() override.

use std::time::Duration;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Easing;

use crate::primitives::{HStack, IconWidget, MaxSize, Spacer, TextWidget, VStack};

/// Large enough to never clip content when fully expanded.
const EXPANDED_MAX_HEIGHT: f32 = 10000.0;

/// A collapsible section with a clickable header that toggles content visibility.
///
/// Content must be pre-registered via `set_content(id)`.
pub struct Accordion {
    title: String,
    expanded: Signal<bool>,
    content_id: Option<WidgetId>,
    pending_content: Option<Box<dyn Widget>>,
    content_height: Option<Signal<f32>>,
    root_child_id: Option<WidgetId>,
}

impl Accordion {
    pub fn new(title: impl Into<fern_i18n::LocalizedString>, expanded: Signal<bool>) -> Self {
        let ls: fern_i18n::LocalizedString = title.into();
        Self {
            title: ls.resolve_now(),
            expanded,
            content_id: None,
            pending_content: None,
            content_height: None,
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw title in `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(title: impl Into<String>, expanded: Signal<bool>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(title), expanded)
    }

    /// Set the content widget by pre-registered ID.
    pub fn set_content(mut self, id: WidgetId) -> Self {
        self.content_id = Some(id);
        self
    }

    /// Set an inline content widget (deferred insertion).
    pub fn content(mut self, widget: impl Widget + 'static) -> Self {
        self.pending_content = Some(Box::new(widget));
        self
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

        // Header: title + spacer + chevron icon
        // Use two chevrons with visible_when so the icon updates reactively
        let chevron_down_id =
            ctx.add(IconWidget::chevron_down(16.0).color(theme.colors.text_primary));
        let chevron_right_id =
            ctx.add(IconWidget::chevron_right(16.0).color(theme.colors.text_primary));
        ctx.visible_when(chevron_down_id, expanded.clone());
        ctx.visible_when(chevron_right_id, expanded.map(|v| !*v));

        let title_widget = TextWidget::new_literal(&self.title)
            .style(theme.typography.body.clone())
            .color(theme.colors.text_primary);
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

        // Focus ring — drawn outside the header on keyboard focus.
        let header_with_ring = ctx.add(
            crate::primitives::FocusRing::new(kb_focused.clone())
                .corner_radius(theme.components.accordion.corner_radius)
                .set_child(header),
        );

        let mut vstack = VStack::new()
            .spacing(2.0)
            .add_child(header_with_ring);
        if let Some(content_id) = self.content_id {
            // Wrap content in MaxSize with animated height for smooth expand/collapse
            let initial_height = if is_expanded {
                EXPANDED_MAX_HEIGHT
            } else {
                0.0
            };
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

        // --- V2 attached handlers ---
        let expanded_tap = self.expanded.clone();
        let expanded_key = self.expanded.clone();
        let height_tap = self.content_height.clone();
        let height_key = self.content_height.clone();
        let kb_focused_focus = kb_focused.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, _ctx: &mut EventContext| {
                    let new_expanded = !expanded_tap.get();
                    expanded_tap.set(new_expanded);
                    if let Some(ref height) = height_tap {
                        let target = if new_expanded {
                            EXPANDED_MAX_HEIGHT
                        } else {
                            0.0
                        };
                        height.animate_to(target, Duration::from_millis(200), Easing::EaseInOut);
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => EventResponse::Handled,
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            let new_expanded = !expanded_key.get();
                            expanded_key.set(new_expanded);
                            if let Some(ref height) = height_key {
                                let target = if new_expanded {
                                    EXPANDED_MAX_HEIGHT
                                } else {
                                    0.0
                                };
                                height.animate_to(
                                    target,
                                    Duration::from_millis(200),
                                    Easing::EaseInOut,
                                );
                            }
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                move |gained: bool, _ctx: &mut EventContext| {
                    // Only show focus ring for keyboard focus (approximation:
                    // always show on gain, clear on loss — the V1 code checked origin)
                    kb_focused_focus.set(gained);
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

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
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.width > 0.0);
    }

    #[test]
    fn click_toggles_expanded_state() {
        let expanded = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()));
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
        let content = tree.add(TextWidget::new_literal("Content text"));
        let acc = tree.add(Accordion::new_literal("Details", expanded.clone()).set_content(content));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let b = tree.bounds(acc);
        assert!(b.height > 0.0);
    }

    #[test]
    fn accessibility() {
        let expanded = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let acc = tree.add(Accordion::new_literal("Details", expanded));
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
        let content = tree.add(TextWidget::new_literal("Some content text here"));
        let acc = tree.add(Accordion::new_literal("Section", expanded.clone()).set_content(content));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let collapsed_height = tree.bounds(acc).height;

        // Click to expand
        tree.click(acc);
        assert!(expanded.get(), "should be expanded after click");

        // Tick animation to completion (accordion uses 200ms animation)
        tree.tick_animations(Duration::from_millis(250));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let expanded_height = tree.bounds(acc).height;

        assert!(
            expanded_height > collapsed_height,
            "expanded height ({}) should be greater than collapsed height ({})",
            expanded_height,
            collapsed_height
        );
    }
}
