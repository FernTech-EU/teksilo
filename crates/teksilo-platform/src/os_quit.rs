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
//!   lets the session end, and on `WM_QUERYENDSESSION` (or a
//!   `WM_ENDSESSION` that comes alone) posts a [`SessionEndNow`] and runs
//!   the event loop's messages until the application has quit, because
//!   the process may be ended soon after that message is answered.
//!   The same watch keeps winit's hidden window from `taskkill`'s
//!   `WM_CLOSE`, which destroyed it, and posts a [`HiddenWindowClose`]
//!   instead.
//!   A `ConsoleWatch` hears the console the application runs in closing
//!   (an application built as a console program, run from a console, as
//!   `cargo run` does): Windows' default handler ended the process there,
//!   past every guard, and it posts a [`ConsoleClosing`] instead.
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
pub use windows::{ConsoleWatch, SessionEndWatch};

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

/// The session is ending now (Windows' `WM_QUERYENDSESSION` or
/// `WM_ENDSESSION`): quit before
/// anything else, then call [`finished`](Self::finished). The process may
/// be ended at any moment once the window procedure that posted it
/// returns, and it returns once this is finished or after
/// [`SESSION_END_BUDGET`].
#[derive(Debug, Clone)]
pub struct SessionEndNow {
    done: Arc<AtomicBool>,
    expired: Arc<AtomicBool>,
}

impl SessionEndNow {
    /// A request whose [`finished`](Self::finished) sets `done`.
    pub fn new(done: Arc<AtomicBool>) -> Self {
        Self {
            done,
            expired: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The application has quit and its settings are on disk.
    pub fn finished(&self) {
        self.done.store(true, Ordering::Release);
    }

    /// The window procedure that posted it stopped waiting (the budget ran
    /// out, the event loop busy in a native modal of the application's
    /// own) and answered Windows. A request delivered after that is for a
    /// session end already over, which another application may have
    /// cancelled: it asks nothing.
    pub fn expire(&self) {
        self.expired.store(true, Ordering::Release);
    }

    /// Whether [`expire`](Self::expire) was called.
    pub fn is_expired(&self) -> bool {
        self.expired.load(Ordering::Acquire)
    }
}

/// The console the application runs in is closing, or the session ends
/// while it runs in one (Windows' `CTRL_CLOSE_EVENT`, `CTRL_LOGOFF_EVENT`,
/// `CTRL_SHUTDOWN_EVENT`): quit before anything else, then call
/// [`finished`](Self::finished). Windows ends the process once the console
/// handler that posted it returns, which it does once this is finished
/// and the process has had the rest of [`SESSION_END_BUDGET`] to end by
/// itself.
#[derive(Debug, Clone)]
pub struct ConsoleClosing {
    session_end: bool,
    done: Arc<AtomicBool>,
}

impl ConsoleClosing {
    /// A request for the session ending (`session_end`) or the console
    /// closing, whose [`finished`](Self::finished) sets `done`.
    pub fn new(session_end: bool, done: Arc<AtomicBool>) -> Self {
        Self { session_end, done }
    }

    /// Whether the session is ending (a logoff or a shutdown) rather than
    /// the console alone closing.
    pub fn session_end(&self) -> bool {
        self.session_end
    }

    /// The application has quit and its settings are on disk.
    pub fn finished(&self) {
        self.done.store(true, Ordering::Release);
    }
}

/// How long the window procedure handling the session's end runs the event
/// loop's messages waiting for the application to quit. Windows shows
/// that an application is holding up the session after about five
/// seconds.
pub const SESSION_END_BUDGET: std::time::Duration = std::time::Duration::from_secs(4);

/// A termination signal arrived (Unix): `SIGTERM` (a service manager, a
/// `kill`, the session ending) or `SIGHUP` (the terminal or the session
/// that started the application closed; systemd sends it with `SIGTERM`
/// when a login session ends). Windows' `taskkill` arrives as
/// [`HiddenWindowClose`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminationSignal {
    /// The signal's number.
    pub signal: i32,
}

/// `SIGTERM`'s number, the same on every platform that has it.
pub const SIGTERM: i32 = 15;

/// `WM_CLOSE` reached winit's hidden event-loop window (Windows): what
/// `taskkill` without `/F` sends when it picks that window, as it does when
/// the application has no window open. With none open it quits the
/// application as `SIGTERM` does; with windows open it is a guarded quit,
/// as the Dock's Quit is on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiddenWindowClose;

/// The replies AppKit is owed: one for each `NSTerminateLater` the
/// `applicationShouldTerminate:` hook answered. A second `terminate:` while
/// the first waits (an AppleScript `quit`, a logout during a user's quit)
/// is answered `NSTerminateLater` too, and AppKit waits for a reply to
/// each; every request's [`reply`](TerminateRequest::reply) gives one while
/// one is owed, and none past that, which AppKit would take as a new
/// decision.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct OwedReplies(std::sync::atomic::AtomicUsize);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl OwedReplies {
    pub(crate) const fn new() -> Self {
        Self(std::sync::atomic::AtomicUsize::new(0))
    }

    /// An `NSTerminateLater` was answered: a reply is owed for it.
    pub(crate) fn owe(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }

    /// Whether a reply is owed, counting this one as given.
    pub(crate) fn take(&self) -> bool {
        // A compare-exchange loop rather than `fetch_update`, which recent
        // toolchains deprecate under a new name older ones lack.
        let mut owed = self.0.load(Ordering::Acquire);
        while let Some(left) = owed.checked_sub(1) {
            match self
                .0
                .compare_exchange_weak(owed, left, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return true,
                Err(now) => owed = now,
            }
        }
        false
    }
}

#[cfg(test)]
mod owed_replies_tests {
    use super::OwedReplies;

    /// Two `terminate:` before the quit is decided: AppKit waits for a
    /// reply to each, so both requests reply. A third reply, owed to
    /// nothing, is not given.
    #[test]
    fn every_later_gets_its_reply_and_no_more() {
        let owed = OwedReplies::new();
        owed.owe();
        owed.owe();
        assert!(owed.take(), "the first request's reply");
        assert!(owed.take(), "the second request's reply");
        assert!(!owed.take(), "nothing more is owed");
    }
}
