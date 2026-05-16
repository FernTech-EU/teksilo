#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSystem {
    Wayland,
    X11,
    Unknown,
}

#[cfg(target_os = "linux")]
fn detect_from_env(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> WindowSystem {
    let session_type = xdg_session_type
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let has_wayland_display = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let has_x11_display = display.is_some_and(|value| !value.trim().is_empty());

    if session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        || has_wayland_display
    {
        WindowSystem::Wayland
    } else if session_type.is_some_and(|value| value.eq_ignore_ascii_case("x11")) || has_x11_display
    {
        WindowSystem::X11
    } else {
        WindowSystem::Unknown
    }
}

pub fn active_window_system() -> WindowSystem {
    #[cfg(target_os = "linux")]
    {
        detect_from_env(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        )
    }

    #[cfg(not(target_os = "linux"))]
    {
        WindowSystem::Unknown
    }
}

pub fn supports_native_modal_windows() -> bool {
    #[cfg(target_os = "linux")]
    {
        !matches!(active_window_system(), WindowSystem::Wayland)
    }

    #[cfg(not(target_os = "linux"))]
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
    #[cfg(target_os = "linux")]
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn prefers_wayland_when_both_displays_exist() {
        assert_eq!(
            detect_from_env(Some("wayland"), Some("wayland-0"), Some(":0")),
            WindowSystem::Wayland
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_x11_from_session_type() {
        assert_eq!(detect_from_env(Some("x11"), None, None), WindowSystem::X11);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_x11_from_display_when_session_is_missing() {
        assert_eq!(detect_from_env(None, None, Some(":0")), WindowSystem::X11);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn returns_unknown_without_display_hints() {
        assert_eq!(detect_from_env(None, None, None), WindowSystem::Unknown);
    }
}
