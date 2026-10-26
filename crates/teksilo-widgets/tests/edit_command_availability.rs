// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A menu row for a standard edit command greys out when the focused editor
//! has nothing for it to act on, as AppKit's text views validate theirs: Copy
//! wants a selection, Cut a selection in an editable editor, Paste an editable
//! editor, Select All some text, Undo and Redo some history.
//!
//! The row asks `WidgetTree::handled_intents_from` (the native menu bar) and
//! `WidgetTree::can_handle` (the in-window menus); both are checked here. What
//! a pressed shortcut does is not affected, and is pinned elsewhere.

use teksilo_canvas::SizeProposal;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::StandardEdit;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;
use teksilo_widgets::code_editor::{CodeEditor, LogView};
use teksilo_widgets::rich_text::RichTextEditor;

struct Rig {
    tree: WidgetTree,
    focus: WidgetId,
}

impl Rig {
    fn new(widget: Box<dyn Widget>) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let root = tree.add(widget);
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let focus = tree
            .first_focusable_descendant(root)
            .expect("the editor is focusable");
        tree.focus(focus);
        Self { tree, focus }
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.tree.layout(SizeProposal::exact(400.0, 200.0));
    }

    fn type_x(&mut self) {
        self.tree.type_text(self.focus, "x");
        self.tree.layout(SizeProposal::exact(400.0, 200.0));
    }

    /// Whether a menu row for `command` shows enabled.
    fn offers(&self, command: StandardEdit) -> bool {
        let offered = self
            .tree
            .handled_intents_from(self.focus)
            .contains(&command.id());
        assert_eq!(
            offered,
            self.tree.can_handle(command.id()),
            "the two questions agree on {command:?}"
        );
        offered
    }
}

fn document(text: &str) -> TextDocument {
    let document = TextDocument::new();
    document.set_plain_text(text).unwrap();
    document
}

#[test]
fn a_rich_text_editor_offers_copy_and_cut_once_something_is_selected() {
    let mut r = Rig::new(Box::new(RichTextEditor::editor(document("hello"))));
    assert!(!r.offers(StandardEdit::Copy), "nothing selected");
    assert!(!r.offers(StandardEdit::Cut), "nothing selected");
    assert!(r.offers(StandardEdit::Paste));
    assert!(r.offers(StandardEdit::PastePlain));
    assert!(r.offers(StandardEdit::SelectAll));

    r.press(Key::A, Modifiers::COMMAND);
    assert!(r.offers(StandardEdit::Copy), "after select all");
    assert!(r.offers(StandardEdit::Cut), "after select all");
}

#[test]
fn a_read_only_rich_text_editor_offers_copy_only() {
    let mut r = Rig::new(Box::new(RichTextEditor::read_only(document("hello"))));
    r.press(Key::A, Modifiers::COMMAND);
    assert!(r.offers(StandardEdit::Copy), "copying is not editing");
    assert!(!r.offers(StandardEdit::Cut));
    assert!(!r.offers(StandardEdit::Paste));
    assert!(!r.offers(StandardEdit::PastePlain));
    assert!(!r.offers(StandardEdit::Undo));
}

#[test]
fn an_empty_rich_text_editor_offers_no_select_all_and_no_undo() {
    let mut r = Rig::new(Box::new(RichTextEditor::editor(TextDocument::new())));
    assert!(!r.offers(StandardEdit::SelectAll));
    assert!(!r.offers(StandardEdit::Undo), "nothing to undo yet");
    assert!(!r.offers(StandardEdit::Redo));
    r.type_x();
    assert!(r.offers(StandardEdit::SelectAll), "after typing");
    assert!(r.offers(StandardEdit::Undo), "after typing");
    r.press(Key::Z, Modifiers::COMMAND);
    assert!(r.offers(StandardEdit::Redo), "after an undo");
}

/// A code editor's Copy and Cut take the caret's line when nothing is
/// selected, so they want text, not a selection.
#[test]
fn a_code_editor_copies_the_line_so_it_offers_copy_with_no_selection() {
    let r = Rig::new(Box::new(CodeEditor::new(document("fn main() {}"))));
    assert!(r.offers(StandardEdit::Copy));
    assert!(r.offers(StandardEdit::Cut));
    assert!(r.offers(StandardEdit::Paste));
    assert!(r.offers(StandardEdit::SelectAll));
}

#[test]
fn an_empty_code_editor_offers_paste_only() {
    let mut r = Rig::new(Box::new(CodeEditor::new(TextDocument::new())));
    assert!(!r.offers(StandardEdit::Copy));
    assert!(!r.offers(StandardEdit::Cut));
    assert!(!r.offers(StandardEdit::SelectAll));
    assert!(!r.offers(StandardEdit::Undo));
    assert!(r.offers(StandardEdit::Paste));
    r.type_x();
    assert!(r.offers(StandardEdit::Copy), "after typing");
    assert!(r.offers(StandardEdit::Undo), "after typing");
}

#[test]
fn a_read_only_code_editor_offers_no_cut_and_no_paste() {
    let r = Rig::new(Box::new(CodeEditor::read_only(document("fn main() {}"))));
    assert!(r.offers(StandardEdit::Copy));
    assert!(r.offers(StandardEdit::SelectAll));
    assert!(!r.offers(StandardEdit::Cut));
    assert!(!r.offers(StandardEdit::Paste));
    assert!(!r.offers(StandardEdit::PastePlain));
    assert!(!r.offers(StandardEdit::Undo));
    assert!(!r.offers(StandardEdit::Redo));
}

#[test]
fn a_log_view_offers_copy_and_select_all_once_it_has_lines() {
    let log = LogView::new();
    let handle = log.handle();
    let mut r = Rig::new(Box::new(log));
    assert!(!r.offers(StandardEdit::Copy), "an empty log");
    assert!(!r.offers(StandardEdit::SelectAll), "an empty log");

    handle.append_line("started");
    r.tree.layout(SizeProposal::exact(400.0, 200.0));
    assert!(r.offers(StandardEdit::Copy), "it copies the caret's line");
    assert!(r.offers(StandardEdit::SelectAll));
    for command in [
        StandardEdit::Cut,
        StandardEdit::Paste,
        StandardEdit::PastePlain,
        StandardEdit::Undo,
        StandardEdit::Redo,
    ] {
        assert!(!r.offers(command), "{command:?}: a log is read-only");
    }
    r.press(Key::A, Modifiers::COMMAND);
    assert!(r.offers(StandardEdit::Copy));
}
