// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-window reactive state.
//!
//! A [`WindowState`] is a refcounted handle to the signal-bound surface
//! of a single window. Widgets bind against these signals
//! (`ctx.window().placement().map(|p| ...)`) for reactive UI that
//! stays in sync with the OS; app code writes to the signals to push
//! state to the OS.
//!
//! ## Two-way sync pattern
//!
//! Every public signal has two writers:
//!
//! - **App-side writes** — `state.title().set("Hello")` or any code
//!   that assigns through the `Signal` handle returned from the
//!   getter. These fire the observer wired in [`WindowState::new`];
//!   the observer pushes a [`WindowCommand`] into
//!   `WindowStateInner::pending_os_commands`, which the app-level
//!   window manager drains once per tick and translates into a winit
//!   call.
//!
//! - **OS-side writes** — the app-level window manager calls the
//!   private `set_*_from_os` methods on `WindowStateInner` when a
//!   winit `WindowEvent` reports that the OS changed state. Those
//!   setters flip the `applying_from_os` guard
//!   before updating the signal; the observer sees the guard is set
//!   and skips enqueuing a command. Without this guard, every
//!   OS-initiated change would loop back into a redundant OS call —
//!   at best wasteful, at worst a mid-animation state-drift bug
//!   (Compose Multiplatform issues #1489, #4006).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::signal::{ObserverHandle, Signal};

use super::command::{UserAttentionKind, WindowCommand};
use super::id::BastydeWindowId;
use super::placement::WindowPlacement;

/// A refcounted handle to a single window's reactive state.
///
/// Cloning gives you another handle to the same underlying state.
/// Widgets should store a [`WindowState`] clone when they need to read
/// or write window-level signals outside of a single `build()` call.
#[derive(Clone)]
pub struct WindowState {
    inner: Rc<WindowStateInner>,
}

impl std::fmt::Debug for WindowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowState")
            .field("id", &self.inner.id)
            .field("string_id", &self.inner.string_id)
            .field("placement", &self.inner.placement.get())
            .field("title", &self.inner.title.get())
            .field("size", &self.inner.size.get())
            .field("position", &self.inner.position.get())
            .field("focused", &self.inner.focused.get())
            .field("resizable", &self.inner.resizable.get())
            .field("always_on_top", &self.inner.always_on_top.get())
            .field(
                "pending_commands",
                &self.inner.pending_os_commands.borrow().len(),
            )
            .finish()
    }
}

/// Initial values for a [`WindowState`] at creation time.
///
/// Built from the equivalent fields on `WindowConfig` by the app-level
/// window manager, then passed to [`WindowState::new`].
#[derive(Debug, Clone)]
pub struct WindowStateInit {
    pub id: BastydeWindowId,
    pub string_id: Option<String>,
    pub placement: WindowPlacement,
    pub title: String,
    pub size: (u32, u32),
    pub position: (i32, i32),
    pub focused: bool,
    pub resizable: bool,
    pub always_on_top: bool,
}

pub(crate) struct WindowStateInner {
    id: BastydeWindowId,
    string_id: Option<String>,

    placement: Signal<WindowPlacement>,
    title: Signal<String>,
    size: Signal<(u32, u32)>,
    position: Signal<(i32, i32)>,
    focused: Signal<bool>,
    resizable: Signal<bool>,
    always_on_top: Signal<bool>,
    /// Caps Lock active state, OS-driven only — no observer and no
    /// app→OS command (the app never sets the keyboard lock). Toggled by
    /// the window manager on each `Key::CapsLock` press; read by password
    /// fields to show a Caps Lock warning.
    caps_lock: Signal<bool>,

    /// `true` while the Alt key is currently held down. OS-driven only —
    /// no observer and no app→OS command. Set by the window manager on
    /// `Key::Alt` `KeyDown`/`KeyUp`. Read by:
    ///
    /// - `MenuLabel` to show / hide mnemonic underlines while Alt is held
    ///   (matches the Win32 `WM_CHANGEUISTATE` underlining convention).
    /// - `MenuBar` to detect bare-Alt-tap (true → false transition with
    ///   `other_key_pressed_during_alt == false`) which focuses the
    ///   first trigger.
    alt_down: Signal<bool>,

    /// Sticky flag that records whether any non-Alt key was pressed
    /// while Alt was held. Set to `false` by the window manager on
    /// every `Key::Alt` `KeyDown`; flipped to `true` by the manager
    /// on any non-Alt `KeyDown` that arrives while `alt_down` is
    /// `true`. Read by `MenuBar` at the Alt → release moment to
    /// decide whether the user did a bare-Alt-tap (no other key
    /// pressed → focus menubar) or used Alt as a modifier for a
    /// real chord (skip).
    ///
    /// `Cell<bool>` rather than `Signal<bool>` — only the MenuBar
    /// effect reads it at the transition moment, never reactively.
    other_key_pressed_during_alt: Cell<bool>,

    /// At-most-one menubar dispatcher per window. See
    /// [`super::menubar_dispatcher`]. Wrapped in `Rc` so the slot
    /// can be shared with the [`super::menubar_dispatcher::MenubarGuard`]
    /// returned to the caller — when the guard drops, it clears the
    /// slot iff it still points to the same dispatcher.
    menubar_dispatcher: Rc<super::menubar_dispatcher::MenubarDispatcherSlot>,

    /// Commands queued by observers on app-side signal writes. Drained
    /// by the app-level window manager once per tick.
    pending_os_commands: RefCell<Vec<WindowCommand>>,

    /// A pending `xdg_activation_v1` token stashed by
    /// [`WindowState::set_activation_token`] and consumed by the next
    /// [`WindowState::focus`], which carries it on the emitted
    /// [`WindowCommand::Focus`]. Only meaningful for Wayland cross-process
    /// raises; ignored on every other platform.
    pending_activation_token: RefCell<Option<String>>,

    /// `true` while a `set_*_from_os` call is in progress. The
    /// observers installed in [`WindowState::new`] check this flag and
    /// do nothing when it is set — the OS already knows, there is no
    /// command to send back.
    applying_from_os: Cell<bool>,

    /// Holds the `ObserverHandle`s returned from
    /// [`Signal::observe`] during construction. They must stay alive
    /// for the lifetime of the `WindowState`; dropping them would
    /// silently unsubscribe the OS-sync observers.
    _observer_handles: RefCell<Vec<ObserverHandle>>,
}

impl WindowState {
    /// Construct a new state from initial values.
    ///
    /// Wires an observer on every signal that pushes a matching
    /// [`WindowCommand`] onto the pending queue, guarded by
    /// `WindowStateInner::applying_from_os`.
    pub fn new(init: WindowStateInit) -> Self {
        let inner = Rc::new(WindowStateInner {
            id: init.id,
            string_id: init.string_id,
            placement: Signal::new(init.placement),
            title: Signal::new(init.title),
            size: Signal::new(init.size),
            position: Signal::new(init.position),
            focused: Signal::new(init.focused),
            resizable: Signal::new(init.resizable),
            always_on_top: Signal::new(init.always_on_top),
            caps_lock: Signal::new(false),
            alt_down: Signal::new(false),
            other_key_pressed_during_alt: Cell::new(false),
            menubar_dispatcher: Rc::new(RefCell::new(None)),
            pending_os_commands: RefCell::new(Vec::new()),
            pending_activation_token: RefCell::new(None),
            applying_from_os: Cell::new(false),
            _observer_handles: RefCell::new(Vec::new()),
        });

        // Wire the observers. Each one queues a WindowCommand on
        // app-side writes and silently ignores OS-originated writes.
        let mut handles = Vec::new();

        {
            let inner_w = Rc::downgrade(&inner);
            handles.push(inner.placement.observe(move |v| {
                if let Some(inner) = inner_w.upgrade() {
                    inner.enqueue_unless_from_os(WindowCommand::SetPlacement(*v));
                }
            }));
        }
        {
            let inner_w = Rc::downgrade(&inner);
            handles.push(inner.title.observe(move |v| {
                if let Some(inner) = inner_w.upgrade() {
                    inner.enqueue_unless_from_os(WindowCommand::SetTitle(v.clone()));
                }
            }));
        }
        {
            let inner_w = Rc::downgrade(&inner);
            handles.push(inner.size.observe(move |v| {
                if let Some(inner) = inner_w.upgrade() {
                    inner.enqueue_unless_from_os(WindowCommand::SetSize(v.0, v.1));
                }
            }));
        }
        {
            let inner_w = Rc::downgrade(&inner);
            handles.push(inner.position.observe(move |v| {
                if let Some(inner) = inner_w.upgrade() {
                    inner.enqueue_unless_from_os(WindowCommand::SetPosition(v.0, v.1));
                }
            }));
        }
        {
            let inner_w = Rc::downgrade(&inner);
            handles.push(inner.resizable.observe(move |v| {
                if let Some(inner) = inner_w.upgrade() {
                    inner.enqueue_unless_from_os(WindowCommand::SetResizable(*v));
                }
            }));
        }
        {
            let inner_w = Rc::downgrade(&inner);
            handles.push(inner.always_on_top.observe(move |v| {
                if let Some(inner) = inner_w.upgrade() {
                    inner.enqueue_unless_from_os(WindowCommand::SetAlwaysOnTop(*v));
                }
            }));
        }
        // `focused` has no observer: it is purely OS-driven. Writes
        // go through `set_focused_from_os`; app code that wants to
        // pull focus calls `focus()` instead, which emits
        // `WindowCommand::Focus` directly.

        *inner._observer_handles.borrow_mut() = handles;
        Self { inner }
    }

    pub fn id(&self) -> BastydeWindowId {
        self.inner.id
    }

    pub fn string_id(&self) -> Option<&str> {
        self.inner.string_id.as_deref()
    }

    pub fn placement(&self) -> &Signal<WindowPlacement> {
        &self.inner.placement
    }

    pub fn title(&self) -> &Signal<String> {
        &self.inner.title
    }

    pub fn size(&self) -> &Signal<(u32, u32)> {
        &self.inner.size
    }

    pub fn position(&self) -> &Signal<(i32, i32)> {
        &self.inner.position
    }

    pub fn focused(&self) -> &Signal<bool> {
        &self.inner.focused
    }

    pub fn resizable(&self) -> &Signal<bool> {
        &self.inner.resizable
    }

    pub fn always_on_top(&self) -> &Signal<bool> {
        &self.inner.always_on_top
    }

    /// Caps Lock active state. OS-driven only — the window manager
    /// toggles it on each `Key::CapsLock` press. Read this (e.g. via
    /// `ctx.window()`) to drive a Caps Lock warning on password fields.
    pub fn caps_lock(&self) -> &Signal<bool> {
        &self.inner.caps_lock
    }

    /// Whether the Alt key is currently held down. OS-driven only — the
    /// window manager flips this on `Key::Alt` `KeyDown` / `KeyUp`. Read
    /// this to drive mnemonic-underline visibility on menus and menubars.
    /// See the menubar key-dispatch documentation for the full Alt-tap
    /// / Alt+letter / mnemonic-underline contract.
    pub fn alt_down(&self) -> &Signal<bool> {
        &self.inner.alt_down
    }

    /// Read whether any non-Alt key has been pressed during the
    /// current Alt-hold window. `false` means the user has not
    /// composed a chord since pressing Alt; the next Alt-release
    /// counts as a bare-Alt-tap. Read by the MenuBar dispatcher.
    pub fn other_key_pressed_during_alt(&self) -> bool {
        self.inner.other_key_pressed_during_alt.get()
    }

    /// Install (or replace) the per-window menubar key dispatcher.
    /// Returns a [`super::menubar_dispatcher::MenubarGuard`] that
    /// clears the slot on drop iff it still points at this exact
    /// dispatcher (`Rc::ptr_eq`).
    ///
    /// At most one dispatcher is supported per window. A second
    /// install while another is still live `debug_assert!`s and
    /// overwrites in release. This matches the "one MenuBar per
    /// window" invariant the framework enforces upstream.
    pub fn install_menubar_dispatcher(
        &self,
        dispatcher: Rc<dyn super::menubar_dispatcher::MenubarDispatcher>,
    ) -> super::menubar_dispatcher::MenubarGuard {
        let slot = self.inner.menubar_dispatcher.clone();
        {
            let mut slot_ref = slot.borrow_mut();
            debug_assert!(
                slot_ref.is_none(),
                "WindowState: a menubar dispatcher is already installed for this \
                 window — only one keyboard-dispatching MenuBar is supported per \
                 window because the dispatcher slot routes F10 / Alt+letter / \
                 Alt-tap to exactly one trigger set.\n\n\
                 \
                 Causes typically fall into one of:\n\
                 \
                 1. Two MenuBar widgets are mounted in the same window. Pick \
                    the primary one — the one that should own F10 / Alt+letter \
                    — and call `.no_dispatcher_install()` on every secondary \
                    MenuBar (showcase content, embedded demos, settings \
                    previews, …). Secondary MenuBars still render and respond \
                    to mouse + arrow-key navigation; only the window-level \
                    keyboard dispatch is left to the primary.\n\
                 \
                 2. A MenuBar was added with `tree.add_boxed(MenuBar::new()…)` \
                    from inside another MenuBar's tab / popover / submenu. Same \
                    fix: mark the inner one with `.no_dispatcher_install()`.\n\
                 \
                 3. The host widget that owns the MenuBar reconstructs a fresh \
                    `MenuBar::new()` on every rebuild without dropping its \
                    previous `MenubarGuard` first. `MenuBar::build` already \
                    handles its own rebuild path; if you're hand-rolling the \
                    install (custom MenubarDispatcher impl), drop the old \
                    `MenubarGuard` BEFORE calling this method again."
            );
            *slot_ref = Some(dispatcher.clone());
        }
        super::menubar_dispatcher::MenubarGuard {
            slot,
            own: dispatcher,
        }
    }

    /// Snapshot of the currently-installed menubar dispatcher. Used by
    /// `bastyde-app`'s key-event arm to consult the menubar BEFORE
    /// focus-based dispatch. Returns `None` when no `MenuBar` is
    /// mounted in this window.
    pub fn menubar_dispatcher(
        &self,
    ) -> Option<Rc<dyn super::menubar_dispatcher::MenubarDispatcher>> {
        self.inner.menubar_dispatcher.borrow().clone()
    }

    /// Request user attention (bouncing dock icon on macOS, flashing
    /// taskbar on Windows). Queues a [`WindowCommand::RequestAttention`]
    /// command for the next drain.
    pub fn request_attention(&self, kind: UserAttentionKind) {
        self.inner
            .pending_os_commands
            .borrow_mut()
            .push(WindowCommand::RequestAttention(kind));
    }

    /// Focus this window — raise it above others and give it keyboard
    /// focus. Queues a [`WindowCommand::Focus`] command for the next
    /// drain, carrying (and clearing) any token set via
    /// [`WindowState::set_activation_token`].
    pub fn focus(&self) {
        let activation_token = self.inner.pending_activation_token.borrow_mut().take();
        self.inner
            .pending_os_commands
            .borrow_mut()
            .push(WindowCommand::Focus { activation_token });
    }

    /// Stash an `xdg_activation_v1` token — an opaque string minted by the
    /// focused requester and handed across a process boundary — to be consumed
    /// by the next [`WindowState::focus`]. Only affects a Wayland raise;
    /// ignored on every other platform, where `focus()` raises on its own.
    pub fn set_activation_token(&self, token: String) {
        *self.inner.pending_activation_token.borrow_mut() = Some(token);
    }

    /// Close this window. Queues a [`WindowCommand::Close`] command
    /// for the next drain.
    pub fn close(&self) {
        self.inner
            .pending_os_commands
            .borrow_mut()
            .push(WindowCommand::Close);
    }

    /// Test helper: returns the count of pending commands without
    /// draining. Test-only to avoid exposing queue state to
    /// application code.
    #[cfg(test)]
    pub(crate) fn pending_command_count(&self) -> usize {
        self.inner.pending_os_commands.borrow().len()
    }
}

// Framework-internal write-back API consumed by the app-level window
// manager when a winit `WindowEvent` reports an OS-initiated state
// change. Each method flips the re-entrancy guard before updating the
// signal so the observers do not push the same change back out as a
// [`WindowCommand`], which would at best duplicate work and at worst
// cause OS↔app drift mid-animation (Compose Multiplatform #1489).
//
// These are `pub` rather than `pub(crate)` because bastyde-app lives in a
// separate crate. Application code should never call them; they read
// like internals and have no stability guarantee. Use the public
// signal setters instead — those fire OS commands through the normal
// drain path.
impl WindowState {
    /// Drain the pending OS-command queue.
    pub fn drain_os_commands(&self) -> Vec<WindowCommand> {
        std::mem::take(&mut *self.inner.pending_os_commands.borrow_mut())
    }

    /// OS-originated placement write. Observers do not push back to
    /// the OS while the guard is set.
    pub fn set_placement_from_os(&self, p: WindowPlacement) {
        self.inner.with_os_guard(|| self.inner.placement.set(p));
    }

    pub fn set_title_from_os(&self, title: String) {
        self.inner.with_os_guard(|| self.inner.title.set(title));
    }

    pub fn set_size_from_os(&self, size: (u32, u32)) {
        self.inner.with_os_guard(|| self.inner.size.set(size));
    }

    pub fn set_position_from_os(&self, position: (i32, i32)) {
        self.inner
            .with_os_guard(|| self.inner.position.set(position));
    }

    pub fn set_focused_from_os(&self, focused: bool) {
        self.inner.with_os_guard(|| self.inner.focused.set(focused));
    }

    pub fn set_resizable_from_os(&self, resizable: bool) {
        self.inner
            .with_os_guard(|| self.inner.resizable.set(resizable));
    }

    pub fn set_always_on_top_from_os(&self, on_top: bool) {
        self.inner
            .with_os_guard(|| self.inner.always_on_top.set(on_top));
    }

    /// Update Caps Lock state from the OS. No observer / command is
    /// wired (the app never drives the keyboard lock), so this writes the
    /// signal directly. Idempotent: skips the write when unchanged to
    /// avoid spurious repaints on auto-repeat.
    pub fn set_caps_lock_from_os(&self, active: bool) {
        if self.inner.caps_lock.get() != active {
            self.inner.caps_lock.set(active);
        }
    }

    /// Update Alt-held state from the OS. Resets the
    /// `other_key_pressed_during_alt` flag on every Alt KeyDown
    /// edge so each Alt-hold window starts fresh. Idempotent.
    pub fn set_alt_from_os(&self, active: bool) {
        if self.inner.alt_down.get() != active {
            self.inner.alt_down.set(active);
            if active {
                // Fresh Alt-hold window — no other key has been
                // pressed yet during this hold.
                self.inner.other_key_pressed_during_alt.set(false);
            }
        }
    }

    /// Mark that a non-Alt key was pressed while Alt is currently
    /// held. Sticky — only cleared by the next
    /// [`set_alt_from_os(true)`](Self::set_alt_from_os) edge. No-op
    /// when Alt is not held. Idempotent.
    pub fn note_non_alt_keydown_during_alt(&self) {
        if self.inner.alt_down.get() {
            self.inner.other_key_pressed_during_alt.set(true);
        }
    }
}

impl WindowStateInner {
    fn enqueue_unless_from_os(&self, cmd: WindowCommand) {
        if self.applying_from_os.get() {
            return;
        }
        self.pending_os_commands.borrow_mut().push(cmd);
    }

    fn with_os_guard<R>(&self, f: impl FnOnce() -> R) -> R {
        // Set-and-restore rather than set-true-then-false: re-entry
        // through nested signal observers stays correct.
        let prev = self.applying_from_os.replace(true);
        let out = f();
        self.applying_from_os.set(prev);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init(id: u64) -> WindowStateInit {
        WindowStateInit {
            id: BastydeWindowId::new(id),
            string_id: Some("test".to_string()),
            placement: WindowPlacement::Floating,
            title: "Test".to_string(),
            size: (800, 600),
            position: (0, 0),
            focused: false,
            resizable: true,
            always_on_top: false,
        }
    }

    #[test]
    fn app_side_write_enqueues_command() {
        let state = WindowState::new(init(1));
        state.placement().set(WindowPlacement::Fullscreen);
        let cmds = state.drain_os_commands();
        assert_eq!(
            cmds,
            vec![WindowCommand::SetPlacement(WindowPlacement::Fullscreen)]
        );
    }

    #[test]
    fn os_side_write_does_not_enqueue_command() {
        let state = WindowState::new(init(1));
        state.set_placement_from_os(WindowPlacement::Maximized);
        assert_eq!(state.placement().get(), WindowPlacement::Maximized);
        assert_eq!(state.drain_os_commands(), vec![]);
    }

    #[test]
    fn os_side_write_still_notifies_derived_signals() {
        let state = WindowState::new(init(1));
        let is_fs = state.placement().map(|p| p.is_fullscreen());
        assert!(!is_fs.get());
        state.set_placement_from_os(WindowPlacement::Fullscreen);
        assert!(is_fs.get());
        // ... but no OS command was emitted.
        assert_eq!(state.drain_os_commands(), vec![]);
    }

    #[test]
    fn drain_is_consuming() {
        let state = WindowState::new(init(1));
        state.title().set("One".to_string());
        state.title().set("Two".to_string());
        assert_eq!(state.pending_command_count(), 2);
        let _ = state.drain_os_commands();
        assert_eq!(state.pending_command_count(), 0);
    }

    #[test]
    fn focus_close_attention_do_not_depend_on_signals() {
        let state = WindowState::new(init(1));
        state.focus();
        state.close();
        state.request_attention(UserAttentionKind::Critical);
        let cmds = state.drain_os_commands();
        assert_eq!(
            cmds,
            vec![
                WindowCommand::Focus {
                    activation_token: None
                },
                WindowCommand::Close,
                WindowCommand::RequestAttention(UserAttentionKind::Critical),
            ]
        );
    }

    #[test]
    fn multiple_app_writes_of_different_fields() {
        let state = WindowState::new(init(1));
        state.title().set("Hello".to_string());
        state.size().set((1200, 800));
        state.resizable().set(false);
        let cmds = state.drain_os_commands();
        assert_eq!(
            cmds,
            vec![
                WindowCommand::SetTitle("Hello".to_string()),
                WindowCommand::SetSize(1200, 800),
                WindowCommand::SetResizable(false),
            ]
        );
    }

    #[test]
    fn guard_is_scoped_to_a_single_from_os_call() {
        let state = WindowState::new(init(1));
        // First OS-originated change: guard suppresses command.
        state.set_size_from_os((1024, 768));
        assert_eq!(state.drain_os_commands(), vec![]);
        // Now an app-side write still works as normal.
        state.size().set((640, 480));
        assert_eq!(
            state.drain_os_commands(),
            vec![WindowCommand::SetSize(640, 480)]
        );
    }

    #[test]
    fn id_and_string_id_accessors() {
        let state = WindowState::new(init(42));
        assert_eq!(state.id(), BastydeWindowId::new(42));
        assert_eq!(state.string_id(), Some("test"));
    }

    #[test]
    fn state_is_cloneable_and_shares_storage() {
        let a = WindowState::new(init(1));
        let b = a.clone();
        a.title().set("From a".to_string());
        assert_eq!(b.title().get(), "From a");
        // Either handle can drain — they share the same queue.
        let cmds = b.drain_os_commands();
        assert_eq!(cmds.len(), 1);
        assert_eq!(a.pending_command_count(), 0);
    }

    // --- Alt-down tracking ---

    #[test]
    fn alt_down_signal_defaults_false() {
        let state = WindowState::new(init(1));
        assert!(!state.alt_down().get());
        assert!(!state.other_key_pressed_during_alt());
    }

    #[test]
    fn set_alt_from_os_toggles_signal() {
        let state = WindowState::new(init(1));
        state.set_alt_from_os(true);
        assert!(state.alt_down().get());
        state.set_alt_from_os(false);
        assert!(!state.alt_down().get());
    }

    #[test]
    fn set_alt_from_os_does_not_enqueue_command() {
        // Alt is keyboard-driven; the app cannot drive it.
        let state = WindowState::new(init(1));
        state.set_alt_from_os(true);
        assert!(state.drain_os_commands().is_empty());
    }

    #[test]
    fn alt_down_edge_resets_other_key_flag() {
        let state = WindowState::new(init(1));
        // Alt+letter chord: Alt down, then letter pressed.
        state.set_alt_from_os(true);
        state.note_non_alt_keydown_during_alt();
        assert!(state.other_key_pressed_during_alt());
        // Release Alt — flag persists (the consumer reads it on
        // release to decide whether the tap is bare or chorded).
        state.set_alt_from_os(false);
        assert!(state.other_key_pressed_during_alt());
        // Next Alt-down edge resets the flag so the new hold window
        // starts fresh.
        state.set_alt_from_os(true);
        assert!(!state.other_key_pressed_during_alt());
    }

    #[test]
    fn note_non_alt_keydown_is_noop_when_alt_not_held() {
        let state = WindowState::new(init(1));
        state.note_non_alt_keydown_during_alt();
        assert!(!state.other_key_pressed_during_alt());
    }

    #[test]
    fn alt_signal_observers_fire_on_transition() {
        let state = WindowState::new(init(1));
        let received: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let received_w = Rc::downgrade(&received);
        let _handle = state.alt_down().observe(move |v| {
            if let Some(r) = received_w.upgrade() {
                r.borrow_mut().push(*v);
            }
        });
        state.set_alt_from_os(true);
        state.set_alt_from_os(true); // idempotent — no second notify
        state.set_alt_from_os(false);
        assert_eq!(*received.borrow(), vec![true, false]);
    }
}
