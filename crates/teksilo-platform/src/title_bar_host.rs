// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-platform implementations of [`teksilo_core::PlatformTitleBarHost`].
//!
//! Each backend lives in its own submodule under `title_bar_host/`. The
//! [`create_title_bar_host`] factory picks the right one based on the current
//! platform and — on unix — the window's live display handle. When no backend
//! can serve the window it returns [`PlatformError::Unsupported`] and the
//! application falls back to native server-side decorations.
//!
//! X11 is supported, but conditionally: custom chrome there depends on the
//! window manager implementing `_NET_WM_MOVERESIZE`, since a borderless window
//! has no other way to be moved or resized. See `title_bar_host/x11.rs`.

use std::rc::Rc;
use std::sync::Arc;

use teksilo_core::{PlatformError, PlatformTitleBarHost, ResizeEdge, TitleBarHostCallbacks};
use winit::window::{ResizeDirection, Window};

/// Map a teksilo-core [`ResizeEdge`] to winit's [`ResizeDirection`].
/// Used by the Wayland and Windows backends — both delegate
/// interactive resize to winit, which translates internally to the
/// platform's native protocol (xdg-shell `resize`, `WM_NCLBUTTONDOWN`
/// with `HTLEFT`/etc.).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn edge_to_direction(edge: ResizeEdge) -> ResizeDirection {
    match edge {
        ResizeEdge::Top => ResizeDirection::North,
        ResizeEdge::TopRight => ResizeDirection::NorthEast,
        ResizeEdge::Right => ResizeDirection::East,
        ResizeEdge::BottomRight => ResizeDirection::SouthEast,
        ResizeEdge::Bottom => ResizeDirection::South,
        ResizeEdge::BottomLeft => ResizeDirection::SouthWest,
        ResizeEdge::Left => ResizeDirection::West,
        ResizeEdge::TopLeft => ResizeDirection::NorthWest,
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

#[cfg(target_os = "macos")]
pub use macos::MacOsHost;
#[cfg(all(unix, not(target_os = "macos")))]
pub use wayland::WaylandHost;
#[cfg(target_os = "windows")]
pub use windows::WindowsHost;
#[cfg(all(unix, not(target_os = "macos")))]
pub use x11::X11Host;

/// Construct a title bar host for the given winit window. Returns
/// [`PlatformError::Unsupported`] when the window system cannot support custom
/// chrome — on X11 that means no EWMH window manager, or one without
/// `_NET_WM_MOVERESIZE`.
///
/// On unix the backend is chosen from the window's **live**
/// `RawDisplayHandle`, not from the environment. `WAYLAND_DISPLAY` and
/// `DISPLAY` are both set in essentially every modern session, so only the
/// handle can say which backend winit actually created — and using one source
/// of truth here keeps the title bar and the DnD backend from ever disagreeing
/// about the same window.
///
/// The host borrows an `Arc` clone of the window so it can keep calling
/// winit (`drag_window`, `set_minimized`, ...) for the lifetime of the
/// title bar widget. `callbacks` carries closures that route operations
/// which must hop through the event loop (currently just `close`) back
/// to `WindowManager` — see [`TitleBarHostCallbacks`].
pub fn create_title_bar_host(
    window: Arc<Window>,
    callbacks: TitleBarHostCallbacks,
) -> Result<Rc<dyn PlatformTitleBarHost>, PlatformError> {
    #[cfg(target_os = "windows")]
    {
        WindowsHost::new(window, callbacks).map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>)
    }

    #[cfg(target_os = "macos")]
    {
        MacOsHost::new(window, callbacks).map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::raw_window_handle::HasDisplayHandle;

        use crate::window_system::{WindowSystem, window_system_for_display_handle};

        let display = window
            .display_handle()
            .map_err(|e| PlatformError::Os(e.to_string()))?;

        match window_system_for_display_handle(&display.as_raw()) {
            WindowSystem::Wayland => WaylandHost::new(window, callbacks)
                .map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>),
            WindowSystem::X11 => {
                // `X11Host::new` refuses when the window manager can't service
                // `_NET_WM_MOVERESIZE`; the caller then keeps native
                // decorations. The same probe already gated
                // `with_decorations(false)` at window-creation time, so the two
                // decisions agree.
                X11Host::new(window, callbacks).map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>)
            }
            WindowSystem::Unknown => {
                eprintln!(
                    "teksilo-platform: window reports neither an X11 nor a Wayland \
                     display handle; custom TitleBar disabled"
                );
                Err(PlatformError::Unsupported)
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        let _ = (window, callbacks);
        Err(PlatformError::Unsupported)
    }
}
