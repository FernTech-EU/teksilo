// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A host that routes Ctrl+X / Ctrl+V / Ctrl+Z to the focused text widget
//! goes through `TextSurface`, not through the widget's keys. The surface must
//! obey what the keys obey: a forward-only editor takes a paste and refuses a
//! cut, a read-only surface refuses every edit, a masked password refuses a
//! copy. `TextSurface::accepts` says so, for a host to grey its rows.

use std::any::TypeId;
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::shortcut::StandardEdit;
use teksilo_core::signal::Signal;
use teksilo_core::text_surface::TextSurface;
use teksilo_core::widget::{EventContext, Widget};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::NoopWindowOps;
use teksilo_platform::clipboard::{ClipboardHandle, MemoryClipboard};
use teksilo_text::text_document::TextDocument;
use teksilo_widgets::code_editor::CodeEditor;
use teksilo_widgets::primitives::TextInputField;
use teksilo_widgets::rich_text::{CommandFilter, RichTextEditor};

/// A tree holding `widget`, focused, with a memory clipboard holding `"ABC"`.
fn focused(widget: impl Widget + 'static) -> (WidgetTree, ClipboardHandle) {
    let mut tree = WidgetTree::new();
    let clipboard = ClipboardHandle::new(MemoryClipboard::new());
    clipboard.set_text("ABC").unwrap();
    let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
    registry.insert(TypeId::of::<ClipboardHandle>(), Box::new(clipboard.clone()));
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(registry)));
    let id = tree.add(widget);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    (tree, clipboard)
}

fn surface(tree: &WidgetTree) -> Rc<dyn TextSurface> {
    tree.focused_text_surface()
        .expect("a text surface has focus")
}

/// Run `command` through the surface, as a host routing the chord does.
fn route(tree: &mut WidgetTree, command: StandardEdit) {
    let surface = surface(tree);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx: &mut EventContext| match command {
        StandardEdit::Cut => surface.cut(ctx),
        StandardEdit::Copy => surface.copy(ctx),
        StandardEdit::Paste => surface.paste(ctx),
        StandardEdit::PastePlain => surface.paste_plain(ctx),
        StandardEdit::Undo => surface.undo(),
        StandardEdit::Redo => surface.redo(),
        _ => surface.select_all(),
    });
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

fn accepted(tree: &WidgetTree) -> Vec<StandardEdit> {
    let surface = surface(tree);
    [
        StandardEdit::Cut,
        StandardEdit::Copy,
        StandardEdit::Paste,
        StandardEdit::PastePlain,
        StandardEdit::Undo,
        StandardEdit::Redo,
        StandardEdit::SelectAll,
    ]
    .into_iter()
    .filter(|command| surface.accepts(*command))
    .collect()
}

/// The writing game's invariant, broken in the live app: under ForwardOnly a
/// routed Ctrl+X over "act" left "galic". It takes a paste, after the
/// selection, and refuses the cut.
#[test]
fn a_forward_only_editor_refuses_a_routed_cut_and_takes_a_paste() {
    let doc = TextDocument::new();
    doc.set_plain_text("galactic").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let (mut tree, _clipboard) = focused(editor);
    handle.set_command_filter(CommandFilter::ForwardOnly);
    handle.select_range(3, 6);

    assert_eq!(
        accepted(&tree),
        [
            StandardEdit::Copy,
            StandardEdit::Paste,
            StandardEdit::PastePlain,
            StandardEdit::SelectAll
        ]
    );
    route(&mut tree, StandardEdit::Cut);
    assert_eq!(doc.to_plain_text().unwrap(), "galactic", "nothing was cut");
    route(&mut tree, StandardEdit::Undo);
    assert_eq!(doc.to_plain_text().unwrap(), "galactic", "nor undone");

    route(&mut tree, StandardEdit::Paste);
    assert_eq!(
        doc.to_plain_text().unwrap(),
        "galactABCic",
        "the paste goes in after the selection"
    );
}

#[test]
fn a_read_only_editor_refuses_every_routed_edit() {
    let doc = TextDocument::new();
    doc.set_plain_text("fixed").unwrap();
    let editor = RichTextEditor::read_only(doc.clone());
    let handle = editor.handle();
    let (mut tree, _clipboard) = focused(editor);
    handle.select_range(0, 5);

    assert_eq!(
        accepted(&tree),
        [StandardEdit::Copy, StandardEdit::SelectAll]
    );
    for command in [
        StandardEdit::Cut,
        StandardEdit::Paste,
        StandardEdit::PastePlain,
        StandardEdit::Undo,
    ] {
        route(&mut tree, command);
        assert_eq!(doc.to_plain_text().unwrap(), "fixed", "{command:?}");
    }
}

#[test]
fn an_editable_editor_takes_every_routed_edit() {
    let doc = TextDocument::new();
    doc.set_plain_text("galactic").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let (mut tree, clipboard) = focused(editor);
    handle.select_range(3, 6);

    assert_eq!(accepted(&tree).len(), 7);
    route(&mut tree, StandardEdit::Cut);
    assert_eq!(doc.to_plain_text().unwrap(), "galic");
    assert_eq!(clipboard.get_text().unwrap(), "act");
}

/// The code editor and the field already refused these one layer down; the
/// surface now says so through `accepts` and refuses at its own door too.
#[test]
fn a_read_only_code_editor_refuses_a_routed_cut_and_paste() {
    let doc = TextDocument::new();
    doc.set_plain_text("let x = 1;").unwrap();
    let (mut tree, _clipboard) = focused(CodeEditor::read_only(doc.clone()));
    route(&mut tree, StandardEdit::SelectAll);

    assert!(!surface(&tree).accepts(StandardEdit::Cut));
    assert!(!surface(&tree).accepts(StandardEdit::Paste));
    assert!(surface(&tree).accepts(StandardEdit::Copy));
    route(&mut tree, StandardEdit::Cut);
    route(&mut tree, StandardEdit::Paste);
    assert_eq!(doc.to_plain_text().unwrap(), "let x = 1;");
}

#[test]
fn a_read_only_field_refuses_a_routed_cut_and_paste() {
    let text = Signal::new("rename me".to_string());
    let (mut tree, _clipboard) = focused(TextInputField::new(text.clone()).read_only(true));
    route(&mut tree, StandardEdit::SelectAll);

    assert!(!surface(&tree).accepts(StandardEdit::Cut));
    assert!(!surface(&tree).accepts(StandardEdit::Paste));
    route(&mut tree, StandardEdit::Cut);
    route(&mut tree, StandardEdit::Paste);
    assert_eq!(text.get(), "rename me");
}

#[test]
fn a_masked_password_refuses_a_routed_copy() {
    use teksilo_widgets::primitives::EchoMode;
    let text = Signal::new("hunter2".to_string());
    let (mut tree, clipboard) = focused(TextInputField::new(text.clone()).secure(EchoMode::Masked));
    route(&mut tree, StandardEdit::SelectAll);

    assert!(!surface(&tree).accepts(StandardEdit::Copy));
    assert!(!surface(&tree).accepts(StandardEdit::Cut));
    route(&mut tree, StandardEdit::Copy);
    route(&mut tree, StandardEdit::Cut);
    assert_eq!(
        clipboard.get_text().unwrap(),
        "ABC",
        "nothing left the field"
    );
    assert_eq!(text.get(), "hunter2");
}
