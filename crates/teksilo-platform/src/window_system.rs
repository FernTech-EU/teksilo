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

/// Attach `child` as a child window of `parent` using
/// `NSWindowOrderingMode::Above` on macOS. Call this *after* both
/// windows' AccessKit adapters have been created (and thus after the
/// child becomes visible). Works for modal and non-modal parented
/// windows alike — popover-as-window, inspector palettes, floating
/// tool panels are all expected to route through this path as the
/// multi-window system matures.
///
/// On non-macOS targets the parent relationship is already set by
/// winit's `WindowAttributes::with_parent_window` at creation time,
/// so this is a no-op. macOS needs the deferred call because AppKit's
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
