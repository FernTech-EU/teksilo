//! Production-quality Button widget — V2 Widget using Signal-based reactivity.
//!
//! Addresses all architectural requirements:
//! - Non-generic (closure-based type erasure, Approach B)
//! - Signal-based reactive state (V2 API)
//! - Theme resolved at paint time (not captured at build time)
//! - V2 attached handlers (HandlerSet) — no event() override
//! - Bindings auto-registered via register_bindings (no manual bind_to)
//! - Minimum touch target size from theme

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{ButtonStyle, ButtonStyleConfig, SharedButtonStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::TextRole;

use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{HStack, TextWidget, VStack};

/// Closed enum naming the design-language variants of `Button`. See
/// [`bastyde_core::styles::ButtonVariant`] for the canonical definition.
///
/// Int UI does **not** ship filled red "destructive" buttons —
/// destructive actions in IntelliJ are plain buttons in confirmation
/// dialogs where the title/body carry the warning. The IntUI default
/// `RecipeButtonStyle` collapses `Destructive → Filled`, `Tinted /
/// Outlined → Plain`, and `Link → Ghost` accordingly. Other design
/// languages (Material 3, macOS) honour the variants distinctly.
pub use bastyde_core::styles::ButtonVariant;
use bastyde_i18n::LocalizedString;

/// Internal interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionState {
    Idle,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

/// Build the interaction handler set shared by every activatable button
/// (`Button`, `IconButton`, `CommandLinkButton`, and any future sibling).
///
/// Centralizes the parts that MUST stay identical across the family and
/// historically drifted when copy-pasted:
/// - hover/focus state tracking,
/// - keyboard `Space`/`Enter` activation with the **lone-KeyUp guard**
///   (a `KeyUp` with no preceding `KeyDown` — e.g. a shortcut consumed
///   the `KeyDown` and focus returned here — must NOT activate),
/// - the AT `Click` action.
///
/// `on_activate` runs on tap, keyboard activation, and AT click. Callers
/// bundle their command action (and any extra side effect, e.g.
/// `IconButton`'s toggle flip) into this single closure so the guard
/// gates all activation paths uniformly. `focusable` is the node's
/// focusability (`Button` is always focusable; `IconButton` exposes it).
pub(crate) fn build_interaction_handlers(
    interaction: Signal<InteractionState>,
    on_activate: Rc<dyn Fn(&mut EventContext)>,
    focusable: bool,
) -> HandlerSet {
    let act_tap = on_activate.clone();
    let act_key = on_activate.clone();
    let act_access = on_activate;
    HandlerSet::new()
        .on_tap({
            let interaction = interaction.clone();
            move |_pos: &bastyde_core::TapEvent, ctx: &mut EventContext| {
                act_tap(ctx);
                interaction.set(InteractionState::Hovered);
            }
        })
        .on_hover({
            let interaction = interaction.clone();
            move |entered: bool, _ctx: &mut EventContext| {
                interaction.set(if entered {
                    InteractionState::Hovered
                } else {
                    InteractionState::Idle
                });
            }
        })
        // Pointer-down press state. The family PROVIDES the Pressed state
        // on mouse-down so the *theme* decides whether to render it: Int
        // UI regular buttons have no pressed state (their recipe resolves
        // pressed → hover), while Int UI icon buttons and other themes do.
        // Returns `Ignored` so the event still reaches the tap recognizer
        // and `on_tap` activation fires. Reverts to Hovered on release
        // only if still Pressed — a drag-out release already went to Idle
        // via `on_hover(false)`, so the guard leaves it there.
        .on_pointer_event({
            let interaction = interaction.clone();
            move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                match event {
                    WidgetEvent::PointerDown { .. } => {
                        interaction.set(InteractionState::Pressed);
                    }
                    WidgetEvent::PointerUp { .. }
                        if interaction.get() == InteractionState::Pressed =>
                    {
                        interaction.set(InteractionState::Hovered);
                    }
                    _ => {}
                }
                EventResponse::Ignored
            }
        })
        .on_key({
            let interaction = interaction.clone();
            move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
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
                        // Lone-KeyUp guard: only activate if we saw the
                        // matching KeyDown (state is Pressed).
                        if interaction.get() != InteractionState::Pressed {
                            return EventResponse::Ignored;
                        }
                        act_key(ctx);
                        interaction.set(InteractionState::Focused);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
        })
        .on_focus({
            let interaction = interaction.clone();
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
        .on_access_action(
            move |action: bastyde_core::accesskit::Action,
                  ctx: &mut EventContext|
                  -> EventResponse {
                if action == bastyde_core::accesskit::Action::Click {
                    act_access(ctx);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            },
        )
        .focusable(focusable)
        .cursor(CursorIcon::Pointer)
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
/// Button::new(lit!("Save"))
///     .variant(ButtonVariant::Filled)
///     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save))
/// ```
/// Type-erased activation closure. Stored as `Box<dyn Fn>` so the
/// same button type works for any handler — typed intent send,
/// direct side effect, window mutation, etc.
type CommandFactory = Box<dyn Fn(&mut EventContext)>;

pub struct Button {
    /// Button label as a `Prop<String>`. `new(...)` / `new(lit!(...))`
    /// store `Prop::Static(resolved)`; `bind_label(signal)` upgrades
    /// it to `Prop::Bound`, so the inner `TextWidget` re-renders
    /// reactively without rebuilding the Button. The accessibility
    /// node's `set_name` reads the current value via `Prop::get()`,
    /// keeping AT in sync with bound updates.
    label: bastyde_core::signal::Prop<String>,
    /// Tier-1 design-language variant hint (Filled, Plain, Ghost, …).
    /// The active [`ButtonStyle`] decides what to do with it.
    variant: ButtonVariant,
    /// Optional per-call override for the active [`ButtonStyle`]. When
    /// `None`, falls through to the theme slot or the
    /// built-in [`crate::styles::RecipeButtonStyle`] default.
    style_override: Option<SharedButtonStyle>,
    action: Option<CommandFactory>,
    /// Initial enabled-state. Forwarded into the arena via
    /// `ctx.enabled_when(self_id, false)` at build time when `false`;
    /// not kept as a runtime snapshot. After `build()` the arena's
    /// `enabled_state` is the single source of truth — leaves resolve
    /// colors via `PaintContext::effective_enabled`, events are gated
    /// by `arena.is_enabled()`, the a11y walker reads it for
    /// `set_disabled()`.
    initial_enabled: bool,
    icon: Option<IconWidget>,
    icon_location: IconLocation,
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    /// Mutually exclusive with `tooltip_text` and `composite_tooltip_content`
    /// — every tooltip setter clears the other two so last-call wins.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body. Hosts an arbitrary widget
    /// tree (charts, grids, conditional rows). Mutually exclusive
    /// with `tooltip_text` and `rich_tooltip_source` per the
    /// last-call-wins matrix.
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    /// Optional `has_popup` hint used when this button acts as a
    /// disclosure trigger for a popup (menu, dialog, listbox, etc.).
    /// Surfaced via `set_has_popup` in `accessibility()`.
    has_popup: Option<bastyde_core::accesskit::HasPopup>,
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
    /// Optional caller-supplied interaction signal. When set, `build()`
    /// uses this signal instead of allocating its own — letting an
    /// external widget (e.g. `PopoverButton`'s disclosure caret)
    /// observe hover / press / focus / disabled state and match the
    /// label's color exactly. See [`Button::share_interaction`].
    shared_interaction: Option<Signal<InteractionState>>,
    /// Optional caller-supplied label/icon color override. When `Some`,
    /// both the label text and any icon are bound to this `ColorProp`
    /// regardless of `style` / interaction state — the auto-derived
    /// cascade is replaced. Used by chrome that has to match a host's
    /// enforced text role (e.g. tab-bar overflow dropdown trigger
    /// inheriting `idle_text_role`). See [`Button::text_role`].
    text_role_override: Option<bastyde_core::color_prop::ColorProp>,
    /// Interaction state signal — set during build().
    interaction: Signal<InteractionState>,
    /// Root child ID — set during build().
    root_child_id: Option<WidgetId>,
}

impl Button {
    /// Construct a button from a `LocalizedString` label. The label may
    /// come from `tr!(...)` (translated) or `lit!(...)`
    /// (explicit non-translated). The text is resolved eagerly at
    /// construction and stored as a plain `String`; locale changes rebuild
    /// the composite parent, which re-creates this `Button` with a fresh
    /// translation.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: bastyde_core::signal::Prop::Static(ls.resolve_now()),
            // Int UI default is a Plain (non-primary) button; the caller
            // opts into `ButtonVariant::Filled` for the one primary action.
            variant: ButtonVariant::Plain,
            style_override: None,
            action: None,
            initial_enabled: true,
            icon: None,
            icon_location: IconLocation::None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            has_popup: None,
            expanded_signal: None,
            shared_interaction: None,
            text_role_override: None,
            leading: None,
            trailing: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Returns the configured visual variant. Used by wrappers like
    /// [`PopoverButton`](crate::popover_widget::PopoverButton) that
    /// derive their own chrome colors from the same recipe-resolution
    /// path the inner Button uses.
    pub fn current_variant(&self) -> ButtonVariant {
        self.variant
    }

    /// Bind the button's internal interaction state to a caller-owned
    /// `Signal<InteractionState>` instead of letting `build()` allocate
    /// its own. Used by wrapper widgets like
    /// [`PopoverButton`](crate::popover_widget::PopoverButton) whose
    /// disclosure caret needs to match the label's color across hover
    /// / press / focus / disabled states.
    ///
    /// The provided signal is reset to `Disabled` when `enabled == false`
    /// during `build()` so the shared signal honors the button's
    /// enabled state without the caller having to seed it.
    pub fn share_interaction(mut self, signal: Signal<InteractionState>) -> Self {
        self.shared_interaction = Some(signal);
        self
    }

    /// Set the Tier-1 design-language variant. The active
    /// [`ButtonStyle`] decides whether to honour or remap it (the IntUI
    /// default `RecipeButtonStyle` collapses Destructive → Filled,
    /// Tinted/Outlined → Plain, Link → Ghost).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the active [`ButtonStyle`] for this widget instance
    /// only. Useful for one-off custom-painted buttons (glassmorphism
    /// CTA, Material-3 ripple, etc.) without forking the Button.
    pub fn style(mut self, style: impl ButtonStyle) -> Self {
        self.style_override = Some(Rc::new(style));
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
    /// `state.map(|s| tr!(status_label(value = s)).resolve_now())` for translated
    /// reactive labels — Button only sees the resolved `String`.
    pub fn bind_label(mut self, label: impl Into<bastyde_core::signal::Prop<String>>) -> Self {
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
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
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
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) — for
    /// one-off tooltips that aren't worth registering in the central
    /// catalog. Overrides any previously set plain `.tooltip(...)`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — third tier, hosting an arbitrary
    /// widget tree (Crusader Kings 3 style: tabbed sections, charts,
    /// progress bars, conditional rows). Promotes to a focusable
    /// `Role::Dialog` after the user dwells for the standard
    /// promotion threshold. Overrides any plain or rich tooltip
    /// previously set on this button.
    pub fn composite_tooltip(
        mut self,
        content: impl bastyde_core::widget::Widget + 'static,
    ) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Set the initial enabled state. Disabled buttons ignore input
    /// and dim their content (the framework's
    /// `PaintContext::effective_enabled` propagates through to the
    /// label/icon leaves). Forwarded into the arena via
    /// `ctx.enabled_when(self_id, false)` at build time.
    ///
    /// For a reactive enabled state, call
    /// `ctx.enabled_when(button_id, my_signal)` from the composing
    /// widget. Both routes write to the same arena `enabled_state`;
    /// an external `enabled_when` registered after this builder runs
    /// wins (last-write semantics) and updates reactively from the
    /// signal.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Override the label and icon's tint with a static `ColorProp`.
    /// When set, the button ignores its `style` and the auto-derived
    /// idle/hover/press text-role cascade — both the label text and
    /// any icon are bound directly to this prop instead. Use for chrome
    /// whose host enforces a single text role across all of its
    /// sub-widgets (e.g. tab-bar overflow-dropdown triggers that must
    /// match the strip's `idle_text_role` regardless of hover state).
    /// Accepts `Color`, `TextRole`, `Signal<Color>`, or `Signal<TextRole>`.
    pub fn text_role(mut self, role: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.text_role_override = Some(role.into());
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
    pub fn has_popup(mut self, kind: bastyde_core::accesskit::HasPopup) -> Self {
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
    /// separated by `btn::BUTTON_ICON_LABEL_GAP`. Single-slot —
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
    /// by the TextWidget. `new(lit!(""))` seeds the placeholder
    /// initial text; `bind_text` immediately overwrites it with the
    /// prop's current value (and tracks updates for `Prop::Bound`).
    fn make_label_text(&self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> TextWidget {
        TextWidget::new(lit!(""))
            .bind_text(self.label.clone())
            .bind_color(color)
            .single_line()
            .a11y_hidden()
    }

    /// Take the configured icon, size it, and bind its tint to `color`.
    /// Shared by every icon-bearing `IconLocation` arm so the size /
    /// color wiring lives in one place.
    ///
    /// A non-`None` `icon_location` with no icon set is a programming
    /// error — `.icon(...)` was never called. In debug builds the
    /// `debug_assert!` surfaces the mistake (mirroring how `Checkbox`
    /// asserts a missing accessible label); release falls back to an
    /// empty path so the button still lays out instead of panicking.
    fn make_icon(&mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> IconWidget {
        use crate::styles::recipe_button_style as btn;
        debug_assert!(
            self.icon.is_some(),
            "Button: icon_location is {:?} but no icon was set via .icon(...)",
            self.icon_location,
        );
        self.icon
            .take()
            .unwrap_or_else(|| {
                IconWidget::from_path(bastyde_canvas::Path::new(), btn::BUTTON_ICON_SIZE)
            })
            .icon_size(btn::BUTTON_ICON_SIZE)
            .bind_color(color)
    }

    /// Assemble the V2 attached-handler set (tap / hover / key / focus /
    /// access-action) wired to `interaction`. Takes `self.action`. The
    /// framework gates pointer / key / access events on
    /// `arena.is_enabled(self_id)` before dispatch and the focus walker
    /// skips disabled subtrees, so none of these closures need a
    /// build-time enabled snapshot — that duality was removed in the
    /// single-sourced-enabled refactor.
    fn build_handler_set(&mut self, interaction: Signal<InteractionState>) -> HandlerSet {
        // Bundle the optional command action into the unified
        // `on_activate` closure consumed by the shared family helper.
        let action: Rc<Option<CommandFactory>> = Rc::new(self.action.take());
        let on_activate: Rc<dyn Fn(&mut EventContext)> = Rc::new(move |ctx: &mut EventContext| {
            if let Some(ref action) = *action {
                action(ctx);
            }
        });
        build_interaction_handlers(interaction, on_activate, true)
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label.get())
            .field("variant", &self.variant)
            .field("initial_enabled", &self.initial_enabled)
            .finish()
    }
}

// --- Label / icon color resolution ---
//
// The active `ButtonStyle` owns chrome (background fill, border, focus
// ring) but the inner content (label + icon) belongs to the Button
// itself, so it picks the text role. The mapping is intentionally
// minimal: `OnAccent` for variants that paint an accent fill, `Primary`
// for everything else, `Disabled` when the button is disabled. Custom
// `ButtonStyle` impls that paint a different background can request
// the Button to use a specific text role via `Button::text_role(...)`.

pub(crate) fn resolve_text_role(variant: ButtonVariant, _state: InteractionState) -> TextRole {
    // Disabled substitution happens at the leaf paint via
    // `ColorProp::resolve(theme, ctx.effective_enabled)` — see
    // `crates/bastyde-core/src/color_prop.rs`. The composite no
    // longer carries `InteractionState::Disabled`; the framework's
    // arena enabled-state drives the dim, and the leaves convert it
    // into `TextRole::Disabled` at paint time.
    match variant {
        ButtonVariant::Filled | ButtonVariant::Destructive => TextRole::OnAccent,
        ButtonVariant::Tinted
        | ButtonVariant::Outlined
        | ButtonVariant::Plain
        | ButtonVariant::Ghost => TextRole::Primary,
        ButtonVariant::Link => TextRole::Link,
    }
}

impl bastyde_core::widget::Widget for Button {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Layout constants for the inner content (icon size,
        // icon-label gap) come from the button recipe. The chrome
        // (padding, corner radius, fill, border) lives on the active
        // `ButtonStyle` impl.
        use crate::styles::recipe_button_style as btn;
        let variant = self.variant;
        let self_id = ctx.self_id();

        // Forward the initial-enabled hint into the arena. After this
        // point the arena is the single source of truth — events,
        // focus, a11y, and the leaves' role-resolution all consult
        // `arena.is_enabled(self_id)` / `PaintContext::effective_enabled`.
        // The interaction signal no longer carries Disabled: that was
        // the snapshot duality the architecture refactor removed.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // Reactive view of "is this widget effectively enabled?".
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Create interaction signal — caller-supplied via
        // `share_interaction` when set (so a wrapping widget's chrome
        // can mirror the label's color), otherwise allocated locally.
        // Seeded to Idle; the arena's enabled-state is consulted
        // separately via `effective_enabled`.
        let interaction = match self.shared_interaction.take() {
            Some(shared) => shared,
            None => ctx.signal(InteractionState::Idle),
        };
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
                bastyde_core::binding::BindingLevel::RepaintOnly,
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
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        // Label/icon color: a caller-supplied override wins over the
        // auto cascade. The override replaces ALL states (idle / hover /
        // press / focus / disabled) — chrome that uses this opts out of
        // interaction-driven color feedback in exchange for matching a
        // host's enforced text role. Both label and icon read this same
        // prop, so a one-line override re-tints the whole button.
        //
        // Chrome (background fill, border, focus ring) is no longer
        // resolved here — the active `ButtonStyle` owns it via
        // `make_body(cfg, ctx)` below. This widget only resolves the
        // CONTENT color (label + icon) since that's part of the inner
        // subtree we hand to the style as `cfg.label`.
        let text_role: bastyde_core::color_prop::ColorProp =
            if let Some(ref over) = self.text_role_override {
                over.clone()
            } else {
                interaction
                    .map(move |s| resolve_text_role(variant, *s))
                    .into()
            };

        // Build the content (icon + label) based on icon_location. The
        // four directional arms (Leading/Trailing/Top/Bottom) share one
        // body: build the icon + label, then assemble them into an
        // HStack or VStack in icon-first / text-first order. Icon size /
        // color wiring is centralized in `make_icon`.
        let icon_location = self.icon_location;
        let content_id = match icon_location {
            IconLocation::None => ctx.add(self.make_label_text(text_role)),
            IconLocation::IconOnly => {
                let icon = self.make_icon(text_role);
                ctx.add(icon)
            }
            // Leading | Trailing | Top | Bottom
            loc => {
                let icon_first = matches!(loc, IconLocation::Leading | IconLocation::Top);
                let vertical = matches!(loc, IconLocation::Top | IconLocation::Bottom);
                let icon = self.make_icon(text_role.clone());
                let icon_id = ctx.add(icon);
                let text_id = ctx.add(self.make_label_text(text_role));
                let (first, second) = if icon_first {
                    (icon_id, text_id)
                } else {
                    (text_id, icon_id)
                };
                let row: Box<dyn Widget> = if vertical {
                    Box::new(
                        VStack::new()
                            .spacing(btn::BUTTON_ICON_LABEL_GAP)
                            .add_child(first)
                            .add_child(second),
                    )
                } else {
                    Box::new(
                        HStack::new()
                            .spacing(btn::BUTTON_ICON_LABEL_GAP)
                            .add_child(first)
                            .add_child(second),
                    )
                };
                ctx.add_boxed(row)
            }
        };

        // If leading or trailing slots are set, wrap the icon+label
        // content in an HStack: `[leading?, content, trailing?]`. When
        // both slots are absent, the wrap is skipped — the original
        // content node goes straight into the padding, keeping the
        // node count identical to the pre-slot Button for the common
        // case.
        let content_id = if self.leading.is_some() || self.trailing.is_some() {
            let mut row = HStack::new().spacing(btn::BUTTON_ICON_LABEL_GAP);
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

        // Delegate chrome (background fill, border, focus ring,
        // padding, min size) to the active `ButtonStyle`. The four
        // boolean signals derive from the local `interaction` state
        // signal so the style can `.zip` them and pick a per-state
        // recipe slot.
        let style: SharedButtonStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.button.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeButtonStyle::default()));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_focused = interaction.map(|s| matches!(s, InteractionState::Focused));
        // `is_disabled` derives from the arena's effective enabled
        // state — NOT from the interaction signal. The interaction
        // signal never carries Disabled anymore (the snapshot-based
        // duality was removed). Style chrome uses this to pick its
        // disabled-background role.
        let is_disabled = effective_enabled.map(|on| !*on);
        let cfg = ButtonStyleConfig {
            label: content_id,
            is_pressed,
            is_hovered,
            is_focused,
            is_disabled,
            variant,
        };
        let root_id = style.make_body(&cfg, ctx);

        // Attach tooltip if configured. The three setters
        // (`tooltip`, `rich_tooltip*`, `composite_tooltip`) are
        // mutually exclusive — every setter clears the other two so
        // exactly one branch runs.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        self.root_child_id = Some(root_id);

        ctx.apply_self_handlers(self.build_handler_set(interaction));

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // A Button is rigid: it sizes to its content and does NOT shrink in an
        // over-constrained row (a truncated action label like "Sav…" reads
        // poorly — the desktop convention is to overflow excess actions into a
        // menu; see `Toolbar`). We therefore take only the content's SIZE and
        // drop its grow/shrink weights. The label still truncates if a caller
        // explicitly constrains the button (e.g. via `FixedSize` / `Shrinkable`).
        match self.root_child_id {
            Some(root_id) => ctx
                .child_size(root_id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
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
        builder.set_role(bastyde_core::accesskit::Role::Button);
        // Read the current label value uniformly through `Prop::get`
        // — Static returns the captured `String`; Bound returns the
        // signal's current value. Keeps AT in sync with `bind_label`.
        builder.set_name(self.label.get());
        // `set_disabled()` is now driven by the framework's
        // accessibility walker from `arena.is_enabled(self_id)`. The
        // composite no longer needs to mirror it — the snapshot path
        // was redundant with the arena and broke under reactive
        // `enabled_when(id, signal)` flips.
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
        builder.add_action(bastyde_core::accesskit::Action::Click);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
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
    use bastyde_core::event::{Modifiers, WidgetEvent};
    use bastyde_core::widget_tree::WidgetTree;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn keyup_without_keydown_does_not_fire() {
        // Regression for the MessageBox reopen bug: when a shortcut
        // consumes Enter's KeyDown (dismissing the modal and restoring
        // focus to the trigger button), the trailing KeyUp must not
        // re-activate the trigger.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(Button::new(lit!("T")).on_activate_fn(move |_ctx| {
            fired_for_btn.set(fired_for_btn.get() + 1);
        }));
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
        use bastyde_core::accessibility::widget_id_to_node_id;
        let label = Signal::new("May 2026".to_string());
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Button::new(lit!(""))
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let plain = tree.add(Button::new(lit!("X")).on_activate_fn(|_| {}));
        let with_slots = tree.add(
            Button::new(lit!("X"))
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

    #[test]
    fn button_is_rigid_and_does_not_shrink_in_a_tight_row() {
        // A Button is rigid: in an over-constrained row it keeps its natural
        // width (overflows) rather than truncating its action label. The
        // desktop convention is to overflow excess actions into a menu (see
        // `Toolbar`), not to silently truncate buttons.
        use crate::primitives::hstack::HStack;
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        let btn = tree.add(Button::new(lit!("Save Document As…")).on_activate_fn(|_| {}));
        let _row = tree.add(HStack::new().add_child(btn));

        tree.layout(SizeProposal::unspecified());
        let natural = tree.bounds(btn).width;
        // Squeeze the row far below natural — the Button keeps its full width.
        tree.layout(SizeProposal::exact(70.0, 40.0));
        let squeezed = tree.bounds(btn).width;

        assert!(
            natural > 100.0,
            "expected a wide natural button, got {natural}"
        );
        assert!(
            (squeezed - natural).abs() < 0.5,
            "button should stay rigid at its natural width \
             (natural={natural}, squeezed={squeezed})"
        );
    }

    #[test]
    fn framework_default_blocks_secondary_tap_on_button() {
        // Framework default: `TapRecognizer::accept = ButtonMask::PRIMARY`.
        // A right-click on a Button does NOT activate. Generalises the
        // tab-specific `primary_click_activates_tab_secondary_does_not`
        // regression to every widget that wires `on_tap`.
        use bastyde_core::event::PointerButton;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(Button::new(lit!("T")).on_activate_fn(move |_ctx| {
            fired_for_btn.set(fired_for_btn.get() + 1);
        }));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let center = tree.bounds(btn).center();

        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.pointer_up_button(center, PointerButton::Secondary);
        assert_eq!(fired.get(), 0, "right-click must not activate a Button");

        tree.pointer_down_button(center, PointerButton::Middle);
        tree.pointer_up_button(center, PointerButton::Middle);
        assert_eq!(fired.get(), 0, "middle-click must not activate a Button");

        // Sanity: primary click still activates.
        tree.pointer_down_button(center, PointerButton::Primary);
        tree.pointer_up_button(center, PointerButton::Primary);
        assert_eq!(fired.get(), 1, "primary-click must activate a Button");
    }

    #[test]
    fn framework_accept_tap_buttons_secondary_fires_handler() {
        // `accept_tap_buttons` opts the auto-wired `TapRecognizer` into
        // a wider button set. With `Secondary` allowed, right-click
        // activates.
        use bastyde_core::event::{ButtonMask, PointerButton};
        use bastyde_core::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let fired = Rc::new(Cell::new(0_u32));
        let fired_for_btn = fired.clone();
        let btn = tree.add(
            Button::new(lit!("T"))
                .on_activate_fn(move |_ctx| {
                    fired_for_btn.set(fired_for_btn.get() + 1);
                })
                .accept_tap_buttons(ButtonMask::PRIMARY | ButtonMask::SECONDARY),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let center = tree.bounds(btn).center();

        tree.pointer_down_button(center, PointerButton::Secondary);
        tree.pointer_up_button(center, PointerButton::Secondary);
        assert_eq!(
            fired.get(),
            1,
            "right-click must activate a Button when accept_tap_buttons includes Secondary",
        );

        tree.pointer_down_button(center, PointerButton::Primary);
        tree.pointer_up_button(center, PointerButton::Primary);
        assert_eq!(fired.get(), 2, "primary-click still activates");
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
        use bastyde_core::accessibility::widget_id_to_node_id;
        use bastyde_core::accesskit::Role;
        use bastyde_core::widget_builder::WidgetBuilder;
        use bastyde_tokens::Color;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(
            Button::new(lit!("Pick"))
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

    #[test]
    fn plain_button_is_a_leaf_no_group_node() {
        // Regression: a Button's chrome is composed from layout primitives
        // (Padding/Center/HStack/…) that emit empty GenericContainer /
        // Unknown AT nodes. VoiceOver announces a GenericContainer as
        // "group", so the button read as "<label>, button, group". The AT
        // walker now collapses presentational nodes — assert the button is
        // a clean leaf and no grouping node survives anywhere.
        use bastyde_core::accessibility::widget_id_to_node_id;
        use bastyde_core::accesskit::Role;
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        let id = tree.add(Button::new(lit!("Valider")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(300.0, 80.0));
        let _ = tree.render();
        let update = tree.sync_accessibility();

        assert!(
            !update
                .nodes
                .iter()
                .any(|(_, n)| n.role() == Role::GenericContainer),
            "no GenericContainer ('group') node should remain in the AT tree"
        );

        let (_, btn) = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == widget_id_to_node_id(id))
            .expect("button node present");
        assert_eq!(btn.role(), Role::Button);
        assert_eq!(btn.label(), Some("Valider"));
        let has_visible_child = btn.children().iter().any(|cid| {
            update
                .nodes
                .iter()
                .find(|(nid, _)| nid == cid)
                .is_some_and(|(_, n)| !n.is_hidden())
        });
        assert!(
            !has_visible_child,
            "button should expose no visible AT child node (it is a leaf)"
        );
    }

    #[test]
    fn theme_slot_supplies_button_style_when_no_override() {
        // End-to-end check that `theme.style_slots.button = Some(rc)`
        // actually feeds the widget when no per-call `.style(...)`
        // override is present. Uses a custom `ButtonStyle` that adds a
        // sentinel `RectWidget` we can spot in the rendered frame.
        use bastyde_core::styles::{ButtonStyle, ButtonStyleConfig};
        use bastyde_tokens::Color;

        struct SentinelButton;
        impl ButtonStyle for SentinelButton {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut bastyde_core::build_context::BuildContext,
            ) -> bastyde_core::widget_id::WidgetId {
                // Distinctive bright-magenta background nobody else paints.
                let bg = ctx.add(
                    crate::primitives::RectWidget::new()
                        .background(Color::new(1.0, 0.0, 1.0, 1.0))
                        .corner_radius(bastyde_tokens::CornerRadius::uniform(0.0)),
                );
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(bg)
                        .add_child(cfg.label),
                )
            }
        }

        let mut theme = bastyde_core::presets::intui::light();
        theme.style_slots.button = Some(Rc::new(SentinelButton));
        let mut tree = WidgetTree::new().with_theme(theme);
        let _btn = tree.add(Button::new(lit!("T")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();

        let sentinel = [1.0_f32, 0.0, 1.0, 1.0];
        assert!(
            frame.shapes.iter().any(|s| s.color == sentinel),
            "the theme's `style_slots.button` impl should drive Button chrome \
             — saw no sentinel magenta rect in the rendered frame",
        );
    }

    #[test]
    fn per_call_style_override_wins_over_theme_slot() {
        // When both `Button::style(...)` AND `theme.style_slots.button`
        // are set, the per-call wins. Verified by installing a sentinel
        // style on the theme then a *different* sentinel via `.style()`.
        use bastyde_core::styles::{ButtonStyle, ButtonStyleConfig};
        use bastyde_tokens::Color;

        struct ThemeSentinel;
        impl ButtonStyle for ThemeSentinel {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut bastyde_core::build_context::BuildContext,
            ) -> bastyde_core::widget_id::WidgetId {
                let bg = ctx.add(
                    crate::primitives::RectWidget::new()
                        .background(Color::new(1.0, 0.0, 1.0, 1.0)) // magenta
                        .corner_radius(bastyde_tokens::CornerRadius::uniform(0.0)),
                );
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(bg)
                        .add_child(cfg.label),
                )
            }
        }

        struct CallSentinel;
        impl ButtonStyle for CallSentinel {
            fn make_body(
                &self,
                cfg: &ButtonStyleConfig,
                ctx: &mut bastyde_core::build_context::BuildContext,
            ) -> bastyde_core::widget_id::WidgetId {
                let bg = ctx.add(
                    crate::primitives::RectWidget::new()
                        .background(Color::new(0.0, 1.0, 0.0, 1.0)) // green
                        .corner_radius(bastyde_tokens::CornerRadius::uniform(0.0)),
                );
                ctx.add(
                    crate::primitives::ZStack::new()
                        .add_child(bg)
                        .add_child(cfg.label),
                )
            }
        }

        let mut theme = bastyde_core::presets::intui::light();
        theme.style_slots.button = Some(Rc::new(ThemeSentinel));
        let mut tree = WidgetTree::new().with_theme(theme);
        let _btn = tree.add(
            Button::new(lit!("T"))
                .style(CallSentinel)
                .on_activate_fn(|_| {}),
        );
        tree.layout(SizeProposal::exact(200.0, 80.0));
        let frame = tree.render();

        let magenta = [1.0_f32, 0.0, 1.0, 1.0];
        let green = [0.0_f32, 1.0, 0.0, 1.0];
        assert!(
            frame.shapes.iter().any(|s| s.color == green),
            "per-call .style(...) override should drive chrome — no green rect found",
        );
        assert!(
            !frame.shapes.iter().any(|s| s.color == magenta),
            "theme slot must be ignored when per-call override is set — magenta should not appear",
        );
    }
}
