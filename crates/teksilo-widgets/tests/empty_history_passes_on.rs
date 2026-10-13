// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TextSurfaces::set_empty_history_passes_on`: a text widget with nothing to
//! undo declines Undo, so the intent reaches the application's own action.
//!
//! Pinned for each widget that answers the `edit.*` commands, because each one
//! guards its own Undo. The case that matters most is the third of every set:
//! a keystroke undone straight away, before any frame tick, must be undone in
//! the widget. A guard reading the widget's debounced `can_undo` signal would
//! still see `false` there and hand the user's typing to the application.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::action::Action;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::StandardEdit;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;
use teksilo_widgets::code_editor::CodeEditor;
use teksilo_widgets::primitives::TextInputField;
use teksilo_widgets::rich_text::RichTextEditor;

/// The application: its own Undo and Redo, above the text widget, counting
/// what reaches them.
struct App {
    child: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
    undos: Rc<Cell<u32>>,
    redos: Rc<Cell<u32>>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("App")
    }
}

impl Widget for App {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let undos = self.undos.clone();
        ctx.register_action(
            Action::new(StandardEdit::Undo.id()).on_invoke(move |_, _| undos.set(undos.get() + 1)),
        );
        let redos = self.redos.clone();
        ctx.register_action(
            Action::new(StandardEdit::Redo.id()).on_invoke(move |_, _| redos.set(redos.get() + 1)),
        );
        let child = ctx.add(self.child.take().expect("built once"));
        self.child_id = Some(child);
        vec![child]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

struct Rig {
    tree: WidgetTree,
    focus: WidgetId,
    undos: Rc<Cell<u32>>,
    redos: Rc<Cell<u32>>,
    text: Box<dyn Fn() -> String>,
}

impl Rig {
    fn new(child: Box<dyn Widget>, text: Box<dyn Fn() -> String>, passes_on: bool) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.text_surfaces().set_empty_history_passes_on(passes_on);
        let undos = Rc::new(Cell::new(0));
        let redos = Rc::new(Cell::new(0));
        let root = tree.add(App {
            child: Some(child),
            child_id: None,
            undos: undos.clone(),
            redos: redos.clone(),
        });
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let focus = tree
            .first_focusable_descendant(root)
            .expect("the text widget is focusable");
        tree.focus(focus);
        Self {
            tree,
            focus,
            undos,
            redos,
            text,
        }
    }

    /// Press a chord and lay out, which writes a field's text back to its
    /// signal. Layout runs no frame tick.
    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
        self.tree.layout(SizeProposal::exact(400.0, 200.0));
    }

    fn undo(&mut self) {
        self.press(Key::Z, Modifiers::COMMAND);
    }

    fn redo(&mut self) {
        self.press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    }

    /// Type one letter and lay out, which applies it to the document. No frame
    /// tick runs, so the widget's debounced `can_undo` is still `false`.
    fn type_x(&mut self) {
        self.tree.type_text(self.focus, "x");
        self.tree.layout(SizeProposal::exact(400.0, 200.0));
    }
}

type Make = fn(bool) -> Rig;

fn rich(passes_on: bool) -> Rig {
    let document = TextDocument::new();
    let read = document.clone();
    Rig::new(
        Box::new(RichTextEditor::editor(document)),
        Box::new(move || read.to_plain_text().unwrap_or_default()),
        passes_on,
    )
}

fn field(passes_on: bool) -> Rig {
    let text = Signal::new(String::new());
    let read = text.clone();
    Rig::new(
        Box::new(TextInputField::new(text)),
        Box::new(move || read.get()),
        passes_on,
    )
}

fn code(passes_on: bool) -> Rig {
    let document = TextDocument::new();
    let read = document.clone();
    Rig::new(
        Box::new(CodeEditor::new(document)),
        Box::new(move || read.to_plain_text().unwrap_or_default()),
        passes_on,
    )
}

const WIDGETS: [(&str, Make); 3] = [
    ("RichTextEditor", rich),
    ("TextInputField", field),
    ("CodeEditor", code),
];

#[test]
fn off_by_default_the_widget_keeps_an_empty_undo() {
    for (name, make) in WIDGETS {
        let mut r = make(false);
        r.undo();
        r.redo();
        assert_eq!(r.undos.get(), 0, "{name}: Undo stopped at the widget");
        assert_eq!(r.redos.get(), 0, "{name}: Redo stopped at the widget");
    }
}

#[test]
fn an_empty_history_passes_undo_and_redo_on() {
    for (name, make) in WIDGETS {
        let mut r = make(true);
        r.undo();
        assert_eq!(
            r.undos.get(),
            1,
            "{name}: nothing to undo, so the app's Undo ran"
        );
        r.redo();
        assert_eq!(
            r.redos.get(),
            1,
            "{name}: nothing to redo, so the app's Redo ran"
        );
    }
}

#[test]
fn a_keystroke_is_undone_where_it_was_typed_even_before_a_frame() {
    for (name, make) in WIDGETS {
        let mut r = make(true);
        r.type_x();
        assert_eq!((r.text)().trim_end(), "x", "{name}: the premise");

        // No frame tick in between: the widget's published `can_undo` has not
        // caught up yet, its document has.
        let published = r
            .tree
            .focused_text_surface()
            .map(|surface| surface.can_undo());
        r.undo();
        assert_eq!(
            r.undos.get(),
            0,
            "{name}: the typing was the widget's to undo (published can_undo: {published:?})"
        );
        assert_eq!((r.text)().trim_end(), "", "{name}: and it was undone");

        // Now the history is spent in one direction and full in the other.
        r.undo();
        assert_eq!(r.undos.get(), 1, "{name}: exhausted, so it passes on");
        r.redo();
        assert_eq!(
            r.redos.get(),
            0,
            "{name}: the undone keystroke is redone here"
        );
        assert_eq!((r.text)().trim_end(), "x", "{name}: redone");
    }
}

#[test]
fn a_read_only_field_still_declines_whatever_the_setting() {
    for passes_on in [false, true] {
        let text = Signal::new("hello".to_string());
        let mut r = Rig::new(
            Box::new(TextInputField::new(text).read_only(true)),
            Box::new(String::new),
            passes_on,
        );
        r.undo();
        assert_eq!(r.undos.get(), 1, "passes_on = {passes_on}");
    }
}
