// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The editor's document as the platform's input system writes it, through
//! the window's registry as a bridge reaches it: writes outside and inside a
//! composition, the undo step a composition leaves, the batch holding back
//! what the application sees, the journal and its cause.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use teksilo_canvas::SizeProposal;
use teksilo_core::ImeOrigin;
use teksilo_core::event::{Key, Modifiers};
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
    let changes = std::sync::Arc::new(AtomicUsize::new(0));
    let _subscription = {
        let changes = changes.clone();
        editor.doc.on_change(move |_| {
            changes.fetch_add(1, Ordering::Relaxed);
        })
    };
    let start = editor.handle.revisions().unwrap().text;
    // An index built before the batch, which its events would invalidate.
    assert_eq!(editor.handle.utf16_index().unwrap().utf16_len(), 3);
    editor.edit(|e| {
        e.begin_input_batch();
        e.replace_text(3..3, "d\u{1F600}", WriteOptions::default())
            .unwrap();
        e.set_selection(0..1, false).unwrap();
    });
    assert!(editor.arrivals.borrow().is_empty(), "held back");
    assert_eq!(caret.get(), before, "the caret signal waits too");
    assert_eq!(
        changes.load(Ordering::Relaxed),
        0,
        "the document's events wait too"
    );
    assert_eq!(
        editor.handle.utf16_index().unwrap().utf16_len(),
        6,
        "the input system reads its write back meanwhile"
    );
    editor.edit(|e| e.end_input_batch());
    assert_eq!(*editor.arrivals.borrow(), vec![(EditSource::Ime, 2)]);
    assert_eq!(caret.get(), 1);
    assert!(
        changes.load(Ordering::Relaxed) > 0,
        "delivered as the batch ends"
    );
    assert_eq!(
        editor.handle.text_changed_since(start).map(|c| c.cause),
        Some(ChangeCause::InputSystem),
        "and logged as the input system's"
    );
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

/// The store's commit of a Japanese composition, as the Microsoft IME
/// writes it (one batch per lock: the start, each reading, the conversion
/// written again unchanged, the commit), then Ctrl+Z with nothing in
/// between: the whole composition goes, whether frames ran between the locks
/// or not, with a second view on the document (the demo's preview) and the
/// store's reads after each lock. Measured on Windows: three times in about
/// fifteen the undo did nothing until the caret moved; not reproduced here.
#[test]
fn an_undo_right_after_a_store_commit_takes_the_composition() {
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};
    use teksilo_core::text_input_document::{CompositionSegment, SegmentStyle};
    for variant in 0..32 {
        let doc = TextDocument::new();
        doc.set_markdown("# RichTextEditor \u{2014} Capability Showcase\n\nThis window hosts two RichTextEditor widgets bound to the same document.\n").unwrap();
        let first = RichTextEditor::editor(doc.clone());
        let handle = first.handle();
        let second = RichTextEditor::editor(doc.clone());
        let mut tree = WidgetTree::new();
        let a = tree.add(first);
        let b = tree.add(second);
        let root = tree.add(crate::primitives::VStack::new().child(a).child(b));
        let _ = root;
        tree.layout(SizeProposal::exact(300.0, 600.0));
        tree.focus(a);
        let settle = |tree: &mut WidgetTree| {
            for _ in 0..3 {
                tree.request_frame();
                tree.tick_animations(std::time::Duration::from_millis(16));
                tree.layout(SizeProposal::exact(300.0, 600.0));
            }
            let _ = tree.render();
        };
        settle(&mut tree);
        let at = 36;
        handle.select_range(at, at);
        settle(&mut tree);
        let doc_before_composition = doc.to_plain_text().unwrap();
        let read = |h: &EditorHandle| {
            let rev = h.revisions();
            let _ = h.utf16_index();
            let _ = TextInputDocument::text_in(h, 0..20);
            let _ = rev.map(|r| h.text_changed_since(r.text.saturating_sub(1)));
        };
        let lock = |f: &dyn Fn(&dyn TextInputEditor)| {
            handle.begin_input_batch();
            f(&handle);
            handle.end_input_batch();
            read(&handle);
        };
        lock(&|e| {
            e.mark_composition(at..at, &[]).unwrap();
            e.replace_text(at..at, "ｎ", WriteOptions::default())
                .unwrap();
            e.set_selection(at + 1..at + 1, false).unwrap();
            e.mark_composition(at..at + 1, &[]).unwrap();
        });
        let steps: [(usize, usize, &str); 7] = [
            (0, 1, "に"),
            (0, 1, "にｈ"),
            (0, 2, "にほ"),
            (0, 2, "にほｎ"),
            (0, 3, "にほんｇ"),
            (0, 4, "にほんご"),
            (0, 4, "にほんご"),
        ];
        for (k, (s0, e0, text)) in steps.iter().enumerate() {
            let end = at + text.chars().count();
            lock(&|e| {
                e.replace_text(at + s0..at + e0, text, WriteOptions::default())
                    .unwrap();
                e.set_selection(end..end, false).unwrap();
                e.mark_composition(
                    at..end,
                    &[CompositionSegment {
                        range: at..end,
                        style: SegmentStyle::Input,
                    }],
                )
                .unwrap();
            });
            if variant & (1 << (k % 5)) != 0 {
                settle(&mut tree);
            }
        }
        lock(&|e| {
            e.replace_text(at..at + 4, "にほんご", WriteOptions::default())
                .unwrap();
            e.set_selection(at + 4..at + 4, false).unwrap();
            e.end_composition(teksilo_core::ImeOrigin::Composition)
                .unwrap();
        });
        if variant & 16 != 0 {
            settle(&mut tree);
        }
        let before = doc.to_plain_text().unwrap();
        assert!(before.contains("にほんご"));
        tree.dispatch_event(WidgetEvent::KeyDown {
            key: Key::Z,
            modifiers: Modifiers::COMMAND,
            text: Some("\u{1a}".into()),
        });
        settle(&mut tree);
        let after = doc.to_plain_text().unwrap();
        assert!(!after.contains("にほんご"), "variant {variant}: {after:?}");
        assert_eq!(after, doc_before_composition, "variant {variant}");
    }
}

/// The macOS accent popup: the held key typed `e`, the character picked in the
/// popup replaces it. One undo takes back both, as in TextEdit; recorded as a
/// replacement of its own, the first undo gave the `e` back.
#[test]
fn a_replacement_of_the_last_typed_character_is_undone_with_that_typing() {
    let mut editor = Editor::with_text("x ");
    editor.handle.select_range(2, 2);
    editor.doc.break_undo_merge();
    editor.tree.type_text(editor.id, "e");
    editor.settle();
    assert_eq!(editor.text(), "x e");
    let landed = editor
        .edit(|e| e.replace_text(2..3, "é", WriteOptions::default()))
        .unwrap();
    assert_eq!(
        landed,
        Landed {
            inserted: 2..3,
            replaced: 2..3
        }
    );
    editor.settle();
    assert_eq!(editor.text(), "x é");
    editor.tree.press_key(Key::Z, Modifiers::COMMAND);
    editor.settle();
    assert_eq!(
        editor.text(),
        "x ",
        "one undo takes back the typing and the pick"
    );
}

/// A word selected, `ab` typed over it, the held `e`'s popup picked: one undo
/// gives the word back, as TextEdit's "Undo Typing" does.
#[test]
fn a_replacement_ending_typing_over_a_selection_is_undone_with_it() {
    let mut editor = Editor::with_text("one two three");
    editor.doc.break_undo_merge();
    editor.handle.select_range(4, 7);
    editor.tree.type_text(editor.id, "ab");
    editor.settle();
    editor.tree.type_text(editor.id, "e");
    editor.settle();
    assert_eq!(editor.text(), "one abe three");
    editor
        .edit(|e| e.replace_text(6..7, "é", WriteOptions::default()))
        .unwrap();
    editor.settle();
    assert_eq!(editor.text(), "one abé three");
    editor.tree.press_key(Key::Z, Modifiers::COMMAND);
    editor.settle();
    assert_eq!(
        editor.text(),
        "one two three",
        "one undo gives the word back"
    );
    editor
        .tree
        .press_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    editor.settle();
    assert_eq!(editor.text(), "one abé three");
}
