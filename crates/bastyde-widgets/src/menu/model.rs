// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`MenuModel`] data type and its builders.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::EventContext;
use bastyde_core::{Intent, MenuItemId};
use bastyde_data::CheckState;
use bastyde_i18n::LocalizedString;
use bastyde_platform::native_menu::StandardMenuRole;

use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;

/// Checkable / radio state for a menu item, mirroring the [`MenuItem`] modes.
#[derive(Clone)]
pub enum MenuItemState {
    /// A plain command, no check column.
    Plain,
    /// Two-state checkbox bound to a `Signal<bool>`; activation flips it.
    Check(Signal<bool>),
    /// Reflect-only checkmark mirroring a `Signal<bool>`; activation does NOT
    /// write it (the `intent`/`on_activate` owns the change). For commands that
    /// mirror externally-owned state — "View ▸ Sidebar / Full Screen".
    ReflectCheck(Signal<bool>),
    /// Tri-state checkbox bound to a `Signal<CheckState>`.
    TriCheck(Signal<CheckState>),
    /// Radio item: selected iff `selected == value`.
    Radio {
        /// This item's value within the group.
        value: usize,
        /// The shared selection signal.
        selected: Signal<usize>,
    },
}

/// One leaf command in the menu tree. Both the builder and the stored spec.
#[derive(Clone)]
pub struct MenuEntry {
    pub(crate) title: LocalizedString,
    pub(crate) intent: Option<&'static str>,
    pub(crate) action: Option<Rc<dyn Fn(&mut EventContext)>>,
    pub(crate) shortcut_id: Option<&'static str>,
    pub(crate) enabled: Prop<bool>,
    pub(crate) visible: Prop<bool>,
    pub(crate) state: MenuItemState,
    pub(crate) id: MenuItemId,
}

impl MenuEntry {
    /// Start a new leaf item with the given (possibly mnemonic-bearing,
    /// localized) title. Allocates a process-unique [`MenuItemId`].
    pub fn new(title: impl Into<LocalizedString>) -> Self {
        Self {
            title: title.into(),
            intent: None,
            action: None,
            shortcut_id: None,
            enabled: Prop::Static(true),
            visible: Prop::Static(true),
            state: MenuItemState::Plain,
            id: MenuItemId::next(),
        }
    }

    /// Fire this intent by name when the item is chosen (in-window or native).
    pub fn intent(mut self, name: &'static str) -> Self {
        self.intent = Some(name);
        self
    }

    /// Run this closure when the item is chosen. Runs after `intent`, if both
    /// are set. The escape hatch for behaviour that isn't a plain intent.
    pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.action = Some(Rc::new(f));
        self
    }

    /// Bind the displayed shortcut to a `ShortcutRegistry` entry by id. The
    /// in-window item shows the resolved chord; the native item gets a key
    /// equivalent (and the OS fires it directly).
    pub fn shortcut(mut self, id: &'static str) -> Self {
        self.shortcut_id = Some(id);
        self
    }

    /// Enabled state (static or signal-bound).
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Visibility (static or signal-bound). A hidden item collapses to zero
    /// height in the in-window menu (reactively); on the native menu it is
    /// omitted from the snapshot at build time (toggling it settles on the next
    /// menu rebuild — for fully-dynamic native menus prefer
    /// [`MenuModel::remove`] / [`MenuModel::push_item`]).
    pub fn visible(mut self, visible: impl Into<Prop<bool>>) -> Self {
        self.visible = visible.into();
        self
    }

    /// Make this a two-state checkbox item bound to `state`. Activation flips
    /// `state` — use when the signal *is* the source of truth.
    pub fn checkable(mut self, state: Signal<bool>) -> Self {
        self.state = MenuItemState::Check(state);
        self
    }

    /// Show a checkmark that **reflects** `state` read-only. Activation does not
    /// write it — pair with [`intent`](Self::intent) / [`on_activate`](Self::on_activate)
    /// that drive the change; the checkmark then follows `state` reactively. Use
    /// when the truth is owned elsewhere (e.g. `DockingModel::dock_open_signal`),
    /// where two-way [`checkable`](Self::checkable) would fight the model.
    pub fn checked(mut self, state: Signal<bool>) -> Self {
        self.state = MenuItemState::ReflectCheck(state);
        self
    }

    /// Make this a tri-state checkbox item bound to `state`.
    pub fn tri_checkable(mut self, state: Signal<CheckState>) -> Self {
        self.state = MenuItemState::TriCheck(state);
        self
    }

    /// Make this a radio item: selected iff `selected.get() == value`.
    pub fn radio(mut self, value: usize, selected: Signal<usize>) -> Self {
        self.state = MenuItemState::Radio { value, selected };
        self
    }

    /// The stable id of this item.
    pub fn id(&self) -> MenuItemId {
        self.id
    }

    /// Build the live [`MenuItem`] widget for the in-window menu.
    pub(crate) fn to_menu_item(&self) -> MenuItem {
        // Pass the enabled `Prop` through (not its current value) so a bound
        // signal greys the in-window item out reactively.
        let mut mi = MenuItem::new(self.title.clone()).enabled(self.enabled.clone());
        if let Some(id) = self.shortcut_id {
            mi = mi.for_shortcut(id);
        }
        let intent = self.intent;
        let action = self.action.clone();
        mi = mi.on_activate_fn(move |ctx| {
            if let Some(name) = intent {
                ctx.send_intent(Intent::new(name));
            }
            if let Some(a) = &action {
                a(ctx);
            }
        });
        match &self.state {
            MenuItemState::Plain => {}
            MenuItemState::Check(s) => mi = mi.bind_checked(s.clone()),
            MenuItemState::ReflectCheck(s) => mi = mi.reflect_checked(s.clone()),
            MenuItemState::TriCheck(s) => mi = mi.bind_check_state(s.clone()),
            MenuItemState::Radio { value, selected } => mi = mi.radio(*value, selected.clone()),
        }
        mi
    }
}

/// One node of the menu tree.
#[derive(Clone)]
pub enum MenuNode {
    /// A leaf command.
    Item(MenuEntry),
    /// A submenu.
    Submenu {
        /// Stable id, so the submenu can be addressed by runtime mutators
        /// ([`MenuModel::push_item`], [`MenuModel::remove`]).
        id: MenuItemId,
        /// Submenu title.
        title: LocalizedString,
        /// Child nodes.
        children: Vec<MenuNode>,
    },
    /// A separator line.
    Separator,
    /// A platform-standard menu (macOS App / Window / Help) with localized
    /// chrome. Rendered by the native backend; ignored by the in-window bar.
    Standard(StandardMenu),
}

/// A platform-standard menu (macOS App / Window / Help) with **localized**
/// labels. The framework wires the system selectors (About / Hide / Quit,
/// Minimize / Zoom); you supply the strings — defaults are English `lit!`s, so
/// pass `tr!`-resolved [`LocalizedString`]s for a localized app menu. This keeps
/// the OS menu bar inside the i18n net like every other widget.
#[derive(Clone)]
pub struct StandardMenu {
    role: StandardMenuRole,
    title: LocalizedString,
    about: LocalizedString,
    hide: LocalizedString,
    quit: LocalizedString,
    minimize: LocalizedString,
    zoom: LocalizedString,
}

impl StandardMenu {
    /// The application menu (About / Hide / Quit). `title` is the bold app-name
    /// submenu label — set it to your localized app name.
    pub fn app() -> Self {
        Self {
            role: StandardMenuRole::App,
            title: LocalizedString::literal("App"),
            about: LocalizedString::literal("About"),
            hide: LocalizedString::literal("Hide"),
            quit: LocalizedString::literal("Quit"),
            minimize: LocalizedString::literal(""),
            zoom: LocalizedString::literal(""),
        }
    }

    /// The Window menu (Minimize / Zoom + the live window list).
    pub fn window() -> Self {
        Self {
            role: StandardMenuRole::Window,
            title: LocalizedString::literal("Window"),
            about: LocalizedString::literal(""),
            hide: LocalizedString::literal(""),
            quit: LocalizedString::literal(""),
            minimize: LocalizedString::literal("Minimize"),
            zoom: LocalizedString::literal("Zoom"),
        }
    }

    /// The Help menu.
    pub fn help() -> Self {
        Self {
            role: StandardMenuRole::Help,
            title: LocalizedString::literal("Help"),
            about: LocalizedString::literal(""),
            hide: LocalizedString::literal(""),
            quit: LocalizedString::literal(""),
            minimize: LocalizedString::literal(""),
            zoom: LocalizedString::literal(""),
        }
    }

    /// Default standard menu for a role.
    pub fn for_role(role: StandardMenuRole) -> Self {
        match role {
            StandardMenuRole::App => Self::app(),
            StandardMenuRole::Window => Self::window(),
            StandardMenuRole::Help => Self::help(),
        }
    }

    /// This menu's role.
    pub fn role(&self) -> StandardMenuRole {
        self.role
    }

    /// Submenu title (the app name for `App`; the menu label for Window / Help).
    pub fn title(mut self, title: impl Into<LocalizedString>) -> Self {
        self.title = title.into();
        self
    }
    /// "About …" label (App).
    pub fn about(mut self, label: impl Into<LocalizedString>) -> Self {
        self.about = label.into();
        self
    }
    /// "Hide …" label (App).
    pub fn hide(mut self, label: impl Into<LocalizedString>) -> Self {
        self.hide = label.into();
        self
    }
    /// "Quit …" label (App).
    pub fn quit(mut self, label: impl Into<LocalizedString>) -> Self {
        self.quit = label.into();
        self
    }
    /// "Minimize" label (Window).
    pub fn minimize(mut self, label: impl Into<LocalizedString>) -> Self {
        self.minimize = label.into();
        self
    }
    /// "Zoom" label (Window).
    pub fn zoom(mut self, label: impl Into<LocalizedString>) -> Self {
        self.zoom = label.into();
        self
    }

    /// Resolve to the platform's localized-label struct (widget-layer i18n
    /// resolution happens here, so the platform never hardcodes English).
    pub(crate) fn resolve_labels(&self) -> bastyde_platform::native_menu::StandardLabels {
        bastyde_platform::native_menu::StandardLabels {
            title: self.title.resolve_now(),
            about: self.about.resolve_now(),
            hide: self.hide.resolve_now(),
            quit: self.quit.resolve_now(),
            minimize: self.minimize.resolve_now(),
            zoom: self.zoom.resolve_now(),
        }
    }
}

/// Builder for the contents of one (sub)menu — a sequence of items, separators,
/// and nested submenus.
#[derive(Clone, Default)]
pub struct MenuItems {
    pub(crate) nodes: Vec<MenuNode>,
}

impl MenuItems {
    /// An empty contents builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a leaf command.
    pub fn item(mut self, entry: MenuEntry) -> Self {
        self.nodes.push(MenuNode::Item(entry));
        self
    }

    /// Append a separator.
    pub fn separator(mut self) -> Self {
        self.nodes.push(MenuNode::Separator);
        self
    }

    /// Append a nested submenu (auto-assigned id).
    pub fn submenu(
        self,
        title: impl Into<LocalizedString>,
        build: impl FnOnce(MenuItems) -> MenuItems,
    ) -> Self {
        self.submenu_with_id(MenuItemId::next(), title, build)
    }

    /// Append a nested submenu with a caller-supplied id, so it can be
    /// addressed later by [`MenuModel::push_item`] / [`MenuModel::remove`].
    pub fn submenu_with_id(
        mut self,
        id: MenuItemId,
        title: impl Into<LocalizedString>,
        build: impl FnOnce(MenuItems) -> MenuItems,
    ) -> Self {
        let children = build(MenuItems::new()).nodes;
        self.nodes.push(MenuNode::Submenu {
            id,
            title: title.into(),
            children,
        });
        self
    }
}

/// A declarative menu tree shared by the in-window [`MenuBar`](crate::menu_bar::MenuBar)
/// and the native OS menu bar. Cloneable by handle (`Rc` inside); a clone shares
/// the same nodes and `version` signal, so mutating one updates every view.
#[derive(Clone)]
pub struct MenuModel {
    nodes: Rc<RefCell<Vec<MenuNode>>>,
    version: Signal<u64>,
}

impl Default for MenuModel {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuModel {
    /// An empty model.
    pub fn new() -> Self {
        Self {
            nodes: Rc::new(RefCell::new(Vec::new())),
            version: Signal::new(0),
        }
    }

    /// Append a top-level menu with the given title and contents (auto id).
    pub fn menu(
        self,
        title: impl Into<LocalizedString>,
        build: impl FnOnce(MenuItems) -> MenuItems,
    ) -> Self {
        self.menu_with_id(MenuItemId::next(), title, build)
    }

    /// Append a top-level menu with a caller-supplied id, so it can be addressed
    /// later by [`push_item`](Self::push_item) / [`remove`](Self::remove).
    pub fn menu_with_id(
        self,
        id: MenuItemId,
        title: impl Into<LocalizedString>,
        build: impl FnOnce(MenuItems) -> MenuItems,
    ) -> Self {
        let children = build(MenuItems::new()).nodes;
        self.nodes.borrow_mut().push(MenuNode::Submenu {
            id,
            title: title.into(),
            children,
        });
        self.bump();
        self
    }

    /// Append a platform-standard top-level menu (macOS App / Window / Help)
    /// with default (English) labels. Use [`standard_menu`](Self::standard_menu)
    /// to supply localized labels.
    pub fn standard(self, role: StandardMenuRole) -> Self {
        self.standard_menu(StandardMenu::for_role(role))
    }

    /// Append a platform-standard top-level menu with localized labels.
    pub fn standard_menu(self, menu: StandardMenu) -> Self {
        self.nodes.borrow_mut().push(MenuNode::Standard(menu));
        self.bump();
        self
    }

    /// A `Signal<u64>` bumped whenever the tree's *structure* changes. The
    /// native bridge re-installs the menu on a bump; per-item state changes go
    /// through the finer-grained `update_item` path instead.
    pub fn version(&self) -> Signal<u64> {
        self.version.clone()
    }

    /// Borrow the top-level nodes.
    pub fn nodes(&self) -> Ref<'_, Vec<MenuNode>> {
        self.nodes.borrow()
    }

    // ── Runtime structural mutation ────────────────────────────────────────
    //
    // These `&self` mutators change the menu *structure* at runtime and bump
    // `version`. A `MenuBar::from_model` bar binds `version` at `Rebuild` level,
    // so a bump re-derives the in-window dropdowns AND re-installs the native
    // menu. Per-item *state* (enabled / check / radio) does NOT need these —
    // bind a `Signal` to the `MenuEntry` instead (reactive without a rebuild).

    /// Mutate the node tree directly, then bump `version`. The escape hatch for
    /// any structural change the typed helpers don't cover (reorder, retitle,
    /// bulk edits). `MenuNode` / `MenuEntry` are public, so the closure can
    /// build whatever it needs.
    pub fn modify(&self, f: impl FnOnce(&mut Vec<MenuNode>)) {
        f(&mut self.nodes.borrow_mut());
        self.bump();
    }

    /// Append a top-level menu at runtime, returning its id. Mirrors
    /// [`menu`](Self::menu) but takes `&self`.
    pub fn push_menu(
        &self,
        title: impl Into<LocalizedString>,
        build: impl FnOnce(MenuItems) -> MenuItems,
    ) -> MenuItemId {
        let id = MenuItemId::next();
        let children = build(MenuItems::new()).nodes;
        self.nodes.borrow_mut().push(MenuNode::Submenu {
            id,
            title: title.into(),
            children,
        });
        self.bump();
        id
    }

    /// Append `entry` to the submenu identified by `into` (a top-level menu or
    /// nested submenu id). Returns `true` if the submenu was found.
    pub fn push_item(&self, into: MenuItemId, entry: MenuEntry) -> bool {
        let ok = {
            let mut nodes = self.nodes.borrow_mut();
            push_into_submenu(&mut nodes, into, MenuNode::Item(entry))
        };
        if ok {
            self.bump();
        }
        ok
    }

    /// Append a separator to the submenu identified by `into`. Returns `true`
    /// if the submenu was found.
    pub fn push_separator(&self, into: MenuItemId) -> bool {
        let ok = {
            let mut nodes = self.nodes.borrow_mut();
            push_into_submenu(&mut nodes, into, MenuNode::Separator)
        };
        if ok {
            self.bump();
        }
        ok
    }

    /// Remove the item or submenu with the given id, anywhere in the tree.
    /// Returns `true` if a node was removed.
    pub fn remove(&self, id: MenuItemId) -> bool {
        let removed = {
            let mut nodes = self.nodes.borrow_mut();
            remove_by_id(&mut nodes, id)
        };
        if removed {
            self.bump();
        }
        removed
    }

    fn bump(&self) {
        let v = self.version.get();
        self.version.set(v.wrapping_add(1));
    }
}

/// Append `node` to the children of the submenu with id `into` (searched
/// recursively). Returns whether the submenu was found.
fn push_into_submenu(nodes: &mut [MenuNode], into: MenuItemId, node: MenuNode) -> bool {
    // Two-phase to avoid moving `node` into a non-matching branch: first locate.
    fn find(nodes: &mut [MenuNode], into: MenuItemId) -> Option<&mut Vec<MenuNode>> {
        for n in nodes {
            if let MenuNode::Submenu { id, children, .. } = n {
                if *id == into {
                    return Some(children);
                }
                if let Some(found) = find(children, into) {
                    return Some(found);
                }
            }
        }
        None
    }
    match find(nodes, into) {
        Some(children) => {
            children.push(node);
            true
        }
        None => false,
    }
}

/// Remove the first node whose id matches (item or submenu), recursively.
fn remove_by_id(nodes: &mut Vec<MenuNode>, id: MenuItemId) -> bool {
    if let Some(pos) = nodes.iter().position(|n| match n {
        MenuNode::Item(e) => e.id == id,
        MenuNode::Submenu { id: sid, .. } => *sid == id,
        _ => false,
    }) {
        nodes.remove(pos);
        return true;
    }
    for n in nodes.iter_mut() {
        if let MenuNode::Submenu { children, .. } = n {
            if remove_by_id(children, id) {
                return true;
            }
        }
    }
    false
}

/// Build the in-window dropdown [`MenuList`] for a slice of nodes. Standard
/// roles are skipped (they only exist in the native bar).
pub(crate) fn build_menu_list(nodes: &[MenuNode]) -> MenuList {
    let mut list = MenuList::new();
    for node in nodes {
        match node {
            MenuNode::Item(entry) => {
                // `item_when` gates visibility reactively (Static(true) ⇒ always
                // shown, equivalent to `.item`).
                list = list.item_when(entry.to_menu_item(), entry.visible.clone());
            }
            MenuNode::Separator => {
                list = list.separator();
            }
            MenuNode::Submenu {
                title, children, ..
            } => {
                let children = children.clone();
                list = list.item(MenuItem::submenu(title.clone(), move || {
                    Box::new(build_menu_list(&children))
                }));
            }
            MenuNode::Standard(_) => {}
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_i18n::lit;

    #[test]
    fn menu_appends_nodes_and_bumps_version() {
        let model = MenuModel::new();
        let v0 = model.version().get();
        let model = model
            .menu(lit!("File"), |m| {
                m.item(MenuEntry::new(lit!("New"))).separator()
            })
            .standard(StandardMenuRole::Window);
        assert!(
            model.version().get() > v0,
            "structural change bumps version"
        );
        let nodes = model.nodes();
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0], MenuNode::Submenu { .. }));
        let MenuNode::Standard(sm) = &nodes[1] else {
            panic!("expected standard menu");
        };
        assert_eq!(sm.role(), StandardMenuRole::Window);
    }

    #[test]
    fn each_entry_gets_a_unique_id() {
        let a = MenuEntry::new(lit!("A"));
        let b = MenuEntry::new(lit!("B"));
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn push_item_into_submenu_by_id_and_remove() {
        let recent = bastyde_core::MenuItemId::next();
        let model = MenuModel::new().menu_with_id(recent, lit!("File"), |m| m);
        let v0 = model.version().get();

        // Add into the addressed submenu.
        let doc = MenuEntry::new(lit!("doc.txt"));
        let doc_id = doc.id();
        assert!(model.push_item(recent, doc));
        assert!(model.version().get() > v0, "push bumps version");

        // It landed inside the File submenu.
        {
            let nodes = model.nodes();
            let MenuNode::Submenu { children, .. } = &nodes[0] else {
                panic!("expected submenu");
            };
            assert_eq!(children.len(), 1);
        }

        // Push to a non-existent submenu is a no-op (returns false).
        assert!(!model.push_item(bastyde_core::MenuItemId::next(), MenuEntry::new(lit!("x"))));

        // Remove the item by id.
        assert!(model.remove(doc_id));
        {
            let nodes = model.nodes();
            let MenuNode::Submenu { children, .. } = &nodes[0] else {
                panic!("expected submenu");
            };
            assert!(children.is_empty());
        }
        assert!(!model.remove(doc_id), "second remove is a no-op");
    }

    #[test]
    fn push_menu_and_modify_at_runtime() {
        let model = MenuModel::new();
        let id = model.push_menu(lit!("Edit"), |m| m.item(MenuEntry::new(lit!("Cut"))));
        assert_eq!(model.nodes().len(), 1);

        // Escape hatch: append a top-level separator-bearing menu via modify.
        model.modify(|nodes| {
            nodes.push(MenuNode::Separator);
        });
        assert_eq!(model.nodes().len(), 2);

        // The pushed menu is addressable.
        assert!(model.remove(id));
        assert_eq!(model.nodes().len(), 1);
    }

    #[test]
    fn submenu_nesting_is_preserved() {
        let model = MenuModel::new().menu(lit!("File"), |m| {
            m.submenu(lit!("Recent"), |s| s.item(MenuEntry::new(lit!("doc.txt"))))
        });
        let nodes = model.nodes();
        let MenuNode::Submenu { children, .. } = &nodes[0] else {
            panic!("expected submenu");
        };
        assert!(matches!(children[0], MenuNode::Submenu { .. }));
    }
}
