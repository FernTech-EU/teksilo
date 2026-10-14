// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The list toolbar: which kind of list the caret is in, and the toggle a
//! "Bullets" or "Numbering" button performs.
//!
//! [`RichTextEditor::insert_list`] only ever *makes* a list, so a toolbar built
//! on it can neither show that the caret already sits in one nor take the list
//! away again. A word processor's two list buttons do both, and do it by kind,
//! not by exact style: a "Bullets" button is pressed in a circle-bullet list as
//! much as in a disc one, and pressing it there takes the paragraph out rather
//! than re-making the list. [`list_style_is_ordered`] is that classification,
//! and [`toggle_list`] is the press:
//!
//! * In a list of the same kind, the paragraphs leave it. Without a selection
//!   that is the caret's paragraph; with one, every selected paragraph that is
//!   in a list.
//! * In a list of the other kind, the list changes kind. Without a selection
//!   the whole list switches, keeping its depth, the way a word processor
//!   renumbers a bulleted list when Numbering is pressed in any of its items.
//!   With one, the selected paragraphs become one list of the new kind.
//! * Outside a list, the paragraphs become one, as [`insert_list`] does.
//!
//! Every branch is one undo step.
//!
//! [`RichTextEditor::insert_list`]: super::RichTextEditor::insert_list
//! [`insert_list`]: super::RichTextEditor::insert_list

use teksilo_text::text_document::{ListFormat, ListStyle};

use super::state::EditorState;

/// Whether `style` numbers its items (decimal, alphabetic, roman) rather than
/// marking them with a bullet (disc, circle, square).
///
/// The kind a toolbar's two list buttons stand for. Exhaustive on purpose: a
/// style added to the document model has to be given a kind here before
/// anything compiles.
pub fn list_style_is_ordered(style: &ListStyle) -> bool {
    match style {
        ListStyle::Disc | ListStyle::Circle | ListStyle::Square => false,
        ListStyle::Decimal
        | ListStyle::LowerAlpha
        | ListStyle::UpperAlpha
        | ListStyle::LowerRoman
        | ListStyle::UpperRoman => true,
    }
}

/// The style a toggle creates for `ordered`: decimal numbers or disc bullets,
/// the same pair [`insert_list`](super::RichTextEditor::insert_list) uses.
fn default_style(ordered: bool) -> ListStyle {
    if ordered {
        ListStyle::Decimal
    } else {
        ListStyle::Disc
    }
}

/// The style of the list the caret's paragraph belongs to, or `None` outside a
/// list.
pub(super) fn current_list_style(st: &EditorState) -> Option<ListStyle> {
    st.cursor.current_list().map(|list| list.style())
}

/// Give the caret's whole list `style`, keeping its depth. Does nothing
/// outside a list.
pub(super) fn set_list_style(st: &EditorState, style: ListStyle) {
    if st.cursor.current_list().is_none() {
        return;
    }
    let _ = st.cursor.set_current_list_format(&ListFormat {
        style: Some(style),
        ..Default::default()
    });
}

/// The press of a "Bullets" (`ordered == false`) or "Numbering"
/// (`ordered == true`) button. See the module documentation for what each case
/// does.
pub(super) fn toggle_list(st: &EditorState, ordered: bool) {
    let current = current_list_style(st);
    let has_selection = st.cursor.has_selection();
    st.cursor.begin_edit_block();
    match current {
        Some(style) if list_style_is_ordered(&style) == ordered => {
            if has_selection {
                remove_selected_blocks_from_lists(st);
            } else {
                let _ = st.cursor.remove_current_block_from_list();
            }
        }
        Some(_) if !has_selection => set_list_style(st, default_style(ordered)),
        // In a list of the other kind with a selection, or outside any list:
        // the selected paragraphs (or the caret's) become one new list. Blocks
        // leaving another list for it take that list away with them.
        _ => {
            let _ = st.cursor.create_list(default_style(ordered));
        }
    }
    st.cursor.end_edit_block();
}

/// Take every paragraph the selection touches out of whatever list it is in.
///
/// The selection's start is read as a character index and its end as a caret,
/// the reading [`create_list`](teksilo_text::text_document::TextCursor::create_list)
/// gives a selection: a selection ending at the end of a paragraph does not
/// reach into the next one.
fn remove_selected_blocks_from_lists(st: &EditorState) {
    let anchor = st.cursor.anchor();
    let position = st.cursor.position();
    let (start, end) = (anchor.min(position), anchor.max(position));
    let blocks = st.document.blocks_in_range(start, end - start);
    for block in blocks {
        if block.list().is_some() {
            let _ = st.cursor.remove_block_from_list(block.id());
        }
    }
}
