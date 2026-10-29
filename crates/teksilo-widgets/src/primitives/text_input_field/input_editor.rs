// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's document as the platform's input system writes it: the
//! [`TextInputEditor`] a [`TextFieldHandle`] answers with while the field
//! takes text (`TextSurface::input_editor`: not read-only, not secure). The
//! order of things is `common::input_editor`'s; the field's own rules are
//! the ones typing goes through: the single-line strip, the `char_filter`,
//! the `max_length` cap counted with the replaced range already gone. A
//! composition's intermediate states pass untouched; its final text meets
//! the rules as it ends.

use std::ops::Range;

use teksilo_core::ImeOrigin;
use teksilo_core::text_input_document::{
    CompositionSegment, Landed, TextInputEditor, WriteOptions, WriteRefused,
};
use teksilo_text::text_document::MoveMode;

use super::TextFieldHandle;
use super::keyboard::single_line_admits;
use super::state::{TextInputState, sync_cursor_signals};
use crate::EditSource;
use crate::common::document_offsets::document_end;
use crate::common::input_editor::{self, Writing};

/// The channel an input-system commit is reported under.
fn source_of(origin: ImeOrigin) -> EditSource {
    match origin {
        ImeOrigin::Dictation => EditSource::Dictation,
        ImeOrigin::WritingTools => EditSource::WritingTools,
        _ => EditSource::Ime,
    }
}

/// What the field's rules keep of `text` put over `replaced`.
fn kept(st: &TextInputState, text: &str, replaced: &Range<usize>) -> String {
    let clean: String = text
        .chars()
        .filter(|c| single_line_admits(*c))
        .filter(|c| st.char_filter_admits(*c))
        .collect();
    match st.max_length {
        Some(max) => {
            let staying = document_end(&st.document) - replaced.len();
            clean.chars().take(max.saturating_sub(staying)).collect()
        }
        None => clean,
    }
}

impl TextFieldHandle {
    /// Run `write` on the field's state if it can take a write now.
    fn writable<R>(
        &self,
        write: impl FnOnce(&mut TextInputState) -> Result<R, WriteRefused>,
    ) -> Result<R, WriteRefused> {
        let state = self.shared_state().ok_or(WriteRefused::Busy)?;
        let result = {
            let mut st = state.try_borrow_mut().map_err(|_| WriteRefused::Busy)?;
            if st.read_only || st.secure {
                return Err(WriteRefused::ReadOnly);
            }
            if !st.pending_chars.is_empty() {
                return Err(WriteRefused::Busy);
            }
            let result = write(&mut st)?;
            st.pending_text_changed = true;
            if let Some(request) = &st.frame_request {
                request.set(true);
            }
            result
        };
        let deferred = state
            .try_borrow()
            .is_ok_and(|st| st.input_system.batch.defer_cursor_moved());
        if !deferred {
            sync_cursor_signals(&state);
        }
        Ok(result)
    }
}

/// The field's state lent to a write.
fn writing(st: &mut TextInputState) -> Writing<'_> {
    let end = document_end(&st.document);
    let TextInputState {
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

impl TextInputEditor for TextFieldHandle {
    fn replace_text(
        &self,
        range: Range<usize>,
        text: &str,
        options: WriteOptions,
    ) -> Result<Landed, WriteRefused> {
        self.writable(|st| {
            let composing = st.composition.is_written(&st.document);
            let filtered =
                (!composing).then(|| kept(st, &input_editor::normalize_breaks(text), &range));
            let landed = input_editor::replace(
                writing(st),
                range,
                text,
                options,
                |text, _| filtered.unwrap_or_else(|| text.to_owned()),
                |cursor, text, _| {
                    cursor.begin_edit_block();
                    if text.is_empty() {
                        if cursor.has_selection() {
                            let _ = cursor.remove_selected_text();
                        }
                    } else {
                        let _ = cursor.insert_text(text);
                    }
                    cursor.end_edit_block();
                    text.chars().count()
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
            if selection.start > selection.end || selection.end > document_end(&st.document) {
                return Err(WriteRefused::InvalidRange);
            }
            let (anchor, caret) = if caret_at_start {
                (selection.end, selection.start)
            } else {
                (selection.start, selection.end)
            };
            st.cursor.set_position(anchor, MoveMode::MoveAnchor);
            st.cursor.set_position(caret, MoveMode::KeepAnchor);
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
            // The rules see the field without the composition.
            let range = st.composition.range(&st.document).unwrap_or_default();
            let filtered = st
                .composition
                .written_text(&st.document)
                .map(|text| kept(st, &text, &range));
            if let Some(arrived) = input_editor::end(
                writing(st),
                |text| filtered.unwrap_or_else(|| text.to_owned()),
                false,
            ) {
                st.report_inserted_chars(source_of(origin), arrived);
            }
            Ok(())
        })
    }

    fn begin_input_batch(&self) {
        if let Some(state) = self.shared_state()
            && let Ok(st) = state.try_borrow()
        {
            st.input_system.batch.begin(&st.document);
        }
    }

    fn end_input_batch(&self) {
        let Some(state) = self.shared_state() else {
            return;
        };
        let Some((ended, marker)) = state.try_borrow().ok().and_then(|st| {
            let ended = st.input_system.batch.end()?;
            Some((ended, st.input_system.journal.marker()))
        }) else {
            return;
        };
        // The document's events, held since the batch began, go out first,
        // as the input system's, with no borrow of the state held.
        let deferred = ended.deferred;
        marker.while_writing(|| drop(ended.hold));
        if let Ok(st) = state.try_borrow() {
            for (source, chars) in deferred.arrivals {
                st.report_inserted_chars(source, chars);
            }
        }
        if deferred.cursor_moved {
            sync_cursor_signals(&state);
        }
    }
}
