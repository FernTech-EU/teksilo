// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The editor's document as the platform's input system writes it, through
//! the window's registry as a bridge reaches it: writes outside and inside a
//! composition, the undo step a composition leaves, the batch holding back
//! what the application sees, the journal and its cause.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::ImeOrigin;
use teksilo_core::text_input_document::{
    ChangeCause, Landed, TextInputDocument, TextInputEditor, WriteOptions, WriteRefused,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::{EditorHandle, RichTextEditor};
use crate::EditSource;

struct Editor {
    tree: WidgetTree,
    id: WidgetId,
    handle: EditorHandle,
    doc: TextDocument,
    arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
}

impl Editor {
    fn with_text(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let arrivals = Rc::new(RefCell::new(Vec::new()));
        let seen = arrivals.clone();
        let editor = RichTextEditor::editor(doc.clone())
            .on_text_inserted(move |source, chars| seen.borrow_mut().push((source, chars)));
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(id);
        let mut editor = Self {
            tree,
            id,
            handle,
            doc,
            arrivals,
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
    }

    /// The editor through the registry, as the platform reaches it.
    fn edit<R>(&self, f: impl FnOnce(&dyn TextInputEditor) -> R) -> R {
        self.tree
            .text_surfaces()
            .with_focused_input_editor(f)
            .expect("an editor that takes text hands out its writing half")
    }

    fn text(&self) -> String {
        self.doc.to_plain_text().unwrap()
    }
}

#[test]
fn a_write_lands_as_one_step_reported_as_the_input_systems() {
    let mut editor = Editor::with_text("abc");
    let before = editor.handle.revisions().unwrap();
    let landed = editor
        .edit(|e| e.replace_text(1..2, "XY", WriteOptions::default()))
        .unwrap();
    assert_eq!(
        landed,
        Landed {
            inserted: 1..3,
            replaced: 1..2
        }
    );
    assert_eq!(editor.text(), "aXYc");
    assert_eq!(*editor.arrivals.borrow(), vec![(EditSource::Ime, 2)]);
    let change = editor.handle.text_changed_since(before.text).unwrap();
    assert_eq!((change.start, change.old_end, change.new_end), (1, 2, 3));
    assert_eq!(change.cause, ChangeCause::InputSystem);
    editor.settle();
    editor.doc.undo().unwrap();
    assert_eq!(editor.text(), "abc");
}

#[test]
fn a_dry_run_and_a_bad_range_change_nothing() {
    let editor = Editor::with_text("abc");
    let dry = WriteOptions {
        dry_run: true,
        ..WriteOptions::default()
    };
    assert_eq!(
        editor.edit(|e| e.replace_text(0..1, "zz", dry)),
        Ok(Landed {
            inserted: 0..2,
            replaced: 0..1
        })
    );
    assert_eq!(
        editor.edit(|e| e.replace_text(2..9, "z", WriteOptions::default())),
        Err(WriteRefused::InvalidRange)
    );
    assert_eq!(editor.text(), "abc");
    assert!(editor.arrivals.borrow().is_empty());
}

/// A text service's composition: written where it says, marked, ended. None
/// of its states is reported or recorded; the end is one step, reported once.
#[test]
fn a_composition_is_written_marked_and_ended_as_one_step() {
    let mut editor = Editor::with_text("ab");
    editor.edit(|e| {
        e.begin_input_batch();
        e.mark_composition(2..2, &[]).unwrap();
        for (range, text) in [(2..2, "ｎ"), (2..3, "に"), (3..3, "ほ"), (2..4, "日本")] {
            e.replace_text(range, text, WriteOptions::default())
                .unwrap();
        }
        e.end_input_batch();
    });
    assert_eq!(editor.text(), "ab日本");
    assert_eq!(editor.handle.composition_range(), Some(2..4));
    assert!(editor.arrivals.borrow().is_empty(), "nothing arrived yet");
    editor
        .edit(|e| e.end_composition(ImeOrigin::Composition))
        .unwrap();
    assert_eq!(editor.handle.composition_range(), None);
    assert_eq!(*editor.arrivals.borrow(), vec![(EditSource::Ime, 2)]);
    editor.settle();
    editor.doc.undo().unwrap();
    assert_eq!(editor.text(), "ab", "one undo takes the whole composition");
}

/// A reconversion: the text service marks recorded text as its composition
/// and writes over it; the end records the result, and an undo gives the
/// original back.
#[test]
fn a_reconversion_composes_over_recorded_text() {
    let mut editor = Editor::with_text("ab日本");
    editor.edit(|e| {
        e.mark_composition(2..4, &[]).unwrap();
        e.replace_text(2..4, "二本", WriteOptions::default())
            .unwrap();
        e.end_composition(ImeOrigin::Composition).unwrap();
    });
    assert_eq!(editor.text(), "ab二本");
    editor.settle();
    editor.doc.undo().unwrap();
    assert_eq!(editor.text(), "ab日本");
}

/// Inside a batch the application sees nothing: the arrivals and the caret's
/// signals wait for its end.
#[test]
fn the_batch_holds_back_what_the_application_sees() {
    let editor = Editor::with_text("abc");
    let caret = editor.handle.cursor_position_signal();
    let before = caret.get();
    editor.edit(|e| {
        e.begin_input_batch();
        e.replace_text(3..3, "d", WriteOptions::default()).unwrap();
        e.set_selection(0..1, false).unwrap();
    });
    assert!(editor.arrivals.borrow().is_empty(), "held back");
    assert_eq!(caret.get(), before, "the caret signal waits too");
    editor.edit(|e| e.end_input_batch());
    assert_eq!(*editor.arrivals.borrow(), vec![(EditSource::Ime, 1)]);
    assert_eq!(caret.get(), 1);
}

/// Typing waiting for the frame tick makes the editor busy; inserting it
/// where a change may be reported makes it writable again, and that change
/// is not the input system's.
#[test]
fn typing_waiting_is_flushed_first_and_is_not_the_input_systems() {
    let mut editor = Editor::with_text("ab");
    editor.handle.select_range(2, 2);
    let start = editor.handle.revisions().unwrap().text;
    editor.tree.type_text(editor.id, "c");
    assert_eq!(
        editor.edit(|e| e.replace_text(0..0, "x", WriteOptions::default())),
        Err(WriteRefused::Busy)
    );
    assert!(editor.handle.flush_pending_input());
    assert!(!editor.handle.flush_pending_input(), "nothing left");
    assert_eq!(
        editor.handle.text_changed_since(start).map(|c| c.cause),
        Some(ChangeCause::Other)
    );
    assert!(
        editor
            .edit(|e| e.replace_text(0..0, "x", WriteOptions::default()))
            .is_ok()
    );
    assert_eq!(editor.text(), "xabc");
    editor.settle();
}

#[test]
fn a_read_only_editor_hands_out_no_writing_half() {
    let doc = TextDocument::new();
    doc.set_plain_text("abc").unwrap();
    let mut tree = WidgetTree::new();
    let id = tree.add(RichTextEditor::read_only(doc));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    assert!(
        tree.text_surfaces()
            .with_focused_input_editor(|_| ())
            .is_none()
    );
}
