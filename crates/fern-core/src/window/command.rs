//! Private queue element produced by `WindowState` observers.
//!
//! When an app-side signal write fires its observer, the observer pushes
//! one of these into [`WindowStateInner::pending_os_commands`](
//! super::state::WindowState). The app-level window manager drains that
//! queue once per tick and translates each command into a winit call.

use super::placement::WindowPlacement;

/// Intensity level for a user-attention request.
///
/// Mirrors winit's `UserAttentionType` so platforms that distinguish a
/// critical (red bouncing dock icon on macOS) vs informational
/// (one-bounce / taskbar flash) request can honour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAttentionKind {
    /// Persistent until the window is focused — macOS red bouncing
    /// dock, Windows flashing taskbar entry.
    Critical,
    /// One-shot — macOS single bounce, Windows brief taskbar highlight.
    Informational,
}

/// App → OS command produced by a `WindowState` signal observer.
///
/// Queued on [`WindowState`](super::state::WindowState) and drained by
/// the app-level window manager after event dispatch. Application code
/// does not construct these directly; writing to a signal field on
/// `WindowState` is how you emit one.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    SetPlacement(WindowPlacement),
    SetTitle(String),
    SetSize(u32, u32),
    SetPosition(i32, i32),
    SetResizable(bool),
    SetAlwaysOnTop(bool),
    RequestAttention(UserAttentionKind),
    Focus,
    Close,
}
