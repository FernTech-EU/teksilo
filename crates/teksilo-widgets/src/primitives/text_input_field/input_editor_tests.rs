// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's document as the platform's input system writes it, through
//! the window's registry: the field's rules outside a composition and on a
//! composition's final text only, the undo step, the batch, the journal.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::ImeOrigin;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::text_input_document::{
    ChangeCause, TextInputDocument, TextInputEditor, WriteOptions, WriteRefused,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

use super::TextInputField;
use crate::EditSource;

struct Field {
    tree: WidgetTree,
    field: WidgetId,
    text: Signal<String>,
    arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
}

impl Field {
    fn new(text: &str, make: impl FnOnce(TextInputField) -> TextInputField) -> Self {
        let signal = Signal::new(text.to_string());
        let arrivals = Rc::new(RefCell::new(Vec::new()));
        let seen = arrivals.clone();
        let field = make(TextInputField::new(signal.clone()))
            .on_text_inserted(move |source, chars| seen.borrow_mut().push((source, chars)));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let root = tree.add(field);
        tree.layout(SizeProposal::exact(300.0, 40.0));
        let field = tree.first_focusable_descendant(root).unwrap_or(root);
        tree.focus(field);
        let mut f = Self {
            tree,
            field,
            text: signal,
            arrivals,
        };
        f.settle();
        f
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(SizeProposal::exact(300.0, 40.0));
        }
    }

    fn edit<R>(&self, f: impl FnOnce(&dyn TextInputEditor) -> R) -> R {
        self.tree
            .text_surfaces()
            .with_focused_input_editor(f)
            .expect("a field that takes text hands out its writing half")
    }

    fn read<R>(&self, f: impl FnOnce(&dyn TextInputDocument) -> R) -> R {
        self.tree
            .text_surfaces()
            .with_focused_input_document(f)
            .expect("the field describes its document")
    }
}

#[test]
fn a_write_goes_through_the_fields_rules_and_follows_into_the_signal() {
    let mut f = Field::new("ab", |field| field.max_length(4));
    let start = f.read(|d| d.revisions()).unwrap().text;
    let landed = f
        .edit(|e| e.replace_text(2..2, "c\nde", WriteOptions::default()))
        .unwrap();
    assert_eq!(
        landed.inserted,
        2..4,
        "the line break goes, the cap keeps two"
    );
    assert_eq!(*f.arrivals.borrow(), vec![(EditSource::Ime, 2)]);
    let change = f.read(|d| d.text_changed_since(start)).unwrap();
    assert_eq!(change.cause, ChangeCause::InputSystem);
    f.settle();
    assert_eq!(f.text.get(), "abcd");
}

/// A composition's states pass untouched, even past the cap; its final text
/// meets the rules as it ends, as one undo step.
#[test]
fn a_composition_meets_the_rules_only_as_it_ends() {
    let mut f = Field::new("ab", |field| field.max_length(3));
    f.edit(|e| {
        e.mark_composition(2..2, &[]).unwrap();
        e.replace_text(2..2, "にほん", WriteOptions::default())
            .unwrap();
    });
    assert_eq!(
        f.read(|d| d.text_in(0..9)).unwrap(),
        "abにほん",
        "past the cap while composing"
    );
    f.edit(|e| e.end_composition(ImeOrigin::Dictation)).unwrap();
    assert_eq!(*f.arrivals.borrow(), vec![(EditSource::Dictation, 1)]);
    f.settle();
    assert_eq!(f.text.get(), "abに");
}

#[test]
fn the_batch_holds_back_the_arrivals() {
    let f = Field::new("ab", |field| field);
    assert_eq!(f.read(|d| d.utf16_index()).unwrap().utf16_len(), 2);
    f.edit(|e| {
        e.begin_input_batch();
        e.replace_text(0..0, "x", WriteOptions::default()).unwrap();
    });
    assert!(f.arrivals.borrow().is_empty());
    assert_eq!(
        f.read(|d| d.utf16_index()).unwrap().utf16_len(),
        3,
        "the write is read back while the document's events are held"
    );
    f.edit(|e| e.end_input_batch());
    assert_eq!(*f.arrivals.borrow(), vec![(EditSource::Ime, 1)]);
}

#[test]
fn typing_waiting_makes_the_field_busy_until_flushed() {
    let mut f = Field::new("ab", |field| field);
    // The caret is at the start: the typing goes there.
    f.tree.type_text(f.field, "c");
    assert_eq!(
        f.edit(|e| e.replace_text(0..0, "x", WriteOptions::default())),
        Err(WriteRefused::Busy)
    );
    assert!(f.read(|d| d.flush_pending_input()));
    assert!(
        f.edit(|e| e.replace_text(0..0, "x", WriteOptions::default()))
            .is_ok()
    );
    f.settle();
    assert_eq!(f.text.get(), "xcab");
}

#[test]
fn a_read_only_or_secure_field_hands_out_no_writing_half() {
    for make in [
        (|field: TextInputField| field.read_only(true)) as fn(TextInputField) -> TextInputField,
        |field| field.secure(super::EchoMode::Masked),
    ] {
        let f = Field::new("ab", make);
        assert!(
            f.tree
                .text_surfaces()
                .with_focused_input_editor(|_| ())
                .is_none()
        );
    }
}

/// The macOS accent popup in a field: the held key typed `e`, the character
/// picked in the popup replaces it, and one undo takes back both.
#[test]
fn a_replacement_of_the_last_typed_character_is_undone_with_that_typing() {
    let mut f = Field::new("", |field| field);
    f.tree.type_text(f.field, "e");
    f.settle();
    assert_eq!(f.text.get(), "e");
    f.edit(|e| e.replace_text(0..1, "é", WriteOptions::default()))
        .unwrap();
    f.settle();
    assert_eq!(f.text.get(), "é");
    f.tree.press_key(Key::Z, Modifiers::COMMAND);
    f.settle();
    assert_eq!(
        f.text.get(),
        "",
        "one undo takes back the typing and the pick"
    );
}
