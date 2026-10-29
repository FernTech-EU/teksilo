// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code editor's document as the platform's text input system reads and
//! writes it: the [`TextInputDocument`] and [`TextInputEditor`] a
//! [`CodeEditorHandle`] answers with. With them the editor leaves the IMM path
//! on Windows for the TSF text store, and answers the macOS input system's
//! questions about its text.
//!
//! The rich editor's counterpart (`rich_text/input_document.rs`) is the model,
//! and the order of things is `common::input_editor`'s. What is the code
//! editor's own:
//!
//! - **Its text is plain code.** A write outside a composition goes in as a
//!   paste does (one paragraph per line, `clipboard::insert_multiline`),
//!   through no rule but the command filter: auto-close, auto-indent and the
//!   like are the keys', and the input system writes text, not keys.
//! - **One caret.** The input system knows one selection; a write collapses
//!   the extra carets of a multi-caret edit first, as the IMM path's
//!   composition does.
//! - **Typing batched for the frame** is inserted before an answer, as the
//!   rich editor's is, at every caret.

use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::ImeOrigin;
use teksilo_core::text_input_document::{
    CompositionSegment, FirstLineRect, InputRevisions, Landed, TextChanged, TextInputDocument,
    TextInputEditor, Utf16Index, WriteOptions, WriteRefused,
};
use teksilo_core::widget::EventContext;
use teksilo_text::CursorAffinity;

use super::policy::CodeCommand;
use super::state::{CodeEditorState, SharedState};
use super::{CodeEditorHandle, clipboard, sync_cursor_signals};
use crate::common::document_offsets::document_end;
use crate::common::input_editor::{self, Writing};

impl CodeEditorHandle {
    /// Run `f` on the editor's state with every typed character inserted.
    /// `None` while the state is borrowed.
    fn with_typing_inserted<R>(&self, f: impl FnOnce(&CodeEditorState) -> R) -> Option<R> {
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

    /// Run `write` on the editor's state if it can take a write now.
    fn writable<R>(
        &self,
        write: impl FnOnce(&mut CodeEditorState) -> Result<R, WriteRefused>,
    ) -> Result<R, WriteRefused> {
        let result = {
            let mut st = self
                .state
                .try_borrow_mut()
                .map_err(|_| WriteRefused::Busy)?;
            if st.policy.is_read_only()
                || !st.policy.command_filter.accepts(CodeCommand::InsertChar)
            {
                return Err(WriteRefused::ReadOnly);
            }
            if !st.pending_chars.is_empty() {
                return Err(WriteRefused::Busy);
            }
            st.clear_extra_carets();
            let result = write(&mut st)?;
            st.pending_text_changed = true;
            st.preferred_x = None;
            st.cursor_affinity = CursorAffinity::Downstream;
            if let Some(request) = &st.frame_request {
                request.set(true);
            }
            result
        };
        let deferred = self
            .state
            .try_borrow()
            .is_ok_and(|st| st.input_system.batch.defer_cursor_moved());
        if !deferred {
            sync_cursor_signals(&self.state);
        }
        Ok(result)
    }
}

/// `range` within `0..=len`, its end not before its start.
fn clamp(range: Range<usize>, len: usize) -> Range<usize> {
    let start = range.start.min(len);
    start..range.end.min(len).max(start)
}

/// An engine caret rectangle (`[x, y, width, height]`, body space) in window
/// space, as the editor paints it: shifted by the body's origin and scroll.
fn to_window(st: &CodeEditorState, c: [f32; 4]) -> Rect {
    Rect::new(
        st.viewport_origin.x + c[0] - st.scroll_x.get(),
        st.viewport_origin.y + c[1] - st.scroll_y.get(),
        c[2].max(1.0),
        c[3],
    )
}

/// The rectangle of `range`'s first visual line, in window space. As the
/// rich editor's: the start read `Upstream` (its first character is on the
/// line that starts there), the covered end `Downstream`.
fn first_line_rect(st: &CodeEditorState, range: Range<usize>) -> Option<FirstLineRect> {
    if !st.engine.has_full_layout() {
        return None;
    }
    let range = clamp(range, document_end(&st.document));
    if range.is_empty() {
        let affinity = if range.start == st.cursor.position() {
            st.cursor_affinity
        } else {
            CursorAffinity::Downstream
        };
        let at = st.shown_position(range.start);
        return Some(FirstLineRect {
            rect: to_window(st, st.engine.caret_rect(at, affinity)),
            covered: range.start..range.start,
        });
    }
    let start = st.shown_position(range.start);
    let end = st.shown_position(range.end);
    let first = to_window(st, st.engine.caret_rect(start, CursorAffinity::Upstream));
    let line_end = st
        .engine
        .visual_line_range_at(start, CursorAffinity::Upstream)
        .map(|(_, end)| end)
        .filter(|&end| end > start);
    let (covered_end, last_x) = match line_end {
        Some(line_end) => {
            let end = end.min(line_end);
            let last = to_window(st, st.engine.caret_rect(end, CursorAffinity::Downstream));
            (end, last.x)
        }
        // On a line break: that one character, drawn as the caret before it.
        None => (start + 1, first.x),
    };
    let left = first.x.min(last_x);
    let right = first.x.max(last_x);
    let covered_end = st
        .document_position(covered_end)
        .max(range.start + 1)
        .min(range.end);
    Some(FirstLineRect {
        rect: Rect::new(left, first.y, (right - left).max(1.0), first.height),
        covered: range.start..covered_end,
    })
}

/// The body's rectangle in the window: where the text is shown.
fn visible_text_rect(st: &CodeEditorState) -> Option<Rect> {
    (st.viewport_width > 0.0 && st.viewport_height > 0.0).then(|| {
        Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            st.viewport_width,
            st.viewport_height,
        )
    })
}

impl TextInputDocument for CodeEditorHandle {
    fn is_available(&self) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|st| st.pending_chars.is_empty())
    }

    fn flush_pending_input(&self) -> bool {
        let Ok(mut st) = self.state.try_borrow_mut() else {
            return false;
        };
        if !super::frame_loop::flush_pending_chars(&mut st) {
            return false;
        }
        if let Some(request) = &st.frame_request {
            request.set(true);
        }
        true
    }

    fn revisions(&self) -> Option<InputRevisions> {
        let st = self.state.try_borrow().ok()?;
        let caret = st.cursor.position();
        let caret_rect = first_line_rect(&st, caret..caret).map(|line| line.rect);
        Some(st.input_system.revisions(
            st.cursor.anchor(),
            caret,
            caret_rect,
            visible_text_rect(&st),
        ))
    }

    fn text_changed_since(&self, revision: u64) -> Option<TextChanged> {
        self.state
            .try_borrow()
            .ok()?
            .input_system
            .journal
            .changed_since(revision)
    }

    fn char_count(&self) -> Option<usize> {
        self.with_typing_inserted(|st| document_end(&st.document))
    }

    fn text_in(&self, range: Range<usize>) -> Option<String> {
        self.with_typing_inserted(|st| {
            let range = clamp(range, document_end(&st.document));
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
        self.with_typing_inserted(|st| st.composition.range(&st.document))
            .flatten()
    }

    fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect> {
        self.with_typing_inserted(|st| first_line_rect(st, range))
            .flatten()
    }

    fn offset_at_point(&self, window_point: Point) -> Option<usize> {
        self.with_typing_inserted(|st| {
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
            Some(
                st.hit_test(local)
                    .map_or_else(|| document_end(&st.document), |hit| hit.position),
            )
        })
        .flatten()
    }

    fn visible_text_rect(&self) -> Option<Rect> {
        self.with_typing_inserted(visible_text_rect).flatten()
    }

    fn is_editable(&self) -> bool {
        self.state.try_borrow().is_ok_and(|st| {
            !st.policy.is_read_only() && st.policy.command_filter.accepts(CodeCommand::InsertChar)
        })
    }

    /// Never: code has no secure mode.
    fn is_secure(&self) -> bool {
        false
    }

    /// Never: a writing assistant rewriting code is not what a code editor
    /// offers.
    fn writing_tools_allowed(&self) -> bool {
        false
    }

    fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
        self.with_typing_inserted(|st| {
            st.utf16
                .current_index(&st.document, st.input_system.batch.is_open())
        })
    }
}

/// The editor's state lent to a write.
fn writing(st: &mut CodeEditorState) -> Writing<'_> {
    let end = document_end(&st.document);
    let CodeEditorState {
        document,
        cursor,
        composition,
        input_system,
        ..
    } = st;
    Writing {
        document,
        cursor,
        composition,
        input: input_system,
        end,
    }
}

/// Code keeps the line breaks and tabs the input system writes, and no
/// other control character, as a paste does.
fn code_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

impl TextInputEditor for CodeEditorHandle {
    fn replace_text(
        &self,
        range: Range<usize>,
        text: &str,
        options: WriteOptions,
    ) -> Result<Landed, WriteRefused> {
        self.writable(|st| {
            input_editor::replace(
                writing(st),
                range,
                text,
                options,
                |text, _| code_text(text),
                |cursor, text, _| {
                    clipboard::insert_multiline(cursor, text);
                    text.chars().count()
                },
            )
        })
    }

    fn set_selection(
        &self,
        selection: Range<usize>,
        caret_at_start: bool,
    ) -> Result<(), WriteRefused> {
        self.writable(|st| {
            if selection.start > selection.end || selection.end > document_end(&st.document) {
                return Err(WriteRefused::InvalidRange);
            }
            let (anchor, caret) = if caret_at_start {
                (selection.end, selection.start)
            } else {
                (selection.start, selection.end)
            };
            st.cursor
                .set_position(anchor, teksilo_text::text_document::MoveMode::MoveAnchor);
            st.cursor
                .set_position(caret, teksilo_text::text_document::MoveMode::KeepAnchor);
            Ok(())
        })
    }

    fn mark_composition(
        &self,
        range: Range<usize>,
        segments: &[CompositionSegment],
    ) -> Result<(), WriteRefused> {
        self.writable(|st| input_editor::mark(writing(st), range, segments))
    }

    fn end_composition(&self, _origin: ImeOrigin) -> Result<(), WriteRefused> {
        self.writable(|st| {
            input_editor::end(writing(st), code_text, false);
            Ok(())
        })
    }

    fn begin_input_batch(&self) {
        if let Ok(st) = self.state.try_borrow() {
            st.input_system.batch.begin(&st.document);
        }
    }

    fn end_input_batch(&self) {
        let Some((ended, marker)) = self.state.try_borrow().ok().and_then(|st| {
            let ended = st.input_system.batch.end()?;
            Some((ended, st.input_system.journal.marker()))
        }) else {
            return;
        };
        // The document's events, held since the batch began, go out first,
        // as the input system's, with no borrow of the state held.
        let deferred = ended.deferred;
        marker.while_writing(|| drop(ended.hold));
        if deferred.cursor_moved {
            sync_cursor_signals(&self.state);
        }
    }
}

/// End the composition the input system writes (a TSF text store's), its
/// text kept as one undo step: before a key the text service passed on acts,
/// before a press, and as focus leaves. The platform is told first
/// ([`EventContext::finish_ime_composition`]): the text service ends its own
/// composition and writes its last text, and the editor then finds it ended.
/// What is still written after that is committed here. Does nothing when no
/// such composition is open; the IMM path's composition keeps its own rules.
pub(super) fn finish_written_composition(state: &SharedState, ctx: &mut EventContext) -> bool {
    let written = state
        .try_borrow()
        .is_ok_and(|st| st.composition.is_written(&st.document));
    if !written {
        return false;
    }
    let _ = ctx.finish_ime_composition();
    {
        let mut guard = state.borrow_mut();
        let st = &mut *guard;
        if st.composition.is_written(&st.document) {
            input_editor::end(writing(st), code_text, false);
            st.pending_text_changed = true;
        }
    }
    sync_cursor_signals(state);
    ctx.request_frame();
    true
}

/// One stroke of a composition's underline, in window space: from `x0` to
/// `x1` at `y`, `width` thick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UnderlineStroke {
    pub(crate) x0: f32,
    pub(crate) x1: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
}

/// The strokes the composition is underlined with, as the rich editor and
/// the text field draw it: one line per clause the input system marked (the
/// clause being converted twice as thick, several clauses a little short at
/// both ends), one thin line for a composition with none (the IMM path's),
/// split where a clause wraps. Empty with no composition shown, or before
/// the layout.
pub(crate) fn composition_strokes(st: &CodeEditorState) -> Vec<UnderlineStroke> {
    let Some(range) = st
        .composition
        .range(&st.document)
        .filter(|range| !range.is_empty())
        .filter(|_| st.shows_transient_edits())
    else {
        return Vec::new();
    };
    if !st.engine.has_full_layout() {
        return Vec::new();
    }
    let lines = crate::common::input_editor::composition_underlines(
        range,
        &st.input_system.segments.borrow(),
    );
    let mut strokes = Vec::new();
    for line in lines {
        let width = if line.thick { 2.0 } else { 1.0 };
        let inset = if line.inset { 1.0 } else { 0.0 };
        // (x where the stroke starts, last x, y of its line, line height)
        let mut current: Option<(f32, f32, f32, f32)> = None;
        let mut first = true;
        for p in line.range.start..=line.range.end {
            let c = to_window(
                st,
                st.engine
                    .caret_rect(st.shown_position(p), CursorAffinity::Downstream),
            );
            match current {
                Some((x0, last, y, h)) if (c.y - y).abs() > 0.5 => {
                    strokes.push(UnderlineStroke {
                        x0,
                        x1: last,
                        y: y + h - width,
                        width,
                    });
                    current = Some((c.x, c.x, c.y, c.height));
                }
                Some((x0, _, y, h)) => current = Some((x0, c.x, y, h)),
                None => {
                    let start = if first { c.x + inset } else { c.x };
                    current = Some((start, c.x, c.y, c.height));
                }
            }
            first = false;
        }
        if let Some((x0, last, y, h)) = current {
            strokes.push(UnderlineStroke {
                x0,
                x1: last - inset,
                y: y + h - width,
                width,
            });
        }
    }
    strokes
}
