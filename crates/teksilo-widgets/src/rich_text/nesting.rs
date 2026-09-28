// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The nesting gestures and where they stop.
//!
//! Three gestures put a block deeper than it was. Tab in a blockquote, and
//! [`RichTextEditor::increase_blockquote_depth`], wrap it in one more quote. The
//! wrapping half of [`RichTextEditor::toggle_blockquote`] does the same to a
//! block that is not quoted yet. Tab in a list, and [`RichTextEditor::indent`],
//! move the item one level down. Held down, Tab used to go on for as long as
//! the key repeated, building a structure nobody can read and that some
//! formats cannot load back: a Djot parser recurses once per blockquote and
//! once per list item, and a loader that bounds that recursion refuses the
//! document.
//!
//! Each gesture now asks this module first, and does nothing past
//! [`MAX_BLOCKQUOTE_DEPTH`] or [`MAX_LIST_DEPTH`]:
//!
//! * A wrap takes a whole selection with it, so it is judged by the deepest
//!   quote the selection holds, not by the caret's.
//! * At a ceiling Tab leaves the text as it was rather than typing a tab
//!   character, and the default context menu greys its blockquote row out.
//! * The gestures that take a block back out (Shift+Tab, `outdent`,
//!   `decrease_blockquote_depth`, Backspace at the start of a quote or a list
//!   item) are not limited by depth, and a document loaded deeper than the
//!   ceilings opens and edits as it is.
//! * A list item moves between levels, and stops at the list ceiling, in a
//!   quote or a table cell as it does in the main text.
//!
//! The keyboard, the public commands and the context menu all ask the same
//! functions, so a greyed-out row is exactly a command that would do nothing.
//!
//! [`RichTextEditor::increase_blockquote_depth`]: super::RichTextEditor::increase_blockquote_depth
//! [`RichTextEditor::toggle_blockquote`]: super::RichTextEditor::toggle_blockquote
//! [`RichTextEditor::indent`]: super::RichTextEditor::indent

use teksilo_text::text_document::{FlowElement, MoveMode, TextFrame};

use super::state::EditorState;

/// The most blockquotes a gesture may leave a block inside. A top-level quote
/// is depth 1.
///
/// Together with [`MAX_LIST_DEPTH`] this bounds the deepest structure the
/// gestures can build: a level-16 list item inside 64 quotes. A Djot parser
/// recurses once per blockquote and once per list item, so that line costs it
/// 80 levels, 16 below a loader that refuses anything past 96.
pub(super) const MAX_BLOCKQUOTE_DEPTH: usize = 64;

/// The deepest list level a gesture may move an item to. A top-level item is
/// level 1; word processors stop list nesting at nine or ten levels. Creating
/// a list is not a nesting gesture: it always makes a level-1 item.
pub(super) const MAX_LIST_DEPTH: usize = 16;

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

/// The list indent a nesting gesture would move the caret's item to, or `None`
/// where the item may not go deeper: outside a list, or at [`MAX_LIST_DEPTH`].
///
/// `ListFormat::indent` counts from 0, so an item's level is its indent plus 1.
pub(super) fn deeper_list_indent(st: &EditorState) -> Option<u8> {
    let list = st.cursor.current_list()?;
    let target = list.indent().checked_add(1)?;
    (usize::from(target) < MAX_LIST_DEPTH).then_some(target)
}

// ---------------------------------------------------------------------------
// Blockquotes
// ---------------------------------------------------------------------------

/// The most blockquotes that any block a wrap at the caret would enclose
/// already sits inside, or `None` where there is nothing to wrap: a selection
/// whose two ends sit in different frames, which the document model refuses
/// to wrap.
///
/// A wrap encloses every block from the one at the selection's start to the
/// one at its end, and any quote between them goes along, one level deeper.
/// A selection holding a deeper quote is therefore judged by that quote rather
/// than by the caret. Without a selection the wrap encloses the caret's own
/// block.
///
/// The answer costs a fixed number of document-wide lookups however much is
/// selected. Both ends sit directly in one frame, so every selected block that
/// is not inside a quote the selection holds is exactly as deep as the caret's
/// block, and only the frames between the two ends are walked, each once.
/// Asking each selected block for its depth instead costs a document-wide
/// lookup per block, which the default context menu used to pay on every
/// right-click: about half a second in a release build for a select-all over
/// twenty thousand paragraphs.
///
/// A table between the ends adds nothing, because nothing the editor does can
/// put a quote inside a table cell. The wrap cannot reach a paragraph in a
/// cell: each cell has a frame of its own, outside the flow text-document
/// searches for the blocks to wrap. A quote pasted or dropped into a cell
/// arrives as plain paragraphs, and an HTML import keeps the text of a quote
/// inside a cell but not the quote. A cell is therefore as deep as the frame
/// its table sits in. A host can still mark a cell's own frame as a quote
/// through `TextCursor::set_frame_format`; this walk does not count that mark,
/// and neither does a Djot export, which writes a cell's text inline.
/// `nesting_tests` holds each of these facts, so the text-document release
/// that first lets a quote into a cell fails there, and this walk must then go
/// into the cells as well.
pub(super) fn deepest_quote_under_wrap(st: &EditorState) -> Option<usize> {
    let (position, anchor) = (st.cursor.position(), st.cursor.anchor());
    // The caret's depth is the answer without a selection.
    let caret_depth = st.cursor.blockquote_depth_at_cursor();
    if position == anchor {
        return Some(caret_depth);
    }
    let (start, end) = (position.min(anchor), position.max(anchor));
    // The same two blocks the wrap itself resolves the selection to: the start
    // read as a character index, the end as a caret, which at the end of a
    // paragraph is still in that paragraph rather than in the next one.
    let first = st.document.block_at_position(start)?;
    let last = st
        .document
        .block_at_caret(end)
        .ok()
        .and_then(|info| st.document.block_by_id(info.block_id))?;
    let frame = first.frame();
    if last.frame().id() != frame.id() {
        return None;
    }
    // Both ends sit in `frame`, so its depth is the depth of the block the
    // selection ends in. A caret on the selection's start can stand at the end
    // of the paragraph before `first`, in another frame, so its own depth is
    // not the answer there.
    let frame_depth = if position == end {
        caret_depth
    } else {
        let probe = st.document.cursor();
        probe.set_position(end, MoveMode::MoveAnchor);
        probe.blockquote_depth_at_cursor()
    };
    let (first, last) = (first.id(), last.id());
    let mut enclosed = Vec::new();
    let mut inside = false;
    for element in frame.flow() {
        match element {
            FlowElement::Block(block) => {
                inside |= block.id() == first;
                if block.id() == last {
                    break;
                }
            }
            FlowElement::Frame(sub_frame) if inside => enclosed.push(sub_frame),
            FlowElement::Frame(_) | FlowElement::Table(_) => {}
        }
    }
    Some(frame_depth + deepest_quote_within(enclosed))
}

/// How many quotes deep the deepest frame among `frames` and everything
/// inside them sits, counting from the frame that holds them: 1 for a quote
/// with no quote inside it, 0 for a frame that holds none.
///
/// Walked with a stack of its own rather than by recursion, since a loaded
/// document may nest far deeper than the gestures ever build.
fn deepest_quote_within(frames: Vec<TextFrame>) -> usize {
    let mut pending: Vec<(TextFrame, usize)> = frames.into_iter().map(|f| (f, 0)).collect();
    let mut deepest = 0;
    while let Some((frame, above)) = pending.pop() {
        let depth = above + usize::from(frame.format().is_blockquote == Some(true));
        deepest = deepest.max(depth);
        for element in frame.flow() {
            if let FlowElement::Frame(sub_frame) = element {
                pending.push((sub_frame, depth));
            }
        }
    }
    deepest
}

/// Whether a wrap at the caret would happen and keep every block it encloses
/// within [`MAX_BLOCKQUOTE_DEPTH`].
fn quote_wrap_fits(st: &EditorState) -> bool {
    deepest_quote_under_wrap(st).is_some_and(|deepest| deepest < MAX_BLOCKQUOTE_DEPTH)
}

/// Whether Tab in a quote, or `increase_blockquote_depth`, would wrap
/// anything: the caret is in a quote and the wrap stays within the ceiling.
pub(super) fn deeper_quote_is_available(st: &EditorState) -> bool {
    st.cursor.is_in_blockquote() && quote_wrap_fits(st)
}

/// Tab in a quote, and `increase_blockquote_depth`: wrap the caret's block (or
/// the selection) in one more quote. Does nothing outside a quote, or where the
/// wrap would take a block past the ceiling.
pub(super) fn increase_blockquote_depth(st: &EditorState) {
    if deeper_quote_is_available(st) {
        let _ = st.cursor.increase_blockquote_depth();
    }
}

/// `toggle_blockquote`: unwrap the innermost quote around the caret, or wrap
/// the caret's block (or the selection) in a new one. The unwrap is never
/// limited; the wrap does nothing where it would take a block past the
/// ceiling.
pub(super) fn toggle_blockquote(st: &EditorState) {
    if st.cursor.is_in_blockquote() || quote_wrap_fits(st) {
        let _ = st.cursor.toggle_blockquote();
    }
}

/// Whether the toggle would do anything, the command filter aside: never over
/// a selection that crosses a frame boundary, always as an unwrap, and as a
/// wrap only within the ceiling.
///
/// The default context menu greys its row out by this. It checks the filter
/// separately, because a filter that refuses the toggle leaves the row out of
/// the menu rather than greying it.
pub(super) fn toggle_is_available(st: &EditorState) -> bool {
    !st.cursor.selection_spans_multiple_frames()
        && (st.cursor.is_in_blockquote() || quote_wrap_fits(st))
}
