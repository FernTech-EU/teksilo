// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The text a Phase 0 TSF text store shows a text service: the half that
//! needs no platform.
//!
//! Phase 0 of the TSF design (`engineering/docs/tsf-text-store.md`, 7.1)
//! measures what text services do with a store before the widgets have a
//! write half. Its store answers from a [`TextMirror`]: a copy of the focused
//! document's text and selection, in the UTF-16 units TSF counts in (its
//! ACP offsets), taken when the field takes the focus and again whenever
//! the document changed and no text service has written since. A text
//! service's writes (`SetText`, `InsertTextAtSelection`, `SetSelection`)
//! land in the mirror only: the field never sees them, and the next sync
//! after the composition ends puts the document's text back.

#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "only the Windows store keeps a mirror")
)]

use std::ops::Range;

/// A change to the mirror's text, as `TS_TEXTCHANGE` reports it: the
/// replaced range was `start..old_end`, the new text spans `start..new_end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextChange {
    pub(crate) start: usize,
    pub(crate) old_end: usize,
    pub(crate) new_end: usize,
}

/// Why the mirror refused an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MirrorError {
    /// A position past the end, or an end before its start
    /// (`TS_E_INVALIDPOS`).
    InvalidPosition,
}

/// The text and selection a Phase 0 store shows, in UTF-16 units.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TextMirror {
    units: Vec<u16>,
    selection: Range<usize>,
    /// Whether the caret is at the selection's start (`TS_AE_START`).
    caret_at_start: bool,
    /// A text service wrote since the last sync.
    written: bool,
}

impl TextMirror {
    /// The length, `GetEndACP`.
    pub(crate) fn len(&self) -> usize {
        self.units.len()
    }

    /// The selection, lowest offset first.
    pub(crate) fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    /// Whether the caret is at the selection's start.
    pub(crate) fn caret_at_start(&self) -> bool {
        self.caret_at_start
    }

    /// Whether a text service wrote since the last sync: the mirror then
    /// holds text the document does not.
    pub(crate) fn written(&self) -> bool {
        self.written
    }

    /// The units, for a trace.
    pub(crate) fn text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    /// Take the document's text and selection (UTF-16 offsets), forgetting
    /// what text services wrote. The change to report (`OnTextChange`), if
    /// the text differs, and whether the selection moved.
    pub(crate) fn sync(
        &mut self,
        text: &str,
        selection: Range<usize>,
        caret_at_start: bool,
    ) -> (Option<TextChange>, bool) {
        let units: Vec<u16> = text.encode_utf16().collect();
        let change = diff(&self.units, &units);
        let end = units.len();
        let selection =
            selection.start.min(end)..selection.end.min(end).max(selection.start.min(end));
        let moved = selection != self.selection || caret_at_start != self.caret_at_start;
        self.units = units;
        self.selection = selection;
        self.caret_at_start = caret_at_start;
        self.written = false;
        (change, moved)
    }

    /// The units of `range`, clamped to the text, `GetText`.
    pub(crate) fn get(&self, range: Range<usize>) -> &[u16] {
        let end = range.end.min(self.units.len());
        let start = range.start.min(end);
        &self.units[start..end]
    }

    /// Replace `range` with `text`, `SetText`. A selection after the range
    /// moves with it; one inside it collapses to the end of the new text.
    pub(crate) fn replace(
        &mut self,
        range: Range<usize>,
        text: &[u16],
    ) -> Result<TextChange, MirrorError> {
        if range.start > range.end || range.end > self.units.len() {
            return Err(MirrorError::InvalidPosition);
        }
        let new_end = range.start + text.len();
        self.units.splice(range.clone(), text.iter().copied());
        let shift = |offset: usize| -> usize {
            if offset >= range.end {
                offset - range.len() + text.len()
            } else if offset > range.start {
                new_end
            } else {
                offset
            }
        };
        self.selection = shift(self.selection.start)..shift(self.selection.end);
        self.written = true;
        Ok(TextChange {
            start: range.start,
            old_end: range.end,
            new_end,
        })
    }

    /// Insert `text` in place of the selection, `InsertTextAtSelection`,
    /// leaving the caret after it. With `query_only`, change nothing and
    /// say where it would go.
    pub(crate) fn insert_at_selection(&mut self, text: &[u16], query_only: bool) -> TextChange {
        let range = self.selection.clone();
        if query_only {
            return TextChange {
                start: range.start,
                old_end: range.end,
                new_end: range.start + text.len(),
            };
        }
        let change = self
            .replace(range, text)
            .expect("the selection is always inside the text");
        self.selection = change.new_end..change.new_end;
        self.caret_at_start = false;
        change
    }

    /// Follow a change of the document the text service has not seen yet:
    /// `range` (UTF-16 units of the mirror's text) became `text`. Unlike
    /// [`replace`](Self::replace) it does not count as a text service's
    /// write. The change to report, `None` for a range outside the text.
    pub(crate) fn follow(&mut self, range: Range<usize>, text: &[u16]) -> Option<TextChange> {
        let written = self.written;
        let change = self.replace(range, text).ok();
        self.written = written;
        change
    }

    /// Follow the document's selection (UTF-16 offsets), clamped; whether
    /// it moved.
    pub(crate) fn follow_selection(
        &mut self,
        selection: Range<usize>,
        caret_at_start: bool,
    ) -> bool {
        let end = self.units.len();
        let start = selection.start.min(end);
        let selection = start..selection.end.min(end).max(start);
        let moved = selection != self.selection || caret_at_start != self.caret_at_start;
        self.selection = selection;
        self.caret_at_start = caret_at_start;
        moved
    }

    /// Set the selection, `SetSelection`.
    pub(crate) fn set_selection(
        &mut self,
        selection: Range<usize>,
        caret_at_start: bool,
    ) -> Result<(), MirrorError> {
        if selection.start > selection.end || selection.end > self.units.len() {
            return Err(MirrorError::InvalidPosition);
        }
        self.selection = selection;
        self.caret_at_start = caret_at_start;
        Ok(())
    }
}

/// The smallest change that turns `old` into `new`, `None` when equal.
pub(crate) fn diff(old: &[u16], new: &[u16]) -> Option<TextChange> {
    if old == new {
        return None;
    }
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    Some(TextChange {
        start: prefix,
        old_end: old.len() - suffix,
        new_end: new.len() - suffix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn synced(text: &str, selection: Range<usize>) -> TextMirror {
        let mut mirror = TextMirror::default();
        mirror.sync(text, selection, false);
        mirror
    }

    #[test]
    fn a_sync_reports_the_smallest_change_and_forgets_writes() {
        let mut mirror = synced("hello world", 5..5);
        mirror.replace(0..0, &units("x")).unwrap();
        assert!(mirror.written());
        let (change, moved) = mirror.sync("hello there", 11..11, false);
        assert_eq!(
            change,
            Some(TextChange {
                start: 0,
                old_end: 12,
                new_end: 11
            }),
            "against the written text"
        );
        assert!(moved);
        assert!(!mirror.written());
        assert_eq!(mirror.sync("hello there", 11..11, false), (None, false));
        assert_eq!(
            diff(&units("hello world"), &units("hello there")),
            Some(TextChange {
                start: 6,
                old_end: 11,
                new_end: 11
            })
        );
        assert_eq!(
            diff(&units("aaa"), &units("aaaa")),
            Some(TextChange {
                start: 3,
                old_end: 3,
                new_end: 4
            }),
            "a repeated letter: the prefix wins"
        );
    }

    #[test]
    fn a_sync_clamps_the_selection() {
        let mirror = synced("ab", 1..9);
        assert_eq!(mirror.selection(), 1..2);
        let mirror = synced("ab", 5..9);
        assert_eq!(mirror.selection(), 2..2);
    }

    #[test]
    fn text_is_read_and_replaced_in_utf16() {
        let mut mirror = synced("a\u{1F600}b", 4..4);
        assert_eq!(mirror.len(), 4);
        assert_eq!(mirror.get(1..3), &units("\u{1F600}")[..]);
        assert_eq!(
            mirror.get(2..99),
            &[0xDE00, u16::from(b'b')][..],
            "clamped, in units"
        );
        let change = mirror.replace(1..3, &units("xyz")).unwrap();
        assert_eq!(
            change,
            TextChange {
                start: 1,
                old_end: 3,
                new_end: 4
            }
        );
        assert_eq!(mirror.text(), "axyzb");
        assert_eq!(mirror.selection(), 5..5, "after the range: moved with it");
        assert_eq!(mirror.replace(3..9, &[]), Err(MirrorError::InvalidPosition));
    }

    #[test]
    fn a_selection_inside_a_replaced_range_collapses_to_its_end() {
        let mut mirror = synced("abcdef", 2..4);
        mirror.replace(1..5, &units("X")).unwrap();
        assert_eq!(mirror.selection(), 2..2);
    }

    #[test]
    fn an_insertion_takes_the_selection_and_leaves_the_caret_after_it() {
        let mut mirror = synced("abcd", 1..3);
        let query = mirror.insert_at_selection(&units("XY"), true);
        assert_eq!(
            query,
            TextChange {
                start: 1,
                old_end: 3,
                new_end: 3
            }
        );
        assert_eq!(mirror.text(), "abcd", "a query changes nothing");
        assert!(!mirror.written());
        mirror.insert_at_selection(&units("XY"), false);
        assert_eq!(mirror.text(), "aXYd");
        assert_eq!(mirror.selection(), 3..3);
        assert!(mirror.written());
    }

    #[test]
    fn following_the_document_is_not_a_write() {
        let mut mirror = synced("hello", 5..5);
        let change = mirror.follow(5..5, &units(" world")).unwrap();
        assert_eq!(
            change,
            TextChange {
                start: 5,
                old_end: 5,
                new_end: 11
            }
        );
        assert_eq!(mirror.text(), "hello world");
        assert!(!mirror.written());
        assert_eq!(mirror.follow(20..21, &[]), None);
        assert_eq!(mirror.selection(), 11..11, "the caret after the insertion");
        assert!(mirror.follow_selection(0..5, true));
        assert!(!mirror.follow_selection(0..5, true), "no move");
        assert!(mirror.follow_selection(3..40, false), "clamped");
        assert_eq!(mirror.selection(), 3..11);
    }

    #[test]
    fn a_selection_must_lie_in_the_text() {
        let mut mirror = synced("abc", 0..0);
        assert_eq!(mirror.set_selection(1..3, true), Ok(()));
        assert!(mirror.caret_at_start());
        assert_eq!(
            mirror.set_selection(2..4, false),
            Err(MirrorError::InvalidPosition)
        );
        #[expect(clippy::reversed_empty_ranges, reason = "a text service may send one")]
        let reversed = 2..1;
        assert_eq!(
            mirror.set_selection(reversed, false),
            Err(MirrorError::InvalidPosition)
        );
    }
}
