//! MenuBar — a horizontal menu bar with dropdown menus.
//!
//! # Bastyde
//! ```ignore
//! MenuBar::new()
//!     .menu(lit!("File"), || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new(lit!("New")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::New)))
//!             .separator()
//!             .item(MenuItem::new(lit!("Quit")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Quit)))
//!     ))
//!     .menu(lit!("Edit"), || Box::new(
//!         MenuList::new()
//!             .item(MenuItem::new(lit!("Cut")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Cut)))
//!             .item(MenuItem::new(lit!("Copy")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Copy)))
//!     ))
//!     .trailing_slot(Button::new(lit!("Settings")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Settings)))
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::{HandlerSet, WidgetBuilder};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::window::{
    MenubarAction, MenubarDispatcher, MenubarGuard, MenubarKeyEvent, MenubarReveal,
};
use bastyde_tokens::{SurfaceRole, TextStyleRole};

use crate::icon_button::IconButton;
use crate::menu_context::MenuContext;
use crate::menu_item::MenuLabel;
use crate::menu_item::ParsedMnemonic;
use crate::menu_item::parse_mnemonic;
use crate::primitives::{HStack, Padding, RectWidget, Spacer, ZStack};
use bastyde_i18n::LocalizedString;

/// Controls how a [`MenuBar`] made collapsible via [`MenuBar::collapsible`]
/// decides between the full inline bar and the hamburger representation.
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

/// A horizontal menu bar with dropdown menus.
///
/// Supports named content slots:
/// - `leading_slot`: content before the menu buttons (e.g., app icon)
/// - `trailing_slot`: content after the menu buttons (e.g., search, user avatar)
pub struct MenuBar {
    entries: Vec<MenuBarEntry>,
    leading_slot: Vec<PendingChild>,
    trailing_slot: Vec<PendingChild>,
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
    /// Idempotence guard for the responsive write (Toolbar pattern).
    last_collapsed: Cell<bool>,
    /// The bar root (ZStack) id, captured in `build()`. Used both as the
    /// inline content and as the floating-overlay content when collapsed.
    bar_id: Option<WidgetId>,
    /// The hamburger `IconButton` id, captured in `build()`.
    hamburger_id: Option<WidgetId>,
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
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            leading_slot: Vec::new(),
            trailing_slot: Vec::new(),
            root_child_id: None,
            menubar_guard: RefCell::new(None),
            install_dispatcher: true,
            collapse_policy: None,
            collapsed: Signal::new(false),
            revealed: Signal::new(false),
            last_collapsed: Cell::new(false),
            bar_id: None,
            hamburger_id: None,
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
                crate::menu::MenuNode::Submenu { title, children, .. } => {
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
    /// with [`collapsible_bound`](Self::collapsible_bound).
    pub fn collapsible(mut self) -> Self {
        self.collapse_policy.get_or_insert(CollapsePolicy::Responsive);
        self
    }

    /// Like [`collapsible`](Self::collapsible), but uses the supplied
    /// signal as the collapsed-state source so the application can
    /// observe (and react to) collapse transitions. The responsive
    /// decision writes this signal.
    pub fn collapsible_bound(mut self, collapsed: Signal<bool>) -> Self {
        self.collapse_policy.get_or_insert(CollapsePolicy::Responsive);
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

    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot
            .push(PendingChild::Deferred(Box::new(widget)));
        self
    }

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
        for pending in self.leading_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }
        row = row.child(Spacer::new());
        for pending in self.trailing_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }
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
            return self.trigger_ids.first().map(|&id| MenubarAction::FocusTrigger {
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
        self.trigger_ids.first().map(|&id| MenubarAction::FocusTrigger {
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
/// action so `bastyde-app` reveals the floating bar (and re-layouts)
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

impl Widget for MenuBarTrigger {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme();
        let radius_control = theme.shape.radius_control;
        use crate::styles::recipe_menu_item_style as menu;
        let index = self.index;
        let menu_ctx = self.menu_ctx.clone();

        // Background role: `AccentSubtle` when open (the Int UI token for
        // highlighted menu-bar entries) or `Transparent` at rest. Replaces
        // the previous hand-mixed `accent.with_alpha(0.12)` wash.
        let bg_role = menu_ctx.open_index.map(move |open| {
            if *open == Some(index) {
                SurfaceRole::AccentSubtle
            } else {
                SurfaceRole::Transparent
            }
        });

        // Text color can't collapse to a pure role: the at-rest state is
        // `text_primary.with_alpha(0.8)` (dimmed primary — distinct from
        // TextRole::Secondary, which is a different hue). Keep a direct
        // `theme_signal` map for the blended case.
        let theme_signal = ctx.theme_signal();
        let text_color = menu_ctx
            .open_index
            .zip(&theme_signal)
            .map(move |(open, t)| {
                if *open == Some(index) {
                    t.colors.text_primary
                } else {
                    t.colors.text_primary.with_alpha(0.8)
                }
            });

        // Label. Uses `MenuLabel` so a single `&` in the trigger
        // string acts as a mnemonic marker — stripped from the
        // visible text and underlined when the window's `alt_down`
        // signal is true.
        let alt_down = ctx
            .window()
            .map(|w| w.alt_down().clone())
            .unwrap_or_else(|| Signal::new(false));
        let label_source: bastyde_core::signal::Prop<String> = self.label.clone().into();
        let label_id = ctx.add(MenuLabel::new(
            label_source,
            alt_down,
            text_color,
            TextStyleRole::Small,
        ));

        let padding =
            Padding::symmetric(4.0, menu::MENU_ITEM_PADDING_HORIZONTAL).child_id(label_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_role)
            .corner_radius(bastyde_tokens::CornerRadius::uniform(radius_control));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap({
                let menu_ctx = menu_ctx.clone();
                move |_pos, ctx: &mut EventContext| {
                    if menu_ctx.open_index.get() == Some(index) {
                        menu_ctx.close(ctx);
                    } else {
                        menu_ctx.open_at(index, ctx);
                    }
                }
            })
            .on_hover({
                let menu_ctx = menu_ctx.clone();
                move |entered: bool, ctx: &mut EventContext| {
                    if entered {
                        // If another menu is open, switch immediately (no delay)
                        let current = menu_ctx.open_index.get();
                        if current.is_some() && current != Some(index) {
                            menu_ctx.open_at(index, ctx);
                        }
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // The menu bar lays out right-to-left under RTL, so the
                    // visual "previous/next menu" arrows swap: ArrowLeft moves
                    // to the next (visually-left) menu and ArrowRight to the
                    // previous one.
                    let (left_delta, right_delta) = if ctx.is_rtl() { (1, -1) } else { (-1, 1) };
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown | Key::Enter | Key::Space,
                            ..
                        } => {
                            menu_ctx.open_at(index, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(left_delta, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(right_delta, ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        // Re-query accessibility when this trigger's open/closed state flips so
        // `set_expanded` stays in sync with the open menu index.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.menu_ctx.open_index.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 28.0)),
            None => proposal.resolve(60.0, 28.0),
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
        builder.set_role(bastyde_core::accesskit::Role::MenuItem);
        // Stripped name — set in `build()` from the parsed mnemonic.
        // Falls back to a fresh resolve if the trigger has not been
        // built yet (rare; AT walks always happen post-build).
        if !self.stripped_name.is_empty() {
            builder.set_name(self.stripped_name.clone());
        } else {
            builder.set_name(parse_mnemonic(&self.label.resolve_now()).stripped);
        }
        // Every top-level menu bar entry opens a dropdown Menu.
        builder.set_has_popup(bastyde_core::accesskit::HasPopup::Menu);
        builder.set_expanded(self.menu_ctx.open_index.get() == Some(self.index));
        // Mnemonic — announced by Windows Narrator as "Access key: F".
        if let Some(k) = self.mnemonic_key {
            builder
                .inner_mut()
                .set_access_key(k.to_ascii_uppercase().to_string());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
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

impl Widget for MenuOverlayHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let inner_widget = self.inner.take().expect("MenuOverlayHost built twice");
        let id = ctx.add_boxed(inner_widget);
        self.inner_id = Some(id);

        // Register inner widget as the focus target for this menu index
        self.menu_ctx.set_focus_id(self.menu_index, id);

        let menu_ctx = self.menu_ctx.clone();
        let menu_index = self.menu_index;
        let handler_set = HandlerSet::new()
            .on_focus({
                let menu_ctx = menu_ctx.clone();
                move |gained: bool, ctx: &mut EventContext| {
                    if !gained && menu_ctx.open_index.get() == Some(menu_index) {
                        // Overlay was dismissed — close the menu and restore focus
                        menu_ctx.close(ctx);
                    }
                }
            })
            .on_key({
                let menu_ctx = menu_ctx.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    // These keys bubble up from the inner MenuList when it
                    // returns Ignored. Under RTL the bar is laid out
                    // right-to-left, so the previous/next arrows swap.
                    let (left_delta, right_delta) = if ctx.is_rtl() { (1, -1) } else { (-1, 1) };
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft,
                            ..
                        } => {
                            menu_ctx.navigate(left_delta, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight,
                            ..
                        } => {
                            menu_ctx.navigate(right_delta, ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            menu_ctx.close(ctx);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            });
        // NOT focusable — the inner MenuList receives focus directly.
        // ArrowLeft/Right and FocusLost bubble from MenuList through here.
        ctx.apply_self_handlers(handler_set);

        vec![id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        self.inner_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
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
        // The inner widget (typically `MenuList`) owns the `Role::Menu`
        // semantics. A second Menu role here would nest two Menu nodes
        // per dropdown, confusing screen readers that look for a single
        // Menu per popup. `GenericContainer` is the ARIA `none`/`presentation`
        // equivalent: the host is kept in the tree for focus/key routing
        // but is ignored by assistive tech.
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// MenuBar Widget impl
// ---------------------------------------------------------------------------

impl Widget for MenuBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Mirror the model into the native OS menu bar (macOS) when requested.
        // The bridge is a no-op without a `NativeMenuHandle` in app-state.
        if self.native_mode.installs_native()
            && cfg!(target_os = "macos")
            && let Some(model) = &self.model
        {
            *self.native_binding.borrow_mut() = crate::menu::native::install(model, ctx);
        }

        // A runtime structural change (`MenuModel::push_item`/`remove`/…) bumps
        // the model version; rebuild so the in-window dropdowns AND the native
        // menu re-derive from the new structure.
        if let Some(model) = &self.model {
            model.version().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                bastyde_core::BindingLevel::Rebuild,
            );
        }

        // On macOS with `Suppress`, the global menu bar IS the menu — render
        // only the optional leading/trailing slots in-window (no triggers, no
        // F10/Alt dispatcher).
        if self.native_mode.suppresses_in_window() {
            return self.build_suppressed(ctx);
        }

        let theme_signal = ctx.theme_signal();

        let open_index: Signal<Option<usize>> = ctx.signal(None);
        let menu_ctx = MenuContext::new(open_index);

        // Build the full row: [leading_slot | triggers... | Spacer | trailing_slot]
        let mut row = HStack::new().spacing(2.0);

        // Leading slot
        for pending in self.leading_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        // Menu triggers + content
        let mut trigger_ids = Vec::new();
        let mut content_ids = Vec::new();
        // Mnemonic table built alongside triggers: `lowercase char →
        // trigger array index`. Drives the window-level dispatcher
        // for Alt+letter activation.
        let mut mnemonic_table: HashMap<char, usize> = HashMap::new();

        // Model-built bars re-derive entries from the (possibly mutated) model
        // every build; classic `.menu()` bars consume their stored entries.
        let entries = match &self.model {
            Some(model) => Self::model_entries(model),
            None => std::mem::take(&mut self.entries),
        };
        for (i, entry) in entries.into_iter().enumerate() {
            let parsed: ParsedMnemonic = parse_mnemonic(&entry.label.resolve_now());

            // Wrap factory output in MenuOverlayHost for focus/key handling
            let host = MenuOverlayHost {
                inner: Some((entry.factory)()),
                menu_ctx: menu_ctx.clone(),
                menu_index: i,
                inner_id: None,
            };
            let content_id = ctx.add(host);
            ctx.set_dormant(content_id);

            let trigger = MenuBarTrigger {
                label: entry.label,
                stripped_name: parsed.stripped.clone(),
                mnemonic_key: parsed.key_lower,
                index: i,
                menu_ctx: menu_ctx.clone(),
                root_child_id: None,
            };
            let trigger_id = ctx.add(trigger);
            row = row.add_child(trigger_id);

            if let Some(k) = parsed.key_lower {
                if let Some(prev) = mnemonic_table.insert(k, i) {
                    debug_assert!(
                        false,
                        "MenuBar: duplicate mnemonic {:?} (triggers {} and {})",
                        k, prev, i
                    );
                }
            }

            trigger_ids.push(trigger_id);
            content_ids.push(content_id);
        }

        // Register all trigger/content IDs in the context.
        // focus_id is initially content_id; MenuOverlayHost::build() will
        // overwrite it with the actual inner MenuList ID.
        for (i, (&tid, &cid)) in trigger_ids.iter().zip(content_ids.iter()).enumerate() {
            menu_ctx.register(i, tid, cid, cid);
        }

        // Spacer pushes triggers left, trailing slot right
        row = row.child(Spacer::new());

        // Trailing slot
        for pending in self.trailing_slot.drain(..) {
            match pending {
                PendingChild::Id(id) => row = row.add_child(id),
                PendingChild::Deferred(w) => {
                    let id = ctx.add_boxed(w);
                    row = row.add_child(id);
                }
            }
        }

        let row_id = ctx.add(row);

        let bg = RectWidget::new()
            .background(SurfaceRole::Main)
            .bind_border_color(theme_signal.map(|t| t.colors.border.with_alpha(0.2)))
            .bind_border_width(0.0_f32);
        let bg_id = ctx.add(bg);

        let padding = Padding::symmetric(0.0, 2.0).child_id(row_id);
        let padding_id = ctx.add(padding);

        let zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        // In collapsible mode the `Role::MenuBar` landmark lives on the
        // bar content node (not the composing widget) so it travels into
        // the floating overlay AND so `overlay_is_host_surface` treats
        // the revealed bar as a host (menu-open dismissal spares it).
        let root_id = if self.collapse_policy.is_some() {
            ctx.add(zstack.access_role(bastyde_core::accesskit::Role::MenuBar))
        } else {
            ctx.add(zstack)
        };
        self.root_child_id = Some(root_id);
        self.bar_id = Some(root_id);

        // Collapsible (hamburger) mode: build the hamburger button and
        // the reveal closure that floats the bar as an overlay; gate
        // inline visibility on `collapsed` / `revealed`.
        let mut children = vec![root_id];
        let collapsible_reveal: Option<MenubarReveal> = if self.collapse_policy.is_some() {
            let bar_id = root_id;
            let revealed = self.revealed.clone();
            let collapsed = self.collapsed.clone();

            // The bar overlay trails the hamburger (the developer is
            // responsible for placing the hamburger). The anchor cell is
            // filled after the button is added, since the reveal closure
            // is created before the button id is known.
            let anchor_cell: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
            // First trigger, focused on reveal so the bar is immediately
            // keyboard-navigable (arrows move between menus, Enter opens).
            let first_trigger = trigger_ids.first().copied();

            let reveal: MenubarReveal = {
                let revealed = revealed.clone();
                let anchor_cell = anchor_cell.clone();
                Rc::new(move |ctx: &mut EventContext| {
                    if revealed.get() {
                        return; // idempotent — already revealed
                    }
                    revealed.set(true);
                    ctx.activate(bar_id);
                    let anchor = anchor_cell.get().unwrap_or(bar_id);
                    let on_dismiss: Rc<dyn Fn()> = {
                        let revealed = revealed.clone();
                        Rc::new(move || revealed.set(false))
                    };
                    ctx.show_overlay(OverlayRequest {
                        content_id: bar_id,
                        anchor,
                        placement: OverlayPlacement::TrailingEdge,
                        dismiss: DismissBehavior::EscapeOrClickOutside,
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: Some(on_dismiss),
                        fade_duration: None,
                    });
                    if let Some(trigger) = first_trigger {
                        ctx.request_focus(trigger);
                    }
                })
            };

            // `IconButton::menu()` already advertises `HasPopup::Menu` and
            // an accessible name ("Menu"). Binding `expanded_when(revealed)`
            // completes the ARIA disclosure pattern: the button reports
            // `expanded=true` while the bar is shown, `false` while collapsed.
            let hamburger = IconButton::menu()
                .expanded_when(revealed.clone())
                .on_activate_fn({
                    let reveal = reveal.clone();
                    move |ctx| reveal(ctx)
                });
            let hamburger_id = ctx.add(hamburger);
            anchor_cell.set(Some(hamburger_id));
            self.hamburger_id = Some(hamburger_id);

            // Hamburger visible only while collapsed.
            ctx.visible_when(hamburger_id, collapsed.clone());
            // Bar active when shown inline (`!collapsed`) OR as the
            // floating overlay (`revealed`). Keeping it active while
            // revealed prevents the visibility binding from fighting the
            // overlay activation.
            let bar_active = collapsed.zip(&revealed).map(|(c, r)| !*c || *r);
            ctx.visible_when(bar_id, bar_active);

            children.push(hamburger_id);
            Some(reveal)
        } else {
            None
        };

        // Window-level menubar key dispatcher (F10 / Alt+letter /
        // Alt-tap). Installed on every platform — `MenuBar` is an
        // in-window widget menu, not the OS system menu, so the
        // dispatcher's job is to wire framework menus to keyboard
        // accelerators regardless of host OS.
        //
        // **macOS**: the dispatcher's `Alt+letter` branch is compiled
        // out (see `MenuBarDispatcher::try_handle`) because the OS
        // rewrites Option+letter for accented character composition
        // before the app sees the keystroke. F10 and bare-Alt-tap
        // continue to fire on macOS through this same dispatcher.
        //
        // Drop the previous guard BEFORE installing the new one so
        // the slot is empty when `install_menubar_dispatcher` runs
        // its `debug_assert!(slot.is_none())`. Otherwise a rebuild
        // of `MenuBar` (e.g. when a composing ancestor rebuilds)
        // trips the assert in debug builds and would over-write the
        // slot under another live guard in release.
        if self.install_dispatcher
            && let Some(window) = ctx.window()
        {
            *self.menubar_guard.borrow_mut() = None;
            let inner = MenuBarDispatcher {
                trigger_ids: trigger_ids.clone(),
                mnemonic_table,
            };
            let dispatcher: Rc<dyn MenubarDispatcher> = match collapsible_reveal {
                Some(reveal) => Rc::new(CollapsibleMenuBarDispatcher {
                    inner,
                    collapsed: self.collapsed.clone(),
                    reveal,
                }),
                None => Rc::new(inner),
            };
            let guard = window.install_menubar_dispatcher(dispatcher);
            *self.menubar_guard.borrow_mut() = Some(guard);
        }

        children
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Collapsed: size to the hamburger's natural size (a small box),
        // don't stretch to the full allotted width.
        if self.collapse_policy.is_some() && self.collapsed.get() {
            return match self.hamburger_id {
                Some(id) => ctx
                    .child_size(id, SizeProposal::unspecified())
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
                None => proposal.resolve(0.0, 0.0),
            }
            .into();
        }
        match self.root_child_id {
            Some(id) => {
                let content_proposal = SizeProposal {
                    width: proposal.width,
                    height: None,
                };
                let size = ctx
                    .child_size(id, content_proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(proposal.width.unwrap_or(size.width), size.height)
            }
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Responsive collapse decision (Toolbar pattern): compare the
        // bar's intrinsic width against the allotted width and toggle
        // `collapsed`, idempotently (the guard avoids relayout churn).
        if let Some(policy) = self.collapse_policy {
            let should_collapse = match policy {
                CollapsePolicy::Always => true,
                CollapsePolicy::Responsive => {
                    if self.revealed.get() {
                        // Don't un-collapse while the overlay is up — it
                        // would make the bar both inline and floating.
                        self.collapsed.get()
                    } else if let (Some(bar_id), Some(avail)) =
                        (self.bar_id, proposal.width)
                    {
                        ctx.measure_intrinsic(bar_id, SizeProposal::unspecified())
                            .map(|s| s.width)
                            .unwrap_or(0.0)
                            > avail + 0.5
                    } else {
                        // Unbounded width (or no bar) → never collapse.
                        false
                    }
                }
            };
            if self.last_collapsed.get() != should_collapse {
                self.last_collapsed.set(should_collapse);
                self.collapsed.set(should_collapse);
            }
        }

        // The hamburger keeps a constant width: place it at its intrinsic
        // size, leading-aligned, so a stretching parent can't widen it.
        // Everything else (the bar, inline or as the re-laid overlay) fills
        // the bounds; dormant children are skipped by the layout pass.
        let collapsed = self.collapse_policy.is_some() && self.collapsed.get();
        for child in children.iter_mut() {
            if collapsed && Some(child.id) == self.hamburger_id {
                let size = ctx
                    .measure_intrinsic(child.id, SizeProposal::unspecified())
                    .unwrap_or_else(|| bounds.size());
                let x = if ctx.is_rtl() {
                    bounds.right() - size.width
                } else {
                    bounds.x
                };
                child.origin = Point::new(x, bounds.y);
                child.size = size;
            } else {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // In collapsible mode the `Role::MenuBar` landmark lives on the
        // bar content node so it travels into the floating overlay; the
        // composing widget node stays a generic container.
        if self.collapse_policy.is_none() {
            builder.set_role(bastyde_core::accesskit::Role::MenuBar);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut v: Vec<WidgetId> = self.root_child_id.into_iter().collect();
        if let Some(h) = self.hamburger_id {
            v.push(h);
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_list::MenuList;
    use bastyde_core::accesskit::Role;
    use bastyde_core::widget_id::WidgetId;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_core::window::state::WindowStateInit;
    use bastyde_core::window::{BastydeWindowId, WindowPlacement, WindowState};
    use bastyde_i18n::lit;

    fn tree_with_window() -> WidgetTree {
        let mut t = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        t.set_window_state(WindowState::new(WindowStateInit {
            id: BastydeWindowId::new(1),
            string_id: Some("test".to_string()),
            placement: WindowPlacement::Floating,
            title: "Test".to_string(),
            size: (800, 600),
            position: (0, 0),
            focused: false,
            resizable: true,
            always_on_top: false,
        }));
        t
    }

    fn first_descendant_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> Option<WidgetId> {
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(from);
        while let Some(id) = queue.pop_front() {
            if t.accessibility_node(id).role() == role {
                return Some(id);
            }
            for child in t.children(id) {
                queue.push_back(child);
            }
        }
        None
    }

    fn collect_descendants_with_role(t: &WidgetTree, from: WidgetId, role: Role) -> Vec<WidgetId> {
        let mut queue = std::collections::VecDeque::new();
        let mut out = Vec::new();
        queue.push_back(from);
        while let Some(id) = queue.pop_front() {
            if t.accessibility_node(id).role() == role {
                out.push(id);
            }
            for child in t.children(id) {
                queue.push_back(child);
            }
        }
        out
    }

    #[test]
    fn menubar_emits_role_menubar() {
        let mut t = tree_with_window();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert_eq!(t.accessibility_node(mb).role(), Role::MenuBar);
    }

    #[test]
    fn trigger_uses_stripped_name_in_at() {
        let mut t = tree_with_window();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        assert_eq!(triggers.len(), 2);
        // The stripped name "File" / "Edit", NOT "&File" / "&Edit".
        let info0 = t.accessibility_node(triggers[0]);
        let info1 = t.accessibility_node(triggers[1]);
        assert_eq!(info0.name(), Some("File"));
        assert_eq!(info1.name(), Some("Edit"));
    }

    #[test]
    fn trigger_arrow_navigation_ltr_right_goes_to_next() {
        let mut t = tree_with_window();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .menu(lit!("&View"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        assert_eq!(triggers.len(), 3);

        // From the File trigger, ArrowRight opens the next (Edit) menu in LTR.
        t.focus(triggers[0]);
        t.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!(t.accessibility_node(triggers[1]).is_expanded());
        assert!(!t.accessibility_node(triggers[0]).is_expanded());
    }

    #[test]
    fn trigger_arrow_navigation_rtl_right_goes_to_previous() {
        let mut t = tree_with_window();
        t.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .menu(lit!("&View"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        assert_eq!(triggers.len(), 3);

        // Under RTL the bar runs right-to-left, so ArrowRight moves to the
        // *previous* menu — from File (index 0) that wraps to View (index 2).
        t.focus(triggers[0]);
        t.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!(t.accessibility_node(triggers[2]).is_expanded());
        assert!(!t.accessibility_node(triggers[0]).is_expanded());
    }

    #[test]
    fn dispatcher_installed_on_every_platform() {
        let mut t = tree_with_window();
        t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        let window = t.window_state().expect("window state attached");
        assert!(
            window.menubar_dispatcher().is_some(),
            "MenuBar should install the window-level dispatcher on every \
             platform — framework menus aren't the OS system menu and need \
             keyboard accelerators wired regardless of host OS"
        );
    }

    #[test]
    fn rebuilding_menubar_does_not_double_install_dispatcher() {
        // Regression: `install_menubar_dispatcher` debug_asserts that
        // the slot is empty before installing. The old `MenuBar::build`
        // implementation called install while the previous build's
        // `MenubarGuard` was still alive in `self.menubar_guard`,
        // which tripped the assert on every rebuild. Fixed by
        // dropping the old guard FIRST.
        let mut t = tree_with_window();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new())),
        );
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert!(t.window_state().unwrap().menubar_dispatcher().is_some());
        // Force a rebuild and confirm the dispatcher install path
        // doesn't crash (debug builds) or silently overwrite a live
        // guard (release builds).
        t.arena_mark_needs_rebuild_for_testing(mb);
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert!(
            t.window_state().unwrap().menubar_dispatcher().is_some(),
            "after rebuild the dispatcher slot must still point at \
             the most-recently-installed dispatcher"
        );
    }

    #[test]
    fn windowstate_dispatcher_slot_reinstall_after_guard_drop() {
        // Direct unit test of the WindowState slot lifecycle —
        // installing a second dispatcher after dropping the first
        // guard must succeed without a debug_assert.
        use bastyde_core::window::{MenubarAction, MenubarDispatcher, MenubarKeyEvent};

        struct Noop;
        impl MenubarDispatcher for Noop {
            fn try_handle(&self, _ev: &MenubarKeyEvent) -> Option<MenubarAction> {
                None
            }
        }

        let mut t = tree_with_window();
        let window = t.window_state().unwrap().clone();
        let guard_a = window.install_menubar_dispatcher(Rc::new(Noop));
        assert!(window.menubar_dispatcher().is_some());
        drop(guard_a);
        assert!(
            window.menubar_dispatcher().is_none(),
            "dropping the guard must clear the slot"
        );
        let _guard_b = window.install_menubar_dispatcher(Rc::new(Noop));
        assert!(
            window.menubar_dispatcher().is_some(),
            "second install after first guard's drop must succeed without an assert"
        );
        let _ = &mut t;
    }

    // --- Pure-function dispatcher tests (platform-independent) ---

    /// Fabricate a `WidgetId` from a numeric tag for tests that don't
    /// need a real arena. Mirrors the convention used across
    /// `bastyde-core`'s signal / overlay tests.
    fn fake_id(n: u64) -> WidgetId {
        slotmap::KeyData::from_ffi(n).into()
    }

    fn make_dispatcher() -> MenuBarDispatcher {
        let mut mnemonic_table = HashMap::new();
        mnemonic_table.insert('f', 0);
        mnemonic_table.insert('e', 1);
        mnemonic_table.insert('v', 2);
        MenuBarDispatcher {
            trigger_ids: vec![fake_id(10), fake_id(11), fake_id(12)],
            mnemonic_table,
        }
    }

    #[test]
    fn dispatcher_f10_focuses_first_trigger() {
        let d = make_dispatcher();
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F10,
            modifiers: Modifiers::NONE,
        });
        assert!(matches!(
            action,
            Some(MenubarAction::FocusTrigger { trigger_id, .. }) if trigger_id == fake_id(10)
        ));
    }

    #[test]
    fn dispatcher_f10_with_modifier_ignored() {
        let d = make_dispatcher();
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F10,
            modifiers: Modifiers::CTRL,
        });
        assert!(action.is_none());
    }

    // Alt+letter is intentionally unwired on macOS — the OS rewrites
    // Option+letter for accented input before the app sees the
    // keystroke, so the dispatcher's Alt branch is compiled out
    // there. These tests assert the Win32 / GTK semantic.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn dispatcher_alt_letter_opens_matching_menu() {
        let d = make_dispatcher();
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F,
            modifiers: Modifiers::ALT,
        });
        assert!(matches!(
            action,
            Some(MenubarAction::OpenMenu { trigger_id, .. }) if trigger_id == fake_id(10)
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn dispatcher_alt_letter_no_match_intercepts() {
        let d = make_dispatcher();
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::Q,
            modifiers: Modifiers::ALT,
        });
        assert!(matches!(action, Some(MenubarAction::Intercept)));
    }

    #[test]
    fn dispatcher_alt_unrelated_key_ignored() {
        // Modifier != bare Alt → no menubar action. We use Modifiers::CTRL
        // here because constructing a multi-modifier value isn't part
        // of the public Modifiers API; the dispatcher relies on exact
        // equality with `Modifiers::ALT`.
        let d = make_dispatcher();
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F,
            modifiers: Modifiers::CTRL,
        });
        assert!(action.is_none());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn dispatcher_case_insensitive_alt_letter() {
        let d = make_dispatcher();
        // Lowercase 'f' and uppercase 'F' both open the matching menu.
        let lower = d.try_handle(&MenubarKeyEvent {
            key: Key::Character('f'),
            modifiers: Modifiers::ALT,
        });
        let upper = d.try_handle(&MenubarKeyEvent {
            key: Key::Character('F'),
            modifiers: Modifiers::ALT,
        });
        assert!(matches!(lower, Some(MenubarAction::OpenMenu { .. })));
        assert!(matches!(upper, Some(MenubarAction::OpenMenu { .. })));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dispatcher_alt_letter_does_not_intercept_on_macos() {
        // macOS-specific: the dispatcher must NOT intercept Alt+letter
        // because the OS rewrites it for accented character input;
        // intercepting would silently break text input.
        let d = make_dispatcher();
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F,
            modifiers: Modifiers::ALT,
        });
        assert!(
            action.is_none(),
            "macOS: Alt+letter must fall through to focus dispatch \
             so accented character input still works in text fields"
        );
    }

    #[test]
    fn dispatcher_alt_tap_focuses_first_trigger() {
        let d = make_dispatcher();
        let action = d.on_alt_tap();
        assert!(matches!(
            action,
            Some(MenubarAction::FocusTrigger { trigger_id, .. }) if trigger_id == fake_id(10)
        ));
    }

    #[test]
    fn dispatcher_alt_tap_with_no_triggers_is_none() {
        let d = MenuBarDispatcher {
            trigger_ids: Vec::new(),
            mnemonic_table: HashMap::new(),
        };
        assert!(d.on_alt_tap().is_none());
        assert!(
            d.try_handle(&MenubarKeyEvent {
                key: Key::F10,
                modifiers: Modifiers::NONE,
            })
            .is_none()
        );
    }

    // ── Collapsible (hamburger) mode ─────────────────────────────────────

    fn collapsible_tree() -> WidgetTree {
        let mut t = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(std::rc::Rc::new(std::cell::RefCell::new(
                bastyde_canvas::MockTextBackend::new(),
            )));
        t.set_window_state(WindowState::new(WindowStateInit {
            id: BastydeWindowId::new(1),
            string_id: Some("test".to_string()),
            placement: WindowPlacement::Floating,
            title: "Test".to_string(),
            size: (800, 600),
            position: (0, 0),
            focused: false,
            resizable: true,
            always_on_top: false,
        }));
        t
    }

    #[test]
    fn collapsible_always_shows_hamburger() {
        let mut t = collapsible_tree();
        let mb_widget = MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapse_policy(CollapsePolicy::Always);
        let collapsed = mb_widget.is_collapsed();
        let mb = t.add(mb_widget);
        // Two passes: pass 1 sets `collapsed`, pass 2 settles visibility.
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(collapsed.get(), "Always policy must collapse to hamburger");
        let children = t.children(mb);
        assert_eq!(children.len(), 2, "[bar, hamburger]");
        let (bar, hamburger) = (children[0], children[1]);
        assert!(t.is_active(hamburger), "hamburger active when collapsed");
        assert!(
            !t.is_active(bar),
            "bar dormant when collapsed and not revealed"
        );
    }

    /// The hamburger keeps a constant (intrinsic) width even when a
    /// stretching parent hands the collapsed MenuBar a much wider slot.
    #[test]
    fn collapsible_hamburger_keeps_constant_width_in_wide_slot() {
        use crate::primitives::FixedSize;
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        // FixedSize fills its child to 600px wide.
        let _slot = t.add(FixedSize::new().bind_width(600.0_f32).child_id(mb));
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));

        let hamburger = t.children(mb)[1];
        let hw = t.bounds(hamburger).width;
        assert!(
            hw > 0.0 && hw < 200.0,
            "hamburger width {hw} must stay compact, not fill the 600px slot"
        );
    }

    #[test]
    fn collapsible_responsive_collapses_when_narrow() {
        let mut t = collapsible_tree();
        let mb_widget = MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .menu(lit!("&View"), || Box::new(MenuList::new()))
            .collapsible();
        let collapsed = mb_widget.is_collapsed();
        let _mb = t.add(mb_widget);
        t.layout(SizeProposal::exact(40.0, 100.0));
        t.layout(SizeProposal::exact(40.0, 100.0));
        assert!(collapsed.get(), "narrow width must collapse to hamburger");
    }

    #[test]
    fn collapsible_responsive_expands_when_wide() {
        let mut t = collapsible_tree();
        let mb_widget = MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .collapsible();
        let collapsed = mb_widget.is_collapsed();
        let mb = t.add(mb_widget);
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(!collapsed.get(), "wide width must show the inline bar");
        let children = t.children(mb);
        assert!(t.is_active(children[0]), "bar active inline when wide");
        assert!(
            !t.is_active(children[1]),
            "hamburger dormant when bar is inline"
        );
    }

    /// A collapsible MenuBar wide enough on its own becomes a hamburger
    /// once its allotted width drops below the bar's intrinsic width.
    #[test]
    fn collapsible_responsive_toggles_with_width() {
        let mut t = collapsible_tree();
        let mb_widget = MenuBar::new()
            .menu(lit!("&File"), || Box::new(MenuList::new()))
            .menu(lit!("&Edit"), || Box::new(MenuList::new()))
            .menu(lit!("&View"), || Box::new(MenuList::new()))
            .collapsible();
        let collapsed = mb_widget.is_collapsed();
        let _mb = t.add(mb_widget);

        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(!collapsed.get(), "wide → inline");

        t.layout(SizeProposal::exact(30.0, 100.0));
        t.layout(SizeProposal::exact(30.0, 100.0));
        assert!(collapsed.get(), "narrow → hamburger");

        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(!collapsed.get(), "wide again → inline");
    }

    #[test]
    fn collapsible_click_hamburger_reveals_bar_overlay() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let children = t.children(mb);
        let (bar, hamburger) = (children[0], children[1]);
        assert!(!t.is_active(bar), "bar hidden before reveal");

        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(t.is_active(bar), "clicking the hamburger reveals the bar");
    }

    #[test]
    fn collapsible_reveal_focuses_first_trigger() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let hamburger = t.children(mb)[1];

        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));

        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        assert_eq!(triggers.len(), 2);
        assert_eq!(
            t.focused(),
            Some(triggers[0]),
            "revealing the bar focuses the first menu trigger"
        );
    }

    /// Regression: ArrowLeft must navigate to the PREVIOUS top-level menu
    /// in the revealed bar, not close the current one. The bar is itself a
    /// host overlay, so the dispatch-level "overlay back" key (ArrowLeft in
    /// LTR) must not mistake an open top-level menu for a nested submenu.
    #[test]
    fn collapsible_revealed_bar_left_navigates_not_closes() {
        let menu = |label: &'static str| {
            move || -> Box<dyn Widget> {
                Box::new(MenuList::new().item(crate::menu_item::MenuItem::new(lit!(label))))
            }
        };
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), menu("New"))
                .menu(lit!("&Edit"), menu("Undo"))
                .menu(lit!("&View"), menu("Zoom"))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let hamburger = t.children(mb)[1];
        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        let expanded = |t: &WidgetTree| -> Vec<bool> {
            triggers
                .iter()
                .map(|&id| t.accessibility_node(id).is_expanded())
                .collect()
        };

        // Open File → Edit → View via ArrowRight.
        t.press_key(Key::ArrowRight, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.press_key(Key::ArrowRight, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert_eq!(expanded(&t), vec![false, false, true], "RIGHT reached View");

        // ArrowLeft must move to Edit (the previous menu), NOT close View.
        t.press_key(Key::ArrowLeft, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert_eq!(
            expanded(&t),
            vec![false, true, false],
            "LEFT navigates to the previous menu (Edit), not closes"
        );

        // And once more to File.
        t.press_key(Key::ArrowLeft, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert_eq!(
            expanded(&t),
            vec![true, false, false],
            "LEFT again reaches File"
        );
    }

    /// Accessibility: the hamburger is a `Role::Button` whose `expanded`
    /// state tracks whether the bar is revealed (the ARIA disclosure
    /// pattern), and dismissing the bar restores focus to the hamburger.
    #[test]
    fn collapsible_hamburger_accessibility() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let hamburger = t.children(mb)[1];

        // Collapsed: a button that is NOT expanded.
        let info = t.accessibility_node(hamburger);
        assert_eq!(info.role(), Role::Button);
        assert!(
            !info.is_expanded(),
            "collapsed hamburger reports expanded=false"
        );

        // Revealed: expanded flips to true; the bar is a MenuBar landmark.
        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(
            t.accessibility_node(hamburger).is_expanded(),
            "revealed hamburger reports expanded=true"
        );
        assert!(first_descendant_with_role(&t, mb, Role::MenuBar).is_some());

        // Dismiss with Escape: expanded back to false, focus restored to
        // the hamburger (not lost in the now-hidden bar).
        t.press_key(Key::Escape, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(
            !t.accessibility_node(hamburger).is_expanded(),
            "collapsed again after Escape"
        );
        assert_eq!(
            t.focused(),
            Some(hamburger),
            "focus returns to the hamburger after the bar is dismissed"
        );
    }

    /// Regression: arrow-navigating between menus must NOT tear down the
    /// revealed bar. `MenuContext::open_at` calls `dismiss_all_except_hosts`;
    /// the bar overlay is marked `Role::MenuBar` (a host) via an access-role
    /// override, so it must survive — and its triggers keep valid (non-zero)
    /// bounds so dropdowns anchor under them, not at the window origin.
    #[test]
    fn collapsible_revealed_bar_survives_arrow_navigation() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .menu(lit!("&View"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let (bar, hamburger) = (t.children(mb)[0], t.children(mb)[1]);

        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(t.is_active(bar), "bar revealed");

        // Arrow-navigate to the next menu.
        t.press_key(Key::ArrowRight, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));

        assert!(
            t.is_active(bar),
            "bar must stay visible while navigating between menus"
        );
        // Triggers remain laid out inside the floating bar (offset from the
        // origin), so the opened dropdown anchors under a trigger.
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        let b = t.bounds(triggers[1]);
        assert!(
            b.width > 0.0 && (b.x > 0.0 || b.y > 0.0),
            "trigger stays laid out in the floating bar, not collapsed to the origin: {b:?}"
        );
    }

    #[test]
    fn collapsible_escape_hides_revealed_bar() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let children = t.children(mb);
        let (bar, hamburger) = (children[0], children[1]);

        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(t.is_active(bar));

        t.press_key(Key::Escape, Modifiers::NONE);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(!t.is_active(bar), "Escape hides the revealed bar");
    }

    #[test]
    fn collapsible_click_outside_hides_revealed_bar() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .menu(lit!("&Edit"), || Box::new(MenuList::new()))
                .collapse_policy(CollapsePolicy::Always),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        let children = t.children(mb);
        let (bar, hamburger) = (children[0], children[1]);

        t.click(hamburger);
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(t.is_active(bar));

        // Click well below the top bar strip — outside the overlay.
        t.pointer_down_button(
            bastyde_canvas::Point::new(400.0, 400.0),
            bastyde_core::event::PointerButton::Primary,
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        assert!(!t.is_active(bar), "click outside hides the revealed bar");
    }

    #[test]
    fn collapsible_bar_carries_menubar_role() {
        let mut t = collapsible_tree();
        let mb = t.add(
            MenuBar::new()
                .menu(lit!("&File"), || Box::new(MenuList::new()))
                .collapsible(),
        );
        t.layout(SizeProposal::exact(800.0, 100.0));
        t.layout(SizeProposal::exact(800.0, 100.0));
        // In collapsible mode the MenuBar landmark moves onto the bar
        // content node (so it travels into the floating overlay and is
        // treated as a host surface). It is still reachable as a descendant.
        assert!(
            first_descendant_with_role(&t, mb, Role::MenuBar).is_some(),
            "the bar content node carries Role::MenuBar"
        );
    }

    #[test]
    fn collapsible_dispatcher_injects_reveal_only_when_collapsed() {
        let collapsed = Signal::new(true);
        let reveal: MenubarReveal = std::rc::Rc::new(|_| {});
        let d = CollapsibleMenuBarDispatcher {
            inner: MenuBarDispatcher {
                trigger_ids: vec![fake_id(10)],
                mnemonic_table: HashMap::new(),
            },
            collapsed: collapsed.clone(),
            reveal,
        };

        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F10,
            modifiers: Modifiers::NONE,
        });
        assert!(
            matches!(
                action,
                Some(MenubarAction::FocusTrigger {
                    reveal: Some(_),
                    ..
                })
            ),
            "collapsed → reveal attached"
        );

        collapsed.set(false);
        let action = d.try_handle(&MenubarKeyEvent {
            key: Key::F10,
            modifiers: Modifiers::NONE,
        });
        assert!(
            matches!(
                action,
                Some(MenubarAction::FocusTrigger { reveal: None, .. })
            ),
            "expanded → no reveal (classic inline behaviour)"
        );
    }

    #[test]
    fn from_model_builds_in_window_triggers() {
        use crate::menu::{MenuEntry, MenuModel};
        let model = MenuModel::new()
            .menu(lit!("&File"), |m| {
                m.item(MenuEntry::new(lit!("&New")).intent("app.new"))
                    .separator()
                    .item(MenuEntry::new(lit!("&Quit")).intent("app.quit"))
            })
            .menu(lit!("&Edit"), |m| {
                m.item(MenuEntry::new(lit!("Cu&t")).intent("app.cut"))
            });

        let mut t = tree_with_window();
        let mb = t.add(MenuBar::from_model(model));
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));

        // Two top-level menus → two triggers, names mnemonic-stripped.
        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        assert_eq!(triggers.len(), 2);
        assert_eq!(t.accessibility_node(triggers[0]).name(), Some("File"));
        assert_eq!(t.accessibility_node(triggers[1]).name(), Some("Edit"));
    }

    #[test]
    fn runtime_model_mutation_rebuilds_in_window_bar() {
        use crate::menu::{MenuEntry, MenuModel};
        let model = MenuModel::new()
            .menu(lit!("&File"), |m| m.item(MenuEntry::new(lit!("&New"))));
        let model_handle = model.clone();

        let mut t = tree_with_window();
        let mb = t.add(MenuBar::from_model(model));
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert_eq!(collect_descendants_with_role(&t, mb, Role::MenuItem).len(), 1);

        // Add a top-level menu at runtime → version bump → Rebuild binding →
        // the next layout re-derives the in-window triggers.
        model_handle.push_menu(lit!("&Edit"), |m| m.item(MenuEntry::new(lit!("Cu&t"))));
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert_eq!(collect_descendants_with_role(&t, mb, Role::MenuItem).len(), 2);

        // Remove it again.
        let nodes_ids: Vec<_> = {
            model_handle
                .nodes()
                .iter()
                .filter_map(|n| match n {
                    crate::menu::MenuNode::Submenu { id, title, .. }
                        if title.resolve_now().contains("Edit") =>
                    {
                        Some(*id)
                    }
                    _ => None,
                })
                .collect()
        };
        assert!(model_handle.remove(nodes_ids[0]));
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));
        assert_eq!(collect_descendants_with_role(&t, mb, Role::MenuItem).len(), 1);
    }

    #[test]
    fn native_suppress_hides_in_window_bar_on_macos() {
        use crate::menu::{MenuEntry, MenuModel, NativeMenuMode};
        let model = MenuModel::new().menu(lit!("&File"), |m| {
            m.item(MenuEntry::new(lit!("&New")).intent("app.new"))
        });
        let mut t = tree_with_window();
        let mb = t.add(MenuBar::from_model(model).native_on_macos(NativeMenuMode::Suppress));
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));

        let triggers = collect_descendants_with_role(&t, mb, Role::MenuItem);
        if cfg!(target_os = "macos") {
            // Suppressed: the OS menu bar carries the menus, no in-window triggers.
            assert!(triggers.is_empty(), "macOS Suppress renders no in-window triggers");
        } else {
            // Other platforms ignore the flag and render the in-window bar.
            assert_eq!(triggers.len(), 1);
        }
    }

    /// End-to-end coverage of the model→native bridge (`menu::native::install`)
    /// via the recording `MemoryNativeMenuBackend` — the testable half of the
    /// native path (the `NSMenu` core itself needs a live AppKit loop). Verifies
    /// title stripping, check state, the auto-injected localized app menu, and
    /// that standard-menu labels go through i18n (no hardcoded English).
    #[cfg(target_os = "macos")]
    #[test]
    fn native_install_records_localized_snapshot() {
        use crate::menu::{MenuEntry, MenuModel, NativeMenuMode, StandardMenu};
        use bastyde_core::AppEventPoster;
        use bastyde_platform::native_menu::{
            MemoryNativeMenuBackend, NativeCheck, NativeMenuHandle, NativeMenuNode, StandardMenuRole,
        };
        use std::any::{Any, TypeId};
        use std::collections::HashMap;
        use std::sync::Arc;

        struct NullPoster;
        impl AppEventPoster for NullPoster {
            fn post_subscription_event(
                &self,
                _: bastyde_core::SubscriptionId,
                _: Box<dyn Any + Send>,
            ) {
            }
            fn post_external(&self, _: Box<dyn Any + Send>) {}
        }

        let grid = Signal::new(true);
        let model = MenuModel::new()
            // App menu with a localized Quit — must NOT be hardcoded English.
            .standard_menu(StandardMenu::app().quit(lit!("Quitter")))
            .menu(lit!("&File"), |m| {
                m.item(MenuEntry::new(lit!("&New")).intent("app.new"))
                    .item(MenuEntry::new(lit!("Show &Grid")).checkable(grid.clone()))
            });

        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let mut app_state: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        app_state.insert(TypeId::of::<NativeMenuHandle>(), Box::new(handle));
        let poster: Arc<dyn AppEventPoster> = Arc::new(NullPoster);

        let mut t = tree_with_window();
        t.set_app_context(Rc::new(
            bastyde_core::event_source::TreeAppContext::empty()
                .with_app_state(app_state)
                .with_poster(poster),
        ));
        let _mb = t.add(MenuBar::from_model(model).native_on_macos(NativeMenuMode::Coexist));
        t.layout(bastyde_canvas::SizeProposal::exact(800.0, 100.0));

        let snap = backend
            .menu_for(BastydeWindowId::new(1))
            .expect("native snapshot recorded for the window");

        // App menu is first, with the localized Quit label (not "Quit").
        match &snap.roots[0] {
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                labels,
            } => {
                assert_eq!(labels.quit, "Quitter", "Quit label routes through i18n");
                assert_eq!(labels.about, "About", "default About label resolved");
            }
            other => panic!("expected leading App menu, got {other:?}"),
        }

        // File submenu: mnemonics stripped, checkable reflects the bound signal.
        let file = snap
            .roots
            .iter()
            .find_map(|n| match n {
                NativeMenuNode::Submenu { title, children } if title == "File" => Some(children),
                _ => None,
            })
            .expect("File submenu in snapshot");
        assert!(
            file.iter()
                .any(|n| matches!(n, NativeMenuNode::Item { title, .. } if title == "New")),
            "New item present, '&' stripped"
        );
        assert!(
            file.iter().any(|n| matches!(
                n,
                NativeMenuNode::Item { title, check: NativeCheck::On, .. } if title == "Show Grid"
            )),
            "checkable item reflects the bound signal (On) with stripped title"
        );
    }
}
