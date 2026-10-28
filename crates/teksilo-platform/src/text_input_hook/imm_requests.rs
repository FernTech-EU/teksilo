// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a window answers an IMM32 input method's `WM_IME_REQUEST`, from the
//! focused text widget's [`TextInputDocument`]: the half that needs no
//! platform.
//!
//! winit 0.30 has no arm for `WM_IME_REQUEST`, so an input method asking an
//! IMM32 window about its document was always answered "nothing", and so was
//! every TSF text service CUAS serves the window through. The Windows hook
//! answers four of the requests from the document:
//!
//! - **`IMR_QUERYCHARPOSITION`**: where a character of the composition is
//!   drawn, so the input method places its candidate window by the
//!   character it converts. Round 5 counted 320 of them in one Japanese run.
//!   [`char_position`] answers, through [`answers::first_rect`].
//! - **`IMR_RECONVERTSTRING`**: the text to reconvert. The Japanese input
//!   method's conversion key on committed text, Hanja on a committed Korean
//!   syllable. [`reconvert_string`] hands over the selection with the text
//!   around it on its line, or, with nothing selected, the text around the
//!   caret and an empty composition string at the caret, for the input method
//!   to choose what it reconverts.
//! - **`IMR_CONFIRMRECONVERTSTRING`**: the input method names the part of
//!   that text it reconverts. The field's [`Reconversion`] checks it against
//!   what it handed over and keeps the range, as the next preedit's
//!   `replacement`, so the composition shows over that text rather than
//!   beside it ([`ImeComposition`](teksilo_core::WidgetEvent::ImeComposition)).
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

#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "only the Windows hook answers IMM requests")
)]

use std::ops::Range;

use teksilo_canvas::Rect;
use teksilo_core::text_input_document::{TextInputDocument, Utf16Index};

use super::answers::{self, Utf16Range};
use super::composition::MarkedReplacement;

/// The most characters handed over on either side of the text a request is
/// about. Chromium hands 20 around a composition for `IMR_DOCUMENTFEED`; a
/// reconversion with nothing selected needs the word around the caret, which
/// is rarely longer.
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

    /// The document's characters that `units` (a range of this string's
    /// UTF-16 units) covers, widened to whole characters; `None` for a range
    /// past the end of the string.
    pub(crate) fn document_range(&self, units: Range<usize>) -> Option<Range<usize>> {
        if units.start > units.end || units.end > self.units.len() {
            return None;
        }
        let text = String::from_utf16_lossy(&self.units);
        let chars = Utf16Index::new(&text).utf16_range_to_chars(units);
        Some(self.start + chars.start..self.start + chars.end)
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

/// `IMR_RECONVERTSTRING`: the selection, as the composition and target
/// strings, with the text around it on its line. With nothing selected, the
/// text around the caret and empty strings at it: the input method chooses
/// what it reconverts and names it in `IMR_CONFIRMRECONVERTSTRING`.
///
/// `None` while a composition is in progress (Chromium refuses then too), in
/// a field that takes no text or gives none out (read-only, secure), and
/// where the document cannot answer.
pub(crate) fn reconvert_string(document: &dyn TextInputDocument) -> Option<ReconvertString> {
    if !document.is_editable() || document.is_secure() || document.composition_range().is_some() {
        return None;
    }
    let selection = document.selection()?;
    around(document, selection.clone(), selection)
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

/// Whether a key down with virtual key `vk` ends a reconversion no preedit
/// has taken yet: every key but one the input method took (`VK_PROCESSKEY`),
/// a modifier, and the input method keys that start one (`VK_CONVERT`, the
/// Hanja key) or switch its mode. Those can reach the window as themselves
/// around the input method's requests; any other key can move the selection
/// or type.
pub(crate) fn key_ends_reconversion(vk: u16) -> bool {
    !matches!(
        vk,
        // VK_SHIFT, VK_CONTROL, VK_MENU.
        0x10..=0x12
        // VK_KANA / VK_HANGUL, VK_IME_ON, VK_JUNJA, VK_FINAL, VK_HANJA /
        // VK_KANJI, VK_IME_OFF.
        | 0x15..=0x1A
        // VK_CONVERT, VK_NONCONVERT, VK_ACCEPT, VK_MODECHANGE.
        | 0x1C..=0x1F
        // VK_LWIN, VK_RWIN.
        | 0x5B | 0x5C
        // VK_LSHIFT to VK_RMENU.
        | 0xA0..=0xA5
        // VK_PROCESSKEY.
        | 0xE5
        // The Japanese keyboard's mode keys (0xF0 to 0xF6: alphanumeric,
        // katakana, hiragana, single and double width, roman, no roman), and
        // the OEM keys VK_ATTN to VK_EREOF on it.
        | 0xF0..=0xF9
    )
}

/// A field's reconversion, from the text it handed over to the preedit that
/// shows over it.
///
/// The input method asks for the text (`IMR_RECONVERTSTRING`, kept here as
/// the offer), may name the part it reconverts (`IMR_CONFIRMRECONVERTSTRING`,
/// [`confirm`](Self::confirm)), then starts a composition, whose first
/// preedit takes the range ([`take`](Self::take)) as its replacement. A
/// selection handed over is that range until a confirmation names another:
/// the input method reconverts what was selected. A range no preedit takes
/// must not ride on a later one, so [`clear`](Self::clear) forgets it at
/// every event that can move the selection or end the reconversion: a key
/// the input method passed on, a click, the focus leaving, the end of a
/// composition.
#[derive(Debug, Default)]
pub(crate) struct Reconversion {
    offer: Option<ReconvertString>,
    replacement: MarkedReplacement,
}

impl Reconversion {
    /// `string` was handed over for a reconversion; a selection in it is the
    /// range the input method reconverts, unless it confirms another.
    pub(crate) fn offered(&mut self, string: ReconvertString) {
        let selected = (!string.target.is_empty())
            .then(|| string.document_range(string.target.clone()))
            .flatten();
        self.replacement.record(selected);
        self.offer = Some(string);
    }

    /// The input method confirms it reconverts `composition` (UTF-16 units)
    /// of `units`, which must be the text last handed over, still in the
    /// document where it was. Returns whether it was accepted; the range is
    /// then the next preedit's replacement.
    pub(crate) fn confirm(
        &mut self,
        document: &dyn TextInputDocument,
        units: &[u16],
        composition: Range<usize>,
    ) -> bool {
        let Some(offer) = &self.offer else {
            return false;
        };
        if offer.units != units {
            return false;
        }
        let whole = offer.document_range(0..offer.units.len());
        let still_there = whole
            .and_then(|whole| document.text_in(whole))
            .is_some_and(|text| text.encode_utf16().eq(offer.units.iter().copied()));
        if !still_there {
            return false;
        }
        let Some(range) = offer.document_range(composition) else {
            return false;
        };
        self.replacement.record(Some(range));
        true
    }

    /// The range the preedit being translated replaces; once.
    pub(crate) fn take(&mut self) -> Option<Range<usize>> {
        self.replacement.take()
    }

    /// Nothing is being reconverted any more.
    pub(crate) fn clear(&mut self) {
        self.offer = None;
        self.replacement.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use teksilo_canvas::Point;
    use teksilo_core::text_input_document::FirstLineRect;

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
    fn a_selection_is_handed_over_with_its_line_around_it() {
        // "한 漢字 test": select 漢字.
        let mut document = Line::new("first\n\u{D55C} \u{6F22}\u{5B57} test\nlast");
        document.selection = 8..10;
        let string = reconvert_string(&document).expect("an answer");
        assert_eq!(string.start, 6, "from the start of its paragraph");
        assert_eq!(string.units, utf16("\u{D55C} \u{6F22}\u{5B57} test"));
        assert_eq!(string.composition, 2..4);
        assert_eq!(string.target, 2..4);
        assert_eq!(string.document_range(2..4), Some(8..10));
    }

    #[test]
    fn with_nothing_selected_the_caret_is_an_empty_string_in_its_text() {
        // The caret right after a committed syllable: the input method
        // chooses the syllable and confirms it.
        let mut document = Line::new("ab\u{D55C}");
        document.selection = 3..3;
        let string = reconvert_string(&document).expect("an answer");
        assert_eq!(string.units, utf16("ab\u{D55C}"));
        assert_eq!(string.composition, 3..3);
        assert_eq!(string.target, 3..3);
    }

    #[test]
    fn the_context_is_bounded_and_counted_in_utf16() {
        let text = format!("{}\u{1F600}x\u{1F600}{}", "a".repeat(100), "b".repeat(100));
        let mut document = Line::new(&text);
        // Select the "x" between the emoji.
        document.selection = 101..102;
        let string = reconvert_string(&document).expect("an answer");
        assert_eq!(string.start, 101 - CONTEXT_CHARS);
        // 63 "a", an emoji (two units), then the "x".
        assert_eq!(string.target, 65..66);
        assert_eq!(
            string.units.len(),
            (CONTEXT_CHARS - 1) + 2 + 1 + 2 + (CONTEXT_CHARS - 1)
        );
        assert_eq!(string.document_range(65..66), Some(101..102));
        assert_eq!(
            string.document_range(64..65),
            Some(100..101),
            "half an emoji widens to the whole of it"
        );
        assert_eq!(string.document_range(0..1000), None, "past the end");
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
    fn no_reconversion_while_composing_nor_in_a_secure_or_read_only_field() {
        let mut document = Line::new("abc");
        document.selection = 0..1;
        document.composition = Some(1..2);
        assert_eq!(
            reconvert_string(&document),
            None,
            "a composition in progress"
        );
        document.composition = None;
        document.secure = true;
        assert_eq!(reconvert_string(&document), None);
        document.secure = false;
        document.editable = false;
        assert_eq!(reconvert_string(&document), None);
        document.editable = true;
        document.busy = true;
        assert_eq!(reconvert_string(&document), None);
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

    #[test]
    fn a_confirmed_range_replaces_the_next_preedit_once() {
        let mut document = Line::new("ab\u{D55C}");
        document.selection = 3..3;
        let mut reconversion = Reconversion::default();
        let string = reconvert_string(&document).expect("an answer");
        let units = string.units.clone();
        reconversion.offered(string);
        assert_eq!(
            reconversion.take(),
            None,
            "nothing selected: nothing replaced until the input method names it"
        );
        assert!(reconversion.confirm(&document, &units, 2..3));
        assert_eq!(reconversion.take(), Some(2..3), "the syllable");
        assert_eq!(reconversion.take(), None, "once");
    }

    #[test]
    fn a_selection_handed_over_is_the_range_unless_another_is_confirmed() {
        let mut document = Line::new("x \u{6F22}\u{5B57} y");
        document.selection = 2..4;
        let mut reconversion = Reconversion::default();
        let string = reconvert_string(&document).expect("an answer");
        let units = string.units.clone();
        reconversion.offered(string.clone());
        assert_eq!(reconversion.take(), Some(2..4));

        reconversion.offered(string);
        assert!(reconversion.confirm(&document, &units, 3..4));
        assert_eq!(reconversion.take(), Some(3..4), "the confirmed part");
    }

    #[test]
    fn a_confirmation_of_other_text_is_refused() {
        let mut document = Line::new("abc");
        document.selection = 1..2;
        let mut reconversion = Reconversion::default();
        assert!(
            !reconversion.confirm(&document, &utf16("abc"), 0..1),
            "nothing was handed over"
        );
        let string = reconvert_string(&document).expect("an answer");
        reconversion.offered(string);
        assert!(!reconversion.confirm(&document, &utf16("abd"), 0..1));
        assert!(
            !reconversion.confirm(&document, &utf16("abc"), 2..9),
            "past the end of the string"
        );
        document.text = "zbc".to_owned();
        assert!(
            !reconversion.confirm(&document, &utf16("abc"), 0..1),
            "the document changed since"
        );
        assert_eq!(reconversion.take(), Some(1..2), "the offer still stands");
    }

    #[test]
    fn only_keys_that_can_move_the_selection_end_a_reconversion() {
        // VK_PROCESSKEY, VK_CONVERT, VK_HANJA, VK_HANGUL, Shift, the left
        // Windows key, the Japanese keyboard's Hiragana key.
        for vk in [0xE5, 0x1C, 0x19, 0x15, 0x10, 0x5B, 0xF2] {
            assert!(!key_ends_reconversion(vk), "{vk:#x}");
        }
        // A letter, Space, the arrows, Home, Backspace, Delete, Enter, Tab,
        // Escape, a Unicode packet.
        for vk in [
            0x41, 0x20, 0x25, 0x26, 0x27, 0x28, 0x24, 0x08, 0x2E, 0x0D, 0x09, 0x1B, 0xE7,
        ] {
            assert!(key_ends_reconversion(vk), "{vk:#x}");
        }
    }

    #[test]
    fn a_range_no_preedit_took_is_cleared() {
        let mut document = Line::new("abc");
        document.selection = 0..2;
        let mut reconversion = Reconversion::default();
        reconversion.offered(reconvert_string(&document).expect("an answer"));
        reconversion.clear();
        assert_eq!(reconversion.take(), None);
        assert!(
            !reconversion.confirm(&document, &utf16("abc"), 0..1),
            "the offer went too"
        );
    }
}
