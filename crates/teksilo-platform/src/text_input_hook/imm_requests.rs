// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a window answers an IMM32 input method's `WM_IME_REQUEST`, from the
//! focused text widget's [`TextInputDocument`]: the half that needs no
//! platform.
//!
//! winit 0.30 has no arm for `WM_IME_REQUEST`, so an input method asking an
//! IMM32 window about its document was always answered "nothing", and so was
//! every TSF text service CUAS serves the window through. The Windows hook
//! answers two of the requests from the document:
//!
//! - **`IMR_QUERYCHARPOSITION`**: where a character of the composition is
//!   drawn, so the input method places its candidate window by the
//!   character it converts. Round 5 counted 320 of them in one Japanese run.
//!   [`char_position`] answers, through [`answers::first_rect`].
//! - **`IMR_DOCUMENTFEED`**: the text around the composition or the caret,
//!   which the Japanese input method reads as it predicts. [`document_feed`]
//!   answers, with the composition (or the selection) as the target, as
//!   Chromium does.
//!
//! The text goes to the input method as a [`ReconvertString`]: UTF-16, and
//! the composition and target strings as UTF-16 ranges inside it, which
//! [`ReconvertString::fields`] lays out as `RECONVERTSTRING` counts them
//! (lengths in units, offsets in bytes from the start of the string).
//!
//! Every function answers `None` where the document cannot answer (busy, no
//! layout, a secure field's text), and the hook then lets the request go on
//! to winit, which answers it as it always did.
//!
//! Reconversion (`IMR_RECONVERTSTRING`, `IMR_CONFIRMRECONVERTSTRING`) is not
//! answered: measured with the Microsoft Japanese IME, it opened in one
//! session and never after, with the text around the selection or the
//! selection alone, while a Win32 edit control reconverted; see the design
//! document. It is left for the TSF store.

#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "only the Windows hook answers IMM requests")
)]

use std::ops::Range;

use teksilo_canvas::Rect;
use teksilo_core::text_input_document::TextInputDocument;

use super::answers::{self, Utf16Range};

/// The most characters handed over on either side of the text a request is
/// about. Chromium hands 20 around a composition for `IMR_DOCUMENTFEED`.
pub(crate) const CONTEXT_CHARS: usize = 64;

/// The size of `RECONVERTSTRING`'s header: eight `DWORD`s, the string after
/// it.
pub(crate) const RECONVERTSTRING_HEADER: usize = 32;

/// Text handed to the input method in a `RECONVERTSTRING`: a run of the
/// document, and the composition and target strings inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReconvertString {
    /// The document's character offset `units` starts at.
    pub(crate) start: usize,
    /// The run of text, UTF-16.
    pub(crate) units: Vec<u16>,
    /// The composition string, in UTF-16 units of `units`.
    pub(crate) composition: Range<usize>,
    /// The target string (the clause the input method converts), in UTF-16
    /// units of `units`.
    pub(crate) target: Range<usize>,
}

/// `RECONVERTSTRING`'s fields for a [`ReconvertString`] written right after
/// the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReconvertFields {
    /// `dwSize`: the header and the string, in bytes.
    pub(crate) size: u32,
    /// `dwStrLen`, in UTF-16 units.
    pub(crate) str_len: u32,
    /// `dwStrOffset`: from the start of the structure, in bytes.
    pub(crate) str_offset: u32,
    /// `dwCompStrLen`, in UTF-16 units.
    pub(crate) comp_len: u32,
    /// `dwCompStrOffset`: from the start of the string, in bytes.
    pub(crate) comp_offset: u32,
    /// `dwTargetStrLen`, in UTF-16 units.
    pub(crate) target_len: u32,
    /// `dwTargetStrOffset`: from the start of the string, in bytes.
    pub(crate) target_offset: u32,
}

impl ReconvertString {
    /// The fields, `None` for a string too long to count in a `DWORD`.
    pub(crate) fn fields(&self) -> Option<ReconvertFields> {
        let bytes = |units: usize| u32::try_from(units.checked_mul(2)?).ok();
        let units = |count: usize| u32::try_from(count).ok();
        Some(ReconvertFields {
            size: u32::try_from(RECONVERTSTRING_HEADER.checked_add(self.units.len() * 2)?).ok()?,
            str_len: units(self.units.len())?,
            str_offset: RECONVERTSTRING_HEADER as u32,
            comp_len: units(self.composition.len())?,
            comp_offset: bytes(self.composition.start)?,
            target_len: units(self.target.len())?,
            target_offset: bytes(self.target.start)?,
        })
    }
}

/// The text around `target` (document characters) on its line, at most
/// [`CONTEXT_CHARS`] on either side, as a [`ReconvertString`] whose target is
/// `target` and whose composition is `composition` (which lies inside
/// `target`, or is empty at `target`'s start).
fn around(
    document: &dyn TextInputDocument,
    target: Range<usize>,
    composition: Range<usize>,
) -> Option<ReconvertString> {
    debug_assert!(target.start <= composition.start && composition.end <= target.end);
    let before = document.text_in(target.start.saturating_sub(CONTEXT_CHARS)..target.start)?;
    let middle = document.text_in(target.clone())?;
    let after = document.text_in(target.end..target.end.saturating_add(CONTEXT_CHARS))?;
    // Nothing past a paragraph break: an input method converts within one.
    let before = match before.rfind('\n') {
        Some(at) => &before[at + 1..],
        None => before.as_str(),
    };
    let after = match after.find('\n') {
        Some(at) => &after[..at],
        None => after.as_str(),
    };
    let start = target.start - before.chars().count();
    let mut units: Vec<u16> = before.encode_utf16().collect();
    let before_units = units.len();
    // UTF-16 units from `start` to the document character `at`, inside
    // `target`.
    let offset = |at: usize| -> usize {
        let chars = at.saturating_sub(target.start);
        before_units
            + middle
                .chars()
                .take(chars)
                .map(char::len_utf16)
                .sum::<usize>()
    };
    let target_units = offset(target.start)..offset(target.end);
    let composition_units = offset(composition.start)..offset(composition.end);
    units.extend(middle.encode_utf16());
    units.extend(after.encode_utf16());
    Some(ReconvertString {
        start,
        units,
        composition: composition_units,
        target: target_units,
    })
}

/// `IMR_DOCUMENTFEED`: the text around the composition, which is its
/// composition and target strings; with no composition, around the
/// selection, which is the target, with an empty composition string at its
/// start. Chromium's answer.
///
/// `None` in a secure field and where the document cannot answer.
pub(crate) fn document_feed(document: &dyn TextInputDocument) -> Option<ReconvertString> {
    if document.is_secure() {
        return None;
    }
    match document.composition_range() {
        Some(composition) if !composition.is_empty() => {
            around(document, composition.clone(), composition)
        }
        _ => {
            let selection = document.selection()?;
            let at = selection.start;
            around(document, selection, at..at)
        }
    }
}

/// The answer to `IMR_QUERYCHARPOSITION`, window-logical: the platform
/// backend converts it to the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CharPosition {
    /// The caret rectangle at the character: its left edge, its line's top
    /// and its line's height.
    pub(crate) caret: Rect,
    /// The part of the text that is on screen (`rcDocument`).
    pub(crate) document: Option<Rect>,
}

/// `IMR_QUERYCHARPOSITION`: where the character `char_pos` UTF-16 units
/// into the composition is drawn. With no composition, `char_pos` counts
/// from the selection's start, and runs at most to its end, as Firefox
/// answers; `char_pos` may be the composition's (the selection's) length,
/// the caret after it.
///
/// `None` for a position past that end, and where the document cannot
/// answer.
pub(crate) fn char_position(
    document: &dyn TextInputDocument,
    char_pos: usize,
) -> Option<CharPosition> {
    let index = document.utf16_index()?;
    let base = match document.composition_range() {
        Some(composition) => composition,
        None => document.selection()?,
    };
    let base = index.char_range_to_utf16(base);
    let location = base.start.checked_add(char_pos)?;
    if location > base.end {
        return None;
    }
    let (caret, _) = answers::first_rect(document, Utf16Range::from_range(location..location))?;
    Some(CharPosition {
        caret,
        document: answers::visible_rect(document),
    })
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use teksilo_canvas::Point;
    use teksilo_core::text_input_document::{FirstLineRect, Utf16Index};

    use super::*;

    /// One line of text, every character 10 wide and 20 tall.
    struct Line {
        text: String,
        selection: Range<usize>,
        composition: Option<Range<usize>>,
        editable: bool,
        secure: bool,
        busy: bool,
    }

    impl Line {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_owned(),
                selection: 0..0,
                composition: None,
                editable: true,
                secure: false,
                busy: false,
            }
        }

        fn len(&self) -> usize {
            self.text.chars().count()
        }
    }

    impl TextInputDocument for Line {
        fn is_available(&self) -> bool {
            !self.busy
        }
        fn char_count(&self) -> Option<usize> {
            (!self.busy).then(|| self.len())
        }
        fn text_in(&self, range: Range<usize>) -> Option<String> {
            if self.busy || self.secure {
                return None;
            }
            let end = range.end.min(self.len());
            let start = range.start.min(end);
            Some(self.text.chars().skip(start).take(end - start).collect())
        }
        fn selection(&self) -> Option<Range<usize>> {
            (!self.busy).then(|| self.selection.clone())
        }
        fn caret(&self) -> Option<usize> {
            (!self.busy).then_some(self.selection.end)
        }
        fn composition_range(&self) -> Option<Range<usize>> {
            if self.busy {
                None
            } else {
                self.composition.clone()
            }
        }
        fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect> {
            if self.busy {
                return None;
            }
            let end = range.end.min(self.len());
            let start = range.start.min(end);
            Some(FirstLineRect {
                rect: Rect::new(
                    start as f32 * 10.0,
                    0.0,
                    ((end - start) as f32 * 10.0).max(1.0),
                    20.0,
                ),
                covered: start..end,
            })
        }
        fn offset_at_point(&self, _: Point) -> Option<usize> {
            None
        }
        fn visible_text_rect(&self) -> Option<Rect> {
            (!self.busy).then(|| Rect::new(0.0, 0.0, 300.0, 20.0))
        }
        fn is_editable(&self) -> bool {
            !self.busy && self.editable
        }
        fn is_secure(&self) -> bool {
            self.busy || self.secure
        }
        fn writing_tools_allowed(&self) -> bool {
            false
        }
        fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
            (!self.busy).then(|| Rc::new(Utf16Index::new(&self.text)))
        }
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn the_context_is_bounded_and_counted_in_utf16() {
        let text = format!("{}\u{1F600}x\u{1F600}{}", "a".repeat(100), "b".repeat(100));
        let mut document = Line::new(&text);
        // Select the "x" between the emoji.
        document.selection = 101..102;
        let string = document_feed(&document).expect("an answer");
        assert_eq!(string.start, 101 - CONTEXT_CHARS);
        // 63 "a", an emoji (two units), then the "x".
        assert_eq!(string.target, 65..66);
        assert_eq!(string.composition, 65..65);
        assert_eq!(
            string.units.len(),
            (CONTEXT_CHARS - 1) + 2 + 1 + 2 + (CONTEXT_CHARS - 1)
        );
    }

    #[test]
    fn the_context_stops_at_a_paragraph_break() {
        let mut document = Line::new("first\n\u{D55C} \u{6F22}\u{5B57} test\nlast");
        document.selection = 8..10;
        let string = document_feed(&document).expect("an answer");
        assert_eq!(string.start, 6, "from the start of its paragraph");
        assert_eq!(string.units, utf16("\u{D55C} \u{6F22}\u{5B57} test"));
        assert_eq!(string.target, 2..4);
    }

    #[test]
    fn the_fields_count_units_and_offset_bytes() {
        let string = ReconvertString {
            start: 0,
            units: utf16("abcdef"),
            composition: 2..5,
            target: 3..4,
        };
        assert_eq!(
            string.fields(),
            Some(ReconvertFields {
                size: 32 + 12,
                str_len: 6,
                str_offset: 32,
                comp_len: 3,
                comp_offset: 4,
                target_len: 1,
                target_offset: 6,
            })
        );
    }

    #[test]
    fn the_document_feed_targets_the_composition_else_the_selection() {
        let mut document = Line::new("say \u{306B}\u{307B}\u{3093} now");
        document.selection = 7..7;
        document.composition = Some(4..7);
        let feed = document_feed(&document).expect("an answer");
        assert_eq!(feed.start, 0);
        assert_eq!(feed.composition, 4..7);
        assert_eq!(feed.target, 4..7);

        document.composition = None;
        document.selection = 4..6;
        let feed = document_feed(&document).expect("an answer");
        assert_eq!(feed.composition, 4..4, "no composition: an empty string");
        assert_eq!(feed.target, 4..6, "the selection");

        document.secure = true;
        assert_eq!(document_feed(&document), None);
    }

    #[test]
    fn a_character_of_the_composition_is_placed_by_its_caret() {
        let mut document = Line::new("ab\u{1F600}\u{306B}\u{307B}");
        document.composition = Some(3..5);
        document.selection = 5..5;
        let first = char_position(&document, 0).expect("an answer");
        assert_eq!(first.caret, Rect::new(30.0, 0.0, 1.0, 20.0));
        assert_eq!(first.document, Some(Rect::new(0.0, 0.0, 300.0, 20.0)));
        assert_eq!(
            char_position(&document, 2).map(|p| p.caret.x),
            Some(50.0),
            "its end, the caret after it"
        );
        assert_eq!(char_position(&document, 3), None, "past its end");
    }

    #[test]
    fn with_no_composition_the_selection_is_counted_from() {
        let mut document = Line::new("\u{1F600}abc");
        document.selection = 1..3;
        assert_eq!(
            char_position(&document, 0).map(|p| p.caret.x),
            Some(10.0),
            "the selection's start, past the emoji's two units"
        );
        assert_eq!(char_position(&document, 2).map(|p| p.caret.x), Some(30.0));
        assert_eq!(char_position(&document, 3), None);
        document.selection = 4..4;
        assert_eq!(char_position(&document, 0).map(|p| p.caret.x), Some(40.0));
        assert_eq!(char_position(&document, 1), None);
        document.busy = true;
        assert_eq!(char_position(&document, 0), None);
    }
}
