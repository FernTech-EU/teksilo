//! Window placement enum.
//!
//! A single unified type covers the four placement modes every modern
//! desktop OS supports: floating, maximized, fullscreen, minimized.
//! Represent state, not transitions — the platform layer decides how to
//! move between any two variants.
//!
//! Size and position are deliberately *not* inside `Floating`. They are
//! independent signals on [`WindowState`](super::state::WindowState)
//! that always hold the last-known *restored* values. This matches
//! native behavior on macOS (`frameAutosaveName`) and Windows
//! (`WINDOWPLACEMENT`), and it removes the "what size do I go back to
//! after un-maximize?" ambiguity that boolean fullscreen/maximize
//! representations expose.

/// Top-level placement state for a window.
///
/// Transitions between any two variants are legal — the platform layer
/// is responsible for preserving the restored rect (held by the
/// `size` / `position` signals on `WindowState`) when crossing through
/// `Maximized`, `Fullscreen`, or `Minimized`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowPlacement {
    /// Regular overlapping window. Uses `WindowState::size` and
    /// `WindowState::position` as the current geometry.
    Floating,
    /// Maximized to fill the current monitor's work area (minus taskbar
    /// / dock / menu bar on platforms that have one).
    Maximized,
    /// Exclusive fullscreen — covers the entire display, title bar and
    /// all chrome hidden. On macOS this is Space-based fullscreen.
    Fullscreen,
    /// Minimized to the taskbar / dock. The window is not visible
    /// on-screen but retains its state and may be restored.
    Minimized,
}

impl WindowPlacement {
    /// Returns `true` when the window is currently in `Fullscreen`.
    pub fn is_fullscreen(self) -> bool {
        matches!(self, WindowPlacement::Fullscreen)
    }

    /// Returns `true` when the window is currently in `Maximized`.
    pub fn is_maximized(self) -> bool {
        matches!(self, WindowPlacement::Maximized)
    }

    /// Returns `true` when the window is currently in `Minimized`.
    pub fn is_minimized(self) -> bool {
        matches!(self, WindowPlacement::Minimized)
    }

    /// Returns `true` when the window is currently `Floating` — i.e.
    /// neither maximized, fullscreen, nor minimized.
    pub fn is_floating(self) -> bool {
        matches!(self, WindowPlacement::Floating)
    }
}

impl Default for WindowPlacement {
    fn default() -> Self {
        WindowPlacement::Floating
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_match_variant() {
        assert!(WindowPlacement::Floating.is_floating());
        assert!(!WindowPlacement::Floating.is_fullscreen());
        assert!(WindowPlacement::Fullscreen.is_fullscreen());
        assert!(WindowPlacement::Maximized.is_maximized());
        assert!(WindowPlacement::Minimized.is_minimized());
    }

    #[test]
    fn default_is_floating() {
        assert_eq!(WindowPlacement::default(), WindowPlacement::Floating);
    }
}
