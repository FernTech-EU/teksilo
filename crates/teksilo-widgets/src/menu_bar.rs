// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MenuBar — a horizontal application menu bar with keyboard-driven dropdowns.
//!
//! `MenuBar` renders a row of labelled trigger buttons; activating one opens a
//! dropdown `MenuList` as an overlay. Menus can be added via the fluent
//! `.menu(label, factory)` API or built from a declarative `MenuModel`
//! (the single source of truth shared with the native macOS menu bar via
//! `from_model` + `native_on_macos`). Leading and trailing slots accept
//! arbitrary widget content (an app icon or a search field, for example).
//!
//! **Keyboard.** F10 and bare-Alt-tap focus the first trigger without opening
//! a menu; Alt+letter opens the menu whose label carries a matching mnemonic
//! marker (`&File` → Alt+F). On macOS the Alt+letter branch is suppressed
//! because the OS rewrites Option+letter for accented character composition —
//! F10 and bare-Alt-tap continue to work. Once a dropdown is open, ArrowLeft
//! and ArrowRight cycle between top-level menus, and Escape closes the active
//! one and returns focus to the trigger.
//!
//! **Hamburger / collapsible mode.** Call `.collapsible()` to let the bar
//! collapse to a single hamburger `IconButton` when its intrinsic width
//! exceeds the allotted space (`CollapsePolicy::Responsive`). `.collapse_policy(Always)`
//! forces the hamburger regardless of width.
//!
//! ## Accessibility
//!
//! The bar carries `Role::MenuBar`; each trigger is `Role::MenuItem` with
//! `set_has_popup(Menu)` and `set_expanded` tracking the open dropdown.
//! Mnemonic letters are announced via `set_access_key` for Windows Narrator.
//!
//! ```rust
//! # use teksilo_widgets::{MenuBar, MenuList, MenuItem};
//! # use teksilo_i18n::lit;
//! # use teksilo_core::Intent;
//! let _w = MenuBar::new()
//!     .menu(lit!("File"), || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new(lit!("New")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.new"))))
//!             .separator()
//!             .item(MenuItem::new(lit!("Quit")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.quit"))))
//!     ))
//!     .menu(lit!("Edit"), || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new(lit!("Cut")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.cut"))))
//!     ));
//! ```

mod trigger;
mod widget_impl;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::window::{
    MenubarAction, MenubarDispatcher, MenubarGuard, MenubarKeyEvent, MenubarReveal,
};
use teksilo_tokens::{SurfaceRole, TextStyleRole};

use crate::animations::Unroll;
use crate::icon_button::{IconButton, IconButtonSize};
use crate::menu_context::MenuContext;
use crate::menu_item::MenuLabel;
use crate::menu_item::ParsedMnemonic;
use crate::menu_item::parse_mnemonic;
use crate::primitives::{HStack, Padding, RectWidget, Spacer, ZStack};
use teksilo_i18n::LocalizedString;

/// Controls when a collapsible [`MenuBar`] switches from the full inline bar
/// to the hamburger `IconButton` representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollapsePolicy {
    /// Collapse to a hamburger only when the bar's intrinsic width
    /// exceeds the width it is allotted; otherwise show the full inline
    /// bar. Mirrors the responsive `Toolbar` overflow behaviour.
    #[default]
    Responsive,
    /// Always show the hamburger, regardless of available width. The
    /// "force hamburger" / compact mode.
    Always,
}

// ---------------------------------------------------------------------------
// MenuBarEntry — pending menu definition
// ---------------------------------------------------------------------------

struct MenuBarEntry {
    label: LocalizedString,
    factory: Box<dyn Fn() -> Box<dyn Widget>>,
}

// ---------------------------------------------------------------------------
// MenuBar — public widget
// ---------------------------------------------------------------------------

/// A horizontal application menu bar with labelled trigger buttons and dropdown menus.
///
/// Each top-level entry becomes a focusable trigger; activating it opens a
/// floating `MenuList` overlay. See the module documentation for the full
/// keyboard, mnemonic, and collapsible-mode details.
pub struct MenuBar {
    entries: Vec<MenuBarEntry>,
    /// Pending leading/trailing slot content (the standard by-value slot
    /// pattern, same as `Card` / `TextInput` / `StandardListItem`). Consumed
    /// on the first build into `leading_slot_ids` / `trailing_slot_ids`, which
    /// are re-attached on every later build. MenuBar is
    /// [`preserves_children_on_rebuild`], so the reconciling rebuild keeps the
    /// re-attached slot widgets alive — a stateful slot control (a search
    /// field, a focused button) survives a theme / locale / model-version
    /// rebuild with its state intact. The menu triggers, by contrast, are
    /// re-derived fresh each build (the model may have changed) and the
    /// reconcile reaps the superseded ones.
    ///
    /// [`preserves_children_on_rebuild`]: teksilo_core::widget::Widget::preserves_children_on_rebuild
    leading_slot: Vec<PendingChild>,
    trailing_slot: Vec<PendingChild>,
    /// Memoized slot widget ids — populated from the pending content on the
    /// first build, reused (re-attached) on every later build so the slot
    /// widgets keep their identity and state across rebuilds.
    leading_slot_ids: Vec<WidgetId>,
    trailing_slot_ids: Vec<WidgetId>,
    root_child_id: Option<WidgetId>,
    /// Window-state guard for the per-window menubar key dispatcher
    /// (F10, Alt+letter, bare-Alt-tap). Owned by the MenuBar so the
    /// slot is cleared on rebuild / unmount.
    menubar_guard: RefCell<Option<MenubarGuard>>,
    /// When `true` (the default), `build()` installs a
    /// [`MenubarDispatcher`] into the window-state slot so this
    /// MenuBar receives F10 / Alt+letter / Alt-tap routing. Set to
    /// `false` via [`MenuBar::no_dispatcher_install`] for showcase /
    /// demo MenuBars that share a window with a primary one — the
    /// window-state slot is single-occupancy and a second install
    /// `debug_assert!`s otherwise.
    install_dispatcher: bool,
    /// When `Some`, the bar can collapse to a hamburger `IconButton`.
    /// `None` (the default) is the classic always-inline MenuBar.
    collapse_policy: Option<CollapsePolicy>,
    /// `true` while collapsed (hamburger shown). Source of truth for
    /// the visibility bindings. Driven by the responsive decision in
    /// `place_children` (or pinned `true` for `CollapsePolicy::Always`).
    collapsed: Signal<bool>,
    /// `true` while the collapsed bar is shown as a floating overlay.
    revealed: Signal<bool>,
    /// Animated 0..1 reveal progress for the floating bar (0 = rolled up
    /// into the hamburger, 1 = fully unrolled). The overlay's deferred
    /// reveal/dismiss drives it; an [`Unroll`] wrapper binds the bar's
    /// width to it so the bar unrolls out of the hamburger on open and
    /// rolls back into it on close. Stays at `1.0` for the inline bar.
    reveal_progress: Signal<f32>,
    /// Idempotence guard for the responsive write (Toolbar pattern).
    last_collapsed: Cell<bool>,
    /// The bar root (ZStack) id, captured in `build()`. Used both as the
    /// inline content and as the floating-overlay content when collapsed.
    bar_id: Option<WidgetId>,
    /// The hamburger `IconButton` id, captured in `build()`.
    hamburger_id: Option<WidgetId>,
    /// Size variant applied to the collapsed-mode hamburger `IconButton`.
    /// Defaults to [`IconButtonSize::Default`] (matching a bare `IconButton`).
    hamburger_size: IconButtonSize,
    /// The declarative source model, when this bar was built via
    /// [`from_model`](Self::from_model). Drives the native menu mirror.
    model: Option<crate::menu::MenuModel>,
    /// macOS native-menu behaviour (mirror to / suppress in-window).
    native_mode: crate::menu::NativeMenuMode,
    /// RAII binding keeping the native menu's reactive observers alive while
    /// this bar is mounted.
    native_binding: RefCell<Option<crate::menu::native::NativeMenuBinding>>,
}

impl MenuBar {
    /// Create an empty menu bar with no menus, slots, or collapse policy.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            leading_slot: Vec::new(),
            trailing_slot: Vec::new(),
            leading_slot_ids: Vec::new(),
            trailing_slot_ids: Vec::new(),
            root_child_id: None,
            menubar_guard: RefCell::new(None),
            install_dispatcher: true,
            collapse_policy: None,
            collapsed: Signal::new(false),
            revealed: Signal::new(false),
            reveal_progress: Signal::new_animated(1.0),
            last_collapsed: Cell::new(false),
            bar_id: None,
            hamburger_id: None,
            hamburger_size: IconButtonSize::Default,
            model: None,
            native_mode: crate::menu::NativeMenuMode::Off,
            native_binding: RefCell::new(None),
        }
    }

    /// Build a menu bar from a declarative [`MenuModel`](crate::menu::MenuModel)
    /// — the single source of truth shared with the native OS menu bar. Each
    /// top-level menu in the model becomes an in-window dropdown; combine with
    /// [`native_on_macos`](Self::native_on_macos) to also mirror it into the
    /// macOS system menu bar.
    pub fn from_model(model: crate::menu::MenuModel) -> Self {
        let mut bar = Self::new();
        // Entries are derived from the model on every `build()` (see
        // `model_entries`), so runtime structural changes — `MenuModel::push_item`
        // / `remove` / `push_menu` — re-render the in-window bar too (the bar
        // binds `model.version()` at `Rebuild` level).
        bar.model = Some(model);
        bar
    }

    /// Derive the in-window menu entries from the model's top-level menus. Each
    /// `Submenu` node becomes a dropdown whose factory builds a `MenuList` from
    /// its children. `Standard` roles + bare items/separators at top level have
    /// no in-window representation.
    fn model_entries(model: &crate::menu::MenuModel) -> Vec<MenuBarEntry> {
        model
            .nodes()
            .iter()
            .filter_map(|node| match node {
                crate::menu::MenuNode::Submenu {
                    title, children, ..
                } => {
                    let children = children.clone();
                    Some(MenuBarEntry {
                        label: title.clone(),
                        factory: Box::new(move || {
                            Box::new(crate::menu::model::build_menu_list(&children))
                        }),
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Add an `HStack`'s worth of slot content to `row`, memoized.
    ///
    /// On the first build `pending` holds the by-value slot widgets: each is
    /// inserted once and its id captured in `cache`. On every later build the
    /// cached ids are re-attached unchanged — re-parenting the same slot
    /// widgets into the fresh row. Because MenuBar is
    /// `preserves_children_on_rebuild`, the reconciling rebuild keeps those
    /// re-homed widgets (and their state) alive while reaping the superseded
    /// menu triggers. Building each slot widget exactly once is what preserves
    /// a stateful slot control across rebuilds.
    fn add_slot(
        ctx: &mut BuildContext,
        mut row: HStack,
        pending: &mut Vec<PendingChild>,
        cache: &mut Vec<WidgetId>,
    ) -> HStack {
        if cache.is_empty() && !pending.is_empty() {
            *cache = pending
                .drain(..)
                .map(|p| match p {
                    PendingChild::Id(id) => id,
                    PendingChild::Deferred(w) => ctx.add_boxed(w),
                })
                .collect();
        }
        for &id in cache.iter() {
            row = row.add_child(id);
        }
        row
    }

    /// Choose how this bar behaves on macOS, where the convention is a global
    /// menu bar at the top of the screen. Requires the bar to have been built
    /// with [`from_model`](Self::from_model) and the app to have called
    /// `install_native_menu()`. No effect on other platforms (the in-window bar
    /// renders there regardless).
    pub fn native_on_macos(mut self, mode: crate::menu::NativeMenuMode) -> Self {
        self.native_mode = mode;
        self
    }

    /// Enable the optional **hamburger** representation. When there
    /// isn't room for the full inline bar, it collapses to a single
    /// hamburger (☰) [`IconButton`]; activating it (click, `Alt`+
    /// mnemonic, `F10`, or bare-`Alt`-tap) reveals the full bar as a
    /// floating overlay over content. Clicking outside the bar or
    /// pressing `Escape` hides it again.
    ///
    /// Uses [`CollapsePolicy::Responsive`]. Observe the collapsed state
    /// via [`is_collapsed`](Self::is_collapsed), or bind your own signal
    /// with [`collapsed_signal`](Self::collapsed_signal).
    pub fn collapsible(mut self) -> Self {
        self.collapse_policy
            .get_or_insert(CollapsePolicy::Responsive);
        self
    }

    /// Like [`collapsible`](Self::collapsible), but uses the supplied
    /// signal as the collapsed-state source so the application can
    /// observe (and react to) collapse transitions. The responsive
    /// decision **writes** this signal (it is not a plain read-only
    /// input) — kept as a `Signal<bool>` rather than `Prop<bool>` since a
    /// static value would have nowhere to receive those writes.
    pub fn collapsed_signal(mut self, collapsed: Signal<bool>) -> Self {
        self.collapse_policy
            .get_or_insert(CollapsePolicy::Responsive);
        self.last_collapsed.set(collapsed.get());
        self.collapsed = collapsed;
        self
    }

    /// Set the collapse policy (and enable collapsible mode).
    /// [`CollapsePolicy::Always`] forces the hamburger regardless of
    /// available width — i.e. **collapsed by default**.
    pub fn collapse_policy(mut self, policy: CollapsePolicy) -> Self {
        self.collapse_policy = Some(policy);
        // Start already-collapsed for `Always` so the first frame shows
        // the hamburger (no one-frame inline flash before `place_children`
        // sets the signal).
        if policy == CollapsePolicy::Always {
            self.collapsed.set(true);
            self.last_collapsed.set(true);
        }
        self
    }

    /// Set the size variant of the collapsed-mode hamburger
    /// [`IconButton`]. Mirrors [`IconButton::size`] — pick
    /// [`IconButtonSize::Toolbar`], [`IconButtonSize::Large`],
    /// [`IconButtonSize::Hero`], etc. so the hamburger matches the
    /// surrounding chrome. Defaults to [`IconButtonSize::Default`].
    pub fn hamburger_size(mut self, size: IconButtonSize) -> Self {
        self.hamburger_size = size;
        self
    }

    /// A clone of the collapsed-state signal (`true` while the
    /// hamburger is shown). Call after [`collapsible`](Self::collapsible).
    pub fn is_collapsed(&self) -> Signal<bool> {
        self.collapsed.clone()
    }

    /// Skip the window-state dispatcher install. The MenuBar still
    /// renders, intercepts mouse clicks, and supports keyboard
    /// navigation when its triggers have focus — only F10 /
    /// Alt+letter / Alt-tap routing through the window-level slot is
    /// disabled. Use this for demo / showcase MenuBars that share a
    /// window with a primary functional MenuBar — the slot is
    /// single-occupancy and a second install would `debug_assert!`.
    pub fn no_dispatcher_install(mut self) -> Self {
        self.install_dispatcher = false;
        self
    }

    /// Add a top-level menu entry. `label` is the trigger text (supports `&`
    /// mnemonic markers, e.g. `"&File"`); `factory` is called each build to
    /// produce the dropdown content — typically a `MenuList`.
    pub fn menu(
        mut self,
        label: impl Into<LocalizedString>,
        factory: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        let ls: LocalizedString = label.into();
        self.entries.push(MenuBarEntry {
            label: ls,
            factory: Box::new(factory),
        });
        self
    }

    /// Add content before the menu buttons (e.g. an app icon). Call more than
    /// once to stack several.
    ///
    /// Takes the widget by value, like every other widget's slot. MenuBar
    /// builds it once and reuses it across rebuilds (it
    /// [`preserves_children_on_rebuild`](teksilo_core::widget::Widget::preserves_children_on_rebuild)),
    /// so the slot — and any state it holds — survives a theme / locale /
    /// model-version rebuild.
    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// Add content after the menu buttons (e.g. a search box or avatar).
    /// Like [`leading_slot`](Self::leading_slot), taken by value and preserved
    /// across rebuilds.
    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }

    /// macOS `Suppress` path: a zero-chrome bar that renders only the
    /// leading/trailing slots (the OS menu bar carries the menus). No triggers,
    /// no F10/Alt dispatcher.
    fn build_suppressed(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut row = HStack::new().spacing(2.0);
        row = Self::add_slot(ctx, row, &mut self.leading_slot, &mut self.leading_slot_ids);
        row = row.child(Spacer::new());
        row = Self::add_slot(
            ctx,
            row,
            &mut self.trailing_slot,
            &mut self.trailing_slot_ids,
        );
        let row_id = ctx.add(row);
        self.root_child_id = Some(row_id);
        self.bar_id = Some(row_id);
        vec![row_id]
    }
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MenuBarDispatcher — window-level F10 / Alt+letter / Alt-tap handler
// ---------------------------------------------------------------------------

/// `MenubarDispatcher` impl backed by the live trigger ids and
/// mnemonic table from the most recent `MenuBar::build`.
struct MenuBarDispatcher {
    /// All top-level trigger ids, in declaration order.
    trigger_ids: Vec<WidgetId>,
    /// Lower-cased mnemonic char → trigger array index.
    mnemonic_table: HashMap<char, usize>,
}

impl MenubarDispatcher for MenuBarDispatcher {
    fn try_handle(&self, event: &MenubarKeyEvent) -> Option<MenubarAction> {
        // F10 (no modifiers): focus the first trigger without
        // opening any menu — matches Win32 / GTK F10 behaviour.
        // Works on every platform (F10 is not transformed by any OS
        // input layer the way Alt+letter is on macOS).
        if event.modifiers == Modifiers::NONE && matches!(event.key, Key::F10) {
            return self
                .trigger_ids
                .first()
                .map(|&id| MenubarAction::FocusTrigger {
                    trigger_id: id,
                    reveal: None,
                });
        }
        // Alt+<letter> mnemonics. On macOS, Option+letter is
        // intercepted by the OS to compose accented characters
        // (Option+E -> ´, Option+F -> ƒ, …) *before* winit sees the
        // keystroke. The app receives the post-composition character
        // (`ƒ`), not the typed letter (`F`), so the mnemonic table
        // can never match. Worse, returning `Intercept` here would
        // silently swallow legitimate accented text input. Skip the
        // entire branch on macOS — F10 + Alt-tap + in-menu
        // bare-letter activation cover the macOS menu-keyboard
        // story instead.
        #[cfg(not(target_os = "macos"))]
        if event.modifiers == Modifiers::ALT {
            // Strict per-OS contract — `Alt+letter` is reserved for
            // menu mnemonics on Win32 / GTK and must be intercepted
            // even when nothing matches, so the chord doesn't
            // appear as garbled text input in a focused text field.
            let lookup_char = match event.key {
                Key::Character(c) => Some(c.to_ascii_lowercase()),
                _ => {
                    let c = event.key.to_char()?;
                    Some(c.to_ascii_lowercase())
                }
            };
            if let Some(c) = lookup_char {
                if let Some(&idx) = self.mnemonic_table.get(&c) {
                    if let Some(&tid) = self.trigger_ids.get(idx) {
                        return Some(MenubarAction::OpenMenu {
                            trigger_id: tid,
                            reveal: None,
                        });
                    }
                }
                // Letter-with-Alt that doesn't match any mnemonic —
                // intercept silently so the chord doesn't leak into
                // focused text input as garbled chars.
                return Some(MenubarAction::Intercept);
            }
        }
        // Suppress an unused-warning on macOS where the Alt branch
        // above is compiled out.
        let _ = &self.mnemonic_table;
        None
    }

    fn on_alt_tap(&self) -> Option<MenubarAction> {
        // Bare-Alt-tap (no other key during the hold) → focus the
        // first trigger in menubar-active mode (no menu opens until
        // ArrowDown / Enter / Space).
        self.trigger_ids
            .first()
            .map(|&id| MenubarAction::FocusTrigger {
                trigger_id: id,
                reveal: None,
            })
    }
}

// ---------------------------------------------------------------------------
// CollapsibleMenuBarDispatcher — wraps MenuBarDispatcher for hamburger mode
// ---------------------------------------------------------------------------

/// Delegates to the inner [`MenuBarDispatcher`], and — when the bar is
/// currently collapsed — attaches a `reveal` closure to the returned
/// action so `teksilo-app` reveals the floating bar (and re-layouts)
/// before focusing / opening. Preserves the inner dispatcher's
/// platform-specific behaviour (macOS Alt+letter compile-out, F10,
/// bare-Alt-tap) by pure delegation.
struct CollapsibleMenuBarDispatcher {
    inner: MenuBarDispatcher,
    collapsed: Signal<bool>,
    reveal: MenubarReveal,
}

impl CollapsibleMenuBarDispatcher {
    fn with_reveal(&self, action: MenubarAction) -> MenubarAction {
        if !self.collapsed.get() {
            return action;
        }
        let reveal = Some(self.reveal.clone());
        match action {
            MenubarAction::OpenMenu { trigger_id, .. } => {
                MenubarAction::OpenMenu { trigger_id, reveal }
            }
            MenubarAction::FocusTrigger { trigger_id, .. } => {
                MenubarAction::FocusTrigger { trigger_id, reveal }
            }
            MenubarAction::Intercept => MenubarAction::Intercept,
        }
    }
}

impl MenubarDispatcher for CollapsibleMenuBarDispatcher {
    fn try_handle(&self, event: &MenubarKeyEvent) -> Option<MenubarAction> {
        self.inner.try_handle(event).map(|a| self.with_reveal(a))
    }

    fn on_alt_tap(&self) -> Option<MenubarAction> {
        self.inner.on_alt_tap().map(|a| self.with_reveal(a))
    }
}

impl std::fmt::Debug for MenuBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuBar")
            .field("entries", &self.entries.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// MenuBarTrigger — internal trigger label
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MenuBarTrigger {
    label: LocalizedString,
    /// Mnemonic-stripped label name used for `AccessNodeBuilder::set_name`.
    /// Captured from the parsed label so screen readers announce "File",
    /// not "ampersand-File". Set in `build()`.
    stripped_name: String,
    /// Mnemonic letter (lowercase) for AT `set_access_key` annotation.
    /// `None` for triggers whose label carries no un-escaped `&`.
    mnemonic_key: Option<char>,
    index: usize,
    menu_ctx: MenuContext,
    root_child_id: Option<WidgetId>,
}

// ---------------------------------------------------------------------------
// MenuOverlayHost — wraps dropdown content, handles focus + cross-menu keys
// ---------------------------------------------------------------------------

/// Wraps dropdown menu content (typically a MenuList). Responsibilities:
/// - Resets `open_index` when focus is lost (overlay dismissed)
/// - Handles ArrowLeft/Right for cross-menu navigation (bubbles up from MenuList)
#[derive(Debug)]
struct MenuOverlayHost {
    inner: Option<Box<dyn Widget>>,
    menu_ctx: MenuContext,
    menu_index: usize,
    inner_id: Option<WidgetId>,
}

// ---------------------------------------------------------------------------
// RevealHeightBox — match the floating bar's height to the hamburger
// ---------------------------------------------------------------------------

/// Wraps the collapsible bar's content. While the bar is shown as a
/// floating overlay (`revealed == true`) it reports a height equal to the
/// hamburger button's measured height, so the floating bar reads as a
/// horizontal extension of the hamburger and the menu-trigger text centers
/// vertically (the inner `HStack`'s default `VAlignment::Center`). When the
/// bar is inline (`revealed == false`) it reports the child's natural size,
/// leaving the normal in-window bar unchanged.
#[derive(Debug)]
struct RevealHeightBox {
    child_id: Option<WidgetId>,
    pending_child: Option<PendingChild>,
    revealed: Signal<bool>,
    /// The hamburger `IconButton` id, filled after it is built (the
    /// `anchor_cell` pattern). Measuring it — rather than mapping the size
    /// table — honours a custom `IconButtonSize` / style for free.
    hamburger_id: Rc<Cell<Option<WidgetId>>>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
