// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A composition that replaces text already in the field: the half that
//! needs no platform.
//!
//! An input method can compose over text the field already holds rather than
//! at the caret. Korean Hanja conversion is the measured case (macOS 27):
//! the input method commits a Hangul syllable, then marks its Hanja over it,
//! naming that syllable's range as the replacement of its `setMarkedText:`.
//! winit drops the range, and reports only the marked text (`Ime::Preedit`),
//! so the composition showed beside the syllable instead of over it.
//!
//! The view converts the range to the focused field's characters and keeps
//! it in a [`MarkedReplacement`] slot before winit runs. winit reports the
//! preedit synchronously from inside that call (when its handler is idle),
//! and the application takes the slot as it translates that preedit. A range
//! the preedit does not take must not ride on a later one: the next
//! `setMarkedText:` replaces it (with its own range, or none), and the next
//! key press, the end of the composition and the window closing clear it.
//!
//! On Windows the range comes from an IMM32 reconversion instead: the input
//! method asks for the text to reconvert and confirms the part it takes
//! (`IMR_RECONVERTSTRING`, `IMR_CONFIRMRECONVERTSTRING`), then composes over
//! it. `imm_requests::Reconversion` keeps it in the same slot.

#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    allow(
        dead_code,
        reason = "only the macOS view and the Windows hook record it"
    )
)]

use std::ops::Range;

/// The replacement range of the latest marked text, until a preedit takes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MarkedReplacement {
    range: Option<Range<usize>>,
}

impl MarkedReplacement {
    /// The input method marked text, replacing `range` of the field's
    /// characters (`None` for none, or for a range the field could not map):
    /// it takes the place of whatever the previous marked text left.
    pub(crate) fn record(&mut self, range: Option<Range<usize>>) {
        self.range = range;
    }

    /// The range, for the preedit being translated; once.
    pub(crate) fn take(&mut self) -> Option<Range<usize>> {
        self.range.take()
    }

    /// A key press, the end of the composition: nothing is left to take.
    pub(crate) fn clear(&mut self) {
        self.range = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_range_is_taken_once() {
        let mut slot = MarkedReplacement::default();
        slot.record(Some(7727..7728));
        assert_eq!(slot.take(), Some(7727..7728));
        assert_eq!(slot.take(), None, "the same preedit, or a later one");
    }

    #[test]
    fn marked_text_without_a_range_clears_one_left_untaken() {
        // The Hanja `setMarkedText:` with a range, whose preedit winit had
        // to buffer, then the next marked text with none: that one must not
        // replace anything.
        let mut slot = MarkedReplacement::default();
        slot.record(Some(10..14));
        slot.record(None);
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn the_latest_range_wins() {
        let mut slot = MarkedReplacement::default();
        slot.record(Some(1..2));
        slot.record(Some(3..5));
        assert_eq!(slot.take(), Some(3..5));
    }

    #[test]
    fn a_key_press_or_the_end_of_the_composition_clears_it() {
        let mut slot = MarkedReplacement::default();
        slot.record(Some(0..1));
        slot.clear();
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn an_empty_range_is_kept_as_an_insertion_point() {
        // A range of length zero names where the composition goes.
        let mut slot = MarkedReplacement::default();
        slot.record(Some(4..4));
        assert_eq!(slot.take(), Some(4..4));
    }
}
