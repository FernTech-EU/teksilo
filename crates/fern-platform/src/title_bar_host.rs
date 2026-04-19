//! Per-platform implementations of [`fern_core::PlatformTitleBarHost`].
//!
//! Each backend lives in its own submodule under `title_bar_host/`. The
//! [`create_title_bar_host`] factory picks the right one based on the
//! current platform and the underlying window system. On unsupported
//! configurations (currently X11) it logs a warning and returns
//! [`PlatformError::Unsupported`]; the application is then expected to
//! fall back to native server-side decorations.

use std::rc::Rc;
use std::sync::Arc;

use fern_core::{PlatformError, PlatformTitleBarHost, TitleBarHostCallbacks};
use winit::window::Window;

mod macos;
mod wayland;
mod windows;
mod x11;

pub use macos::MacOsHost;
pub use wayland::WaylandHost;
pub use windows::WindowsHost;
pub use x11::X11Host;

/// Construct a title bar host for the given winit window. Returns
/// [`PlatformError::Unsupported`] when running on a window system that
/// cannot support custom chrome (currently: X11, and any non-Wayland Linux
/// where the window system cannot be detected).
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
        return WindowsHost::new(window, callbacks)
            .map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>);
    }

    #[cfg(target_os = "macos")]
    {
        return MacOsHost::new(window, callbacks)
            .map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use crate::window_system::{WindowSystem, active_window_system};
        match active_window_system() {
            WindowSystem::Wayland => WaylandHost::new(window, callbacks)
                .map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>),
            WindowSystem::X11 => {
                eprintln!(
                    "fern-platform: custom TitleBar is not supported on X11; \
                     falling back to native server-side decorations"
                );
                Err(PlatformError::Unsupported)
            }
            WindowSystem::Unknown => {
                eprintln!(
                    "fern-platform: could not detect window system; \
                     custom TitleBar disabled"
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
