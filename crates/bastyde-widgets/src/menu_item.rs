// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MenuItem widget — a single item in a menu or context menu.
//!
//! Non-generic, closure-based command erasure (same pattern as Button).
//! Supports icons, shortcut labels, disabled state, and submenu triggers.
//!
//! ### Modes
//!
//! Every item is one of three modes — `Plain` (the default), `Check`
//! (two-state or tri-state checkmark), or `Radio` (one of N
//! mutually-exclusive options) — selected through builder methods:
//!
//! - `.bind_checked(Signal<bool>)`     → `Role::MenuItemCheckBox`, checkmark glyph
//! - `.bind_check_state(Signal<CheckState>)` → tri-state (`Checked` / `Indeterminate` / `Unchecked`)
//! - `.radio(value, selected)`          → `Role::MenuItemRadio`, filled-dot glyph
//!
//! The three setters are mutually exclusive (last call wins). They are
//! also mutually exclusive with `.icon(...)` — by Windows convention a
//! checkable item carries no icon; if both are set the check/radio
//! glyph wins and a debug assertion fires.

use bastyde_data::CheckState;
use bastyde_i18n::lit;
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::styles::{MenuItemStyleConfig, SharedMenuItemStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::keystroke_format::format_keystroke;
use crate::primitives::{HStack, IconWidget, Spacer, Switcher, TextWidget};
use bastyde_i18n::LocalizedString;

mod menu_label;
mod mnemonic;
mod safe_triangle;
pub(crate) use menu_label::MenuLabel;
pub(crate) use mnemonic::{ParsedMnemonic, parse_mnemonic};
pub(crate) use safe_triangle::point_in_safe_triangle;

/// Type-erased command factory. Stored as `Rc` (not `Box`) so the closure
/// can be cloned and shared — in particular with SplitButton, which reads
/// the action out of a MenuItem via `MenuItem::action()` and re-fires it
/// from its main region without disturbing the MenuItem's own use of it.
type CommandFactory = Rc<dyn Fn(&mut EventContext)>;

/// Interaction state for a menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItemState {
    Idle,
    Hovered,
    Pressed,
    Disabled,
}

/// Default delay before a submenu opens on hover (400 ms — IntelliJ's value).
/// This delay also provides diagonal movement tolerance: when the pointer
/// crosses other menu items while moving toward a submenu, those items
/// don't open their submenus because the delay hasn't elapsed yet. 400 ms
/// is long enough that a casual sweep past a submenu trigger doesn't
/// accidentally open it, but short enough that a deliberate hover feels
/// responsive.
const DEFAULT_SUBMENU_OPEN_DELAY: Duration = Duration::from_millis(400);
const DEFAULT_SUBMENU_CLOSE_DELAY: Duration = Duration::from_millis(150);

/// Glyph size for the check / dash / radio-dot rendered in the
/// 16dp `MENU_ICON_COLUMN_WIDTH` leading slot. 12dp matches the
/// existing `chevron_right(12.0)` used for submenu triggers.
const MENU_INDICATOR_GLYPH_SIZE: f32 = 12.0;

/// Internal selection mode of a `MenuItem`. `Plain` is the default
/// and produces `Role::MenuItem`. `Check` swaps the leading-slot
/// icon for a checkmark (binary) or check/dash/spacer (tri-state)
/// and emits `Role::MenuItemCheckBox`. `Radio` swaps the leading
/// slot for a filled dot when the radio group's `selected` signal
/// matches `value` and emits `Role::MenuItemRadio`.
///
/// The state signals are kept here unboxed so `accessibility()`
/// can read the current value cheaply via `Signal::get()`.
enum MenuItemMode {
    Plain,
    Check(CheckKind),
    Radio {
        value: usize,
        selected: Signal<usize>,
    },
}

/// Internal dual-mode for checkable items — mirrors `Checkbox`'s
/// internal `CheckKind` exactly so MenuItem and Checkbox behave
/// identically when they share the same `Signal<bool>` /
/// `Signal<CheckState>`.
enum CheckKind {
    TwoState(Signal<bool>),
    TriState(Signal<CheckState>),
}

/// A single menu item: icon + label + shortcut label + optional submenu chevron.
pub struct MenuItem {
    label: LocalizedString,
    icon: Option<IconWidget>,
    shortcut_label: Option<String>,
    /// Optional shortcut id. When set and `shortcut_label` is not,
    /// the rendered trailing label is pulled from the tree's
    /// [`ShortcutRegistry`](bastyde_core::shortcut::ShortcutRegistry) and
    /// tracks user rebindings automatically (the build registers the
    /// registry's version signal as a Relayout binding on self).
    shortcut_id: Option<&'static str>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn bastyde_core::widget::Widget>>,
    action: Option<CommandFactory>,
    /// Enabled-state (static or signal-bound); forwarded to the arena at build
    /// time via `enabled_when`, so a bound signal disables/enables the item
    /// reactively (paint, cursor, AT all follow).
    enabled: Prop<bool>,
    /// Plain / Check / Radio — see [`MenuItemMode`].
    mode: MenuItemMode,
    /// Sibling ids for radio-group AT announcement. Set by
    /// [`MenuList::build`](crate::menu_list::MenuList::build) on
    /// every radio-mode item that shares a `Signal<usize>` with
    /// other items in the same list, via
    /// `set_radio_group_ids(...)`. Used in `accessibility()` to
    /// emit `push_to_radio_group(sibling_id)` so AT announces
    /// "Theme Dark, 2 of 3". Empty for non-radio items and for
    /// solitary radio items.
    radio_group_ids: Option<Rc<std::cell::RefCell<Vec<WidgetId>>>>,
    submenu_factory: Option<Box<dyn Fn() -> Box<dyn Widget>>>,
    submenu_open_delay: Duration,
    // Build state
    interaction: Signal<MenuItemState>,
    /// Whether this item's submenu overlay is currently visible.
    /// Flipped to `true` by every open path (tap, hover, Enter,
    /// ArrowRight) and flipped back to `false` by the overlay
    /// manager's `on_dismiss` callback — regardless of dismiss
    /// path. `accessibility()` reads this for `set_expanded`.
    /// Only meaningful when `submenu_factory.is_some()`.
    submenu_open: Signal<bool>,
    /// Shortcut text after resolving `shortcut_label` / `shortcut_id`.
    /// Captured during `build()` and read by `accessibility()` for
    /// `set_keyboard_shortcut`, so screen readers announce the chord
    /// that the visual trailing label shows.
    resolved_shortcut: Option<String>,
    /// Per-call override for the label's text style (font, size, weight).
    /// `None` ⇒ the default `TextStyleRole::Body`.
    label_style: Option<bastyde_core::color_prop::TextStyleProp>,
    /// Per-call override for the label text color. `None` ⇒ the
    /// interaction/enabled-derived cascade (hover / disabled). Setting
    /// this replaces the cascade (loses the hover/disabled tint), so use
    /// it only when a host enforces a fixed text role.
    text_role_override: Option<bastyde_core::color_prop::ColorProp>,
    /// Per-call style override. When `None`, falls back to the
    /// theme-wide slot (`theme.style_slots.menu_item`) and finally to
    /// the IntUI default `RecipeMenuItemStyle`.
    style_override: Option<SharedMenuItemStyle>,
    root_child_id: Option<WidgetId>,
    submenu_content_id: Option<WidgetId>,
    /// Parsed mnemonic from the label, captured during `build()`. The
    /// enclosing [`MenuList`](crate::menu_list::MenuList) reads this
    /// to wire in-menu mnemonic activation (bare-letter Alt
    /// shortcut) and the keyboard-driven type-ahead.
    parsed_mnemonic: Option<ParsedMnemonic>,
    /// Shared safe-triangle state owned by the enclosing
    /// [`MenuList`](crate::menu_list::MenuList). Submenu triggers
    /// write to it on hover-enter (stamp the anchor); sibling items
    /// read it before firing their hover-switch so a diagonal
    /// pointer trajectory toward the open submenu doesn't steal
    /// focus. `None` for items that haven't been adopted by a
    /// MenuList (e.g. solo menu items in tests).
    safe_triangle: Option<crate::menu_list::SharedSafeTriangleState>,
}

impl MenuItem {
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            icon: None,
            shortcut_label: None,
            shortcut_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            action: None,
            enabled: Prop::Static(true),
            mode: MenuItemMode::Plain,
            radio_group_ids: None,
            submenu_factory: None,
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            submenu_open: Signal::new(false),
            resolved_shortcut: None,
            label_style: None,
            text_role_override: None,
            style_override: None,
            root_child_id: None,
            submenu_content_id: None,
            parsed_mnemonic: None,
            safe_triangle: None,
        }
    }

    /// Closure invoked on activation.
    /// Note: shortcut label auto-lookup is not available with this variant
    /// since there is no typed command to look up.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
        self
    }

    /// Read the item's display label. Exposed so SplitButton (and any other
    /// compound widget that embeds a MenuItem) can mirror the label in its
    /// own chrome.
    pub fn label(&self) -> String {
        self.label.resolve_now()
    }

    /// Like [`label`](Self::label) but returns the unresolved
    /// [`LocalizedString`], so embedders can mirror the label *reactively*
    /// (re-resolving on a locale switch) instead of freezing a snapshot.
    pub fn label_localized(&self) -> LocalizedString {
        self.label.clone()
    }

    /// Clone out a shared handle to the activation closure. Returns `None`
    /// when this MenuItem has no action (e.g. it's a submenu trigger). The
    /// returned `Rc` aliases MenuItem's own internal handle — invoking it
    /// has the same effect as the user clicking this menu item (minus the
    /// overlay dismissal that the tap handler also performs).
    pub fn action(&self) -> Option<Rc<dyn Fn(&mut EventContext)>> {
        self.action.clone()
    }

    /// Set a leading icon.
    pub fn icon(mut self, icon: IconWidget) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set a trailing shortcut label (e.g., "Ctrl+X"). Shortcut labels are
    /// typically not translated (they're the key combination literal), so
    /// this accepts a plain string.
    pub fn shortcut_label(mut self, label: impl Into<String>) -> Self {
        self.shortcut_label = Some(label.into());
        self
    }

    /// Bind the trailing shortcut label to a registered
    /// [`Shortcut`](bastyde_core::shortcut::Shortcut) by its stable id.
    /// At build time the effective primary keystroke is rendered;
    /// rebinds performed through
    /// [`ShortcutRegistry`](bastyde_core::shortcut::ShortcutRegistry)
    /// rebuild this item automatically via the registry's version
    /// signal.
    ///
    /// A manual [`shortcut_label`](Self::shortcut_label) takes
    /// precedence when both are set.
    pub fn for_shortcut(mut self, id: &'static str) -> Self {
        self.shortcut_id = Some(id);
        self
    }

    /// Set the enabled state — static or signal-bound. A bound `Signal<bool>`
    /// enables/disables the item reactively (paint, cursor, and AT all follow),
    /// so `MenuItem::new(...).enabled(can_save_signal)` greys out live without a
    /// rebuild.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `MenuItemStyle` for just this MenuItem instance.
    pub fn style(mut self, style: impl bastyde_core::styles::MenuItemStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override the label's text style (font, size, weight). Accepts a
    /// `TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
    /// (unset) is `TextStyleRole::Body`.
    pub fn text_style(mut self, style: impl Into<bastyde_core::color_prop::TextStyleProp>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// Override the label text color. Accepts `Color`, a role, or a
    /// `Signal` of either. Default (unset) is the interaction/enabled
    /// cascade; setting this replaces that cascade (the hover / disabled
    /// tint no longer applies), so reserve it for chrome that enforces a
    /// fixed text role.
    pub fn text_role(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.text_role_override = Some(color.into());
        self
    }

    /// Attach a tooltip that appears after a hover delay, same mechanism
    /// as [`Button::tooltip`](crate::button::Button::tooltip).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip
    /// registry. Body text supports inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`); the entry's shortcut
    /// and long-form "more" fields are rendered automatically.
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
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Create a submenu trigger item. The factory is invoked during `build()` to
    /// pre-create the submenu content (typically a `MenuList`), which is kept
    /// dormant until the hover delay elapses.
    pub fn submenu(
        label: impl Into<LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            icon: None,
            shortcut_label: None,
            shortcut_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            action: None,
            enabled: Prop::Static(true),
            mode: MenuItemMode::Plain,
            radio_group_ids: None,
            submenu_factory: Some(Box::new(factory)),
            submenu_open_delay: DEFAULT_SUBMENU_OPEN_DELAY,
            interaction: Signal::new(MenuItemState::Idle),
            submenu_open: Signal::new(false),
            resolved_shortcut: None,
            label_style: None,
            text_role_override: None,
            style_override: None,
            root_child_id: None,
            submenu_content_id: None,
            parsed_mnemonic: None,
            safe_triangle: None,
        }
    }

    /// Set a custom submenu open delay (default: 200ms).
    pub fn submenu_delay(mut self, delay: Duration) -> Self {
        self.submenu_open_delay = delay;
        self
    }

    /// Whether this is a submenu trigger.
    pub fn is_submenu(&self) -> bool {
        self.submenu_factory.is_some()
    }

    /// Bind this item to a two-state `Signal<bool>`. The item renders
    /// `Role::MenuItemCheckBox`; activation flips the signal. By
    /// Windows convention, the leading icon slot becomes a checkmark
    /// when the signal is `true`, blank otherwise.
    ///
    /// Mutually exclusive with [`bind_check_state`](Self::bind_check_state)
    /// and [`radio`](Self::radio) — last call wins.
    pub fn bind_checked(mut self, state: Signal<bool>) -> Self {
        self.mode = MenuItemMode::Check(CheckKind::TwoState(state));
        self
    }

    /// Bind this item to a tri-state `Signal<CheckState>`. The item
    /// renders `Role::MenuItemCheckBox`; activation cycles
    /// `Unchecked` ↔ `Checked` (per Windows / [`Checkbox`](crate::checkbox::Checkbox)
    /// convention: `Indeterminate` is reserved for external sources
    /// like `TreeCheckedModel`; clicking from `Indeterminate`
    /// promotes to `Checked`).
    ///
    /// The leading-slot glyph is `checkmark` for `Checked`, `dash`
    /// for `Indeterminate`, blank for `Unchecked` — matching the
    /// Windows mixed-state convention.
    ///
    /// Mutually exclusive with [`bind_checked`](Self::bind_checked)
    /// and [`radio`](Self::radio) — last call wins.
    pub fn bind_check_state(mut self, state: Signal<CheckState>) -> Self {
        self.mode = MenuItemMode::Check(CheckKind::TriState(state));
        self
    }

    /// Bind this item to a radio group via a shared `Signal<usize>`.
    /// Activation writes `value` into `selected`; all radio items
    /// sharing the same `selected` signal observe the change and
    /// update their leading-slot dot accordingly. The item renders
    /// `Role::MenuItemRadio`.
    ///
    /// For "2 of 3"-style AT announcement, the enclosing
    /// [`MenuList`](crate::menu_list::MenuList) groups radio items
    /// by selection-signal identity and emits `push_to_radio_group`
    /// relationships automatically — no app-side wiring required.
    ///
    /// Mutually exclusive with [`bind_checked`](Self::bind_checked)
    /// and [`bind_check_state`](Self::bind_check_state) — last call
    /// wins.
    pub fn radio(mut self, value: usize, selected: Signal<usize>) -> Self {
        self.mode = MenuItemMode::Radio { value, selected };
        self
    }

    /// Internal accessor for [`MenuList::build`](crate::menu_list::MenuList::build)
    /// — read whether this item is a radio with a given group-id
    /// (the `Rc`-identity of its `selected` signal).
    pub(crate) fn radio_selection_handle(&self) -> Option<(usize, Signal<usize>)> {
        match &self.mode {
            MenuItemMode::Radio { value, selected } => Some((*value, selected.clone())),
            _ => None,
        }
    }

    /// Internal setter for [`MenuList::build`](crate::menu_list::MenuList::build)
    /// — install the sibling id buffer so `accessibility()` can
    /// announce "2 of N" via `push_to_radio_group`.
    pub(crate) fn set_radio_group_ids(&mut self, ids: Rc<std::cell::RefCell<Vec<WidgetId>>>) {
        self.radio_group_ids = Some(ids);
    }

    /// Read the parsed mnemonic for this item's label. Populated
    /// inside `build()`. Returns `None` for items that haven't been
    /// built yet, or whose label contains no un-escaped `&` marker.
    ///
    /// Used by [`MenuList`](crate::menu_list::MenuList) to wire
    /// in-menu mnemonic activation (bare-letter activation of the
    /// matching item) — the lookup runs on every `KeyDown` so a
    /// fresh `parse_mnemonic` per keypress would be wasteful.
    pub(crate) fn mnemonic(&self) -> Option<&ParsedMnemonic> {
        self.parsed_mnemonic.as_ref()
    }

    /// Pre-parse the label so that
    /// [`MenuList::build`](crate::menu_list::MenuList::build) can
    /// read this item's mnemonic *before* the item is committed to
    /// the arena. Idempotent — calls after the first one are no-ops.
    pub(crate) fn ensure_mnemonic_parsed(&mut self) {
        if self.parsed_mnemonic.is_none() {
            self.parsed_mnemonic = Some(parse_mnemonic(&self.label.resolve_now()));
        }
    }

    /// Install the enclosing
    /// [`MenuList`](crate::menu_list::MenuList)'s shared
    /// safe-triangle state. Called by `MenuList::build` for every
    /// item before it reaches the arena. The handle lets:
    ///
    /// - a submenu trigger stamp the anchor (pointer position at
    ///   submenu-open time) and the open submenu's content id;
    /// - a sibling item read the anchor + submenu id on hover and
    ///   skip its dismiss / open call when the cursor is currently
    ///   inside the safe triangle.
    pub(crate) fn set_safe_triangle_state(
        &mut self,
        state: crate::menu_list::SharedSafeTriangleState,
    ) {
        self.safe_triangle = Some(state);
    }
}

impl std::fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = match &self.mode {
            MenuItemMode::Plain => "Plain",
            MenuItemMode::Check(CheckKind::TwoState(_)) => "Check(TwoState)",
            MenuItemMode::Check(CheckKind::TriState(_)) => "Check(TriState)",
            MenuItemMode::Radio { .. } => "Radio",
        };
        f.debug_struct("MenuItem")
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .field("is_submenu", &self.submenu_factory.is_some())
            .field("mode", &mode)
            .finish()
    }
}

fn resolve_text_role(state: MenuItemState) -> TextRole {
    match state {
        MenuItemState::Disabled => TextRole::Disabled,
        _ => TextRole::Primary,
    }
}

fn resolve_shortcut_role(state: MenuItemState) -> TextRole {
    match state {
        MenuItemState::Disabled => TextRole::Disabled,
        _ => TextRole::TooltipShortcut,
    }
}

impl Widget for MenuItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_menu_item_style as menu;
        let self_id = ctx.self_id();
        // Forward enabled (static or signal-bound) into the arena. A bound
        // signal makes enable/disable reactive — the framework's
        // effective_enabled drives paint / cursor / AT.
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Interaction seeds to Idle; the framework's effective_enabled
        // drives the Disabled visual via the recipe and through the
        // leaves' role substitution.
        let interaction = ctx.signal(MenuItemState::Idle);
        self.interaction = interaction.clone();

        // Combine interaction + effective_enabled so `text_role`
        // resolves to Disabled when disabled. Keeps the icon and label
        // muted on hover-while-disabled too (defense in depth — the
        // leaves' `ColorProp::resolve(theme, ctx.effective_enabled)`
        // would substitute Disabled anyway).
        let text_role = interaction.zip(&effective_enabled).map(|(s, on)| {
            if !*on {
                TextRole::Disabled
            } else {
                resolve_text_role(*s)
            }
        });

        // Build the three slots fed to the active `MenuItemStyle`.
        // The style decides the row layout (and chrome); the widget
        // owns the slot contents.
        //
        // Leading: icon column — always reserved at `icon_column_width`,
        // even when the item has no icon, so labels line up vertically
        // between icon'd and icon-less items.
        //
        // For Check / Radio modes the slot becomes a `Switcher`
        // driven by the bound state signal, swapping between the
        // glyph and a `Spacer`. The framework's binding system
        // re-paints the leaf when the signal flips — no rebuild.
        //
        // Icon + Check/Radio are mutually exclusive (Windows
        // convention). If both are set, `debug_assert!` fires and the
        // check/radio mode wins in release.
        let leading = {
            let icon_child_id = match &self.mode {
                MenuItemMode::Plain => {
                    if let Some(icon) = self.icon.take() {
                        ctx.add(icon.bind_color(text_role.clone()))
                    } else {
                        ctx.add(Spacer::new())
                    }
                }
                MenuItemMode::Check(CheckKind::TwoState(s)) => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with .bind_checked()"
                    );
                    self.icon = None;
                    // 0 = checkmark, 1 = spacer.
                    let idx = s.map(|b| if *b { 0_usize } else { 1 });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::checkmark(MENU_INDICATOR_GLYPH_SIZE)
                                    .bind_color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
                MenuItemMode::Check(CheckKind::TriState(s)) => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with .bind_check_state()"
                    );
                    self.icon = None;
                    // 0 = checkmark (Checked), 1 = dash (Indeterminate), 2 = spacer (Unchecked).
                    let idx = s.map(|cs| match cs {
                        CheckState::Checked => 0_usize,
                        CheckState::Indeterminate => 1,
                        CheckState::Unchecked => 2,
                    });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::checkmark(MENU_INDICATOR_GLYPH_SIZE)
                                    .bind_color(text_role.clone()),
                            )
                            .child(
                                IconWidget::dash(MENU_INDICATOR_GLYPH_SIZE)
                                    .bind_color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
                MenuItemMode::Radio { value, selected } => {
                    debug_assert!(
                        self.icon.is_none(),
                        "MenuItem: .icon() is mutually exclusive with .radio()"
                    );
                    self.icon = None;
                    let v = *value;
                    // 0 = filled dot (selected == value), 1 = spacer.
                    let idx = selected.map(move |sel| if *sel == v { 0_usize } else { 1 });
                    ctx.add(
                        Switcher::new(idx)
                            .child(
                                IconWidget::radio_dot(MENU_INDICATOR_GLYPH_SIZE)
                                    .bind_color(text_role.clone()),
                            )
                            .child(Spacer::new()),
                    )
                }
            };
            ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(menu::MENU_ICON_COLUMN_WIDTH)
                    .bind_height(menu::MENU_ICON_COLUMN_WIDTH)
                    .child_id(icon_child_id),
            )
        };

        // Label. Uses `MenuLabel` (not `TextWidget`) so a single `&`
        // in the label is parsed as a mnemonic marker — stripped from
        // the visible text and underlined when `alt_down` is held.
        // The parsed form is cached so the enclosing MenuList can
        // read it for type-ahead and in-menu mnemonic activation.
        let parsed = parse_mnemonic(&self.label.resolve_now());
        self.parsed_mnemonic = Some(parsed.clone());
        let alt_down = ctx
            .window()
            .map(|w| w.alt_down().clone())
            .unwrap_or_else(|| Signal::new(false));
        let label_source: bastyde_core::signal::Prop<String> = self.label.clone().into();
        let label_color: bastyde_core::color_prop::ColorProp = self
            .text_role_override
            .clone()
            .unwrap_or_else(|| text_role.clone().into());
        let label_style: bastyde_core::color_prop::TextStyleProp = self
            .label_style
            .clone()
            .unwrap_or_else(|| TextStyleRole::Body.into());
        let label = ctx.add(MenuLabel::new(
            label_source,
            alt_down,
            label_color,
            label_style,
        ));

        // Resolve the trailing shortcut text — manual label wins;
        // otherwise pull from the registry by id. Bind the registry's
        // version signal so a rebind triggers a Rebuild on this widget.
        let resolved_shortcut = self.shortcut_label.clone().or_else(|| {
            self.shortcut_id.and_then(|id| {
                ctx.effective_shortcut(id)
                    .and_then(|eff| eff.primary.map(format_keystroke))
            })
        });
        self.resolved_shortcut = resolved_shortcut.clone();
        if self.shortcut_id.is_some() {
            ctx.shortcut_version().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::Rebuild,
            );
        }

        // Pre-create submenu content if this is a submenu trigger. Kept
        // dormant until hover opens the overlay.
        let submenu_content_id = if let Some(factory) = self.submenu_factory.take() {
            let submenu_widget = factory();
            let id = ctx.add_boxed(submenu_widget);
            ctx.set_dormant(id);
            self.submenu_content_id = Some(id);
            Some(id)
        } else {
            None
        };

        // Trailing slot — combines (optional shortcut + fixed gap +
        // optional chevron column). The chevron column is always
        // reserved at `item_padding_horizontal` so submenu and
        // regular items share the same trailing edge.
        let trailing = {
            let mut trailing_row = HStack::new().spacing(0.0);
            if let Some(ref shortcut_text) = resolved_shortcut {
                let shortcut_role = interaction.map(|s| resolve_shortcut_role(*s));
                let shortcut = TextWidget::new(lit!(shortcut_text))
                    .style(TextStyleRole::Body)
                    .bind_color(shortcut_role)
                    .single_line()
                    .a11y_hidden();
                trailing_row = trailing_row.child(shortcut);
            }
            // Chevron column. Always reserved (Spacer when no submenu)
            // so the row's right edge sits at exactly the same X
            // regardless of submenu-ness.
            //
            // The submenu opens on the trailing edge
            // (`OverlayPlacement::TrailingEdge`) — right under LTR, left
            // under RTL — so the chevron must point the same way. Drive a
            // `Switcher` off the locale's direction signal so it flips
            // live on a locale change (0 = LTR → ▶, 1 = RTL → ◀). With no
            // i18n manager installed there's no RTL, so fall back to the
            // plain right-pointing chevron.
            let chevron_child_id = if submenu_content_id.is_some() {
                match bastyde_i18n::current_direction() {
                    Some(direction) => {
                        let idx = direction.map(|d| {
                            if *d == bastyde_core::environment::LayoutDirection::RightToLeft {
                                1_usize
                            } else {
                                0
                            }
                        });
                        ctx.add(
                            Switcher::new(idx)
                                .child(
                                    IconWidget::chevron_right(12.0).bind_color(text_role.clone()),
                                )
                                .child(
                                    IconWidget::chevron_left(12.0).bind_color(text_role.clone()),
                                ),
                        )
                    }
                    None => ctx.add(IconWidget::chevron_right(12.0).bind_color(text_role.clone())),
                }
            } else {
                ctx.add(Spacer::new())
            };
            let chevron_column = ctx.add(
                crate::primitives::FixedSize::new()
                    .bind_width(menu::MENU_ITEM_PADDING_HORIZONTAL)
                    .bind_height(menu::MENU_ICON_COLUMN_WIDTH)
                    .child_id(chevron_child_id),
            );
            trailing_row = trailing_row.add_child(chevron_column);
            ctx.add(trailing_row)
        };

        // Derive the four boolean signals the trait wants.
        let is_hovered = interaction.map(|s| matches!(s, MenuItemState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, MenuItemState::Pressed));
        let is_disabled = interaction.map(|s| matches!(s, MenuItemState::Disabled));

        // MenuItem doesn't track focus/highlight separately today —
        // hovered already covers the keyboard-arrow case in the
        // existing dispatcher. Wire is_focused to a constant false
        // signal; is_highlighted reads the same as is_hovered for
        // the IntUI default (the recipe `or`s them anyway).
        let is_focused = ctx.signal(false);
        let is_highlighted = is_hovered.clone();

        let style: SharedMenuItemStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.menu_item.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeMenuItemStyle));
        let cfg = MenuItemStyleConfig {
            label,
            leading: Some(leading),
            trailing: Some(trailing),
            is_hovered,
            is_pressed,
            is_focused,
            is_disabled,
            is_highlighted,
        };
        let root_id = style.make_body(&cfg, ctx);

        self.root_child_id = Some(root_id);

        // Attach tooltip if configured. The three setters
        // (`tooltip`, `rich_tooltip*`, `composite_tooltip`) are
        // mutually exclusive — setters clear the other two so at most
        // one branch runs.
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

        // --- Handlers ---
        let action = self.action.take();
        let action_rc: std::rc::Rc<Option<CommandFactory>> = std::rc::Rc::new(action);
        let action_for_key = action_rc.clone();

        // Shared closure that performs the bound-state mutation on
        // activation — flips the check signal, cycles the tristate
        // signal, or writes the radio value. Captured by both the
        // tap and key handlers so click and Enter/Space have
        // identical semantics. `None` for `Plain` and for submenu
        // triggers (which never carry a bound state).
        type ActivateFn = std::rc::Rc<dyn Fn()>;
        let mode_activate: Option<ActivateFn> = match &self.mode {
            MenuItemMode::Plain => None,
            MenuItemMode::Check(CheckKind::TwoState(s)) => {
                let s = s.clone();
                Some(std::rc::Rc::new(move || s.set(!s.get())))
            }
            MenuItemMode::Check(CheckKind::TriState(s)) => {
                let s = s.clone();
                // Click toggles Unchecked <-> Checked. Indeterminate
                // (driven by external aggregation models) promotes
                // to Checked. Mirrors `Checkbox::toggle`.
                Some(std::rc::Rc::new(move || match s.get() {
                    CheckState::Unchecked => s.set(CheckState::Checked),
                    CheckState::Checked => s.set(CheckState::Unchecked),
                    CheckState::Indeterminate => s.set(CheckState::Checked),
                }))
            }
            MenuItemMode::Radio { value, selected } => {
                let v = *value;
                let selected = selected.clone();
                Some(std::rc::Rc::new(move || selected.set(v)))
            }
        };
        let mode_activate_for_tap = mode_activate.clone();
        let mode_activate_for_key = mode_activate.clone();

        let int_hover = interaction.clone();
        let self_id = ctx.self_id();
        let is_submenu = submenu_content_id.is_some();

        // Shared dismiss callback for the submenu overlay. Flipped
        // to `false` by the overlay manager when the submenu is
        // dismissed by any path (pointer leave, cascade, Escape,
        // click outside) so `accessibility()` can report accurate
        // `set_expanded` without needing to track the overlay state
        // from inside the MenuItem's own handlers.
        //
        // Also clears the safe-triangle anchor when the overlay
        // actually closes — keeping the anchor alive across
        // hover-leave (so sibling hovers heading toward the
        // submenu are properly gated) means we MUST clear it here
        // once the submenu is finally gone.
        let submenu_open_signal = self.submenu_open.clone();
        let submenu_content_id_for_dismiss = submenu_content_id;
        let safe_triangle_for_dismiss = self.safe_triangle.clone();
        let submenu_dismiss_callback: bastyde_core::overlay::OverlayDismissCallback = {
            let open = submenu_open_signal.clone();
            std::rc::Rc::new(move || {
                open.set(false);
                if let (Some(sub_id), Some(state_rc)) = (
                    submenu_content_id_for_dismiss,
                    safe_triangle_for_dismiss.as_ref(),
                ) {
                    let mut state = state_rc.borrow_mut();
                    if state.submenu_content_id == Some(sub_id) {
                        state.submenu_content_id = None;
                        state.anchor = None;
                    }
                }
            })
        };

        let mut handler_set = HandlerSet::new();

        if is_submenu {
            // --- Submenu trigger: timer-based delayed open ---
            // On hover enter: request a delayed overlay via the widget tree's
            // timer system (like tooltips). On hover leave: cancel the pending
            // request. The widget tree checks pending overlays during layout()
            // and opens them once the delay elapses.
            let sub_id = submenu_content_id.expect("is_submenu implies submenu_content_id is Some");
            let open_delay = self.submenu_open_delay;

            let open_for_tap = submenu_open_signal.clone();
            let dismiss_for_tap = submenu_dismiss_callback.clone();
            let open_for_hover = submenu_open_signal.clone();
            let dismiss_for_hover = submenu_dismiss_callback.clone();
            // Capture the safe-triangle shared state so we can stamp
            // / clear the anchor on submenu open / close.
            let safe_triangle_open = self.safe_triangle.clone();
            let safe_triangle_close = self.safe_triangle.clone();
            // Framework gates events on `arena.is_enabled(self_id)`.
            handler_set = handler_set
                .on_tap({
                    move |_pos, ctx: &mut EventContext| {
                        // Click on submenu trigger opens it immediately
                        ctx.dismiss_child_overlays_except(sub_id);
                        ctx.activate(sub_id);
                        open_for_tap.set(true);
                        ctx.show_overlay(OverlayRequest {
                            content_id: sub_id,
                            anchor: self_id,
                            placement: OverlayPlacement::TrailingEdge,
                            dismiss: DismissBehavior::PointerLeave {
                                delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                            },
                            layer: OverlayLayer::InTree,
                            parent_overlay: None,
                            on_dismiss: Some(dismiss_for_tap.clone()),
                            fade_duration: None,
                        });
                        ctx.request_focus(sub_id);
                    }
                })
                .on_hover({
                    let int_hover = int_hover.clone();
                    move |entered: bool, ctx: &mut EventContext| {
                        if entered {
                            int_hover.set(MenuItemState::Hovered);
                            ctx.dismiss_child_overlays_except(sub_id);
                            open_for_hover.set(true);
                            // Stamp the safe-triangle anchor so sibling
                            // hover-switches can suppress themselves
                            // while the cursor is travelling toward
                            // the open submenu. We use the current
                            // cursor position; if unavailable, the
                            // gate falls back to "no apex" (always
                            // false → no suppression).
                            if let Some(state_rc) = safe_triangle_open.as_ref() {
                                let mut state = state_rc.borrow_mut();
                                state.submenu_content_id = Some(sub_id);
                                state.anchor = ctx.tree_pointer_position();
                            }
                            ctx.show_overlay_after_with_focus(
                                OverlayRequest {
                                    content_id: sub_id,
                                    anchor: self_id,
                                    placement: OverlayPlacement::TrailingEdge,
                                    dismiss: DismissBehavior::PointerLeave {
                                        delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                    },
                                    layer: OverlayLayer::InTree,
                                    parent_overlay: None,
                                    on_dismiss: Some(dismiss_for_hover.clone()),
                                    fade_duration: None,
                                },
                                open_delay,
                                sub_id,
                            );
                        } else {
                            int_hover.set(MenuItemState::Idle);
                            ctx.cancel_delayed_overlay(sub_id);
                            // If the overlay was still pending (delay
                            // not yet elapsed), its dismiss callback
                            // will never fire — we must reset the
                            // open flag ourselves. Idempotent if the
                            // overlay already showed: the framework
                            // dismiss callback will also set it false
                            // when the PointerLeave behavior tears
                            // the overlay down shortly afterward.
                            open_for_hover.set(false);
                            // Clear the safe-triangle anchor ONLY when
                            // the submenu never actually opened (the
                            // 400 ms delay was cancelled while still
                            // pending). When the overlay IS open, we
                            // leave the anchor in place — sibling
                            // hover handlers consult it during the
                            // user's diagonal travel toward the
                            // submenu, and the dismiss callback
                            // installed above clears it the moment
                            // the overlay actually closes. Clearing
                            // on every hover-leave would defeat the
                            // entire safe-triangle gate, because the
                            // trigger's hover-leave fires *before* a
                            // sibling's hover-enter.
                            if let Some(state_rc) = safe_triangle_close.as_ref()
                                && ctx.overlay_bounds_for_content(sub_id).is_none()
                            {
                                let mut state = state_rc.borrow_mut();
                                if state.submenu_content_id == Some(sub_id) {
                                    state.submenu_content_id = None;
                                    state.anchor = None;
                                }
                            }
                        }
                    }
                });
        } else {
            // --- Regular menu item: tap to activate ---
            let action_for_tap = action_rc.clone();
            let int_tap = interaction.clone();

            handler_set = handler_set
                .on_tap({
                    move |_pos, ctx: &mut EventContext| {
                        int_tap.set(MenuItemState::Pressed);
                        // 1. Flip the bound state first (Check / Radio),
                        //    so the user-supplied action sees the
                        //    post-activation value.
                        if let Some(ref activate) = mode_activate_for_tap {
                            activate();
                        }
                        // 2. Invoke the user action if any.
                        if let Some(ref action) = *action_for_tap {
                            action(ctx);
                        }
                        // 3. Dismiss the chain when EITHER an action
                        //    fired OR a mode flip happened. Plain items
                        //    without an action used to no-op the click;
                        //    Check/Radio items without an action still
                        //    dismiss because the visible state changed.
                        if action_for_tap.is_some() || mode_activate_for_tap.is_some() {
                            ctx.dismiss_self_overlay_chain();
                        }
                        // Reset to Idle after dispatching — the
                        // overlay dismissal swallows the trailing
                        // PointerUp that would normally clear Pressed,
                        // and the dormant content widgets keep their
                        // last-painted state. Without this the
                        // previously-clicked item reads as Pressed
                        // (highlighted) the next time the menu opens,
                        // until a hover transition overwrites it.
                        int_tap.set(MenuItemState::Idle);
                    }
                })
                .on_hover({
                    let safe_triangle_sibling = self.safe_triangle.clone();
                    move |entered: bool, ctx: &mut EventContext| {
                        if entered {
                            // Safe-triangle gate: if another submenu is
                            // currently open AND the cursor is inside
                            // the triangle anchored at the
                            // submenu-open pointer position with its
                            // base on the open submenu's near edge,
                            // skip the dismiss — the user is en route
                            // to the submenu and we don't want to
                            // close it out from under them.
                            let suppress = safe_triangle_sibling
                                .as_ref()
                                .and_then(|state_rc| {
                                    let state = state_rc.borrow();
                                    let sub_content_id = state.submenu_content_id?;
                                    let anchor = state.anchor?;
                                    let pointer = ctx.tree_pointer_position()?;
                                    let bounds = ctx.overlay_bounds_for_content(sub_content_id)?;
                                    Some(point_in_safe_triangle(pointer, anchor, bounds))
                                })
                                .unwrap_or(false);
                            if !suppress {
                                ctx.dismiss_child_overlays();
                            }
                            int_hover.set(MenuItemState::Hovered);
                        } else {
                            int_hover.set(MenuItemState::Idle);
                        }
                    }
                });
        }

        // Keyboard handler shared by both submenu and regular items
        handler_set = handler_set.on_key({
            let interaction = interaction.clone();
            let sub_id = submenu_content_id;
            let open_for_key = submenu_open_signal.clone();
            let dismiss_for_key = submenu_dismiss_callback.clone();
            move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                // The "open submenu / go deeper" key is inline-forward:
                // ArrowRight under LTR, ArrowLeft under RTL (submenus open
                // on the trailing edge, which mirrors). The inline-back
                // key (ArrowLeft under LTR, ArrowRight under RTL) is left
                // to bubble / to the framework's nested-overlay dismissal.
                let open_submenu_key = if ctx.is_rtl() {
                    Key::ArrowLeft
                } else {
                    Key::ArrowRight
                };
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::Enter | Key::Space,
                        ..
                    } => {
                        // Mirror the tap activation order: bound-state
                        // mutation first, then user action, then chain
                        // dismissal. Submenu triggers fall through to
                        // the existing open path (they never carry a
                        // bound mode signal).
                        if let Some(ref activate) = mode_activate_for_key {
                            activate();
                        }
                        if let Some(ref action) = *action_for_key {
                            action(ctx);
                            ctx.dismiss_self_overlay_chain();
                        } else if mode_activate_for_key.is_some() {
                            // Check/Radio with no user action — still dismiss.
                            ctx.dismiss_self_overlay_chain();
                        } else if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.activate(sub_id);
                            open_for_key.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::PointerLeave {
                                    delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                },
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(dismiss_for_key.clone()),
                                fade_duration: None,
                            });
                            ctx.request_focus(sub_id);
                        }
                        interaction.set(MenuItemState::Pressed);
                        EventResponse::Handled
                    }
                    // Inline-forward arrow opens submenu (ignored on
                    // regular items). RTL-flipped via `open_submenu_key`.
                    WidgetEvent::KeyDown { key, .. } if *key == open_submenu_key => {
                        if let Some(sub_id) = sub_id {
                            ctx.dismiss_child_overlays_except(sub_id);
                            ctx.activate(sub_id);
                            open_for_key.set(true);
                            ctx.show_overlay(OverlayRequest {
                                content_id: sub_id,
                                anchor: self_id,
                                placement: OverlayPlacement::TrailingEdge,
                                dismiss: DismissBehavior::PointerLeave {
                                    delay: DEFAULT_SUBMENU_CLOSE_DELAY,
                                },
                                layer: OverlayLayer::InTree,
                                parent_overlay: None,
                                on_dismiss: Some(dismiss_for_key.clone()),
                                fade_duration: None,
                            });
                            ctx.request_focus(sub_id);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                    _ => EventResponse::Ignored,
                }
            }
        });

        // Cursor: NotAllowed when effectively disabled (the original
        // intent), Pointer otherwise. Sourced from the arena via the
        // reactive effective_enabled signal so it reacts to
        // `enabled_when` flips.
        handler_set = handler_set.cursor(if effective_enabled.get() {
            CursorIcon::Pointer
        } else {
            CursorIcon::NotAllowed
        });

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => {
                let size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                // Claim the full proposed width when the parent offers one.
                // This is what makes menu items stretch to the popup width:
                // MenuList sizes its VStack to the widest item, then the
                // VStack proposes that width to each child. Without this
                // line, each MenuItem would report only its own content
                // width and the row's internal Spacer would have no room
                // to stretch — so the shortcut would sit flush against
                // the label instead of pushing to the trailing edge.
                let width = proposal.width.unwrap_or(size.width);
                Size::new(width, size.height)
            }
            None => proposal.resolve(120.0, 24.0),
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
        use bastyde_core::accesskit::{HasPopup, Role, Toggled};

        // Role reflects the mode: Plain → MenuItem, Check → MenuItemCheckBox,
        // Radio → MenuItemRadio. Submenu triggers always render as
        // Role::MenuItem (independent of mode — submenu+checkable is
        // not a supported combination).
        let role = match &self.mode {
            MenuItemMode::Plain => Role::MenuItem,
            MenuItemMode::Check(_) => Role::MenuItemCheckBox,
            MenuItemMode::Radio { .. } => Role::MenuItemRadio,
        };
        builder.set_role(role);
        // Use the stripped form for the announced name — screen readers
        // say "Save", not "ampersand-Save". Re-parse from a fresh
        // `resolve_now()` every walk rather than reading the build-time
        // `parsed_mnemonic` cache: a locale switch marks the tree dirty
        // (re-walking AT) but does NOT rebuild the item, so the cache
        // would otherwise announce the stale-locale name. The cached
        // mnemonic index is still used for the underline in `paint`.
        let parsed_name = parse_mnemonic(&self.label.resolve_now()).stripped;
        builder.set_name(parsed_name);

        // Toggle state for Check / Radio. Mirrors `Checkbox`:
        // `set_toggled(bool)` for binary, `inner_mut().set_toggled(Toggled::Mixed)`
        // for tri-state Indeterminate.
        match &self.mode {
            MenuItemMode::Plain => {}
            MenuItemMode::Check(CheckKind::TwoState(s)) => {
                builder.set_toggled(s.get());
            }
            MenuItemMode::Check(CheckKind::TriState(s)) => match s.get() {
                CheckState::Unchecked => builder.set_toggled(false),
                CheckState::Checked => builder.set_toggled(true),
                CheckState::Indeterminate => {
                    builder.inner_mut().set_toggled(Toggled::Mixed);
                }
            },
            MenuItemMode::Radio { value, selected } => {
                builder.set_toggled(selected.get() == *value);
            }
        }

        // Radio "2 of N" — push every group member id (including self)
        // into the AT node so assistive tech can announce
        // position-in-set. Only emitted for Radio items where the
        // enclosing MenuList wired up the group buffer. Mirrors
        // [`RadioButton::accessibility`] exactly.
        if let (MenuItemMode::Radio { .. }, Some(buf)) = (&self.mode, self.radio_group_ids.as_ref())
        {
            for sibling in buf.borrow().iter().copied() {
                builder.push_to_radio_group(bastyde_core::accessibility::widget_id_to_node_id(
                    sibling,
                ));
            }
        }

        // A submenu trigger exposes `has_popup(Menu)` so screen
        // readers announce the item as leading into a nested menu,
        // and `set_expanded` reflects whether the submenu is
        // currently visible. We check `submenu_content_id` rather
        // than `submenu_factory`: the factory is moved out during
        // `build()` via `take()`, so by the time the framework
        // queries accessibility the factory is always `None`,
        // but the content id survives.
        if self.submenu_content_id.is_some() {
            builder.set_has_popup(HasPopup::Menu);
            builder.set_expanded(self.submenu_open.get());
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(bastyde_core::accesskit::Action::Click);
        if let Some(ref shortcut) = self.resolved_shortcut {
            builder.set_keyboard_shortcut(shortcut.clone());
        }

        // Mnemonic — populates AccessKit's `access_key` field, which
        // Windows Narrator announces as "Access key: F" on items
        // carrying a single-character menu accelerator. Distinct from
        // the (rebindable) `keyboard_shortcut` field above, which
        // carries Ctrl+S-style accelerators. Empty / non-mnemonic
        // labels emit nothing.
        if let Some(parsed) = self.parsed_mnemonic.as_ref()
            && let Some(k) = parsed.key_lower
        {
            builder
                .inner_mut()
                .set_access_key(k.to_ascii_uppercase().to_string());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    /// Opt into reflection so [`MenuList::build`](crate::menu_list::MenuList::build)
    /// can downcast a pending boxed item and install its radio group
    /// buffer before the item is added to the arena.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_list::MenuList;
    use bastyde_core::accesskit::Role;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;

    fn tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
    }

    fn layout(tree: &mut WidgetTree) {
        tree.layout(SizeProposal::exact(400.0, 300.0));
    }

    // --- Role coverage ---

    #[test]
    fn plain_item_emits_role_menuitem() {
        let mut t = tree();
        let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("Save"))));
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
        let info = t.accessibility_node(item_id);
        assert_eq!(info.role(), Role::MenuItem);
        assert_eq!(info.name(), Some("Save"));
    }

    #[test]
    fn bind_checked_emits_role_menuitemcheckbox() {
        let checked = Signal::new(false);
        let mut t = tree();
        let list_id =
            t.add(MenuList::new().item(MenuItem::new(lit!("Word Wrap")).bind_checked(checked)));
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
        let info = t.accessibility_node(item_id);
        assert_eq!(info.role(), Role::MenuItemCheckBox);
        assert_eq!(info.name(), Some("Word Wrap"));
        assert!(!info.is_toggled());
    }

    #[test]
    fn bind_check_state_emits_role_menuitemcheckbox() {
        let state = Signal::new(CheckState::Unchecked);
        let mut t = tree();
        let list_id = t.add(
            MenuList::new().item(MenuItem::new(lit!("Show Inspector")).bind_check_state(state)),
        );
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
        let info = t.accessibility_node(item_id);
        assert_eq!(info.role(), Role::MenuItemCheckBox);
    }

    #[test]
    fn radio_emits_role_menuitemradio() {
        let sel = Signal::new(0_usize);
        let mut t = tree();
        let list_id =
            t.add(MenuList::new().item(MenuItem::new(lit!("Light")).radio(0, sel.clone())));
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemRadio);
        let info = t.accessibility_node(item_id);
        assert_eq!(info.role(), Role::MenuItemRadio);
        assert!(info.is_toggled());
    }

    // --- Activation: state mutation ---

    #[test]
    fn bind_checked_click_flips_signal() {
        let checked = Signal::new(false);
        let mut t = tree();
        let list_id = t.add(
            MenuList::new().item(MenuItem::new(lit!("Word Wrap")).bind_checked(checked.clone())),
        );
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
        t.click(item_id);
        assert!(checked.get());
        // Re-add and click again to confirm round-trip — but the menu
        // already dismissed; rebuild a fresh tree to test the second flip.
        let mut t2 = tree();
        let checked2 = Signal::new(true);
        let list_id2 = t2.add(
            MenuList::new().item(MenuItem::new(lit!("Word Wrap")).bind_checked(checked2.clone())),
        );
        layout(&mut t2);
        let item_id2 = first_descendant_with_role(&t2, list_id2, Role::MenuItemCheckBox);
        t2.click(item_id2);
        assert!(!checked2.get());
    }

    #[test]
    fn bind_check_state_click_cycles_two_states_not_three() {
        // Mirror Checkbox: click toggles Unchecked <-> Checked only.
        // Indeterminate (external) promotes to Checked on click.
        let state = Signal::new(CheckState::Unchecked);
        let mut t = tree();
        let list_id = t.add(
            MenuList::new().item(MenuItem::new(lit!("Inspector")).bind_check_state(state.clone())),
        );
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
        t.click(item_id);
        assert_eq!(state.get(), CheckState::Checked);

        let state2 = Signal::new(CheckState::Checked);
        let mut t2 = tree();
        let list_id2 = t2.add(
            MenuList::new().item(MenuItem::new(lit!("Inspector")).bind_check_state(state2.clone())),
        );
        layout(&mut t2);
        let item_id2 = first_descendant_with_role(&t2, list_id2, Role::MenuItemCheckBox);
        t2.click(item_id2);
        assert_eq!(state2.get(), CheckState::Unchecked);

        let state3 = Signal::new(CheckState::Indeterminate);
        let mut t3 = tree();
        let list_id3 = t3.add(
            MenuList::new().item(MenuItem::new(lit!("Inspector")).bind_check_state(state3.clone())),
        );
        layout(&mut t3);
        let item_id3 = first_descendant_with_role(&t3, list_id3, Role::MenuItemCheckBox);
        t3.click(item_id3);
        // Indeterminate -> Checked (promotion, not cycle to Unchecked).
        assert_eq!(state3.get(), CheckState::Checked);
    }

    #[test]
    fn radio_click_writes_value_to_shared_signal() {
        let sel = Signal::new(0_usize);
        let mut t = tree();
        let _list_id = t.add(
            MenuList::new()
                .item(MenuItem::new(lit!("Light")).radio(0, sel.clone()))
                .item(MenuItem::new(lit!("Dark")).radio(1, sel.clone()))
                .item(MenuItem::new(lit!("System")).radio(2, sel.clone())),
        );
        layout(&mut t);
        // Find the "Dark" item by label.
        let dark_id = t
            .find_by_label("Dark")
            .expect("Dark menu item should exist");
        t.click(dark_id);
        assert_eq!(sel.get(), 1);
    }

    #[test]
    fn bind_checked_space_keypress_flips_signal() {
        let checked = Signal::new(false);
        let mut t = tree();
        let list_id = t.add(
            MenuList::new().item(MenuItem::new(lit!("Word Wrap")).bind_checked(checked.clone())),
        );
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
        t.focus(item_id);
        t.press_key(Key::Space, Modifiers::NONE);
        assert!(checked.get());
    }

    #[test]
    fn radio_external_signal_change_reflects_in_at() {
        // The bound `Signal<usize>` is the source of truth; clicking is
        // only one path. An external write must flip every item's
        // is_toggled() the next time the AT walker reads it.
        let sel = Signal::new(0_usize);
        let mut t = tree();
        let list_id = t.add(
            MenuList::new()
                .item(MenuItem::new(lit!("Light")).radio(0, sel.clone()))
                .item(MenuItem::new(lit!("Dark")).radio(1, sel.clone())),
        );
        layout(&mut t);
        let light_id = t.find_by_label("Light").expect("Light exists");
        let dark_id = t.find_by_label("Dark").expect("Dark exists");

        assert!(t.accessibility_node(light_id).is_toggled());
        assert!(!t.accessibility_node(dark_id).is_toggled());

        sel.set(1);
        let _ = list_id;
        assert!(!t.accessibility_node(light_id).is_toggled());
        assert!(t.accessibility_node(dark_id).is_toggled());
    }

    // --- Reactive role state ---

    #[test]
    fn bind_checked_at_state_reflects_signal() {
        let checked = Signal::new(true);
        let mut t = tree();
        let list_id = t.add(
            MenuList::new().item(MenuItem::new(lit!("Word Wrap")).bind_checked(checked.clone())),
        );
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItemCheckBox);
        assert!(t.accessibility_node(item_id).is_toggled());
        checked.set(false);
        assert!(!t.accessibility_node(item_id).is_toggled());
    }

    // --- Mnemonic plumbing ---

    #[test]
    fn ampersand_stripped_from_at_name() {
        // The `&` marker is parsed out of the label so screen readers
        // don't announce "ampersand Save" — they announce "Save".
        let mut t = tree();
        let list_id = t.add(MenuList::new().item(MenuItem::new(lit!("&Save"))));
        layout(&mut t);
        let item_id = first_descendant_with_role(&t, list_id, Role::MenuItem);
        let info = t.accessibility_node(item_id);
        assert_eq!(info.name(), Some("Save"));
    }

    #[test]
    fn mnemonic_parsed_from_label_when_builder_returns() {
        // Build the item, drop it back to inspect — the mnemonic
        // accessor should reflect the parse.
        let mut mi = MenuItem::new(lit!("&File"));
        mi.ensure_mnemonic_parsed();
        let m = mi.mnemonic().expect("mnemonic exists");
        assert_eq!(m.stripped, "File");
        assert_eq!(m.key_lower, Some('f'));
    }

    // --- Plain item AT smoke ---

    // --- Helpers ---

    fn first_descendant_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> WidgetId {
        // BFS through the tree starting at `from`.
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(from);
        while let Some(id) = queue.pop_front() {
            if t.accessibility_node(id).role() == role {
                return id;
            }
            for child in t.children(id) {
                queue.push_back(child);
            }
        }
        panic!("no descendant of {from:?} has role {role:?}");
    }
}
