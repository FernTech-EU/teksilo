// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's context menu, opened from the keyboard.
//!
//! Shift+F10 opens the menu a right-click opens, with no click behind it: the
//! menu is anchored on the field, and the reader expects it to act where they
//! left the caret. Closing it must hand the field back as it was. Each check
//! reads the caret and the selection as the platform adapter hands them to a
//! screen reader.

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_platform::clipboard::{ClipboardHandle, MemoryClipboard};

use super::TextInputField;
use crate::common::heard_test::{Listener, TextAnswer};

/// Sixty characters: wider than the field, so its middle, where a menu opened
/// from the keyboard is anchored, is far from the start.
fn sixty() -> String {
    "abcdefghij".repeat(6)
}

struct Field {
    tree: WidgetTree,
    text: Signal<String>,
    clipboard: ClipboardHandle,
    reader: Listener,
}

impl Field {
    /// A focused field holding [`sixty`], the caret at its start, a reader
    /// attached, and a clipboard for the menu's Paste.
    fn at_start() -> Self {
        let text = Signal::new(sixty());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let clipboard = ClipboardHandle::new(MemoryClipboard::new());
        let mut registry: std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>> =
            std::collections::HashMap::new();
        registry.insert(
            std::any::TypeId::of::<ClipboardHandle>(),
            Box::new(clipboard.clone()),
        );
        tree.set_app_context(std::rc::Rc::new(
            teksilo_core::event_source::TreeAppContext::empty().with_app_state(registry),
        ));
        let padded = tree.add(
            crate::primitives::Padding::symmetric(40.0, 20.0)
                .child(TextInputField::new(text.clone())),
        );
        let other = tree.add(
            crate::primitives::FixedSize::new()
                .width(40.0)
                .height(20.0)
                .child(crate::primitives::RectWidget::new().focusable(true)),
        );
        tree.add(crate::primitives::VStack::new().child(padded).child(other));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        let field = tree
            .first_focusable_descendant(padded)
            .expect("the field is focusable");
        tree.focus(field);
        tree.press_key(Key::Home, Modifiers::NONE);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        let reader = Listener::attach(&mut tree);
        let this = Self {
            tree,
            text,
            clipboard,
            reader,
        };
        assert_eq!(
            this.read(),
            Some(TextAnswer {
                caret: 0,
                selection: None
            }),
            "precondition: Home put the caret at the start, nothing selected"
        );
        this
    }

    fn settle(&mut self) {
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = self.tree.render();
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = self.reader.heard(&mut self.tree);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.settle();
    }

    /// The caret and selection a reader is given.
    fn read(&self) -> Option<TextAnswer> {
        self.reader.text_input()
    }

    fn menu_is_open(&self) -> bool {
        self.reader.finds(Role::MenuItem, "Paste")
    }
}

#[test]
fn shift_f10_leaves_the_caret_where_the_reader_put_it() {
    let mut f = Field::at_start();
    f.press(Key::F10, Modifiers::SHIFT);
    assert!(f.menu_is_open(), "Shift+F10 must open the field's menu");
    assert_eq!(
        f.read().map(|t| t.caret),
        Some(0),
        "opening the menu from the keyboard moved the caret: it was treated as a \
         click at the anchor in the middle of the field"
    );
}

#[test]
fn escape_from_the_menu_hands_the_field_back_as_it_was() {
    let mut f = Field::at_start();
    f.press(Key::F10, Modifiers::SHIFT);
    assert!(f.menu_is_open(), "Shift+F10 must open the field's menu");
    f.press(Key::Escape, Modifiers::NONE);
    assert!(!f.menu_is_open(), "Escape must close the menu");
    assert_eq!(
        f.read(),
        Some(TextAnswer {
            caret: 0,
            selection: None
        }),
        "focus coming back from the field's own menu must find the caret where the \
         reader left it, nothing selected"
    );

    f.tree
        .type_text(f.tree.focused().expect("focus is back"), "x");
    f.settle();
    assert_eq!(
        f.text.get(),
        format!("x{}", sixty()),
        "the next key must go in at the caret, not replace the field"
    );
}

#[test]
fn paste_from_the_menu_leaves_the_caret_after_what_it_pasted() {
    let mut f = Field::at_start();
    f.clipboard.set_text("XY").unwrap();
    f.press(Key::F10, Modifiers::SHIFT);
    let paste = f
        .reader
        .widget(Role::MenuItem, "Paste")
        .expect("the reader can reach the menu's Paste");
    f.tree.click(paste);
    f.settle();
    assert!(!f.menu_is_open(), "Paste must close the menu");
    assert_eq!(
        f.text.get(),
        format!("XY{}", sixty()),
        "the menu's Paste must insert at the caret"
    );
    assert_eq!(
        f.read(),
        Some(TextAnswer {
            caret: 2,
            selection: None
        }),
        "after the menu's Paste the caret must follow what was pasted, with \
         nothing selected"
    );
}

/// The return from the menu is spent by that return: a later arrival by Tab
/// still selects the field, as a keyboard arrival always has.
#[test]
fn a_tab_arrival_after_the_menu_still_selects_the_field() {
    let mut f = Field::at_start();
    f.press(Key::F10, Modifiers::SHIFT);
    f.press(Key::Escape, Modifiers::NONE);
    f.press(Key::Tab, Modifiers::NONE);
    f.press(Key::Tab, Modifiers::SHIFT);
    assert_eq!(
        f.read(),
        Some(TextAnswer {
            caret: 60,
            selection: Some((0, 60))
        }),
        "Shift+Tab back into the field must select it"
    );
}
