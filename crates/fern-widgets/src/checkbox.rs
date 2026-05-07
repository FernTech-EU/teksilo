//! Checkbox — a togglable checkbox with optional label and tristate support.
//!
//! Two modes:
//! - **Two-state** (`Checkbox::new(Signal<bool>)`): toggles between checked/unchecked.
//! - **Tristate** (`Checkbox::tristate(Signal<CheckState>)`): cycles through
//!   Unchecked → Checked → Indeterminate → Unchecked. Useful for tree views
//!   where a parent represents partially-selected children.
//!
//! V2 attached handlers — no event() override.

use fern_canvas::{Path, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole, TextRole, TextStyleRole, VAlignment};

use crate::button::InteractionState;
use crate::primitives::{
    FixedSize, HStack, IconWidget, MinSize, RectWidget, TextWidget, VStack, ZStack,
};

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

    /// A reactive `Signal<CheckState>` that tracks the underlying
    /// mutable root of either variant. Used to compose multi-source
    /// derived visuals (e.g. box colors that depend on both interaction
    /// state and check state) so they dirty-track the check-state
    /// source in addition to the interaction source.
    fn check_state_signal(&self) -> Signal<CheckState> {
        match self {
            CheckKind::TwoState(s) => s.map(|b| CheckState::from(*b)),
            CheckKind::TriState(s) => s.clone(),
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
    caption: Option<String>,
    kind: CheckKind,
    enabled: bool,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn fern_core::widget::Widget>>,
    interaction: Option<Signal<InteractionState>>,
    root_child_id: Option<WidgetId>,
}

impl Checkbox {
    /// Create a two-state checkbox bound to a `Signal<bool>`.
    pub fn new(checked: Signal<bool>) -> Self {
        Self {
            label: None,
            caption: None,
            kind: CheckKind::TwoState(checked),
            enabled: true,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            interaction: None,
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
            caption: None,
            kind: CheckKind::TriState(state),
            enabled: true,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            interaction: None,
            root_child_id: None,
        }
    }

    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Secondary explanatory text rendered below the label, left-aligned
    /// with the label (not the box). Uses the `small` / `text_secondary`
    /// style. Has no effect unless `label(...)` is also set.
    pub fn caption(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.caption = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `caption(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn caption_literal(mut self, text: impl Into<String>) -> Self {
        self.caption = Some(text.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. See [`Button::rich_tooltip`](crate::button::Button::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree. See [`Button::composite_tooltip`](crate::button::Button::composite_tooltip).
    pub fn composite_tooltip(mut self, content: impl fern_core::widget::Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    fn check_state(&self) -> CheckState {
        self.kind.check_state()
    }
}

impl std::fmt::Debug for Checkbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkbox")
            .field("label", &self.label)
            .field("caption", &self.caption)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Color resolution
// ---------------------------------------------------------------------------

fn resolve_box_bg_role(state: InteractionState, check: CheckState) -> SurfaceRole {
    match state {
        InteractionState::Disabled => SurfaceRole::AccentDisabled,
        _ if check.is_filled() => match state {
            InteractionState::Hovered => SurfaceRole::AccentHover,
            InteractionState::Pressed => SurfaceRole::AccentPressed,
            _ => SurfaceRole::Accent,
        },
        _ => SurfaceRole::Transparent,
    }
}

fn resolve_box_border_role(state: InteractionState, check: CheckState) -> BorderRole {
    // Focus wins over every other state: even a filled
    // (Checked / Indeterminate) checkbox still shows the accent
    // border when keyboard-focused, because that's the only
    // focus indicator — there is no external ring.
    match state {
        InteractionState::Focused => BorderRole::Focused,
        InteractionState::Disabled => BorderRole::AccentDisabled,
        _ if check.is_filled() => BorderRole::Transparent,
        InteractionState::Hovered => BorderRole::Strong,
        _ => BorderRole::Default,
    }
}

fn resolve_icon_role(state: InteractionState) -> TextRole {
    if state == InteractionState::Disabled {
        TextRole::Disabled
    } else {
        TextRole::OnAccent
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
        let theme = ctx.theme();
        let cb_style = theme.components.checkbox;
        let focus_ring_width = theme.shape.focus_ring_width;
        let border_width = theme.shape.border_width;
        let kind = self.kind.clone();
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = Some(interaction.clone());

        // Derive box roles from interaction state AND check state. `zip` on
        // the two registers both upstream roots so the visuals refresh on
        // click or external check-state flips; theme switches repaint via
        // `mark_all_dirty` without needing a separate zip here.
        let check_state = kind.check_state_signal();
        let bg_role = interaction
            .zip(&check_state)
            .map(|(s, cs)| resolve_box_bg_role(*s, *cs));
        let border_role = interaction
            .zip(&check_state)
            .map(|(s, cs)| resolve_box_border_role(*s, *cs));

        let icon_size = cb_style.box_visual_size * 0.75;

        // Border width — Int UI focus convention: thicken the
        // existing border to `focus_ring_width` on focus, instead of
        // wrapping the box in a separate ring.
        let border_width_signal = interaction.map(move |s| match *s {
            InteractionState::Focused => focus_ring_width,
            _ => border_width,
        });

        let box_rect = RectWidget::new()
            .bind_background(bg_role)
            .bind_border_color(border_role)
            .bind_border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(cb_style.corner_radius));
        let box_id = ctx.add(box_rect);
        let box_sized = ctx.add(
            FixedSize::new()
                .bind_width(cb_style.box_visual_size)
                .bind_height(cb_style.box_visual_size)
                .child_id(box_id),
        );

        let icon_role = interaction.map(|s| resolve_icon_role(*s));

        let checkmark = IconWidget::checkmark(icon_size).bind_color(icon_role.clone());
        let checkmark_id = ctx.add(checkmark);

        let dash = indeterminate_icon(icon_size).bind_color(icon_role);
        let dash_id = ctx.add(dash);

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

        // Compose the visual box with checkmark/dash icons on top.
        // No external focus ring — the box's own border is the
        // focus indicator (thickened + accent-colored) per the Int
        // UI text-field convention, applied uniformly to every
        // input widget with a resting border.
        let check_box = ctx.add(
            ZStack::new()
                .add_child(box_sized)
                .add_child(checkmark_id)
                .add_child(dash_id),
        );

        let mut row = HStack::new()
            .spacing(cb_style.label_gap)
            .add_child(check_box);
        if let Some(ref label) = self.label {
            let label_widget = TextWidget::new_literal(label)
                .style(TextStyleRole::Body)
                .color(TextRole::Primary)
                .single_line()
                .a11y_hidden();
            let label_id = ctx.add(label_widget);

            let label_column_id = if let Some(ref caption) = self.caption {
                let caption_widget = TextWidget::new_literal(caption)
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary)
                    .a11y_hidden();
                let caption_id = ctx.add(caption_widget);
                ctx.add(
                    VStack::new()
                        .spacing(2.0)
                        .add_child(label_id)
                        .add_child(caption_id),
                )
            } else {
                label_id
            };
            row = row.add_child(label_column_id);
        }
        // When a caption is present, top-align the row so the box sits next
        // to the label's first line rather than the center of both lines.
        if self.caption.is_some() && self.label.is_some() {
            row = row.alignment(VAlignment::Top);
        }

        let row_id = ctx.add(row);
        let root_id =
            ctx.add(MinSize::new(cb_style.box_hit_area, cb_style.box_hit_area).child_id(row_id));

        if let Some(content) = self.composite_tooltip_content.take() {
            crate::tooltip::attach_composite_tooltip_boxed(
                ctx,
                root_id,
                content,
                crate::tooltip::DEFAULT_COMPOSITE_TOOLTIP_DELAY,
            );
        } else if let Some(source) = self.rich_tooltip_source.take() {
            crate::tooltip::attach_rich_tooltip_source(
                ctx,
                root_id,
                source,
                crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
            );
        } else if let Some(ref tooltip_text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            ctx.attach_tooltip(root_id, tooltip_id, std::time::Duration::from_millis(500));
        }

        self.root_child_id = Some(root_id);

        // --- V2 attached handlers ---
        let kind_tap = self.kind.clone();
        let kind_key = self.kind.clone();
        let kind_access = self.kind.clone();
        let int_tap = interaction.clone();
        let int_hover = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                move |_pos, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    kind_tap.toggle();
                    int_tap.set(InteractionState::Hovered);
                }
            })
            .on_hover({
                move |entered: bool, _ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    if entered {
                        int_hover.set(InteractionState::Hovered);
                    } else {
                        int_hover.set(InteractionState::Idle);
                    }
                }
            })
            .on_key({
                move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space, ..
                        } => {
                            int_key.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space, ..
                        } => {
                            kind_key.toggle();
                            int_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus({
                move |gained: bool, _ctx: &mut EventContext| {
                    if gained {
                        if int_focus.get() == InteractionState::Idle {
                            int_focus.set(InteractionState::Focused);
                        }
                    } else {
                        int_focus.set(InteractionState::Idle);
                    }
                }
            })
            .on_access_action({
                move |action: fern_core::accesskit::Action,
                      _ctx: &mut EventContext|
                      -> EventResponse {
                    if action == fern_core::accesskit::Action::Click && enabled {
                        kind_access.toggle();
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

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
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
        debug_assert!(
            self.label.is_some(),
            "Checkbox is missing an accessible label — \
             screen readers will announce \"checkbox\" with no context. \
             Call .label(...) when constructing the widget."
        );
        builder.set_role(fern_core::accesskit::Role::CheckBox);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        if let Some(ref caption) = self.caption {
            builder.set_description(caption);
        }
        match self.check_state() {
            CheckState::Checked => builder.set_toggled(true),
            CheckState::Unchecked => builder.set_toggled(false),
            CheckState::Indeterminate => {
                // AccessKit's Toggled::Mixed maps to ARIA "mixed"
                builder
                    .inner_mut()
                    .set_toggled(fern_core::accesskit::Toggled::Mixed);
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
        let cb = tree.add(Checkbox::new(checked.clone()).label_literal("Accept"));
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
        let cb = tree.add(Checkbox::new(checked.clone()).label_literal("Accept"));
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
            Checkbox::new(checked.clone())
                .label_literal("Accept")
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.click(cb);
        assert!(!checked.get());
    }

    #[test]
    fn two_state_accessibility() {
        let checked = Signal::new(true);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::new(checked).label_literal("Accept"));
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
        let cb = tree.add(Checkbox::tristate(state.clone()).label_literal("Select All"));
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
        let cb = tree.add(Checkbox::tristate(state.clone()).label_literal("Select All"));
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
        tree.add(Checkbox::tristate(state).label_literal("Partial"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let primary = Theme::light_default().colors.accent.to_array();
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
        tree.add(
            Checkbox::new(checked)
                .label_literal("Disabled")
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let disabled_fill = Theme::light_default().colors.accent_disabled.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == disabled_fill),
            "disabled checkbox should render with disabled_fill color"
        );
    }

    #[test]
    fn accessibility_has_actions() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let cb = tree.add(Checkbox::new(checked).label_literal("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(cb);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }
}
