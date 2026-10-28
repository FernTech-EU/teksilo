// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a window's native view answers the platform's text input system,
//! from the focused text widget's [`TextInputDocument`]: the half that needs
//! no platform.
//!
//! The document counts in characters and places things in window-logical
//! coordinates. The platform counts in UTF-16 code units, with a "not found"
//! location for "no range" (macOS's `NSNotFound`), and in its own screen
//! space. This module does the counting: ranges in and out, the conversions
//! through the document's
//! [`Utf16Index`](teksilo_core::text_input_document::Utf16Index), the
//! clamping, and the walk over a
//! selection's lines. The coordinates it hands back are still window-logical,
//! and the platform backend converts them to and from its screen space with
//! the platform's own calls, as winit converts its IME cursor area.
//!
//! Every function answers `None` where the document could not answer (busy,
//! no layout yet): the caller then answers as it would without a document.

#![cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only the macOS view answers from the document, for now"
    )
)]

use std::ops::Range;

use teksilo_canvas::{Point, Rect};
use teksilo_core::text_input_document::TextInputDocument;

/// The "no range" location: macOS's `NSNotFound`, an `NSInteger`'s largest
/// value. Any location from it up reads as none.
pub(crate) const NOT_FOUND: usize = isize::MAX as usize;

/// A range as the platform counts it: a location and a length in UTF-16 code
/// units, the location [`NOT_FOUND`] for none. The shape of an `NSRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utf16Range {
    pub(crate) location: usize,
    pub(crate) length: usize,
}

impl Utf16Range {
    /// No range.
    pub(crate) const NONE: Self = Self {
        location: NOT_FOUND,
        length: 0,
    };

    /// `range` as a location and a length; an end before the start is an
    /// empty range at the start.
    pub(crate) fn from_range(range: Range<usize>) -> Self {
        Self {
            location: range.start,
            length: range.end.saturating_sub(range.start),
        }
    }

    /// The range, `None` for none. An end past `usize::MAX` stops there.
    pub(crate) fn to_range(self) -> Option<Range<usize>> {
        (self.location < NOT_FOUND)
            .then(|| self.location..self.location.saturating_add(self.length))
    }
}

/// `writingToolsBehavior`'s `NSWritingToolsBehaviorNone`: Writing Tools
/// leaves the view alone.
pub(crate) const WRITING_TOOLS_NONE: isize = -1;
/// `writingToolsBehavior`'s `NSWritingToolsBehaviorLimited`: Writing Tools
/// works in its panel and replaces the text from there, never inline.
pub(crate) const WRITING_TOOLS_LIMITED: isize = 2;
/// `allowedWritingToolsResultOptions`' `NSWritingToolsResultPlainText`: the
/// text Writing Tools hands back carries no formatting.
pub(crate) const WRITING_TOOLS_PLAIN_TEXT: usize = 1;

/// The most lines [`union_rect_in_visible_selection`] walks. Far more than a
/// window shows: the walk starts at the first visible line.
pub(crate) const MAX_UNION_LINES: usize = 1024;

/// `selectedRange`: the selection, in UTF-16.
pub(crate) fn selected_range(document: &dyn TextInputDocument) -> Option<Utf16Range> {
    let index = document.utf16_index()?;
    let selection = document.selection()?;
    Some(Utf16Range::from_range(index.char_range_to_utf16(selection)))
}

/// `markedRange`: the composition, in UTF-16, while the platform's own view
/// holds marked text (`view_has_marked_text`); none otherwise. `None` too
/// when the view holds marked text the document has no composition for, so
/// the view's own answer stands.
pub(crate) fn marked_range(
    document: &dyn TextInputDocument,
    view_has_marked_text: bool,
) -> Option<Utf16Range> {
    if !view_has_marked_text {
        return Some(Utf16Range::NONE);
    }
    let index = document.utf16_index()?;
    let composition = document.composition_range()?;
    Some(Utf16Range::from_range(
        index.char_range_to_utf16(composition),
    ))
}

/// The answer to `attributedSubstringForProposedRange:actualRange:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Substring {
    /// No text: a secure field, or no range asked.
    Nil,
    /// `text`, which is the document's `actual` range: the proposed one,
    /// clamped to the document and widened to whole characters.
    Text { text: String, actual: Utf16Range },
}

/// `attributedSubstringForProposedRange:actualRange:`.
pub(crate) fn substring(
    document: &dyn TextInputDocument,
    proposed: Utf16Range,
) -> Option<Substring> {
    if document.is_secure() {
        return Some(Substring::Nil);
    }
    let Some(proposed) = proposed.to_range() else {
        return Some(Substring::Nil);
    };
    let index = document.utf16_index()?;
    let chars = index.utf16_range_to_chars(proposed);
    let text = document.text_in(chars.clone())?;
    Some(Substring::Text {
        text,
        actual: Utf16Range::from_range(index.char_range_to_utf16(chars)),
    })
}

/// `characterIndexForPoint:`, for a point already in window-logical
/// coordinates: the UTF-16 offset of the character there, [`NOT_FOUND`]
/// where the document has none (outside its text, a field that shows
/// nothing).
pub(crate) fn character_index(
    document: &dyn TextInputDocument,
    window_point: Point,
) -> Option<usize> {
    let index = document.utf16_index()?;
    Some(
        document
            .offset_at_point(window_point)
            .map_or(NOT_FOUND, |offset| index.char_to_utf16(offset)),
    )
}

/// `firstRectForCharacterRange:actualRange:`: where the first line of
/// `range` is drawn, window-logical, and the UTF-16 range of the characters
/// that line holds. No range asks for the selection's, which is where a
/// panel the input system opens (Emoji & Symbols) is anchored.
pub(crate) fn first_rect(
    document: &dyn TextInputDocument,
    range: Utf16Range,
) -> Option<(Rect, Utf16Range)> {
    let index = document.utf16_index()?;
    let chars = match range.to_range() {
        Some(range) => index.utf16_range_to_chars(range),
        None => document.selection()?,
    };
    let line = document.first_line_rect(chars)?;
    Some((
        line.rect,
        Utf16Range::from_range(index.char_range_to_utf16(line.covered)),
    ))
}

/// The character range a UTF-16 range the input system named stands for,
/// for an [`ImeCommitPayload`](super::ImeCommitPayload)'s replacement.
pub(crate) fn replacement_chars(
    document: &dyn TextInputDocument,
    range: Utf16Range,
) -> Option<Range<usize>> {
    let range = range.to_range()?;
    Some(document.utf16_index()?.utf16_range_to_chars(range))
}

/// `documentVisibleRect`: the part of the text that is on screen,
/// window-logical.
pub(crate) fn visible_rect(document: &dyn TextInputDocument) -> Option<Rect> {
    document.visible_text_rect()
}

/// `unionRectInVisibleSelectedRange`: the union of the selection's lines,
/// each clipped to the visible text, window-logical; the caret's rectangle,
/// clipped, for an empty selection. `None` when none of it is visible.
///
/// The walk starts at the first visible line when the selection begins
/// above it (the lower of the offsets at the visible text's two top corners,
/// so a right-to-left line is taken whole), stops at the first line below
/// the visible text, and takes at most [`MAX_UNION_LINES`] lines.
pub(crate) fn union_rect_in_visible_selection(document: &dyn TextInputDocument) -> Option<Rect> {
    let visible = document.visible_text_rect()?;
    let selection = document.selection()?;
    if selection.is_empty() {
        let caret = document.first_line_rect(selection)?.rect;
        return intersection(caret, visible);
    }
    let first_visible = [visible.x + 0.5, visible.right() - 0.5]
        .into_iter()
        .filter_map(|x| document.offset_at_point(Point::new(x, visible.y + 0.5)))
        .min();
    let mut start = match first_visible {
        Some(offset) if offset > selection.start && offset < selection.end => offset,
        _ => selection.start,
    };
    let mut union: Option<Rect> = None;
    for _ in 0..MAX_UNION_LINES {
        if start >= selection.end {
            break;
        }
        let Some(line) = document.first_line_rect(start..selection.end) else {
            break;
        };
        if line.rect.y > visible.bottom() {
            break;
        }
        if let Some(clipped) = intersection(line.rect, visible) {
            union = Some(union.map_or(clipped, |union| bounding(union, clipped)));
        }
        if line.covered.end <= start {
            break;
        }
        start = line.covered.end;
    }
    union
}

/// `isEditable`: whether the focused widget takes text. `false` with no
/// document.
pub(crate) fn is_editable(document: Option<&dyn TextInputDocument>) -> bool {
    document.is_some_and(TextInputDocument::is_editable)
}

/// `writingToolsBehavior`: [`WRITING_TOOLS_LIMITED`] where the focused
/// widget lets Writing Tools at its text, [`WRITING_TOOLS_NONE`] otherwise
/// and with no document.
pub(crate) fn writing_tools_behavior(document: Option<&dyn TextInputDocument>) -> isize {
    if document.is_some_and(TextInputDocument::writing_tools_allowed) {
        WRITING_TOOLS_LIMITED
    } else {
        WRITING_TOOLS_NONE
    }
}

/// Where `a` and `b` overlap, edges included, so a caret one point wide on
/// the visible text's edge stays; `None` when they do not touch.
pub(crate) fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    (right >= left && bottom >= top).then(|| Rect::new(left, top, right - left, bottom - top))
}

/// The smallest rectangle holding `a` and `b`.
pub(crate) fn bounding(a: Rect, b: Rect) -> Rect {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    Rect::new(left, top, right - left, bottom - top)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use teksilo_core::text_input_document::{FirstLineRect, Utf16Index};

    use super::*;

    /// A document laid out in a fixed grid: every character 10 wide, lines
    /// of `per_line` characters 20 tall, the text scrolled up by `scroll_y`
    /// in a visible area at the window's origin, `width` by `height`.
    struct Grid {
        text: String,
        selection: Range<usize>,
        composition: Option<Range<usize>>,
        per_line: usize,
        scroll_y: f32,
        width: f32,
        height: f32,
        secure: bool,
        editable: bool,
        writing_tools: bool,
        busy: bool,
        lines_asked: Cell<usize>,
    }

    impl Grid {
        fn new(text: &str) -> Self {
            Self {
                text: text.to_owned(),
                selection: 0..0,
                composition: None,
                per_line: 10,
                scroll_y: 0.0,
                width: 100.0,
                height: 60.0,
                secure: false,
                editable: true,
                writing_tools: true,
                busy: false,
                lines_asked: Cell::new(0),
            }
        }

        fn len(&self) -> usize {
            self.text.chars().count()
        }

        fn place(&self, offset: usize) -> (f32, f32) {
            let line = offset / self.per_line;
            let column = offset % self.per_line;
            (column as f32 * 10.0, line as f32 * 20.0 - self.scroll_y)
        }
    }

    impl TextInputDocument for Grid {
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
            self.lines_asked.set(self.lines_asked.get() + 1);
            let end = range.end.min(self.len());
            let start = range.start.min(end);
            let (x, y) = self.place(start);
            if start == end {
                return Some(FirstLineRect {
                    rect: Rect::new(x, y, 1.0, 20.0),
                    covered: start..start,
                });
            }
            let line_end = (start / self.per_line + 1) * self.per_line;
            let covered_end = end.min(line_end);
            let width = (covered_end - start) as f32 * 10.0;
            Some(FirstLineRect {
                rect: Rect::new(x, y, width, 20.0),
                covered: start..covered_end,
            })
        }
        fn offset_at_point(&self, point: Point) -> Option<usize> {
            if self.busy || !self.visible_text_rect()?.contains(point) {
                return None;
            }
            let line = ((point.y + self.scroll_y) / 20.0).floor() as usize;
            let column = ((point.x / 10.0).round() as usize).min(self.per_line);
            Some((line * self.per_line + column).min(self.len()))
        }
        fn visible_text_rect(&self) -> Option<Rect> {
            (!self.busy).then(|| Rect::new(0.0, 0.0, self.width, self.height))
        }
        fn is_editable(&self) -> bool {
            !self.busy && self.editable
        }
        fn is_secure(&self) -> bool {
            self.busy || self.secure
        }
        fn writing_tools_allowed(&self) -> bool {
            !self.busy && !self.secure && self.writing_tools
        }
        fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
            (!self.busy).then(|| Rc::new(Utf16Index::new(&self.text)))
        }
    }

    /// "a😀b😀c": characters 0..5, UTF-16 units 0..7.
    const EMOJI: &str = "a\u{1F600}b\u{1F600}c";

    fn range(location: usize, length: usize) -> Utf16Range {
        Utf16Range { location, length }
    }

    #[test]
    fn a_range_converts_both_ways_and_none_is_none() {
        assert_eq!(Utf16Range::from_range(2..5), range(2, 3));
        #[expect(
            clippy::reversed_empty_ranges,
            reason = "a reversed range must collapse, not underflow"
        )]
        let reversed = 5..2;
        assert_eq!(Utf16Range::from_range(reversed), range(5, 0));
        assert_eq!(range(2, 3).to_range(), Some(2..5));
        assert_eq!(Utf16Range::NONE.to_range(), None);
        assert_eq!(range(NOT_FOUND, 4).to_range(), None);
        assert_eq!(range(usize::MAX, 0).to_range(), None);
        assert_eq!(
            range(1, usize::MAX).to_range(),
            Some(1..usize::MAX),
            "the end stops at the top"
        );
    }

    #[test]
    fn the_selection_is_answered_in_utf16() {
        let mut document = Grid::new(EMOJI);
        document.selection = 2..4;
        assert_eq!(selected_range(&document), Some(range(3, 3)));
        document.selection = 5..5;
        assert_eq!(selected_range(&document), Some(range(7, 0)));
        document.busy = true;
        assert_eq!(selected_range(&document), None, "busy: the view answers");
    }

    #[test]
    fn the_marked_range_follows_the_views_own_marked_text() {
        let mut document = Grid::new(EMOJI);
        document.composition = Some(1..3);
        assert_eq!(marked_range(&document, true), Some(range(1, 3)));
        assert_eq!(
            marked_range(&document, false),
            Some(Utf16Range::NONE),
            "no marked text in the view: none, whatever the document says"
        );
        document.composition = None;
        assert_eq!(
            marked_range(&document, true),
            None,
            "marked text the document does not know: the view answers"
        );
    }

    #[test]
    fn a_substring_is_widened_to_whole_characters_and_clamped() {
        let document = Grid::new(EMOJI);
        // Units 2..4 are the first emoji's second half and "b".
        assert_eq!(
            substring(&document, range(2, 2)),
            Some(Substring::Text {
                text: "\u{1F600}b".to_owned(),
                actual: range(1, 3),
            })
        );
        assert_eq!(
            substring(&document, range(5, 100)),
            Some(Substring::Text {
                text: "\u{1F600}c".to_owned(),
                actual: range(4, 3),
            })
        );
        assert_eq!(
            substring(&document, range(50, 3)),
            Some(Substring::Text {
                text: String::new(),
                actual: range(7, 0),
            })
        );
        assert_eq!(substring(&document, Utf16Range::NONE), Some(Substring::Nil));
    }

    #[test]
    fn a_secure_field_gives_no_text() {
        let mut document = Grid::new("secret");
        document.secure = true;
        assert_eq!(substring(&document, range(0, 6)), Some(Substring::Nil));
    }

    #[test]
    fn the_character_under_a_point_is_a_utf16_offset_or_not_found() {
        let document = Grid::new(EMOJI);
        // Column 3 is character 3, the second emoji, UTF-16 offset 4.
        assert_eq!(character_index(&document, Point::new(30.0, 5.0)), Some(4));
        assert_eq!(
            character_index(&document, Point::new(500.0, 5.0)),
            Some(NOT_FOUND),
            "outside the text"
        );
    }

    #[test]
    fn the_first_rect_covers_the_first_line_and_says_how_far() {
        let mut document = Grid::new("0123456789abcdefghij");
        assert_eq!(
            first_rect(&document, range(8, 6)),
            Some((Rect::new(80.0, 0.0, 20.0, 20.0), range(8, 2)))
        );
        document.selection = 12..15;
        assert_eq!(
            first_rect(&document, Utf16Range::NONE),
            Some((Rect::new(20.0, 20.0, 30.0, 20.0), range(12, 3))),
            "no range: the selection's"
        );
    }

    #[test]
    fn a_replacement_range_becomes_characters() {
        let document = Grid::new(EMOJI);
        assert_eq!(replacement_chars(&document, range(3, 3)), Some(2..4));
        assert_eq!(replacement_chars(&document, Utf16Range::NONE), None);
    }

    #[test]
    fn the_union_of_a_selection_is_clipped_to_the_visible_text() {
        // Three lines of ten, the visible text two lines and a half tall.
        let mut document = Grid::new(&"x".repeat(30));
        document.height = 50.0;
        document.selection = 5..25;
        assert_eq!(
            union_rect_in_visible_selection(&document),
            Some(Rect::new(0.0, 0.0, 100.0, 50.0)),
            "lines 0 (from column 5), 1 and 2 (to column 5), clipped at the bottom"
        );
        document.selection = 3..6;
        assert_eq!(
            union_rect_in_visible_selection(&document),
            Some(Rect::new(30.0, 0.0, 30.0, 20.0))
        );
    }

    #[test]
    fn an_empty_selection_answers_the_caret() {
        let mut document = Grid::new("abc");
        document.selection = 2..2;
        assert_eq!(
            union_rect_in_visible_selection(&document),
            Some(Rect::new(20.0, 0.0, 1.0, 20.0))
        );
        document.scroll_y = 200.0;
        assert_eq!(
            union_rect_in_visible_selection(&document),
            None,
            "a caret scrolled out of view is not visible"
        );
    }

    #[test]
    fn a_long_selection_is_walked_from_the_first_visible_line_only() {
        // 1000 lines, scrolled to line 900: three lines visible.
        let mut document = Grid::new(&"x".repeat(10_000));
        document.scroll_y = 900.0 * 20.0;
        document.selection = 0..10_000;
        assert_eq!(
            union_rect_in_visible_selection(&document),
            Some(Rect::new(0.0, 0.0, 100.0, 60.0))
        );
        assert!(
            document.lines_asked.get() <= 5,
            "asked {} lines, not the 900 above the view",
            document.lines_asked.get()
        );
    }

    #[test]
    fn editable_and_writing_tools_need_a_document_that_allows_them() {
        let mut document = Grid::new("abc");
        assert!(is_editable(Some(&document)));
        assert!(!is_editable(None));
        assert_eq!(
            writing_tools_behavior(Some(&document)),
            WRITING_TOOLS_LIMITED
        );
        assert_eq!(writing_tools_behavior(None), WRITING_TOOLS_NONE);
        document.writing_tools = false;
        assert_eq!(writing_tools_behavior(Some(&document)), WRITING_TOOLS_NONE);
        document.writing_tools = true;
        document.secure = true;
        assert_eq!(writing_tools_behavior(Some(&document)), WRITING_TOOLS_NONE);
        document.editable = false;
        assert!(!is_editable(Some(&document)));
    }

    #[test]
    fn a_busy_document_answers_nothing() {
        let mut document = Grid::new(EMOJI);
        document.busy = true;
        assert!(document.utf16_index().is_none());
        assert_eq!(selected_range(&document), None);
        assert_eq!(marked_range(&document, true), None);
        assert_eq!(
            substring(&document, range(0, 2)),
            Some(Substring::Nil),
            "busy reads as secure"
        );
        assert_eq!(character_index(&document, Point::new(5.0, 5.0)), None);
        assert_eq!(first_rect(&document, range(0, 1)), None);
        assert_eq!(union_rect_in_visible_selection(&document), None);
        assert!(!is_editable(Some(&document)));
        assert_eq!(writing_tools_behavior(Some(&document)), WRITING_TOOLS_NONE);
    }

    #[test]
    fn rectangles_meet_and_join() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 5.0, 10.0, 10.0);
        assert_eq!(
            intersection(a, b),
            Some(Rect::new(10.0, 5.0, 0.0, 5.0)),
            "an edge counts"
        );
        assert_eq!(intersection(a, Rect::new(11.0, 0.0, 1.0, 1.0)), None);
        assert_eq!(bounding(a, b), Rect::new(0.0, 0.0, 20.0, 15.0));
    }
}
