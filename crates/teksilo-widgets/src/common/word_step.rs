// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a word step forward takes the caret.
//!
//! text-document's forward word moves (`WordRight`, `NextWord`, `EndOfWord`)
//! all stop at the end of a word. That is macOS's `⌥→`. Windows and Linux
//! editors stop at the start of the next word instead, past the rest of this
//! one and the spaces after it, and a screen reader then reads the word the
//! caret lands in: stopping at a word's end, NVDA read the word just left a
//! second time and never landed on the first word of a wrapped line.
//!
//! So the editors run their moves through [`move_cursor`], which reads
//! `WordRight` as the platform does ([`forward_word_stop_for`]) and passes
//! every other operation through unchanged. The words are the Unicode words
//! text-document's backward word move stops at, so `Ctrl+→` and `Ctrl+←` stop
//! at the same word starts: punctuation between two words (the hyphen of
//! "Pall-Mall") is skipped either way.

use teksilo_text::text_document::{MoveMode, MoveOperation, TextCursor, TextDocument};
use unicode_segmentation::UnicodeSegmentation;

use super::text_nav::{ForwardWordStop, TextNavConvention, forward_word_stop_for};

/// The character offset in `text` at which the first word after character
/// `offset` starts, if one does.
pub(crate) fn next_word_start(text: &str, offset: usize) -> Option<usize> {
    let mut chars = 0;
    let mut counted_to = 0;
    for (byte, _) in text.unicode_word_indices() {
        chars += text[counted_to..byte].chars().count();
        counted_to = byte;
        if chars > offset {
            return Some(chars);
        }
    }
    None
}

/// Run `op` on `cursor`, a word step forward (`WordRight`) stopping where
/// this platform's editors stop. `document` is the one `cursor` edits.
pub(crate) fn move_cursor(
    document: &TextDocument,
    cursor: &TextCursor,
    op: MoveOperation,
    mode: MoveMode,
) -> bool {
    move_cursor_for(TextNavConvention::CURRENT, document, cursor, op, mode)
}

/// [`move_cursor`] against an explicit convention.
pub(crate) fn move_cursor_for(
    convention: TextNavConvention,
    document: &TextDocument,
    cursor: &TextCursor,
    op: MoveOperation,
    mode: MoveMode,
) -> bool {
    if op != MoveOperation::WordRight
        || forward_word_stop_for(convention) == ForwardWordStop::WordEnd
    {
        return cursor.move_position(op, mode, 1);
    }
    let from = cursor.position();
    match next_word_start_from(document, from) {
        Landing::At(target) => {
            cursor.set_position(target, mode);
            cursor.position() != from
        }
        Landing::By(op) => cursor.move_position(op, mode, 1),
    }
}

/// Where the start of the next word is, from a caret at `position`.
enum Landing {
    /// A position in the caret's own paragraph.
    At(usize),
    /// Where text-document's own move goes.
    By(MoveOperation),
}

fn next_word_start_from(document: &TextDocument, position: usize) -> Landing {
    let Ok(block) = document.block_at_caret(position) else {
        return Landing::By(MoveOperation::WordRight);
    };
    let end = block.start + block.length;
    // A caret on a table's anchor stands in the first cell, before that
    // cell's text: text-document's own move knows how to leave it.
    if position < block.start || position > end {
        return Landing::By(MoveOperation::WordRight);
    }
    // At the end of a paragraph, the start of the next one, as Right goes
    // (and into a table's first cell the way Right does): the next word may
    // be paragraphs away, and every paragraph break is a stop.
    if position == end {
        return Landing::By(MoveOperation::NextCharacter);
    }
    let Ok(text) = document.text_at(block.start, block.length) else {
        return Landing::By(MoveOperation::WordRight);
    };
    // No word left in the paragraph: its end, past the last word and
    // whatever follows it.
    let in_block = next_word_start(&text, position - block.start).unwrap_or(block.length);
    Landing::At(block.start + in_block)
}

#[cfg(test)]
mod tests {
    use super::TextNavConvention::{Mac, PcStyle};
    use super::*;

    #[test]
    fn the_next_word_starts_past_the_rest_of_this_one_and_the_spaces() {
        let text = "quinze fois  son";
        assert_eq!(next_word_start(text, 0), Some(7), "from a word's start");
        assert_eq!(next_word_start(text, 3), Some(7), "from inside it");
        assert_eq!(next_word_start(text, 6), Some(7), "from its end");
        assert_eq!(next_word_start(text, 7), Some(13), "over two spaces");
        assert_eq!(next_word_start(text, 13), None, "none after the last");
    }

    #[test]
    fn punctuation_between_words_is_not_a_stop() {
        assert_eq!(next_word_start("Pall-Mall", 1), Some(5));
        assert_eq!(next_word_start("fin. Suite", 0), Some(5));
        // An apostrophe between letters is inside the word, as Ctrl+Left reads it.
        assert_eq!(next_word_start("l'homme qui", 0), Some(8));
    }

    #[test]
    fn offsets_count_characters_not_bytes() {
        assert_eq!(next_word_start("déjà été là", 1), Some(5));
        assert_eq!(next_word_start("déjà été là", 5), Some(9));
    }

    fn cursor_at(document: &TextDocument, position: usize) -> TextCursor {
        let cursor = document.cursor();
        cursor.set_position(position, MoveMode::MoveAnchor);
        cursor
    }

    fn step(convention: TextNavConvention, text: &str, from: usize) -> usize {
        let document = TextDocument::new();
        document.set_plain_text(text).unwrap();
        let cursor = cursor_at(&document, from);
        move_cursor_for(
            convention,
            &document,
            &cursor,
            MoveOperation::WordRight,
            MoveMode::MoveAnchor,
        );
        cursor.position()
    }

    #[test]
    fn a_pc_word_step_lands_on_the_next_words_start() {
        let text = "cinq quinze fois son";
        assert_eq!(step(PcStyle, text, 7), 12, "inside quinze, to fois");
        assert_eq!(step(PcStyle, text, 12), 17, "from fois, to son");
        assert_eq!(step(PcStyle, text, 11), 12, "from a word's end");
    }

    #[test]
    fn a_mac_word_step_still_stops_at_the_words_end() {
        let text = "cinq quinze fois son";
        assert_eq!(step(Mac, text, 7), 11, "the end of quinze");
        assert_eq!(step(Mac, text, 11), 16, "the end of fois");
    }

    /// The landings the editors' and the field's word tests expect, on both
    /// conventions. Those tests press the word chord of the host they run on,
    /// so on Linux and Windows they never reach the Mac arm: this does.
    #[test]
    fn the_widget_tests_landings_on_both_conventions() {
        let text = "alpha beta gamma";
        assert_eq!((step(PcStyle, text, 2), step(Mac, text, 2)), (6, 5));
        assert_eq!((step(PcStyle, text, 0), step(Mac, text, 0)), (6, 5));
        assert_eq!((step(PcStyle, text, 6), step(Mac, text, 5)), (11, 10));
        let text = "my long pass";
        assert_eq!((step(PcStyle, text, 5), step(Mac, text, 5)), (8, 7));
        // Inside the word before a wrap: the next word starts the next line,
        // and the word's end is before the space ending this one.
        let text = "word word word";
        assert_eq!((step(PcStyle, text, 7), step(Mac, text, 7)), (10, 9));

        for (convention, left) in [(PcStyle, "albeta gamma"), (Mac, "al beta gamma")] {
            let document = TextDocument::new();
            document.set_plain_text("alpha beta gamma").unwrap();
            let cursor = cursor_at(&document, 2);
            move_cursor_for(
                convention,
                &document,
                &cursor,
                MoveOperation::WordRight,
                MoveMode::KeepAnchor,
            );
            cursor.remove_selected_text().unwrap();
            assert_eq!(document.to_plain_text().unwrap(), left, "{convention:?}");
        }
    }

    #[test]
    fn a_pc_word_step_stops_at_a_paragraphs_end_and_then_the_next_start() {
        let text = "un deux.\n  trois";
        assert_eq!(step(PcStyle, text, 3), 8, "past the last word to the end");
        assert_eq!(step(PcStyle, text, 8), 9, "the next paragraph's start");
        assert_eq!(step(PcStyle, text, 9), 11, "its first word");
        assert_eq!(step(PcStyle, text, 16), 16, "the end of the text stays");
    }

    #[test]
    fn a_pc_word_step_selects_up_to_the_next_words_start() {
        let document = TextDocument::new();
        document.set_plain_text("quinze fois son").unwrap();
        let cursor = cursor_at(&document, 2);
        move_cursor_for(
            PcStyle,
            &document,
            &cursor,
            MoveOperation::WordRight,
            MoveMode::KeepAnchor,
        );
        assert_eq!((cursor.anchor(), cursor.position()), (2, 7));
    }

    #[test]
    fn every_other_move_is_text_documents_own() {
        let document = TextDocument::new();
        document.set_plain_text("quinze fois son").unwrap();
        for op in [
            MoveOperation::WordLeft,
            MoveOperation::Right,
            MoveOperation::EndOfBlock,
        ] {
            let ours = cursor_at(&document, 9);
            let theirs = cursor_at(&document, 9);
            move_cursor_for(PcStyle, &document, &ours, op, MoveMode::MoveAnchor);
            theirs.move_position(op, MoveMode::MoveAnchor, 1);
            assert_eq!(ours.position(), theirs.position(), "{op:?}");
        }
    }
}
