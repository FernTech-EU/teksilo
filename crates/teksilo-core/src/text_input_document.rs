// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextInputDocument`: a text widget's document as the platform's text input
//! system reads it.
//!
//! # Why the framework has to answer this
//!
//! A platform's text features do not only insert text; they read the
//! document they insert into. On macOS, Dictation, the press-and-hold accent
//! popup and Writing Tools each start by asking the view where the selection
//! is (`selectedRange`), and Writing Tools then reads the selected text
//! (`attributedSubstringForProposedRange:actualRange:`). A view that answers
//! "nowhere" and "nothing" gets nothing back: dictation never starts, the
//! accent popup never shows, Writing Tools says "Try a longer selection". The
//! same view is asked where a range is on screen
//! (`firstRectForCharacterRange:actualRange:`) to place a candidate window or
//! a popover, and which character is under a point
//! (`characterIndexForPoint:`).
//!
//! There is one native view per window and many text widgets in it, so the
//! view can only answer for the focused one, and only that widget knows its
//! own document. This trait is how it says what it knows. A widget reaches it
//! through [`TextSurface::input_document`](crate::text_surface::TextSurface::input_document),
//! so a platform bridge holding the window's
//! [`TextSurfaces`](crate::text_surface::TextSurfaces) answers for whichever
//! text widget has focus without knowing which kind it is.
//!
//! # Offsets
//!
//! Every offset crossing this trait is a **character** offset (a count of
//! Unicode scalar values) into the widget's document, the space its caret,
//! its selection and an [`ImeCommit`](crate::event::WidgetEvent::ImeCommit)'s
//! replacement range are in. The platforms count in UTF-16 code units instead
//! (an `NSRange` does), so a character outside the Basic Multilingual Plane,
//! an emoji, is one offset here and two there. [`Utf16Index`] converts
//! between the two over the same text, and
//! [`utf16_index`](TextInputDocument::utf16_index) hands out the one for the
//! document as it stands.
//!
//! # Coordinates
//!
//! Every point and rectangle is in **window** logical coordinates, the space
//! [`WidgetEvent::PointerDown`](crate::event::WidgetEvent::PointerDown)
//! positions arrive in and the IME cursor area is reported in. The bridge
//! converts to the platform's screen space.
//!
//! # Busy and unanswerable
//!
//! The bridge is called by the platform whenever the platform likes, which
//! can be while the widget is in the middle of its own work with its state
//! borrowed. Every query that returns an `Option` answers `None` then, and
//! the boolean queries give the cautious answer their documentation names,
//! rather than panicking. `None` is also the answer when there is nothing to
//! say: no layout yet, a point outside the text, a secure field's text.
//!
//! # Typing not yet inserted
//!
//! A widget that batches typed characters until its next frame must answer as
//! if they were in the document: the input system names offsets in the text
//! the user has typed, and the commit that follows a question is applied to
//! that text. The framework's widgets insert the batch before answering, as
//! their frame tick would; see each implementation for why that is safe
//! there.

use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};

/// A text widget's document, as a platform's text input system reads it.
///
/// See the [module documentation](self) for the offsets (characters), the
/// coordinates (window-logical) and what `None` means. Object-safe: a bridge
/// holds `&dyn TextInputDocument` for whichever widget has focus.
pub trait TextInputDocument {
    /// The length of the document, in characters.
    fn char_count(&self) -> Option<usize>;

    /// The text of `range`, clamped to the document.
    ///
    /// In the document's own character space: a paragraph break is one `\n`,
    /// a line break inside a paragraph is `U+2028`, an embedded object (an
    /// image, a table's anchor) is its `U+FFFC`, so offsets into the answer
    /// are offsets into the document. `None` for a secure field, whatever its
    /// reveal state: its text does not leave it.
    fn text_in(&self, range: Range<usize>) -> Option<String>;

    /// The selection, lowest offset first, whichever end the caret is at. An
    /// empty range is a caret with nothing selected.
    fn selection(&self) -> Option<Range<usize>>;

    /// The caret: the end of the selection that moves.
    fn caret(&self) -> Option<usize>;

    /// The range the input method's composition occupies, while one is in
    /// progress: its preedit, inserted tentatively in the document (macOS
    /// calls it marked text). `None` when nothing is being composed.
    fn composition_range(&self) -> Option<Range<usize>>;

    /// Where the first line of `range` is drawn, and which characters of
    /// `range` that line holds.
    ///
    /// `range` is clamped to the document. A range that wraps onto later
    /// lines answers for its first line only, and
    /// [`covered`](FirstLineRect::covered) ends where that line does, so a
    /// caller wanting every line asks again from there. For a non-empty range
    /// `covered` is never empty, so such a walk always advances. An empty
    /// range answers with the caret rectangle at its offset.
    ///
    /// A secure field that shows nothing at all (no echo) answers every range
    /// with the same rectangle, at the start of its empty line, covering the
    /// whole range: its geometry would otherwise tell the text's length.
    ///
    /// `None` before the widget's first layout.
    fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect>;

    /// The character offset nearest `window_point`.
    ///
    /// `None` outside the [`visible_text_rect`](Self::visible_text_rect), and
    /// for a secure field that shows nothing. Inside it, a point beside a
    /// line answers that line's nearest end, and a point below the last line
    /// the end of the document.
    fn offset_at_point(&self, window_point: Point) -> Option<usize>;

    /// The rectangle the widget shows its text in, the part of the document
    /// that is on screen.
    fn visible_text_rect(&self) -> Option<Rect>;

    /// Does the widget take text? `false` for a read-only one, which refuses
    /// an input-system commit, and while the widget is busy.
    fn is_editable(&self) -> bool;

    /// Is this a secure field, a password? Its text is never given out (see
    /// [`text_in`](Self::text_in)), and a bridge offers it none of the
    /// features that would read it or write into it from outside the
    /// keyboard. `true` while the widget is busy, the cautious answer.
    fn is_secure(&self) -> bool;

    /// May the system's Writing Tools read and replace this widget's text?
    /// The application's `writing_tools` setting; always `false` for a
    /// secure field, and while the widget is busy.
    fn writing_tools_allowed(&self) -> bool;

    /// The UTF-16 index of the document as it stands, for converting
    /// offsets.
    ///
    /// Built once per change of the document's text and shared until the
    /// next, so a caller converting several offsets for one question asks
    /// for it once. The conversion methods below go through it.
    fn utf16_index(&self) -> Option<Rc<Utf16Index>>;

    /// A character offset as a UTF-16 offset. See
    /// [`Utf16Index::char_to_utf16`].
    fn char_to_utf16(&self, char_offset: usize) -> Option<usize> {
        Some(self.utf16_index()?.char_to_utf16(char_offset))
    }

    /// A UTF-16 offset as a character offset. See
    /// [`Utf16Index::utf16_to_char`].
    fn utf16_to_char(&self, utf16_offset: usize) -> Option<usize> {
        Some(self.utf16_index()?.utf16_to_char(utf16_offset))
    }

    /// A character range as a UTF-16 range. See
    /// [`Utf16Index::char_range_to_utf16`].
    fn char_range_to_utf16(&self, range: Range<usize>) -> Option<Range<usize>> {
        Some(self.utf16_index()?.char_range_to_utf16(range))
    }

    /// A UTF-16 range as a character range. See
    /// [`Utf16Index::utf16_range_to_chars`].
    fn utf16_range_to_chars(&self, range: Range<usize>) -> Option<Range<usize>> {
        Some(self.utf16_index()?.utf16_range_to_chars(range))
    }
}

/// The answer to [`TextInputDocument::first_line_rect`].
#[derive(Debug, Clone, PartialEq)]
pub struct FirstLineRect {
    /// The rectangle, in window-logical coordinates: from the left of the
    /// line's first character in the range to the right of its last, one
    /// line tall. At least 1 wide, so a caret has a width.
    pub rect: Rect,
    /// The characters of the asked range that `rect` covers: the start of
    /// the asked range (clamped) up to the end of the range or of its first
    /// line, whichever comes first.
    pub covered: Range<usize>,
}

/// Character offsets and UTF-16 offsets over one text, both ways.
///
/// A character takes one UTF-16 code unit unless it is outside the Basic
/// Multilingual Plane (above `U+FFFF`, most emoji), when it takes two, a
/// surrogate pair. So the index keeps only where those characters are.
///
/// # Invariant
///
/// `supplementary` holds, in ascending order, the character offset of every
/// character above `U+FFFF`. The UTF-16 offset of character offset `c` is
/// `c` plus the number of entries below `c`, and the `k`-th entry (from 0)
/// starts at UTF-16 offset `supplementary[k] + k`, which also ascends. Both
/// conversions are a binary search over it: `O(log s)` for `s` such
/// characters, nothing at all for a text with none, and the index takes
/// `s` words. Building it reads the text once.
///
/// # Rounding
///
/// A UTF-16 offset can fall between the two halves of a surrogate pair, in
/// the middle of a character, where no character offset is. A single offset
/// is moved back to the start of that character; a range is widened to take
/// in the whole character at either end. Every conversion clamps to the
/// text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Utf16Index {
    char_count: usize,
    supplementary: Vec<usize>,
}

impl Utf16Index {
    /// Index `text`. Reads it once.
    pub fn new(text: &str) -> Self {
        let mut char_count = 0;
        let mut supplementary = Vec::new();
        for c in text.chars() {
            if c.len_utf16() == 2 {
                supplementary.push(char_count);
            }
            char_count += 1;
        }
        Self {
            char_count,
            supplementary,
        }
    }

    /// The length of the text, in characters.
    pub fn char_count(&self) -> usize {
        self.char_count
    }

    /// The length of the text, in UTF-16 code units.
    pub fn utf16_len(&self) -> usize {
        self.char_count + self.supplementary.len()
    }

    /// The UTF-16 offset of character offset `char_offset`, clamped to the
    /// end of the text.
    pub fn char_to_utf16(&self, char_offset: usize) -> usize {
        let c = char_offset.min(self.char_count);
        c + self.supplementary.partition_point(|&s| s < c)
    }

    /// The character offset of UTF-16 offset `utf16_offset`, clamped to the
    /// end of the text. An offset between the halves of a surrogate pair is
    /// the start of that character.
    pub fn utf16_to_char(&self, utf16_offset: usize) -> usize {
        self.utf16_to_char_rounding(utf16_offset, false)
    }

    /// A character range as the UTF-16 range over the same characters. Both
    /// ends are clamped; an end before the start is taken as the start.
    pub fn char_range_to_utf16(&self, range: Range<usize>) -> Range<usize> {
        let start = self.char_to_utf16(range.start);
        let end = self.char_to_utf16(range.end).max(start);
        start..end
    }

    /// A UTF-16 range as the character range over the same characters,
    /// widened to whole characters: a start in the middle of a surrogate
    /// pair moves back to the character's start, an end in the middle moves
    /// on past the character. An empty range stays empty, an insertion point
    /// at its start's character offset, so it never takes in a character
    /// it only touched. Both ends are clamped; an end before the start is
    /// taken as the start.
    pub fn utf16_range_to_chars(&self, range: Range<usize>) -> Range<usize> {
        let start = self.utf16_to_char_rounding(range.start, false);
        if range.end <= range.start {
            return start..start;
        }
        let end = self.utf16_to_char_rounding(range.end, true).max(start);
        start..end
    }

    /// The character at `utf16_offset`, or the one after it when `up` and
    /// the offset is in the middle of a surrogate pair.
    fn utf16_to_char_rounding(&self, utf16_offset: usize, up: bool) -> usize {
        let u = utf16_offset.min(self.utf16_len());
        // How many supplementary characters end at or before `u`: the `k`-th
        // spans UTF-16 offsets `s + k` and `s + k + 1`, so it ends at
        // `s + k + 2`, and that end ascends with `k`.
        let (mut low, mut high) = (0, self.supplementary.len());
        while low < high {
            let middle = (low + high) / 2;
            if self.supplementary[middle] + middle + 2 <= u {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let ended = low;
        let char_offset = u - ended;
        // The next one may start before `u`, which then sits between its
        // halves: `char_offset` is one past its start.
        let splits_a_pair = self
            .supplementary
            .get(ended)
            .is_some_and(|&s| s + ended < u);
        if splits_a_pair && !up {
            char_offset - 1
        } else {
            char_offset
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The conversions, checked against the standard library's own UTF-16
    /// encoding of every prefix.
    fn brute_char_to_utf16(text: &str, c: usize) -> usize {
        text.chars().take(c).map(char::len_utf16).sum()
    }

    #[test]
    fn ascii_converts_one_for_one() {
        let index = Utf16Index::new("hello");
        assert_eq!(index.char_count(), 5);
        assert_eq!(index.utf16_len(), 5);
        for c in 0..=5 {
            assert_eq!(index.char_to_utf16(c), c);
            assert_eq!(index.utf16_to_char(c), c);
        }
    }

    #[test]
    fn a_supplementary_character_takes_two_units_and_one_offset() {
        // "a😀b": the emoji is character 1 and UTF-16 units 1 and 2.
        let text = "a\u{1F600}b";
        let index = Utf16Index::new(text);
        assert_eq!(index.char_count(), 3);
        assert_eq!(index.utf16_len(), 4);
        assert_eq!(index.utf16_len(), text.encode_utf16().count());
        assert_eq!(index.char_to_utf16(0), 0);
        assert_eq!(index.char_to_utf16(1), 1);
        assert_eq!(index.char_to_utf16(2), 3, "past the pair");
        assert_eq!(index.char_to_utf16(3), 4);
        assert_eq!(index.utf16_to_char(0), 0);
        assert_eq!(index.utf16_to_char(1), 1);
        assert_eq!(index.utf16_to_char(2), 1, "between the halves: the emoji");
        assert_eq!(index.utf16_to_char(3), 2);
        assert_eq!(index.utf16_to_char(4), 3);
    }

    #[test]
    fn every_offset_round_trips_and_matches_the_standard_library() {
        // BMP specials on purpose: a paragraph break, U+2028, an object
        // anchor and an accented letter each take one unit, like ASCII.
        let text = "\u{1F600}\u{1F600}x\n\u{2028}\u{FFFC}\u{E9}\u{1F44D}\u{1F3FD}end\u{10FFFF}";
        let index = Utf16Index::new(text);
        let chars = text.chars().count();
        assert_eq!(index.char_count(), chars);
        assert_eq!(index.utf16_len(), text.encode_utf16().count());
        for c in 0..=chars {
            let u = index.char_to_utf16(c);
            assert_eq!(u, brute_char_to_utf16(text, c), "char {c}");
            assert_eq!(index.utf16_to_char(u), c, "round trip from char {c}");
        }
    }

    #[test]
    fn offsets_past_the_end_clamp() {
        let index = Utf16Index::new("a\u{1F600}");
        assert_eq!(index.char_to_utf16(99), 3);
        assert_eq!(index.utf16_to_char(99), 2);
        assert_eq!(index.char_range_to_utf16(1..99), 1..3);
        assert_eq!(index.utf16_range_to_chars(0..99), 0..2);
        assert_eq!(index.utf16_range_to_chars(50..99), 2..2);
    }

    #[test]
    fn a_range_splitting_a_pair_widens_to_the_whole_character() {
        let index = Utf16Index::new("a\u{1F600}b");
        // Units 2..3 are the emoji's second half and nothing else.
        assert_eq!(index.utf16_range_to_chars(2..3), 1..2);
        // Units 1..2 are its first half.
        assert_eq!(index.utf16_range_to_chars(1..2), 1..2);
        // An empty range between the halves is an insertion point at the
        // emoji's start: replacing it must not delete the emoji.
        assert_eq!(index.utf16_range_to_chars(2..2), 1..1);
        assert_eq!(index.char_range_to_utf16(1..2), 1..3);
    }

    #[test]
    #[expect(
        clippy::reversed_empty_ranges,
        reason = "a platform may hand one over; the conversion must not panic on it"
    )]
    fn a_reversed_range_collapses_at_its_start() {
        let index = Utf16Index::new("abc");
        assert_eq!(index.char_range_to_utf16(2..1), 2..2);
        assert_eq!(index.utf16_range_to_chars(2..1), 2..2);
    }

    #[test]
    fn an_empty_text_answers_zero() {
        let index = Utf16Index::new("");
        assert_eq!(index, Utf16Index::default());
        assert_eq!(index.char_to_utf16(3), 0);
        assert_eq!(index.utf16_to_char(3), 0);
    }
}
