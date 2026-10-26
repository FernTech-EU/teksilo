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

/// The application: its own Undo, Redo, Cut and Copy, above the text
/// widget, counting what reaches them.
struct App {
    child: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
    undos: Rc<Cell<u32>>,
    redos: Rc<Cell<u32>>,
    cuts: Rc<Cell<u32>>,
    copies: Rc<Cell<u32>>,
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
        let cuts = self.cuts.clone();
        ctx.register_action(
            Action::new(StandardEdit::Cut.id()).on_invoke(move |_, _| cuts.set(cuts.get() + 1)),
        );
        let copies = self.copies.clone();
        ctx.register_action(
            Action::new(StandardEdit::Copy.id())
                .on_invoke(move |_, _| copies.set(copies.get() + 1)),
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
    cuts: Rc<Cell<u32>>,
    copies: Rc<Cell<u32>>,
    text: Box<dyn Fn() -> String>,
}

impl Rig {
    fn new(child: Box<dyn Widget>, text: Box<dyn Fn() -> String>, passes_on: bool) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.text_surfaces().set_empty_history_passes_on(passes_on);
        let undos = Rc::new(Cell::new(0));
        let redos = Rc::new(Cell::new(0));
        let cuts = Rc::new(Cell::new(0));
        let copies = Rc::new(Cell::new(0));
        let root = tree.add(App {
            child: Some(child),
            child_id: None,
            undos: undos.clone(),
            redos: redos.clone(),
            cuts: cuts.clone(),
            copies: copies.clone(),
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
            cuts,
            copies,
            text,
        }
    }

    /// Whether a menu row for `command` shows enabled, as the native menu bar
    /// and the in-window menus ask.
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

/// An Edit menu row is enabled exactly when choosing it runs something. With
/// nothing to undo, a widget that passes Undo on leaves the row to the
/// application's Undo above it, and one that keeps an empty Undo greys the
/// row, since the application's never runs. With nothing selected, Copy
/// greys in a widget that copies a selection: Copy chosen there stops at the
/// widget, which copies nothing, and the application's Copy never runs.
#[test]
fn an_edit_row_is_enabled_exactly_when_choosing_it_runs_something() {
    for (name, make) in WIDGETS {
        for passes_on in [false, true] {
            let mut r = make(passes_on);
            let case = format!("{name}, passes on {passes_on}");
            let row = r.offers(StandardEdit::Undo);
            r.undo();
            assert_eq!(row, passes_on, "{case}: the Undo row");
            assert_eq!(r.undos.get(), u32::from(passes_on), "{case}: Undo ran");
            let row = r.offers(StandardEdit::Redo);
            r.redo();
            assert_eq!(row, passes_on, "{case}: the Redo row");
            assert_eq!(r.redos.get(), u32::from(passes_on), "{case}: Redo ran");

            r.type_x();
            assert!(r.offers(StandardEdit::Undo), "{case}: after typing");
            r.undo();
            assert_eq!(r.undos.get(), u32::from(passes_on), "{case}: undone here");

            // Copy and Cut with nothing selected: the code editor takes the
            // caret's line, the others nothing.
            r.type_x();
            let copies_the_line = name == "CodeEditor";
            assert_eq!(
                r.offers(StandardEdit::Copy),
                copies_the_line,
                "{case}: Copy"
            );
            r.press(Key::C, Modifiers::COMMAND);
            assert_eq!(r.copies.get(), 0, "{case}: Copy stopped at the widget");
            assert_eq!(r.offers(StandardEdit::Cut), copies_the_line, "{case}: Cut");
        }
    }
}

/// A read-only widget passes Cut on, so the Cut row is the application's,
/// and choosing it runs the application's Cut.
#[test]
fn a_read_only_widget_leaves_the_cut_row_to_the_application() {
    let text = Signal::new("hello".to_string());
    let field = Rig::new(
        Box::new(TextInputField::new(text).read_only(true)),
        Box::new(String::new),
        false,
    );
    let code = Rig::new(
        Box::new(CodeEditor::read_only({
            let document = TextDocument::new();
            document.set_plain_text("fn main() {}").unwrap();
            document
        })),
        Box::new(String::new),
        false,
    );
    for (name, mut r) in [("TextInputField", field), ("CodeEditor", code)] {
        r.press(Key::A, Modifiers::COMMAND);
        assert!(r.offers(StandardEdit::Cut), "{name}: the application's Cut");
        r.press(Key::X, Modifiers::COMMAND);
        assert_eq!(r.cuts.get(), 1, "{name}: and choosing it runs it");
    }
}

/// A masked field has nothing it may copy and still takes Copy, so the row
/// greys and the application's Copy does not run.
#[test]
fn a_masked_field_greys_copy_under_an_application_copy() {
    let text = Signal::new("secret".to_string());
    let mut r = Rig::new(
        Box::new(TextInputField::new(text).secure(teksilo_widgets::primitives::EchoMode::Masked)),
        Box::new(String::new),
        false,
    );
    r.press(Key::A, Modifiers::COMMAND);
    assert!(!r.offers(StandardEdit::Copy));
    r.press(Key::C, Modifiers::COMMAND);
    assert_eq!(r.copies.get(), 0);
}
