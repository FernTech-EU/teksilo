//! IconButton — a square, icon-only, flat-surface button.
//!
//! Five sizes covering both **embedded** use (inside another widget's
//! trailing slot — TextInput's clear-X, ComboBox's chevron, SearchField's
//! magnifier) and **stand-alone** use (toolbars, rich menus, hero CTAs).
//! The `.embedded()` flag opts into the JetBrains "built-in" look —
//! dimmer icon at rest (Secondary), brightening on hover (Primary),
//! flashing accent on press — so an IconButton living inside a TextInput
//! doesn't compete visually with the field's text. Without the flag the
//! icon stays at full visual weight (Primary at rest), the right default
//! for stand-alone toolbar / menu rows.
//!
//! ```ignore
//! // Stand-alone toolbar use — full-weight icon.
//! IconButton::new(IconWidget::from_svg(MY_SVG))
//!     .toolbar()
//!     .tooltip(tr!(save()))
//!     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save))
//!
//! // Embedded inside a TextInput's trailing slot — dim until hover.
//! IconButton::clear()
//!     .embedded()
//!     .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Clear))
//! ```
//!
//! ## Predefined constructors
//!
//! Common roles ship with the appropriate icon and an i18n tooltip
//! (which doubles as the AT name). They are size- and mode-agnostic —
//! call `.embedded()`, `.toolbar()`, `.large()`, etc. to configure:
//!
//! ```ignore
//! IconButton::browse().embedded()           // 24 dp, dim — TextInput trailing
//! IconButton::clear().embedded()            // 24 dp, dim — clear-X
//! IconButton::search().toolbar()            // 40 dp, full weight — toolbar
//! IconButton::visibility_toggle(visible)    // password-field eye toggle
//! ```
//!
//! ## Bistate
//!
//! Two distinct toggle modes:
//!
//! - [`IconButton::toggle`] — surface-tint bistate: clicking flips the
//!   bound `Signal<bool>`; while `true`, the background reads as
//!   `SurfaceRole::Selected` ("on"). Same icon throughout. The
//!   pin-this-row / select-this-tool pattern.
//! - [`IconButton::toggle_with_icon`] — surface-tint **and** icon-swap
//!   bistate: same surface flip plus the icon glyph swaps to a second
//!   icon. The visibility-toggle pattern (eye ↔ eye-off).
//!
//! ## Slot convention
//!
//! Host widgets that accept icon buttons follow the `trailing_slot`
//! convention established by [`TabWidget`](crate::tab_widget::TabWidget):
//!
//! ```ignore
//! TextInput::new(value)
//!     .trailing_slot(HStack::new().spacing(0.0)
//!         .child(IconButton::clear().embedded().on_activate_fn(|ctx| ctx.send_intent(AppIntent::Clear)))
//!         .child(IconButton::browse().embedded().on_activate_fn(|ctx| ctx.send_intent(AppIntent::Browse)))
//!     )
//! ```

use bastyde_i18n::lit;
use std::rc::Rc;
use std::sync::OnceLock;

use bastyde_canvas::{Path, Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{IconButtonStyleConfig, SharedIconButtonStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::TextRole;

use crate::primitives::Switcher;
use crate::primitives::icon_widget::IconWidget;

/// Size variant for [`IconButton`]. See [`bastyde_core::styles::IconButtonSize`]
/// for the canonical definition. Variants are calibrated to the
/// IntelliJ Int UI scale (Compact 22 dp, Default 24 dp, Toolbar 30 dp,
/// Large 40 dp, Hero 50 dp).
pub use bastyde_core::styles::IconButtonSize;

use crate::button::InteractionState;

/// Type-erased action factory — captures the concrete command type.
type ActionFactory = Box<dyn Fn(&mut EventContext)>;

/// A square, icon-only, flat-surface button. See module docs for
/// embedded vs stand-alone modes, the five sizes, and the two bistate
/// toggle modes.
pub struct IconButton {
    // Configuration (set via builder)
    icon: IconWidget,
    tooltip_text: Option<String>,
    /// Optional rich tooltip source — registry key or inline content.
    /// Mutually exclusive with `tooltip_text` and `composite_tooltip_content`.
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (CK3-style widget tree).
    /// Mutually exclusive with the other two tooltip slots.
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    /// Initial enabled-state. Forwarded into the arena via
    /// `ctx.enabled_when(self_id, false)` at build time when `false`;
    /// not kept as a runtime snapshot. After `build()` the arena's
    /// `enabled_state` is the single source of truth; the leaves
    /// (icon, label) read it through `PaintContext::effective_enabled`
    /// for color resolution, event dispatch reads it via
    /// `arena.is_enabled()` for gating, the a11y walker reads it for
    /// `set_disabled()`.
    initial_enabled: bool,
    size: IconButtonSize,
    /// Embedded mode — Secondary-at-rest icon color, the JetBrains
    /// "built-in" look. Default `false` (stand-alone, full-weight icon).
    embedded: bool,
    action: Option<ActionFactory>,
    /// Whether the button takes keyboard focus on Tab navigation.
    /// `true` (default): focusable when enabled. `false`: never
    /// focusable — used for the close-button-inside-a-tab pattern
    /// (Firefox / Chrome convention: Tab moves between *tabs*, not
    /// onto each tab's close button).
    focusable: bool,

    // Toggle support
    toggled: Option<Signal<bool>>,
    /// Optional alternate icon for the icon-swap toggle mode set via
    /// [`IconButton::toggle_with_icon`]. When `None`, surface-tint-only
    /// toggle mode applies (set via [`IconButton::toggle`]).
    toggled_icon: Option<IconWidget>,

    // Disclosure support — wired up by `PopoverIconButton` so AT
    // announces the button as a menu / popup trigger and reflects the
    // open state. Both fields are opt-in via `.has_popup(...)` /
    // `.expanded_when(...)`.
    has_popup: Option<bastyde_core::accesskit::HasPopup>,
    expanded_signal: Option<Signal<bool>>,

    /// Optional caller-supplied interaction signal. When set, `build()`
    /// uses this signal instead of allocating its own — letting an
    /// external widget (e.g. `PopoverIconButton`'s disclosure caret)
    /// observe hover / press / focus / disabled state and match the
    /// icon's color exactly. See [`IconButton::share_interaction`].
    shared_interaction: Option<Signal<InteractionState>>,

    /// Optional caller-supplied icon-color override. When `Some`, the
    /// icon's tint is bound to this `ColorProp` (a `Color`, role, or
    /// `Signal<Color>`) regardless of `embedded` / interaction state —
    /// the auto-derived idle/hover/press cascade is replaced. Used by
    /// chrome that has to read with a host's text-role rather than the
    /// IconButton's stand-alone palette (e.g. tab-bar scroll arrows
    /// inheriting `idle_text_role`).
    icon_role_override: Option<bastyde_core::color_prop::ColorProp>,

    /// Per-call style override. When `None`, falls back to the IntUI
    /// default `RecipeIconButtonStyle`.
    style_override: Option<SharedIconButtonStyle>,

    // Build state (set in build())
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl IconButton {
    /// Create an icon button from a custom icon. Defaults to
    /// `IconButtonSize::Default` (24 dp) and stand-alone visual mode.
    /// Apply `.embedded()` for the JetBrains "built-in" dim look,
    /// and one of the size methods (`.large()` / `.toolbar()` /
    /// `.hero()`) or `.size(...)` to pick a different size.
    pub fn new(icon: IconWidget) -> Self {
        Self {
            icon,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            initial_enabled: true,
            size: IconButtonSize::Default,
            embedded: false,
            action: None,
            focusable: true,
            toggled: None,
            toggled_icon: None,
            has_popup: None,
            expanded_signal: None,
            shared_interaction: None,
            icon_role_override: None,
            style_override: None,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `IconButtonStyle` for just this IconButton instance — same role
    /// as `Button::style(...)`. The override fully owns the background +
    /// border + size composition; icon coloring stays on the widget.
    pub fn style(mut self, style: impl bastyde_core::styles::IconButtonStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Returns the configured size variant. Used by wrappers like
    /// [`PopoverIconButton`](crate::popover_icon_button::PopoverIconButton)
    /// that need to reason about the trigger's footprint at build time
    /// (e.g. to skip a corner decoration that wouldn't fit at Compact).
    pub fn size_variant(&self) -> IconButtonSize {
        self.size
    }

    /// Returns whether the button is in the JetBrains "built-in" /
    /// embedded color profile (Secondary at rest). Mirror getter to
    /// [`size_variant`](Self::size_variant) for wrappers that want to
    /// derive their own chrome colors from the same icon role.
    pub fn is_embedded(&self) -> bool {
        self.embedded
    }

    /// Bind the button's internal interaction state to a caller-owned
    /// `Signal<InteractionState>` instead of letting `build()` allocate
    /// its own. Used by wrapper widgets like
    /// [`PopoverIconButton`](crate::popover_icon_button::PopoverIconButton)
    /// whose disclosure caret needs to match the icon's color across
    /// hover / press / focus / disabled states.
    ///
    /// The provided signal is reset to `Disabled` when `enabled == false`
    /// during `build()` so the shared signal honors the button's
    /// enabled state without the caller having to seed it.
    pub fn share_interaction(mut self, signal: Signal<InteractionState>) -> Self {
        self.shared_interaction = Some(signal);
        self
    }

    /// Opt into the **embedded** visual treatment — the JetBrains
    /// "built-in button" look. Icon dims to `Secondary` at rest,
    /// brightens to `Primary` on hover, flashes `Accent` on press —
    /// designed to live inside another widget's trailing slot
    /// (TextInput's clear-X, ComboBox's chevron) without competing
    /// visually with the host's content. Default mode is stand-alone
    /// (icon at full visual weight, `Primary` always).
    pub fn embedded(mut self) -> Self {
        self.embedded = true;
        self
    }

    /// Override the icon's tint with a static `ColorProp`. When set,
    /// the icon ignores `embedded` and the auto-derived idle/hover/press
    /// role cascade — its color is bound directly to this prop instead.
    /// Use for chrome whose host enforces a single text role across all
    /// of its sub-widgets (e.g. tab-bar scroll arrows that must match
    /// the tab strip's `idle_text_role` regardless of hover state).
    /// Accepts `Color`, `TextRole`, `Signal<Color>`, or `Signal<TextRole>`.
    pub fn icon_role(mut self, role: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.icon_role_override = Some(role.into());
        self
    }

    /// Whether the button takes keyboard focus. Default `true` —
    /// the button is focusable when enabled. Set to `false` for
    /// embedded-control patterns where the parent owns focus and
    /// keyboard interaction goes through the parent (e.g. the
    /// close button inside a tab header — Tab moves between tabs,
    /// not onto their close buttons).
    pub fn focusable(mut self, on: bool) -> Self {
        self.focusable = on;
        self
    }

    /// Attach a tooltip that appears after a hover delay. Required —
    /// the tooltip text doubles as the AT name for icon-only buttons.
    pub fn tooltip(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = text.into();
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
    /// and dim their icon (handled by the framework's
    /// `PaintContext::effective_enabled`). Forwarded into the arena
    /// via `ctx.enabled_when(self_id, false)` at build time.
    ///
    /// For a reactive enabled state — e.g. a toolbar button that
    /// enables only when the caret is inside a table — call
    /// `ctx.enabled_when(button_id, my_signal)` from the composing
    /// widget's `build()` instead of (or in addition to) this builder.
    /// Both routes write to the same arena `enabled_state`; an
    /// external `enabled_when` registered after this builder runs
    /// wins (last-write semantics) and updates reactively from the
    /// signal.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Set the size variant. Most callers prefer the named shortcuts
    /// [`large`](Self::large) / [`toolbar`](Self::toolbar) /
    /// [`hero`](Self::hero); use `.size(...)` for `Compact` or for
    /// programmatic size selection.
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Shortcut for `.size(IconButtonSize::Toolbar)` (30 dp) — the
    /// IntelliJ side-toolbar density (left / right / top window edges).
    pub fn toolbar(mut self) -> Self {
        self.size = IconButtonSize::Toolbar;
        self
    }

    /// Shortcut for `.size(IconButtonSize::Large)` (40 dp) —
    /// emphasized stand-alone buttons in rich menus and detail panes.
    pub fn large(mut self) -> Self {
        self.size = IconButtonSize::Large;
        self
    }

    /// Shortcut for `.size(IconButtonSize::Hero)` (50 dp) — hero /
    /// landing-screen CTAs.
    pub fn hero(mut self) -> Self {
        self.size = IconButtonSize::Hero;
        self
    }

    /// Closure invoked on activation. Fires after the toggle signal
    /// (if any) is flipped, so apps observing the closure see the
    /// post-flip state.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Box::new(f));
        self
    }

    /// Enable **surface-tint** bistate: clicking flips `state` and the
    /// background reads as `SurfaceRole::Selected` while `state == true`.
    /// The icon glyph is unchanged. Pin / select / lock-toggle pattern.
    /// `on_activate_fn`, if any, still fires after the flip.
    ///
    /// For the eye / eye-off pattern where the icon glyph also changes,
    /// use [`toggle_with_icon`](Self::toggle_with_icon) instead.
    pub fn toggle(mut self, state: Signal<bool>) -> Self {
        self.toggled = Some(state);
        self.toggled_icon = None;
        self
    }

    /// Enable **surface-tint plus icon-swap** bistate: clicking flips
    /// `state`, the background flips to `Selected`, **and** the icon
    /// swaps to `toggled_icon`. The visibility-toggle pattern (eye ↔
    /// eye-off). For surface-only bistate (icon stays the same), use
    /// [`toggle`](Self::toggle).
    pub fn toggle_with_icon(mut self, state: Signal<bool>, toggled_icon: IconWidget) -> Self {
        self.toggled = Some(state);
        self.toggled_icon = Some(toggled_icon);
        self
    }

    /// Declare that this button is a disclosure trigger for a popup
    /// (menu, dialog, listbox, …). Surfaced via `set_has_popup` in
    /// the a11y node so screen readers announce it as opening the
    /// named popup kind. Wired automatically by
    /// [`PopoverIconButton`](crate::popover_icon_button::PopoverIconButton).
    pub fn has_popup(mut self, kind: bastyde_core::accesskit::HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }

    /// Bind a signal reporting whether this button's popup is
    /// currently visible. The popover wrapper owns the signal and
    /// flips it on show / dismiss; IconButton reads it in
    /// `accessibility()` to publish `set_expanded`. Only meaningful
    /// alongside [`has_popup`](Self::has_popup).
    pub fn expanded_when(mut self, signal: Signal<bool>) -> Self {
        self.expanded_signal = Some(signal);
        self
    }

    // ── Predefined constructors ─────────────────────────────────────────
    //
    // Each ships a standard icon and an i18n tooltip. They are size-
    // and mode-agnostic — chain `.embedded()` for the dim look,
    // `.toolbar()` / `.large()` / `.hero()` for the size.

    /// Browse button (ellipsis icon). Opens a file/directory chooser.
    pub fn browse() -> Self {
        Self::new((BuiltInIcons::global().browse)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_browse()))
    }

    /// Expand button (diagonal resize arrows). Enlarges a constrained field.
    pub fn expand() -> Self {
        Self::new((BuiltInIcons::global().expand)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_expand()))
    }

    /// Search button (magnifier icon). Triggers a search.
    pub fn search() -> Self {
        Self::new((BuiltInIcons::global().search)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_search()))
    }

    /// Copy button (clipboard icon). Copies the field content.
    pub fn copy() -> Self {
        Self::new((BuiltInIcons::global().copy)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_copy()))
    }

    /// Clear button (X icon). Clears the field content.
    pub fn clear() -> Self {
        Self::new((BuiltInIcons::global().clear)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_clear()))
    }

    /// Add button (plus icon). Adds a new entry.
    pub fn add() -> Self {
        Self::new((BuiltInIcons::global().add)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_add()))
    }

    /// Notification bell. Used by
    /// [`NotificationCenterButton`](crate::notification::NotificationCenterButton)
    /// — the bell-icon trigger that opens the notification log popover.
    pub fn bell() -> Self {
        Self::new((BuiltInIcons::global().bell)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_bell()))
    }

    /// Visibility toggle (eye / eye-off). Toggles password visibility.
    /// Uses the icon-swap bistate mode internally — the icon advertises
    /// the **expected action**, matching the prevailing password-field
    /// convention (1Password, Bitwarden, KeePass, Chrome, GitHub):
    /// `eye` (open) while the value is hidden, suggesting "click to
    /// reveal"; `eye_off` (closed) once revealed, suggesting "click to
    /// hide". `set_toggled` still reports the literal current state, so
    /// AT readers are not misled.
    ///
    /// For a current-state-instead semantics (icon shows what IS),
    /// build your own with [`toggle_with_icon`](Self::toggle_with_icon)
    /// and the eye glyphs in the opposite order.
    ///
    /// The `visible` signal is flipped on each click. The host widget reads
    /// it to decide whether to mask or show the text.
    pub fn visibility_toggle(visible: Signal<bool>) -> Self {
        let icons = BuiltInIcons::global();
        Self::new((icons.eye)())
            .toggle_with_icon(visible, (icons.eye_off)())
            .tooltip(bastyde_i18n::tr_widget!(a11y_builtin_visibility()))
    }
}

impl std::fmt::Debug for IconButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconButton")
            .field("initial_enabled", &self.initial_enabled)
            .field("size", &self.size)
            .field("embedded", &self.embedded)
            .finish()
    }
}

// ── Icon coloring ───────────────────────────────────────────────────────────
//
// Background / border / size composition lives in the active
// `IconButtonStyle` (default: `RecipeIconButtonStyle`). The widget retains
// only icon coloring policy — embedded mode dims to `Secondary` at rest
// (the JetBrains "built-in" look), stand-alone mode stays at `Primary`
// always so toolbar / menu icons read at full weight.

pub(crate) fn resolve_icon_role_embedded(state: InteractionState) -> TextRole {
    match state {
        InteractionState::Idle | InteractionState::Focused => TextRole::Secondary,
        InteractionState::Hovered => TextRole::Primary,
        InteractionState::Pressed => TextRole::Accent,
        InteractionState::Disabled => TextRole::Disabled,
    }
}

pub(crate) fn resolve_icon_role_standalone(state: InteractionState) -> TextRole {
    match state {
        InteractionState::Disabled => TextRole::Disabled,
        _ => TextRole::Primary,
    }
}

/// Per-size icon dimension. The two smallest buttons (Compact 22,
/// Default 24) share the standard `icon_size` (16 dp); Toolbar / Large
/// / Hero scale up via dedicated tokens so a 50 dp button doesn't
/// carry a tiny 16 dp glyph.
fn resolve_icon_size(size: IconButtonSize) -> f32 {
    use crate::styles::recipe_icon_button_style as icon_dims;
    match size {
        IconButtonSize::Compact | IconButtonSize::Default => icon_dims::ICON_BUTTON_ICON_SIZE,
        IconButtonSize::Toolbar => icon_dims::ICON_BUTTON_ICON_SIZE_TOOLBAR,
        IconButtonSize::Large => icon_dims::ICON_BUTTON_ICON_SIZE_LARGE,
        IconButtonSize::Hero => icon_dims::ICON_BUTTON_ICON_SIZE_HERO,
    }
}

// ── Widget trait ─────────────────────────────────────────────────────────────

impl bastyde_core::widget::Widget for IconButton {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let embedded = self.embedded;
        let icon_size = resolve_icon_size(self.size);
        let size = self.size;
        let self_id = ctx.self_id();

        // Forward the initial-enabled hint into the arena. After this
        // point the arena is the single source of truth — events,
        // focus, a11y, and the leaves' role-resolution all consult
        // `arena.is_enabled(self_id)` / `PaintContext::effective_enabled`.
        // We never carry an `InteractionState::Disabled` in the
        // interaction signal: that was the snapshot duality the
        // architecture refactor removed.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // Reactive view of "is this widget effectively enabled?",
        // factoring this node and every ancestor's `enabled_state`.
        // Used both to derive `is_disabled` for the style chrome and
        // to flip the cursor between Pointer (enabled) / Default
        // (disabled).
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Interaction signal — caller-supplied via `share_interaction`
        // when set (so a wrapping widget's chrome can mirror the icon's
        // color), otherwise allocated locally. Seeded to Idle; the
        // arena's enabled-state is consulted separately.
        let interaction = match self.shared_interaction.take() {
            Some(shared) => shared,
            None => ctx.signal(InteractionState::Idle),
        };
        self.interaction = interaction.clone();

        // Register toggled signal for repaint + a11y refresh if present.
        // AccessibilityOnly pushes a fresh set_toggled() into the a11y
        // tree on every flip without forcing a relayout.
        if let Some(ref toggled) = self.toggled {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            toggled.bind_to(self_id, registry, BindingLevel::RepaintOnly);
            toggled.bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        }

        // Register the popover-open signal so AT picks up `set_expanded`
        // flips when a wrapping `PopoverIconButton` toggles the popover.
        // No relayout — AccessibilityOnly is enough.
        if let Some(ref expanded) = self.expanded_signal {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            expanded.bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        }

        // Icon color: a caller-supplied override wins over the auto
        // cascade. The override replaces ALL states (idle / hover /
        // press / focus / disabled) — chrome that uses this opts out
        // of interaction-driven color feedback in exchange for matching
        // a host's enforced text role.
        let icon_color: bastyde_core::color_prop::ColorProp =
            if let Some(ref over) = self.icon_role_override {
                over.clone()
            } else if embedded {
                interaction.map(|s| resolve_icon_role_embedded(*s)).into()
            } else {
                interaction.map(|s| resolve_icon_role_standalone(*s)).into()
            };

        // Build the icon content. Icon-swap toggle (eye / eye-off) only
        // applies when a `toggled_icon` was provided via
        // `toggle_with_icon`; surface-tint-only toggle keeps the same
        // glyph throughout.
        let icon_content_id = if let (Some(toggled), Some(_)) =
            (self.toggled.as_ref(), self.toggled_icon.as_ref())
        {
            let toggled_index = toggled.map(|v| if *v { 1 } else { 0 });
            let primary_icon =
                std::mem::replace(&mut self.icon, IconWidget::from_path(Path::new(), 0.0))
                    .icon_size(icon_size)
                    .bind_color(icon_color.clone());
            let alt_icon = self
                .toggled_icon
                .take()
                .expect("toggled_icon checked above")
                .icon_size(icon_size)
                .bind_color(icon_color);
            ctx.add(
                Switcher::new(toggled_index)
                    .child(primary_icon)
                    .child(alt_icon),
            )
        } else {
            let icon = std::mem::replace(&mut self.icon, IconWidget::from_path(Path::new(), 0.0))
                .icon_size(icon_size)
                .bind_color(icon_color);
            ctx.add(icon)
        };

        // Delegate background + border + size to the active style.
        // The four boolean signals derive from `interaction`; `is_on` is
        // populated only when a `toggled` signal is bound (drives the
        // bistate `Selected` background mode).
        let style: SharedIconButtonStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.icon_button.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeIconButtonStyle));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_focused = interaction.map(|s| matches!(s, InteractionState::Focused));
        // `is_disabled` derives from the arena's effective enabled
        // state — NOT from the interaction signal (which never
        // carries Disabled anymore). Style chrome uses this to pick
        // its disabled-background role.
        let is_disabled = effective_enabled.map(|on| !*on);
        let cfg = IconButtonStyleConfig {
            icon: icon_content_id,
            is_pressed,
            is_hovered,
            is_focused,
            is_disabled,
            is_on: self.toggled.clone(),
            size,
        };
        let root_id = style.make_body(&cfg, ctx);

        // Tooltip — three mutually-exclusive setters; setters clear
        // the others so exactly one branch runs.
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
            let tooltip_widget = crate::tooltip::TooltipWidget::new(lit!(tooltip_text));
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
                // The framework gates pointer/key events on
                // `arena.is_enabled(self_id)` before dispatch, so a
                // disabled subtree never reaches this closure. No
                // need for a redundant `if !enabled { return; }`
                // guard — the snapshot it captured is gone.
                move |_pos, ctx: &mut EventContext| {
                    if let Some(ref toggled) = toggled_for_tap {
                        toggled.set(!toggled.get());
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
                // Framework gates this on `arena.is_enabled()`; no
                // need for an inline `&& enabled` check.
                move |action: bastyde_core::accesskit::Action,
                      ctx: &mut EventContext|
                      -> EventResponse {
                    if action == bastyde_core::accesskit::Action::Click {
                        if let Some(ref toggled) = toggled_for_access {
                            toggled.set(!toggled.get());
                        }
                        if let Some(ref act) = *action_for_access {
                            act(ctx);
                        }
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            // The focus walker skips disabled subtrees on its own
            // (see `find_focusable_at_or_above`), so we don't AND
            // with enabled here. The static `self.focusable` flag
            // is the caller's intent (e.g. close-button-inside-tab
            // wants `false`).
            .focusable(self.focusable)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Button);
        // The accessible name is sourced from whichever tooltip flavor
        // is configured. Plain text is used directly; for a rich
        // tooltip we use the inline content's body text when
        // available; for a composite tooltip the caller is expected
        // to provide an explicit `.access_label(...)` via the
        // accessibility-overrides API.
        let rich_name: Option<String> = self.rich_tooltip_source.as_ref().and_then(|s| match s {
            crate::tooltip::RichTooltipSource::Content(c) => Some(c.text.resolve_now()),
            crate::tooltip::RichTooltipSource::Key(_) => None,
        });
        debug_assert!(
            self.tooltip_text.is_some()
                || rich_name.is_some()
                || self.rich_tooltip_source.is_some()
                || self.composite_tooltip_content.is_some(),
            "IconButton: expected a tooltip (used as the accessible name). \
             Use .tooltip(tr!(…)) or a predefined constructor like IconButton::clear(). \
             For rich/composite tooltips, also pair with `.access_label(...)`."
        );
        if let Some(ref text) = self.tooltip_text {
            builder.set_name(text.as_str());
        } else if let Some(ref text) = rich_name {
            builder.set_name(text.as_str());
        } else {
            builder.set_name("Button");
        }
        // Note: `set_disabled()` is now driven by the framework's
        // accessibility walker from `arena.is_enabled(self_id)`. The
        // composite no longer needs to mirror it — the snapshot path
        // was redundant with the arena and broke under reactive
        // `enabled_when(id, signal)` flips.
        if let Some(ref toggled) = self.toggled {
            builder.set_toggled(toggled.get());
        }
        // ARIA disclosure pattern: a button that opens a popup
        // declares `has_popup` and, when the wrapper tracks it,
        // `expanded`. Both are opt-in — regular icon buttons stay
        // silent on these properties.
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
        self.root_child_id.into_iter().collect()
    }
}

// ── Overridable icon set ────────────────────────────────────────────────────
//
// Default icons are real SVGs embedded via `include_str!` and parsed once
// via `LazyLock`. The `res!` macro cannot be used here because it emits
// `::bastyde::` paths and bastyde-widgets sits below bastyde in the
// dependency graph.
//
// Applications can replace the default icon set globally at startup via
// `BuiltInIcons::set_global(custom_set)`.

/// Icon factory set for predefined built-in buttons.
///
/// Each field is a function pointer that creates an [`IconWidget`].
/// The default implementation uses SVG icons embedded in bastyde-widgets.
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
    pub bell: fn() -> IconWidget,
    pub eye: fn() -> IconWidget,
    pub eye_off: fn() -> IconWidget,
}

static GLOBAL_ICONS: OnceLock<BuiltInIcons> = OnceLock::new();

impl BuiltInIcons {
    /// Return the default icon set (SVGs embedded in bastyde-widgets).
    pub fn defaults() -> Self {
        Self {
            browse: default_browse_icon,
            expand: default_expand_icon,
            search: default_search_icon,
            copy: default_copy_icon,
            clear: default_clear_icon,
            add: default_add_icon,
            bell: default_bell_icon,
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

fn default_bell_icon() -> IconWidget {
    IconWidget::from_svg(include_str!("../resources/icons/builtin-bell.svg"))
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
    use bastyde_core::signal::Signal;
    use bastyde_core::widget_tree::WidgetTree;

    /// Reactive enabled-state via `ctx.enabled_when(btn_id, signal)`
    /// must dim the IconButton's icon when the signal flips to false —
    /// regression test for the FormatToolbar bug where the
    /// table-operation buttons stayed full-color despite the framework
    /// correctly gating their events. Until the enabled-state
    /// architecture refactor (this commit and the framework commit
    /// that preceded it) the icon's color was driven by an internal
    /// `InteractionState::Disabled` seed captured at build time,
    /// which never updated from a later `enabled_when` call.
    #[test]
    fn icon_button_enabled_when_signal_dims_icon_color() {
        let theme = bastyde_core::presets::intui::light();
        let mut tree = WidgetTree::new();
        tree.set_theme(theme.clone());

        let is_enabled = Signal::new(true);
        let btn_id = tree.add(IconButton::new(IconWidget::checkmark(24.0)).tooltip(lit!("test")));
        tree.enabled_when(btn_id, is_enabled.clone());
        tree.layout(bastyde_canvas::SizeProposal::exact(100.0, 40.0));

        let primary = theme.colors.text_primary.to_array();
        let disabled = theme.colors.text_disabled.to_array();
        let frame = tree.render();
        assert!(
            frame.paths.iter().any(|p| p.color == primary),
            "enabled IconButton must render its icon at text_primary; \
             got path colors = {:?}",
            frame.paths.iter().map(|p| p.color).collect::<Vec<_>>()
        );

        is_enabled.set(false);
        tree.layout(bastyde_canvas::SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert!(
            frame.paths.iter().any(|p| p.color == disabled),
            "after flipping enabled→false, IconButton's icon must \
             render at text_disabled (the FormatToolbar bug as a unit \
             test); got path colors = {:?}",
            frame.paths.iter().map(|p| p.color).collect::<Vec<_>>()
        );

        is_enabled.set(true);
        tree.layout(bastyde_canvas::SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert!(
            frame.paths.iter().any(|p| p.color == primary),
            "flipping enabled→true must restore the primary color"
        );
    }

    /// Static `.enabled(false)` builder must route through the arena —
    /// after migration, the snapshot path is gone and the only correct
    /// behavior is that `arena.is_enabled(btn_id)` returns false.
    #[test]
    fn icon_button_static_enabled_false_propagates_to_arena() {
        let mut tree = WidgetTree::new();
        let btn_id = tree.add(
            IconButton::new(IconWidget::checkmark(24.0))
                .tooltip(lit!("test"))
                .enabled(false),
        );
        tree.layout(bastyde_canvas::SizeProposal::exact(100.0, 40.0));
        assert!(
            !tree.is_enabled(btn_id),
            "IconButton::enabled(false) must propagate to the arena"
        );
    }
}
