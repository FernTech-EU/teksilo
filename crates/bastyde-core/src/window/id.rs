// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Opaque per-window identifier.

use std::fmt;

/// Opaque identifier for an application window.
///
/// Allocated by the app-level window manager when a window is created
/// and passed back to the app via [`WindowState::id`](super::state::WindowState::id)
/// or the return value of window-opening APIs. IDs are `Copy`, unique
/// within a process, and never reused after a window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BastydeWindowId(u64);

impl BastydeWindowId {
    /// Construct an id from a raw `u64`. Intended for the window
    /// manager's allocator — application code should not fabricate ids.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw numeric id (for serialization or debugging).
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for BastydeWindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Window({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_is_by_value() {
        let a = BastydeWindowId::new(1);
        let b = BastydeWindowId::new(1);
        let c = BastydeWindowId::new(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn display_format() {
        assert_eq!(format!("{}", BastydeWindowId::new(7)), "Window(7)");
    }
}
