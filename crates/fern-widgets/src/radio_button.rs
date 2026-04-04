//! RadioButton — mutually exclusive selection within a group.
//!
//! Non-generic: uses `usize` for values. Multiple RadioButtons share a
//! `State<usize>` — selecting one automatically deselects others.

use std::cell::RefCell;

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::composite_widget::{BuildContext, CompositeWidget};
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer};
use fern_core::state::{Reactive, State};
use fern_core::widget::{CursorIcon, EventContext};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::button::InteractionState;
use crate::primitives::{FixedSize, HStack, MinSize, RectWidget, TextWidget, ZStack};

/// A radio button that sets a shared `State<usize>` to its value when selected.
pub struct RadioButton {
    label: Option<String>,
    value: usize,
    selected: State<usize>,
    enabled: bool,
    tooltip_text: Option<String>,
    interaction: RefCell<Option<State<InteractionState>>>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
    visible_when_state: Option<Reactive<bool>>,
    enabled_when_state: Option<Reactive<bool>>,
}

impl RadioButton {
    pub fn new(value: usize, selected: State<usize>) -> Self {
        Self {
            label: None,
            value,
            selected,
            enabled: true,
            tooltip_text: None,
            interaction: RefCell::new(None),
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    pub fn visible_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.visible_when_state = Some(state.into());
        self
    }

    pub fn enabled_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.enabled_when_state = Some(state.into());
        self
    }

    fn select(&self) {
        self.selected.set(self.value);
    }

    fn is_selected(&self) -> bool {
        *self.selected.get() == self.value
    }

    fn set_interaction(&self, state: InteractionState) {
        if let Some(ref s) = *self.interaction.borrow() {
            s.set(state);
        }
    }

    fn interaction_state(&self) -> InteractionState {
        self.interaction
            .borrow()
            .as_ref()
            .map(|s| *s.get())
            .unwrap_or(InteractionState::Idle)
    }
}

impl std::fmt::Debug for RadioButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioButton")
            .field("label", &self.label)
            .field("value", &self.value)
            .finish()
    }
}

fn resolve_circle_border(
    state: InteractionState,
    selected: bool,
    colors: &fern_tokens::ColorTokens,
) -> Color {
    match state {
        InteractionState::Disabled => colors.disabled_fill,
        _ if selected => colors.primary,
        InteractionState::Hovered => colors.border_strong,
        _ => colors.border,
    }
}

fn resolve_focus_ring(state: InteractionState, colors: &fern_tokens::ColorTokens) -> Color {
    if state == InteractionState::Focused {
        colors.focus_ring
    } else {
        Color::TRANSPARENT
    }
}

impl CompositeWidget for RadioButton {
    fn build(&self, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme().clone();
        let selected = self.selected.clone();
        let value = self.value;

        let interaction = ctx.state(if self.enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        *self.interaction.borrow_mut() = Some(interaction.clone());

        // Outer circle border
        let border_color = {
            let colors = theme.colors.clone();
            let selected = selected.clone();
            interaction.map(move |s| resolve_circle_border(*s, *selected.get() == value, &colors))
        };
        let outer = RectWidget::new()
            .bind_border_color(border_color)
            .border_width(2.0)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_full));
        let outer_id = ctx.add(outer);
        let outer_sized = ctx.add(
            FixedSize::new()
                .bind_width(fern_core::state::Reactive::Static(18.0))
                .bind_height(fern_core::state::Reactive::Static(18.0))
                .set_child(outer_id),
        );

        // Inner dot — visible when selected via observer-driven State<bool>
        let dot_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| {
                if *s == InteractionState::Disabled {
                    colors.disabled_text
                } else {
                    colors.primary
                }
            })
        };
        let dot = RectWidget::new()
            .bind_background(dot_color)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_full));
        let dot_id = ctx.add(dot);
        let dot_sized = ctx.add(
            FixedSize::new()
                .bind_width(fern_core::state::Reactive::Static(10.0))
                .bind_height(fern_core::state::Reactive::Static(10.0))
                .set_child(dot_id),
        );

        // Drive dot visibility from the shared selected state
        ctx.visible_when(dot_sized, selected.map(move |s| *s == value));

        // Outer focus ring — 3px offset outside the 18×18 circle (24×24 total)
        let focus_ring_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_focus_ring(*s, &colors))
        };
        let focus_ring_width = interaction.map(|s| {
            if *s == InteractionState::Focused { 2.0_f32 } else { 0.0 }
        });
        let focus_ring = RectWidget::new()
            .bind_border_color(focus_ring_color)
            .bind_border_width(focus_ring_width)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_full));
        let focus_ring_id = ctx.add(focus_ring);
        let focus_ring_sized = ctx.add(
            FixedSize::new()
                .bind_width(fern_core::state::Reactive::Static(24.0))
                .bind_height(fern_core::state::Reactive::Static(24.0))
                .set_child(focus_ring_id),
        );

        let radio = ctx.add(
            ZStack::new()
                .add_child(focus_ring_sized)
                .add_child(outer_sized)
                .add_child(dot_sized),
        );

        let mut row = HStack::new().spacing(8.0).add_child(radio);
        if let Some(ref label) = self.label {
            let label_widget = TextWidget::new(label)
                .style(theme.typography.body.clone())
                .color(theme.colors.on_surface);
            let label_id = ctx.add(label_widget);
            row = row.add_child(label_id);
        }

        let row_id = ctx.add(row);
        let root_id = ctx.add(MinSize::new(48.0, 48.0).set_child(row_id));

        if let Some(ref tooltip_text) = self.tooltip_text {
            let tw = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tid = ctx.add(tw);
            ctx.attach_tooltip(root_id, tid, std::time::Duration::from_millis(500));
        }

        root_id
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }

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
                    self.select();
                }
                self.set_interaction(InteractionState::Hovered);
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position } => {
                self.tap_recognizer.process(&RawPointerEvent::Move {
                    position: *position,
                });
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
            WidgetEvent::KeyDown { key: Key::Space, .. } => {
                self.set_interaction(InteractionState::Pressed);
                EventResponse::Handled
            }
            WidgetEvent::KeyUp { key: Key::Space, .. } => {
                self.select();
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
                    self.select();
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        self.enabled
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::RadioButton);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        builder.set_toggled(self.is_selected());
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn take_visible_when(&mut self) -> Option<Reactive<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<Reactive<bool>> {
        self.enabled_when_state.take()
    }
}

fern_core::impl_composite_into_widget_tree!(RadioButton);

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn selecting_one_deselects_others() {
        use crate::primitives::VStack;
        let selected = State::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let r0 = tree.add_composite(RadioButton::new(0, selected.clone()).label("A"));
        let r1 = tree.add_composite(RadioButton::new(1, selected.clone()).label("B"));
        let r2 = tree.add_composite(RadioButton::new(2, selected.clone()).label("C"));
        let _root = tree.add(VStack::new().add_child(r0).add_child(r1).add_child(r2));
        tree.layout(SizeProposal::exact(200.0, 300.0));

        assert_eq!(*selected.get(), 0);
        tree.click(r1);
        assert_eq!(*selected.get(), 1);
        tree.click(r2);
        assert_eq!(*selected.get(), 2);
        tree.click(r0);
        assert_eq!(*selected.get(), 0);
    }

    #[test]
    fn space_selects() {
        let selected = State::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let _r0 = tree.add_composite(RadioButton::new(0, selected.clone()).label("A"));
        let r1 = tree.add_composite(RadioButton::new(1, selected.clone()).label("B"));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        tree.focus(r1);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(*selected.get(), 1);
    }

    #[test]
    fn accessibility() {
        let selected = State::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let r0 = tree.add_composite(RadioButton::new(0, selected.clone()).label("A"));
        let r1 = tree.add_composite(RadioButton::new(1, selected.clone()).label("B"));
        tree.layout(SizeProposal::exact(200.0, 200.0));

        let info0 = tree.accessibility_node(r0);
        assert_eq!(info0.role(), fern_core::accesskit::Role::RadioButton);
        assert!(!info0.is_toggled());

        let info1 = tree.accessibility_node(r1);
        assert!(info1.is_toggled());
    }

    #[test]
    fn accessibility_has_actions() {
        let selected = State::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let r0 = tree.add_composite(RadioButton::new(0, selected).label("A"));
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let info = tree.accessibility_node(r0);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }
}
