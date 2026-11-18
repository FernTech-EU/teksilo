// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `on_text_inserted` and `on_change` run with no borrow of the editor held,
//! so they can read the editor through its handle and edit it, and an edit
//! they make reaches them again after them, never inside them.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::text_input_document::WriteOptions;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;

use super::{EditorHandle, RichTextEditor};
use crate::EditSource;

fn tick(tree: &mut WidgetTree) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(400.0, 300.0));
    }
}

fn type_char(tree: &mut WidgetTree, ch: char) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::NONE,
        text: Some(ch.to_string()),
    });
}

/// `editor` mounted and focused, its caret at the end of its text.
fn mount(editor: RichTextEditor) -> WidgetTree {
    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    tick(&mut tree);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::End,
        modifiers: Modifiers::COMMAND,
        text: None,
    });
    tick(&mut tree);
    tree
}

/// The issue's reproduction: reading the caret and the text from
/// `on_text_inserted` panicked with "already mutably borrowed".
#[test]
fn on_text_inserted_reads_the_editor_through_its_handle() {
    let doc = TextDocument::new();
    doc.set_plain_text("ab").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let weak = editor.handle().downgrade();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = editor.on_text_inserted({
        let seen = seen.clone();
        move |_, _| {
            let handle = weak.upgrade().expect("the editor is alive");
            seen.borrow_mut()
                .push((handle.cursor_position(), handle.to_plain_text()));
        }
    });
    let mut tree = mount(editor);

    type_char(&mut tree, 'c');
    tick(&mut tree);

    assert_eq!(*seen.borrow(), vec![(3, "abc".to_owned())]);
}

#[test]
fn on_change_reads_the_text_it_reports() {
    let doc = TextDocument::new();
    doc.set_plain_text("ab").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let weak = editor.handle().downgrade();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = editor.on_change({
        let seen = seen.clone();
        move || {
            let handle = weak.upgrade().expect("the editor is alive");
            seen.borrow_mut().push(handle.to_plain_text());
        }
    });
    let mut tree = mount(editor);

    type_char(&mut tree, 'c');
    tick(&mut tree);
    type_char(&mut tree, 'd');
    tick(&mut tree);

    assert_eq!(*seen.borrow(), vec!["abc".to_owned(), "abcd".to_owned()]);
}

/// The reporter's case: after ":)" and a space, the callback replaces the
/// emoticon. Its own edit comes back to it once, after it has returned, and
/// finds nothing more to replace.
#[test]
fn on_text_inserted_replaces_an_emoticon_through_the_handle() {
    let doc = TextDocument::new();
    doc.set_plain_text("hi ").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let weak = editor.handle().downgrade();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = editor.on_text_inserted({
        let seen = seen.clone();
        move |source, chars| {
            seen.borrow_mut().push((source, chars));
            let handle = weak.upgrade().expect("the editor is alive");
            let caret = handle.cursor_position();
            let before: String = handle.to_plain_text().chars().take(caret).collect();
            if before.ends_with(":) ") {
                handle.replace_range(caret - 3, caret, "\u{1F642} ");
            }
        }
    });
    let handle = editor.handle();
    let mut tree = mount(editor);

    for ch in [':', ')'] {
        type_char(&mut tree, ch);
    }
    tick(&mut tree);
    type_char(&mut tree, ' ');
    tick(&mut tree);

    assert_eq!(doc.to_plain_text().unwrap(), "hi \u{1F642} ");
    assert_eq!(handle.cursor_position(), 5, "the caret follows the space");
    assert_eq!(
        *seen.borrow(),
        vec![
            (EditSource::Keyboard, 2),
            (EditSource::Keyboard, 1),
            (EditSource::Programmatic, 2),
        ],
        "each insertion reported once, the callback's own after it returned"
    );

    type_char(&mut tree, 'x');
    tick(&mut tree);
    assert_eq!(
        doc.to_plain_text().unwrap(),
        "hi \u{1F642} x",
        "typing goes on after the replacement"
    );
}

/// Text the platform's input system writes is reported as the write returns,
/// and the callback can read the editor then too.
#[test]
fn an_input_system_write_reports_with_the_editor_free() {
    let doc = TextDocument::new();
    doc.set_plain_text("ab").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let weak = editor.handle().downgrade();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = editor.on_text_inserted({
        let seen = seen.clone();
        move |source, chars| {
            let handle = weak.upgrade().expect("the editor is alive");
            seen.borrow_mut()
                .push((source, chars, handle.to_plain_text()));
        }
    });
    let tree = mount(editor);

    tree.text_surfaces()
        .with_focused_input_editor(|e| e.replace_text(2..2, "cd", WriteOptions::default()))
        .expect("an editable editor takes text")
        .unwrap();

    assert_eq!(
        *seen.borrow(),
        vec![(EditSource::Ime, 2, "abcd".to_owned())],
        "reported before the write returns, not a frame later"
    );
}

/// A callback that answers every insertion with another one never ends. It
/// must stop with a message naming the loop rather than hang the window (each
/// report is delivered after the previous callback returned, so the stack does
/// not grow).
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "on_text_inserted")]
fn a_callback_that_always_inserts_is_stopped() {
    let doc = TextDocument::new();
    let editor = RichTextEditor::editor(doc.clone());
    let weak = editor.handle().downgrade();
    let editor = editor.on_text_inserted(move |_, _| {
        let handle: EditorHandle = weak.upgrade().expect("the editor is alive");
        let caret = handle.cursor_position();
        handle.replace_range(caret, caret, "x");
    });
    let mut tree = mount(editor);

    type_char(&mut tree, 'a');
    tick(&mut tree);
}
