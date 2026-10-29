// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The editor's document as the platform's input system writes it: the
//! [`TextInputEditor`] an [`EditorHandle`] answers with while the editor
//! takes text (`TextSurface::input_editor`). The order of things is
//! `common::input_editor`'s; what is the editor's own:
//!
//! - text outside a composition goes in as a paste of plain text does
//!   (`clipboard::insert_plain_text_at`: a paragraph break starts a block),
//!   and a correction takes the replaced text's character format;
//! - its rules are its command filter's: an editor that takes no typed text
//!   takes none from the input system either;
//! - typing still batched for the frame tick makes it busy: the input system
//!   asks for it to be inserted first (`flush_pending_input`), where a change
//!   may be reported.

use std::ops::Range;

use teksilo_core::ImeOrigin;
use teksilo_core::text_input_document::{
    CompositionSegment, Landed, TextInputEditor, WriteOptions, WriteRefused,
};
use teksilo_text::text_document::InsertionOrigin;

use super::input_document::document_end;
use super::policy::EditCommandKind;
use super::state::EditorState;
use super::{EditorHandle, clipboard, sync_cursor_signals};
use crate::EditSource;
use crate::common::input_editor::{self, Writing};

/// The channel an input-system commit is reported under.
pub(crate) fn source_of(origin: ImeOrigin) -> EditSource {
    match origin {
        ImeOrigin::Dictation => EditSource::Dictation,
        ImeOrigin::WritingTools => EditSource::WritingTools,
        _ => EditSource::Ime,
    }
}

impl EditorHandle {
    /// Run `write` on the editor's state if it can take a write now.
    fn writable<R>(
        &self,
        write: impl FnOnce(&mut EditorState) -> Result<R, WriteRefused>,
    ) -> Result<R, WriteRefused> {
        let result = {
            let mut st = self
                .state
                .try_borrow_mut()
                .map_err(|_| WriteRefused::Busy)?;
            if !st
                .policy
                .command_filter
                .accepts(EditCommandKind::InsertChar)
            {
                return Err(WriteRefused::ReadOnly);
            }
            if !st.pending_chars.is_empty() {
                return Err(WriteRefused::Busy);
            }
            let result = write(&mut st)?;
            st.pending_text_changed = true;
            st.preferred_x = None;
            if let Some(request) = &st.frame_request {
                request.set(true);
            }
            result
        };
        self.cursor_moved();
        Ok(result)
    }

    /// The caret moved: its signals now, or at the end of the batch.
    fn cursor_moved(&self) {
        let deferred = self
            .state
            .try_borrow()
            .is_ok_and(|st| st.input_system.batch.defer_cursor_moved());
        if !deferred {
            sync_cursor_signals(&self.state);
        }
    }
}

/// The editor's state lent to a write.
fn writing(st: &mut EditorState) -> Writing<'_> {
    let end = document_end(st);
    let EditorState {
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

impl TextInputEditor for EditorHandle {
    fn replace_text(
        &self,
        range: Range<usize>,
        text: &str,
        options: WriteOptions,
    ) -> Result<Landed, WriteRefused> {
        self.writable(|st| {
            let composing = st.composition.is_written(&st.document);
            let landed = input_editor::replace(
                writing(st),
                range,
                text,
                options,
                |text, _| text.to_owned(),
                |cursor, text, correction| {
                    if correction && let Ok(format) = cursor.char_format() {
                        let _ = cursor.insert_formatted_text_with_origin(
                            text,
                            &format,
                            InsertionOrigin::Composed,
                        );
                        text.chars().count()
                    } else {
                        clipboard::insert_plain_text_at(cursor, text)
                    }
                },
            )?;
            if !composing && !options.dry_run {
                st.report_inserted_chars(EditSource::Ime, landed.inserted.len());
            }
            Ok(landed)
        })
    }

    fn set_selection(
        &self,
        selection: Range<usize>,
        caret_at_start: bool,
    ) -> Result<(), WriteRefused> {
        self.writable(|st| {
            if selection.start > selection.end || selection.end > document_end(st) {
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

    fn end_composition(&self, origin: ImeOrigin) -> Result<(), WriteRefused> {
        self.writable(|st| {
            if let Some(arrived) = input_editor::end(writing(st), |text| text.to_owned(), true) {
                st.report_inserted_chars(source_of(origin), arrived);
            }
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
        if let Ok(st) = self.state.try_borrow() {
            for (source, chars) in deferred.arrivals {
                st.report_inserted_chars(source, chars);
            }
        }
        if deferred.cursor_moved {
            sync_cursor_signals(&self.state);
        }
    }
}
