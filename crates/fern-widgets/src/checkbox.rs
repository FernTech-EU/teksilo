//! Checkbox — a togglable checkbox with optional label and tristate support.
//!
//! Two modes:
//! - **Two-state** (`Checkbox::new(Signal<bool>)`): toggles between checked/unchecked.
//! - **Tristate** (`Checkbox::tristate(Signal<CheckState>)`): cycles through
//!   Unchecked → Checked → Indeterminate → Unchecked. Useful for tree views
//!   where a parent represents partially-selected children.
//!
//! Follows the Button pattern: Widget with TapRecognizer,
//! InteractionState-driven reactive colors, and accessibility.

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer,
};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius};

use crate::button::InteractionState;
use crate::primitives::{FixedSize, HStack, IconWidget, MinSize, RectWidget, TextWidget, ZStack};

// ---------------------------------------------------------------------------
// CheckState
// ---------------------------------------------------------------------------

/// The three possible states of a tristate checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckState {
    Unchecked,
    Checked,
    Indeterminate,
}

impl CheckState {
    /// Whether the box shows a filled background (checked or indeterminate).
    pub fn is_filled(self) -> bool {
        self != CheckState::Unchecked
    }

    /// Cycle to the next state: Unchecked → Checked → Indeterminate → Unchecked.
    pub fn next_tristate(self) -> Self {
        match self {
            CheckState::Unchecked => CheckState::Checked,
            CheckState::Checked => CheckState::Indeterminate,
            CheckState::Indeterminate => CheckState::Unchecked,
        }
    }
}

impl From<bool> for CheckState {
    fn from(checked: bool) -> Self {
        if checked {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    }
}

// ---------------------------------------------------------------------------
// Internal state wrapper
// ---------------------------------------------------------------------------

/// Wraps either a bool state (two-state) or a CheckState state (tristate).
#[derive(Clone)]
enum CheckKind {
    TwoState(Signal<bool>),
    TriState(Signal<CheckState>),
}

impl CheckKind {
    fn check_state(&self) -> CheckState {
        match self {
            CheckKind::TwoState(s) => CheckState::from(s.get()),
            CheckKind::TriState(s) => s.get(),
        }
    }

    fn toggle(&self) {
        match self {
            CheckKind::TwoState(s) => {
                let current = s.get();
                s.set(!current);
            }
            CheckKind::TriState(s) => {
                let current = s.get();
                s.set(current.next_tristate());
            }
        }
    }
}

impl std::fmt::Debug for CheckKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckKind::TwoState(_) => write!(f, "TwoState"),
            CheckKind::TriState(_) => write!(f, "TriState"),
        }
    }
}

// ---------------------------------------------------------------------------
// Checkbox
// ---------------------------------------------------------------------------

/// A checkbox that toggles a `Signal<bool>` or cycles a `Signal<CheckState>`.
pub struct Checkbox {
    label: Option<String>,
    kind: CheckKind,
    enabled: bool,
    tooltip_text: Option<String>,
    interaction: Option<Signal<InteractionState>>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
    root_child_id: Option<WidgetId>,
}

impl Checkbox {
    /// Create a two-state checkbox bound to a `Signal<bool>`.
    pub fn new(checked: Signal<bool>) -> Self {
        Self {
            label: None,
            kind: CheckKind::TwoState(checked),
            enabled: true,
            tooltip_text: None,
            interaction: None,
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            root_child_id: None,
        }
    }

    /// Create a tristate checkbox bound to a `Signal<CheckState>`.
    ///
    /// Clicking cycles: Unchecked → Checked → Indeterminate → Unchecked.
    /// Useful for parent checkboxes in tree views.
    pub fn tristate(state: Signal<CheckState>) -> Self {
        Self {
            label: None,
            kind: CheckKind::TriState(state),
            enabled: true,
            tooltip_text: None,
            interaction: None,
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            root_child_id: None,
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

    fn toggle(&self) {
        self.kind.toggle();
    }

    fn check_state(&self) -> CheckState {
        self.kind.check_state()
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

impl std::fmt::Debug for Checkbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkbox")
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Color resolution
// ---------------------------------------------------------------------------

fn resolve_box_bg(
    state: InteractionState,
    check: CheckState,
    colors: &fern_tokens::ColorTokens,
) -> Color {
    match state {
        InteractionState::Disabled => colors.disabled_fill,
        _ if check.is_filled() => match state {
            InteractionState::Hovered => colors.primary_hover,
            InteractionState::Pressed => colors.primary_pressed,
            _ => colors.primary,
        },
        _ => Color::TRANSPARENT,
    }
}

fn resolve_box_border(
    state: InteractionState,
    check: CheckState,
    colors: &fern_tokens::ColorTokens,
) -> Color {
    match state {
        InteractionState::Disabled => colors.disabled_fill,
        _ if check.is_filled() => Color::TRANSPARENT,
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

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// A horizontal dash icon for the indeterminate state.
fn indeterminate_icon(size: f32) -> IconWidget {
    let mut path = Path::new();
    let s = size;
    path.move_to(Point::new(s * 0.2, s * 0.5));
    path.line_to(Point::new(s * 0.8, s * 0.5));
    IconWidget::from_path(path, size)
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

impl Widget for Checkbox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let kind = self.kind.clone();

        let interaction = ctx.signal(if self.enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = Some(interaction.clone());

        // Derive box colors from interaction state AND check state
        let bg_color = {
            let colors = theme.colors.clone();
            let kind = kind.clone();
            interaction.map(move |s| resolve_box_bg(*s, kind.check_state(), &colors))
        };
        let border_color = {
            let colors = theme.colors.clone();
            let kind = kind.clone();
            interaction.map(move |s| resolve_box_border(*s, kind.check_state(), &colors))
        };

        // Checkbox box (18x18)
        let box_rect = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .border_width(theme.shape.border_width)
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm));
        let box_id = ctx.add(box_rect);
        let box_sized = ctx.add(
            FixedSize::new()
                .bind_width(18.0_f32)
                .bind_height(18.0_f32)
                .set_child(box_id),
        );

        // Icon color (on-primary or disabled)
        let icon_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| {
                if *s == InteractionState::Disabled {
                    colors.disabled_text
                } else {
                    colors.on_primary
                }
            })
        };

        // Checkmark icon — visible when Checked
        let checkmark = IconWidget::checkmark(14.0).bind_color(icon_color.clone());
        let checkmark_id = ctx.add(checkmark);

        // Indeterminate dash icon — visible when Indeterminate
        let dash = indeterminate_icon(14.0).bind_color(icon_color);
        let dash_id = ctx.add(dash);

        // Control visibility based on check state.
        match &self.kind {
            CheckKind::TwoState(checked) => {
                ctx.visible_when(checkmark_id, checked.clone());
                ctx.visible_when(dash_id, false);
            }
            CheckKind::TriState(state) => {
                ctx.visible_when(checkmark_id, state.map(|v| *v == CheckState::Checked));
                ctx.visible_when(dash_id, state.map(|v| *v == CheckState::Indeterminate));
            }
        }

        // Outer focus ring — 3px offset outside the 18x18 box (24x24 total)
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
            .corner_radius(CornerRadius::uniform(theme.shape.radius_sm + 3.0));
        let focus_ring_id = ctx.add(focus_ring);
        let focus_ring_sized = ctx.add(
            FixedSize::new()
                .bind_width(24.0_f32)
                .bind_height(24.0_f32)
                .set_child(focus_ring_id),
        );

        let check_box = ctx.add(
            ZStack::new()
                .add_child(focus_ring_sized)
                .add_child(box_sized)
                .add_child(checkmark_id)
                .add_child(dash_id),
        );

        // Compose with optional label
        let mut row = HStack::new().spacing(8.0).add_child(check_box);
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
            let tooltip_widget = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            ctx.attach_tooltip(root_id, tooltip_id, std::time::Duration::from_millis(500));
        }

        self.root_child_id = Some(root_id);
        vec![root_id]
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
                    self.toggle();
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
                self.toggle();
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
                    self.toggle();
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
        builder.set_role(fern_core::accesskit::Role::CheckBox);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        match self.check_state() {
            CheckState::Checked => builder.set_toggled(true),
            CheckState::Unchecked => builder.set_toggled(false),
            CheckState::Indeterminate => {
                // AccessKit's Toggled::Mixed maps to ARIA "mixed"
                builder.inner_mut().set_toggled(fern_core::accesskit::Toggled::Mixed);
            }
        }
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    // --- Two-state tests ---

    #[test]
    fn click_toggles_bool_state() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::new(checked.clone()).label("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert!(!checked.get());
        tree.click(cb);
        assert!(checked.get());
        tree.click(cb);
        assert!(!checked.get());
    }

    #[test]
    fn space_toggles_bool_state() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::new(checked.clone()).label("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(cb);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(checked.get());
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(!checked.get());
    }

    #[test]
    fn disabled_ignores_click() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(
            Checkbox::new(checked.clone()).label("Accept").enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.click(cb);
        assert!(!checked.get());
    }

    #[test]
    fn two_state_accessibility() {
        let checked = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::new(checked).label("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let info = tree.accessibility_node(cb);
        assert_eq!(info.role(), fern_core::accesskit::Role::CheckBox);
        assert_eq!(info.name(), Some("Accept"));
        assert!(info.is_toggled());
    }

    // --- Tristate tests ---

    #[test]
    fn tristate_cycles_through_all_states() {
        let state = Signal::new(CheckState::Unchecked);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::tristate(state.clone()).label("Select All"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert_eq!(state.get(), CheckState::Unchecked);
        tree.click(cb);
        assert_eq!(state.get(), CheckState::Checked);
        tree.click(cb);
        assert_eq!(state.get(), CheckState::Indeterminate);
        tree.click(cb);
        assert_eq!(state.get(), CheckState::Unchecked);
    }

    #[test]
    fn tristate_space_cycles() {
        let state = Signal::new(CheckState::Unchecked);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::tristate(state.clone()).label("Select All"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(cb);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(state.get(), CheckState::Checked);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(state.get(), CheckState::Indeterminate);
    }

    #[test]
    fn tristate_indeterminate_shows_filled_background() {
        // Indeterminate is_filled() == true, so it should have a primary background
        let state = Signal::new(CheckState::Indeterminate);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Checkbox::tristate(state).label("Partial"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let primary = Theme::light_default().colors.primary.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == primary),
            "indeterminate checkbox should have primary-colored background"
        );
    }

    #[test]
    fn check_state_conversions() {
        assert_eq!(CheckState::from(true), CheckState::Checked);
        assert_eq!(CheckState::from(false), CheckState::Unchecked);
        assert!(CheckState::Checked.is_filled());
        assert!(CheckState::Indeterminate.is_filled());
        assert!(!CheckState::Unchecked.is_filled());
    }

    #[test]
    fn disabled_has_disabled_colors() {
        let checked = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Checkbox::new(checked).label("Disabled").enabled(false));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let disabled_fill = Theme::light_default().colors.disabled_fill.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == disabled_fill),
            "disabled checkbox should render with disabled_fill color"
        );
    }

    #[test]
    fn accessibility_has_actions() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::new(checked).label("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(cb);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }
}
