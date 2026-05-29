//! Window-level menubar key dispatcher.
//!
//! Allows a single `MenuBar` per window to intercept F10 and
//! `Alt+<letter>` key events BEFORE focus-based event dispatch — the
//! same architecture Win32 uses with `WM_SYSKEYDOWN` and
//! `DefWindowProc`. Without this hook, a `Key::F10` press would only
//! reach widgets that are strict ancestors of whatever widget
//! happens to be focused, which the menubar usually isn't.
//!
//! See [`docs/shortcut-intent-action.md`](../../../../docs/shortcut-intent-action.md)
//! for the broader keystroke pipeline.
//!
//! ### Lifetime model
//!
//! `MenuBar::build` constructs an `Rc<dyn MenubarDispatcher>` and
//! installs it via [`WindowState::install_menubar_dispatcher`](crate::window::WindowState::install_menubar_dispatcher),
//! receiving a `MenubarGuard`. The guard is stored on the MenuBar's
//! per-build state; dropping it (on rebuild or removal) clears the
//! window's slot. At most one dispatcher is registered per window
//! at a time — the most-recently-installed wins; a `debug_assert!`
//! fires in debug builds if a second installation happens while the
//! slot is still held.
//!
//! ### Mnemonic activation
//!
//! Mnemonics derived from `&File` / `&Edit` menubar labels are
//! deliberately **not** registered as `Shortcut`s in
//! `ShortcutRegistry`. They are derived from labels (which change
//! with locale), they are not user-rebindable per Win32 / GNOME
//! convention, and they would clutter `ShortcutSettings`. Routing
//! them through this dedicated window-level slot keeps the
//! Action/Intent/Shortcut pipeline free of derived noise.
//!
//! ### macOS limitation
//!
//! The dispatcher is installed on every platform, but its
//! `Alt+<letter>` branch is **compiled out on macOS** (see the
//! `#[cfg(not(target_os = "macos"))]` gate inside the impl). The
//! reason: macOS rewrites Option+letter for accented character
//! composition (Option+E → ´, Option+F → ƒ, …) *before* winit hands
//! the keystroke to the app. The post-rewrite character can never
//! match the mnemonic table, and silently intercepting the chord
//! would break accented text input system-wide. **F10** and
//! **bare-Alt-tap** continue to work on macOS (neither involves a
//! transformed letter), and **bare-letter activation inside an open
//! menu** is unaffected (the rewriting is Option+letter, not
//! letter-alone). The `MenuLabel` widget hides the visual
//! underline on macOS to match. macOS-native users typically
//! prefer F10 + arrows + Enter, plus the existing `Shortcut`
//! system for Cmd+? accelerators (which the OS does not rewrite).

use std::cell::RefCell;
use std::rc::Rc;

use crate::event::{Key, Modifiers};
use crate::widget_id::WidgetId;

/// A `KeyDown` event passed through the menubar dispatcher.
#[derive(Debug, Clone)]
pub struct MenubarKeyEvent {
    pub key: Key,
    pub modifiers: Modifiers,
}

/// The result of `MenubarDispatcher::try_handle`. The caller (the
/// app-level event loop in `bastyde-app`) translates this into the
/// concrete WidgetTree calls.
#[derive(Debug, Clone)]
pub enum MenubarAction {
    /// Focus the trigger and synthesise a click on it so its menu
    /// opens. Used for `Alt+<letter>` mnemonic activation.
    OpenMenu { trigger_id: WidgetId },
    /// Focus the trigger without opening any menu. Used for `F10`
    /// (which puts the menubar in "navigation" mode without revealing
    /// a dropdown).
    FocusTrigger { trigger_id: WidgetId },
    /// Swallow the event silently. Used for `Alt+<letter>` chords
    /// with no matching menubar mnemonic — without this, the chord
    /// would reach a focused text input as an unwanted character.
    Intercept,
}

/// Object-safe trait implemented by `MenuBar`. `bastyde-app` consults
/// the installed dispatcher (if any) BEFORE normal focus-based key
/// dispatch on every `KeyboardInput`, and separately on every
/// `ModifiersChanged` that detects the bare-Alt-tap pattern.
pub trait MenubarDispatcher: 'static {
    /// Called on every `KeyDown` reaching the window. Returns `None`
    /// to fall through to normal dispatch.
    fn try_handle(&self, event: &MenubarKeyEvent) -> Option<MenubarAction>;

    /// Called when the OS reports an Alt-release whose Alt-hold
    /// window saw no non-Alt KeyDowns. Standard Win32 / GTK semantic:
    /// focus the first menubar trigger without opening any menu. The
    /// default implementation returns `None` so dispatchers can
    /// opt into the behaviour explicitly.
    fn on_alt_tap(&self) -> Option<MenubarAction> {
        None
    }
}

/// Internal storage type for the per-window dispatcher slot.
pub(crate) type MenubarDispatcherSlot = RefCell<Option<Rc<dyn MenubarDispatcher>>>;

/// RAII guard returned from [`WindowState::install_menubar_dispatcher`](crate::window::WindowState::install_menubar_dispatcher).
/// Dropping the guard clears the slot iff it still points at the same
/// dispatcher (`Rc::ptr_eq`); a later install + drop-of-stale-guard
/// is a no-op (so racing rebuilds don't accidentally clear each
/// other's slot).
pub struct MenubarGuard {
    pub(crate) slot: Rc<MenubarDispatcherSlot>,
    pub(crate) own: Rc<dyn MenubarDispatcher>,
}

impl Drop for MenubarGuard {
    fn drop(&mut self) {
        let mut slot = self.slot.borrow_mut();
        if let Some(current) = slot.as_ref()
            && Rc::ptr_eq(current, &self.own)
        {
            *slot = None;
        }
    }
}
