// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS native menu backend.
//!
//! Mirrors a [`NativeMenuSnapshot`] into the global menu bar via
//! `NSApplication.setMainMenu:`. Each window's menu is built once and stored;
//! `activate_window` swaps which one is the visible `mainMenu` so the bar
//! follows window focus.
//!
//! Each leaf `NSMenuItem` carries the logical [`MenuItemId`] in its `tag`, and
//! targets a per-window `TeksiloMenuTarget` object whose action selector reads
//! that tag and posts a [`NativeMenuEventPayload`] through the app's
//! [`AppEventPoster`]. `teksilo-app` routes the payload back into the window's
//! widget tree, where the item's intent / action fires.
//!
//! The conventional application menu (About, Settings, Services, Hide, Hide
//! Others, Show All, Quit) comes from the
//! snapshot's [`StandardMenuRole::App`] root, which the widget layer guarantees:
//! it injects one with localized labels when the model declares none, so this
//! backend never fabricates user-visible strings of its own, and the app always
//! has the conventional first menu with a working ⌘Q. The backend puts that
//! root first in the bar wherever the model declares it, since AppKit shows
//! the bar's first menu as the application menu whatever it holds.

use std::collections::HashMap;
use std::sync::Arc;

use teksilo_core::AppEventPoster;
use teksilo_core::MenuItemId;
use teksilo_core::window::TeksiloWindowId;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValue, NSEventModifierFlags, NSMenu, NSMenuItem, NSWindow,
};
use objc2_foundation::{NSString, ns_string};

use super::{
    MacAppMenuRow, MenuItemDelta, NativeAppMenuEventPayload, NativeCheck, NativeKeyEquivalent,
    NativeMenuBackend, NativeMenuEventPayload, NativeMenuNode, NativeMenuSnapshot, StandardLabels,
    StandardMenuRole, StandardRoutedItem, bar_after_app_menu, bar_after_clear, key_window_bar,
    mac_app_menu_rows, mac_bar_plan, mac_relists_window,
};

// ============================================================
// TeksiloMenuTarget — the action receiver for a window's items
// ============================================================

struct MenuTargetIvars {
    /// The window whose menu this is; `None` for the application menu.
    window_id: Option<TeksiloWindowId>,
    poster: Arc<dyn AppEventPoster>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TeksiloMenuTarget"]
    #[ivars = MenuTargetIvars]
    struct TeksiloMenuTarget;

    unsafe impl NSObjectProtocol for TeksiloMenuTarget {}

    impl TeksiloMenuTarget {
        // Fired by AppKit when a menu item with this target is chosen. The
        // sender's `tag` is the logical MenuItemId; post it back to the loop.
        #[unsafe(method(teksiloMenuAction:))]
        fn menu_action(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            if tag <= 0 {
                return;
            }
            let ivars = self.ivars();
            let item_id = MenuItemId::from_raw(tag as u64);
            match ivars.window_id {
                Some(window_id_owner) => ivars.poster.post_external(Box::new(
                    NativeMenuEventPayload {
                        window_id_owner,
                        item_id,
                    },
                )),
                None => ivars
                    .poster
                    .post_external(Box::new(NativeAppMenuEventPayload { item_id })),
            }
        }
    }
);

impl TeksiloMenuTarget {
    fn new(
        mtm: MainThreadMarker,
        window_id: Option<TeksiloWindowId>,
        poster: Arc<dyn AppEventPoster>,
    ) -> Retained<Self> {
        let this = mtm
            .alloc::<Self>()
            .set_ivars(MenuTargetIvars { window_id, poster });
        unsafe { msg_send![super(this), init] }
    }
}

// ============================================================
// Per-window record
// ============================================================

struct WindowMenu {
    menu: Retained<NSMenu>,
    /// The submenu AppKit keeps the window list in while this bar shows:
    /// the bar's Window menu, or a hidden one. See [`register_windows_menu`].
    windows_menu: Retained<NSMenu>,
    /// The bar's Help menu, registered as `NSApp.helpMenu` while it shows.
    help_menu: Option<Retained<NSMenu>>,
    /// The App menu's Services submenu, registered as `NSApp.servicesMenu`
    /// while the bar shows, so AppKit fills it.
    services_menu: Option<Retained<NSMenu>>,
    /// Kept alive for the menu's lifetime (items reference it as their target).
    _target: Retained<TeksiloMenuTarget>,
    /// Leaf items by id, for `update_item`.
    items: HashMap<MenuItemId, Retained<NSMenuItem>>,
}

impl WindowMenu {
    /// Make this bar `mainMenu`, with the menus AppKit fills in itself.
    fn show(&self, mtm: MainThreadMarker) {
        let app = NSApplication::sharedApplication(mtm);
        app.setMainMenu(Some(&self.menu));
        app.setHelpMenu(self.help_menu.as_deref());
        if let Some(services) = &self.services_menu {
            app.setServicesMenu(Some(services));
        }
        register_windows_menu(&app, &self.windows_menu);
    }
}

/// Register `menu` as the one AppKit keeps the window list in, and fill it.
///
/// AppKit adds a window's row to the registered `windowsMenu` when the
/// window is ordered in, and removes it when the window closes; the Dock
/// icon's menu lists the same windows. A window ordered in while another
/// menu was registered, or none (the window a mount action opens at
/// startup, before the first bar exists), is never added by itself. So, as
/// Qt does in `insertWindowMenu`, flip `excludedFromWindowsMenu` on every
/// listed window, which makes AppKit evaluate it again: once while the
/// outgoing menu is still registered, so its rows go away and a window that
/// closes while it is not registered leaves no stale row in it, and once
/// with `menu` registered, so every window that belongs there is added. A
/// window excluded on purpose (a dialog) is left alone, and so is a hidden
/// one (ordered out, not miniaturized), which the flip would list: see
/// [`mac_relists_window`].
fn register_windows_menu(app: &NSApplication, menu: &NSMenu) {
    if app
        .windowsMenu()
        .is_some_and(|current| std::ptr::eq(&*current, menu))
    {
        return;
    }
    let listed: Vec<Retained<NSWindow>> = app
        .windows()
        .iter()
        .filter(|window| {
            mac_relists_window(
                window.isExcludedFromWindowsMenu(),
                window.isVisible(),
                window.isMiniaturized(),
            )
        })
        .collect();
    for window in &listed {
        window.setExcludedFromWindowsMenu(true);
    }
    app.setWindowsMenu(Some(menu));
    for window in &listed {
        window.setExcludedFromWindowsMenu(false);
    }
}

// ============================================================
// Backend
// ============================================================

/// macOS native-menu backend. See the module docs.
#[derive(Default)]
pub struct MacOsNativeMenuBackend {
    windows: HashMap<TeksiloWindowId, WindowMenu>,
    /// The window whose own menu is `mainMenu`; `None` while the
    /// application menu is.
    active: Option<TeksiloWindowId>,
    /// The last window `activate_window` named: the key window.
    key: Option<TeksiloWindowId>,
    /// Shown while no window's menu is: see
    /// [`NativeMenuBackend::set_app_menu`].
    app: Option<WindowMenu>,
    /// Whether the application declared [`app`](Self::app), rather than
    /// `teksilo-app` supplying a default App menu.
    app_declared: bool,
    /// The window each window belongs to: see
    /// [`NativeMenuBackend::set_window_owner`].
    owners: HashMap<TeksiloWindowId, TeksiloWindowId>,
}

impl MacOsNativeMenuBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// The bar shown while `key` is key, by [`key_window_bar`].
    fn bar_for(&self, key: TeksiloWindowId) -> Option<TeksiloWindowId> {
        key_window_bar(
            key,
            |window| self.windows.contains_key(&window),
            |window| self.owners.get(&window).copied(),
            self.app_declared,
            self.active,
        )
    }

    /// Show `bar` (`None`: the application menu) and remember it.
    fn show(&mut self, mtm: MainThreadMarker, bar: Option<TeksiloWindowId>) {
        self.active = bar;
        match bar {
            Some(window_id) => self.apply_main_menu(mtm, window_id),
            None => self.apply_app_menu(mtm),
        }
    }

    fn apply_main_menu(&self, mtm: MainThreadMarker, window_id: TeksiloWindowId) {
        if let Some(wm) = self.windows.get(&window_id) {
            wm.show(mtm);
        }
    }

    /// Show the application menu, or no menu when the app set none.
    fn apply_app_menu(&self, mtm: MainThreadMarker) {
        match &self.app {
            Some(app) => app.show(mtm),
            None => NSApplication::sharedApplication(mtm).setMainMenu(None),
        }
    }

    fn build(
        mtm: MainThreadMarker,
        window_id: Option<TeksiloWindowId>,
        menu: &NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
    ) -> WindowMenu {
        let target = TeksiloMenuTarget::new(mtm, window_id, poster);
        let mut items = HashMap::new();
        let bar = build_root_menu(mtm, menu, &target, &mut items);
        WindowMenu {
            menu: bar.menu,
            windows_menu: bar.windows_menu,
            help_menu: bar.help_menu,
            services_menu: bar.services_menu,
            _target: target,
            items,
        }
    }
}

impl NativeMenuBackend for MacOsNativeMenuBackend {
    fn set_window_menu(
        &mut self,
        window_id: TeksiloWindowId,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        self.windows
            .insert(window_id, Self::build(mtm, Some(window_id), &menu, poster));

        // The bar the key window shows (this menu when it is the key
        // window's or its owner's), and a window's menu set before any
        // window was activated (the first window's, built before its first
        // `Focused`).
        let bar = match self.key {
            Some(key) => self.bar_for(key),
            None => Some(window_id),
        };
        if bar == Some(window_id) || bar != self.active {
            self.show(mtm, bar);
        }
    }

    fn activate_window(&mut self, window_id: TeksiloWindowId) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        self.key = Some(window_id);
        // Its own menu, its owner's, the application menu, or, with none
        // declared, the bar shown before: see `key_window_bar`.
        let bar = self.bar_for(window_id);
        self.show(mtm, bar);
    }

    fn set_window_owner(&mut self, window_id: TeksiloWindowId, owner: Option<TeksiloWindowId>) {
        match owner {
            Some(owner) => self.owners.insert(window_id, owner),
            None => self.owners.remove(&window_id),
        };
    }

    fn clear_window(&mut self, window_id: TeksiloWindowId) {
        self.windows.remove(&window_id);
        self.owners.remove(&window_id);
        // When the closed window owned the visible menu (its target object is
        // now dropped), it never stays up, even with no application menu
        // declared: the key window's bar if another window is key, else the
        // application menu, until the window AppKit makes key next is
        // activated; an app left with no window keeps the application menu.
        let Some(bar) = bar_after_clear(
            window_id,
            self.key,
            self.active,
            |window| self.windows.contains_key(&window),
            |window| self.owners.get(&window).copied(),
            self.app_declared,
        ) else {
            return;
        };
        match MainThreadMarker::new() {
            Some(mtm) => self.show(mtm, bar),
            None => self.active = None,
        }
    }

    fn set_app_menu(
        &mut self,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
        declared: bool,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        self.app = Some(Self::build(mtm, None, &menu, poster));
        self.app_declared = declared;
        // The key window's bar again: the new application menu when that is
        // its pick, which it now is for a key window with no menu of its own
        // whose bar was kept only while no application menu was declared.
        let key_bar = self.key.map(|key| self.bar_for(key));
        if let Some(bar) = bar_after_app_menu(key_bar, self.active) {
            self.show(mtm, bar);
        }
    }

    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta) {
        // A `MenuModel` cloned across windows shares item ids, so update every
        // menu that carries this item, the application menu's included.
        for wm in self.windows.values().chain(self.app.as_ref()) {
            let Some(item) = wm.items.get(&id) else {
                continue;
            };
            if let Some(enabled) = delta.enabled {
                item.setEnabled(enabled);
            }
            if let Some(visible) = delta.visible {
                item.setHidden(!visible);
            }
            if let Some(check) = delta.check {
                item.setState(control_state(check));
            }
            if let Some(title) = &delta.title {
                item.setTitle(&NSString::from_str(title));
            }
            if let Some(key_equiv) = &delta.key_equiv {
                apply_key_equiv(item, key_equiv.as_ref());
            }
        }
    }
}

// ============================================================
// Menu construction
// ============================================================

/// A built bar and the submenus AppKit is told about while it shows.
struct BuiltBar {
    menu: Retained<NSMenu>,
    windows_menu: Retained<NSMenu>,
    help_menu: Option<Retained<NSMenu>>,
    services_menu: Option<Retained<NSMenu>>,
}

/// Build the top-level `mainMenu`: a bar whose items each carry a submenu.
fn build_root_menu(
    mtm: MainThreadMarker,
    snapshot: &NativeMenuSnapshot,
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) -> BuiltBar {
    let bar = NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!(""));
    bar.setAutoenablesItems(false);
    let plan = mac_bar_plan(snapshot);
    let mut windows_menu = None;
    let mut help_menu = None;
    let mut services_menu = None;

    // The widget layer guarantees an App menu (with localized labels), so
    // the platform never fabricates user-visible strings of its own; the
    // plan puts it first, where AppKit shows the application menu.
    for (index, node) in plan.menus.iter().copied().enumerate() {
        match node {
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                labels,
                quit_item,
                settings_item,
            } => {
                let (item, services) = app_menu_item(
                    mtm,
                    labels,
                    quit_item.as_ref(),
                    settings_item.as_ref(),
                    target,
                    items,
                );
                bar.addItem(&item);
                services_menu = Some(services);
            }
            NativeMenuNode::Standard {
                role: StandardMenuRole::Window,
                labels,
                ..
            } => {
                let item = top_level_item(mtm, &labels.title);
                let sub = NSMenu::initWithTitle(
                    mtm.alloc::<NSMenu>(),
                    &NSString::from_str(&labels.title),
                );
                sub.setAutoenablesItems(false);
                // Standard window-management items (localized titles, system
                // selectors), then the live window list AppKit maintains.
                sub.addItem(&standard_item(
                    mtm,
                    &labels.minimize,
                    sel!(performMiniaturize:),
                    "m",
                    NSEventModifierFlags::Command,
                ));
                sub.addItem(&standard_item(
                    mtm,
                    &labels.zoom,
                    sel!(performZoom:),
                    "",
                    NSEventModifierFlags::Command,
                ));
                sub.addItem(&NSMenuItem::separatorItem(mtm));
                // The app's own Window rows, between Zoom and the window
                // list, set apart from the list.
                let own = &plan.folded[index];
                if !own.is_empty() {
                    fill_menu(mtm, &sub, own.iter().copied(), target, items);
                    sub.addItem(&NSMenuItem::separatorItem(mtm));
                }
                item.setSubmenu(Some(&sub));
                bar.addItem(&item);
                if plan.windows_menu == Some(index) {
                    windows_menu = Some(sub);
                }
            }
            NativeMenuNode::Standard {
                role: StandardMenuRole::Help,
                labels,
                ..
            } => {
                let item = top_level_item(mtm, &labels.title);
                let sub = NSMenu::initWithTitle(
                    mtm.alloc::<NSMenu>(),
                    &NSString::from_str(&labels.title),
                );
                sub.setAutoenablesItems(false);
                // The app's own Help rows; AppKit adds its search field above.
                fill_menu(mtm, &sub, plan.folded[index].iter().copied(), target, items);
                item.setSubmenu(Some(&sub));
                bar.addItem(&item);
                if plan.help_menu == Some(index) {
                    help_menu = Some(sub);
                }
            }
            NativeMenuNode::Submenu { title, children } => {
                let item = top_level_item(mtm, title);
                let sub = build_submenu(mtm, title, children, target, items);
                item.setSubmenu(Some(&sub));
                bar.addItem(&item);
            }
            // The plan leaves leaves and separators out of the bar.
            NativeMenuNode::Item { .. } | NativeMenuNode::Separator => {}
        }
    }

    // A bar with no Window menu still registers one, hidden, so the Dock
    // icon's menu lists the windows (Qt's `insertWindowMenu` does the same).
    let windows_menu = windows_menu.unwrap_or_else(|| {
        let item = top_level_item(mtm, "");
        item.setHidden(true);
        let sub = NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!(""));
        sub.setAutoenablesItems(false);
        // `setWindowsMenu:` reads the menu's last item without checking for
        // an empty menu (Qt works around FB13369198 the same way).
        let dummy = top_level_item(mtm, "");
        dummy.setHidden(true);
        sub.addItem(&dummy);
        item.setSubmenu(Some(&sub));
        bar.addItem(&item);
        sub
    });

    BuiltBar {
        menu: bar,
        windows_menu,
        help_menu,
        services_menu,
    }
}

fn build_submenu(
    mtm: MainThreadMarker,
    title: &str,
    nodes: &[NativeMenuNode],
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), &NSString::from_str(title));
    menu.setAutoenablesItems(false);
    fill_menu(mtm, &menu, nodes, target, items);
    menu
}

/// Append `nodes` to `menu`, recording each leaf under its id.
fn fill_menu<'a>(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    nodes: impl IntoIterator<Item = &'a NativeMenuNode>,
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) {
    for node in nodes {
        match node {
            NativeMenuNode::Separator => {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
            }
            NativeMenuNode::Item {
                id,
                title,
                key_equiv,
                enabled,
                visible,
                check,
            } => {
                let item = leaf_item(mtm, title, target);
                item.setTag(id.raw() as isize);
                item.setEnabled(*enabled);
                item.setHidden(!*visible);
                item.setState(control_state(*check));
                apply_key_equiv(&item, key_equiv.as_ref());
                menu.addItem(&item);
                items.insert(*id, item);
            }
            NativeMenuNode::Submenu { title, children } => {
                let item = top_level_item(mtm, title);
                let sub = build_submenu(mtm, title, children, target, items);
                item.setSubmenu(Some(&sub));
                menu.addItem(&item);
            }
            // Standard roles only make sense at the top level.
            NativeMenuNode::Standard { .. } => {}
        }
    }
}

/// A top-level / submenu-parent item (no action, just a title + submenu).
fn top_level_item(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    let item = mtm.alloc::<NSMenuItem>();
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            item,
            &NSString::from_str(title),
            None,
            ns_string!(""),
        )
    }
}

/// A leaf command item targeting the window's action receiver.
fn leaf_item(
    mtm: MainThreadMarker,
    title: &str,
    target: &TeksiloMenuTarget,
) -> Retained<NSMenuItem> {
    let item = mtm.alloc::<NSMenuItem>();
    let item: Retained<NSMenuItem> = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            item,
            &NSString::from_str(title),
            Some(sel!(teksiloMenuAction:)),
            ns_string!(""),
        )
    };
    unsafe {
        item.setTarget(Some(target));
    }
    // A hidden row's chord falls through to the window, whose shortcut
    // dispatcher fires it as it would with no native menu, the way it did
    // when a hidden row was left out of the menu. Set rather than left to
    // AppKit's default, so it does not depend on the SDK the app links.
    item.setAllowsKeyEquivalentWhenHidden(false);
    item
}

fn apply_key_equiv(item: &NSMenuItem, key: Option<&NativeKeyEquivalent>) {
    match key {
        None => {
            item.setKeyEquivalent(ns_string!(""));
        }
        Some(k) => {
            item.setKeyEquivalent(&NSString::from_str(&k.key));
            let mut mask = NSEventModifierFlags::empty();
            if k.command {
                mask |= NSEventModifierFlags::Command;
            }
            if k.shift {
                mask |= NSEventModifierFlags::Shift;
            }
            if k.alt {
                mask |= NSEventModifierFlags::Option;
            }
            if k.control {
                mask |= NSEventModifierFlags::Control;
            }
            item.setKeyEquivalentModifierMask(mask);
        }
    }
}

fn control_state(check: NativeCheck) -> NSControlStateValue {
    // `NSControlStateValue` is a type alias for `NSInteger` (isize):
    // On = 1, Off = 0, Mixed = -1.
    match check {
        NativeCheck::On => 1,
        NativeCheck::Mixed => -1,
        NativeCheck::Off | NativeCheck::None => 0,
    }
}

/// Build the conventional macOS application menu from localized `labels`,
/// with the rows of [`mac_app_menu_rows`] in AppKit's order. About, Services,
/// Hide, Hide Others and Show All target the standard responder-chain
/// selectors, so they work without any app wiring. Returns the bar item and
/// the Services submenu, which AppKit fills once it is registered as
/// `NSApp.servicesMenu` (see [`WindowMenu::show`]).
///
/// Quit has two shapes. With `quit_item` `None` it is `terminate:` like its
/// neighbours, for a snapshot built by other means. With `Some(id)`, which
/// the widget layer always sends, it becomes an ordinary routed item under
/// that id, keeping ⌘Q, so the app's close guards hear about it; see
/// `NativeMenuNode::Standard::quit_item` for why an app-side ⌘Q shortcut
/// cannot do that job itself.
fn app_menu_item(
    mtm: MainThreadMarker,
    labels: &StandardLabels,
    quit_item: Option<&StandardRoutedItem>,
    settings_item: Option<&StandardRoutedItem>,
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
    let bar_item = top_level_item(mtm, &labels.title);
    let menu = NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), &NSString::from_str(&labels.title));
    menu.setAutoenablesItems(false);
    // AppKit fills the Services menu and enables each service from the
    // current selection, so it keeps AppKit's own enabling; no delta of
    // the app's ever reaches it.
    let services =
        NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), &NSString::from_str(&labels.services));
    let command = NSEventModifierFlags::Command;

    for row in mac_app_menu_rows(settings_item.is_some()) {
        let item = match row {
            MacAppMenuRow::About => standard_item(
                mtm,
                &labels.about,
                sel!(orderFrontStandardAboutPanel:),
                "",
                command,
            ),
            // Settings sits directly under About in its own group, routed
            // or absent: AppKit has no selector that opens an arbitrary
            // app's settings, so an unrouted slot could only ever render a
            // dead row.
            MacAppMenuRow::Settings => match settings_item {
                Some(routed) => routed_item(mtm, &labels.settings, routed, target, items),
                None => continue,
            },
            MacAppMenuRow::Services => {
                let item = top_level_item(mtm, &labels.services);
                item.setSubmenu(Some(&services));
                item
            }
            MacAppMenuRow::Hide => standard_item(mtm, &labels.hide, sel!(hide:), "h", command),
            MacAppMenuRow::HideOthers => standard_item(
                mtm,
                &labels.hide_others,
                sel!(hideOtherApplications:),
                "h",
                command | NSEventModifierFlags::Option,
            ),
            MacAppMenuRow::ShowAll => standard_item(
                mtm,
                &labels.show_all,
                sel!(unhideAllApplications:),
                "",
                command,
            ),
            MacAppMenuRow::Quit => match quit_item {
                None => standard_item(mtm, &labels.quit, sel!(terminate:), "q", command),
                Some(routed) => routed_item(mtm, &labels.quit, routed, target, items),
            },
            MacAppMenuRow::Separator => NSMenuItem::separatorItem(mtm),
        };
        menu.addItem(&item);
    }

    bar_item.setSubmenu(Some(&menu));
    (bar_item, services)
}

/// A row inside a standard menu that the app routes: the window's action
/// receiver, the logical id on the tag, and whatever key equivalent the widget
/// layer resolved.
///
/// Shared by Quit and Settings, which differ only in label and placement — as
/// two hand-written copies they had already drifted into hardcoding a chord
/// each, which is the one thing a routed row must not do.
fn routed_item(
    mtm: MainThreadMarker,
    title: &str,
    routed: &StandardRoutedItem,
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) -> Retained<NSMenuItem> {
    let item = leaf_item(mtm, title, target);
    item.setTag(routed.id.raw() as isize);
    // `setAutoenablesItems(false)` on the enclosing menu means nothing enables
    // this for us — a disabled Quit would swallow ⌘Q silently, which is the one
    // failure this whole path exists to rule out.
    item.setEnabled(true);
    apply_key_equiv(&item, routed.key_equiv.as_ref());
    // Registered like any routed item so a later `update_item` — a relabel on a
    // locale change, say — can still find it.
    items.insert(routed.id, item.clone());
    item
}

/// A leaf item bound to a standard responder-chain selector (target nil so the
/// action travels up to NSApp), with `key` and `modifiers` as its key
/// equivalent (an empty `key` has none).
fn standard_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = mtm.alloc::<NSMenuItem>();
    let item: Retained<NSMenuItem> = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            item,
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    if !key.is_empty() {
        item.setKeyEquivalentModifierMask(modifiers);
    }
    item
}
