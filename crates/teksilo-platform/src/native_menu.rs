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
//! `teksilo-app` picks it up in its `AppEvent::External` arm, looks the
//! [`MenuItemId`] up in the [`NativeMenuHandle`], and fires the item's intent /
//! action inside the originating window's `EventContext` — the same
//! `Action`/`Intent` pipeline an in-window `MenuItem` uses.
//!
//! # Multi-window
//!
//! On macOS there is exactly one global menu bar; it must reflect the *focused*
//! window. Each window registers its snapshot via [`NativeMenuHandle::set_window_menu`];
//! `teksilo-app` calls [`NativeMenuHandle::activate_window`] on focus change so
//! the focused window's menu becomes `mainMenu`. Single-window apps work with
//! set-on-build alone.

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
/// macOS App / Window / Help menus, with their About / Hide / Quit /
/// window-management items wired to system selectors). The backend supplies the
/// native structure; the in-window `MenuBar` ignores these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardMenuRole {
    /// The application menu (About / Hide / Quit). Must be first.
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
    /// "Hide …" (App).
    pub hide: String,
    /// "Quit …" (App).
    pub quit: String,
    /// "Minimize" (Window).
    pub minimize: String,
    /// "Zoom" (Window).
    pub zoom: String,
}

/// One node of a native menu tree.
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
        /// App menu only: route **Quit** back to the app under this id instead
        /// of firing the platform's own terminate selector.
        ///
        /// `None` — the default — keeps the system behaviour: on macOS the item
        /// is bound to `terminate:`, which works with no app wiring at all and
        /// is why ⌘Q is live even for an app that declares no menus.
        ///
        /// `Some(id)` builds Quit as an ordinary routed item — same id → the
        /// activation recorded for it, same ⌘Q key equivalent — so choosing it
        /// (or pressing ⌘Q) reaches the app's own handler. **An app with
        /// anything to lose on exit must set this**: a main-menu key equivalent
        /// is dispatched by the platform before the responder chain, and
        /// `terminate:` does not run winit's exit path, so an in-app quit
        /// shortcut is shadowed rather than merely duplicated. Whatever the app
        /// routes to then owes the exit itself — nothing here terminates.
        quit_item: Option<MenuItemId>,
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
/// A menu item's direct activation closure.
pub type MenuActionFn = Rc<dyn Fn(&mut EventContext)>;

/// `Rc`), so the router can pull a copy out of the handle and run it.
#[derive(Clone, Default)]
pub struct NativeMenuActivation {
    /// Fire this intent by name through the `Action`/`Intent` pipeline.
    pub intent: Option<&'static str>,
    /// Or run this closure directly (the escape hatch). Runs after `intent`.
    pub action: Option<MenuActionFn>,
}

impl std::fmt::Debug for NativeMenuActivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeMenuActivation")
            .field("intent", &self.intent)
            .field("action", &self.action.as_ref().map(|_| "<closure>"))
            .finish()
    }
}

// ============================================================
// Event payload
// ============================================================

/// Boxed inside `AppEvent::External` when the user picks a native menu item.
/// `teksilo-app` downcasts to this and routes the [`MenuItemId`] back to the
/// originating window's tree.
#[derive(Debug, Clone)]
pub struct NativeMenuEventPayload {
    /// The window whose menu was active when the item was chosen.
    pub window_id_owner: TeksiloWindowId,
    /// The chosen item.
    pub item_id: MenuItemId,
}

// ============================================================
// Backend trait
// ============================================================

/// Swappable native-menu backend. One instance serves the whole app.
pub trait NativeMenuBackend {
    /// Build (or replace) the native menu for `window_id` from `menu`. For
    /// every item the user later chooses, the backend MUST post a
    /// [`NativeMenuEventPayload`] — with `window_id_owner == window_id` —
    /// through `poster`. If `window_id` is (or becomes) the active window, the
    /// backend should also make this menu the visible one.
    fn set_window_menu(
        &mut self,
        window_id: TeksiloWindowId,
        menu: NativeMenuSnapshot,
        poster: Arc<dyn AppEventPoster>,
    );

    /// Make `window_id`'s previously-set menu the active/visible one (focus
    /// follows window). No-op if that window never set a menu.
    fn activate_window(&mut self, window_id: TeksiloWindowId);

    /// Forget `window_id`'s menu (window closed).
    fn clear_window(&mut self, window_id: TeksiloWindowId);

    /// Apply a reactive delta to a single already-installed item.
    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta);
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
    fn clear_window(&mut self, window_id: TeksiloWindowId) {
        (**self).clear_window(window_id)
    }
    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta) {
        (**self).update_item(id, delta)
    }
}

// ============================================================
// NativeMenuHandle
// ============================================================

/// Per-window map from item id to its activation.
type WindowActivations = HashMap<MenuItemId, NativeMenuActivation>;

struct NativeMenuState {
    backend: RefCell<Box<dyn NativeMenuBackend>>,
    /// Per-window: item id → what to do when chosen.
    activations: RefCell<HashMap<TeksiloWindowId, WindowActivations>>,
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
        self.inner
            .activations
            .borrow_mut()
            .insert(window_id, activations);
        self.inner
            .backend
            .borrow_mut()
            .set_window_menu(window_id, menu, poster);
    }

    /// Make `window_id`'s menu the visible one (focus-follows-window).
    pub fn activate_window(&self, window_id: TeksiloWindowId) {
        self.inner.backend.borrow_mut().activate_window(window_id);
    }

    /// Forget a window's menu + activations (window closed).
    pub fn clear_window(&self, window_id: TeksiloWindowId) {
        self.inner.activations.borrow_mut().remove(&window_id);
        self.inner.backend.borrow_mut().clear_window(window_id);
    }

    /// Apply a reactive delta to one installed item.
    pub fn update_item(&self, id: MenuItemId, delta: MenuItemDelta) {
        self.inner.backend.borrow_mut().update_item(id, delta);
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
    active: Option<TeksiloWindowId>,
    deltas: Vec<(MenuItemId, MenuItemDelta)>,
    cleared: Vec<TeksiloWindowId>,
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

    /// The window whose menu is active (last `activate_window`, or the window
    /// of the most recent `set_window_menu` if none was activated).
    pub fn active_window(&self) -> Option<TeksiloWindowId> {
        self.inner.borrow().active
    }

    /// All item deltas applied so far, in order.
    pub fn deltas(&self) -> Vec<(MenuItemId, MenuItemDelta)> {
        self.inner.borrow().deltas.clone()
    }

    /// Windows whose menus were cleared, in order.
    pub fn cleared(&self) -> Vec<TeksiloWindowId> {
        self.inner.borrow().cleared.clone()
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
        // First menu set becomes active by default (mirrors the real backend
        // installing the first window's menu as mainMenu).
        if rec.active.is_none() {
            rec.active = Some(window_id);
        }
    }
    fn activate_window(&mut self, window_id: TeksiloWindowId) {
        self.inner.borrow_mut().active = Some(window_id);
    }
    fn clear_window(&mut self, window_id: TeksiloWindowId) {
        let mut rec = self.inner.borrow_mut();
        rec.menus.remove(&window_id);
        rec.cleared.push(window_id);
        if rec.active == Some(window_id) {
            rec.active = None;
        }
    }
    fn update_item(&mut self, id: MenuItemId, delta: MenuItemDelta) {
        self.inner.borrow_mut().deltas.push((id, delta));
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
