// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Plain-text clipboard for the code editor.
//!
//! A source editor's clipboard is plain text — no rich fragment, no HTML. The
//! one subtlety is paste: `TextCursor::insert_text` stores an embedded `\n` as a
//! literal character *inside a single block* (verified), which would break the
//! one-block-per-line model the gutter, the bracket scan, and the line
//! operations all rely on. So paste splits on newlines and inserts one block per
//! line, and the whole paste is one undo step.
//!
//! The handle is retrieved via `EventContext::app_state::<ClipboardHandle>()`. In
//! headless tests, or a build without the platform clipboard, the handle is
//! absent and every operation silently no-ops — the correct behaviour for a
//! surface with nowhere to read or write.

use bastyde_core::widget::EventContext;
use bastyde_platform::clipboard::ClipboardHandle;
use bastyde_text::text_document::{MoveMode, TextCursor};

use super::state::CodeEditorState;

/// The text a copy or cut acts on, and whether it was a whole line (so cut knows
/// to remove the line, not just clear a selection). With a selection it is the
/// selection; with none it is the caret's line.
fn target_text(st: &CodeEditorState) -> Option<(String, bool)> {
    if st.cursor.has_selection() {
        st.cursor.selected_text().ok().map(|t| (t, false))
    } else {
        st.document
            .snapshot_block_at_position_without_highlights(st.cursor.position())
            .map(|b| (b.text, true))
    }
}

/// Copy the selection, or the whole current line when there is none. A no-op
/// without a clipboard handle.
pub(super) fn copy(st: &CodeEditorState, ctx: &EventContext) {
    let Some((text, _)) = target_text(st) else {
        return;
    };
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        let _ = cb.set_text(&text);
    }
}

/// Cut the selection, or the whole current line when there is none — removing
/// the line and one adjacent separator so no blank line is left behind.
pub(super) fn cut(st: &mut CodeEditorState, ctx: &EventContext) {
    let Some((text, is_line)) = target_text(st) else {
        return;
    };
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        let _ = cb.set_text(&text);
    }
    if is_line {
        st.clear_extra_carets();
        delete_line(st);
    } else {
        let _ = st.cursor.remove_selected_text();
    }
    st.merge_collided_carets();
}

/// Remove the caret's whole line, taking one adjacent separator with it: the
/// trailing one when a line follows (so the next line moves up), otherwise the
/// leading one (so the previous line absorbs it), and neither for a lone line.
pub(super) fn delete_line(st: &mut CodeEditorState) {
    let pos = st.cursor.position();
    let Some(block) = st
        .document
        .snapshot_block_at_position_without_highlights(pos)
    else {
        return;
    };
    let start = block.position;
    let end = start + block.length;
    let doc_end = st.document.character_count();

    let (from, to) = if end < doc_end {
        (start, end + 1) // a line follows: take the trailing separator
    } else if start > 0 {
        (start - 1, end) // last line: take the leading separator
    } else {
        (start, end) // the only line: just clear it
    };
    st.cursor.set_position(from, MoveMode::MoveAnchor);
    st.cursor.set_position(to, MoveMode::KeepAnchor);
    let _ = st.cursor.remove_selected_text();
}

/// Paste clipboard text at the primary caret.
///
/// Single-caret: extra carets collapse first. Pasting the same block at several
/// carets, or one clipboard line per caret, is a refinement a source editor does
/// not owe — the predictable behaviour is one insertion at the primary caret.
pub(super) fn paste(st: &mut CodeEditorState, ctx: &EventContext) {
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else {
        return;
    };
    let Ok(text) = cb.get_text() else {
        return;
    };
    if text.is_empty() {
        return;
    }
    // Normalise line endings so a Windows clipboard does not leave stray CRs in
    // the document.
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    st.clear_extra_carets();
    insert_multiline(&st.cursor, &normalized);
}

/// Insert possibly-multi-line `text` at `cursor`, one block per line so no block
/// carries a literal newline. The whole insertion is one undo step.
pub(super) fn insert_multiline(cursor: &TextCursor, text: &str) {
    cursor.begin_edit_block();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            let _ = cursor.insert_block();
        }
        if !line.is_empty() {
            let _ = cursor.insert_text(line);
        }
    }
    cursor.end_edit_block();
}
