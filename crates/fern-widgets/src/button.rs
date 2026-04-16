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

use crate::primitives::{HStack, Padding, RectWidget, TextWidget, VStack, ZStack};
use crate::primitives::icon_widget::IconWidget;

/// Visual role of the button.
///
/// - [`ButtonVariant::Default`] — the primary action in a dialog or form.
///   Filled with `accent`, white label, no border. There should be at most
///   one Default button per dialog (the one that Enter activates).
/// - [`ButtonVariant::Regular`] — any non-primary button. A visible surface
///   fill with a 1 dp border and a `text_primary` label. This is the default
///   because most buttons are not the primary action.
/// - [`ButtonVariant::Flat`] — a borderless button used in toolbars, action
///   rows, and inline contexts. Transparent at idle, `surface_hover` on
///   hover, `text_primary` label.
///
/// Int UI does **not** use filled red "destructive" buttons. Destructive
/// actions in IntelliJ are plain `Regular` buttons ("Delete", "Revert", …)
/// in confirmation dialogs where the dialog title, icon, and body text
/// carry the warning — the button itself is not colored. For inline row
/// actions ("Remove this plugin"), use a `Flat` button or a `Link` widget
/// with an error-colored label. Do not reintroduce a filled red variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Primary action — accent-filled, one per dialog.
    Default,
    /// Non-primary action — surface fill with a 1 dp border.
    #[default]
    Regular,
    /// Borderless — toolbar / inline actions.
    Flat,
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

/// Internal interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconLocation {
    /// No icon (default).
    #[default]
    None,
    /// Icon only, no label.
    IconOnly,
    /// Icon to the left of the label (default).
    Leading,
    /// Icon to the right of the label.
    Trailing,
    /// Icon above the label.
    Top,
     /// Icon below the label.
    Bottom,

}

/// A production-quality button widget — non-generic, composition-based.
///
/// ```ignore
/// Button::new_literal("Save")
///     .style(ButtonVariant::Default)
///     .on_activate(AppCmd::Save)
/// ```
/// Type-erased command factory — captures the concrete command type
/// and produces a fresh ErasedCommand each time (since ErasedCommand isn't Clone).
type CommandFactory = Box<dyn Fn(&mut EventContext)>;

pub struct Button {
    label: String,
    style: ButtonVariant,
    action: Option<CommandFactory>,
    enabled: bool,
    icon: Option<IconWidget>,
    icon_location: IconLocation,
    tooltip_text: Option<String>,
    /// Optional rich tooltip source (registry key or inline content).
    /// Takes precedence over `tooltip_text` when both are set — last
    /// call wins because the setters clear the other field.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional `has_popup` hint used when this button acts as a
    /// disclosure trigger for a popup (menu, dialog, listbox, etc.).
    /// Surfaced via `set_has_popup` in `accessibility()`.
    has_popup: Option<fern_core::accesskit::HasPopup>,
    /// Optional signal reporting whether the button's popup is
    /// currently visible. Surfaced via `set_expanded` in
    /// `accessibility()`. Used alongside `has_popup` for the
    /// standard ARIA disclosure pattern.
    expanded_signal: Option<Signal<bool>>,
    /// Interaction state signal — set during build().
    interaction: Signal<InteractionState>,
    /// Root child ID — set during build().
    root_child_id: Option<WidgetId>,
}

impl Button {
    /// Construct a button from a `LocalizedString` label. The label may
    /// come from `tr!(...)` (translated) or `LocalizedString::literal(...)`
    /// (explicit non-translated). The text is resolved eagerly at
    /// construction and stored as a plain `String`; locale changes rebuild
    /// the composite parent, which re-creates this `Button` with a fresh
    /// translation.
    pub fn new(label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            // Int UI default is a Regular (non-primary) button; the caller
            // opts into `ButtonVariant::Default` for the one primary action.
            style: ButtonVariant::Regular,
            action: None,
            enabled: true,
            icon: None,
            icon_location: IconLocation::None,
            tooltip_text: None,
            rich_tooltip_source: None,
            has_popup: None,
            expanded_signal: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Shim (permanent, `#[doc(hidden)]`) — wraps a raw label in
    /// `LocalizedString::literal` for tests and scaffolding where
    /// translation is overkill. Production code uses
    /// `new(tr!(...))`; the `*_literal` suffix is the grep marker for
    /// untranslated strings alongside `LocalizedString::literal`.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(label))
    }

    pub fn style(mut self, style: ButtonVariant) -> Self {
        self.style = style;
        self
    }

    /// Set the command to emit on activation. The generic only appears at
    /// this call site — the Button struct itself is non-generic (Approach B).
    pub fn on_activate<C: AppCommand>(mut self, command: C) -> Self {
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }

    /// Escape hatch: arbitrary closure invoked on activation. Use only when
    /// the action cannot be expressed as a typed command — plugin systems,
    /// scripting consoles, or direct model mutation from data-driven widgets.
    /// Loses recordability, command palette integration, and assertion-based
    /// testing. See architecture Section 9.2.6.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }

    /// Attach a tooltip that appears after a hover delay.
    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self.rich_tooltip_source = None;
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip registry.
    /// The `key` is looked up via
    /// [`TooltipRegistry`](crate::tooltip::TooltipRegistry) at build
    /// time; the resolved body text supports inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`) and the entry's
    /// shortcut / long-form "more" fields are rendered automatically.
    ///
    /// Overrides any previously set plain `.tooltip(...)` text.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self
    }

    /// Attach a rich tooltip driven by inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) — for
    /// one-off tooltips that aren't worth registering in the central
    /// catalog. Overrides any previously set plain `.tooltip(...)`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add an icon to the button at the specified location.
    pub fn icon(mut self, icon: IconWidget, location: IconLocation) -> Self {
        self.icon = Some(icon);
        self.icon_location = location;
        self
    }

    /// Declare that this button is a disclosure trigger for a
    /// popup (menu, dialog, listbox, tree, grid). Surfaced via
    /// `set_has_popup` in the a11y node so screen readers announce
    /// it as leading into the named popup kind.
    pub fn has_popup(mut self, kind: fern_core::accesskit::HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }

    /// Bind a signal reporting whether this button's popup is
    /// currently visible. The Popover / Dialog wrapper owns the
    /// signal and flips it on show / dismiss; Button reads it in
    /// `accessibility()` to publish `set_expanded`. Only
    /// meaningful alongside `.has_popup(...)`.
    pub fn expanded_when(mut self, signal: Signal<bool>) -> Self {
        self.expanded_signal = Some(signal);
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

// --- Color resolution: variant × state × theme (resolved at paint time) ---
//
// Per the Int UI reference (v2 §1), emphasis comes from fill color not from
// border thickness or stroke style. Each variant maps to a distinct surface
// role; only Default uses the accent family.

fn resolve_bg(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        // Default: accent-filled primary action.
        (ButtonVariant::Default, InteractionState::Disabled) => colors.accent_disabled,
        (ButtonVariant::Default, InteractionState::Pressed) => colors.accent_pressed,
        (ButtonVariant::Default, InteractionState::Hovered) => colors.accent_hover,
        (ButtonVariant::Default, _) => colors.accent,

        // Regular: visible surface fill. Disabled keeps surface_main so
        // the button doesn't look primary; only the label dims.
        (ButtonVariant::Regular, InteractionState::Pressed) => colors.surface_pressed,
        (ButtonVariant::Regular, InteractionState::Hovered) => colors.surface_hover,
        (ButtonVariant::Regular, _) => colors.surface_main,

        // Flat: transparent at idle, light wash on hover/press.
        (ButtonVariant::Flat, InteractionState::Pressed) => colors.surface_pressed,
        (ButtonVariant::Flat, InteractionState::Hovered) => colors.surface_hover,
        (ButtonVariant::Flat, _) => Color::TRANSPARENT,
    }
}

fn resolve_text(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    match (style, state) {
        // Default: white label on accent fill.
        (ButtonVariant::Default, InteractionState::Disabled) => colors.text_disabled,
        (ButtonVariant::Default, _) => colors.text_on_accent,

        // Regular / Flat: primary text, dim when disabled.
        (ButtonVariant::Regular | ButtonVariant::Flat, InteractionState::Disabled) => {
            colors.text_disabled
        }
        (ButtonVariant::Regular | ButtonVariant::Flat, _) => colors.text_primary,
    }
}

fn resolve_border(style: ButtonVariant, state: InteractionState, colors: &ColorTokens) -> Color {
    // Int UI: borders are always 1 dp; emphasis is color-only. The focus
    // ring is drawn outside the control by the `FocusRing` wrapper, so
    // borders never encode focus state.
    match style {
        // Default / Flat: no border — the fill (or absence of one) carries
        // the affordance.
        ButtonVariant::Default | ButtonVariant::Flat => Color::TRANSPARENT,
        // Regular: always a visible border. Strong variant on hover/press.
        ButtonVariant::Regular => match state {
            InteractionState::Disabled => colors.border,
            InteractionState::Hovered | InteractionState::Pressed => colors.border_strong,
            _ => colors.border,
        },
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

        // If an `expanded_signal` was wired up (disclosure
        // pattern — see `.has_popup()` / `.expanded_when()`),
        // register it with the framework so changes trigger a
        // repaint/a11y refresh on this button. Without the
        // binding registration, the signal updates but the
        // widget's `accessibility()` output won't be re-queried.
        if let Some(ref expanded_signal) = self.expanded_signal {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            expanded_signal.bind_to(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }

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
        let button_style = theme.components.button;

        // Build the content (icon + label) based on icon_location
        let content_id = match self.icon_location {
            IconLocation::None => {
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_color)
                    .single_line()
                    .a11y_hidden();
                ctx.add(text)
            }
            IconLocation::IconOnly => {
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon = icon.icon_size(button_style.icon_size).bind_color(text_color);
                ctx.add(icon)
            }
            IconLocation::Leading => {
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_color.clone()));
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_color)
                    .single_line()
                    .a11y_hidden();
                let text_id = ctx.add(text);
                ctx.add(
                    HStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(icon_id)
                        .add_child(text_id),
                )
            }
            IconLocation::Trailing => {
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_color.clone())
                    .single_line()
                    .a11y_hidden();
                let text_id = ctx.add(text);
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_color));
                ctx.add(
                    HStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(text_id)
                        .add_child(icon_id),
                )
            }
            IconLocation::Top => {
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_color.clone()));
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_color)
                    .single_line()
                    .a11y_hidden();
                let text_id = ctx.add(text);
                ctx.add(
                    VStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(icon_id)
                        .add_child(text_id),
                )
            }
            IconLocation::Bottom => {
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_color.clone())
                    .single_line()
                    .a11y_hidden();
                let text_id = ctx.add(text);
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_color));
                ctx.add(
                    VStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(text_id)
                        .add_child(icon_id),
                )
            }
        };

        let padding = Padding::symmetric(
            button_style.padding_vertical,
            button_style.padding_horizontal,
        )
        .set_child(content_id);
        let padding_id = ctx.add(padding);

        // Int UI: border is fixed at 1 dp. Focus is shown via the FocusRing
        // wrapper, not by thickening the border.
        let rect = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .border_width(button_style.border_width)
            .corner_radius(CornerRadius::uniform(button_style.corner_radius));
        let rect_id = ctx.add(rect);

        let zstack = ZStack::new().add_child(rect_id).add_child(padding_id);
        let zstack_id = ctx.add(zstack);

        // Int UI buttons are 24 dp tall with a 72 dp minimum width.
        let sized_id = ctx.add(
            crate::primitives::MinSize::new(button_style.min_width, button_style.height)
                .set_child(zstack_id),
        );

        // Wrap in a FocusRing so the ring is drawn outside the control.
        // Only keyboard focus shows the ring — `focused` is derived from the
        // interaction state.
        let focused = interaction.map(|s| *s == InteractionState::Focused);
        let root_id = ctx.add(
            crate::primitives::FocusRing::new(focused)
                .corner_radius(button_style.corner_radius)
                .set_child(sized_id),
        );

        // Attach tooltip if configured. Rich-tooltip source takes
        // precedence — both setters clear the other, so at most one
        // branch runs.
        if let Some(source) = self.rich_tooltip_source.take() {
            crate::tooltip::attach_rich_tooltip_source(
                ctx,
                root_id,
                source,
                crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
            );
        } else if let Some(ref tooltip_text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
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
        // Re-wrap action into Rc so it can be shared between tap, key, and access handlers
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = int_tap;
                move |_pos, ctx: &mut EventContext| {
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
                move |action: fern_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
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
        // ARIA disclosure pattern: a button that opens a popup
        // should declare `has_popup` and, if the wrapper tracks
        // it, `expanded`. Both are opt-in — regular buttons with
        // no popup stay silent on these properties.
        if let Some(kind) = self.has_popup {
            builder.set_has_popup(kind);
        }
        if let Some(ref signal) = self.expanded_signal {
            builder.set_expanded(signal.get());
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
        let btn = tree.add(Button::new_literal("Save").on_activate(TestCmd::Save));
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
        let btn = tree.add(
            Button::new_literal("Save")
                .on_activate(TestCmd::Save)
                .enabled(false),
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
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Click)
        );
    }

    #[test]
    fn label_text_widget_is_hidden_from_a11y_tree() {
        // Regression guard: the TextWidget child that paints the button
        // label used to emit its own `Role::Label` node with the same
        // string, producing a duplicate in the a11y tree. `.a11y_hidden()`
        // on the label child suppresses that.
        let (tree, _btn) = setup();
        assert!(
            tree.find_by_role(fern_core::accesskit::Role::Label).is_none(),
            "button label TextWidget must not emit a Role::Label node"
        );
    }

    #[test]
    fn default_variant_renders_accent_background() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Button::new_literal("Save").style(ButtonVariant::Default));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();
        let accent = Theme::light_default().colors.accent.to_array();
        assert!(frame.shapes.iter().any(|s| s.color == accent));
    }

    #[test]
    fn regular_variant_has_border() {
        // Regular is the default constructed variant.
        let (mut tree, _btn) = setup();
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
        let btn = tree.add(Button::new_literal("X"));
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
        assert_eq!(
            tree.focus_origin(),
            Some(fern_core::focus::FocusOrigin::Keyboard)
        );
    }

    #[test]
    fn keyboard_focus_shows_focus_ring() {
        let (mut tree, _btn) = setup();

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
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
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
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            tree.focus_origin(),
            Some(fern_core::focus::FocusOrigin::Pointer)
        );
    }

    #[test]
    fn button_tooltip_appears_after_delay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            Button::new_literal("Save")
                .on_activate(TestCmd::Save)
                .tooltip_literal("Save the document"),
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
            Button::new_literal("Save")
                .on_activate(TestCmd::Save)
                .tooltip_literal("Save the document"),
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

    #[test]
    fn on_activate_fn_click() {
        let counter = Rc::new(Cell::new(0_u32));
        let c = counter.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(Button::new_literal("Inc").on_activate_fn(move |_ctx| {
            c.set(c.get() + 1);
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.click(btn);
        assert_eq!(counter.get(), 1);
        tree.click(btn);
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn on_activate_fn_keyboard() {
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(Button::new_literal("Do").on_activate_fn(move |_ctx| {
            c.set(true);
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(btn);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(called.get());
    }

    #[test]
    fn on_activate_fn_disabled_ignores() {
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            Button::new_literal("Nope")
                .on_activate_fn(move |_ctx| c.set(true))
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.click(btn);
        assert!(!called.get());
    }

    #[test]
    fn on_activate_fn_overwrites_on_activate() {
        let cmd_called = Rc::new(Cell::new(false));
        let fn_called = Rc::new(Cell::new(false));
        let cc = cmd_called.clone();
        let fc = fn_called.clone();

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.on_command(move |_cmd: &TestCmd| cc.set(true));
        let btn = tree.add(
            Button::new_literal("Test")
                .on_activate(TestCmd::Save)
                .on_activate_fn(move |_ctx| fc.set(true)),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.click(btn);
        assert!(fn_called.get(), "on_activate_fn should fire (last wins)");
        assert!(!cmd_called.get(), "on_activate should be overwritten");
    }
}
