// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Which keys a text service took, on Windows with the TSF text store: the
//! key-trace verdict (`engineering/docs/tsf-text-store.md`, 4.3), the half
//! that needs no platform.
//!
//! TSF processes the keys itself (Chromium's model), and a key a text
//! service takes reaches winit as its key-up only: no `WM_KEYDOWN`, no
//! `WM_CHAR` (measured, M3). `ITfKeyTraceEventSink` tells the store of every
//! key TSF saw, down and up, before the window gets the message. So a key
//! whose trace went down and up with no `WM_KEYDOWN` (or `WM_SYSKEYDOWN`)
//! for it in between was taken.
//!
//! The application needs to know for the bare-Alt menu: a key taken while
//! Alt is held (the Japanese IME's Alt+`) is still a key of the chord, and
//! the Alt release must not open the menu bar (round 4's bug). It reads the
//! verdict with [`KeyTrace::take_taken`] at Alt's press, to start afresh,
//! and at its release.

#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "only the Windows store traces keys")
)]

/// The keys traced since the application last asked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct KeyTrace {
    /// The virtual key whose trace went down and whose `WM_KEYDOWN` has not
    /// arrived yet.
    pending: Option<u32>,
    /// A key was taken since the last [`take_taken`](Self::take_taken).
    taken: bool,
}

impl KeyTrace {
    /// TSF saw `vk` go down (`OnKeyTraceDown`).
    pub(crate) fn trace_down(&mut self, vk: u32) {
        self.pending = Some(vk);
    }

    /// The window got `WM_KEYDOWN` or `WM_SYSKEYDOWN` for `vk`: the key was
    /// passed on.
    pub(crate) fn window_key_down(&mut self, vk: u32) {
        if self.pending == Some(vk) {
            self.pending = None;
        }
    }

    /// TSF saw `vk` go up (`OnKeyTraceUp`): a key-down still pending was
    /// taken.
    pub(crate) fn trace_up(&mut self, vk: u32) {
        if self.pending == Some(vk) {
            self.pending = None;
            self.taken = true;
        }
    }

    /// Whether a key was taken since the last call.
    pub(crate) fn take_taken(&mut self) -> bool {
        std::mem::take(&mut self.taken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VK_OEM_3: u32 = 0xC0;
    const VK_DOWN: u32 = 0x28;
    const VK_MENU: u32 = 0x12;

    #[test]
    fn a_key_with_no_window_key_down_between_its_traces_was_taken() {
        let mut trace = KeyTrace::default();
        // Alt goes down and is passed on.
        trace.trace_down(VK_MENU);
        trace.window_key_down(VK_MENU);
        // ` is taken by the IME (no WM_SYSKEYDOWN for it).
        trace.trace_down(VK_OEM_3);
        trace.trace_up(VK_OEM_3);
        assert!(trace.take_taken());
        assert!(!trace.take_taken(), "asked once");
    }

    #[test]
    fn a_key_passed_on_was_not_taken() {
        let mut trace = KeyTrace::default();
        trace.trace_down(VK_DOWN);
        trace.window_key_down(VK_DOWN);
        trace.trace_up(VK_DOWN);
        // Alt's own trace up after its key-down was passed on.
        trace.trace_down(VK_MENU);
        trace.window_key_down(VK_MENU);
        trace.trace_up(VK_MENU);
        assert!(!trace.take_taken());
    }

    #[test]
    fn a_key_down_for_another_key_does_not_pass_this_one_on() {
        let mut trace = KeyTrace::default();
        trace.trace_down(VK_OEM_3);
        trace.window_key_down(VK_DOWN);
        trace.trace_up(VK_OEM_3);
        assert!(trace.take_taken());
    }
}
