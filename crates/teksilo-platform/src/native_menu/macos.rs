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
//! A minimal application menu (About / Hide / Quit) is auto-prepended unless the
//! snapshot already declares a [`StandardMenuRole::App`] root, so the app always
//! has the conventional first menu with a working ⌘Q.

use std::collections::HashMap;
use std::sync::Arc;

use teksilo_core::AppEventPoster;
use teksilo_core::MenuItemId;
use teksilo_core::window::TeksiloWindowId;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSControlStateValue, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{NSString, ns_string};

use super::{
    MenuItemDelta, NativeCheck, NativeKeyEquivalent, NativeMenuBackend, NativeMenuEventPayload,
    NativeMenuNode, NativeMenuSnapshot, StandardLabels, StandardMenuRole,
};

// ============================================================
// TeksiloMenuTarget — the action receiver for a window's items
// ============================================================

struct MenuTargetIvars {
    window_id: TeksiloWindowId,
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
            ivars.poster.post_external(Box::new(NativeMenuEventPayload {
                window_id_owner: ivars.window_id,
                item_id: MenuItemId::from_raw(tag as u64),
            }));
        }
    }
);

impl TeksiloMenuTarget {
    fn new(
        mtm: MainThreadMarker,
        window_id: TeksiloWindowId,
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
    /// Kept alive for the menu's lifetime (items reference it as their target).
    _target: Retained<TeksiloMenuTarget>,
    /// Leaf items by id, for `update_item`.
    items: HashMap<MenuItemId, Retained<NSMenuItem>>,
}

// ============================================================
// Backend
// ============================================================

/// macOS native-menu backend. See the module docs.
#[derive(Default)]
pub struct MacOsNativeMenuBackend {
    windows: HashMap<TeksiloWindowId, WindowMenu>,
    active: Option<TeksiloWindowId>,
}

impl MacOsNativeMenuBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_main_menu(&self, mtm: MainThreadMarker, window_id: TeksiloWindowId) {
        if let Some(wm) = self.windows.get(&window_id) {
            let app = NSApplication::sharedApplication(mtm);
            app.setMainMenu(Some(&wm.menu));
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
        let target = TeksiloMenuTarget::new(mtm, window_id, poster);
        let mut items = HashMap::new();
        let root = build_root_menu(mtm, &menu, &target, &mut items);

        self.windows.insert(
            window_id,
            WindowMenu {
                menu: root,
                _target: target,
                items,
            },
        );

        // First menu becomes active; an already-active window keeps focus.
        let make_active = match self.active {
            None => true,
            Some(active) => active == window_id,
        };
        if make_active {
            self.active = Some(window_id);
            self.apply_main_menu(mtm, window_id);
        }
    }

    fn activate_window(&mut self, window_id: TeksiloWindowId) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if self.windows.contains_key(&window_id) {
            self.active = Some(window_id);
            self.apply_main_menu(mtm, window_id);
        }
    }

    fn clear_window(&mut self, window_id: TeksiloWindowId) {
        self.windows.remove(&window_id);
        if self.active != Some(window_id) {
            return;
        }
        self.active = None;
        // The closed window owned the visible menu (its target object is now
        // dropped). Hand the bar to any remaining window, or clear it, so we
        // never leave `mainMenu` pointing at a dead target.
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(&next) = self.windows.keys().next() {
            self.active = Some(next);
            self.apply_main_menu(mtm, next);
        } else {
            NSApplication::sharedApplication(mtm).setMainMenu(None);
        }
    }

    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta) {
        // A `MenuModel` cloned across windows shares item ids, so update every
        // window that carries this item (single-window apps match exactly once).
        for wm in self.windows.values() {
            let Some(item) = wm.items.get(&id) else {
                continue;
            };
            if let Some(enabled) = delta.enabled {
                item.setEnabled(enabled);
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

/// Build the top-level `mainMenu`: a bar whose items each carry a submenu.
fn build_root_menu(
    mtm: MainThreadMarker,
    snapshot: &NativeMenuSnapshot,
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) -> Retained<NSMenu> {
    let bar = NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), ns_string!(""));
    bar.setAutoenablesItems(false);

    // The widget layer guarantees a leading App menu (with localized labels), so
    // the platform never fabricates user-visible strings of its own.
    for node in &snapshot.roots {
        match node {
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                labels,
                quit_item,
                settings_item,
            } => {
                bar.addItem(&app_menu_item(
                    mtm,
                    labels,
                    *quit_item,
                    *settings_item,
                    target,
                    items,
                ));
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
                ));
                sub.addItem(&standard_item(mtm, &labels.zoom, sel!(performZoom:), ""));
                sub.addItem(&NSMenuItem::separatorItem(mtm));
                item.setSubmenu(Some(&sub));
                bar.addItem(&item);
                NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&sub));
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
                item.setSubmenu(Some(&sub));
                bar.addItem(&item);
                NSApplication::sharedApplication(mtm).setHelpMenu(Some(&sub));
            }
            NativeMenuNode::Submenu { title, children } => {
                let item = top_level_item(mtm, title);
                let sub = build_submenu(mtm, title, children, target, items);
                item.setSubmenu(Some(&sub));
                bar.addItem(&item);
            }
            // A leaf or separator at top level isn't meaningful in a menu bar;
            // skip it (the bar holds submenus only).
            NativeMenuNode::Item { .. } | NativeMenuNode::Separator => {}
        }
    }

    bar
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
                check,
            } => {
                let item = leaf_item(mtm, title, target);
                item.setTag(id.raw() as isize);
                item.setEnabled(*enabled);
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
    menu
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

/// Build the conventional macOS application menu (About / Hide / Quit) from
/// localized `labels`. About and Hide target the standard responder-chain
/// selectors, so they work without any app wiring.
///
/// Quit has two shapes. With `quit_item` `None` it is `terminate:` like its
/// neighbours — the behaviour every app gets for free, including one that
/// declares no menu model at all. With `Some(id)` it becomes an ordinary routed
/// item under that id, keeping ⌘Q, so an app that must ask before exiting hears
/// about it; see `NativeMenuNode::Standard::quit_item` for why an app-side ⌘Q
/// shortcut cannot do that job itself.
fn app_menu_item(
    mtm: MainThreadMarker,
    labels: &StandardLabels,
    quit_item: Option<MenuItemId>,
    settings_item: Option<MenuItemId>,
    target: &TeksiloMenuTarget,
    items: &mut HashMap<MenuItemId, Retained<NSMenuItem>>,
) -> Retained<NSMenuItem> {
    let bar_item = top_level_item(mtm, &labels.title);
    let menu = NSMenu::initWithTitle(mtm.alloc::<NSMenu>(), &NSString::from_str(&labels.title));
    menu.setAutoenablesItems(false);

    menu.addItem(&standard_item(
        mtm,
        &labels.about,
        sel!(orderFrontStandardAboutPanel:),
        "",
    ));
    // Settings sits directly under About in its own group — the placement every
    // Mac app shares, and the one users reach for without looking. Routed or
    // absent: AppKit has no selector that opens an arbitrary app's settings, so
    // an unrouted slot could only ever render a dead row.
    if let Some(id) = settings_item {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let item = leaf_item(mtm, &labels.settings, target);
        item.setTag(id.raw() as isize);
        // `setAutoenablesItems(false)` above means nothing enables this for us.
        item.setEnabled(true);
        apply_key_equiv(
            &item,
            Some(&NativeKeyEquivalent {
                key: ",".to_string(),
                command: true,
                shift: false,
                alt: false,
                control: false,
            }),
        );
        menu.addItem(&item);
        items.insert(id, item);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&standard_item(mtm, &labels.hide, sel!(hide:), "h"));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    match quit_item {
        None => menu.addItem(&standard_item(mtm, &labels.quit, sel!(terminate:), "q")),
        Some(id) => {
            let item = leaf_item(mtm, &labels.quit, target);
            item.setTag(id.raw() as isize);
            // `setAutoenablesItems(false)` above means nothing enables this for
            // us — an item left disabled would swallow ⌘Q silently, which is the
            // one failure this whole path exists to rule out.
            item.setEnabled(true);
            apply_key_equiv(
                &item,
                Some(&NativeKeyEquivalent {
                    key: "q".to_string(),
                    command: true,
                    shift: false,
                    alt: false,
                    control: false,
                }),
            );
            menu.addItem(&item);
            // Registered like any routed item so a later `update_item` — a
            // relabel on a locale change, say — can still find it.
            items.insert(id, item);
        }
    }

    bar_item.setSubmenu(Some(&menu));
    bar_item
}

/// A leaf item bound to a standard responder-chain selector (target nil so the
/// action travels up to NSApp).
fn standard_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
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
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    }
    item
}
