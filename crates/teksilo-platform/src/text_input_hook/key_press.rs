// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Whether the input method swallowed a key press: the half that needs no
//! platform.
//!
//! A window hands each key press to the platform's input method, and the
//! input method answers by acting on the window: it inserts text, marks a
//! composition, ends one, or names a command (an arrow, Escape). Sometimes it
//! does none of these: it took the key for itself. On macOS the
//! press-and-hold accent popup takes the auto-repeats of the held letter and
//! the key that picks an accent, then inserts the accent on its own. winit
//! cannot tell: it reports such a key with its text all the same, so the
//! text would be typed beside the accent ("eeê" for "ê").
//!
//! [`KeyPress`] records, for one key press, whether the press went to the
//! input method and whether the input method acted while it had it.
//! [`swallowed`](KeyPress::swallowed) is the verdict: the press went to the
//! input method, the input method is done with it, and it did nothing.

#![cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS view tells, for now")
)]

/// What the input method did with one key press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct KeyPress {
    /// The press was handed to the input method (on macOS,
    /// `interpretKeyEvents:` ran for it).
    interpreted: bool,
    /// How many hand-overs are running: the input method still has the key.
    interpreting: u32,
    /// The input method acted on the window while it had the key.
    acted: bool,
}

impl KeyPress {
    /// The press is handed to the input method.
    pub(crate) fn begin_interpreting(&mut self) {
        self.interpreted = true;
        self.interpreting = self.interpreting.saturating_add(1);
    }

    /// The input method has handed the press back.
    pub(crate) fn end_interpreting(&mut self) {
        self.interpreting = self.interpreting.saturating_sub(1);
    }

    /// The input method acted on the window: inserted, marked or unmarked
    /// text, or named a command. Counts only while it has the key: the same
    /// calls made before it has the key or after it handed it back are not
    /// its answer to this press.
    pub(crate) fn note_acted(&mut self) {
        if self.interpreting > 0 {
            self.acted = true;
        }
    }

    /// The input method had the key, handed it back, and did nothing with
    /// it: whatever text the platform reports for the press must not be
    /// typed. `false` for a press the input method never saw (a window with
    /// no input method enabled), and while it still has the key.
    pub(crate) fn swallowed(&self) -> bool {
        self.interpreted && self.interpreting == 0 && !self.acted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_the_input_method_never_saw_is_not_swallowed() {
        let press = KeyPress::default();
        assert!(!press.swallowed(), "no input method: typing as before");
    }

    #[test]
    fn a_typed_letter_is_not_swallowed() {
        let mut press = KeyPress::default();
        press.begin_interpreting();
        press.note_acted(); // insertText:
        press.end_interpreting();
        assert!(!press.swallowed());
    }

    #[test]
    fn a_command_or_a_composition_is_not_swallowed() {
        // An arrow or Escape (doCommandBySelector:), a dead key
        // (setMarkedText:), the end of a composition (unmarkText).
        let mut press = KeyPress::default();
        press.begin_interpreting();
        press.note_acted();
        press.note_acted();
        press.end_interpreting();
        assert!(!press.swallowed());
    }

    #[test]
    fn a_key_the_input_method_kept_is_swallowed() {
        let mut press = KeyPress::default();
        press.begin_interpreting();
        press.end_interpreting();
        assert!(press.swallowed(), "the accent popup took the repeat");
    }

    #[test]
    fn nothing_is_decided_while_the_input_method_has_the_key() {
        let mut press = KeyPress::default();
        press.begin_interpreting();
        assert!(!press.swallowed());
        press.begin_interpreting();
        press.end_interpreting();
        assert!(!press.swallowed(), "still inside the outer hand-over");
        press.end_interpreting();
        assert!(press.swallowed());
    }

    #[test]
    fn an_action_outside_the_hand_over_is_not_an_answer() {
        let mut press = KeyPress::default();
        press.note_acted();
        press.begin_interpreting();
        press.end_interpreting();
        press.note_acted();
        assert!(
            press.swallowed(),
            "acting before or after the input method had the key does not count"
        );
    }

    #[test]
    fn an_unbalanced_end_does_not_underflow() {
        let mut press = KeyPress::default();
        press.end_interpreting();
        assert!(!press.swallowed());
    }
}
