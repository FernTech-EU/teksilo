//! Window decoration mode.

/// How a window's chrome is drawn.
///
/// Three-valued and explicit. Replaces the older `custom_chrome: bool`
/// flag on `WindowConfig`, which could not represent the
/// "borderless / no host" case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationsMode {
    /// OS-provided title bar, borders, and resize handles. The default
    /// for application windows on every platform.
    Native,
    /// No native title bar; a
    /// [`PlatformTitleBarHost`](crate::PlatformTitleBarHost) is
    /// constructed and attached to the tree so the app can paint its
    /// own chrome. Falls back to `Native` on window systems that do
    /// not support custom chrome (currently X11).
    CustomChrome,
    /// No decorations at all — neither OS chrome nor a host. Use for
    /// splash screens, borderless popups, or fully chrome-less embeds.
    None,
}

impl Default for DecorationsMode {
    fn default() -> Self {
        DecorationsMode::Native
    }
}

impl DecorationsMode {
    /// Returns `true` when this mode wants a
    /// [`PlatformTitleBarHost`](crate::PlatformTitleBarHost) to be
    /// constructed during window creation.
    pub fn wants_custom_chrome_host(self) -> bool {
        matches!(self, DecorationsMode::CustomChrome)
    }

    /// Returns `true` when the OS should draw its own chrome.
    pub fn wants_native_decorations(self) -> bool {
        matches!(self, DecorationsMode::Native)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_native() {
        assert_eq!(DecorationsMode::default(), DecorationsMode::Native);
    }

    #[test]
    fn predicates() {
        assert!(DecorationsMode::Native.wants_native_decorations());
        assert!(!DecorationsMode::Native.wants_custom_chrome_host());
        assert!(DecorationsMode::CustomChrome.wants_custom_chrome_host());
        assert!(!DecorationsMode::CustomChrome.wants_native_decorations());
        assert!(!DecorationsMode::None.wants_custom_chrome_host());
        assert!(!DecorationsMode::None.wants_native_decorations());
    }
}
