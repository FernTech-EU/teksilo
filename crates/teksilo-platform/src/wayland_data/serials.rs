// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The serial `wl_data_device::set_selection` is given, and whether one of
//! our surfaces holds the keyboard.
//!
//! The core clipboard protocol ties a selection to user input twice:
//!
//! * the request carries the **serial of an input event**, and a compositor
//!   may refuse a serial older than the selection it already holds (Weston and
//!   Mutter both compare `selection_serial - serial < UINT32_MAX / 2` and
//!   ignore the request when it is older);
//! * Mutter refuses (and cancels the source of) a `set_selection` from any
//!   client but the one with **keyboard focus**, and every compositor sends a
//!   client the current selection only while it has that focus.
//!
//! Copy and paste are user actions in a focused window, so both conditions
//! hold whenever they matter. This records what is needed to meet them: the
//! newest serial of any input event the seat delivered to us (key, keyboard
//! enter, button, touch down), and the keyboard's enter / leave.
//!
//! The events arrive on objects of our own (`wl_keyboard`, `wl_pointer`,
//! `wl_touch` bound on the shared connection), which compositors broadcast to
//! every resource the focused client holds, with the same serial winit's own
//! objects receive. And because libwayland queues an event on every queue
//! when *any* thread reads the socket, the key press that makes winit run a
//! Copy command is already in our queue when the command runs: the hub
//! dispatches it before choosing a serial, so the serial is never the one
//! before the press.

/// Newest input serial and keyboard focus. Pure bookkeeping, so it is tested
/// without a compositor.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputSerials {
    /// The newest serial of an input event, or `None` before the first.
    latest: Option<u32>,
    /// Whether the seat has a keyboard we bound. Without one there is no
    /// focus to know about, and the focus check is skipped.
    keyboard_bound: bool,
    /// Whether one of our surfaces has keyboard focus.
    keyboard_focus: bool,
}

/// Why the selection cannot be set now.
#[cfg_attr(
    not(feature = "clipboard"),
    allow(dead_code, reason = "setting the selection is the clipboard's")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SerialError {
    #[error("no window of this application has keyboard focus")]
    NotFocused,
    #[error("this application has received no input yet")]
    NoInput,
}

impl InputSerials {
    /// An input event with `serial` arrived. Serials wrap at `u32::MAX`, so
    /// "newer" is decided the way compositors decide it: a serial less than
    /// half the range ahead of the current one is newer.
    pub(crate) fn record(&mut self, serial: u32) {
        self.latest = Some(match self.latest {
            Some(current) if !is_newer(serial, current) => current,
            _ => serial,
        });
    }

    /// The seat gained (or lost) its keyboard, which we bound (or released).
    pub(crate) fn set_keyboard_bound(&mut self, bound: bool) {
        self.keyboard_bound = bound;
        if !bound {
            self.keyboard_focus = false;
        }
    }

    /// One of our surfaces received keyboard focus with `serial`.
    pub(crate) fn keyboard_enter(&mut self, serial: u32) {
        self.keyboard_focus = true;
        self.record(serial);
    }

    /// Our surface lost keyboard focus. The leave serial names no input the
    /// user gave us, so it is not recorded.
    pub(crate) fn keyboard_leave(&mut self) {
        self.keyboard_focus = false;
    }

    /// Whether one of our surfaces holds keyboard focus.
    #[cfg(all(test, feature = "clipboard"))]
    pub(crate) fn has_keyboard_focus(&self) -> bool {
        self.keyboard_focus
    }

    /// The serial to set the selection with, or why there is none.
    #[cfg_attr(
        not(feature = "clipboard"),
        allow(dead_code, reason = "setting the selection is the clipboard's")
    )]
    pub(crate) fn for_selection(&self) -> Result<u32, SerialError> {
        if self.keyboard_bound && !self.keyboard_focus {
            return Err(SerialError::NotFocused);
        }
        self.latest.ok_or(SerialError::NoInput)
    }
}

/// Whether `candidate` is newer than `current` on the wrapping serial line.
fn is_newer(candidate: u32, current: u32) -> bool {
    let ahead = candidate.wrapping_sub(current);
    ahead != 0 && ahead < u32::MAX / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_serial_before_any_input() {
        let serials = InputSerials::default();
        assert_eq!(serials.for_selection(), Err(SerialError::NoInput));
    }

    #[test]
    fn the_newest_serial_wins() {
        let mut serials = InputSerials::default();
        serials.record(10);
        serials.record(42);
        serials.record(17); // delivered late, older than 42
        assert_eq!(serials.for_selection(), Ok(42));
    }

    #[test]
    fn serials_compare_across_the_wrap() {
        let mut serials = InputSerials::default();
        serials.record(u32::MAX - 1);
        serials.record(3); // wrapped: newer
        assert_eq!(serials.for_selection(), Ok(3));
        serials.record(u32::MAX - 5); // before the wrap: older
        assert_eq!(serials.for_selection(), Ok(3));
    }

    #[test]
    fn a_bound_keyboard_without_focus_refuses() {
        let mut serials = InputSerials::default();
        serials.set_keyboard_bound(true);
        serials.record(5);
        assert_eq!(serials.for_selection(), Err(SerialError::NotFocused));
        serials.keyboard_enter(6);
        assert_eq!(serials.for_selection(), Ok(6));
        serials.keyboard_leave();
        assert_eq!(serials.for_selection(), Err(SerialError::NotFocused));
    }

    #[test]
    fn a_key_after_the_enter_is_the_serial_used() {
        let mut serials = InputSerials::default();
        serials.set_keyboard_bound(true);
        serials.keyboard_enter(100);
        serials.record(101); // Ctrl
        serials.record(102); // C
        assert_eq!(serials.for_selection(), Ok(102));
    }

    #[test]
    fn a_seat_without_keyboard_uses_pointer_serials_alone() {
        let mut serials = InputSerials::default();
        serials.record(9); // a button press
        assert_eq!(serials.for_selection(), Ok(9));
    }

    #[test]
    fn losing_the_keyboard_forgets_its_focus() {
        let mut serials = InputSerials::default();
        serials.set_keyboard_bound(true);
        serials.keyboard_enter(1);
        serials.set_keyboard_bound(false);
        // No keyboard any more: the pointer's serials decide.
        assert_eq!(serials.for_selection(), Ok(1));
    }
}
