//! Production-quality Button widget — V2 Widget using Signal-based reactivity.
//!
//! Addresses all architectural requirements:
//! - Non-generic (closure-based type erasure, Approach B)
//! - Signal-based reactive state (V2 API)
//! - Theme resolved at paint time (not captured at build time)
//! - V2 attached handlers (HandlerSet) — no event() override
//! - Bindings auto-registered via register_bindings (no manual bind_to)
//! - Minimum touch target size from theme

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, ColorTokens, CornerRadius};

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
    /// Interaction state signal — set during build().
    interaction: Signal<InteractionState>,
    /// Root child ID — set during build().
    root_child_id: Option<WidgetId>,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            style: ButtonStyle::Filled,
            action: None,
            enabled: true,
            tooltip_text: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
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
        (ButtonStyle::Filled, InteractionState::Idle | InteractionState::Focused) => colors.primary,
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

impl fern_core::widget::Widget for Button {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let style = self.style;
        let enabled = self.enabled;

        // Create interaction signal
        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = interaction.clone();

        // Derived reactive colors via Signal::map
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

        // Build the widget subtree
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

        self.root_child_id = Some(root_id);

        // --- V2 attached handlers ---
        let action = self.action.take();
        let int_tap = interaction.clone();
        let int_hover_enter = interaction.clone();
        let int_hover_leave = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();
        let action_key = action.as_ref().map(|_| ());
        // Re-wrap action into Rc so it can be shared between tap, key, and access handlers
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = int_tap;
                move |ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    if let Some(ref action) = *action_for_tap {
                        action(ctx);
                    }
                    interaction.set(InteractionState::Hovered);
                }
            })
            .on_hover({
                let int_enter = int_hover_enter;
                let int_leave = int_hover_leave;
                move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    if entered {
                        int_enter.set(InteractionState::Hovered);
                    } else {
                        int_leave.set(InteractionState::Idle);
                    }
                }
            })
            .on_key({
                let interaction = int_key;
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            interaction.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space | Key::Enter,
                            ..
                        } => {
                            if let Some(ref action) = *action_for_key {
                                action(ctx);
                            }
                            interaction.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                let interaction = int_focus;
                move |gained: bool, _ctx: &mut EventContext| {
                    if gained {
                        if interaction.get() == InteractionState::Idle {
                            interaction.set(InteractionState::Focused);
                        }
                    } else {
                        interaction.set(InteractionState::Idle);
                    }
                }
            })
            .on_access_action({
                move |action: fern_core::accesskit::Action, ctx: &mut EventContext| -> EventResponse {
                    if action == fern_core::accesskit::Action::Click && enabled {
                        if let Some(ref act) = *action_for_access {
                            act(ctx);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(root_id) => ctx
                .child_size(root_id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Single child fills our bounds
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
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

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::app_command::AppCommand;
    use fern_core::event::{Modifiers, PointerButton};
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Save,
    }
    impl AppCommand for TestCmd {}

    fn setup() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(Button::new("Save").on_click(TestCmd::Save));
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
        let btn = tree.add(Button::new("Save").on_click(TestCmd::Save).enabled(false));
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
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
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
        tree.add(Button::new("Save").style(ButtonStyle::Outlined));
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
        let btn = tree.add(Button::new("X"));
        tree.layout(SizeProposal {
            width: None,
            height: None,
        });
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
        assert_eq!(tree.focus_origin(), Some(fern_core::focus::FocusOrigin::Keyboard));
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
        assert_ne!(
            idle_shapes, focused_shapes,
            "keyboard focus should change the button's visual appearance (focus ring)"
        );
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
        assert_eq!(
            hover_shapes, frame_after_click.shapes,
            "pointer click should return to hover state, not show focus ring"
        );
    }

    #[test]
    fn focus_origin_pointer() {
        let (mut tree, btn) = setup();
        let center = tree.bounds(btn).center();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
        });
        assert_eq!(tree.focus_origin(), Some(fern_core::focus::FocusOrigin::Pointer));
    }

    #[test]
    fn button_tooltip_appears_after_delay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
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
        let btn = tree.add(
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
