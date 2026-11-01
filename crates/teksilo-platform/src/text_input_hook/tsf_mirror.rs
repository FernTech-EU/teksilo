// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The text a TSF text store's text services last heard of: the half that
//! needs no platform.
//!
//! The store answers every read from the focused field's document
//! (`engineering/docs/tsf-text-store.md`, 4.8) and writes through the
//! field's write half (4.9). What it keeps is a [`TextMirror`]: the text and
//! selection, in the UTF-16 units TSF counts in (its ACP offsets), as they
//! were when the text services were last told of a change. A change is
//! reported against it (`OnTextChange`), and it follows the field's change
//! journal: the journal's range is applied to it ([`TextMirror::follow`]),
//! and the whole text is compared ([`TextMirror::sync`]) only when the
//! journal cannot say. It is also what a read answers while the document
//! cannot be read.

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

/// The text and selection the text services last heard of, in UTF-16 units.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TextMirror {
    units: Vec<u16>,
    selection: Range<usize>,
    /// Whether the caret is at the selection's start (`TS_AE_START`).
    caret_at_start: bool,
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

    /// The units, for a trace.
    pub(crate) fn text(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }

    /// Take the document's text and selection (UTF-16 offsets). The change
    /// to report (`OnTextChange`), if the text differs, and whether the
    /// selection moved.
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
        (change, moved)
    }

    /// The units of `range`, clamped to the text, `GetText`.
    pub(crate) fn get(&self, range: Range<usize>) -> &[u16] {
        let end = range.end.min(self.units.len());
        let start = range.start.min(end);
        &self.units[start..end]
    }

    /// Follow a change of the document: `range` (UTF-16 units of the
    /// mirror's text) became `text`. The change to report is the smallest
    /// one inside the range: a document that names a wider range than it
    /// changed (text-document's undo names the whole paragraph) is not told
    /// on, and `Some(None)` when the range reads as it did. A selection after
    /// the change moves with it; one inside it collapses to the end of the new
    /// text. `None` for a range outside the text.
    pub(crate) fn follow(
        &mut self,
        range: Range<usize>,
        text: &[u16],
    ) -> Option<Option<TextChange>> {
        if range.start > range.end || range.end > self.units.len() {
            return None;
        }
        let Some(local) = diff(&self.units[range.clone()], text) else {
            return Some(None);
        };
        let start = range.start + local.start;
        let range = start..range.start + local.old_end;
        let text = &text[local.start..local.new_end];
        let new_end = start + text.len();
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
        Some(Some(TextChange {
            start,
            old_end: range.end,
            new_end,
        }))
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
    fn a_sync_reports_the_smallest_change() {
        let mut mirror = synced("hello world", 5..5);
        let (change, moved) = mirror.sync("hello there", 11..11, false);
        assert_eq!(
            change,
            Some(TextChange {
                start: 6,
                old_end: 11,
                new_end: 11
            })
        );
        assert!(moved);
        assert_eq!(mirror.sync("hello there", 11..11, false), (None, false));
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
    fn text_is_read_and_followed_in_utf16() {
        let mut mirror = synced("a\u{1F600}b", 4..4);
        assert_eq!(mirror.len(), 4);
        assert_eq!(mirror.get(1..3), &units("\u{1F600}")[..]);
        assert_eq!(
            mirror.get(2..99),
            &[0xDE00, u16::from(b'b')][..],
            "clamped, in units"
        );
        let change = mirror.follow(1..3, &units("xyz")).unwrap().unwrap();
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
        assert_eq!(mirror.follow(3..9, &[]), None);
    }

    /// text-document's undo names the whole paragraph it restored; only the
    /// text that changed in it is reported.
    #[test]
    fn a_followed_range_is_narrowed_to_what_changed_in_it() {
        let mut mirror = synced("Title\nA sentence. Dictated", 26..26);
        let change = mirror
            .follow(6..26, &units("A sentence."))
            .unwrap()
            .unwrap();
        assert_eq!(
            change,
            TextChange {
                start: 17,
                old_end: 26,
                new_end: 17
            }
        );
        assert_eq!(mirror.text(), "Title\nA sentence.");
        assert_eq!(
            mirror.follow(0..5, &units("Title")),
            Some(None),
            "unchanged"
        );
    }

    #[test]
    fn a_selection_inside_a_followed_range_collapses_to_its_end() {
        let mut mirror = synced("abcdef", 2..4);
        mirror.follow(1..5, &units("X")).unwrap();
        assert_eq!(mirror.selection(), 2..2);
    }

    #[test]
    fn the_selection_follows_the_document() {
        let mut mirror = synced("hello", 5..5);
        assert!(mirror.follow_selection(0..5, true));
        assert!(mirror.caret_at_start());
        assert!(!mirror.follow_selection(0..5, true), "no move");
        assert!(mirror.follow_selection(3..40, false), "clamped");
        assert_eq!(mirror.selection(), 3..5);
    }
}
