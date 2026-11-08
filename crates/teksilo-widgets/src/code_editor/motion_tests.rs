// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Caret motion through the laid-out text: Up, Down, Page Up, Page Down, Home
//! and End keep the visual column across a run of vertical moves and stop at
//! the end of the *visual* line, with one caret or several, wrapped or not.
//!
//! Every test drives real key events through a rendered tree, because the
//! motions read the engine's layout. Lines are made of one repeated letter so
//! that the same x is the same column whatever the font.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::CursorAffinity;
use teksilo_text::WrapMode;
use teksilo_text::text_document::{MoveMode, TextDocument};

use super::CodeEditor;
use super::state::SharedState;
use crate::common::text_nav::{WORD_MODIFIER, by_convention};

struct Driven {
    tree: WidgetTree,
    state: SharedState,
    doc: TextDocument,
    width: f32,
}

impl Driven {
    fn new(text: &str) -> Self {
        Self::with(text, WrapMode::None, 400.0)
    }

    fn with(text: &str, wrap: WrapMode, width: f32) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let editor = CodeEditor::new(doc.clone()).wrap_mode(wrap);
        let state = editor.state.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let root = tree.add(editor);
        tree.layout(SizeProposal::exact(width, 400.0));
        let _ = tree.render();
        let focus = tree
            .first_focusable_descendant(root)
            .expect("the editor is focusable");
        tree.focus(focus);
        let mut d = Self {
            tree,
            state,
            doc,
            width,
        };
        d.settle();
        d
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(SizeProposal::exact(self.width, 400.0));
        }
        let _ = self.tree.render();
    }

    fn key(&mut self, key: Key) {
        self.key_with(key, Modifiers::NONE);
    }

    fn key_with(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        let focused = self.tree.focused().expect("the editor is focused");
        self.tree.type_text(focused, text);
        self.settle();
    }

    fn put(&mut self, position: usize) {
        self.state
            .borrow()
            .cursor
            .set_position(position, MoveMode::MoveAnchor);
    }

    fn caret(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    fn anchor(&self) -> usize {
        self.state.borrow().cursor.anchor()
    }

    /// Where the caret at `position` is drawn: `[x, y, w, h]`.
    fn rect(&self, position: usize) -> [f32; 4] {
        self.state
            .borrow()
            .engine
            .caret_rect(position, CursorAffinity::Downstream)
    }

    fn carets(&self) -> Vec<usize> {
        self.state
            .borrow()
            .all_carets()
            .map(|c| c.position())
            .collect()
    }

    fn add_caret(&mut self, position: usize) {
        let c = self.doc.cursor();
        c.set_position(position, MoveMode::MoveAnchor);
        self.state.borrow_mut().extra_carets.push(c);
    }
}

/// `line` repeated: `"aaaaaaaa"` is eight columns.
fn a(n: usize) -> String {
    "a".repeat(n)
}

fn lines(parts: &[usize]) -> String {
    parts.iter().map(|n| a(*n)).collect::<Vec<_>>().join("\n")
}

// --- Up and Down ----------------------------------------------------------

/// The reported bug: Down went to column 0 of the next line, and Up likewise.
#[test]
fn down_and_up_keep_the_column() {
    let mut d = Driven::new(&lines(&[8, 8, 8]));
    d.put(6);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 9 + 6);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 18 + 6);
    d.key(Key::ArrowUp);
    assert_eq!(d.caret(), 9 + 6);
    d.key(Key::ArrowUp);
    assert_eq!(d.caret(), 6);
}

/// Through a shorter line the caret lands at its end and, on the next long
/// line, comes back to the column it started from.
#[test]
fn the_column_survives_a_short_line() {
    // Lines start at 0, 9, 11.
    let mut d = Driven::new(&lines(&[8, 1, 8]));
    d.put(6);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 9 + 1, "the short line's end");
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 11 + 6, "the column it started from");
    d.key(Key::ArrowUp);
    assert_eq!(d.caret(), 9 + 1);
    d.key(Key::ArrowUp);
    assert_eq!(d.caret(), 6);
}

#[test]
fn a_horizontal_move_ends_the_run() {
    let mut d = Driven::new(&lines(&[8, 1, 8]));
    d.put(6);
    d.key(Key::ArrowDown);
    d.key(Key::ArrowLeft);
    assert_eq!(d.caret(), 9);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 11, "column 0, not the old column 6");
}

/// Left and Right bring the caret back to where the run left it; the run is
/// over all the same, so the column to hold is the one it stands in now.
#[test]
fn a_horizontal_move_that_returns_to_the_same_place_still_ends_the_run() {
    let mut d = Driven::new(&lines(&[8, 2, 8]));
    d.put(6);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 9 + 2);
    d.key(Key::ArrowLeft);
    d.key(Key::ArrowRight);
    assert_eq!(d.caret(), 9 + 2);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 12 + 2, "the column it stands in, not 6");
}

#[test]
fn an_edit_ends_the_run() {
    let mut d = Driven::new(&lines(&[8, 1, 8]));
    d.put(6);
    d.key(Key::ArrowDown);
    d.type_text("b");
    assert_eq!(d.caret(), 9 + 2);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 12 + 2, "the column after the edit");
}

#[test]
fn shift_with_down_selects_and_keeps_the_column() {
    let mut d = Driven::new(&lines(&[8, 1, 8]));
    d.put(6);
    d.key_with(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!((d.anchor(), d.caret()), (6, 10));
    d.key_with(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!((d.anchor(), d.caret()), (6, 11 + 6));
    d.key_with(Key::ArrowUp, Modifiers::SHIFT);
    assert_eq!((d.anchor(), d.caret()), (6, 10));
}

/// Each caret keeps its own column.
#[test]
fn every_caret_keeps_its_own_column() {
    // Line starts: 0, 9, 11, 20, 22, 31.
    let mut d = Driven::new(&lines(&[8, 1, 8, 1, 8, 8]));
    d.put(6);
    d.add_caret(11 + 3);
    d.key(Key::ArrowDown);
    assert_eq!(d.carets(), vec![9 + 1, 20 + 1]);
    d.key(Key::ArrowDown);
    assert_eq!(d.carets(), vec![11 + 6, 22 + 3]);
    d.key(Key::ArrowUp);
    d.key(Key::ArrowUp);
    assert_eq!(d.carets(), vec![6, 11 + 3]);
}

/// A soft-wrapped line is several visual lines: Down goes to the next visual
/// line of the same paragraph, not to the next paragraph.
#[test]
fn down_follows_the_visual_lines_of_a_wrapped_paragraph() {
    let text = format!("{}\nend", "word ".repeat(24));
    let mut d = Driven::with(&text, WrapMode::Word, 160.0);
    let first_line_end = d
        .state
        .borrow()
        .engine
        .visual_line_range_at(0, CursorAffinity::Downstream)
        .expect("laid out")
        .1;
    assert!(first_line_end < text.len() - 4, "the paragraph wraps");
    d.put(7);
    let before = d.rect(7);
    d.key(Key::ArrowDown);
    let at = d.caret();
    assert!(
        at > first_line_end && at < text.find('\n').unwrap(),
        "the second visual line of the same paragraph, got {at}"
    );
    let after = d.rect(at);
    assert!(after[1] > before[1], "a line lower");
    assert!(
        (after[0] - before[0]).abs() < 10.0,
        "the same x: {} vs {}",
        after[0],
        before[0]
    );
    d.key(Key::ArrowUp);
    assert_eq!(d.caret(), 7);
}

// --- Page Up and Page Down ------------------------------------------------

#[test]
fn page_down_and_page_up_keep_the_column() {
    let mut parts = vec![8; 120];
    parts[1] = 1;
    let mut d = Driven::new(&lines(&parts));
    d.put(6);
    d.key(Key::ArrowDown);
    assert_eq!(d.caret(), 9 + 1);
    // Lines after the short one start at 11 and are nine apart.
    let column = |p: usize| (p - 11) % 9;
    d.key(Key::PageDown);
    let first = d.caret();
    assert!(first > 11 + 9 * 5, "it moved a page, got {first}");
    assert_eq!(column(first), 6, "the column it started from");
    d.key(Key::PageDown);
    let second = d.caret();
    assert!(second > first + 9 * 5);
    assert_eq!(column(second), 6);
    d.key(Key::PageUp);
    let back = d.caret();
    assert!(back < second && back > 11);
    assert_eq!(column(back), 6);
}

// --- End and Home ---------------------------------------------------------

/// The reported bug: End at a line's end jumped to the end of the next line.
#[test]
fn end_at_a_line_end_stays() {
    let mut d = Driven::new("abc\ndef\nghi");
    d.put(1);
    d.key(Key::End);
    assert_eq!(d.caret(), 3);
    d.key(Key::End);
    assert_eq!(d.caret(), 3, "End again stays");
    d.put(7);
    d.key(Key::End);
    assert_eq!(d.caret(), 7);
}

#[test]
fn shift_end_selects_to_the_line_end_and_no_further() {
    let mut d = Driven::new("abc\ndef");
    d.put(1);
    d.key_with(Key::End, Modifiers::SHIFT);
    assert_eq!((d.anchor(), d.caret()), (1, 3));
    d.key_with(Key::End, Modifiers::SHIFT);
    assert_eq!((d.anchor(), d.caret()), (1, 3));
}

#[test]
fn every_caret_goes_to_the_end_of_its_own_line() {
    let mut d = Driven::new("abc\nd\nefgh");
    d.put(3);
    d.add_caret(4);
    d.key(Key::End);
    assert_eq!(d.carets(), vec![3, 5]);
}

#[test]
fn home_at_a_line_end_goes_to_its_own_indent() {
    let mut d = Driven::new("  abc\n  def");
    d.put(5);
    d.key(Key::Home);
    assert_eq!(d.caret(), 2);
    d.key(Key::Home);
    assert_eq!(d.caret(), 0);
    d.put(11);
    d.key(Key::Home);
    assert_eq!(d.caret(), 8);
}

/// A paragraph that wraps in a 160 px editor, and where its first visual line
/// ends.
fn wrapped() -> (Driven, usize, usize) {
    let text = format!("{}\nend", "word ".repeat(24));
    let d = Driven::with(&text, WrapMode::Word, 160.0);
    let (_, first_end) = d
        .state
        .borrow()
        .engine
        .visual_line_range_at(0, CursorAffinity::Downstream)
        .expect("laid out");
    let paragraph_end = text.find('\n').unwrap();
    assert!(first_end < paragraph_end, "the paragraph wraps");
    (d, first_end, paragraph_end)
}

/// A caret Home left at the start of a wrapped line is on that line at both
/// ends of the selection: UIA's `GetSelection` range starts at the anchor, and a
/// reader expanding its line from there heard the line above.
#[test]
fn home_on_a_wrapped_line_is_reported_on_that_line() {
    let (mut d, first_end, _) = wrapped();
    d.put(first_end + 3);
    d.key(Key::Home);
    assert_eq!(d.caret(), first_end);

    let update = d.tree.sync_accessibility();
    let selection = update
        .nodes
        .iter()
        .find_map(|(_, node)| node.text_selection())
        .expect("a selection");
    assert_eq!(selection.anchor, selection.focus);
    assert_eq!(selection.focus.character_index, 0, "the line's first run");
    let st = d.state.borrow();
    let map = st.synthetic_to_element.borrow();
    assert_eq!(map[&selection.focus.node].absolute_start, first_end);
}

/// End stops where the line is drawn to end, at a soft wrap, and stays there.
#[test]
fn end_goes_to_the_end_of_the_visual_line() {
    let (mut d, first_end, paragraph_end) = wrapped();
    d.put(2);
    d.key(Key::End);
    assert_eq!(d.caret(), first_end, "the end of the first visual line");
    d.key(Key::End);
    assert_eq!(d.caret(), first_end, "End again stays");

    d.put(first_end + 3);
    d.key(Key::End);
    assert!(
        d.caret() > first_end + 3,
        "the end of the second visual line"
    );
    assert!(d.caret() < paragraph_end);
}

/// Home on a continuation line goes to where that visual line starts, not to
/// the start of the paragraph.
#[test]
fn home_goes_to_the_start_of_the_visual_line() {
    let (mut d, first_end, _) = wrapped();
    d.put(first_end + 3);
    d.key(Key::Home);
    assert_eq!(d.caret(), first_end);
    // On the first visual line it is the line's own Home.
    d.put(7);
    d.key(Key::Home);
    assert_eq!(d.caret(), 0);
}

/// Where a reader's Line unit starts for the caret, in document positions: the
/// run the selection's anchor is in, as UIA expands it.
fn reported_run_start(d: &mut Driven) -> (usize, usize) {
    let update = d.tree.sync_accessibility();
    let selection = update
        .nodes
        .iter()
        .find_map(|(_, node)| node.text_selection())
        .expect("a selection");
    let st = d.state.borrow();
    let map = st.synthetic_to_element.borrow();
    (
        map[&selection.anchor.node].absolute_start,
        selection.anchor.character_index,
    )
}

#[test]
fn left_and_right_stop_at_the_start_of_a_wrapped_line() {
    let (mut d, wrap, _) = wrapped();
    d.put(wrap + 1);
    d.key(Key::ArrowLeft);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "Left");
    d.key(Key::ArrowLeft);
    assert_eq!(d.caret(), wrap - 1);

    d.key(Key::ArrowRight);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "Right");
    d.key(Key::ArrowRight);
    assert_eq!(d.caret(), wrap + 1);
}

/// End leaves the caret at the end of the line; Right from there reaches the
/// next line's first character before its second.
#[test]
fn right_from_the_end_of_a_wrapped_line_reaches_the_start_of_the_next() {
    let (mut d, wrap, _) = wrapped();
    d.put(2);
    d.key(Key::End);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d).1, wrap, "End, at its run's end");
    d.key(Key::ArrowRight);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0));
    d.key(Key::ArrowRight);
    assert_eq!(d.caret(), wrap + 1);
}

#[test]
fn backspace_and_delete_stay_on_a_wrapped_line() {
    let (mut d, wrap, _) = wrapped();
    d.put(wrap + 1);
    d.key(Key::Backspace);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "Backspace");

    let (mut d, wrap, _) = wrapped();
    d.put(wrap + 1);
    d.key(Key::ArrowLeft);
    d.key(Key::Delete);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "Delete");
}

/// Undo after a Backspace at the start of a wrapped line puts the caret back
/// on that line, from the key and from the handle. It was reported on the line
/// above.
#[test]
fn undoing_a_change_at_a_wrap_puts_the_caret_on_the_line_below() {
    let (mut d, wrap, _) = wrapped();
    d.put(wrap + 1);
    d.key(Key::ArrowLeft);
    d.key(Key::Backspace);
    assert_eq!(d.caret(), wrap - 1);
    d.key_with(Key::Z, Modifiers::COMMAND);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "Ctrl+Z");

    let handle = super::CodeEditorHandle {
        state: d.state.clone(),
    };
    handle.redo();
    d.settle();
    handle.undo();
    d.settle();
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "the handle's undo");
}

/// An undo or a redo with nothing to take back moves nothing: End leaves the
/// caret at the end of the first line, at the offset the next line starts at,
/// and an undo that did nothing reported it at the start of the next line. From
/// the keys and from the handle.
#[test]
fn an_undo_with_nothing_to_undo_leaves_the_caret_on_its_line() {
    let (mut d, wrap, _) = wrapped();
    d.put(2);
    d.key(Key::End);
    assert_eq!(reported_run_start(&mut d), (0, wrap), "End, on its line");
    assert!(!d.doc.can_undo() && !d.doc.can_redo(), "no history");

    d.key_with(Key::Z, Modifiers::COMMAND);
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (0, wrap), "Ctrl+Z");
    d.key_with(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(reported_run_start(&mut d), (0, wrap), "Ctrl+Shift+Z");

    let handle = super::CodeEditorHandle {
        state: d.state.clone(),
    };
    handle.undo();
    d.settle();
    assert_eq!(reported_run_start(&mut d), (0, wrap), "the handle's undo");
    handle.redo();
    d.settle();
    assert_eq!(reported_run_start(&mut d), (0, wrap), "the handle's redo");
}

/// A composition's text is not on the undo stack, but every undo takes it out
/// first. From the handle, with nothing else to undo, that still moves the
/// caret, so its side of a wrap is set again: back at the start of the second
/// line, it is reported there.
#[test]
fn an_undo_that_takes_out_a_composition_puts_the_caret_on_its_line() {
    let (mut d, wrap, _) = wrapped();
    d.put(wrap + 2);
    d.key(Key::Home);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "Home");
    d.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "xyz".to_string(),
        cursor: None,
        replacement: None,
    });
    d.settle();
    assert!(d.doc.has_transient_edits() && !d.doc.can_undo());
    assert_eq!(d.caret(), wrap + 3);

    let handle = super::CodeEditorHandle {
        state: d.state.clone(),
    };
    handle.undo();
    d.settle();
    assert!(!d.doc.has_transient_edits(), "the composition is taken out");
    assert_eq!(d.caret(), wrap);
    assert_eq!(reported_run_start(&mut d), (wrap, 0), "the handle's undo");
}

/// The extra carets stay when an undo or a redo has nothing to take back: a
/// host's Edit menu calling the handle with an empty history dropped them.
#[test]
fn an_undo_with_nothing_to_undo_keeps_the_extra_carets() {
    let mut d = Driven::new("abc\ndef\nghi");
    d.put(1);
    d.add_caret(5);
    d.add_caret(9);
    let handle = super::CodeEditorHandle {
        state: d.state.clone(),
    };
    handle.undo();
    handle.redo();
    d.settle();
    assert_eq!(d.carets(), vec![1, 5, 9], "the handle");
    d.key_with(Key::Z, Modifiers::COMMAND);
    d.key_with(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(d.carets(), vec![1, 5, 9], "the keys");
}

/// Ctrl+Right stops at the next word's start, which may start a wrapped line:
/// the caret is then reported on that line. On macOS `⌥→` stops at the word's
/// end, before the space that ends the first line.
#[test]
fn a_word_step_forward_lands_on_the_next_words_start_on_its_line() {
    let (mut d, wrap, _) = wrapped();
    d.put(wrap - 3);
    d.key_with(Key::ArrowRight, WORD_MODIFIER);
    assert_eq!(d.caret(), by_convention(wrap, wrap - 1));
    assert_eq!(
        reported_run_start(&mut d),
        by_convention((wrap, 0), (0, wrap - 1))
    );
}

/// Ctrl+Shift+Right selects up to the next word's start, and Ctrl+Delete
/// removes the rest of the word and the space after it. On macOS `⌥⇧→` and
/// `⌥⌦` stop at the word's end.
#[test]
fn a_word_selection_and_a_word_delete_reach_the_next_words_start() {
    let mut d = Driven::new("alpha beta gamma");
    d.put(2);
    d.key_with(Key::ArrowRight, WORD_MODIFIER | Modifiers::SHIFT);
    assert_eq!((d.anchor(), d.caret()), (2, by_convention(6, 5)));

    d.put(2);
    d.key_with(Key::Delete, WORD_MODIFIER);
    assert_eq!(
        d.doc.to_plain_text().unwrap(),
        by_convention("albeta gamma", "al beta gamma")
    );
    assert_eq!(d.caret(), 2);
}

#[test]
fn left_and_right_with_a_selection_go_to_its_ends() {
    let (mut d, wrap, _) = wrapped();
    for (anchor, caret) in [(1, 4), (4, 1), (wrap - 2, wrap + 2), (wrap + 2, wrap - 2)] {
        let (start, end) = (anchor.min(caret), anchor.max(caret));
        d.put(anchor);
        d.state
            .borrow()
            .cursor
            .set_position(caret, MoveMode::KeepAnchor);
        d.key(Key::ArrowLeft);
        assert_eq!(
            (d.anchor(), d.caret()),
            (start, start),
            "Left {anchor}..{caret}"
        );
        d.put(anchor);
        d.state
            .borrow()
            .cursor
            .set_position(caret, MoveMode::KeepAnchor);
        d.key(Key::ArrowRight);
        assert_eq!(
            (d.anchor(), d.caret()),
            (end, end),
            "Right {anchor}..{caret}"
        );
    }
}

/// Every caret collapses to its own selection's end.
#[test]
fn left_with_several_selections_collapses_each_to_its_start() {
    let mut d = Driven::new("aaaaaaaa\nbbbbbbbb");
    d.put(2);
    d.state
        .borrow()
        .cursor
        .set_position(4, MoveMode::KeepAnchor);
    d.add_caret(11);
    d.state.borrow().extra_carets[0].set_position(13, MoveMode::KeepAnchor);
    d.key(Key::ArrowLeft);
    assert_eq!(d.carets(), vec![2, 11]);
    assert_eq!(d.anchor(), 2);
}

#[test]
fn a_selection_starting_at_a_wrap_is_on_the_next_line() {
    let (mut d, wrap, _) = wrapped();
    d.put(wrap);
    d.state
        .borrow()
        .cursor
        .set_position(wrap + 3, MoveMode::KeepAnchor);
    assert_eq!(reported_run_start(&mut d), (wrap, 0));
}
