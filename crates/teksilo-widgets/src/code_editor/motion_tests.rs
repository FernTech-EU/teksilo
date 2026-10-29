// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Caret motion through the laid-out text: Up, Down, Page Up and Page Down keep
//! the visual column across a run of vertical moves, with one caret or several,
//! wrapped or not.
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
