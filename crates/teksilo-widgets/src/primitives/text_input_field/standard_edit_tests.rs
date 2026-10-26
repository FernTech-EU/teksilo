// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's clipboard, undo and select-all follow the user's bindings.
//!
//! They are reached through the `edit.*` shortcuts every tree registers, so a
//! rebind in the registry moves them, and the field no longer answers the old
//! chord itself.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::action::Action;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::{KeyStroke, StandardEdit};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_platform::clipboard::{ClipboardHandle, MemoryClipboard};

use super::TextInputField;

/// A parent that handles `edit.cut` and `edit.copy` itself, as an
/// application's own Cut and Copy would, and counts what reaches them; or,
/// without `app_actions`, a parent that takes nothing.
#[derive(Debug)]
struct AppCut {
    field: Option<TextInputField>,
    app_actions: bool,
    cuts: Rc<Cell<u32>>,
    copies: Rc<Cell<u32>>,
    child: Option<WidgetId>,
}

impl Widget for AppCut {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if self.app_actions {
            let cuts = self.cuts.clone();
            ctx.register_action(
                Action::new(StandardEdit::Cut.id()).on_invoke(move |_, _| cuts.set(cuts.get() + 1)),
            );
            let copies = self.copies.clone();
            ctx.register_action(
                Action::new(StandardEdit::Copy.id())
                    .on_invoke(move |_, _| copies.set(copies.get() + 1)),
            );
        }
        let field = self.field.take().expect("built once");
        let child = ctx.add(field);
        self.child = Some(child);
        vec![child]
    }

    fn layout_response(
        &self,
        proposal: teksilo_canvas::SizeProposal,
        ctx: &LayoutContext,
    ) -> LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

struct Field {
    tree: WidgetTree,
    field: WidgetId,
    text: Signal<String>,
    clipboard: ClipboardHandle,
    app_cuts: Rc<Cell<u32>>,
    app_copies: Rc<Cell<u32>>,
}

impl Field {
    /// A focused field holding `hello`, inside a parent with its own Cut
    /// and Copy.
    fn new(read_only: bool) -> Self {
        Self::with("hello", |field| field.read_only(read_only), true)
    }

    /// A focused field holding `text`, inside a parent that takes nothing,
    /// so what the route takes is what the field takes.
    fn alone(text: &str, read_only: bool) -> Self {
        Self::with(text, |field| field.read_only(read_only), false)
    }

    /// A focused field holding `text`, made by `make`, inside a parent with
    /// its own Cut and Copy when `app_actions`.
    fn with(
        text: &str,
        make: impl FnOnce(TextInputField) -> TextInputField,
        app_actions: bool,
    ) -> Self {
        let text = Signal::new(text.to_string());
        let field = make(TextInputField::new(text.clone()));
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let clipboard = ClipboardHandle::new(MemoryClipboard::new());
        let mut registry: std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>> =
            std::collections::HashMap::new();
        registry.insert(
            std::any::TypeId::of::<ClipboardHandle>(),
            Box::new(clipboard.clone()),
        );
        tree.set_app_context(Rc::new(
            teksilo_core::event_source::TreeAppContext::empty().with_app_state(registry),
        ));
        let app_cuts = Rc::new(Cell::new(0));
        let app_copies = Rc::new(Cell::new(0));
        let root = tree.add(AppCut {
            field: Some(field),
            app_actions,
            cuts: app_cuts.clone(),
            copies: app_copies.clone(),
            child: None,
        });
        tree.layout(SizeProposal::exact(300.0, 40.0));
        let field = tree
            .first_focusable_descendant(root)
            .expect("the field is focusable");
        tree.focus(field);
        Self {
            tree,
            field,
            text,
            clipboard,
            app_cuts,
            app_copies,
        }
    }

    /// Whether a menu row for `command` shows enabled, as the native menu bar
    /// and the in-window menus ask.
    fn offers(&self, command: StandardEdit) -> bool {
        let offered = self
            .tree
            .handled_intents_from(self.field)
            .contains(&command.id());
        assert_eq!(
            offered,
            self.tree.can_handle(command.id()),
            "the two questions agree on {command:?}"
        );
        offered
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.tree.layout(SizeProposal::exact(300.0, 40.0));
    }

    fn clipboard(&self) -> String {
        self.clipboard.get_text().unwrap_or_default()
    }
}

#[test]
fn the_default_chords_still_copy_and_cut() {
    let mut f = Field::new(false);
    f.press(Key::A, Modifiers::COMMAND);
    f.press(Key::C, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "hello", "select all, then copy");
    f.press(Key::X, Modifiers::COMMAND);
    assert_eq!(f.text.get(), "", "cut empties the field");
    assert_eq!(
        f.app_cuts.get(),
        0,
        "the field took Cut; the app never saw it"
    );
    f.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(f.text.get(), "hello", "undo brings the text back");
}

#[test]
fn a_rebound_copy_leaves_the_old_chord_and_answers_the_new_one() {
    let mut f = Field::new(false);
    // At QWERTY's C position, Bépo types `x`; a user moving Copy there moves
    // Cut out of its way too, as the settings UI does on a conflict.
    let copy_here = KeyStroke::new(Key::X, Modifiers::COMMAND);
    let registry = f.tree.shortcut_registry_mut();
    registry.rebind_primary(StandardEdit::Cut.id(), None);
    registry.rebind_primary(StandardEdit::Copy.id(), Some(copy_here));

    f.press(Key::A, Modifiers::COMMAND);
    f.press(Key::C, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "", "Ctrl+C no longer copies");
    assert_eq!(f.text.get(), "hello", "nor does it type a `c`");
    f.press(Key::X, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "hello", "the new chord copies");
    assert_eq!(f.text.get(), "hello", "and does not cut");
}

#[test]
fn a_secondary_chord_works_beside_the_default() {
    let mut f = Field::new(false);
    let select_all_too = KeyStroke::new(Key::Character('à'), Modifiers::CTRL);
    f.tree
        .shortcut_registry_mut()
        .rebind_secondary(StandardEdit::SelectAll.id(), Some(select_all_too));

    f.press(Key::Character('à'), Modifiers::CTRL);
    f.press(Key::C, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "hello", "the secondary chord selected all");
}

#[test]
fn a_read_only_field_passes_cut_on_to_the_application() {
    let mut f = Field::new(true);
    f.press(Key::A, Modifiers::COMMAND);
    f.press(Key::X, Modifiers::COMMAND);
    assert_eq!(f.text.get(), "hello", "a read-only field cuts nothing");
    assert_eq!(f.app_cuts.get(), 1, "the application's Cut runs instead");
    f.press(Key::C, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "hello", "copying is not editing");
}

#[test]
fn copy_and_cut_grey_out_until_something_is_selected() {
    let mut f = Field::alone("hello", false);
    assert!(!f.offers(StandardEdit::Copy), "nothing selected");
    assert!(!f.offers(StandardEdit::Cut), "nothing selected");
    assert!(f.offers(StandardEdit::Paste), "an editable field");
    assert!(f.offers(StandardEdit::PastePlain), "an editable field");
    assert!(f.offers(StandardEdit::SelectAll), "the field holds text");

    f.press(Key::A, Modifiers::COMMAND);
    assert!(f.offers(StandardEdit::Copy), "after select all");
    assert!(f.offers(StandardEdit::Cut), "after select all");

    f.press(Key::ArrowRight, Modifiers::empty());
    assert!(!f.offers(StandardEdit::Copy), "the selection collapsed");
}

#[test]
fn a_read_only_field_offers_no_paste_and_no_cut() {
    let mut f = Field::alone("hello", true);
    f.press(Key::A, Modifiers::COMMAND);
    assert!(f.offers(StandardEdit::Copy), "copying is not editing");
    assert!(!f.offers(StandardEdit::Cut));
    assert!(!f.offers(StandardEdit::Paste));
    assert!(!f.offers(StandardEdit::PastePlain));
}

#[test]
fn an_empty_field_offers_no_select_all() {
    let f = Field::alone("", false);
    assert!(!f.offers(StandardEdit::SelectAll));
    assert!(!f.offers(StandardEdit::Copy));
    assert!(f.offers(StandardEdit::Paste));
}

#[test]
fn undo_and_redo_follow_the_history() {
    let mut f = Field::alone("", false);
    assert!(!f.offers(StandardEdit::Undo), "nothing to undo yet");
    assert!(!f.offers(StandardEdit::Redo));
    f.tree.type_text(f.field, "x");
    f.tree.layout(SizeProposal::exact(300.0, 40.0));
    assert_eq!(f.text.get(), "x");
    assert!(f.offers(StandardEdit::Undo), "after typing");
    f.press(Key::Z, Modifiers::COMMAND);
    assert!(f.offers(StandardEdit::Redo), "after an undo");
}

#[test]
fn a_masked_field_offers_no_copy() {
    let mut f = Field::with(
        "secret",
        |field| field.secure(super::EchoMode::Masked),
        false,
    );
    f.press(Key::A, Modifiers::COMMAND);
    assert!(!f.offers(StandardEdit::Copy), "the plaintext stays put");
    assert!(!f.offers(StandardEdit::Cut));
    assert!(f.offers(StandardEdit::Paste));
}

/// Copy with nothing selected stops at the field, which takes it and copies
/// nothing, so the application's own Copy above it does not run, and the
/// row greys: choosing it would run nothing. Once something is selected, the
/// field copies it and the row is enabled.
#[test]
fn copy_with_nothing_selected_is_still_the_fields() {
    let mut f = Field::new(false);
    f.clipboard.set_text("before").unwrap();
    assert!(
        !f.offers(StandardEdit::Copy),
        "the field takes Copy and has nothing to copy"
    );
    f.press(Key::C, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "before");
    assert_eq!(f.app_copies.get(), 0, "the field took the chord");
    assert_eq!(f.text.get(), "hello", "and typed no `c`");

    f.press(Key::A, Modifiers::COMMAND);
    assert!(f.offers(StandardEdit::Copy), "something to copy");
    f.press(Key::C, Modifiers::COMMAND);
    assert_eq!(f.clipboard(), "hello");
    assert_eq!(f.app_copies.get(), 0);
}

/// A read-only field passes Cut on, so the Cut row is the application's.
#[test]
fn a_read_only_field_leaves_the_cut_row_to_the_application() {
    let mut f = Field::new(true);
    assert!(f.offers(StandardEdit::Cut), "the application's Cut");
    f.press(Key::X, Modifiers::COMMAND);
    assert_eq!(f.app_cuts.get(), 1, "choosing it runs it");
}
