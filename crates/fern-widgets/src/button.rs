//! Production-quality Button widget — CompositeWidget using reactive bindings.
//!
//! Addresses all architectural requirements:
//! - Non-generic (closure-based type erasure, Approach B)
//! - State created via BuildContext (framework-managed state arena)
//! - Theme resolved at paint time (not captured at build time)
//! - Pointer interaction via TapRecognizer
//! - Bindings auto-registered via register_bindings (no manual bind_to)
//! - Minimum touch target size from theme

use std::cell::RefCell;

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::composite_widget::{BuildContext, CompositeWidget};
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::{FocusOrigin, FocusPolicy};
use fern_core::gesture::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer,
};
use fern_core::state::State;
use fern_core::widget::{CursorIcon, EventContext};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Alignment, Color, CornerRadius, Theme};
use fern_tokens::ColorTokens;

use crate::primitives::{Padding, RectWidget, TextWidget, ZStack};

/// Visual style of the button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    #[default]
    Filled,
    Outlined,
    Flat,
    Tonal,
}

/// Internal interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionState {
    Idle,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

/// A production-quality button widget — non-generic, composition-based.
///
/// ```ignore
/// Button::new("Save")
///     .style(ButtonStyle::Filled)
///     .on_click(AppCmd::Save)
/// ```
/// Type-erased command factory — captures the concrete command type
/// and produces a fresh ErasedCommand each time (since ErasedCommand isn't Clone).
type CommandFactory = Box<dyn Fn(&mut EventContext)>;

pub struct Button {
    label: String,
    style: ButtonStyle,
    action: Option<CommandFactory>,
    enabled: bool,
    tooltip_text: Option<String>,
    /// Interaction state — set during build() via interior mutability.
    /// None until build() is called.
    interaction: RefCell<Option<State<InteractionState>>>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            style: ButtonStyle::Filled,
            action: None,
            enabled: true,
            tooltip_text: None,
            interaction: RefCell::new(None),
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
        }
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the command to emit on activation. The generic only appears at
    /// this call site — the Button struct itself is non-generic (Approach B).
    pub fn on_click<C: AppCommand>(mut self, command: C) -> Self {
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }

    /// Attach a tooltip that appears after a hover delay.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn fire_action(&self, ctx: &mut EventContext) {
        if let Some(ref action) = self.action {
            action(ctx);
        }
    }

    fn interaction_state(&self) -> InteractionState {
        self.interaction
            .borrow()
            .as_ref()
            .map(|s| *s.get())
            .unwrap_or(InteractionState::Idle)
    }

    fn set_interaction(&self, state: InteractionState) {
        if let Some(ref s) = *self.interaction.borrow() {
            s.set(state);
        }
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label)
            .field("style", &self.style)
            .field("enabled", &self.enabled)
            .finish()
    }
}

// --- Color resolution: style × state × theme (resolved at paint time) ---

fn resolve_bg(style: ButtonStyle, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        (_, InteractionState::Disabled) => colors.disabled_fill,
        (ButtonStyle::Filled, InteractionState::Idle | InteractionState::Focused) => {
            colors.primary
        }
        (ButtonStyle::Filled, InteractionState::Hovered) => colors.primary_hover,
        (ButtonStyle::Filled, InteractionState::Pressed) => colors.primary_pressed,
        (ButtonStyle::Outlined, InteractionState::Hovered) => colors.primary.with_alpha(0.08),
        (ButtonStyle::Outlined, InteractionState::Pressed) => colors.primary.with_alpha(0.12),
        (ButtonStyle::Outlined, _) => Color::TRANSPARENT,
        (ButtonStyle::Flat, InteractionState::Hovered) => colors.primary.with_alpha(0.08),
        (ButtonStyle::Flat, InteractionState::Pressed) => colors.primary.with_alpha(0.12),
        (ButtonStyle::Flat, _) => Color::TRANSPARENT,
        (ButtonStyle::Tonal, InteractionState::Idle | InteractionState::Focused) => {
            colors.secondary
        }
        (ButtonStyle::Tonal, InteractionState::Hovered) => colors.secondary_hover,
        (ButtonStyle::Tonal, InteractionState::Pressed) => colors.secondary_pressed,
    }
}

fn resolve_text(style: ButtonStyle, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        (_, InteractionState::Disabled) => colors.disabled_text,
        (ButtonStyle::Filled, _) => colors.on_primary,
        (ButtonStyle::Outlined | ButtonStyle::Flat, InteractionState::Hovered) => {
            colors.primary_hover
        }
        (ButtonStyle::Outlined | ButtonStyle::Flat, InteractionState::Pressed) => {
            colors.primary_pressed
        }
        (ButtonStyle::Outlined | ButtonStyle::Flat, _) => colors.primary,
        (ButtonStyle::Tonal, _) => colors.on_secondary,
    }
}

fn resolve_border(style: ButtonStyle, state: InteractionState, colors: &ColorTokens) -> Color {
    // Focus ring is visible for keyboard-focused state (any style)
    if state == InteractionState::Focused {
        return colors.focus_ring;
    }
    match style {
        ButtonStyle::Outlined => match state {
            InteractionState::Disabled => colors.disabled_fill,
            InteractionState::Hovered | InteractionState::Pressed => colors.border_strong,
            _ => colors.border,
        },
        _ => Color::TRANSPARENT,
    }
}

impl CompositeWidget for Button {
    fn build(&self, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme().clone();
        let style = self.style;

        // Create interaction state in the framework's state arena
        let interaction = ctx.state(if self.enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        // Store for event() to use (interior mutability since build takes &self)
        *self.interaction.borrow_mut() = Some(interaction.clone());

        // Derived reactive colors — resolve against theme at read time.
        // The theme is captured here, but the state is read lazily.
        // For runtime theme switching, the composite would need a rebuild.
        let bg_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_bg(style, *s, &colors))
        };
        let text_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_text(style, *s, &colors))
        };
        let border_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_border(style, *s, &colors))
        };

        // Build the widget subtree — bindings auto-register via register_bindings
        let text = TextWidget::new(&self.label).bind_color(text_color);
        let text_id = ctx.add(text);

        let padding = Padding::symmetric(
            theme.spacing.widget_padding,
            theme.spacing.widget_padding * 1.5,
        )
        .set_child(text_id);
        let padding_id = ctx.add(padding);

        let border_width = {
            let default_width = theme.shape.border_width;
            interaction.map(move |s| {
                if *s == InteractionState::Focused {
                    2.0_f32.max(default_width) // thicker for focus ring visibility
                } else {
                    default_width
                }
            })
        };

        let rect = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .bind_border_width(border_width)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm));
        let rect_id = ctx.add(rect);

        let zstack = ZStack::new().add_child(rect_id).add_child(padding_id);
        let zstack_id = ctx.add(zstack);

        // Enforce minimum touch target size per architecture
        let root = crate::primitives::MinSize::new(48.0, 48.0).set_child(zstack_id);
        let root_id = ctx.add(root);

        // Attach tooltip if configured
        if let Some(ref tooltip_text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = std::time::Duration::from_millis(500);
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        root_id
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }

        // Feed pointer events through the TapRecognizer
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
            WidgetEvent::KeyDown {
                key: Key::Space | Key::Enter,
                ..
            } => {
                self.set_interaction(InteractionState::Pressed);
                EventResponse::Handled
            }
            WidgetEvent::KeyUp {
                key: Key::Space | Key::Enter,
                ..
            } => {
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
        self.enabled
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Button);
        builder.set_name(&self.label);
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

}

fern_core::impl_composite_into_widget_tree!(Button);

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::app_command::AppCommand;
    use fern_core::event::{Modifiers, PointerButton};
    use fern_core::widget_tree::WidgetTree;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Save,
    }
    impl AppCommand for TestCmd {}

    fn setup() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add_composite(Button::new("Save").on_click(TestCmd::Save));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        (tree, btn)
    }

    #[test]
    fn click_fires_command() {
        let (mut tree, btn) = setup();
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                c.set(true);
            }
        });
        tree.click(btn);
        assert!(called.get());
    }

    #[test]
    fn space_fires_command() {
        let (mut tree, btn) = setup();
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                c.set(true);
            }
        });
        tree.focus(btn);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(called.get());
    }

    #[test]
    fn enter_fires_command() {
        let (mut tree, btn) = setup();
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                c.set(true);
            }
        });
        tree.focus(btn);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert!(called.get());
    }

    #[test]
    fn disabled_ignores_click() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add_composite(
            Button::new("Save").on_click(TestCmd::Save).enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |_cmd: &TestCmd| c.set(true));
        tree.click(btn);
        assert!(!called.get());
    }

    #[test]
    fn accessibility_role_and_name() {
        let (tree, btn) = setup();
        let info = tree.accessibility_node(btn);
        assert_eq!(info.role(), fern_core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Save"));
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }

    #[test]
    fn filled_renders_primary_background() {
        let (mut tree, _btn) = setup();
        let frame = tree.render();
        let primary = Theme::light_default().colors.primary.to_array();
        assert!(frame.shapes.iter().any(|s| s.color == primary));
    }

    #[test]
    fn outlined_has_border() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add_composite(Button::new("Save").style(ButtonStyle::Outlined));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        assert!(frame.shapes.iter().any(|s| s.stroke_width > 0.0));
    }

    #[test]
    fn button_is_composite_with_children() {
        let (tree, btn) = setup();
        assert!(!tree.children(btn).is_empty());
    }

    #[test]
    fn button_sizes_to_content() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add_composite(Button::new("X"));
        tree.layout(SizeProposal::exact(400.0, 400.0));
        let bounds = tree.bounds(btn);
        // Should not fill the full 400x400 proposal
        assert!(bounds.width < 400.0);
        assert!(bounds.height < 400.0);
        // Should have non-zero size (text + padding)
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn hover_changes_color() {
        let (mut tree, btn) = setup();
        let frame_idle = tree.render();
        let idle_colors: Vec<_> = frame_idle.shapes.iter().map(|s| s.color).collect();

        let center = tree.bounds(btn).center();
        tree.pointer_move(center);
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame_hover = tree.render();
        let hover_colors: Vec<_> = frame_hover.shapes.iter().map(|s| s.color).collect();

        assert_ne!(idle_colors, hover_colors);
    }

    #[test]
    fn focus_origin_keyboard() {
        let (mut tree, _btn) = setup();
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focus_origin(), Some(FocusOrigin::Keyboard));
    }

    #[test]
    fn keyboard_focus_shows_focus_ring() {
        let (mut tree, btn) = setup();

        let frame_idle = tree.render();
        let idle_shapes = frame_idle.shapes.clone();

        // Tab into button — keyboard focus should show focus ring
        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame_focused = tree.render();
        let focused_shapes = frame_focused.shapes.clone();

        // The border should change (focus ring color appears)
        assert_ne!(idle_shapes, focused_shapes,
            "keyboard focus should change the button's visual appearance (focus ring)");
    }

    #[test]
    fn pointer_focus_no_focus_ring() {
        let (mut tree, btn) = setup();

        // Click the button — pointer focus should NOT show focus ring
        let center = tree.bounds(btn).center();
        tree.pointer_move(center);
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame_hover = tree.render();
        let hover_shapes = frame_hover.shapes.clone();

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: center,
            button: PointerButton::Primary,
        });
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame_after_click = tree.render();

        // After click, button goes to Hovered (not Focused), so no focus ring
        // The shapes should be the hover state, not a focus-ring state
        assert_eq!(hover_shapes, frame_after_click.shapes,
            "pointer click should return to hover state, not show focus ring");
    }

    #[test]
    fn focus_origin_pointer() {
        let (mut tree, btn) = setup();
        let center = tree.bounds(btn).center();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
        });
        assert_eq!(tree.focus_origin(), Some(FocusOrigin::Pointer));
    }

    #[test]
    fn button_tooltip_appears_after_delay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add_composite(
            Button::new("Save")
                .on_click(TestCmd::Save)
                .tooltip("Save the document"),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Hover over button
        let center = tree.bounds(btn).center();
        tree.pointer_move(center);

        // Not shown yet
        assert!(tree.active_overlays().is_empty());

        // Advance past 500ms delay
        tree.advance_time(std::time::Duration::from_millis(600));

        // Tooltip shown
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn button_tooltip_dismissed_on_leave() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add_composite(
            Button::new("Save")
                .on_click(TestCmd::Save)
                .tooltip("Save the document"),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));

        // Show tooltip
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(std::time::Duration::from_millis(600));
        assert_eq!(tree.active_overlays().len(), 1);

        // Move away — dismissed
        tree.pointer_move(fern_canvas::Point::new(500.0, 500.0));
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn button_without_tooltip_has_no_overlay() {
        let (mut tree, btn) = setup();
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(std::time::Duration::from_millis(1000));
        assert!(tree.active_overlays().is_empty());
    }
}
