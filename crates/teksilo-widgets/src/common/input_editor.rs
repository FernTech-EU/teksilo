// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The writing half a text surface hands the platform's input system
//! ([`TextInputEditor`]), and the journal and revisions its reading half
//! answers with: the part `RichTextEditor` and `TextInputField` share.
//!
//! Each surface keeps an [`InputSystemState`] beside its document and calls
//! the functions here with its own rules: the filter its typing goes
//! through, and how it inserts recorded text. What is shared is the order of
//! things:
//!
//! - **Outside a composition** a write goes through the surface's rules and
//!   is recorded, logged in the journal as the input system's
//!   ([`ChangeCause::InputSystem`]), and reported to `on_text_inserted`
//!   (which the batch holds back while one is open).
//! - **Inside a composition** (from [`mark`] to [`end`]) a write lands as
//!   given, as a transient edit (`Composition::write`): no undo record, no
//!   filter, so a text service's intermediate states are neither recorded nor
//!   cut. [`end`] puts the final text through the rules and records it as one
//!   undo step (`Composition::commit`).
//!
//! [`TextInputEditor`]: teksilo_core::text_input_document::TextInputEditor
//! [`ChangeCause::InputSystem`]: teksilo_core::text_input_document::ChangeCause::InputSystem

use std::cell::{Cell, RefCell};
use std::ops::Range;

use teksilo_canvas::Rect;
use teksilo_core::text_input_document::{
    CompositionSegment, InputRevisions, Landed, SegmentStyle, WriteOptions, WriteRefused,
};
use teksilo_text::text_document::{MoveMode, TextCursor, TextDocument};

use super::composition::{Composition, PreeditStroke, preedit_strokes};
use super::input_batch::InputBatch;
use super::input_journal::DocumentJournal;

/// What a surface keeps for the platform's input system, beside its
/// document.
#[derive(Debug)]
pub(crate) struct InputSystemState {
    pub(crate) journal: DocumentJournal,
    pub(crate) batch: InputBatch,
    /// The styled stretches of the written composition, for drawing it.
    pub(crate) segments: RefCell<Vec<CompositionSegment>>,
    /// The selection last answered, and its revision.
    selection_seen: Cell<(usize, usize, u64)>,
    /// The caret and visible rectangles last answered, and the layout
    /// revision.
    layout_seen: Cell<([u32; 8], u64)>,
}

impl InputSystemState {
    /// Start keeping `document`'s journal.
    pub(crate) fn watching(document: &TextDocument) -> Self {
        Self {
            journal: DocumentJournal::watching(document),
            batch: InputBatch::default(),
            segments: RefCell::new(Vec::new()),
            selection_seen: Cell::new((0, 0, 0)),
            layout_seen: Cell::new(([0; 8], 0)),
        }
    }

    /// The revisions, given the selection (anchor, caret) and where the caret
    /// and the visible text are drawn now: the selection and layout revisions
    /// grow when what they count differs from what was last answered.
    pub(crate) fn revisions(
        &self,
        anchor: usize,
        caret: usize,
        caret_rect: Option<Rect>,
        visible: Option<Rect>,
    ) -> InputRevisions {
        let (seen_anchor, seen_caret, mut selection) = self.selection_seen.get();
        if (seen_anchor, seen_caret) != (anchor, caret) {
            selection += 1;
            self.selection_seen.set((anchor, caret, selection));
        }
        let bits = rect_bits(caret_rect, visible);
        let (seen_bits, mut layout) = self.layout_seen.get();
        if seen_bits != bits {
            layout += 1;
            self.layout_seen.set((bits, layout));
        }
        InputRevisions {
            text: self.journal.revision(),
            selection,
            layout,
        }
    }
}

/// Two rectangles as bits, for telling a move.
fn rect_bits(a: Option<Rect>, b: Option<Rect>) -> [u32; 8] {
    let mut bits = [0; 8];
    for (slot, rect) in [a, b].into_iter().enumerate() {
        if let Some(rect) = rect {
            bits[slot * 4] = rect.x.to_bits();
            bits[slot * 4 + 1] = rect.y.to_bits();
            bits[slot * 4 + 2] = rect.width.to_bits();
            bits[slot * 4 + 3] = rect.height.to_bits();
        }
    }
    bits
}

/// One underline a composition is drawn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Underline {
    /// The characters it is under.
    pub(crate) range: Range<usize>,
    /// The clause being converted (a target): drawn thicker.
    pub(crate) thick: bool,
    /// One of several clauses: drawn a little short at both ends, so that
    /// the clauses read apart, as Windows draws them.
    pub(crate) inset: bool,
}

/// The underlines the composition at `range` is drawn with: one per segment
/// the text service styled ([`InputSystemState::segments`]), clipped to the
/// composition, in order, a target thicker; the stretches no segment covers,
/// and the whole composition when there is none, as typed input.
pub(crate) fn composition_underlines(
    range: Range<usize>,
    segments: &[CompositionSegment],
) -> Vec<Underline> {
    let mut styled: Vec<(Range<usize>, bool)> = segments
        .iter()
        .map(|segment| {
            let start = segment.range.start.clamp(range.start, range.end);
            let end = segment.range.end.clamp(start, range.end);
            let thick = !matches!(segment.style, SegmentStyle::Input | SegmentStyle::Converted);
            (start..end, thick)
        })
        .filter(|(range, _)| !range.is_empty())
        .collect();
    styled.sort_by_key(|(range, _)| range.start);
    let mut out = Vec::new();
    let mut at = range.start;
    for (segment, thick) in styled {
        if segment.start < at {
            continue;
        }
        if segment.start > at {
            out.push((at..segment.start, false));
        }
        at = segment.end;
        out.push((segment, thick));
    }
    if at < range.end {
        out.push((at..range.end, false));
    }
    let inset = out.len() > 1;
    out.into_iter()
        .map(|(range, thick)| Underline {
            range,
            thick,
            inset,
        })
        .collect()
}

/// The underlines the composition at `range` is drawn with on any input
/// path: the clauses the text service styled when it styled any
/// ([`composition_underlines`]), otherwise the segment the input method's
/// preedit names, `selected`, thick and the rest thin, none of them inset
/// ([`preedit_strokes`]).
pub(crate) fn preedit_underlines(
    range: Range<usize>,
    segments: &[CompositionSegment],
    selected: Option<&Range<usize>>,
) -> Vec<Underline> {
    if !segments.is_empty() {
        return composition_underlines(range, segments);
    }
    preedit_strokes(&range, selected)
        .into_iter()
        .map(|(range, stroke)| Underline {
            range,
            thick: stroke == PreeditStroke::Thick,
            inset: false,
        })
        .collect()
}

/// A text service's line breaks as the document's: `\r\n` and a lone `\r`
/// become `\n`.
pub(crate) fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// What a surface lends a write.
pub(crate) struct Writing<'a> {
    pub(crate) document: &'a TextDocument,
    pub(crate) cursor: &'a TextCursor,
    pub(crate) composition: &'a mut Composition,
    pub(crate) input: &'a InputSystemState,
    /// The document's last offset.
    pub(crate) end: usize,
}

/// [`TextInputEditor::replace_text`]. `rules` is the surface's filter for
/// text outside a composition (what its typing would keep of `text` over
/// `range`); `insert` writes the filtered text over the selection the
/// cursor it is handed holds, records it, and says how many characters
/// arrived.
///
/// [`TextInputEditor::replace_text`]: teksilo_core::text_input_document::TextInputEditor::replace_text
pub(crate) fn replace(
    writing: Writing<'_>,
    range: Range<usize>,
    text: &str,
    options: WriteOptions,
    rules: impl FnOnce(&str, &Range<usize>) -> String,
    insert: impl FnOnce(&TextCursor, &str, bool) -> usize,
) -> Result<Landed, WriteRefused> {
    if range.start > range.end || range.end > writing.end {
        return Err(WriteRefused::InvalidRange);
    }
    let text = normalize_breaks(text);
    if writing.composition.is_written(writing.document) {
        let count = text.chars().count();
        let landed = Landed {
            inserted: range.start..range.start + count,
            replaced: range.clone(),
        };
        if options.dry_run {
            return Ok(landed);
        }
        let Writing {
            document,
            cursor,
            composition,
            input,
            ..
        } = writing;
        let written = input
            .journal
            .input_system_write(|| composition.write(document, cursor, range, &text));
        return if written {
            Ok(landed)
        } else {
            Err(WriteRefused::InvalidRange)
        };
    }
    let kept = rules(&text, &range);
    if options.dry_run {
        return Ok(Landed {
            inserted: range.start..range.start + kept.chars().count(),
            replaced: range,
        });
    }
    writing
        .cursor
        .set_position(range.start, MoveMode::MoveAnchor);
    writing.cursor.set_position(range.end, MoveMode::KeepAnchor);
    let arrived = writing
        .input
        .journal
        .input_system_write(|| insert(writing.cursor, &kept, options.correction));
    Ok(Landed {
        inserted: range.start..range.start + arrived,
        replaced: range,
    })
}

/// [`TextInputEditor::mark_composition`]: the first mark starts a written
/// composition, over `range` when it holds text (a reconversion: that text
/// becomes the composition's, put back if it is cancelled).
///
/// [`TextInputEditor::mark_composition`]: teksilo_core::text_input_document::TextInputEditor::mark_composition
pub(crate) fn mark(
    writing: Writing<'_>,
    range: Range<usize>,
    segments: &[CompositionSegment],
) -> Result<(), WriteRefused> {
    if range.start > range.end || range.end > writing.end {
        return Err(WriteRefused::InvalidRange);
    }
    let Writing {
        document,
        cursor,
        composition,
        input,
        ..
    } = writing;
    if !composition.is_written(document) {
        composition.begin_written(document);
        if !range.is_empty() {
            let text = document
                .text_at(range.start, range.len())
                .map_err(|_| WriteRefused::InvalidRange)?;
            let written = input
                .journal
                .input_system_write(|| composition.write(document, cursor, range, &text));
            if !written {
                return Err(WriteRefused::InvalidRange);
            }
        }
    }
    *input.segments.borrow_mut() = segments.to_vec();
    Ok(())
}

/// [`TextInputEditor::end_composition`]: the written composition's final
/// text through `rules`, recorded as one undo step. How many characters
/// arrived; `None` when no composition was going on.
///
/// [`TextInputEditor::end_composition`]: teksilo_core::text_input_document::TextInputEditor::end_composition
pub(crate) fn end(
    writing: Writing<'_>,
    rules: impl FnOnce(&str) -> String,
    paragraphs: bool,
) -> Option<usize> {
    let Writing {
        document,
        cursor,
        composition,
        input,
        ..
    } = writing;
    let text = composition.written_text(document)?;
    input.segments.borrow_mut().clear();
    let kept = rules(&text);
    Some(
        input
            .journal
            .input_system_write(|| composition.commit(document, cursor, &kept, paragraphs)),
    )
}

#[cfg(test)]
mod tests {
    use teksilo_core::text_input_document::ChangeCause;

    use super::*;

    fn rig(text: &str) -> (TextDocument, TextCursor, Composition, InputSystemState) {
        let document = TextDocument::new();
        document.set_plain_text(text).unwrap();
        let cursor = document.cursor();
        let input = InputSystemState::watching(&document);
        (document, cursor, Composition::default(), input)
    }

    fn writing<'a>(
        document: &'a TextDocument,
        cursor: &'a TextCursor,
        composition: &'a mut Composition,
        input: &'a InputSystemState,
    ) -> Writing<'a> {
        let end = document.character_count();
        Writing {
            document,
            cursor,
            composition,
            input,
            end,
        }
    }

    #[test]
    fn a_composition_is_underlined_by_its_clauses() {
        let segment = |range: Range<usize>, style| CompositionSegment { range, style };
        assert_eq!(
            composition_underlines(2..8, &[]),
            vec![Underline {
                range: 2..8,
                thick: false,
                inset: false
            }],
            "no segment: one thin line"
        );
        let lines = composition_underlines(
            2..8,
            &[
                segment(5..8, SegmentStyle::Converted),
                segment(2..5, SegmentStyle::Target),
                segment(0..1, SegmentStyle::Input),
            ],
        );
        assert_eq!(
            lines,
            vec![
                Underline {
                    range: 2..5,
                    thick: true,
                    inset: true
                },
                Underline {
                    range: 5..8,
                    thick: false,
                    inset: true
                },
            ],
            "in order, the target thick, one outside the composition dropped"
        );
        let gaps = composition_underlines(0..6, &[segment(2..4, SegmentStyle::TargetInput)]);
        assert_eq!(
            gaps.iter()
                .map(|line| (line.range.clone(), line.thick))
                .collect::<Vec<_>>(),
            vec![(0..2, false), (2..4, true), (4..6, false)],
            "what no segment covers is drawn as typed input"
        );
    }

    fn upper(text: &str, _: &Range<usize>) -> String {
        text.to_uppercase()
    }

    #[test]
    fn a_write_outside_a_composition_goes_through_the_rules_and_is_the_input_systems() {
        let (document, cursor, mut composition, input) = rig("abc");
        let before = input.journal.revision();
        let landed = replace(
            writing(&document, &cursor, &mut composition, &input),
            1..2,
            "xy",
            WriteOptions::default(),
            upper,
            |cursor, text, _| {
                cursor.insert_text(text).unwrap();
                text.chars().count()
            },
        )
        .unwrap();
        assert_eq!(
            landed,
            Landed {
                inserted: 1..3,
                replaced: 1..2
            }
        );
        assert_eq!(document.to_plain_text().unwrap(), "aXYc");
        let change = input.journal.changed_since(before).unwrap();
        assert_eq!(change.cause, ChangeCause::InputSystem);
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let (document, cursor, mut composition, input) = rig("abc");
        let landed = replace(
            writing(&document, &cursor, &mut composition, &input),
            0..3,
            "\r\nz",
            WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
            |text, _| text.to_owned(),
            |_, _, _| unreachable!("a dry run inserts nothing"),
        )
        .unwrap();
        assert_eq!(landed.inserted, 0..2, "a \\r\\n is one break");
        assert_eq!(document.to_plain_text().unwrap(), "abc");
        assert_eq!(
            replace(
                writing(&document, &cursor, &mut composition, &input),
                2..9,
                "",
                WriteOptions::default(),
                |text, _| text.to_owned(),
                |_, _, _| 0,
            ),
            Err(WriteRefused::InvalidRange)
        );
    }

    #[test]
    fn a_composition_lands_as_written_and_ends_under_the_rules_as_one_step() {
        let (document, cursor, mut composition, input) = rig("ab");
        mark(
            writing(&document, &cursor, &mut composition, &input),
            2..2,
            &[],
        )
        .unwrap();
        for (range, text) in [(2..2, "n"), (2..3, "に"), (3..3, "ほ")] {
            replace(
                writing(&document, &cursor, &mut composition, &input),
                range,
                text,
                WriteOptions::default(),
                |_, _| unreachable!("no rules inside a composition"),
                |_, _, _| unreachable!("no recorded insert inside a composition"),
            )
            .unwrap();
        }
        assert_eq!(document.to_plain_text().unwrap(), "abにほ");
        let arrived = end(
            writing(&document, &cursor, &mut composition, &input),
            |text| text.chars().take(1).collect(),
            false,
        );
        assert_eq!(arrived, Some(1), "the rules apply to the final text only");
        assert_eq!(document.to_plain_text().unwrap(), "abに");
        document.undo().unwrap();
        assert_eq!(document.to_plain_text().unwrap(), "ab", "one step");
        assert_eq!(
            end(
                writing(&document, &cursor, &mut composition, &input),
                |text| text.to_owned(),
                false
            ),
            None,
            "no composition left"
        );
    }

    #[test]
    fn a_mark_over_recorded_text_makes_it_the_composition() {
        let (document, cursor, mut composition, input) = rig("ab日本");
        mark(
            writing(&document, &cursor, &mut composition, &input),
            2..4,
            &[],
        )
        .unwrap();
        assert_eq!(composition.range(&document), Some(2..4));
        replace(
            writing(&document, &cursor, &mut composition, &input),
            2..4,
            "二本",
            WriteOptions::default(),
            |text, _| text.to_owned(),
            |_, _, _| 0,
        )
        .unwrap();
        assert!(composition.cancel(&document));
        assert_eq!(document.to_plain_text().unwrap(), "ab日本");
    }

    #[test]
    fn revisions_grow_with_what_they_count() {
        let (document, _cursor, _composition, input) = rig("ab");
        let first = input.revisions(0, 0, None, None);
        assert_eq!(input.revisions(0, 0, None, None), first, "nothing moved");
        let moved = input.revisions(0, 1, None, None);
        assert_eq!(moved.selection, first.selection + 1);
        let laid = input.revisions(0, 1, Some(Rect::new(1.0, 2.0, 3.0, 4.0)), None);
        assert_eq!(laid.layout, first.layout + 1);
        document.cursor_at(2).insert_text("c").unwrap();
        assert!(
            input
                .revisions(0, 1, Some(Rect::new(1.0, 2.0, 3.0, 4.0)), None)
                .text
                > first.text
        );
    }
}
