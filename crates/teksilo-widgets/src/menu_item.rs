// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MenuItem — a single command row in a menu or context menu.
//!
//! Each item consists of an optional leading icon, a label, an optional
//! trailing shortcut label, and an activation closure. `MenuItem` is
//! non-generic: actions are type-erased closures identical to `Button`'s
//! `on_activate_fn` model. Submenus are declared with `MenuItem::submenu`
//! — the factory builds the nested `MenuList` lazily at hover time.
//!
//! Every item operates in one of three **modes** selected by builder methods:
//!
//! | Builder | AT Role | Leading glyph |
//! |---|---|---|
//! | (default) | `Role::MenuItem` | icon or blank |
//! | `.checked(signal)` | `Role::MenuItemCheckBox` | checkmark / blank |
//! | `.check_state(signal)` | `Role::MenuItemCheckBox` | check / dash / blank |
//! | `.reflect_checked(signal)` | `Role::MenuItemCheckBox` | checkmark (read-only) |
//! | `.radio(value, selected)` | `Role::MenuItemRadio` | filled dot / blank |
//!
//! Check and radio modes are mutually exclusive with `.icon(...)` — the
//! Windows convention reserves the leading slot for state glyphs on
//! checkable items; a `debug_assert!` fires when both are set.
//!
//! ## An icon that keeps its own colour
//!
//! `.icon(...)` recolours whatever it is handed with the row's text role, so the
//! glyph follows hover, press and disabled alongside the label. That is right for
//! an icon that says the same thing as the label, and wrong for one whose colour
//! *is* the content — a tag's swatch, a status light, a colour a person chose.
//!
//! `.icon_keeps_color()` leaves it alone. Two costs come with it: the icon no
//! longer follows the highlight (on a style whose highlighted row is a solid
//! accent fill, it has to carry its own contrast against that fill), and a
//! *literal* colour does not dim in a disabled row — `ColorProp::Static` and
//! `Bound` ignore the enabled state, while every role variant substitutes its
//! disabled counterpart. An icon that should dim wants a role, and then it does
//! not want this at all.
//!
//! ```rust
//! # use teksilo_widgets::{MenuItem, primitives::IconWidget};
//! # use teksilo_canvas::{Path, Point};
//! # use teksilo_i18n::lit;
//! # use teksilo_tokens::Color;
//! let swatch = IconWidget::from_path(Path::circle(Point::new(5.0, 5.0), 4.5), 10.0)
//!     .color(Color::from_hex("#e91e63"));
//! let _w = MenuItem::new(lit!("Characters"))
//!     .icon(swatch)
//!     .icon_keeps_color();
//! ```
//!
//! **Mnemonic markers** use the in-string `&` convention (`&Save` →
//! underline 'S' when Alt is held; `&&` → literal `&`). The enclosing
//! `MenuList` wires bare-letter in-menu activation automatically.
//!
//! ```rust
//! # use teksilo_widgets::MenuItem;
//! # use teksilo_i18n::lit;
//! # use teksilo_core::Intent;
//! let _w = MenuItem::new(lit!("&Save"))
//!     .on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.save")));
//! ```

use std::rc::Rc;
use std::time::Duration;
use teksilo_data::CheckState;
use teksilo_i18n::lit;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use teksilo_core::shortcut::KeyStroke;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{MenuItemStyleConfig, SharedMenuItemStyle};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{TextRole, TextStyleRole};

use crate::keystroke_format::format_keystroke;
use crate::primitives::{HStack, IconWidget, Spacer, Switcher, TextWidget};
use teksilo_i18n::LocalizedString;

mod menu_label;
mod mnemonic;
mod widget_impl;
pub(crate) use menu_label::MenuLabel;
pub(crate) use mnemonic::{ParsedMnemonic, parse_mnemonic};

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

/// Glyph size for the check / dash / radio-dot rendered in the leading
/// slot, whose width is the active style's
/// [`MenuItemMetrics::icon_column_width`](teksilo_core::styles::MenuItemMetrics).
/// 12dp matches the existing `chevron_right(12.0)` used for submenu
/// triggers, and fits inside the smallest column any shipped preset asks
/// for (macOS's 14dp).
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
    /// Reflect-only: the checkmark mirrors `state`, but activation does **not**
    /// write it — the bound value's truth lives elsewhere (a model / method) and
    /// the item's `on_activate`/intent is solely responsible for changing it.
    /// The classic "View ▸ Sidebar / Full Screen" pattern, where the check
    /// follows layout state the menu doesn't own. Renders identically to
    /// `TwoState`; differs only in that clicking has no built-in toggle.
    Reflect(Prop<bool>),
}

/// A single command row in a `MenuList` or context menu.
///
/// See the module documentation for the full mode table, mnemonic syntax, and
/// submenu construction pattern.
pub struct MenuItem {
    label: LocalizedString,
    icon: Option<IconWidget>,
    /// Leave the icon's own colour alone instead of tinting it with the row's
    /// text role — see [`MenuItem::icon_keeps_color`].
    icon_keeps_color: bool,
    shortcut_label: Option<String>,
    /// A trailing *descriptive* phrase — not an accelerator. Unlike
    /// `shortcut_label` this stays a [`LocalizedString`], so it re-resolves
    /// on a live locale change, and it is announced as the item's
    /// accessible *description* rather than its keyboard shortcut.
    trailing_hint: Option<LocalizedString>,
    /// Optional shortcut id. When set and `shortcut_label` is not, the
    /// rendered trailing label is pulled from the tree's
    /// [`ShortcutRegistry`](teksilo_core::shortcut::ShortcutRegistry) and
    /// tracks user rebindings automatically — reactively, via a *per-id*
    /// signal (see `shortcut_signal`), so a rebind refreshes the chord in
    /// place instead of rebuilding the whole item.
    shortcut_id: Option<&'static str>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn teksilo_core::widget::Widget>>,
    action: Option<CommandFactory>,
    /// Enabled-state (static or signal-bound); forwarded to the arena at build
    /// time via `enabled_when`, so a bound signal disables/enables the item
    /// reactively (paint and AT follow). Cursor stays `Pointer` — see
    /// the cursor assignment in `build` for why it is not derived from this.
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
    /// "This submenu has been wanted at least once" — the reveal gate for its
    /// deferred content. Distinct from `submenu_open`, which is the disclosure
    /// state AT reads and the chevron follows: the hover path schedules a
    /// *delayed* overlay and must have the content built before the delay
    /// matures, while the item is not yet open.
    submenu_needed: Signal<bool>,
    /// Live per-id handle to the effective primary keystroke for
    /// `shortcut_id`, obtained in `build()` from
    /// [`BuildContext::effective_shortcut_signal`]. The trailing label
    /// binds it (leaf-level, so a rebind repaints in place and the item
    /// is never rebuilt on registry churn), and `accessibility()` reads
    /// it live so screen readers announce the current chord. `None` for
    /// items with a manual `shortcut_label` or no shortcut at all.
    shortcut_signal: Option<Signal<Option<KeyStroke>>>,
    /// Per-call override for the label's text style (font, size, weight).
    /// `None` ⇒ the default `TextStyleRole::Body`.
    label_style: Option<teksilo_core::color_prop::TextStyleProp>,
    /// Per-call override for the label text color. `None` ⇒ the
    /// interaction/enabled-derived cascade (hover / disabled). Setting
    /// this replaces the cascade (loses the hover/disabled tint), so use
    /// it only when a host enforces a fixed text role.
    text_role_override: Option<teksilo_core::color_prop::ColorProp>,
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
    /// Create a plain menu item with the given label and no action yet.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            icon: None,
            icon_keeps_color: false,
            shortcut_label: None,
            trailing_hint: None,
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
            submenu_needed: Signal::new(false),
            shortcut_signal: None,
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

    /// Keep the icon's **own** colour rather than tinting it with the row's.
    ///
    /// A menu icon normally says the same thing as the label beside it, so it takes
    /// the row's text role and follows it through hover, press and disabled — which
    /// is why [`icon`](Self::icon) recolours whatever it is handed. Some icons are
    /// not that. A tag's swatch, a status light, a colour a person chose: there the
    /// colour *is* the content, and tinting it to the menu's foreground deletes the
    /// only thing the icon was there to say.
    ///
    /// Opt-in, because the default is right for nearly every row, and keeping a
    /// colour has two costs the caller takes on:
    ///
    /// * **It does not follow the highlight.** On a style whose highlighted row is a
    ///   solid accent fill (the macOS recipe), the icon has to carry its own contrast
    ///   against that fill as well as against the menu's surface.
    /// * **It does not dim when the row is disabled** — if it is a literal colour.
    ///   That is [`ColorProp`](teksilo_core::ColorProp)'s own rule everywhere, not a
    ///   special case here: `Static` and `Bound` ignore the enabled state, while every
    ///   role variant substitutes its disabled counterpart. An icon that should dim
    ///   should be given a role instead, and then it does not need this at all.
    ///
    /// Ignored in the check and radio modes, which draw an indicator glyph of the
    /// framework's own rather than the caller's icon.
    pub fn icon_keeps_color(mut self) -> Self {
        self.icon_keeps_color = true;
        self
    }

    /// Set a trailing shortcut label (e.g., "Ctrl+X"). Shortcut labels are
    /// typically not translated (they're the key combination literal), so
    /// this accepts a plain string.
    pub fn shortcut_label(mut self, label: impl Into<String>) -> Self {
        self.shortcut_label = Some(label.into());
        self
    }

    /// Set a trailing *descriptive* hint (e.g. "inside", "after parent") —
    /// a secondary phrase explaining what the item will do, rendered in the
    /// same trailing slot as an accelerator but semantically unrelated to one.
    ///
    /// Prefer this over [`shortcut_label`](Self::shortcut_label) for any
    /// trailing text that is not a key combination. It differs in two ways
    /// that matter:
    ///
    /// * it takes a [`LocalizedString`], so a `tr!(...)` hint re-resolves on
    ///   a live locale change instead of being frozen at build time;
    /// * it is announced as the item's accessible **description**, not as
    ///   `keyboard_shortcut` — a screen reader would otherwise read the
    ///   phrase out as if it were a chord to press.
    ///
    /// Independent of the accelerator: an item may carry both, in which case
    /// the chord renders first and the hint follows it.
    pub fn trailing_hint(mut self, text: impl Into<LocalizedString>) -> Self {
        self.trailing_hint = Some(text.into());
        self
    }

    /// Bind the trailing shortcut label to a registered
    /// [`Shortcut`](teksilo_core::shortcut::Shortcut) by its stable id.
    /// At build time the effective primary keystroke is rendered;
    /// rebinds performed through
    /// [`ShortcutRegistry`](teksilo_core::shortcut::ShortcutRegistry)
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
    /// enables/disables the item reactively (paint and AT follow), so
    /// `MenuItem::new(...).enabled(can_save_signal)` greys out live without a
    /// rebuild. Cursor is always `Pointer` (see `build`); disabled items are
    /// gated by the arena before hover runs, so a `NotAllowed` cursor cannot
    /// be applied from a build-time snapshot of this prop either.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `MenuItemStyle` for just this MenuItem instance.
    pub fn style(mut self, style: impl teksilo_core::styles::MenuItemStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override the label's text style (font, size, weight). Accepts a
    /// `TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default
    /// (unset) is `TextStyleRole::Body`.
    pub fn text_style(mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// Override the label text color. Accepts `Color`, a role, or a
    /// `Signal` of either. Default (unset) is the interaction/enabled
    /// cascade; setting this replaces that cascade (the hover / disabled
    /// tint no longer applies), so reserve it for chrome that enforces a
    /// fixed text role.
    pub fn text_role(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self {
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
            icon_keeps_color: false,
            shortcut_label: None,
            trailing_hint: None,
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
            submenu_needed: Signal::new(false),
            shortcut_signal: None,
            label_style: None,
            text_role_override: None,
            style_override: None,
            root_child_id: None,
            submenu_content_id: None,
            parsed_mnemonic: None,
            safe_triangle: None,
        }
    }

    /// Set a custom submenu open delay (default: 400 ms, IntelliJ's value;
    /// see `DEFAULT_SUBMENU_OPEN_DELAY` for what that delay also buys).
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
    /// Mutually exclusive with [`check_state`](Self::check_state)
    /// and [`radio`](Self::radio) — last call wins.
    pub fn checked(mut self, state: Signal<bool>) -> Self {
        self.mode = MenuItemMode::Check(CheckKind::TwoState(state));
        self
    }

    /// Render `Role::MenuItemCheckBox` whose checkmark **reflects** `state`
    /// read-only: activation does NOT write the signal — the truth lives
    /// elsewhere (a model / method), and this item's `on_activate`/intent is
    /// responsible for the change, after which `state` updates the checkmark
    /// reactively. Use for "View ▸ Sidebar / Full Screen"-style commands that
    /// mirror externally-owned state (e.g. `DockingModel::dock_open_signal`),
    /// where two-way [`checked`](Self::checked) would fight the model.
    ///
    /// Mutually exclusive with the other check / radio binders — last call wins.
    pub fn reflect_checked(mut self, state: impl Into<Prop<bool>>) -> Self {
        self.mode = MenuItemMode::Check(CheckKind::Reflect(state.into()));
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
    /// Mutually exclusive with [`checked`](Self::checked)
    /// and [`radio`](Self::radio) — last call wins.
    pub fn check_state(mut self, state: Signal<CheckState>) -> Self {
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
    /// Mutually exclusive with [`checked`](Self::checked)
    /// and [`check_state`](Self::check_state) — last call
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
            MenuItemMode::Check(CheckKind::Reflect(_)) => "Check(Reflect)",
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

#[cfg(test)]
mod tests;
