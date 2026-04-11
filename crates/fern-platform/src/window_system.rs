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

#[cfg(test)]
mod tests {
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
