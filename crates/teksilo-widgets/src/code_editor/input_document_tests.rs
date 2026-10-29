// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code editor's document as the platform's input system reads and
//! writes it, through the window's registry as a bridge reaches it.

use teksilo_canvas::SizeProposal;
use teksilo_core::ImeOrigin;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::text_input_document::{
    ChangeCause, TextInputDocument, TextInputEditor, WriteOptions,
};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::CodeEditorHandle;
use super::widget::CodeEditor;
use crate::common::ime_ops_test::ImeOps;

struct Editor {
    tree: WidgetTree,
    handle: CodeEditorHandle,
    doc: TextDocument,
    ops: ImeOps,
}

impl Editor {
    fn with_text(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let editor = CodeEditor::new(doc.clone());
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let focusable = tree.first_focusable_descendant(id).unwrap_or(id);
        tree.focus(focusable);
        let mut editor = Self {
            tree,
            handle,
            doc,
            ops: ImeOps::default(),
        };
        editor.settle();
        editor
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(SizeProposal::exact(400.0, 300.0));
        }
        let _ = self.tree.render();
    }

    fn text(&self) -> String {
        self.doc.to_plain_text().unwrap()
    }

    fn read<R>(&self, f: impl FnOnce(&dyn TextInputDocument) -> R) -> R {
        self.tree
            .text_surfaces()
            .with_focused_input_document(f)
            .expect("the code editor describes its document")
    }

    fn edit<R>(&self, f: impl FnOnce(&dyn TextInputEditor) -> R) -> R {
        self.tree
            .text_surfaces()
            .with_focused_input_editor(f)
            .expect("an editable code editor takes the input system's writes")
    }
}

#[test]
fn the_code_editor_describes_its_text_in_its_offsets() {
    let editor = Editor::with_text("fn main() {\n    x\n}");
    editor
        .handle
        .state
        .borrow()
        .cursor
        .set_position(13, teksilo_text::text_document::MoveMode::MoveAnchor);
    assert_eq!(editor.read(|d| d.char_count()), Some(19));
    assert_eq!(editor.read(|d| d.text_in(12..17)).as_deref(), Some("    x"));
    assert_eq!(editor.read(|d| d.selection()), Some(13..13));
    assert_eq!(editor.read(|d| d.utf16_index()).unwrap().utf16_len(), 19);
    assert!(editor.read(|d| d.is_editable()));
    assert!(!editor.read(|d| d.writing_tools_allowed()));
    let rect = editor.read(|d| d.first_line_rect(12..14)).unwrap();
    assert_eq!(rect.covered, 12..14);
    assert!(rect.rect.width > 0.0);
}

#[test]
fn a_write_lands_as_plain_code_and_is_the_input_systems() {
    let mut editor = Editor::with_text("ab");
    let before = editor.handle.revisions().unwrap().text;
    editor
        .edit(|e| e.replace_text(1..1, "x", WriteOptions::default()))
        .unwrap();
    let change = editor.handle.text_changed_since(before).unwrap();
    assert_eq!((change.start, change.new_end), (1, 2));
    assert_eq!(change.cause, ChangeCause::InputSystem);
    editor
        .edit(|e| e.replace_text(2..2, "\r\ny", WriteOptions::default()))
        .unwrap();
    assert_eq!(editor.text(), "ax\nyb", "a break is a line of its own");
    editor.settle();
    editor.doc.undo().unwrap();
    editor.doc.undo().unwrap();
    assert_eq!(editor.text(), "ab", "one step per write");
}

#[test]
fn a_written_composition_is_one_step_and_a_passed_key_ends_it_first() {
    let mut editor = Editor::with_text("let x = ;");
    editor.edit(|e| {
        e.begin_input_batch();
        e.mark_composition(8..8, &[]).unwrap();
        e.replace_text(8..8, "\u{306B}", WriteOptions::default())
            .unwrap();
        e.end_input_batch();
    });
    editor.edit(|e| {
        e.replace_text(8..9, "\u{65E5}\u{672C}", WriteOptions::default())
            .unwrap();
    });
    assert_eq!(editor.read(|d| d.composition_range()), Some(8..10));
    // A key the text service passed on ends it, its text kept.
    editor.tree.dispatch_event_with_ops(
        WidgetEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::NONE,
            text: None,
        },
        &mut editor.ops,
    );
    editor.settle();
    assert_eq!(editor.ops.finished, 1, "the platform is told");
    assert_eq!(editor.read(|d| d.composition_range()), None);
    assert_eq!(editor.text(), "let x = \u{65E5}\u{672C};");
    editor.doc.undo().unwrap();
    assert_eq!(editor.text(), "let x = ;", "one step");
}

#[test]
fn a_composition_the_input_system_ends_is_its_commit() {
    let mut editor = Editor::with_text("a");
    editor.edit(|e| {
        e.mark_composition(1..1, &[]).unwrap();
        e.replace_text(1..1, "\u{AC00}", WriteOptions::default())
            .unwrap();
        e.end_composition(ImeOrigin::Composition).unwrap();
    });
    editor.settle();
    assert_eq!(editor.text(), "a\u{AC00}");
    assert_eq!(editor.read(|d| d.composition_range()), None);
}

/// The paint draws the composition's underline from the same clauses the
/// rich editor and the text field draw: a thick line under the clause being
/// converted, a thin one under the rest, and one thin line for the IMM
/// path's composition.
#[test]
fn the_composition_is_underlined_clause_by_clause() {
    use teksilo_core::text_input_document::{CompositionSegment, SegmentStyle};
    let mut editor = Editor::with_text("x = ;");
    let strokes =
        |editor: &Editor| super::input_document::composition_strokes(&editor.handle.state.borrow());
    assert!(strokes(&editor).is_empty(), "no composition, no line");
    editor.edit(|e| {
        e.mark_composition(4..4, &[]).unwrap();
        e.replace_text(4..4, "\u{79C1}\u{306F}\u{65E5}", WriteOptions::default())
            .unwrap();
        e.mark_composition(
            4..7,
            &[
                CompositionSegment {
                    range: 4..6,
                    style: SegmentStyle::Target,
                },
                CompositionSegment {
                    range: 6..7,
                    style: SegmentStyle::Converted,
                },
            ],
        )
        .unwrap();
    });
    editor.settle();
    let lines = strokes(&editor);
    assert_eq!(
        lines.iter().map(|s| s.width).collect::<Vec<_>>(),
        vec![2.0, 1.0],
        "{lines:?}"
    );
    assert!(lines[0].x1 <= lines[1].x0, "in order, apart: {lines:?}");
    assert!(lines.iter().all(|s| s.x1 > s.x0));

    // The IMM path's composition: one thin line.
    let mut imm = Editor::with_text("x = ;");
    imm.handle
        .state
        .borrow()
        .cursor
        .set_position(4, teksilo_text::text_document::MoveMode::MoveAnchor);
    imm.tree.dispatch_event_with_ops(
        WidgetEvent::ImeComposition {
            text: "\u{306B}\u{307B}".into(),
            cursor: None,
            replacement: None,
        },
        &mut imm.ops,
    );
    imm.settle();
    let lines = strokes(&imm);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].width, 1.0);
}

/// Now that the editor describes its document, the macOS input system names
/// ranges in it: a commit over a range replaces it (the accent popup's
/// letter), and a composition over one shows in its place (Korean Hanja),
/// as in the text fields.
#[test]
fn a_range_the_input_system_names_is_replaced() {
    let mut editor = Editor::with_text("cafe;");
    editor.tree.dispatch_event_with_ops(
        WidgetEvent::ImeCommit {
            text: "\u{E9}".into(),
            origin: ImeOrigin::Insertion,
            replacement: Some(3..4),
        },
        &mut editor.ops,
    );
    editor.settle();
    assert_eq!(editor.text(), "caf\u{E9};");

    let mut hanja = Editor::with_text("\u{D55C};");
    hanja.tree.dispatch_event_with_ops(
        WidgetEvent::ImeComposition {
            text: "\u{97D3}".into(),
            cursor: None,
            replacement: Some(0..1),
        },
        &mut hanja.ops,
    );
    hanja.settle();
    assert_eq!(hanja.text(), "\u{97D3};", "in place of the syllable");
    hanja
        .tree
        .dispatch_event_with_ops(WidgetEvent::ime_commit("\u{97D3}"), &mut hanja.ops);
    hanja.settle();
    assert_eq!(hanja.text(), "\u{97D3};");
    hanja.doc.undo().unwrap();
    assert_eq!(
        hanja.text(),
        "\u{D55C};",
        "one undo gives the syllable back"
    );
}
