// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The platform's text input system, past what winit forwards.
//!
//! winit hands an application two kinds of text: what a key press produced,
//! and the end of an IME composition the user typed. The text input system
//! sends more than that, and winit 0.30 drops the rest:
//!
//! - **macOS.** Emoji & Symbols, Dictation, AutoFill and Writing Tools all
//!   write through `NSTextInputClient` on winit's view, outside any key
//!   press. winit commits an `insertText:` only while it holds marked text,
//!   so the string is discarded. The same view answers every question about
//!   the document (`selectedRange`, `attributedSubstringForProposedRange:`)
//!   with "nothing", so nothing that reads or corrects text can work.
//! - **Windows.** Text injected as Unicode keystrokes (`VK_PACKET`) reaches
//!   winit as a key it cannot name, and a character outside the BMP arrives
//!   as two lone surrogates.
//!
//! This module attaches to each window's native surface once it exists and
//! detaches when it closes. With `TEKSILO_TEXT_INPUT_TRACE=1` set it logs
//! every call it sees to stderr, which is how the per-OS behaviour it relies
//! on was measured.

use std::sync::{Arc, OnceLock};

use teksilo_core::AppEventPoster;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Whether `TEKSILO_TEXT_INPUT_TRACE` is set to anything but `0`. Read once.
pub fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("TEKSILO_TEXT_INPUT_TRACE").is_some_and(|value| value != "0")
    })
}

/// Write one trace line to stderr, if tracing is on.
#[allow(dead_code)] // Only the macOS and Windows backends trace.
pub(crate) fn trace(args: std::fmt::Arguments<'_>) {
    if trace_enabled() {
        eprintln!("[text-input] {args}");
    }
}

/// Attach to a window's native surface. Main thread only, once the window
/// exists. A no-op where the platform needs nothing.
pub fn attach(window_id: TeksiloWindowId, parent: &ParentHandle, poster: Arc<dyn AppEventPoster>) {
    #[cfg(target_os = "macos")]
    macos::attach(window_id, parent, poster);
    #[cfg(target_os = "windows")]
    windows::attach(window_id, parent, poster);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window_id, parent, poster);
}

/// Forget a window. Safe to call for a window that was never attached.
pub fn detach(window_id: TeksiloWindowId) {
    #[cfg(target_os = "macos")]
    macos::detach(window_id);
    #[cfg(target_os = "windows")]
    windows::detach(window_id);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = window_id;
}
