// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The operating system asking the application to quit, heard where winit
//! does not report it, and handed to the event loop as a payload.
//!
//! - **macOS.** AppKit asks its application delegate
//!   `applicationShouldTerminate:` before it ends the process, for the Dock
//!   menu's Quit, `terminate:` sent by a menu item or another application
//!   (AppleScript's `quit`), and a logout, restart or shutdown. winit's
//!   delegate does not implement it, so AppKit used to end the process
//!   past every close guard. `install_terminate_hook` adds the method to
//!   winit's delegate class: it answers `NSTerminateLater` and posts a
//!   [`TerminateRequest`], whose [`reply`](TerminateRequest::reply) gives
//!   AppKit the answer once the quit is decided.
//! - **Windows.** winit's windows answer `WM_QUERYENDSESSION` and
//!   `WM_ENDSESSION` with the default procedure, so the session ended with
//!   nothing saved. A `SessionEndWatch` is a hidden top-level window of
//!   its own (a message-only window does not receive the broadcast): it
//!   lets the session end, and on `WM_ENDSESSION` posts a
//!   [`SessionEndNow`] and runs the event loop's messages until the
//!   application has quit, because the process may be ended as soon as
//!   that message is answered.
//! - **Unix.** A `SignalWatch` turns `SIGTERM` and `SIGHUP` into a
//!   [`TerminationSignal`], from a thread of its own; one the process
//!   inherited as ignored (`nohup`) stays ignored.
//!
//! Each watch exists on its platform only; the payloads are plain data on
//! every platform, so the routing that answers them is tested on any host.
//!
//! Hidden from the documentation: the names are not settled, and only
//! `teksilo-app` uses them.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod signals;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
pub use macos::install_terminate_hook;
#[cfg(unix)]
pub use signals::SignalWatch;
#[cfg(target_os = "windows")]
pub use windows::SessionEndWatch;

/// AppKit asked whether the application may end: a Dock Quit,
/// `terminate:`, an AppleScript `quit`, or the session ending. Answered
/// once with [`reply`](Self::reply), on any thread; a request dropped
/// unanswered answers that the application goes on.
pub struct TerminateRequest {
    session_end: bool,
    reply: Option<Box<dyn FnOnce(bool) + Send>>,
}

impl TerminateRequest {
    /// A request, `session_end` when the user is logging out, restarting
    /// or shutting down; `reply` hears whether the application ends.
    pub fn new(session_end: bool, reply: impl FnOnce(bool) + Send + 'static) -> Self {
        Self {
            session_end,
            reply: Some(Box::new(reply)),
        }
    }

    /// Whether the session is ending (a logout, restart or shutdown)
    /// rather than the application alone quitting.
    pub fn session_end(&self) -> bool {
        self.session_end
    }

    /// Tell AppKit whether the application ends: `true` once every window
    /// has closed, `false` when the quit was refused. Only the first
    /// answer counts.
    pub fn reply(mut self, terminate: bool) {
        if let Some(reply) = self.reply.take() {
            reply(terminate);
        }
    }
}

impl Drop for TerminateRequest {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            reply(false);
        }
    }
}

impl std::fmt::Debug for TerminateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminateRequest")
            .field("session_end", &self.session_end)
            .field("answered", &self.reply.is_none())
            .finish()
    }
}

/// The session is ending now (Windows' `WM_ENDSESSION`): quit before
/// anything else, then call [`finished`](Self::finished). The process may
/// be ended at any moment once the window procedure that posted it
/// returns, and it returns once this is finished or after
/// [`SESSION_END_BUDGET`].
#[derive(Debug, Clone)]
pub struct SessionEndNow {
    done: Arc<AtomicBool>,
}

impl SessionEndNow {
    /// A request whose [`finished`](Self::finished) sets `done`.
    pub fn new(done: Arc<AtomicBool>) -> Self {
        Self { done }
    }

    /// The application has quit and its settings are on disk.
    pub fn finished(&self) {
        self.done.store(true, Ordering::Release);
    }
}

/// How long the window procedure handling `WM_ENDSESSION` runs the event
/// loop's messages waiting for the application to quit. Windows shows
/// that an application is holding up the session after about five
/// seconds.
pub const SESSION_END_BUDGET: std::time::Duration = std::time::Duration::from_secs(4);

/// A termination signal arrived (Unix): `SIGTERM` (a service manager, a
/// `kill`, the session ending) or `SIGHUP` (the terminal or the session
/// that started the application closed; systemd sends it with `SIGTERM`
/// when a login session ends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminationSignal {
    /// The signal's number.
    pub signal: i32,
}
