//! Checkbox — a togglable checkbox with optional label and tristate support.
//!
//! Two modes:
//! - **Two-state** (`Checkbox::new(Signal<bool>)`): toggles between checked/unchecked.
//! - **Tristate** (`Checkbox::tristate(Signal<CheckState>)`): cycles through
//!   Unchecked → Checked → Indeterminate → Unchecked. Useful for tree views
//!   where a parent represents partially-selected children.
//!
//! V2 attached handlers — no event() override.

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::styles::{CheckboxState, CheckboxStyleConfig, CheckboxVariant, SharedCheckboxStyle};
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_data::CheckState;
use fern_tokens::{TextRole, TextStyleRole, VAlignment};

use crate::primitives::{HStack, MinSize, TextWidget, VStack};

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
                // User clicks toggle Checked ↔ Unchecked. The
                // `Indeterminate` state is reserved for external
                // sources (e.g. `TreeCheckedModel` aggregation when
                // descendants are mixed) — the user can't *set* a
                // checkbox to "half"; clicking from Indeterminate
                // checks the whole. This matches the Outlook /
                // Files-app folder-checkbox semantic.
                let current = s.get();
                let next = if matches!(current, CheckState::Checked) {
                    CheckState::Unchecked
                } else {
                    CheckState::Checked
                };
                s.set(next);
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
    /// When true, the checkbox renders only the box (no visual label /
    /// caption next to it) AND its `accessibility(builder)` skips the
    /// missing-label `debug_assert` — the parent composite is responsible
    /// for providing the AT name (typically via its own `set_name(...)`
    /// or an `access_label*` override). Used by `StandardListItem` /
    /// `StandardTreeItem`.
    labels_hidden: bool,
    tooltip_text: Option<String>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn fern_core::widget::Widget>>,
    variant: CheckboxVariant,
    style_override: Option<SharedCheckboxStyle>,
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
            labels_hidden: false,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            variant: CheckboxVariant::default(),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Create a tristate checkbox bound to a `Signal<CheckState>`.
    ///
    /// User clicks toggle Checked ↔ Unchecked (clicking from Indeterminate
    /// checks the whole). The `Indeterminate` state is reserved for external
    /// sources — `TreeCheckedModel` aggregation when descendants are mixed,
    /// "select all" indicators, etc. Matches the Outlook / Files-app
    /// folder-checkbox semantic. Useful for parent checkboxes in tree views.
    pub fn tristate(state: Signal<CheckState>) -> Self {
        Self {
            label: None,
            caption: None,
            kind: CheckKind::TriState(state),
            enabled: true,
            labels_hidden: false,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            variant: CheckboxVariant::default(),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Suppress the visual label/caption AND the debug-time
    /// "missing accessible label" assertion. Use this **only** when
    /// the checkbox is embedded inside a composite that owns the
    /// row's accessible name (e.g. `StandardListItem` /
    /// `StandardTreeItem`, where the row's `accessibility(builder)`
    /// calls `set_name(...)` with the row label).
    ///
    /// **A11y contract:** when `labels_hidden(true)` is set, the
    /// caller MUST guarantee that an addressable AT ancestor
    /// provides the name — either via that ancestor's own
    /// `accessibility()` impl or a builder-level
    /// `.access_label*` override. Without it the AT tree exposes a
    /// `Role::CheckBox` node with no name; screen readers announce
    /// "checkbox, checked" with no context. The Outlook /
    /// Files-app row pattern (where the row label covers the
    /// embedded checkbox) is the supported use case.
    pub fn labels_hidden(mut self, hidden: bool) -> Self {
        self.labels_hidden = hidden;
        self
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

    /// Pick the design-language variant. Default `Square`. The active
    /// `CheckboxStyle` impl decides what the variant means visually
    /// (the IntUI `RecipeCheckboxStyle` honours all three variants
    /// directly via corner-shape changes).
    pub fn variant(mut self, variant: CheckboxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `CheckboxStyle` for just this Checkbox instance — same role as
    /// `Button::style(...)`.
    pub fn style(mut self, style: impl fern_core::styles::CheckboxStyle) -> Self {
        self.style_override = Some(Rc::new(style));
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
// Widget
// ---------------------------------------------------------------------------

/// Internal interaction state — local to this widget's handlers; the
/// active `CheckboxStyle` only sees the four derived boolean signals
/// (is_hovered, is_pressed, is_focused, is_disabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionState {
    Idle,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

impl Widget for Checkbox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let cb_style = ctx.theme().components.checkbox;
        let kind = self.kind.clone();
        let enabled = self.enabled;
        let variant = self.variant;

        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });

        // Bridge the widget-side `CheckState` (fern-data) to the style-
        // protocol-side `CheckboxState` (fern-core). The mapping is 1-to-1;
        // `.map()` registers the upstream root so the body repaints when
        // the check state flips.
        let style_state = kind
            .check_state_signal()
            .map(|cs| match *cs {
                CheckState::Unchecked => CheckboxState::Unchecked,
                CheckState::Checked => CheckboxState::Checked,
                CheckState::Indeterminate => CheckboxState::Indeterminate,
            });

        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_focused = interaction.map(|s| matches!(s, InteractionState::Focused));
        let is_disabled = interaction.map(|s| matches!(s, InteractionState::Disabled));

        let style: SharedCheckboxStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.checkbox.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeCheckboxStyle::default()));
        let cfg = CheckboxStyleConfig {
            state: style_state,
            is_hovered,
            is_pressed,
            is_focused,
            is_disabled,
            variant,
        };
        let body_id = style.make_body(&cfg, ctx);

        let mut row = HStack::new().spacing(cb_style.label_gap).add_child(body_id);
        if !self.labels_hidden && let Some(ref label) = self.label {
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
            self.label.is_some() || self.labels_hidden,
            "Checkbox is missing an accessible label — \
             screen readers will announce \"checkbox\" with no context. \
             Call .label(...) when constructing the widget, or \
             .labels_hidden(true) when embedded in a composite that \
             owns the AT name."
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
    use fern_core::Theme;

    // --- Two-state tests ---

    #[test]
    fn click_toggles_bool_state() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let cb = tree.add(Checkbox::new(checked).label_literal("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let info = tree.accessibility_node(cb);
        assert_eq!(info.role(), fern_core::accesskit::Role::CheckBox);
        assert_eq!(info.name(), Some("Accept"));
        assert!(info.is_toggled());
    }

    // --- Tristate tests ---

    #[test]
    fn tristate_user_click_toggles_two_states() {
        // User clicks only toggle Checked ↔ Unchecked. Indeterminate is
        // reserved for external sources (TreeCheckedModel aggregation, etc.)
        // — clicking from Indeterminate checks the whole. Outlook / Files-app
        // folder-checkbox semantic.
        let state = Signal::new(CheckState::Unchecked);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let cb = tree.add(Checkbox::tristate(state.clone()).label_literal("Select All"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert_eq!(state.get(), CheckState::Unchecked);
        tree.click(cb);
        assert_eq!(state.get(), CheckState::Checked);
        tree.click(cb);
        assert_eq!(state.get(), CheckState::Unchecked);

        // Clicking from Indeterminate checks the whole, NOT cycles.
        state.set(CheckState::Indeterminate);
        tree.click(cb);
        assert_eq!(state.get(), CheckState::Checked);
    }

    #[test]
    fn tristate_space_toggles_two_states() {
        let state = Signal::new(CheckState::Unchecked);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let cb = tree.add(Checkbox::tristate(state.clone()).label_literal("Select All"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(cb);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(state.get(), CheckState::Checked);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(state.get(), CheckState::Unchecked);
    }

    #[test]
    fn tristate_indeterminate_shows_filled_background() {
        // Indeterminate is_filled() == true, so it should have a primary background
        let state = Signal::new(CheckState::Indeterminate);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(Checkbox::tristate(state).label_literal("Partial"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let primary = fern_core::presets::intui::light().colors.accent.to_array();
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
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        tree.add(
            Checkbox::new(checked)
                .label_literal("Disabled")
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let disabled_fill = fern_core::presets::intui::light().colors.accent_disabled.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == disabled_fill),
            "disabled checkbox should render with disabled_fill color"
        );
    }

    #[test]
    fn accessibility_has_actions() {
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
        let cb = tree.add(Checkbox::new(checked).label_literal("Accept"));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(cb);
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }
}
