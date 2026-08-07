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
///
/// `Serialize`/`Deserialize` so it can appear inside a persisted type
/// (e.g. `teksilo_widgets::toast::ToastRoute::Window` mirrored into an
/// archived `NotificationEntry`) — but note the id is NOT stable across
/// process restarts, only "unique within a process": a persisted
/// window-scoped route from a previous run will not match any window
/// in the current one, which is the intended, accepted behaviour for
/// something that was inherently about a transient session's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TeksiloWindowId(u64);

impl TeksiloWindowId {
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

impl fmt::Display for TeksiloWindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Window({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_is_by_value() {
        let a = TeksiloWindowId::new(1);
        let b = TeksiloWindowId::new(1);
        let c = TeksiloWindowId::new(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn display_format() {
        assert_eq!(format!("{}", TeksiloWindowId::new(7)), "Window(7)");
    }
}
