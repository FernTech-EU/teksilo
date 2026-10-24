// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Keeping a window out of the taskbar, the per-OS backend behind
//! `WindowConfig::skip_taskbar`.
//!
//! | Platform | How |
//! |---|---|
//! | Windows | winit's `with_skip_taskbar`, at creation ([`skip_taskbar_attributes`]) |
//! | X11 | `_NET_WM_STATE_SKIP_TASKBAR` written before the first map ([`skip_taskbar_before_show`]) |
//! | Wayland | nothing: no protocol open to every client asks the compositor for it |
//! | macOS | nothing: the Dock lists applications, not windows |
//!
//! Both halves are called for every window. Each does its platform's part and
//! nothing elsewhere, so a caller needs no `cfg`.

use winit::window::{Window, WindowAttributes};

/// The attributes for a window created with `skip` set: on Windows, winit's
/// `with_skip_taskbar`; elsewhere `attrs` unchanged.
pub fn skip_taskbar_attributes(attrs: WindowAttributes, skip: bool) -> WindowAttributes {
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs.with_skip_taskbar(skip)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = skip;
        attrs
    }
}

/// Mark `window` for the taskbar to skip, on X11, before it is shown for the
/// first time; nothing on other window systems, or when `skip` is false.
///
/// It has to come between creation and the first map, which in Teksilo is
/// the window's creation with `with_visible(false)` and the show that
/// follows once its accessibility adapter exists. See
/// [`mark_skip_taskbar`](crate::x11::ewmh::mark_skip_taskbar) for why. A
/// failure is reported on stderr and leaves the window in the taskbar.
pub fn skip_taskbar_before_show(window: &Window, skip: bool) {
    #[cfg(all(unix, not(target_os = "macos")))]
    if skip {
        let Some(xid) = crate::window_system::x11_window_id(window) else {
            return;
        };
        if let Err(err) = crate::x11::ewmh::mark_skip_taskbar(xid) {
            eprintln!(
                "teksilo-platform: could not keep the window out of the taskbar ({err}); \
                 it will be listed there"
            );
        }
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = (window, skip);
}
