// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The typing format: Bold, Italic, Underline or Strikethrough toggled on a
//! collapsed caret, for the text typed next at that caret.
//!
//! A format command formats the selection, and a collapsed caret selects
//! nothing, so the document has nothing to format there. Every word processor
//! (Word, LibreOffice, TextEdit, a `QTextEdit` given `setFontWeight` on a
//! collapsed cursor) keeps the format instead and gives it to the text typed
//! next at that caret. So does this: the toggle is kept here, as pending, until
//! the next insertion at the caret (typed text, the commit of an input
//! method's composition, a commit with no composition before it), which goes
//! in with it as one undo step. Moving the caret or selecting drops it.
//!
//! [`caret_format`] is the format the next typed character takes: the format
//! of the character before the caret, which typed text inherits (at the start
//! of a paragraph, the format of its first character; none in an empty one),
//! with the pending toggles over it. The toolbar's toggles and their accessible
//! state read it through
//! [`RichTextEditor::caret_char_format`](super::RichTextEditor::caret_char_format)
//! and its `is_bold` siblings; a change of the pending toggles bumps
//! `format_version`, as a format change of the document does, so a toolbar
//! watching it reads them again.
//!
//! A composition is shown in that format too, the toggles merged over the
//! format it inherits (`TextCursor::insert_text_transient_merged`), and its
//! commit records them with the text.

use teksilo_text::text_document::{MoveMode, TextFormat};

use super::state::EditorState;

/// One of the toggles a collapsed caret keeps.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum FormatToggle {
    Bold,
    Italic,
    Underline,
    Strikethrough,
}

impl FormatToggle {
    fn get(self, format: &TextFormat) -> Option<bool> {
        match self {
            Self::Bold => format.font_bold,
            Self::Italic => format.font_italic,
            Self::Underline => format.font_underline,
            Self::Strikethrough => format.font_strikeout,
        }
    }

    fn set(self, format: &mut TextFormat, value: bool) {
        let field = match self {
            Self::Bold => &mut format.font_bold,
            Self::Italic => &mut format.font_italic,
            Self::Underline => &mut format.font_underline,
            Self::Strikethrough => &mut format.font_strikeout,
        };
        *field = Some(value);
    }

    /// The toggle a format command sets, when it sets only one of them.
    fn of(format: &TextFormat) -> Option<(Self, bool)> {
        let only = TextFormat {
            font_bold: format.font_bold,
            font_italic: format.font_italic,
            font_underline: format.font_underline,
            font_strikeout: format.font_strikeout,
            ..Default::default()
        };
        if &only != format {
            return None;
        }
        let mut set = [
            Self::Bold,
            Self::Italic,
            Self::Underline,
            Self::Strikethrough,
        ]
        .into_iter()
        .filter_map(|toggle| toggle.get(format).map(|value| (toggle, value)));
        match (set.next(), set.next()) {
            (Some(one), None) => Some(one),
            _ => None,
        }
    }
}

/// The toggles kept for the caret at `at`, a position of the recorded
/// document (the editor's own composition, shown as transient edits, does not
/// move it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TypingFormat {
    at: usize,
    /// Only the toggled fields are set, or the fields carried across a
    /// paragraph break ([`insert_paragraph`]).
    format: TextFormat,
}

/// Where the caret is, when it is collapsed: the recorded document's position,
/// which the editor's own composition leaves where the composition began.
fn collapsed_caret(st: &EditorState) -> Option<usize> {
    let at = st.cursor.recorded_position();
    (st.cursor.recorded_anchor() == at && !st.cursor.has_selection()).then_some(at)
}

/// The pending toggles, while the caret is still where they were set.
pub(crate) fn pending(st: &EditorState) -> Option<&TextFormat> {
    let kept = st.typing_format.as_ref()?;
    (collapsed_caret(st) == Some(kept.at)).then_some(&kept.format)
}

/// Drop the pending toggles if the caret moved or selected since they were
/// set. Returns whether they were dropped, so the caller tells the toolbar.
pub(crate) fn prune(st: &mut EditorState) -> bool {
    if st.typing_format.is_some() && pending(st).is_none() {
        st.typing_format = None;
        return true;
    }
    false
}

/// The character format the next character typed at the collapsed caret
/// takes: the format of the character before it, which typed text inherits
/// (at the start of a paragraph the format of its first character, none in an
/// empty paragraph), with the pending toggles over it.
pub(crate) fn caret_format(st: &EditorState) -> TextFormat {
    let position = st.cursor.position();
    let probe = st.document.cursor();
    probe.set_position(position, MoveMode::MoveAnchor);
    // A cell of a table at the document's start is at position 0 without
    // being at a block start: it answers its own format.
    if !probe.at_block_start()
        && let Some(before) = position.checked_sub(1)
    {
        probe.set_position(before, MoveMode::MoveAnchor);
    }
    let mut format = probe.char_format().unwrap_or_default();
    if let Some(kept) = pending(st) {
        overlay(&mut format, kept);
    }
    format
}

/// Every character field `over` sets, set on `format`. Not the link: a link
/// is never kept for the text typed next.
fn overlay(format: &mut TextFormat, over: &TextFormat) {
    fn set<T: Clone>(field: &mut Option<T>, over: &Option<T>) {
        if over.is_some() {
            field.clone_from(over);
        }
    }
    set(&mut format.font_family, &over.font_family);
    set(&mut format.font_point_size, &over.font_point_size);
    set(&mut format.font_weight, &over.font_weight);
    set(&mut format.font_bold, &over.font_bold);
    set(&mut format.font_italic, &over.font_italic);
    set(&mut format.font_underline, &over.font_underline);
    set(&mut format.font_overline, &over.font_overline);
    set(&mut format.font_strikeout, &over.font_strikeout);
    set(&mut format.letter_spacing, &over.letter_spacing);
    set(&mut format.word_spacing, &over.word_spacing);
    set(&mut format.underline_style, &over.underline_style);
    set(&mut format.vertical_alignment, &over.vertical_alignment);
    set(&mut format.foreground_color, &over.foreground_color);
    set(&mut format.background_color, &over.background_color);
    set(&mut format.underline_color, &over.underline_color);
}

/// Insert a paragraph break at the collapsed caret with `insert`, and carry
/// the caret's format across it: the format the text typed before the break
/// had (the character before the caret, with the pending toggles over it)
/// becomes the pending format at the start of the new, empty paragraph, as
/// Word, TextEdit and a `QTextEdit` (the new block's character format) carry
/// it. So bold typing goes on bold after Enter, and the toggle reads On.
///
/// Only into an empty paragraph: one that starts with text (Enter in the
/// middle of a run) types in the format of that text's first character,
/// which text-document gives it. The link is not carried. With a selection,
/// which the break replaces, nothing is.
pub(crate) fn insert_paragraph(st: &mut EditorState, insert: impl FnOnce(&mut EditorState)) {
    super::frame_loop::flush_pending_chars(st);
    let mut carried = TextFormat::default();
    if !st.cursor.has_selection() {
        overlay(&mut carried, &caret_format(st));
    }
    st.typing_format = None;
    insert(st);
    let empty = st.cursor.current_block_is_empty();
    if let Some(at) = collapsed_caret(st)
        && empty
        && carried != TextFormat::default()
    {
        st.typing_format = Some(TypingFormat {
            at,
            format: carried,
        });
    }
}

/// Keep `toggle` at `value` for the text typed next at the collapsed caret.
/// What was typed in this frame before it goes in first, without it.
pub(crate) fn keep(st: &mut EditorState, toggle: FormatToggle, value: bool) {
    super::frame_loop::flush_pending_chars(st);
    let Some(at) = collapsed_caret(st) else {
        return;
    };
    let mut format = pending(st).cloned().unwrap_or_default();
    toggle.set(&mut format, value);
    st.typing_format = Some(TypingFormat { at, format });
    st.bump_format_version();
}

/// Flip `toggle` for the text typed next at the collapsed caret, from the
/// format that text would have had.
pub(crate) fn toggle(st: &mut EditorState, toggle: FormatToggle) {
    super::frame_loop::flush_pending_chars(st);
    let current = toggle.get(&caret_format(st)).unwrap_or(false);
    keep(st, toggle, !current);
}

/// A format command on a collapsed caret: kept for the text typed next when
/// it sets exactly one toggle. Returns whether it was kept; anything else
/// formats the selection, which on a collapsed caret is nothing.
pub(crate) fn keep_command(st: &mut EditorState, format: &TextFormat) -> bool {
    if st.cursor.has_selection() {
        return false;
    }
    let Some((toggle, value)) = FormatToggle::of(format) else {
        return false;
    };
    keep(st, toggle, value);
    true
}

/// Make the insertion `insert` performs at the caret take the pending toggles,
/// as one undo step with it. `insert` leaves the caret after what it inserted
/// and returns how many characters that was. Without pending toggles, it is
/// only `insert`.
pub(crate) fn insert_with(st: &mut EditorState, insert: impl FnOnce(&mut EditorState) -> usize) {
    let Some(format) = pending(st).cloned() else {
        st.typing_format = None;
        insert(st);
        return;
    };
    st.typing_format = None;
    st.cursor.begin_edit_block();
    let inserted = insert(st);
    if inserted > 0 {
        let end = st.cursor.position();
        let range = st.document.cursor();
        range.set_position(end - inserted.min(end), MoveMode::MoveAnchor);
        range.set_position(end, MoveMode::KeepAnchor);
        let _ = range.merge_char_format(&format);
    }
    st.cursor.end_edit_block();
}

/// The character format a toolbar shows: the selection's, read at its start,
/// or on a collapsed caret the format the next typed character takes
/// ([`caret_format`]).
pub(crate) fn shown_format(st: &EditorState) -> TextFormat {
    if !st.cursor.has_selection() {
        return caret_format(st);
    }
    // Read through a fresh cursor so the editor's own is not moved. The start
    // of the selection, not its end: the end may sit past the last character.
    let probe = st.document.cursor();
    probe.set_position(st.cursor.selection_start(), MoveMode::MoveAnchor);
    probe.char_format().unwrap_or_default()
}

/// A character-format command from the editor's API: merged into the
/// selection, or kept for the text typed next when the caret is collapsed and
/// the command sets one of the toggles ([`keep_command`]).
pub(crate) fn apply_command(st: &mut EditorState, format: &TextFormat) {
    if keep_command(st, format) {
        return;
    }
    super::frame_loop::flush_pending_chars(st);
    let _ = st.cursor.merge_char_format(format);
}
