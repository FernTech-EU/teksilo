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

/// A parent that handles `edit.cut` itself, as an application's own Cut
/// would, and counts what reaches it.
#[derive(Debug)]
struct AppCut {
    field: Option<TextInputField>,
    cuts: Rc<Cell<u32>>,
    child: Option<WidgetId>,
}

impl Widget for AppCut {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let cuts = self.cuts.clone();
        ctx.register_action(
            Action::new(StandardEdit::Cut.id()).on_invoke(move |_, _| cuts.set(cuts.get() + 1)),
        );
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
    text: Signal<String>,
    clipboard: ClipboardHandle,
    app_cuts: Rc<Cell<u32>>,
}

impl Field {
    /// A focused field holding `hello`, inside a parent with its own Cut.
    fn new(read_only: bool) -> Self {
        let text = Signal::new("hello".to_string());
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
        let root = tree.add(AppCut {
            field: Some(TextInputField::new(text.clone()).read_only(read_only)),
            cuts: app_cuts.clone(),
            child: None,
        });
        tree.layout(SizeProposal::exact(300.0, 40.0));
        let field = tree
            .first_focusable_descendant(root)
            .expect("the field is focusable");
        tree.focus(field);
        Self {
            tree,
            text,
            clipboard,
            app_cuts,
        }
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
