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
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
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
    /// Button label as a `Prop<String>`. `new(...)` / `new_literal(...)`
    /// store `Prop::Static(resolved)`; `bind_label(signal)` upgrades
    /// it to `Prop::Bound`, so the inner `TextWidget` re-renders
    /// reactively without rebuilding the Button. The accessibility
    /// node's `set_name` reads the current value via `Prop::get()`,
    /// keeping AT in sync with bound updates.
    label: fern_core::signal::Prop<String>,
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
    /// Arbitrary widget rendered to the leading edge of the button's
    /// content (left in LTR, right in RTL). Composes with `.icon(...)`:
    /// the order is `[leading_slot, icon+label, trailing_slot]`. Slot
    /// widgets paint and report a11y on their own — Button does not
    /// retint them and does not auto-suppress their AT roles. Apps
    /// whose slot widgets would otherwise pollute the AT tree
    /// (e.g. ColorSwatch with `Role::ColorWell`) should pass
    /// `widget.access_hidden(true)` so the Button's
    /// `Role::Button` stays the single declared role.
    leading: Option<Box<dyn Widget>>,
    /// Same shape as `leading`, rendered to the trailing edge.
    trailing: Option<Box<dyn Widget>>,
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
            label: fern_core::signal::Prop::Static(ls.resolve_now()),
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
            leading: None,
            trailing: None,
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

    /// Bind the button's label to a reactive source — replaces the
    /// static label captured at `new(...)`. Accepts any
    /// `impl Into<Prop<String>>`: a `Signal<String>` for live
    /// updates, or a plain `String` (which is the same as constructing
    /// the button with that string). Mirrors
    /// [`TextWidget::bind_text`](crate::primitives::TextWidget::bind_text).
    /// The inner label `TextWidget` is built with the bound prop, so
    /// the visible text refreshes without rebuilding the Button. The
    /// AT node's `set_name` reads the current value via `Prop::get`.
    ///
    /// Translation note: derive the signal with
    /// `state.map(|s| tr!("key", s).resolve_now())` for translated
    /// reactive labels — Button only sees the resolved `String`.
    pub fn bind_label(mut self, label: impl Into<fern_core::signal::Prop<String>>) -> Self {
        self.label = label.into();
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

    /// Insert a widget at the leading edge of the button's content
    /// (left in LTR, right in RTL). Composes with `.icon(...)`: the
    /// final order is `[leading_slot, icon+label, trailing_slot]`,
    /// separated by `button_style.icon_label_gap`. Single-slot —
    /// calling `.leading(...)` again replaces the previous slot.
    /// Stack multiple widgets with an explicit `HStack`.
    ///
    /// The slot widget paints itself and emits its own a11y. Button
    /// does **not** retint it (so e.g. a `ColorSwatch` keeps its own
    /// color through every interaction state). If the slot widget
    /// declares an AT role of its own — `ColorSwatch` is the canonical
    /// case (`Role::ColorWell`) — pass `widget.access_hidden(true)`
    /// so the trigger reads as a single Button node instead of a
    /// Button containing a redundant ColorWell child.
    pub fn leading(mut self, widget: impl Widget + 'static) -> Self {
        self.leading = Some(Box::new(widget));
        self
    }

    /// Same as [`leading`](Self::leading) but at the trailing edge
    /// (right in LTR, left in RTL). Common uses: chevron-down hint
    /// on disclosure triggers, clear-X on search fields, status
    /// badges on segmented control segments.
    pub fn trailing(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing = Some(Box::new(widget));
        self
    }

    /// Construct the label `TextWidget` used inside the button's
    /// content layout. Always routes through `bind_text(prop)` —
    /// `Prop::Static` and `Prop::Bound` are both handled uniformly
    /// by the TextWidget. `new_literal("")` seeds the placeholder
    /// initial text; `bind_text` immediately overwrites it with the
    /// prop's current value (and tracks updates for `Prop::Bound`).
    fn make_label_text(
        &self,
        color: impl Into<fern_core::color_prop::ColorProp>,
    ) -> TextWidget {
        TextWidget::new_literal("")
            .bind_text(self.label.clone())
            .bind_color(color)
            .single_line()
            .a11y_hidden()
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label.get())
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

        // If `bind_label(signal)` was used, register the prop on the
        // Button itself at AccessibilityOnly so `set_name` re-runs
        // when the signal changes. The inner `TextWidget` already
        // re-renders via its own `bind_text` plumbing — this binding
        // is purely for the AT name.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.label.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

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
                let text = self.make_label_text(text_role);
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
                let text = self.make_label_text(text_role);
                let text_id = ctx.add(text);
                ctx.add(
                    HStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(icon_id)
                        .add_child(text_id),
                )
            }
            IconLocation::Trailing => {
                let text = self.make_label_text(text_role.clone());
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
                let text = self.make_label_text(text_role);
                let text_id = ctx.add(text);
                ctx.add(
                    VStack::new()
                        .spacing(button_style.icon_label_gap)
                        .add_child(icon_id)
                        .add_child(text_id),
                )
            }
            IconLocation::Bottom => {
                let text = self.make_label_text(text_role.clone());
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

        // If leading or trailing slots are set, wrap the icon+label
        // content in an HStack: `[leading?, content, trailing?]`. When
        // both slots are absent, the wrap is skipped — the original
        // content node goes straight into the padding, keeping the
        // node count identical to the pre-slot Button for the common
        // case.
        let content_id = if self.leading.is_some() || self.trailing.is_some() {
            let mut row = HStack::new().spacing(button_style.icon_label_gap);
            if let Some(leading) = self.leading.take() {
                let id = ctx.add_boxed(leading);
                row = row.add_child(id);
            }
            row = row.add_child(content_id);
            if let Some(trailing) = self.trailing.take() {
                let id = ctx.add_boxed(trailing);
                row = row.add_child(id);
            }
            ctx.add(row)
        } else {
            content_id
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
            let delay = std::time::Duration::from_millis(200);
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
                            // Fire only if we saw the matching KeyDown. A lone
                            // KeyUp means the KeyDown was consumed elsewhere
                            // (shortcut registry, focus transfer) and this
                            // widget is not the activation target.
                            if interaction.get() != InteractionState::Pressed {
                                return EventResponse::Ignored;
                            }
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

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(root_id) => ctx
                .child_size(root_id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }.into()
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
        // Read the current label value uniformly through `Prop::get`
        // — Static returns the captured `String`; Bound returns the
        // signal's current value. Keeps AT in sync with `bind_label`.
        builder.set_name(self.label.get());
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
    use fern_core::event::{Modifiers, WidgetEvent};
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn keyup_without_keydown_does_not_fire() {
        // Regression for the MessageBox reopen bug: when a shortcut
        // consumes Enter's KeyDown (dismissing the modal and restoring
        // focus to the trigger button), the trailing KeyUp must not
        // re-activate the trigger.
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(
            Button::new_literal("T").on_activate_fn(move |_ctx| {
                fired_for_btn.set(fired_for_btn.get() + 1);
            }),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        tree.focus(btn);

        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            0,
            "a lone KeyUp (no matching KeyDown) must not activate the button",
        );

        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            text: None,
        });
        tree.dispatch_event(WidgetEvent::KeyUp {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            fired.get(),
            1,
            "a matched KeyDown + KeyUp pair must activate exactly once",
        );
    }

    #[test]
    fn bind_label_updates_at_name_when_signal_changes() {
        // Regression for the calendar header use case: a Button bound
        // to a `Signal<String>` must (1) display the signal's current
        // value and (2) refresh its accessibility name when the
        // signal changes — without rebuilding the parent.
        use fern_core::accessibility::widget_id_to_node_id;
        let label = Signal::new("May 2026".to_string());
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Button::new_literal("")
                .bind_label(label.clone())
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let target = widget_id_to_node_id(id);
        let update = tree.sync_accessibility();
        let (_, node) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .expect("button node");
        assert_eq!(node.label().unwrap_or_default(), "May 2026");

        // Flip the signal — AT name should refresh after the next
        // layout pass (the bind_label registration triggers a
        // re-evaluation of `accessibility()`).
        label.set("2026".to_string());
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let update = tree.sync_accessibility();
        let (_, node) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .expect("button node after relabel");
        assert_eq!(node.label().unwrap_or_default(), "2026");
    }

    #[test]
    fn slots_widen_button_to_accommodate_their_intrinsic_size() {
        // A button with leading + trailing slots reports a wider
        // intrinsic size than the same button without slots — proves
        // the slots actually entered the layout pass. Layout uses
        // `unspecified()` so each button reports its intrinsic width
        // rather than getting stretched to a parent proposal. Both
        // sides also clear the theme's `min_width` (~72dp) which
        // would otherwise mask the slot contribution on the plain
        // button.
        use crate::primitives::MinSize;
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let plain = tree.add(Button::new_literal("X").on_activate_fn(|_| {}));
        let with_slots = tree.add(
            Button::new_literal("X")
                .leading(MinSize::new(120.0, 12.0))
                .trailing(MinSize::new(120.0, 12.0))
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::unspecified());
        let plain_w = tree.bounds(plain).width;
        let slot_w = tree.bounds(with_slots).width;
        assert!(
            slot_w >= plain_w + 200.0,
            "expected slot button to be at least 200dp wider than plain (plain={plain_w}, slot={slot_w})",
        );
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn hidden_slot_marks_swatch_node_as_at_hidden() {
        // ColorSwatch declares `Role::ColorWell`. Dropped raw into a
        // Button slot it would appear as a redundant ColorWell child
        // under the Button's node. `.access_hidden(true)` is the
        // documented escape hatch — confirm the swatch's AT node
        // carries the hidden flag (AT readers skip nodes flagged
        // hidden, even though the node still exists in the tree).
        use crate::color_picker::ColorSwatch;
        use fern_core::accessibility::widget_id_to_node_id;
        use fern_core::accesskit::Role;
        use fern_core::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            Button::new_literal("Pick")
                .leading(ColorSwatch::new(Color::RED).access_hidden(true))
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let target = widget_id_to_node_id(id);
        let update = tree.sync_accessibility();
        let (_, btn_node) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == target)
            .expect("button node");
        assert_eq!(btn_node.role(), Role::Button);
        let color_well_visible = update
            .nodes
            .iter()
            .any(|(_, n)| n.role() == Role::ColorWell && !n.is_hidden());
        assert!(
            !color_well_visible,
            "hidden swatch should not emit a non-hidden ColorWell node",
        );
    }
}

