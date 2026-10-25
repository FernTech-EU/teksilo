// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a quit comes from: [`QuitSource`].

/// Where a quit request came from, read by a close guard or the
/// application's [`on_quit_requested`] guard through
/// [`EventContext::quit_source`](crate::widget::EventContext::quit_source).
///
/// A guard that asks the user before closing can skip its dialog when the
/// user is not there to answer: for [`SessionEnd`](Self::SessionEnd) and
/// [`Signal`](Self::Signal) it saves directly and lets the close go ahead,
/// as AppKit and Qt applications do when the session is ending.
///
/// [`on_quit_requested`]: https://docs.rs/teksilo/latest/teksilo/struct.TeksiloAppBuilder.html#method.on_quit_requested
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuitSource {
    /// The application asked: `ctx.quit()`, `ctx.quit_forced()`, or the App
    /// menu's Quit row.
    Application,
    /// The user quit from outside the application's own controls: macOS's
    /// Dock menu Quit, AppleScript's `quit`, or another `terminate:`. A
    /// guarded quit: a guard can ask, and refuse.
    UserQuit,
    /// The session ends (log out, restart, shut down). On macOS a guarded
    /// quit (AppKit waits for the answer, and a refusal cancels the
    /// logout); on Windows every guard is asked once and cannot refuse,
    /// since the session ends whatever the application answers.
    SessionEnd,
    /// A termination signal on Unix (`SIGTERM`, `SIGHUP`), heard while the
    /// event loop runs. Nobody is there to answer: every guard is asked
    /// once and cannot refuse.
    Signal,
}
