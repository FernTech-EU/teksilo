//! Link — a clickable text label with underline.
//!
//! Follows the Button pattern for interaction but renders as underlined text.

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::button::InteractionState;
use crate::primitives::{RectWidget, TextWidget, VStack, ZStack};

type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// A clickable text link with underline.
pub struct Link {
    text: String,
    url: Option<String>,
    action: Option<CommandFactory>,
    tooltip_text: Option<String>,
    interaction: Option<Signal<InteractionState>>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
    root_child_id: Option<WidgetId>,
}

impl Link {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            url: None,
            action: None,
            tooltip_text: None,
            interaction: None,
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            root_child_id: None,
        }
    }

    pub fn on_click<C: AppCommand>(mut self, command: C) -> Self {
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }

    /// Set a URL for the link (informational — not automatically opened).
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    /// Get the URL, if set.
    pub fn get_url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    fn fire_action(&self, ctx: &mut EventContext) {
        if let Some(ref action) = self.action {
            action(ctx);
        }
    }

    fn set_interaction(&self, state: InteractionState) {
        if let Some(ref s) = self.interaction {
            s.set(state);
        }
    }

    fn interaction_state(&self) -> InteractionState {
        self.interaction
            .as_ref()
            .map(|s| s.get())
            .unwrap_or(InteractionState::Idle)
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("text", &self.text)
            .finish()
    }
}

fn resolve_link_color(state: InteractionState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        InteractionState::Disabled => colors.disabled_text,
        InteractionState::Hovered => colors.primary_hover,
        InteractionState::Pressed => colors.primary_pressed,
        InteractionState::Focused => colors.primary,
        InteractionState::Idle => colors.primary,
    }
}

fn resolve_focus_border(state: InteractionState, colors: &fern_tokens::ColorTokens) -> Color {
    if state == InteractionState::Focused {
        colors.focus_ring
    } else {
        Color::TRANSPARENT
    }
}

impl Widget for Link {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let interaction = ctx.signal(InteractionState::Idle);
        self.interaction = Some(interaction.clone());

        let text_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_link_color(*s, &colors))
        };

        let underline_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_link_color(*s, &colors))
        };

        let text = TextWidget::new(&self.text)
            .style(theme.typography.body.clone())
            .bind_color(text_color);
        let text_id = ctx.add(text);

        // 1px underline below the text
        let underline = RectWidget::new()
            .bind_background(underline_color);
        let underline_id = ctx.add(underline);
        let underline_sized = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_height(1.0_f32)
                .set_child(underline_id),
        );

        let content_id = ctx.add(VStack::new().spacing(0.0).add_child(text_id).add_child(underline_sized));

        // Focus ring border — visible only on keyboard focus
        let border_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_focus_border(*s, &colors))
        };
        let border_width = interaction.map(move |s| {
            if *s == InteractionState::Focused { 2.0_f32 } else { 0.0 }
        });
        let focus_rect = RectWidget::new()
            .bind_border_color(border_color)
            .bind_border_width(border_width)
            .corner_radius(fern_tokens::CornerRadius::uniform(theme.shape.radius_sm));
        let focus_rect_id = ctx.add(focus_rect);

        let root_id = ctx.add(ZStack::new().add_child(focus_rect_id).add_child(content_id));

        if let Some(ref tooltip_text) = self.tooltip_text {
            let tw = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tid = ctx.add(tw);
            ctx.attach_tooltip(root_id, tid, std::time::Duration::from_millis(500));
        }

        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        if let Some(root) = self.root_child_id {
            if let Some(size) = ctx.child_size(root, proposal) {
                return size;
            }
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
                self.set_interaction(InteractionState::Pressed);
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
                    self.fire_action(ctx);
                }
                self.set_interaction(InteractionState::Hovered);
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position } => {
                self.tap_recognizer.process(&RawPointerEvent::Move { position: *position });
                EventResponse::Ignored
            }
            WidgetEvent::PointerEnter => {
                self.set_interaction(InteractionState::Hovered);
                ctx.set_cursor(CursorIcon::Pointer);
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.set_interaction(InteractionState::Idle);
                self.tap_recognizer.reset();
                ctx.set_cursor(CursorIcon::Default);
                EventResponse::Handled
            }
            WidgetEvent::KeyDown { key: Key::Space | Key::Enter, .. } => {
                self.set_interaction(InteractionState::Pressed);
                EventResponse::Handled
            }
            WidgetEvent::KeyUp { key: Key::Space | Key::Enter, .. } => {
                self.fire_action(ctx);
                self.set_interaction(InteractionState::Focused);
                EventResponse::Handled
            }
            WidgetEvent::FocusGained { origin } => {
                self.focus_origin = Some(*origin);
                if self.interaction_state() == InteractionState::Idle {
                    self.set_interaction(InteractionState::Focused);
                }
                EventResponse::Handled
            }
            WidgetEvent::FocusLost => {
                self.focus_origin = None;
                self.set_interaction(InteractionState::Idle);
                EventResponse::Handled
            }
            WidgetEvent::AccessAction { action, .. } => {
                if *action == fern_core::accesskit::Action::Click {
                    self.fire_action(ctx);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Link);
        builder.set_name(&self.text);
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
    use fern_core::app_command::AppCommand;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Navigate,
    }
    impl AppCommand for TestCmd {}

    #[test]
    fn click_fires_command() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let link = tree.add(Link::new("Go here").on_click(TestCmd::Navigate));
        tree.layout(SizeProposal::exact(200.0, 50.0));

        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Navigate {
                c.set(true);
            }
        });
        tree.click(link);
        assert!(called.get());
    }

    #[test]
    fn accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let link = tree.add(Link::new("Go here"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(link);
        assert_eq!(info.role(), fern_core::accesskit::Role::Link);
        assert_eq!(info.name(), Some("Go here"));
    }

    #[test]
    fn accessibility_has_actions() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let link = tree.add(Link::new("Go here"));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(link);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }
}
