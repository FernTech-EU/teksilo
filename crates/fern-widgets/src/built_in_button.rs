//! BuiltInButton — a small icon-only button designed to live inside
//! another widget's trailing edge (text field, combo box, search field).
//!
//! Follows the JetBrains Int UI "built-in button" pattern: flat/transparent
//! at idle, subtle hover/press feedback, sized to [`IconButtonStyle`] tokens.
//!
//! ```ignore
//! BuiltInButton::new(IconWidget::from_svg(MY_SVG))
//!     .tooltip(tr!("Browse..."))
//!     .on_activate(Cmd::Browse)
//! ```
//!
//! ## Predefined constructors
//!
//! Common built-in button types ship with appropriate icons and i18n tooltips:
//!
//! ```ignore
//! BuiltInButton::browse().on_activate(Cmd::Browse)
//! BuiltInButton::clear().on_activate(Cmd::Clear)
//! BuiltInButton::search().on_activate(Cmd::Search)
//! BuiltInButton::visibility_toggle(visible_signal)
//! ```
//!
//! ## Slot convention
//!
//! Host widgets that accept built-in buttons follow the `trailing_slot`
//! convention established by [`TabWidget`](crate::tab_widget::TabWidget):
//!
//! ```ignore
//! TextInput::new(value)
//!     .trailing_slot(HStack::new().spacing(0.0)
//!         .child(BuiltInButton::clear().on_activate(Cmd::Clear))
//!         .child(BuiltInButton::browse().on_activate(Cmd::Browse))
//!     )
//! ```

use std::sync::OnceLock;

use fern_canvas::{Path, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::app_command::AppCommand;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, ColorTokens, CornerRadius};

use crate::button::InteractionState;
use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{Center, FixedSize, RectWidget, Switcher, ZStack};

/// Size variant for built-in buttons, mapping to [`IconButtonStyle`] token
/// dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuiltInButtonSize {
    /// 22 dp — `icon_button.size_compact`.
    Compact,
    /// 24 dp — `icon_button.size_default`.
    #[default]
    Default,
    /// 30 dp — `icon_button.size_large`.
    Large,
}

/// Type-erased action factory — captures the concrete command type.
type ActionFactory = Box<dyn Fn(&mut EventContext)>;

/// A small icon-only button designed to be embedded inside another widget's
/// trailing edge via the `trailing_slot` convention.
pub struct BuiltInButton {
    // Configuration (set via builder)
    icon: IconWidget,
    tooltip_text: Option<String>,
    enabled: bool,
    size: BuiltInButtonSize,
    action: Option<ActionFactory>,

    // Toggle support (visibility_toggle use case)
    toggled: Option<Signal<bool>>,
    toggled_icon: Option<IconWidget>,

    // Build state (set in build())
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl BuiltInButton {
    /// Create a built-in button from a custom icon.
    pub fn new(icon: IconWidget) -> Self {
        Self {
            icon,
            tooltip_text: None,
            enabled: true,
            size: BuiltInButtonSize::Default,
            action: None,
            toggled: None,
            toggled_icon: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Attach a tooltip that appears after a hover delay.
    pub fn tooltip(mut self, text: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        self.tooltip_text = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `tooltip(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn tooltip_literal(mut self, text: impl Into<String>) -> Self {
        self.tooltip_text = Some(text.into());
        self
    }

    /// Set the enabled state. Disabled buttons ignore input and show dimmed icons.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the size variant (compact/default/large).
    pub fn size(mut self, size: BuiltInButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Set the command to emit on activation. The generic only appears at
    /// this call site — the struct itself is non-generic (Approach B).
    pub fn on_activate<C: AppCommand>(mut self, command: C) -> Self {
        self.action = Some(Box::new(move |ctx: &mut EventContext| {
            ctx.emit(command.clone());
        }));
        self
    }

    /// Escape hatch: arbitrary closure invoked on activation.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }

    /// Enable toggle mode: clicking flips `state` instead of firing `action`,
    /// and the icon swaps between the primary icon and `toggled_icon`.
    pub fn toggle(mut self, state: Signal<bool>, toggled_icon: IconWidget) -> Self {
        self.toggled = Some(state);
        self.toggled_icon = Some(toggled_icon);
        self
    }

    // ── Predefined constructors ─────────────────────────────────────────

    /// Browse button (ellipsis icon). Opens a file/directory chooser.
    pub fn browse() -> Self {
        Self::new((BuiltInIcons::global().browse)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_browse()))
    }

    /// Expand button (diagonal resize arrows). Enlarges a constrained field.
    pub fn expand() -> Self {
        Self::new((BuiltInIcons::global().expand)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_expand()))
    }

    /// Search button (magnifier icon). Triggers a search.
    pub fn search() -> Self {
        Self::new((BuiltInIcons::global().search)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_search()))
    }

    /// Copy button (clipboard icon). Copies the field content.
    pub fn copy() -> Self {
        Self::new((BuiltInIcons::global().copy)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_copy()))
    }

    /// Clear button (X icon). Clears the field content.
    pub fn clear() -> Self {
        Self::new((BuiltInIcons::global().clear)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_clear()))
    }

    /// Add button (plus icon). Adds a new entry.
    pub fn add() -> Self {
        Self::new((BuiltInIcons::global().add)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_add()))
    }

    /// Visibility toggle (eye / eye-off). Toggles password visibility.
    ///
    /// The `visible` signal is flipped on each click. The host widget reads
    /// it to decide whether to mask or show the text.
    pub fn visibility_toggle(visible: Signal<bool>) -> Self {
        let icons = BuiltInIcons::global();
        Self::new((icons.eye)())
            .toggle(visible, (icons.eye_off)())
            .tooltip(fern_i18n::tr_widget!(a11y_builtin_visibility()))
    }
}

impl std::fmt::Debug for BuiltInButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltInButton")
            .field("enabled", &self.enabled)
            .field("size", &self.size)
            .finish()
    }
}

// ── Color resolution ────────────────────────────────────────────────────────
//
// Built-in buttons are always flat: transparent idle, subtle hover/press.
// The icon dims to text_secondary at rest, brightens to text_primary on
// hover, and flashes accent on press.

fn resolve_bg(state: InteractionState, colors: &ColorTokens) -> Color {
    match state {
        InteractionState::Idle | InteractionState::Focused => Color::TRANSPARENT,
        InteractionState::Hovered => colors.surface_hover,
        InteractionState::Pressed => colors.surface_pressed,
        InteractionState::Disabled => Color::TRANSPARENT,
    }
}

fn resolve_icon_color(state: InteractionState, colors: &ColorTokens) -> Color {
    match state {
        InteractionState::Idle | InteractionState::Focused => colors.text_secondary,
        InteractionState::Hovered => colors.text_primary,
        InteractionState::Pressed => colors.accent,
        InteractionState::Disabled => colors.text_disabled,
    }
}

fn resolve_size(size: BuiltInButtonSize, style: &fern_tokens::IconButtonStyle) -> f32 {
    match size {
        BuiltInButtonSize::Compact => style.size_compact,
        BuiltInButtonSize::Default => style.size_default,
        BuiltInButtonSize::Large => style.size_large,
    }
}

// ── Widget trait ─────────────────────────────────────────────────────────────

impl fern_core::widget::Widget for BuiltInButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let ib_style = theme.components.icon_button;
        let enabled = self.enabled;

        // Interaction signal
        let interaction = ctx.signal(if enabled {
            InteractionState::Idle
        } else {
            InteractionState::Disabled
        });
        self.interaction = interaction.clone();

        // Propagate enabled state to the arena so the a11y tree and
        // `is_enabled()` queries pick it up.
        if !enabled {
            let self_id = ctx.self_id();
            ctx.enabled_when(self_id, false);
        }

        // Register toggled signal for repaint if present
        if let Some(ref toggled) = self.toggled {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            toggled.bind_to(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // Derived reactive colors
        let bg_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_bg(*s, &colors))
        };
        let icon_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_icon_color(*s, &colors))
        };

        // Build the icon content
        let icon_content_id = if let Some(ref toggled) = self.toggled {
            // Toggle mode: Switcher with two icons driven by the bool signal
            let toggled_index = toggled.map(|v| if *v { 1 } else { 0 });
            let primary_icon = std::mem::replace(
                &mut self.icon,
                IconWidget::from_path(Path::new(), 0.0),
            )
            .icon_size(ib_style.icon_size)
            .bind_color(icon_color.clone());
            let alt_icon = self
                .toggled_icon
                .take()
                .unwrap_or_else(|| IconWidget::from_path(Path::new(), 0.0))
                .icon_size(ib_style.icon_size)
                .bind_color(icon_color);
            ctx.add(
                Switcher::new(toggled_index)
                    .child(primary_icon)
                    .child(alt_icon),
            )
        } else {
            // Normal mode: single icon
            let icon = std::mem::replace(
                &mut self.icon,
                IconWidget::from_path(Path::new(), 0.0),
            )
            .icon_size(ib_style.icon_size)
            .bind_color(icon_color);
            ctx.add(icon)
        };

        let centered_id = ctx.add(Center::new().child_id(icon_content_id));

        // Background rect (no border, rounded corners)
        let bg_id = ctx.add(
            RectWidget::new()
                .bind_background(bg_color)
                .corner_radius(CornerRadius::uniform(ib_style.corner_radius)),
        );

        let zstack_id = ctx.add(ZStack::new().add_child(bg_id).add_child(centered_id));

        // Fixed square size
        let button_dim = resolve_size(self.size, &ib_style);
        let sized_id = ctx.add(
            FixedSize::new()
                .bind_width(button_dim)
                .bind_height(button_dim)
                .child_id(zstack_id),
        );

        // Focus ring
        let focused = interaction.map(|s| *s == InteractionState::Focused);
        let root_id = ctx.add(
            crate::primitives::FocusRing::new(focused)
                .corner_radius(ib_style.corner_radius)
                .child_id(sized_id),
        );

        // Tooltip
        if let Some(ref tooltip_text) = self.tooltip_text {
            let tooltip_widget = crate::tooltip::TooltipWidget::new_literal(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = std::time::Duration::from_millis(500);
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        self.root_child_id = Some(root_id);

        // --- V2 attached handlers ---
        let action = self.action.take();
        let toggled_for_tap = self.toggled.clone();
        let toggled_for_key = self.toggled.clone();
        let toggled_for_access = self.toggled.clone();

        let action_rc: std::rc::Rc<Option<ActionFactory>> = std::rc::Rc::new(action);
        let action_for_tap = action_rc.clone();
        let action_for_key = action_rc.clone();
        let action_for_access = action_rc.clone();

        let int_tap = interaction.clone();
        let int_hover_enter = interaction.clone();
        let int_hover_leave = interaction.clone();
        let int_key = interaction.clone();
        let int_focus = interaction.clone();

        let handler_set = HandlerSet::new()
            .on_tap({
                let interaction = int_tap;
                move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    if let Some(ref toggled) = toggled_for_tap {
                        toggled.set(!toggled.get());
                    } else if let Some(ref action) = *action_for_tap {
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
                            if let Some(ref toggled) = toggled_for_key {
                                toggled.set(!toggled.get());
                            } else if let Some(ref action) = *action_for_key {
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
                        if let Some(ref toggled) = toggled_for_access {
                            toggled.set(!toggled.get());
                        } else if let Some(ref act) = *action_for_access {
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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Button);
        if let Some(ref text) = self.tooltip_text {
            builder.set_name(text.as_str());
        }
        if !self.enabled {
            builder.set_disabled();
        }
        if let Some(ref toggled) = self.toggled {
            builder.set_toggled(toggled.get());
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ── Overridable icon set ────────────────────────────────────────────────────
//
// Default icons are real SVGs embedded via `include_str!` and parsed once
// via `LazyLock`. The `res!` macro cannot be used here because it emits
// `::fern_ui::` paths and fern-widgets sits below fern-ui in the
// dependency graph.
//
// Applications can replace the default icon set globally at startup via
// `BuiltInIcons::set_global(custom_set)`.

/// Icon factory set for predefined built-in buttons.
///
/// Each field is a function pointer that creates an [`IconWidget`].
/// The default implementation uses SVG icons embedded in fern-widgets.
///
/// # Overriding
///
/// Call [`BuiltInIcons::set_global`] at app startup (before creating any
/// built-in buttons) to replace the default icon set:
///
/// ```ignore
/// BuiltInIcons::set_global(BuiltInIcons {
///     browse: || IconWidget::from_svg(MY_BROWSE_SVG),
///     clear: || IconWidget::from_svg(MY_CLEAR_SVG),
///     ..BuiltInIcons::defaults()
/// });
/// ```
pub struct BuiltInIcons {
    pub browse: fn() -> IconWidget,
    pub expand: fn() -> IconWidget,
    pub search: fn() -> IconWidget,
    pub copy: fn() -> IconWidget,
    pub clear: fn() -> IconWidget,
    pub add: fn() -> IconWidget,
    pub eye: fn() -> IconWidget,
    pub eye_off: fn() -> IconWidget,
}

static GLOBAL_ICONS: OnceLock<BuiltInIcons> = OnceLock::new();

impl BuiltInIcons {
    /// Return the default icon set (SVGs embedded in fern-widgets).
    pub fn defaults() -> Self {
        Self {
            browse: default_browse_icon,
            expand: default_expand_icon,
            search: default_search_icon,
            copy: default_copy_icon,
            clear: default_clear_icon,
            add: default_add_icon,
            eye: default_eye_icon,
            eye_off: default_eye_off_icon,
        }
    }

    /// Set the global icon set. Call at app startup before creating any
    /// built-in buttons. Can only be set once; subsequent calls are ignored.
    /// Use [`defaults()`](Self::defaults) with struct update syntax to
    /// override only specific icons.
    pub fn set_global(icons: Self) {
        GLOBAL_ICONS.set(icons).ok();
    }

    /// Access the registered global icon set, falling back to the
    /// compiled-in SVG defaults. Intended for widgets in this crate
    /// that need a themed icon without binding to a specific asset
    /// path — e.g. the clear button inside `TextInput`. Applications
    /// still use `set_global(..)` to override the defaults.
    pub(crate) fn global() -> &'static Self {
        GLOBAL_ICONS.get_or_init(Self::defaults)
    }
}

// ── Default SVG icons ───────────────────────────────────────────────────────

fn default_browse_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-browse.svg"))
}

fn default_expand_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-expand.svg"))
}

fn default_search_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-search.svg"))
}

fn default_copy_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-copy.svg"))
}

fn default_clear_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-clear.svg"))
}

fn default_add_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-add.svg"))
}

fn default_eye_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-eye.svg"))
}

fn default_eye_off_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-eye-off.svg"))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::SizeProposal;
    use fern_core::app_command::AppCommand;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Activate,
    }
    impl AppCommand for TestCmd {}

    fn setup() -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .tooltip_literal("Test")
                .on_activate(TestCmd::Activate),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        (tree, btn)
    }

    // ── Construction & sizing ───────────────────────────────────────────

    #[test]
    fn builds_without_panic() {
        let (tree, btn) = setup();
        let bounds = tree.bounds(btn);
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn sizes_to_default() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .tooltip_literal("Test"),
        );
        tree.layout(SizeProposal { width: None, height: None });
        let bounds = tree.bounds(btn);
        let theme = Theme::light_default();
        let envelope =
            (theme.shape.focus_ring_offset + theme.shape.focus_ring_width) * 2.0;
        let expected = theme.components.icon_button.size_default + envelope;
        assert!(
            (bounds.width - expected).abs() < 0.01,
            "expected width {expected}, got {}",
            bounds.width
        );
        assert!(
            (bounds.height - expected).abs() < 0.01,
            "expected height {expected}, got {}",
            bounds.height
        );
    }

    #[test]
    fn compact_size() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .size(BuiltInButtonSize::Compact),
        );
        tree.layout(SizeProposal { width: None, height: None });
        let bounds = tree.bounds(btn);
        let theme = Theme::light_default();
        let envelope =
            (theme.shape.focus_ring_offset + theme.shape.focus_ring_width) * 2.0;
        let expected = theme.components.icon_button.size_compact + envelope;
        assert!(
            (bounds.width - expected).abs() < 0.01,
            "expected width {expected}, got {}",
            bounds.width
        );
    }

    #[test]
    fn large_size() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .size(BuiltInButtonSize::Large),
        );
        tree.layout(SizeProposal { width: None, height: None });
        let bounds = tree.bounds(btn);
        let theme = Theme::light_default();
        let envelope =
            (theme.shape.focus_ring_offset + theme.shape.focus_ring_width) * 2.0;
        let expected = theme.components.icon_button.size_large + envelope;
        assert!(
            (bounds.width - expected).abs() < 0.01,
            "expected width {expected}, got {}",
            bounds.width
        );
    }

    // ── Interaction ─────────────────────────────────────────────────────

    #[test]
    fn click_fires_command() {
        let (mut tree, btn) = setup();
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Activate {
                c.set(true);
            }
        });
        tree.click(btn);
        assert!(called.get());
    }

    #[test]
    fn click_fires_closure() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let btn = tree.add(
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .on_activate_fn(move |_ctx| c.set(true)),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        tree.click(btn);
        assert!(called.get());
    }

    #[test]
    fn space_activates() {
        let (mut tree, btn) = setup();
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Activate {
                c.set(true);
            }
        });
        tree.focus(btn);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(called.get());
    }

    #[test]
    fn enter_activates() {
        let (mut tree, btn) = setup();
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Activate {
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
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .on_activate(TestCmd::Activate)
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        tree.on_command(move |_cmd: &TestCmd| c.set(true));
        tree.click(btn);
        assert!(!called.get());
    }

    // ── Toggle ──────────────────────────────────────────────────────────

    #[test]
    fn visibility_toggle_flips_signal() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let visible = Signal::new(false);
        let btn = tree.add(BuiltInButton::visibility_toggle(visible.clone()));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        assert!(!visible.get());
        tree.click(btn);
        assert!(visible.get());
        tree.click(btn);
        assert!(!visible.get());
    }

    #[test]
    fn toggle_does_not_fire_action() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let visible = Signal::new(false);
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let btn = tree.add(
            BuiltInButton::visibility_toggle(visible)
                .on_activate(TestCmd::Activate),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        tree.on_command(move |_cmd: &TestCmd| c.set(true));
        tree.click(btn);
        // Toggle mode takes priority — action should not fire
        assert!(!called.get());
    }

    // ── Accessibility ───────────────────────────────────────────────────

    #[test]
    fn a11y_role_and_name() {
        let (tree, btn) = setup();
        let info = tree.accessibility_node(btn);
        assert_eq!(info.role(), fern_core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Test"));
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }

    #[test]
    fn a11y_disabled_state() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(
            BuiltInButton::new(IconWidget::checkmark(16.0))
                .tooltip_literal("Disabled")
                .enabled(false),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(btn);
        assert!(info.is_disabled());
    }

    #[test]
    fn a11y_toggled_state() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let visible = Signal::new(true);
        let btn = tree.add(BuiltInButton::visibility_toggle(visible.clone()));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(btn);
        assert!(info.is_toggled());
    }

    // ── Predefined constructors ─────────────────────────────────────────

    #[test]
    fn browse_has_tooltip() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(BuiltInButton::browse());
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(btn);
        assert_eq!(info.role(), fern_core::accesskit::Role::Button);
        // Tooltip text serves as a11y name
        assert!(info.name().is_some());
    }

    #[test]
    fn clear_has_tooltip() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(BuiltInButton::clear());
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(btn);
        assert!(info.name().is_some());
    }

    #[test]
    fn search_has_tooltip() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let btn = tree.add(BuiltInButton::search());
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let info = tree.accessibility_node(btn);
        assert!(info.name().is_some());
    }

    #[test]
    fn all_predefined_build_without_panic() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(BuiltInButton::browse());
        tree.add(BuiltInButton::expand());
        tree.add(BuiltInButton::search());
        tree.add(BuiltInButton::copy());
        tree.add(BuiltInButton::clear());
        tree.add(BuiltInButton::add());
        tree.add(BuiltInButton::visibility_toggle(Signal::new(false)));
        tree.layout(SizeProposal::exact(800.0, 200.0));
    }

    // ── Visual ──────────────────────────────────────────────────────────

    #[test]
    fn hover_changes_visual() {
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
    fn keyboard_focus_shows_focus_ring() {
        let (mut tree, _btn) = setup();
        let frame_idle = tree.render();
        let idle_shapes = frame_idle.shapes.clone();

        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame_focused = tree.render();
        let focused_shapes = frame_focused.shapes.clone();

        assert_ne!(
            idle_shapes, focused_shapes,
            "keyboard focus should change visual appearance (focus ring)"
        );
    }
}
