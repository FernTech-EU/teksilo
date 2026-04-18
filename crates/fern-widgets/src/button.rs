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
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, Color, ColorTokens, CornerRadius, SurfaceRole, TextRole};

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
///     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save))
/// ```
/// Type-erased activation closure. Stored as `Box<dyn Fn>` so the
/// same button type works for any handler — typed intent send,
/// direct side effect, window mutation, etc.
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

    /// Closure invoked on activation. Use `ctx.send_intent(...)` to
    /// route activation through the Action/Intent system, or inline
    /// the behavior directly.
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

fn resolve_bg_role(style: ButtonVariant, state: InteractionState) -> SurfaceRole {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Disabled) => SurfaceRole::AccentDisabled,
        (ButtonVariant::Default, InteractionState::Pressed) => SurfaceRole::AccentPressed,
        (ButtonVariant::Default, InteractionState::Hovered) => SurfaceRole::AccentHover,
        (ButtonVariant::Default, _) => SurfaceRole::Accent,

        (ButtonVariant::Regular, InteractionState::Pressed) => SurfaceRole::Pressed,
        (ButtonVariant::Regular, InteractionState::Hovered) => SurfaceRole::Hover,
        (ButtonVariant::Regular, _) => SurfaceRole::Main,

        (ButtonVariant::Flat, InteractionState::Pressed) => SurfaceRole::Pressed,
        (ButtonVariant::Flat, InteractionState::Hovered) => SurfaceRole::Hover,
        (ButtonVariant::Flat, _) => SurfaceRole::Transparent,
    }
}

fn resolve_text_role(style: ButtonVariant, state: InteractionState) -> TextRole {
    match (style, state) {
        (ButtonVariant::Default, InteractionState::Disabled) => TextRole::Disabled,
        (ButtonVariant::Default, _) => TextRole::OnAccent,
        (ButtonVariant::Regular | ButtonVariant::Flat, InteractionState::Disabled) => {
            TextRole::Disabled
        }
        (ButtonVariant::Regular | ButtonVariant::Flat, _) => TextRole::Primary,
    }
}

fn resolve_border_role(style: ButtonVariant, state: InteractionState) -> BorderRole {
    // Int UI convention: the border IS the focus indicator (accent color,
    // thicker stroke) — no external ring.
    if state == InteractionState::Focused {
        return BorderRole::Focused;
    }
    match style {
        ButtonVariant::Default | ButtonVariant::Flat => BorderRole::Transparent,
        ButtonVariant::Regular => match state {
            InteractionState::Hovered | InteractionState::Pressed => BorderRole::Strong,
            _ => BorderRole::Default,
        },
    }
}

/// Resolve the button's border width. Focused → `focus_ring_width`
/// so the accent border is visually distinct; otherwise the
/// variant-specific rest width (0 dp for Default/Flat, 1 dp for
/// Regular).
fn resolve_border_width(style: ButtonVariant, state: InteractionState, normal_bw: f32, focus_bw: f32) -> f32 {
    if state == InteractionState::Focused {
        return focus_bw;
    }
    match style {
        ButtonVariant::Default | ButtonVariant::Flat => 0.0,
        ButtonVariant::Regular => normal_bw,
    }
}

impl fern_core::widget::Widget for Button {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // `button_style` is a one-time snapshot of layout constants
        // (padding, icon size, corner radius, min width, height). These are
        // typography/shape tokens that don't vary between light and dark
        // themes; colors are driven reactively through role signals below.
        let button_style = ctx.theme_signal().get().components.button;
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

        // Derived reactive roles — map interaction state to semantic roles,
        // resolved against the current theme at paint time. Signal<Role>
        // replaces the older `interaction.zip(&theme_signal).map(...)` zip
        // and drops the explicit theme-signal plumbing.
        let bg_role = interaction.map(move |s| resolve_bg_role(style, *s));
        let text_role = interaction.map(move |s| resolve_text_role(style, *s));
        let border_role = interaction.map(move |s| resolve_border_role(style, *s));
        let normal_bw = button_style.border_width;
        let focus_bw = ctx.theme_signal().get().shape.focus_ring_width;
        let border_width = interaction.map(move |s| {
            resolve_border_width(style, *s, normal_bw, focus_bw)
        });

        // Build the content (icon + label) based on icon_location
        let content_id = match self.icon_location {
            IconLocation::None => {
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_role)
                    .single_line()
                    .a11y_hidden();
                ctx.add(text)
            }
            IconLocation::IconOnly => {
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon = icon.icon_size(button_style.icon_size).bind_color(text_role);
                ctx.add(icon)
            }
            IconLocation::Leading => {
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_role.clone()));
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_role)
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
                    .bind_color(text_role.clone())
                    .single_line()
                    .a11y_hidden();
                let text_id = ctx.add(text);
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_role));
                ctx.add(
                    HStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(text_id)
                        .add_child(icon_id),
                )
            }
            IconLocation::Top => {
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_role.clone()));
                let text = TextWidget::new_literal(&self.label)
                    .bind_color(text_role)
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
                    .bind_color(text_role.clone())
                    .single_line()
                    .a11y_hidden();
                let text_id = ctx.add(text);
                let icon = self.icon.take().unwrap_or_else(|| IconWidget::from_path(fern_canvas::Path::new(), button_style.icon_size));
                let icon_id = ctx.add(icon.icon_size(button_style.icon_size).bind_color(text_role));
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
        .child_id(content_id);
        let padding_id = ctx.add(padding);

        // Int UI convention: the button's own border is the focus
        // indicator. Border width reacts to focus via
        // `resolve_border_width`; color reacts via `resolve_border`.
        // No external ring.
        let rect = RectWidget::new()
            .bind_background(bg_role)
            .bind_border_color(border_role)
            .bind_border_width(border_width)
            .corner_radius(CornerRadius::uniform(button_style.corner_radius));
        let rect_id = ctx.add(rect);

        let zstack = ZStack::new().add_child(rect_id).add_child(padding_id);
        let zstack_id = ctx.add(zstack);

        // Int UI buttons are 24 dp tall with a 72 dp minimum width.
        let root_id = ctx.add(
            crate::primitives::MinSize::new(button_style.min_width, button_style.height)
                .child_id(zstack_id),
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

