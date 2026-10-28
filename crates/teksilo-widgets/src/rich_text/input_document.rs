// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The rich text editor's document, as the platform's text input system reads
//! it: the [`TextInputDocument`] an [`EditorHandle`] answers with, for both
//! faces of the widget (the editor and the read-only viewer).
//!
//! Offsets are the document's character offsets, the space of its cursor:
//! a paragraph break, a `U+2028` line break and an object anchor (`U+FFFC`,
//! an image or a table) are one character each. The text comes from
//! `TextDocument::text_at`, the length from the end of the document
//! ([`document_end`]) and the UTF-16 index from its addressable text. Never
//! from `to_plain_text`, which leaves a table's anchor out and is two
//! characters short per table, nor from `character_count`, which is also
//! short by one per paragraph break.
//!
//! # Typing not yet inserted
//!
//! Typed characters wait in `pending_chars` until the frame tick inserts them
//! (`frame_loop::flush_pending_chars`). Every answer about the text, the
//! selection, the composition or where they are drawn inserts them first,
//! exactly as the tick's first step would, rather than answering as if they
//! were in:
//!
//! * **It must answer for them.** The input system asks where the selection
//!   is, then commits text naming a range in the same text, and the commit
//!   (`keyboard::replace_from_input_system`) flushes the batch before it
//!   reads that range. On macOS the accent popup asks inside the repeat of
//!   the key whose letter it will replace: an answer that left the letter out
//!   would aim the replacement one character too early.
//! * **Inserting is the answer that stays true.** Answering as if inserted
//!   would mean splicing the batch into every answer, and the batch has no
//!   layout to give a rectangle for.
//! * **It is safe outside dispatch,** because that is where it already runs:
//!   the frame tick calls it with no `EventContext`, and the clipboard
//!   commands call it on their way in. It inserts through the editor's
//!   cursor, fires the document's change callbacks (the editor's own queue,
//!   which the next tick drains and relays out from, and the application's),
//!   and reports the batch to the application's arrivals callback, which
//!   takes no context. The tick that follows finds the events and does the
//!   rest; a frame is requested so that it comes.
//!
//! Until that tick, the inserted characters have no layout of their own, so a
//! rectangle asked for them is approximate for one frame.
//!
//! # Busy
//!
//! Every query borrows the editor's state with `try_borrow` (and
//! `try_borrow_mut` only when there is typing to insert), and answers `None`
//! when the editor holds it: the platform may ask at any moment, including
//! from inside the editor's own work.

use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::text_input_document::{FirstLineRect, TextInputDocument, Utf16Index};
use teksilo_text::CursorAffinity;

use super::EditorHandle;
use super::keyboard::engine_caret_to_window;
use super::policy::EditCommandKind;
use super::state::EditorState;
use crate::common::document_offsets;

/// The document's last offset, paragraph breaks and object anchors included:
/// [`document_offsets::document_end`].
pub(super) fn document_end(st: &EditorState) -> usize {
    document_offsets::document_end(&st.document)
}

impl EditorHandle {
    /// Run `f` on the editor's state with every typed character inserted.
    /// `None` while the state is borrowed. See the module documentation.
    fn with_typing_inserted<R>(&self, f: impl FnOnce(&EditorState) -> R) -> Option<R> {
        {
            let st = self.state.try_borrow().ok()?;
            if st.pending_chars.is_empty() {
                return Some(f(&st));
            }
        }
        let mut st = self.state.try_borrow_mut().ok()?;
        super::frame_loop::flush_pending_chars(&mut st);
        if let Some(request) = &st.frame_request {
            request.set(true);
        }
        Some(f(&st))
    }
}

impl TextInputDocument for EditorHandle {
    fn char_count(&self) -> Option<usize> {
        self.with_typing_inserted(document_end)
    }

    fn text_in(&self, range: Range<usize>) -> Option<String> {
        self.with_typing_inserted(|st| {
            let range = clamp(range, document_end(st));
            if range.is_empty() {
                return Some(String::new());
            }
            st.document.text_at(range.start, range.len()).ok()
        })
        .flatten()
    }

    fn selection(&self) -> Option<Range<usize>> {
        self.with_typing_inserted(|st| {
            let (anchor, caret) = (st.cursor.anchor(), st.cursor.position());
            anchor.min(caret)..anchor.max(caret)
        })
    }

    fn caret(&self) -> Option<usize> {
        self.with_typing_inserted(|st| st.cursor.position())
    }

    fn composition_range(&self) -> Option<Range<usize>> {
        // The document says where the composition is: a recorded write since
        // it was shown reverted it, and an ended one waits only for its commit.
        self.with_typing_inserted(|st| st.composition.range(&st.document))
            .flatten()
    }

    fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect> {
        self.with_typing_inserted(|st| first_line_rect(st, range))
            .flatten()
    }

    fn offset_at_point(&self, window_point: Point) -> Option<usize> {
        self.with_typing_inserted(|st| offset_at_point(st, window_point))
            .flatten()
    }

    fn visible_text_rect(&self) -> Option<Rect> {
        self.with_typing_inserted(visible_text_rect).flatten()
    }

    fn is_editable(&self) -> bool {
        self.state.try_borrow().is_ok_and(|st| {
            st.policy
                .command_filter
                .accepts(EditCommandKind::InsertChar)
        })
    }

    /// Never: the rich editor has no secure mode.
    fn is_secure(&self) -> bool {
        false
    }

    fn writing_tools_allowed(&self) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|st| st.writing_tools_allowed())
    }

    fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
        self.with_typing_inserted(|st| st.utf16.index(&st.document))
    }
}

/// `range` within `0..=len`, its end not before its start.
fn clamp(range: Range<usize>, len: usize) -> Range<usize> {
    let start = range.start.min(len);
    start..range.end.min(len).max(start)
}

/// The rectangle of `range`'s first visual line, in window space.
///
/// Mind text-typeset's affinity names: at a soft-wrap position, `Upstream` is
/// the START of the next line and `Downstream` the END of the previous one.
/// So the range's start is read `Upstream` (its first character is on the
/// line that starts there) and the covered end `Downstream` (it ends the line
/// it is on).
fn first_line_rect(st: &EditorState, range: Range<usize>) -> Option<FirstLineRect> {
    if !st.engine.has_full_layout() {
        return None;
    }
    let range = clamp(range, document_end(st));
    let start = range.start;
    if range.is_empty() {
        // At the caret, the caret's own placement, so the answer is where the
        // editor draws it; elsewhere the editor's default placement.
        let affinity = if start == st.cursor.position() {
            st.cursor_affinity
        } else {
            CursorAffinity::Downstream
        };
        let rect = engine_caret_to_window(st, st.engine.caret_rect(start, affinity));
        return Some(FirstLineRect {
            rect,
            covered: start..start,
        });
    }

    let first = engine_caret_to_window(st, st.engine.caret_rect(start, CursorAffinity::Upstream));
    let line_end = st
        .engine
        .visual_line_range_at(start, CursorAffinity::Upstream)
        .map(|(_, end)| end)
        .filter(|&end| end > start);
    let (covered_end, last_x) = match line_end {
        Some(line_end) => {
            let end = range.end.min(line_end);
            let last =
                engine_caret_to_window(st, st.engine.caret_rect(end, CursorAffinity::Downstream));
            (end, last.x)
        }
        // The range starts where its line ends: on a paragraph break, or on
        // an object anchor with no line of its own. Cover that one character,
        // drawn as the caret before it, so a caller walking the range line by
        // line still moves on.
        None => (start + 1, first.x),
    };
    let left = first.x.min(last_x);
    let right = first.x.max(last_x);
    Some(FirstLineRect {
        rect: Rect::new(left, first.y, (right - left).max(1.0), first.height),
        covered: start..covered_end,
    })
}

/// The document offset nearest `window_point`, `None` outside the body.
fn offset_at_point(st: &EditorState, window_point: Point) -> Option<usize> {
    if !st.engine.has_full_layout() {
        return None;
    }
    let local = super::touch::window_to_engine_local(st, window_point);
    let inside = local.x >= 0.0
        && local.y >= 0.0
        && local.x < st.viewport_width
        && local.y < st.viewport_height;
    if !inside {
        return None;
    }
    // The engine finds no block only in the empty part of the body below the
    // last line, which belongs to the end of the text.
    Some(
        super::hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
            .map_or_else(|| document_end(st), |hit| hit.position),
    )
}

/// The body's rectangle in the window: where the text is shown.
fn visible_text_rect(st: &EditorState) -> Option<Rect> {
    (st.viewport_width > 0.0 && st.viewport_height > 0.0).then(|| {
        Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            st.viewport_width,
            st.viewport_height,
        )
    })
}
