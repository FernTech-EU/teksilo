// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Native (OS) menu service.
//!
//! Mirrors a logical menu tree (the `teksilo-widgets` `MenuModel`) into the
//! platform's *native* menu surface — the global menu bar at the top of the
//! screen on macOS (`NSApplication.mainMenu`), and, in the future, an `HMENU`
//! on Windows or a DBus app-menu on Linux. A serious desktop app is expected to
//! present its menus this way on macOS; an in-window menu strip alone reads as
//! non-native.
//!
//! Three concerns are separated, mirroring [`crate::file_dialog`] and
//! [`crate::external_dnd`]:
//!
//! - **Boundary data** — [`NativeMenuSnapshot`] is a plain, already-resolved
//!   description of the whole tree (display strings, key equivalents, enabled /
//!   check state, stable [`MenuItemId`]s). It carries no widgets, signals, or
//!   localized strings — the widget layer resolves all of that before handing a
//!   snapshot down, so `teksilo-platform` never depends on `teksilo-widgets`.
//! - **Trait surface** — [`NativeMenuBackend`] is the swappable platform
//!   abstraction (macOS `NSMenu`; [`NoopNativeMenuBackend`] elsewhere).
//! - **Handle** — [`NativeMenuHandle`] is the per-app service registered in
//!   app-state. It owns the backend and, per window, the map from
//!   [`MenuItemId`] to the action to run when that item is chosen.
//!
//! # Activation routing
//!
//! When the user picks a native menu item, the backend posts a
//! [`NativeMenuEventPayload`] through [`teksilo_core::AppEventPoster::post_external`].
//! `teksilo-app` picks it up in its `AppEvent::External` arm and looks the
//! [`MenuItemId`] up in the [`NativeMenuHandle`]. It runs the item's action
//! in the window whose menu it is (in the application tree for an item of the
//! application menu), and sends the item's intent from the focused widget of
//! the window with the OS focus, as the item's shortcut would be; see
//! `teksilo-app`'s `native_menu_route`.
//!
//! # The application menu and the windows' menus
//!
//! On macOS there is exactly one global menu bar. It shows the
//! **application menu** ([`NativeMenuHandle::set_app_menu`]), which
//! `teksilo-app` installs from the application tree, whenever the key window
//! has no menu of its own and while no window is open. A window with a menu
//! of its own ([`NativeMenuHandle::set_window_menu`], from a `MenuBar` built
//! with `MenuBar::from_model`) replaces it while that window is key.
//! `teksilo-app` calls [`NativeMenuHandle::activate_window`] on every focus
//! change, which picks between the two.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use teksilo_core::AppEventPoster;
use teksilo_core::MenuItemId;
use teksilo_core::widget::EventContext;
use teksilo_core::window::TeksiloWindowId;

#[cfg(target_os = "macos")]
mod macos;

// ============================================================
// Snapshot data (the platform boundary type)
// ============================================================

/// On/off/mixed state for a checkable native menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeCheck {
    /// Not a checkable item — no check-mark column behaviour.
    #[default]
    None,
    /// Checkable, currently unchecked.
    Off,
    /// Checkable, currently checked.
    On,
    /// Checkable, currently mixed/indeterminate (tri-state parents).
    Mixed,
}

/// A platform-neutral key equivalent for a native menu item. Already resolved
/// from the app's `ShortcutRegistry` by the widget layer. `key` is the base
/// character the OS menu expects (e.g. `"s"`, `"\r"`); the booleans are the
/// modifier flags. An item with an empty `key` displays no shortcut.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeKeyEquivalent {
    /// The base key as the single string the native menu expects.
    pub key: String,
    /// Command (⌘ on macOS) / the platform's primary accelerator modifier.
    pub command: bool,
    /// Shift (⇧).
    pub shift: bool,
    /// Alt / Option (⌥).
    pub alt: bool,
    /// Control (⌃).
    pub control: bool,
}

/// Standard, platform-defined menus with required placement/behaviour (the
/// macOS App / Window / Help menus, with their About / Services / Hide /
/// Hide Others / Show All / Quit and window-management items wired to system
/// selectors). The backend supplies the
/// native structure; the in-window `MenuBar` ignores these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardMenuRole {
    /// The application menu (About, Settings, Services, Hide, Hide Others,
    /// Show All, Quit). The macOS backend shows
    /// it first in the bar wherever the snapshot declares it, since AppKit
    /// shows the bar's first menu as the application menu.
    App,
    /// The Window menu (Minimize / Zoom / window list).
    Window,
    /// The Help menu.
    Help,
}

/// Display strings for a [`StandardMenuRole`], **already localized** by the
/// widget layer. The platform layer never hardcodes user-visible menu text — it
/// applies whatever the snapshot carries — so a standard menu honours the app's
/// locale (e.g. "Quitter" / "Masquer" on a French system) instead of leaking
/// English literals onto the most visible native surface.
#[derive(Debug, Clone, Default)]
pub struct StandardLabels {
    /// Submenu title (Window / Help; the App submenu typically uses the app name).
    pub title: String,
    /// "About …" (App).
    pub about: String,
    /// "Settings…" / "Preferences…" (App). Only rendered when the snapshot
    /// also carries a `settings_item` — the platform has no default action
    /// for it, unlike About, Hide and the rest.
    pub settings: String,
    /// "Services" (App): the submenu AppKit fills with the services that
    /// apply to the current selection.
    pub services: String,
    /// "Hide …" (App).
    pub hide: String,
    /// "Hide Others" (App).
    pub hide_others: String,
    /// "Show All" (App).
    pub show_all: String,
    /// "Quit …" (App).
    pub quit: String,
    /// "Minimize" (Window).
    pub minimize: String,
    /// "Zoom" (Window).
    pub zoom: String,
}

/// A row inside a platform-standard menu that the app routes rather than the
/// platform selects — Quit and Settings today.
///
/// Carries the key equivalent alongside the id because the widget layer is the
/// only place that knows it. Every other item's chord comes from the
/// `ShortcutRegistry` there, resolved through the primary-accelerator
/// convention; if the platform layer picked one for these two it would be the
/// one surface in the app advertising a chord nobody registered — live even
/// after the user rebound the command, and immune to the rewriting the registry
/// does for everything else. `None` means no key equivalent at all.
#[derive(Debug, Clone)]
pub struct StandardRoutedItem {
    /// Correlates the native item back to the logical one on activation.
    pub id: MenuItemId,
    /// Key equivalent to advertise, already resolved.
    pub key_equiv: Option<NativeKeyEquivalent>,
}

/// One node of a native menu tree.
// A standard menu's labels make it the large variant. A snapshot is built
// once per install and holds a handful of standard menus, so boxing them
// would cost every construction for no measurable saving.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum NativeMenuNode {
    /// A leaf command.
    Item {
        /// Correlates the native item back to the logical one on activation.
        id: MenuItemId,
        /// Display text (mnemonics already stripped, locale already resolved).
        title: String,
        /// Key equivalent, if any.
        key_equiv: Option<NativeKeyEquivalent>,
        /// Whether the item is enabled.
        enabled: bool,
        /// Whether the item is shown. A hidden item keeps its place, so a
        /// [`MenuItemDelta`] can show it again without rebuilding the menu.
        visible: bool,
        /// Check-mark state.
        check: NativeCheck,
    },
    /// A submenu with its own children.
    Submenu {
        /// Submenu title.
        title: String,
        /// Child nodes.
        children: Vec<NativeMenuNode>,
    },
    /// A separator line.
    Separator,
    /// A platform-standard menu the backend fills in, with localized chrome.
    Standard {
        /// Which standard menu.
        role: StandardMenuRole,
        /// Localized display strings (supplied by the widget layer).
        labels: StandardLabels,
        /// App menu only: build **Quit** as an ordinary routed item under this
        /// id (the activation recorded for it, and the key equivalent the
        /// widget layer resolved), instead of the platform's own terminate
        /// selector.
        ///
        /// The widget layer always routes it: the activation sends the app's
        /// quit intent, and asks the application to quit through every
        /// window's close guard when nothing takes the intent or there is none
        /// ([`NativeMenuActivation::quits`]). A main-menu key equivalent is
        /// dispatched by the platform before the responder chain, so an
        /// app-side ⌘Q never sees the keystroke; and the platform's own item
        /// (`terminate:`, which asks the quit through `os_quit` when
        /// `teksilo-app` runs the loop) could not send the app's quit intent.
        ///
        /// `None` binds the item to the platform's terminate selector
        /// (`terminate:` on macOS), for a snapshot built by other means.
        quit_item: Option<StandardRoutedItem>,
        /// App menu only: build a **Settings…** item under this id, placed where
        /// the platform expects it (on macOS: after About), with the key
        /// equivalent the widget layer resolved.
        ///
        /// Unlike Quit there is no `None` fallback that still does something —
        /// no platform ships a default action for opening an app's settings —
        /// so `None` simply omits the item. An app that has a settings window
        /// routes it; one that has none leaves the slot empty rather than
        /// showing a row that does nothing.
        settings_item: Option<StandardRoutedItem>,
    },
}

/// A complete, resolved description of one window's menu tree.
#[derive(Debug, Clone, Default)]
pub struct NativeMenuSnapshot {
    /// The top-level menus (each typically a [`NativeMenuNode::Submenu`] or a
    /// [`NativeMenuNode::Standard`]).
    pub roots: Vec<NativeMenuNode>,
}

/// A reactive change to a single already-installed native item, applied without
/// rebuilding the whole menu. Each `Some` field replaces that property.
#[derive(Debug, Clone, Default)]
pub struct MenuItemDelta {
    /// New enabled state.
    pub enabled: Option<bool>,
    /// New shown state.
    pub visible: Option<bool>,
    /// New check state.
    pub check: Option<NativeCheck>,
    /// New display title.
    pub title: Option<String>,
    /// New key equivalent (`Some(None)` clears it; `None` leaves it unchanged).
    pub key_equiv: Option<Option<NativeKeyEquivalent>>,
}

// ============================================================
// Activation (kept on the app side of the boundary)
// ============================================================

/// What to do when a native menu item is chosen. Cloneable (the action is an
/// `Rc`), so the router can pull a copy out of the handle and run it.
pub type MenuActionFn = Rc<dyn Fn(&mut EventContext)>;

/// What choosing a native menu item does: an intent, an action, or both.
///
/// `teksilo-app` runs `action` where the state it captured lives (the window
/// whose menu the item is in, or the application tree for an item of the
/// application menu), then sends `intent` from the focused widget of the
/// window with the OS focus, which may be another one (a dialog with no menu
/// of its own), or in the application tree when no window has the focus.
#[derive(Clone, Default)]
pub struct NativeMenuActivation {
    /// Fire this intent by name through the `Action`/`Intent` pipeline, from
    /// the focused widget of the window with the OS focus. Dispatched after
    /// `action` has run.
    pub intent: Option<&'static str>,
    /// Or run this closure directly (the escape hatch), in the window whose
    /// menu the item is in, anchored at that window's root, or in the
    /// application tree for an item of the application menu.
    pub action: Option<MenuActionFn>,
    /// The App menu's Quit row: when nothing on the route takes `intent`, or
    /// there is none, `teksilo-app` asks the application to quit, through
    /// every window's close guard, instead.
    pub quits: bool,
}

impl std::fmt::Debug for NativeMenuActivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeMenuActivation")
            .field("intent", &self.intent)
            .field("action", &self.action.as_ref().map(|_| "<closure>"))
            .field("quits", &self.quits)
            .finish()
    }
}

// ============================================================
// Event payload
// ============================================================

/// Boxed inside `AppEvent::External` when the user picks a native menu item.
/// `teksilo-app` downcasts to this, runs the item's action in the window whose
/// menu it is, and sends its intent from the window with the OS focus (see
/// the [module](self) doc).
#[derive(Debug, Clone)]
pub struct NativeMenuEventPayload {
    /// The window whose menu was active when the item was chosen.
    pub window_id_owner: TeksiloWindowId,
    /// The chosen item.
    pub item_id: MenuItemId,
}

/// Boxed inside `AppEvent::External` when the user picks an item of the
/// [application menu](NativeMenuBackend::set_app_menu), the menu shown while
/// no window's own menu is. `teksilo-app` runs the item's action in the
/// application tree and sends its intent from the focus of the window with
/// the OS focus, or in the application tree when none has it.
#[derive(Debug, Clone)]
pub struct NativeAppMenuEventPayload {
    /// The chosen item.
    pub item_id: MenuItemId,
}

// ============================================================
// Backend trait
// ============================================================

/// Swappable native-menu backend. One instance serves the whole app.
pub trait NativeMenuBackend {
    /// Build (or replace) the native menu of `window_id`, a window with a
    /// menu of its own, from `menu`. For every item the user later chooses,
    /// the backend MUST post a [`NativeMenuEventPayload`], with
    /// `window_id_owner == window_id`, through `poster`. If `window_id` is
    /// the key window (the last one [`activate_window`](Self::activate_window)
    /// named), or no window has been activated yet, the backend shows this
    /// menu.
    fn set_window_menu(
        &mut self,
        window_id: TeksiloWindowId,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
    );

    /// `window_id` became the key window (focus follows window): show the
    /// bar [`key_window_bar`] picks, its own menu if it set one, else the
    /// menu of the window it belongs to (see
    /// [`set_window_owner`](Self::set_window_owner)), else the
    /// [application menu](Self::set_app_menu), or, when the application
    /// declared none, the bar shown before.
    fn activate_window(&mut self, window_id: TeksiloWindowId);

    /// `window_id` belongs to `owner` (a modal dialog to the window it
    /// blocks), or to no window (`None`). While `window_id` is key and has
    /// no menu of its own, the bar shows its owner's, as AppKit keeps the
    /// menu bar for a panel or a sheet. `teksilo-app` calls it as it opens a
    /// modal window; the relation goes with
    /// [`clear_window`](Self::clear_window). The default ignores it, for a
    /// backend whose menus belong to windows alone.
    #[doc(hidden)]
    fn set_window_owner(&mut self, window_id: TeksiloWindowId, owner: Option<TeksiloWindowId>) {
        let _ = (window_id, owner);
    }

    /// Forget `window_id`'s menu (the window closed, or no longer has a menu
    /// of its own). If it was showing, show the
    /// [application menu](Self::set_app_menu) until another window becomes
    /// key.
    fn clear_window(&mut self, window_id: TeksiloWindowId);

    /// Apply a reactive delta to a single already-installed item, in every
    /// menu that carries it, the application menu included.
    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta);

    /// Whether this backend shows a menu bar the user can open. `false` for
    /// one that renders nothing ([`NoopNativeMenuBackend`]): `teksilo-app`
    /// then keeps no row's availability up to date.
    #[doc(hidden)]
    fn shows_bar(&self) -> bool {
        true
    }

    /// Set the application menu: the menu shown whenever the key window has
    /// no menu of its own nor an owner with one, and while no window is
    /// open. `teksilo-app` sets it from the application tree at startup and
    /// again whenever it changes; it replaces any earlier one, and the bar
    /// is chosen again for the key window, as
    /// [`activate_window`](Self::activate_window) chooses it: an
    /// application menu declared while a window
    /// with no menu of its own is key shows at once. `declared`
    /// says whether the application declared it: `false` for the default
    /// App menu `teksilo-app` supplies when it declared none, which a key
    /// window without a menu does not bring up (see [`key_window_bar`]).
    /// For every item the user later chooses in it, the backend MUST post a
    /// [`NativeAppMenuEventPayload`] through `poster`. The default does
    /// nothing, for a backend whose menus belong to windows alone.
    fn set_app_menu(
        &mut self,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
        declared: bool,
    ) {
        let _ = (menu, poster, declared);
    }
}

/// Forward through a boxed backend so `NativeMenuHandle::new(default_backend())`
/// type-checks.
impl NativeMenuBackend for Box<dyn NativeMenuBackend> {
    fn set_window_menu(
        &mut self,
        window_id: TeksiloWindowId,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
    ) {
        (**self).set_window_menu(window_id, menu, poster)
    }
    fn activate_window(&mut self, window_id: TeksiloWindowId) {
        (**self).activate_window(window_id)
    }
    fn set_window_owner(&mut self, window_id: TeksiloWindowId, owner: Option<TeksiloWindowId>) {
        (**self).set_window_owner(window_id, owner)
    }
    fn clear_window(&mut self, window_id: TeksiloWindowId) {
        (**self).clear_window(window_id)
    }
    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta) {
        (**self).update_item(id, delta)
    }
    fn shows_bar(&self) -> bool {
        (**self).shows_bar()
    }
    fn set_app_menu(
        &mut self,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
        declared: bool,
    ) {
        (**self).set_app_menu(menu, poster, declared)
    }
}

// ============================================================
// NativeMenuHandle
// ============================================================

/// Per-window map from item id to its activation.
type WindowActivations = HashMap<MenuItemId, NativeMenuActivation>;

/// A row whose only effect is its intent, enabled only while something on
/// the intent's route takes it. See [`NativeMenuHandle::refresh_availability`].
struct Gate {
    intent: &'static str,
    /// The row's own enabled state (its `enabled` signal).
    own: bool,
    /// Whether the route took the intent at the last refresh.
    handled: bool,
    /// What the backend was last told.
    shown: bool,
}

struct NativeMenuState {
    backend: RefCell<Box<dyn NativeMenuBackend>>,
    /// Per-window: item id → what to do when chosen.
    activations: RefCell<HashMap<TeksiloWindowId, WindowActivations>>,
    /// The application menu's items → what to do when chosen.
    app_activations: RefCell<WindowActivations>,
    /// The rows that grey out when nothing takes their intent, by id.
    gates: RefCell<HashMap<MenuItemId, Gate>>,
    /// The intents taken at the last refresh, `None` once a menu installed
    /// since has gates the refresh has not evaluated: while it holds the
    /// same answer, a refresh has nothing to do.
    refreshed: RefCell<Option<Vec<&'static str>>>,
    /// How many rows the refreshes evaluated, for the tests.
    #[cfg(test)]
    evaluations: std::cell::Cell<usize>,
}

/// Per-app native-menu service. Registered in app-state by
/// `TeksiloAppBuilder::install_native_menu` (or `.app_state(NativeMenuHandle::new(..))`
/// for a custom backend). Cloneable; clones share one backend + activation map.
#[derive(Clone)]
pub struct NativeMenuHandle {
    inner: Rc<NativeMenuState>,
}

impl NativeMenuHandle {
    /// Build a handle wrapping the given backend.
    pub fn new<B: NativeMenuBackend + 'static>(backend: B) -> Self {
        Self {
            inner: Rc::new(NativeMenuState {
                backend: RefCell::new(Box::new(backend)),
                activations: RefCell::new(HashMap::new()),
                app_activations: RefCell::new(HashMap::new()),
                gates: RefCell::new(HashMap::new()),
                refreshed: RefCell::new(None),
                #[cfg(test)]
                evaluations: std::cell::Cell::new(0),
            }),
        }
    }

    /// Install `window_id`'s menu, recording the per-item activations so a later
    /// click can be routed. Replaces any prior menu for that window.
    pub fn set_window_menu(
        &self,
        window_id: TeksiloWindowId,
        menu: NativeMenuSnapshot,
        activations: HashMap<MenuItemId, NativeMenuActivation>,
        poster: Arc<dyn AppEventPoster>,
    ) {
        self.gate_rows(&menu, &activations);
        self.inner
            .activations
            .borrow_mut()
            .insert(window_id, activations);
        self.inner
            .backend
            .borrow_mut()
            .set_window_menu(window_id, menu, poster);
    }

    /// `window_id` became the key window: show the bar
    /// [`key_window_bar`] picks for it.
    pub fn activate_window(&self, window_id: TeksiloWindowId) {
        self.inner.backend.borrow_mut().activate_window(window_id);
    }

    /// `window_id` belongs to `owner` (a modal dialog to the window it
    /// blocks): see [`NativeMenuBackend::set_window_owner`].
    #[doc(hidden)]
    pub fn set_window_owner(&self, window_id: TeksiloWindowId, owner: Option<TeksiloWindowId>) {
        self.inner
            .backend
            .borrow_mut()
            .set_window_owner(window_id, owner);
    }

    /// Forget a window's menu + activations (window closed).
    pub fn clear_window(&self, window_id: TeksiloWindowId) {
        self.inner.activations.borrow_mut().remove(&window_id);
        self.forget_dead_gates();
        self.inner.backend.borrow_mut().clear_window(window_id);
    }

    /// Apply a reactive delta to one installed item. For a row that greys
    /// out when nothing takes its intent, a new `enabled` is the row's own
    /// state, and what reaches the backend is that state and whether the
    /// intent was taken at the last
    /// [`refresh_availability`](Self::refresh_availability).
    pub fn update_item(&self, id: MenuItemId, mut delta: MenuItemDelta) {
        if let Some(own) = delta.enabled
            && let Some(gate) = self.inner.gates.borrow_mut().get_mut(&id)
        {
            gate.own = own;
            gate.shown = own && gate.handled;
            delta.enabled = Some(gate.shown);
        }
        self.inner.backend.borrow_mut().update_item(id, delta);
    }

    /// Grey out or enable again every installed row whose only effect is
    /// its intent (an item with an intent, no action, and not the App menu's
    /// Quit), from `handled`, the sorted intent names something on the route
    /// takes now, combined with the row's own enabled state. Only a row
    /// whose state changes reaches the backend, through
    /// [`NativeMenuBackend::update_item`].
    ///
    /// `teksilo-app` calls it once per event-loop turn, with what the route
    /// a chosen row's intent takes answers (the focused window's
    /// `WidgetTree::handled_intents_kept_in_modal_from` its live focus, where
    /// the row's intent starts and which a dialog open in the window keeps,
    /// else the application tree's), so the
    /// menu is current whenever the user opens it: focus, the key window and
    /// the actions only change while the loop runs. With the same `handled`
    /// as the last call and no menu installed since, nothing can have
    /// changed (a row's own state reaches the backend through
    /// [`update_item`](Self::update_item)), and no row is evaluated.
    #[doc(hidden)]
    pub fn refresh_availability(&self, handled: &[&'static str]) {
        if self
            .inner
            .refreshed
            .borrow()
            .as_deref()
            .is_some_and(|last| last == handled)
        {
            return;
        }
        *self.inner.refreshed.borrow_mut() = Some(handled.to_vec());
        let mut changed = Vec::new();
        for (id, gate) in self.inner.gates.borrow_mut().iter_mut() {
            #[cfg(test)]
            self.inner.evaluations.set(self.inner.evaluations.get() + 1);
            gate.handled = handled.binary_search(&gate.intent).is_ok();
            let shown = gate.own && gate.handled;
            if shown != gate.shown {
                gate.shown = shown;
                changed.push((*id, shown));
            }
        }
        let mut backend = self.inner.backend.borrow_mut();
        for (id, enabled) in changed {
            backend.update_item(
                id,
                MenuItemDelta {
                    enabled: Some(enabled),
                    ..Default::default()
                },
            );
        }
    }

    /// Whether the backend shows a menu bar at all
    /// ([`NativeMenuBackend::shows_bar`]): `teksilo-app` refreshes the rows'
    /// availability only then.
    #[doc(hidden)]
    pub fn shows_bar(&self) -> bool {
        self.inner.backend.borrow().shows_bar()
    }

    /// How many rows the availability refreshes evaluated.
    #[cfg(test)]
    fn evaluations(&self) -> usize {
        self.inner.evaluations.get()
    }

    /// The enabled state the backend was last given for a row that greys
    /// out when nothing takes its intent; `None` for any other item.
    #[doc(hidden)]
    pub fn gated_enabled(&self, id: MenuItemId) -> Option<bool> {
        self.inner.gates.borrow().get(&id).map(|gate| gate.shown)
    }

    /// Record the rows of `menu` that grey out when nothing takes their
    /// intent, enabled as `menu` installs them until the next refresh.
    fn gate_rows(&self, menu: &NativeMenuSnapshot, activations: &WindowActivations) {
        fn own_enabled(nodes: &[NativeMenuNode], id: MenuItemId) -> Option<bool> {
            nodes.iter().find_map(|node| match node {
                NativeMenuNode::Item {
                    id: item, enabled, ..
                } if *item == id => Some(*enabled),
                NativeMenuNode::Submenu { children, .. } => own_enabled(children, id),
                _ => None,
            })
        }
        let mut gates = self.inner.gates.borrow_mut();
        // The new rows start enabled: the next refresh evaluates them,
        // whatever it is told.
        *self.inner.refreshed.borrow_mut() = None;
        for (id, activation) in activations {
            let (Some(intent), None, false) =
                (activation.intent, &activation.action, activation.quits)
            else {
                continue;
            };
            let own = own_enabled(&menu.roots, *id).unwrap_or(true);
            gates.insert(
                *id,
                Gate {
                    intent,
                    own,
                    handled: true,
                    shown: own,
                },
            );
        }
    }

    /// Forget the gates of rows no installed menu holds any more.
    fn forget_dead_gates(&self) {
        let windows = self.inner.activations.borrow();
        let app = self.inner.app_activations.borrow();
        self.inner.gates.borrow_mut().retain(|id, _| {
            app.contains_key(id) || windows.values().any(|menu| menu.contains_key(id))
        });
    }

    /// Set the application menu, shown whenever the key window has no menu
    /// of its own and while no window is open (see
    /// [`NativeMenuBackend::set_app_menu`]), recording its activations.
    /// Replaces any earlier one. `teksilo-app` calls it from the application
    /// tree's declaration; a window's menu never sets it.
    pub fn set_app_menu(
        &self,
        menu: NativeMenuSnapshot,
        activations: HashMap<MenuItemId, NativeMenuActivation>,
        poster: Arc<dyn AppEventPoster>,
        declared: bool,
    ) {
        self.gate_rows(&menu, &activations);
        *self.inner.app_activations.borrow_mut() = activations;
        self.forget_dead_gates();
        self.inner
            .backend
            .borrow_mut()
            .set_app_menu(menu, poster, declared);
    }

    /// The activation for an item chosen in the application menu.
    pub fn app_activation(&self, id: MenuItemId) -> Option<NativeMenuActivation> {
        self.inner.app_activations.borrow().get(&id).cloned()
    }

    /// Look up (and clone) the activation for a chosen item, for the router.
    pub fn activation(
        &self,
        window_id: TeksiloWindowId,
        id: MenuItemId,
    ) -> Option<NativeMenuActivation> {
        self.inner
            .activations
            .borrow()
            .get(&window_id)
            .and_then(|m| m.get(&id).cloned())
    }
}

impl std::fmt::Debug for NativeMenuHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeMenuHandle")
            .field("windows", &self.inner.activations.borrow().len())
            .finish_non_exhaustive()
    }
}

// ============================================================
// Which bar a key window shows (pure, so every OS tests it)
// ============================================================

/// The bar to show while `key` is the key window: `Some(window)` for that
/// window's own menu, `None` for the application menu.
///
/// The key window's own menu if it has one (`has_menu`); else the menu of
/// the window it belongs to, following `owner` (a modal dialog's owner, then
/// that window's owner); else the application menu when the application
/// declared one (`app_declared`), or `shown`, the bar shown before, when it
/// did not: AppKit keeps `mainMenu` up while a panel or a sheet is key, so
/// an app that declared no application menu keeps its window's File and
/// Edit while a dialog or a secondary window is key.
#[doc(hidden)]
pub fn key_window_bar(
    key: TeksiloWindowId,
    has_menu: impl Fn(TeksiloWindowId) -> bool,
    owner: impl Fn(TeksiloWindowId) -> Option<TeksiloWindowId>,
    app_declared: bool,
    shown: Option<TeksiloWindowId>,
) -> Option<TeksiloWindowId> {
    let mut window = key;
    let mut visited = vec![window];
    loop {
        if has_menu(window) {
            return Some(window);
        }
        match owner(window) {
            Some(next) if !visited.contains(&next) => {
                visited.push(next);
                window = next;
            }
            _ => break,
        }
    }
    if app_declared { None } else { shown }
}

/// What a backend shows once the application menu has been set or replaced
/// ([`NativeMenuBackend::set_app_menu`]): `Some(bar)` to show `bar` (as
/// [`key_window_bar`] names it, `None` for the application menu), `None`
/// to leave the bar up as it is.
///
/// `key_bar` is what [`key_window_bar`] picks for the key window now, `None`
/// before any window was activated; `shown` is the bar up. The key window's
/// pick is shown when it differs from the bar up (an application menu
/// declared while a window with no menu of its own is key replaces the
/// window bar kept for it), and whenever it is the application menu, which
/// is a new menu now. With no key window, only an application menu already
/// up is replaced.
pub(crate) fn bar_after_app_menu(
    key_bar: Option<Option<TeksiloWindowId>>,
    shown: Option<TeksiloWindowId>,
) -> Option<Option<TeksiloWindowId>> {
    match key_bar {
        Some(bar) => (bar.is_none() || bar != shown).then_some(bar),
        None => shown.is_none().then_some(None),
    }
}

/// What a backend shows once it has forgotten `cleared`'s menu
/// ([`NativeMenuBackend::clear_window`]): `Some(bar)` to show `bar` (as
/// [`key_window_bar`] names it, `None` for the application menu), `None` to
/// leave the bar up as it is.
///
/// Only a bar that was `cleared`'s own (`shown`) is replaced, by the key
/// window's pick (`key`, with `has_menu`, `owner` and `app_declared` as for
/// [`key_window_bar`]) when another window is key, else by the application
/// menu. The pick is made as if `cleared` were already gone and no bar were
/// up, so it is never `cleared`'s menu, declared application menu or not,
/// even when the key window is `cleared` itself or a dialog it owned.
pub(crate) fn bar_after_clear(
    cleared: TeksiloWindowId,
    key: Option<TeksiloWindowId>,
    shown: Option<TeksiloWindowId>,
    has_menu: impl Fn(TeksiloWindowId) -> bool,
    owner: impl Fn(TeksiloWindowId) -> Option<TeksiloWindowId>,
    app_declared: bool,
) -> Option<Option<TeksiloWindowId>> {
    if shown != Some(cleared) {
        return None;
    }
    Some(key.filter(|key| *key != cleared).and_then(|key| {
        key_window_bar(
            key,
            |window| window != cleared && has_menu(window),
            |window| owner(window).filter(|owner| *owner != cleared),
            app_declared,
            None,
        )
    }))
}

// ============================================================
// NoopNativeMenuBackend
// ============================================================

/// Backend that renders nothing. Used on platforms without a native-menu
/// implementation (everything except macOS today) so cross-platform code that
/// installs a native menu compiles and runs — the in-window `MenuBar` remains
/// the menu surface there.
#[derive(Default)]
pub struct NoopNativeMenuBackend;

impl NoopNativeMenuBackend {
    /// Build the no-op backend.
    pub fn new() -> Self {
        Self
    }
}

impl NativeMenuBackend for NoopNativeMenuBackend {
    fn set_window_menu(
        &mut self,
        _window_id: TeksiloWindowId,
        _menu: NativeMenuSnapshot,
        _poster: Arc<dyn AppEventPoster>,
    ) {
    }
    fn activate_window(&mut self, _window_id: TeksiloWindowId) {}
    fn clear_window(&mut self, _window_id: TeksiloWindowId) {}
    fn update_item(&mut self, _id: MenuItemId, _delta: MenuItemDelta) {}
    fn shows_bar(&self) -> bool {
        false
    }
}

// ============================================================
// The macOS bar's layout (pure, so every OS tests it)
// ============================================================

/// The top-level menus of a snapshot as the macOS menu bar shows them.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug)]
pub(crate) struct MacBarPlan<'a> {
    /// The menus in bar order: the first App-role menu, then every other
    /// submenu and standard menu in the order the snapshot declares them.
    pub(crate) menus: Vec<&'a NativeMenuNode>,
    /// The index in `menus` of the menu AppKit keeps the window list in
    /// (`NSApp.windowsMenu`): the first Window-role menu. `None`: the bar
    /// declares none, and the backend adds a hidden one, so the Dock icon's
    /// menu still lists the windows.
    pub(crate) windows_menu: Option<usize>,
    /// The index in `menus` of the menu registered as `NSApp.helpMenu`: the
    /// first Help-role menu.
    pub(crate) help_menu: Option<usize>,
    /// For each entry of `menus`, the rows of the app's own menus folded into
    /// it: a menu the app titles like a standard Window or Help menu in the
    /// same snapshot, which the bar would otherwise show twice. Empty for
    /// every other menu.
    pub(crate) folded: Vec<Vec<&'a NativeMenuNode>>,
}

/// Lay out `snapshot` for the macOS menu bar.
///
/// AppKit shows the bar's first menu as the bold application menu, under
/// the application's name, whatever that menu holds. So the App-role menu
/// goes first wherever the model declared it: declared after File, File's
/// rows would sit under the application's name and About, Hide and Quit in
/// a second, ordinary menu. A later App-role menu is left out, since a bar
/// has one application menu. A leaf or separator at the top level is left
/// out too: the bar holds menus only.
///
/// A menu of the app's own titled like a standard Window or Help menu of the
/// same snapshot is not shown beside it: its rows go into the first standard
/// menu with that title, where a Mac app keeps them (its Window commands
/// under Minimize and Zoom). A model that gives the in-window bar a Window
/// menu and macOS its standard one shows one Window menu on macOS, with
/// both.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn mac_bar_plan(snapshot: &NativeMenuSnapshot) -> MacBarPlan<'_> {
    let is_app = |node: &NativeMenuNode| {
        matches!(
            node,
            NativeMenuNode::Standard {
                role: StandardMenuRole::App,
                ..
            }
        )
    };
    let app = snapshot.roots.iter().find(|node| is_app(node));
    let rest = snapshot.roots.iter().filter(|node| {
        !is_app(node)
            && matches!(
                node,
                NativeMenuNode::Submenu { .. } | NativeMenuNode::Standard { .. }
            )
    });
    let candidates: Vec<&NativeMenuNode> = app.into_iter().chain(rest).collect();
    // The first Window- or Help-role menu with this title, which an app menu
    // of the same title folds into.
    let host_of = |wanted: &str| {
        candidates.iter().position(|node| {
            matches!(
                node,
                NativeMenuNode::Standard { role: StandardMenuRole::Window | StandardMenuRole::Help, labels, .. }
                    if labels.title == wanted
            )
        })
    };
    let mut menus = Vec::with_capacity(candidates.len());
    let mut folded: Vec<Vec<&NativeMenuNode>> = Vec::with_capacity(candidates.len());
    // `host_index[i]`: where `candidates[i]` landed in `menus`.
    let mut host_index = vec![None; candidates.len()];
    let mut pending: Vec<(usize, &[NativeMenuNode])> = Vec::new();
    for (i, node) in candidates.iter().copied().enumerate() {
        if let NativeMenuNode::Submenu { title, children } = node
            && let Some(host) = host_of(title)
        {
            pending.push((host, children));
            continue;
        }
        host_index[i] = Some(menus.len());
        menus.push(node);
        folded.push(Vec::new());
    }
    for (host, children) in pending {
        if let Some(at) = host_index[host] {
            folded[at].extend(children);
        }
    }
    let first = |wanted: StandardMenuRole| {
        menus.iter().position(
            |node| matches!(node, NativeMenuNode::Standard { role, .. } if *role == wanted),
        )
    };
    MacBarPlan {
        windows_menu: first(StandardMenuRole::Window),
        help_menu: first(StandardMenuRole::Help),
        menus,
        folded,
    }
}

/// Whether the macOS backend evaluates a window again for the Window menu
/// it registers, by flipping its `excludedFromWindowsMenu`: a window that
/// is not excluded (as a dialog is) and that AppKit lists, on screen
/// (`visible`) or in the Dock (`miniaturized`). AppKit adds a row for a
/// window it evaluates with the flag off, so flipping a hidden window's
/// would list a window the app hid.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn mac_relists_window(excluded: bool, visible: bool, miniaturized: bool) -> bool {
    !excluded && (visible || miniaturized)
}

/// A row of the macOS App menu. See [`mac_app_menu_rows`].
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MacAppMenuRow {
    /// About, AppKit's `orderFrontStandardAboutPanel:`.
    About,
    /// Settings, routed to the app.
    Settings,
    /// The Services submenu, registered as `NSApp.servicesMenu`.
    Services,
    /// Hide <App>, `hide:`, ⌘H.
    Hide,
    /// Hide Others, `hideOtherApplications:`, ⌥⌘H.
    HideOthers,
    /// Show All, `unhideAllApplications:`.
    ShowAll,
    /// Quit, routed to the app.
    Quit,
    /// A separator.
    Separator,
}

/// The rows of the macOS App menu, in the order AppKit's own application
/// menu has them: About; Settings in a group of its own when the app routes
/// it; Services; Hide, Hide Others and Show All; Quit.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn mac_app_menu_rows(settings: bool) -> Vec<MacAppMenuRow> {
    use MacAppMenuRow::*;
    let mut rows = vec![About, Separator];
    if settings {
        rows.extend([Settings, Separator]);
    }
    rows.extend([
        Services, Separator, Hide, HideOthers, ShowAll, Separator, Quit,
    ]);
    rows
}

// ============================================================
// Default backend factory
// ============================================================

/// The default native-menu backend for the current target: macOS gets the real
/// `NSMenu` backend, every other target gets [`NoopNativeMenuBackend`].
pub fn default_backend() -> Box<dyn NativeMenuBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsNativeMenuBackend::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(NoopNativeMenuBackend::new())
    }
}

// ============================================================
// MemoryNativeMenuBackend (test backend)
// ============================================================

/// Recording backend for headless tests. Captures the snapshot set per window,
/// which window is active, item deltas, and cleared windows. Cloneable; clones
/// share the recording so a test can keep a clone after handing one to
/// [`NativeMenuHandle::new`].
#[derive(Clone, Default)]
pub struct MemoryNativeMenuBackend {
    inner: Rc<RefCell<MemoryRecording>>,
}

#[derive(Default)]
struct MemoryRecording {
    menus: HashMap<TeksiloWindowId, NativeMenuSnapshot>,
    app_menu: Option<NativeMenuSnapshot>,
    /// The last window `activate_window` named.
    key: Option<TeksiloWindowId>,
    /// The window whose menu shows; `None` while the application menu does.
    active: Option<TeksiloWindowId>,
    /// The window each window belongs to (`set_window_owner`).
    owners: HashMap<TeksiloWindowId, TeksiloWindowId>,
    /// Whether the application menu set last was declared by the app.
    app_declared: bool,
    deltas: Vec<(MenuItemId, MenuItemDelta)>,
    cleared: Vec<TeksiloWindowId>,
}

impl MemoryRecording {
    /// The bar shown while `key` is key, by [`key_window_bar`].
    fn bar_for(&self, key: TeksiloWindowId) -> Option<TeksiloWindowId> {
        key_window_bar(
            key,
            |window| self.menus.contains_key(&window),
            |window| self.owners.get(&window).copied(),
            self.app_declared,
            self.active,
        )
    }
}

impl MemoryNativeMenuBackend {
    /// Build a new empty recording backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// The snapshot currently set for `window_id`, if any.
    pub fn menu_for(&self, window_id: TeksiloWindowId) -> Option<NativeMenuSnapshot> {
        self.inner.borrow().menus.get(&window_id).cloned()
    }

    /// The window whose own menu shows, as on macOS: the one
    /// [`key_window_bar`] picks for the key window (the last
    /// `activate_window`), or a window that set one before any was
    /// activated. `None` while the application menu shows.
    pub fn active_window(&self) -> Option<TeksiloWindowId> {
        self.inner.borrow().active
    }

    /// Whether the application menu is the one showing: no window's own
    /// menu is ([`active_window`](Self::active_window) is `None`).
    pub fn shows_app_menu(&self) -> bool {
        self.inner.borrow().active.is_none()
    }

    /// All item deltas applied so far, in order.
    pub fn deltas(&self) -> Vec<(MenuItemId, MenuItemDelta)> {
        self.inner.borrow().deltas.clone()
    }

    /// Windows whose menus were cleared, in order.
    pub fn cleared(&self) -> Vec<TeksiloWindowId> {
        self.inner.borrow().cleared.clone()
    }

    /// The application menu last set, if any.
    pub fn app_menu(&self) -> Option<NativeMenuSnapshot> {
        self.inner.borrow().app_menu.clone()
    }
}

impl NativeMenuBackend for MemoryNativeMenuBackend {
    fn set_window_menu(
        &mut self,
        window_id: TeksiloWindowId,
        menu: NativeMenuSnapshot,
        _poster: Arc<dyn AppEventPoster>,
    ) {
        let mut rec = self.inner.borrow_mut();
        rec.menus.insert(window_id, menu);
        // As the macOS backend: the bar the key window shows, and a
        // window's menu set before any window was activated.
        rec.active = match rec.key {
            Some(key) => rec.bar_for(key),
            None => Some(window_id),
        };
    }
    fn activate_window(&mut self, window_id: TeksiloWindowId) {
        let mut rec = self.inner.borrow_mut();
        rec.key = Some(window_id);
        rec.active = rec.bar_for(window_id);
    }
    fn set_window_owner(&mut self, window_id: TeksiloWindowId, owner: Option<TeksiloWindowId>) {
        let mut rec = self.inner.borrow_mut();
        match owner {
            Some(owner) => rec.owners.insert(window_id, owner),
            None => rec.owners.remove(&window_id),
        };
    }
    fn clear_window(&mut self, window_id: TeksiloWindowId) {
        let mut rec = self.inner.borrow_mut();
        rec.menus.remove(&window_id);
        rec.owners.remove(&window_id);
        rec.cleared.push(window_id);
        // As the macOS backend: never the dead window's menu.
        let bar = bar_after_clear(
            window_id,
            rec.key,
            rec.active,
            |window| rec.menus.contains_key(&window),
            |window| rec.owners.get(&window).copied(),
            rec.app_declared,
        );
        if let Some(bar) = bar {
            rec.active = bar;
        }
    }
    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta) {
        self.inner.borrow_mut().deltas.push((id, delta));
    }
    fn set_app_menu(
        &mut self,
        menu: NativeMenuSnapshot,
        _poster: Arc<dyn AppEventPoster>,
        declared: bool,
    ) {
        let mut rec = self.inner.borrow_mut();
        rec.app_menu = Some(menu);
        rec.app_declared = declared;
        // As the macOS backend: the key window's bar again, which the new
        // application menu may be.
        let key_bar = rec.key.map(|key| rec.bar_for(key));
        if let Some(bar) = bar_after_app_menu(key_bar, rec.active) {
            rec.active = bar;
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use teksilo_core::SubscriptionId;

    struct NullPoster;
    impl AppEventPoster for NullPoster {
        fn post_subscription_event(
            &self,
            _sub_id: SubscriptionId,
            _event: Box<dyn std::any::Any + Send>,
        ) {
        }
        fn post_external(&self, _payload: Box<dyn std::any::Any + Send>) {}
    }

    fn poster() -> Arc<dyn AppEventPoster> {
        Arc::new(NullPoster)
    }

    fn win(n: u64) -> TeksiloWindowId {
        TeksiloWindowId::new(n)
    }

    fn sample_snapshot(id: MenuItemId) -> NativeMenuSnapshot {
        NativeMenuSnapshot {
            roots: vec![NativeMenuNode::Submenu {
                title: "File".into(),
                children: vec![NativeMenuNode::Item {
                    id,
                    title: "New".into(),
                    key_equiv: None,
                    enabled: true,
                    visible: true,
                    check: NativeCheck::None,
                }],
            }],
        }
    }

    #[test]
    fn set_menu_records_snapshot_and_activations() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let id = MenuItemId::next();

        let fired = Arc::new(Mutex::new(false));
        let fired2 = fired.clone();
        let mut acts = HashMap::new();
        acts.insert(
            id,
            NativeMenuActivation {
                intent: Some("app.new"),
                action: Some(Rc::new(move |_ctx: &mut EventContext| {
                    *fired2.lock().unwrap() = true;
                })),
                quits: false,
            },
        );

        handle.set_window_menu(win(1), sample_snapshot(id), acts, poster());

        assert!(backend.menu_for(win(1)).is_some());
        assert_eq!(backend.active_window(), Some(win(1)));
        let act = handle.activation(win(1), id).expect("activation recorded");
        assert_eq!(act.intent, Some("app.new"));
        assert!(act.action.is_some());
    }

    #[test]
    fn activate_and_clear_window() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let id = MenuItemId::next();
        handle.set_window_menu(win(1), sample_snapshot(id), HashMap::new(), poster());
        handle.set_window_menu(
            win(2),
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
        );

        handle.activate_window(win(2));
        assert_eq!(backend.active_window(), Some(win(2)));

        handle.clear_window(win(2));
        assert_eq!(backend.cleared(), vec![win(2)]);
        assert!(handle.activation(win(2), id).is_none());
        assert!(backend.menu_for(win(2)).is_none());
    }

    /// A window with no menu of its own, nor an owner with one, shows the
    /// application menu the app declared when it becomes key, as the macOS
    /// backend does: the previous window's menu used to stay up, its items
    /// acting on a window the user had left.
    #[test]
    fn activating_a_window_without_a_menu_shows_the_application_menu() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        handle.set_app_menu(
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
            true,
        );
        handle.set_window_menu(
            win(1),
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
        );
        handle.activate_window(win(1));
        assert_eq!(backend.active_window(), Some(win(1)));
        handle.activate_window(win(2));
        assert_eq!(backend.active_window(), None);
        assert!(backend.shows_app_menu());
        handle.activate_window(win(1));
        assert_eq!(backend.active_window(), Some(win(1)), "and back");
    }

    /// A window that installs its menu while another is key does not take
    /// the bar; the key window that closes leaves the application menu.
    #[test]
    fn the_key_window_decides_and_its_close_leaves_the_application_menu() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        handle.activate_window(win(1));
        handle.set_window_menu(
            win(2),
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
        );
        assert!(backend.shows_app_menu(), "window 1 is key and has none");
        handle.set_window_menu(
            win(1),
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
        );
        assert_eq!(backend.active_window(), Some(win(1)));
        handle.clear_window(win(1));
        assert!(backend.shows_app_menu());
    }

    /// A row whose only effect is its intent greys out while nothing takes
    /// the intent, its own enabled state still counting; a row with an
    /// action, and the App menu's Quit, never do.
    #[test]
    fn an_intent_only_row_greys_out_while_nothing_takes_its_intent() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let (plain, with_action, quit) =
            (MenuItemId::next(), MenuItemId::next(), MenuItemId::next());
        let activation = |action: bool, quits: bool| NativeMenuActivation {
            intent: Some("app.print"),
            action: action.then(|| -> MenuActionFn { Rc::new(|_| {}) }),
            quits,
        };
        handle.set_window_menu(
            win(1),
            sample_snapshot(plain),
            [
                (plain, activation(false, false)),
                (with_action, activation(true, false)),
                (quit, activation(false, true)),
            ]
            .into_iter()
            .collect(),
            poster(),
        );
        let enabled_deltas = |backend: &MemoryNativeMenuBackend| {
            backend
                .deltas()
                .into_iter()
                .filter_map(|(id, delta)| delta.enabled.map(|enabled| (id, enabled)))
                .collect::<Vec<_>>()
        };
        handle.refresh_availability(&["app.print"]);
        assert!(enabled_deltas(&backend).is_empty(), "nothing changed");
        handle.refresh_availability(&[]);
        assert_eq!(enabled_deltas(&backend), [(plain, false)]);
        assert_eq!(handle.gated_enabled(plain), Some(false));
        assert_eq!(handle.gated_enabled(with_action), None);
        assert_eq!(handle.gated_enabled(quit), None);

        // Its own state turning true does not enable it while nothing takes
        // the intent; the intent taken again does.
        handle.update_item(
            plain,
            MenuItemDelta {
                enabled: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(enabled_deltas(&backend).last(), Some(&(plain, false)));
        handle.refresh_availability(&["app.print"]);
        assert_eq!(enabled_deltas(&backend).last(), Some(&(plain, true)));

        // Its own state turning false greys it out whatever the route says.
        handle.update_item(
            plain,
            MenuItemDelta {
                enabled: Some(false),
                ..Default::default()
            },
        );
        handle.refresh_availability(&["app.print"]);
        assert_eq!(enabled_deltas(&backend).last(), Some(&(plain, false)));

        handle.clear_window(win(1));
        assert_eq!(handle.gated_enabled(plain), None, "forgotten with its menu");
    }

    fn standard(role: StandardMenuRole, title: &str) -> NativeMenuNode {
        NativeMenuNode::Standard {
            role,
            labels: StandardLabels {
                title: title.into(),
                ..Default::default()
            },
            quit_item: None,
            settings_item: None,
        }
    }

    fn bar_titles(plan: &MacBarPlan<'_>) -> Vec<String> {
        plan.menus
            .iter()
            .map(|node| match node {
                NativeMenuNode::Submenu { title, .. } => title.clone(),
                NativeMenuNode::Standard { labels, .. } => labels.title.clone(),
                other => panic!("the bar holds menus only, got {other:?}"),
            })
            .collect()
    }

    /// AppKit shows the bar's first menu as the application menu, so the
    /// App-role menu goes first wherever the model declares it. Declared
    /// after File, File's rows sat under the application's name and About,
    /// Hide and Quit in a second, ordinary menu (macOS findings, item 2).
    #[test]
    fn the_app_menu_is_first_in_the_mac_bar_wherever_it_is_declared() {
        let mut file = sample_snapshot(MenuItemId::next()).roots;
        let snapshot = NativeMenuSnapshot {
            roots: vec![
                file.remove(0),
                NativeMenuNode::Separator,
                standard(StandardMenuRole::Window, "Window"),
                standard(StandardMenuRole::App, "Probe"),
                standard(StandardMenuRole::Help, "Help"),
                standard(StandardMenuRole::App, "Second"),
            ],
        };
        assert_eq!(
            bar_titles(&mac_bar_plan(&snapshot)),
            ["Probe", "File", "Window", "Help"],
            "App first, the rest in declared order, one App menu, no top-level separator"
        );

        let declared_first = NativeMenuSnapshot {
            roots: vec![
                standard(StandardMenuRole::App, "Probe"),
                sample_snapshot(MenuItemId::next()).roots.remove(0),
            ],
        };
        assert_eq!(
            bar_titles(&mac_bar_plan(&declared_first)),
            ["Probe", "File"]
        );
    }

    /// A window registered again in the Window menu is one AppKit lists:
    /// not excluded, and on screen or in the Dock. A hidden window (ordered
    /// out, not miniaturized) is left alone: flipping its flag lists it,
    /// since AppKit keeps a row for every window it evaluates with the flag
    /// off, and a window the app hid would show in the Window and Dock
    /// menus.
    #[test]
    fn the_mac_window_menu_relists_only_shown_or_miniaturized_windows() {
        assert!(mac_relists_window(false, true, false), "on screen");
        assert!(mac_relists_window(false, false, true), "in the Dock");
        assert!(!mac_relists_window(false, false, false), "hidden");
        assert!(
            !mac_relists_window(true, true, false),
            "excluded (a dialog)"
        );
    }

    /// The App menu's rows in AppKit's order: About; Settings in a group of
    /// its own; Services; Hide, Hide Others and Show All; Quit.
    #[test]
    fn the_mac_app_menu_has_appkits_rows_in_appkits_order() {
        use MacAppMenuRow::*;
        assert_eq!(
            mac_app_menu_rows(true),
            [
                About, Separator, Settings, Separator, Services, Separator, Hide, HideOthers,
                ShowAll, Separator, Quit,
            ]
        );
        assert_eq!(
            mac_app_menu_rows(false),
            [
                About, Separator, Services, Separator, Hide, HideOthers, ShowAll, Separator, Quit,
            ],
            "no Settings row, and no empty group for it"
        );
    }

    /// AppKit keeps the window list in the menu registered as `windowsMenu`,
    /// and the Dock icon's menu lists the windows from it: the bar's first
    /// Window-role menu when it declares one, else a hidden one the backend
    /// adds (Qt's `insertWindowMenu`), so the Dock still lists the windows
    /// (macOS findings, item 3).
    #[test]
    fn the_mac_bar_keeps_the_window_list_in_its_window_menu_or_a_hidden_one() {
        let snapshot = NativeMenuSnapshot {
            roots: vec![
                sample_snapshot(MenuItemId::next()).roots.remove(0),
                standard(StandardMenuRole::Window, "Window"),
                standard(StandardMenuRole::App, "Probe"),
                standard(StandardMenuRole::Window, "Second window menu"),
                standard(StandardMenuRole::Help, "Help"),
            ],
        };
        let plan = mac_bar_plan(&snapshot);
        assert_eq!(
            plan.windows_menu,
            Some(2),
            "the first Window menu, in bar order"
        );
        assert_eq!(plan.help_menu, Some(4));

        let no_window_menu = NativeMenuSnapshot {
            roots: vec![
                sample_snapshot(MenuItemId::next()).roots.remove(0),
                standard(StandardMenuRole::App, "Probe"),
            ],
        };
        let plan = mac_bar_plan(&no_window_menu);
        assert_eq!(plan.windows_menu, None, "a hidden one is added");
        assert_eq!(plan.help_menu, None);
    }

    /// An app menu titled like the snapshot's standard Window or Help menu
    /// is not shown beside it: its rows go into the standard menu. The
    /// `native-menu` example gave the in-window bar a Window menu (Inspector)
    /// and macOS its standard one, and the macOS bar showed Window twice.
    #[test]
    fn an_app_menu_titled_like_a_standard_one_folds_into_it_on_the_mac_bar() {
        let inspector = MenuItemId::next();
        let own = |title: &str, id: MenuItemId| NativeMenuNode::Submenu {
            title: title.into(),
            children: vec![NativeMenuNode::Item {
                id,
                title: "Row".into(),
                key_equiv: None,
                enabled: true,
                visible: true,
                check: NativeCheck::None,
            }],
        };
        let manual = MenuItemId::next();
        let snapshot = NativeMenuSnapshot {
            roots: vec![
                standard(StandardMenuRole::App, "Probe"),
                own("Window", inspector),
                own("Help", manual),
                standard(StandardMenuRole::Window, "Window"),
                standard(StandardMenuRole::Help, "Help"),
                own("Tools", MenuItemId::next()),
            ],
        };
        let plan = mac_bar_plan(&snapshot);
        assert_eq!(
            bar_titles(&plan),
            ["Probe", "Window", "Help", "Tools"],
            "one Window menu, one Help menu"
        );
        let ids = |rows: &[&NativeMenuNode]| -> Vec<MenuItemId> {
            rows.iter()
                .map(|node| match node {
                    NativeMenuNode::Item { id, .. } => *id,
                    other => panic!("expected a row, got {other:?}"),
                })
                .collect()
        };
        assert_eq!(
            ids(&plan.folded[1]),
            [inspector],
            "into the standard Window menu"
        );
        assert_eq!(
            ids(&plan.folded[2]),
            [manual],
            "into the standard Help menu"
        );
        assert!(plan.folded[0].is_empty() && plan.folded[3].is_empty());
        assert_eq!((plan.windows_menu, plan.help_menu), (Some(1), Some(2)));

        // With no standard Window menu, the app's own stays as it is.
        let alone = NativeMenuSnapshot {
            roots: vec![
                standard(StandardMenuRole::App, "Probe"),
                own("Window", MenuItemId::next()),
            ],
        };
        let plan = mac_bar_plan(&alone);
        assert_eq!(bar_titles(&plan), ["Probe", "Window"]);
        assert_eq!(plan.windows_menu, None);
    }

    /// The bar a key window shows: its own menu; else the menu of the window
    /// it belongs to (a modal dialog's owner, up the chain of owners); else
    /// the application menu, or, when the application declared none, the bar
    /// shown before, as AppKit keeps `mainMenu` up for its panels and
    /// sheets. A dialog used to show the application menu, so an app with
    /// no application menu lost File and Edit whenever one was key.
    #[test]
    fn a_key_window_without_a_menu_shows_its_owners_or_the_applications() {
        let menus = [win(1)];
        let has_menu = |w: TeksiloWindowId| menus.contains(&w);
        // 2 is a dialog of 1, 3 a dialog of 2; 4 is a secondary window.
        let owner = |w: TeksiloWindowId| match w.raw() {
            2 => Some(win(1)),
            3 => Some(win(2)),
            _ => None,
        };
        let bar = |key, declared, shown| key_window_bar(key, has_menu, owner, declared, shown);
        assert_eq!(bar(win(1), true, None), Some(win(1)), "its own");
        assert_eq!(bar(win(2), true, None), Some(win(1)), "its owner's");
        assert_eq!(bar(win(3), true, None), Some(win(1)), "up the chain");
        assert_eq!(bar(win(4), true, Some(win(1))), None, "the application's");
        assert_eq!(
            bar(win(4), false, Some(win(1))),
            Some(win(1)),
            "none declared: the bar shown before stays"
        );
        assert_eq!(bar(win(4), false, None), None);

        // An owner relation that loops ends the walk rather than the loop.
        let looping = |w: TeksiloWindowId| Some(if w == win(5) { win(6) } else { win(5) });
        assert_eq!(key_window_bar(win(5), has_menu, looping, true, None), None);
    }

    /// `MemoryNativeMenuBackend` follows the same rule as the macOS one.
    #[test]
    fn the_memory_backend_shows_a_dialogs_owners_menu() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        handle.set_app_menu(
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
            false,
        );
        handle.set_window_menu(
            win(1),
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
        );
        handle.set_window_owner(win(2), Some(win(1)));
        handle.activate_window(win(1));
        handle.activate_window(win(2));
        assert_eq!(
            backend.active_window(),
            Some(win(1)),
            "the dialog's owner's"
        );
        handle.activate_window(win(3));
        assert_eq!(
            backend.active_window(),
            Some(win(1)),
            "no application menu declared: a secondary window keeps the bar"
        );
        handle.set_app_menu(
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
            true,
        );
        handle.activate_window(win(3));
        assert!(backend.shows_app_menu(), "declared: the application's");
        handle.activate_window(win(2));
        assert_eq!(backend.active_window(), Some(win(1)));
        handle.clear_window(win(1));
        assert!(backend.shows_app_menu(), "the owner's menu went with it");
    }

    /// Declaring the application menu while a window with no menu of its own
    /// is key shows it at once, without waiting for focus to move: the bar
    /// left up was another window's, kept only because no application menu
    /// was declared. Declaring it while the key window shows its own menu
    /// leaves that one up.
    #[test]
    fn declaring_the_application_menu_shows_it_for_a_menu_less_key_window() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let app_menu = |declared| {
            handle.set_app_menu(
                sample_snapshot(MenuItemId::next()),
                HashMap::new(),
                poster(),
                declared,
            )
        };
        app_menu(false);
        handle.set_window_menu(
            win(1),
            sample_snapshot(MenuItemId::next()),
            HashMap::new(),
            poster(),
        );
        handle.activate_window(win(1));
        handle.activate_window(win(2));
        assert_eq!(backend.active_window(), Some(win(1)), "precondition");
        app_menu(true);
        assert!(backend.shows_app_menu(), "window 2 is key and has none");

        handle.activate_window(win(1));
        app_menu(true);
        assert_eq!(
            backend.active_window(),
            Some(win(1)),
            "a key window's own menu stays"
        );
    }

    /// After a window's menu is forgotten the bar is never that window's,
    /// whoever is key: the window itself, a dialog it owned, another window
    /// with or without a menu, none; with an application menu declared or
    /// not. A bar that was not the forgotten window's stays.
    #[test]
    fn a_cleared_windows_bar_never_comes_back() {
        let (closed, dialog, other, plain) = (win(1), win(2), win(3), win(4));
        // `has_menu` and `owner` still answer for the closed window, as a
        // backend that had not forgotten it yet would.
        let has_menu = |w: TeksiloWindowId| w == closed || w == other;
        let owner = |w: TeksiloWindowId| (w == dialog).then_some(closed);
        for declared in [true, false] {
            for key in [Some(closed), Some(dialog), Some(other), Some(plain), None] {
                let bar = bar_after_clear(closed, key, Some(closed), has_menu, owner, declared)
                    .expect("its bar was up: replaced");
                assert_ne!(bar, Some(closed), "key {key:?}, declared {declared}");
                if key == Some(other) {
                    assert_eq!(bar, Some(other), "the key window's own menu");
                }
            }
            assert_eq!(
                bar_after_clear(closed, Some(plain), Some(other), has_menu, owner, declared),
                None,
                "another window's bar stays"
            );
        }
    }

    /// The rule both backends follow when the application menu is set:
    /// show what [`key_window_bar`] picks for the key window when it differs
    /// from the bar up, and the new application menu whenever that is the
    /// pick; with no window key yet, replace only an application menu
    /// already showing.
    #[test]
    fn what_setting_the_application_menu_shows() {
        let app = None;
        assert_eq!(bar_after_app_menu(Some(app), Some(win(1))), Some(app));
        assert_eq!(bar_after_app_menu(Some(app), app), Some(app), "rebuilt");
        assert_eq!(bar_after_app_menu(Some(Some(win(1))), Some(win(1))), None);
        assert_eq!(
            bar_after_app_menu(Some(Some(win(1))), app),
            Some(Some(win(1)))
        );
        assert_eq!(bar_after_app_menu(None, app), Some(app));
        assert_eq!(bar_after_app_menu(None, Some(win(1))), None);
    }

    /// A refresh whose inputs are the last refresh's (the same intents
    /// taken, no menu installed since) evaluates no row: the loop asks every
    /// turn, and most turns change nothing. A menu installed since, or
    /// another answer, is evaluated again.
    #[test]
    fn an_unchanged_refresh_evaluates_nothing() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let row = MenuItemId::next();
        let intent_only = || NativeMenuActivation {
            intent: Some("app.print"),
            action: None,
            quits: false,
        };
        handle.set_window_menu(
            win(1),
            sample_snapshot(row),
            [(row, intent_only())].into_iter().collect(),
            poster(),
        );
        handle.refresh_availability(&["app.print"]);
        assert_eq!(handle.evaluations(), 1);
        handle.refresh_availability(&["app.print"]);
        assert_eq!(handle.evaluations(), 1, "nothing changed: no row evaluated");

        let other = MenuItemId::next();
        handle.set_window_menu(
            win(2),
            sample_snapshot(other),
            [(other, intent_only())].into_iter().collect(),
            poster(),
        );
        handle.refresh_availability(&["app.print"]);
        assert_eq!(handle.evaluations(), 3, "a menu installed since: both rows");
        handle.refresh_availability(&[]);
        assert_eq!(handle.evaluations(), 5, "another answer");
        assert_eq!(handle.gated_enabled(other), Some(false));
    }

    /// The no-op backend shows no menu bar, so the loop skips the
    /// availability refresh altogether where it is the backend.
    #[test]
    fn only_a_backend_with_a_menu_bar_says_it_shows_one() {
        assert!(!NativeMenuHandle::new(NoopNativeMenuBackend::new()).shows_bar());
        assert!(NativeMenuHandle::new(MemoryNativeMenuBackend::new()).shows_bar());
    }

    #[test]
    fn update_item_records_delta() {
        let backend = MemoryNativeMenuBackend::new();
        let handle = NativeMenuHandle::new(backend.clone());
        let id = MenuItemId::next();
        handle.update_item(
            id,
            MenuItemDelta {
                enabled: Some(false),
                check: Some(NativeCheck::On),
                ..Default::default()
            },
        );
        let deltas = backend.deltas();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].0, id);
        assert_eq!(deltas[0].1.enabled, Some(false));
        assert_eq!(deltas[0].1.check, Some(NativeCheck::On));
    }

    #[test]
    fn noop_backend_is_inert() {
        let handle = NativeMenuHandle::new(NoopNativeMenuBackend::new());
        let id = MenuItemId::next();
        handle.set_window_menu(win(1), sample_snapshot(id), HashMap::new(), poster());
        handle.activate_window(win(1));
        handle.update_item(id, MenuItemDelta::default());
        handle.clear_window(win(1));
        // No activation was recorded for noop set? Activations live in the
        // handle, not the backend, so they ARE recorded then cleared.
        assert!(handle.activation(win(1), id).is_none());
    }
}
