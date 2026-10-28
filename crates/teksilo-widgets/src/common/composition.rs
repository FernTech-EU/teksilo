// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An input method's composition, shown in a text surface's document as
//! transient edits.
//!
//! The composition (the candidates of a Chinese or Japanese input method, the
//! Korean syllable being built) is shown in the text, where the writer is
//! looking, and replaced at every update. Each update is a transient edit
//! (`TextCursor::insert_text_transient`): it changes the document and is
//! announced as any edit is, so the surface lays it out, draws it and reads it
//! to assistive technology, but no undo history records it. Once the
//! composition is committed, an undo takes back the committed text and nothing
//! of the states it went through; a cancel is a revert, which puts back what
//! the composition took the place of exactly, formats included.
//!
//! # The document holds the composition
//!
//! The document owns its transient edits, and any recorded write to it (an
//! edit through any cursor, an undo, a redo, a format, a load, the application
//! writing the text) reverts them first. A surface therefore never keeps its
//! own record of where its composition is: [`Composition::range`] asks the
//! document each time. What a surface does keep (the text it showed, whether
//! the input method has ended the composition, and which layer of transient
//! edits the composition is in, `TextDocument::transient_layer`) holds only
//! while the document still names that layer. So it never claims a composition
//! a write has already taken away, nor a layer someone else made since, and
//! [`Composition::reconcile`] forgets it.
//!
//! # The empty update
//!
//! winit sends a composition's empty update right before every commit, on
//! every platform, as well as when the input method cancels it. The empty
//! update alone decides nothing: [`Composition::end`] leaves the text where it
//! is, and the next event says which it was. A commit commits it; anything
//! else (a key, another composition, a press, a focus change, the next frame)
//! means the composition was cancelled.
//!
//! Shared by `RichTextEditor`, `TextInputField` and `CodeEditor`, which each
//! apply their own rules (filters, a length cap, a forward-only mode) to the
//! text before handing it here.
//!
//! # The selected segment
//!
//! The input method also says which part of the composition it is working on
//! (`Ime::Preedit`'s cursor range, byte offsets into the update's text). A
//! range with an extent is the segment being converted (a Mozc clause, a
//! highlighted Pinyin syllable run): native toolkits draw it with a thick
//! underline and the rest of the composition with a thin one, and so does
//! [`preedit_strokes`]. A range with no extent is the caret's place inside the
//! composition, and [`Composition::place_caret`] puts the caret there.
//! [`Composition::mark`] keeps both, as character offsets into what the
//! surface actually showed (its filters may have dropped characters).
//!
//! # Whose composition a view shows
//!
//! The transient edits are the document's, and every view of a shared document
//! hears them. A view shows them only while they are its own composition
//! ([`Composition::is_held`]); every other view of the document, editable or
//! not, lays out the recorded document, as a revert would leave it
//! ([`shows_transient_edits`]). The flavour flips as a composition starts or
//! ends in the view, and [`flip_relayout`] says what the flip has to relay out.

use std::ops::Range;

use teksilo_text::text_document::{
    InsertionOrigin, MoveMode, TextCursor, TextDocument, TransientLayerId,
};

/// Whether a view with `composition` lays out `doc`'s transient edits: while
/// they are its own composition, or always when the host asked for it
/// (`host_override`, for transient edits a host makes itself through the API).
/// Otherwise it lays out the recorded document. See the
/// [module documentation](self#whose-composition-a-view-shows).
pub(crate) fn shows_transient_edits(
    composition: &Composition,
    doc: &TextDocument,
    host_override: bool,
) -> bool {
    host_override || composition.is_held(doc)
}

/// What a view's layout has to relay out when the flavour it shows flips (see
/// [`shows_transient_edits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlipRelayout {
    /// The one paragraph the transient edits are in, at this position, the
    /// same in the document as it is and as recorded.
    Block(usize),
    /// The whole text: the transient edits span paragraphs.
    Full,
}

/// What a flip of the flavour a view shows has to relay out in `doc`: nothing
/// when no transient edits are outstanding, since both flavours then read the
/// same document. A composition that starts in the view flips it with its own
/// first update, whose events relay its paragraph out anyway: the flip adds that
/// one paragraph, once, and nothing per update after it.
pub(crate) fn flip_relayout(doc: &TextDocument) -> Option<FlipRelayout> {
    let range = doc.transient_range()?;
    let Some(shown) = doc.block_at_position(range.start).map(|block| block.id()) else {
        return Some(FlipRelayout::Full);
    };
    let shown_end = doc
        .block_at_position(range.end)
        .map(|block| block.id())
        .filter(|end| *end == shown);
    let recorded = doc.snapshot_recorded_block_at_position_masked(
        range.start,
        &teksilo_text::text_document::HighlightMask::none(),
    );
    let one_paragraph = shown_end.is_some()
        && recorded.is_some_and(|block| {
            block.block_id == shown
                && block.position + block.length >= doc.recorded_position(range.end)
        });
    Some(if one_paragraph {
        FlipRelayout::Block(range.start)
    } else {
        FlipRelayout::Full
    })
}

/// Where `at`, a position outside the composition at `range` (in the document
/// as it stands, the composition in it), is once the composition is committed
/// as `committed` characters: one after it moves with its end. The input
/// method's commit can be longer or shorter than what the composition showed
/// (Microsoft Pinyin's "nihao" for "ni'hao").
pub(crate) fn position_after_commit(at: usize, range: &Range<usize>, committed: usize) -> usize {
    if at > range.end {
        at - range.len() + committed
    } else {
        at
    }
}

/// How a stretch of a composition is underlined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreeditStroke {
    /// The composition's text: tentative, still being typed.
    Thin,
    /// The segment the input method is converting.
    Thick,
}

/// The underlines a composition at `range` is drawn with, in order, as
/// document ranges: all of it thin, except the `segment` (clamped into
/// `range`), which is thick. Never empty for a non-empty `range`; the pieces
/// are contiguous and cover `range` exactly.
pub(crate) fn preedit_strokes(
    range: &Range<usize>,
    segment: Option<&Range<usize>>,
) -> Vec<(Range<usize>, PreeditStroke)> {
    if range.is_empty() {
        return Vec::new();
    }
    let segment = segment
        .map(|seg| seg.start.max(range.start)..seg.end.min(range.end))
        .filter(|seg| seg.start < seg.end);
    let Some(segment) = segment else {
        return vec![(range.clone(), PreeditStroke::Thin)];
    };
    let mut strokes = Vec::with_capacity(3);
    if range.start < segment.start {
        strokes.push((range.start..segment.start, PreeditStroke::Thin));
    }
    strokes.push((segment.clone(), PreeditStroke::Thick));
    if segment.end < range.end {
        strokes.push((segment.end..range.end, PreeditStroke::Thin));
    }
    strokes
}

/// A text surface's composition. See the [module documentation](self).
#[derive(Debug, Default)]
pub(crate) struct Composition {
    /// The segment the input method is converting, in characters of the
    /// composition's text. See [`mark`](Self::mark).
    segment: Option<Range<usize>>,
    /// The caret's place inside the composition, in characters of its text.
    caret: Option<usize>,
    /// The text the composition shows, as the surface inserted it: `Some`
    /// while it is live.
    shown: Option<String>,
    /// The input method sent the composition's empty update: its text is still
    /// in the document, and the next event decides whether it is committed or
    /// cancelled.
    ended: bool,
    /// The layer of transient edits the composition is shown in, which is its
    /// own while the document still names it (`TextDocument::transient_layer`).
    /// The document keeps one layer whoever makes it: a recorded write that
    /// reverts this composition, then transient edits someone else makes, leave
    /// a layer outstanding that is not this one.
    layer: Option<TransientLayerId>,
}

impl Composition {
    /// Whether the composition is live in `doc`: shown, and the document still
    /// holds it.
    pub(crate) fn is_live(&self, doc: &TextDocument) -> bool {
        self.shown.is_some() && self.owns_layer(doc)
    }

    /// Whether the layer `doc` holds is the one this composition is shown in.
    fn owns_layer(&self, doc: &TextDocument) -> bool {
        self.layer.is_some() && doc.transient_layer() == self.layer
    }

    /// Whether the input method ended the composition with its empty update
    /// and its text is still in `doc`, waiting for the commit or the cancel.
    pub(crate) fn is_ended(&self, doc: &TextDocument) -> bool {
        self.ended && self.owns_layer(doc)
    }

    /// Whether `doc` holds this surface's composition, live or ended.
    pub(crate) fn is_held(&self, doc: &TextDocument) -> bool {
        (self.shown.is_some() || self.ended) && self.owns_layer(doc)
    }

    /// The text the live composition shows.
    pub(crate) fn text(&self, doc: &TextDocument) -> Option<&str> {
        self.shown.as_deref().filter(|_| self.owns_layer(doc))
    }

    /// Where the live composition is in `doc`, in its character offsets. `None`
    /// while there is none, and once it is ended: an ended composition is not
    /// one the writer is still making.
    pub(crate) fn range(&self, doc: &TextDocument) -> Option<Range<usize>> {
        if !self.is_live(doc) {
            return None;
        }
        doc.transient_range().filter(|range| !range.is_empty())
    }

    /// Forget a composition `doc` no longer holds: a recorded write reverted it,
    /// and transient edits outstanding now, if any, are another layer. Returns
    /// whether there was one to forget.
    pub(crate) fn reconcile(&mut self, doc: &TextDocument) -> bool {
        let claimed = self.shown.is_some() || self.ended;
        if claimed && !self.owns_layer(doc) {
            self.forget();
            true
        } else {
            false
        }
    }

    /// Make way for a new composition. Transient edits `doc` holds then are not
    /// this surface's (another view's of the same document, the application's),
    /// and the document keeps one layer of them, which a commit replaces whole:
    /// they are reverted, before the caller places the new composition.
    pub(crate) fn begin(&mut self, doc: &TextDocument) {
        self.forget();
        doc.revert_transient_edits();
    }

    /// Show `text` as the composition: over the live composition's text when
    /// there is one, else at `cursor`, over its selection (the text the
    /// composition takes the place of, which goes back if it is cancelled).
    /// The cursor is left after it. Returns whether it is shown; when the
    /// document refuses it, the composition is cancelled.
    pub(crate) fn show(&mut self, doc: &TextDocument, cursor: &TextCursor, text: &str) -> bool {
        if self.is_live(doc)
            && let Some(range) = doc.transient_range()
        {
            cursor.set_position(range.start, MoveMode::MoveAnchor);
            cursor.set_position(range.end, MoveMode::KeepAnchor);
        }
        if cursor.insert_text_transient(text).is_ok() {
            self.shown = Some(text.to_owned());
            self.ended = false;
            self.segment = None;
            self.caret = None;
            // The layer the first update began, or joined: an empty first update
            // changes nothing and begins none, and the next one notes it.
            self.layer = doc.transient_layer();
            true
        } else {
            self.cancel(doc);
            false
        }
    }

    /// Note which part of the composition just [`show`](Self::show)n the input
    /// method is on: `cursor` is its range, byte offsets into `update`, the
    /// text of the update as it came, and `kept` says, for each character of
    /// `update`, whether the surface kept it (its filters drop the rest). An
    /// extent is the segment being converted, no extent the caret's place;
    /// `None` (the input method hides its cursor) leaves the caret after the
    /// composition and none of it selected.
    pub(crate) fn mark(&mut self, update: &str, cursor: Option<&Range<usize>>, kept: &[bool]) {
        self.segment = None;
        self.caret = None;
        let (Some(cursor), Some(shown)) = (cursor, self.shown.as_deref()) else {
            return;
        };
        let shown_chars = shown.chars().count();
        let kept_before = |byte: usize| {
            update
                .char_indices()
                .take_while(|&(at, _)| at < byte)
                .zip(kept)
                .filter(|&(_, &kept)| kept)
                .count()
                .min(shown_chars)
        };
        let (a, b) = (kept_before(cursor.start), kept_before(cursor.end));
        let (begin, end) = (a.min(b), a.max(b));
        if begin == end {
            self.caret = Some(begin);
        } else {
            self.segment = Some(begin..end);
        }
    }

    /// The segment being converted, as a range of `doc`, while the composition
    /// is live.
    pub(crate) fn segment(&self, doc: &TextDocument) -> Option<Range<usize>> {
        let range = self.range(doc)?;
        let segment = self.segment.as_ref()?;
        let start = (range.start + segment.start).min(range.end);
        let end = (range.start + segment.end).min(range.end);
        (start < end).then_some(start..end)
    }

    /// Put `cursor` where the input method says the caret is inside the live
    /// composition, when it says. Otherwise the caret stays after it, where
    /// [`show`](Self::show) left it.
    pub(crate) fn place_caret(&self, doc: &TextDocument, cursor: &TextCursor) {
        if let (Some(at), Some(range)) = (self.caret, self.range(doc)) {
            cursor.set_position((range.start + at).min(range.end), MoveMode::MoveAnchor);
        }
    }

    /// The input method's empty update: the live composition ends, its text
    /// left in the document until the next event says whether it is committed
    /// or cancelled. Returns whether there was a live one.
    pub(crate) fn end(&mut self, doc: &TextDocument) -> bool {
        if !self.is_live(doc) {
            return false;
        }
        self.shown = None;
        self.ended = true;
        true
    }

    /// Cancel the composition: the document back to its recorded content,
    /// recording nothing, the text the composition took the place of back
    /// with its formats. Returns whether there was one in the document.
    pub(crate) fn cancel(&mut self, doc: &TextDocument) -> bool {
        let held = self.is_held(doc);
        self.forget();
        held && doc.revert_transient_edits()
    }

    /// Commit the composition as `text`, the cursor left after it. Returns how
    /// many characters arrived. Nothing is committed when `doc` holds no
    /// composition of this surface's; an empty `text` commits nothing and ends
    /// the composition as a cancel.
    ///
    /// The commit is **one undo step of its own**, closed on both sides
    /// (`TextCursor::commit_transient_text`): an undo takes back exactly the
    /// committed text, and gives back the text the composition took the place
    /// of. This is the one place that makes that choice; for a commit that
    /// coalesces with the typing around it instead, cancel and insert the text
    /// as typing does.
    ///
    /// With `paragraphs`, a paragraph separator (`U+2029`) in `text` ends its
    /// paragraph, as it does in typed text: the separators go in with the
    /// text, then each is made a paragraph break, all in the same step.
    pub(crate) fn commit(
        &mut self,
        doc: &TextDocument,
        cursor: &TextCursor,
        text: &str,
        paragraphs: bool,
    ) -> usize {
        if !self.is_held(doc) {
            self.forget();
            return 0;
        }
        if text.is_empty() {
            self.cancel(doc);
            return 0;
        }
        self.forget();
        let breaks = paragraphs && text.contains('\u{2029}');
        if breaks {
            // The commit joins the group, which closes as one step.
            cursor.begin_edit_block();
        }
        let committed = cursor
            .commit_transient_text_with_origin(text, InsertionOrigin::Composed)
            .is_ok();
        if !committed {
            // Nothing was recorded; the document is back as it was before the
            // composition, or holds it still when the commit was refused.
            doc.revert_transient_edits();
        } else if breaks {
            let end = cursor.position();
            let start = end - text.chars().count();
            let separators: Vec<usize> = text
                .chars()
                .enumerate()
                .filter(|&(_, c)| c == '\u{2029}')
                .map(|(at, _)| start + at)
                .collect();
            // From the last: a paragraph break takes the one position the
            // separator held, so the earlier ones stay where they are.
            for at in separators.into_iter().rev() {
                cursor.set_position(at, MoveMode::MoveAnchor);
                cursor.set_position(at + 1, MoveMode::KeepAnchor);
                let _ = cursor.remove_selected_text();
                let _ = cursor.insert_block();
            }
            cursor.set_position(end, MoveMode::MoveAnchor);
        }
        if breaks {
            cursor.end_edit_block();
            // The group joined the history after the commit closed the step
            // before it: close the group too.
            doc.break_undo_merge();
        }
        if committed { text.chars().count() } else { 0 }
    }

    fn forget(&mut self) {
        self.segment = None;
        self.caret = None;
        self.shown = None;
        self.ended = false;
        self.layer = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> (TextDocument, TextCursor) {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let cursor = doc.cursor();
        (doc, cursor)
    }

    fn text(doc: &TextDocument) -> String {
        doc.to_plain_text().unwrap_or_default()
    }

    #[test]
    fn each_update_replaces_the_last_and_one_undo_takes_the_commit_back() {
        let (doc, cursor) = doc("abc");
        cursor.set_position(3, MoveMode::MoveAnchor);
        let mut composition = Composition::default();
        for state in ["n", "ni", "nih", "niha", "nihao"] {
            assert!(composition.show(&doc, &cursor, state));
            assert_eq!(text(&doc), format!("abc{state}"));
            assert_eq!(composition.range(&doc), Some(3..3 + state.len()));
        }
        assert_eq!(composition.commit(&doc, &cursor, "你好", false), 2);
        assert_eq!(text(&doc), "abc你好");
        assert_eq!(cursor.position(), 5);
        assert!(!composition.is_held(&doc));
        doc.undo().unwrap();
        assert_eq!(text(&doc), "abc");
    }

    #[test]
    fn an_ended_composition_is_kept_until_the_next_event_decides() {
        let (doc, cursor) = doc("ab");
        cursor.set_position(2, MoveMode::MoveAnchor);
        let mut composition = Composition::default();
        composition.show(&doc, &cursor, "ni");
        assert!(composition.end(&doc));
        assert_eq!(text(&doc), "abni", "the empty update leaves the text");
        assert!(composition.is_ended(&doc));
        assert!(!composition.is_live(&doc));
        assert_eq!(composition.range(&doc), None);
        assert!(composition.cancel(&doc));
        assert_eq!(text(&doc), "ab");
        assert_eq!(cursor.position(), 2);
    }

    #[test]
    fn a_recorded_write_is_reconciled() {
        let (doc, cursor) = doc("ab");
        cursor.set_position(2, MoveMode::MoveAnchor);
        let mut composition = Composition::default();
        composition.show(&doc, &cursor, "ni");
        doc.cursor_at(0).insert_text("x").unwrap();
        assert_eq!(text(&doc), "xab", "the write reverted the composition");
        assert!(!composition.is_live(&doc));
        assert_eq!(composition.text(&doc), None);
        assert!(composition.reconcile(&doc));
        assert_eq!(
            composition.commit(&doc, &cursor, "你", false),
            0,
            "nothing to commit once the document holds no composition"
        );
        assert_eq!(text(&doc), "xab");
    }

    #[test]
    fn paragraph_separators_in_a_commit_end_paragraphs_in_one_step() {
        let (doc, cursor) = doc("ab");
        cursor.set_position(2, MoveMode::MoveAnchor);
        let mut composition = Composition::default();
        composition.show(&doc, &cursor, "x");
        assert_eq!(composition.commit(&doc, &cursor, "one\u{2029}two", true), 7);
        assert_eq!(text(&doc), "abone\ntwo");
        assert_eq!(doc.block_count(), 2);
        assert_eq!(cursor.position(), 9);
        doc.undo().unwrap();
        assert_eq!(text(&doc), "ab", "one undo takes the whole commit back");
    }

    #[test]
    fn strokes_are_thin_except_the_segment() {
        use PreeditStroke::{Thick, Thin};
        let range = 5..11;
        assert_eq!(preedit_strokes(&range, None), vec![(5..11, Thin)]);
        assert_eq!(
            preedit_strokes(&range, Some(&(7..9))),
            vec![(5..7, Thin), (7..9, Thick), (9..11, Thin)]
        );
        assert_eq!(
            preedit_strokes(&range, Some(&(5..11))),
            vec![(5..11, Thick)],
            "a segment that is the whole composition is all thick"
        );
        assert_eq!(
            preedit_strokes(&range, Some(&(5..8))),
            vec![(5..8, Thick), (8..11, Thin)]
        );
        assert_eq!(
            preedit_strokes(&range, Some(&(9..20))),
            vec![(5..9, Thin), (9..11, Thick)],
            "clamped into the composition"
        );
        assert_eq!(
            preedit_strokes(&range, Some(&(20..30))),
            vec![(5..11, Thin)]
        );
        assert!(preedit_strokes(&(3..3), Some(&(3..3))).is_empty());
    }

    #[test]
    fn the_cursor_range_is_a_segment_or_a_caret_in_the_shown_characters() {
        let (doc, cursor) = doc("ab");
        cursor.set_position(2, MoveMode::MoveAnchor);
        let mut composition = Composition::default();
        // "にほん": three characters of three bytes. A range with an extent is
        // the segment, in characters.
        composition.show(&doc, &cursor, "にほん");
        composition.mark("にほん", Some(&(0..6)), &[true; 3]);
        assert_eq!(composition.segment(&doc), Some(2..4));
        composition.place_caret(&doc, &cursor);
        assert_eq!(
            cursor.position(),
            5,
            "a segment leaves the caret at the end"
        );
        // No extent: the caret's place inside it.
        composition.show(&doc, &cursor, "にほん");
        composition.mark("にほん", Some(&(3..3)), &[true; 3]);
        assert_eq!(composition.segment(&doc), None);
        composition.place_caret(&doc, &cursor);
        assert_eq!(cursor.position(), 3);
        // The input method hides its cursor: neither.
        composition.show(&doc, &cursor, "にほん");
        composition.mark("にほん", None, &[true; 3]);
        composition.place_caret(&doc, &cursor);
        assert_eq!(composition.segment(&doc), None);
        assert_eq!(cursor.position(), 5);
    }

    #[test]
    fn the_cursor_range_counts_only_the_characters_the_surface_kept() {
        let (doc, cursor) = doc("");
        let mut composition = Composition::default();
        // The surface dropped the second character of "a1b2c": the segment
        // "1b2" (bytes 1..4) is "b" of what it shows, "abc".
        composition.show(&doc, &cursor, "abc");
        composition.mark("a1b2c", Some(&(1..4)), &[true, false, true, false, true]);
        assert_eq!(composition.segment(&doc), Some(1..2));
        // A range past what was kept is clamped to it.
        composition.show(&doc, &cursor, "ab");
        composition.mark("abcdef", Some(&(1..6)), &[true; 6]);
        assert_eq!(composition.segment(&doc), Some(1..2));
    }

    #[test]
    fn a_position_after_the_composition_moves_with_a_shorter_commit() {
        // "hello" + "ni'hao" + " world", committed as "nihao".
        let range = 5..11;
        assert_eq!(position_after_commit(13, &range, 5), 12);
        assert_eq!(position_after_commit(2, &range, 5), 2, "before it stays");
        assert_eq!(position_after_commit(13, &range, 6), 13, "the same length");
        assert_eq!(position_after_commit(12, &range, 8), 14, "a longer one");
    }
}
