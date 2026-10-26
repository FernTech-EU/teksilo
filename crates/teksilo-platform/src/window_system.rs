// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Which windowing system a Linux/BSD session is running on, and what that
//! implies for window chrome and modal windows.
//!
//! Two detectors live here and they are **not** interchangeable:
//!
//! - [`active_window_system`] predicts, from the environment, which backend
//!   winit will pick. It is only valid *before* a window exists — its one job
//!   is deciding `WindowAttributes::with_decorations(..)`, which has to be
//!   chosen at window-construction time. It mirrors winit's own precedence
//!   exactly; any divergence means Teksilo and winit
//!   disagree about the session and the chrome comes out wrong.
//! - [`window_system_for_display_handle`] reads the *live* handle of an
//!   already-created window. It is authoritative and should be preferred
//!   everywhere a window (or a `ParentHandle`) is in reach — the title-bar host
//!   factory and the external-DnD backend both dispatch on it, so they can
//!   never disagree with each other or with winit.

/// The windowing system backing a window (or the session, when predicted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSystem {
    Wayland,
    X11,
    Unknown,
}

/// Predict winit's backend choice from the environment.
///
/// This mirrors winit 0.30's `platform_impl::linux::EventLoop::new` precedence
/// verbatim: a non-empty `WAYLAND_DISPLAY` **or** `WAYLAND_SOCKET` selects
/// Wayland; otherwise a non-empty `DISPLAY` selects X11.
///
/// `XDG_SESSION_TYPE` is deliberately **not** consulted — winit ignores it, and
/// honouring it here would make Teksilo disagree with the backend that actually
/// gets created. Concretely: a Wayland session where the user clears
/// `WAYLAND_DISPLAY` to force an X11 client still reports
/// `XDG_SESSION_TYPE=wayland`, so trusting it would claim Wayland for a window
/// winit created on X11.
#[cfg(all(unix, not(target_os = "macos")))]
fn detect_from_env(
    wayland_display: Option<&str>,
    wayland_socket: Option<&str>,
    display: Option<&str>,
) -> WindowSystem {
    let non_empty = |value: Option<&str>| value.is_some_and(|value| !value.is_empty());

    if non_empty(wayland_display) || non_empty(wayland_socket) {
        WindowSystem::Wayland
    } else if non_empty(display) {
        WindowSystem::X11
    } else {
        WindowSystem::Unknown
    }
}

/// The window system winit is expected to use for windows created in this
/// process. See the module docs: this is a *prediction* for the pre-window-
/// creation decisions only. Once a window exists, prefer
/// [`window_system_for_display_handle`].
pub fn active_window_system() -> WindowSystem {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        detect_from_env(
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("WAYLAND_SOCKET").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        )
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        WindowSystem::Unknown
    }
}

/// The window system a live window actually runs on, read from its raw display
/// handle. Authoritative — unlike [`active_window_system`] it cannot disagree
/// with winit, because the handle *is* winit's answer.
///
/// Every consumer that has a window (or a
/// [`ParentHandle`](teksilo_core::raw_handle::ParentHandle)) in hand should
/// dispatch on this so the title-bar host and the DnD backend never diverge.
pub fn window_system_for_display_handle(
    handle: &raw_window_handle::RawDisplayHandle,
) -> WindowSystem {
    use raw_window_handle::RawDisplayHandle;

    match handle {
        RawDisplayHandle::Wayland(_) => WindowSystem::Wayland,
        RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => WindowSystem::X11,
        _ => WindowSystem::Unknown,
    }
}

/// Whether a modal dialog should be presented as a real OS window rather than
/// an in-tree overlay.
///
/// `false` for the whole Linux/BSD family. On Wayland there is no protocol for
/// a client to make another surface input-blocking at all; on X11
/// `_NET_WM_STATE_MODAL` is a *hint* — window managers use it for stacking and
/// focus policy, but none of them enforce input blocking on the parent, so a
/// "native" modal there would look modal without behaving modally. An in-tree
/// modal is genuinely modal on both, so both use it.
pub fn supports_native_modal_windows() -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        false
    }

    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        true
    }
}

/// Show `window`, built hidden, without making it the key or focused window:
/// on macOS `orderFront:`, where winit's `set_visible(true)` is
/// `makeKeyAndOrderFront:`. Elsewhere winit's `set_visible(true)`, which on
/// Windows shows a window built with `with_active(false)` with
/// `SW_SHOWNOACTIVATE` the first time, and on X11 and Wayland leaves the
/// focus to the window manager or compositor.
pub(crate) fn show_without_activating(window: &winit::window::Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_app_kit::NSView;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::AppKit(appkit) = handle.as_raw()
        {
            // SAFETY: the ns_view pointer is valid while `window` is alive,
            // which the caller holds for this call; winit runs on the main
            // thread, as AppKit requires.
            unsafe {
                if let Some(view) = Retained::<NSView>::retain(appkit.ns_view.as_ptr().cast())
                    && let Some(ns_window) = view.window()
                {
                    ns_window.orderFront(None);
                    return;
                }
            }
        }
        window.set_visible(true);
    }

    #[cfg(not(target_os = "macos"))]
    window.set_visible(true);
}

/// Attach `child` as a child window of `parent` using
/// `NSWindowOrderingMode::Above` on macOS. Call this *after* both
/// windows' AccessKit adapters have been created (and thus after the
/// child becomes visible). Works for modal and non-modal parented
/// windows alike — popover-as-window, inspector palettes, floating
/// tool panels are all expected to route through this path as the
/// multi-window system matures.
///
/// On Windows the parent relationship is the owner window winit sets at
/// creation, and on X11 it is [`set_transient_for_before_show`], so this is a
/// no-op there. macOS needs the deferred call because AppKit's
/// `-[NSWindow addChildWindow:ordered:]` orders the child window
/// front (making it visible), which conflicts with AccessKit's
/// requirement that its adapter be created while the window is still
/// hidden.
pub fn attach_child_window(parent: &winit::window::Window, child: &winit::window::Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_app_kit::{NSView, NSWindowOrderingMode};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Ok(parent_handle) = parent.window_handle() else {
            return;
        };
        let Ok(child_handle) = child.window_handle() else {
            return;
        };

        let (RawWindowHandle::AppKit(p), RawWindowHandle::AppKit(c)) =
            (parent_handle.as_raw(), child_handle.as_raw())
        else {
            return;
        };

        // SAFETY: the ns_view pointers are valid while `parent` /
        // `child` are alive; both are held by the caller (typically
        // the WindowManager) for the lifetime of this call.
        unsafe {
            let Some(parent_view): Option<Retained<NSView>> =
                Retained::retain(p.ns_view.as_ptr().cast())
            else {
                return;
            };
            let Some(child_view): Option<Retained<NSView>> =
                Retained::retain(c.ns_view.as_ptr().cast())
            else {
                return;
            };
            if let (Some(pw), Some(cw)) = (parent_view.window(), child_view.window()) {
                pw.addChildWindow_ordered(&cw, NSWindowOrderingMode::Above);
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (parent, child);
    }
}

/// Keep `window` out of the Window menu's window list and the Dock icon's
/// menu on macOS, or list it there again; nothing on other platforms.
///
/// AppKit lists every titled window it orders in, and `teksilo-app` keeps
/// a modal dialog and a window kept out of the taskbar (a palette) out, as
/// AppKit's own sheets and panels are: the list is for the windows a user
/// switches between, and a dialog comes forward with the window it blocks. Call it before the window is first shown, so its
/// row never appears.
#[doc(hidden)]
pub fn set_listed_in_window_menu(window: &winit::window::Window, listed: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2_app_kit::NSView;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        // SAFETY: the ns_view pointer is valid while `window` is alive, and
        // the caller holds it for the duration of this call.
        let view: Option<Retained<NSView>> =
            unsafe { Retained::retain(appkit.ns_view.as_ptr().cast()) };
        if let Some(ns_window) = view.and_then(|view| view.window()) {
            ns_window.setExcludedFromWindowsMenu(!listed);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, listed);
    }
}

/// Make `child` a transient window of `parent` on X11 (ICCCM
/// `WM_TRANSIENT_FOR`), before `child` is shown for the first time; nothing
/// on other window systems.
///
/// winit 0.30 has no call for it: its `with_parent_window` creates an X11
/// window *inside* the parent, confined to and clipped by its client area,
/// and is ignored on Wayland, where no protocol open to every client sets a
/// parent. The window manager keeps a transient window above its parent and
/// treats it as a dialog. The property is written before the first map, as
/// [`skip_taskbar_before_show`](crate::taskbar::skip_taskbar_before_show)
/// writes its own. A failure is reported on stderr and leaves `child` a window
/// of its own.
pub fn set_transient_for_before_show(
    child: &winit::window::Window,
    parent: &winit::window::Window,
) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let (Some(child), Some(parent)) = (x11_window_id(child), x11_window_id(parent)) else {
            return;
        };
        if let Err(err) = crate::x11::ewmh::set_transient_for(child, parent) {
            eprintln!(
                "teksilo-platform: could not make the window transient for its parent ({err}); \
                 the window manager will treat it as a window of its own"
            );
        }
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = (child, parent);
}

/// Make `window`, a modal dialog, modal to the window manager on X11
/// (`_NET_WM_STATE_MODAL`), before it is shown for the first time; nothing on
/// other window systems. With `WM_TRANSIENT_FOR`
/// ([`set_transient_for_before_show`]) and the dialog window type, it is how
/// GTK marks a modal dialog. A failure is reported on stderr and leaves the
/// dialog a plain transient window.
#[doc(hidden)]
pub fn mark_modal_before_show(window: &winit::window::Window) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some(xid) = x11_window_id(window) else {
            return;
        };
        if let Err(err) = crate::x11::ewmh::mark_modal(xid) {
            eprintln!(
                "teksilo-platform: could not mark the dialog modal ({err}); the window manager \
                 will treat it as a plain transient window"
            );
        }
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = window;
}

/// The X11 window behind `window`, `None` on any other window system.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn x11_window_id(window: &winit::window::Window) -> Option<u32> {
    use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

    let display = window.display_handle().ok()?;
    if window_system_for_display_handle(&display.as_raw()) != WindowSystem::X11 {
        return None;
    }
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(handle) => Some(handle.window as u32),
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The env-detection table below is winit 0.30's own precedence
    // (`platform_impl/linux/mod.rs`): Wayland vars win over `DISPLAY`, empty
    // strings count as unset. If a winit upgrade changes that order, these
    // tests are the tripwire — Teksilo predicting a different backend than
    // winit creates means custom chrome is requested for the wrong protocol.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn prefers_wayland_when_both_displays_exist() {
        assert_eq!(
            detect_from_env(Some("wayland-0"), None, Some(":0")),
            WindowSystem::Wayland
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn wayland_socket_also_selects_wayland() {
        assert_eq!(
            detect_from_env(None, Some("4"), Some(":0")),
            WindowSystem::Wayland
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn detects_x11_from_display() {
        assert_eq!(detect_from_env(None, None, Some(":0")), WindowSystem::X11);
    }

    /// The XWayland escape hatch: clearing `WAYLAND_DISPLAY` in a Wayland
    /// session makes winit create a genuine X11 client. Teksilo must follow,
    /// which is exactly why `XDG_SESSION_TYPE` (still "wayland" here) is not
    /// consulted.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn empty_wayland_display_falls_back_to_x11() {
        assert_eq!(
            detect_from_env(Some(""), Some(""), Some(":0")),
            WindowSystem::X11
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn returns_unknown_without_display_hints() {
        assert_eq!(detect_from_env(None, None, None), WindowSystem::Unknown);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn returns_unknown_when_all_hints_are_empty() {
        assert_eq!(
            detect_from_env(Some(""), Some(""), Some("")),
            WindowSystem::Unknown
        );
    }

    #[test]
    fn display_handle_discriminates_the_live_backend() {
        use raw_window_handle::{RawDisplayHandle, XcbDisplayHandle, XlibDisplayHandle};

        assert_eq!(
            window_system_for_display_handle(&RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                None, 0
            ))),
            WindowSystem::X11
        );
        assert_eq!(
            window_system_for_display_handle(&RawDisplayHandle::Xcb(XcbDisplayHandle::new(
                None, 0
            ))),
            WindowSystem::X11
        );
    }

    /// Linux/BSD never gets a native modal window: Wayland has no protocol for
    /// it and X11's `_NET_WM_STATE_MODAL` is advisory. See the fn docs.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_family_never_uses_native_modals() {
        assert!(!supports_native_modal_windows());
    }
}
