// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `RichTextEditor::base_direction` and the content language around an
//! editor: a paragraph that stores no direction follows them, one that
//! stores its own keeps it, and nothing is written to the document.

use teksilo_canvas::SizeProposal;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::language::LanguageTag;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{TextDirection, TextDocument};
use teksilo_text::{CursorAffinity, TextDirection as EngineDirection};

use super::RichTextEditor;
use super::state::SharedState;
use crate::TextBaseDirection;

const WIDTH: f32 = 400.0;

fn settle(tree: &mut WidgetTree) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(WIDTH, 300.0));
    }
    let _ = tree.render();
}

/// An editor over `text`, built by `build`, in a window of `direction`,
/// settled. Returns the tree, the document and the editor's state.
fn mounted(
    text: &str,
    direction: LayoutDirection,
    build: impl FnOnce(RichTextEditor) -> Box<dyn teksilo_core::widget::Widget>,
) -> (WidgetTree, TextDocument, SharedState) {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();
    let mut tree = WidgetTree::new();
    tree.set_layout_direction(direction);
    tree.add_boxed(build(editor));
    settle(&mut tree);
    (tree, doc, state)
}

/// The x of the caret at `position`, in the engine's space.
fn caret_x(state: &SharedState, position: usize) -> f32 {
    state
        .borrow()
        .engine
        .caret_rect(position, CursorAffinity::Downstream)[0]
}

#[test]
fn an_explicit_direction_reaches_every_paragraph_that_stores_none() {
    let (_tree, doc, state) = mounted("abc\ndef", LayoutDirection::LeftToRight, |e| {
        Box::new(e.base_direction(TextBaseDirection::RightToLeft))
    });
    // A right-to-left paragraph of Latin text sits against the right edge.
    assert!(caret_x(&state, 0) > WIDTH / 2.0, "{}", caret_x(&state, 0));
    assert!(caret_x(&state, 4) > WIDTH / 2.0, "{}", caret_x(&state, 4));
    assert_eq!(
        state.borrow().engine.default_direction(),
        EngineDirection::RightToLeft
    );
    assert!(!doc.can_undo(), "nothing was written to the document");
    assert_eq!(doc.to_plain_text().unwrap(), "abc\ndef");
}

#[test]
fn a_paragraph_with_its_own_direction_keeps_it() {
    let doc = TextDocument::new();
    doc.set_plain_text("abc").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.set_direction(TextDirection::LeftToRight);
    let state = editor.state_handle();
    let mut tree = WidgetTree::new();
    tree.add(editor.base_direction(TextBaseDirection::RightToLeft));
    settle(&mut tree);
    assert!(caret_x(&state, 0) < 1.0, "{}", caret_x(&state, 0));
}

#[test]
fn an_empty_paragraph_takes_the_declared_languages_direction() {
    let arabic = LanguageTag::parse("ar").unwrap();
    let (_tree, _doc, state) = mounted("", LayoutDirection::LeftToRight, |e| {
        Box::new(e.content_language(arabic))
    });
    let st = state.borrow();
    assert_eq!(st.engine.default_language(), Some("ar"));
    assert_eq!(st.engine.neutral_direction(), EngineDirection::RightToLeft);
    drop(st);
    assert!(caret_x(&state, 0) > WIDTH / 2.0, "{}", caret_x(&state, 0));
}

#[test]
fn with_no_language_an_empty_paragraph_takes_the_windows_direction() {
    let (_tree, _doc, state) = mounted("", LayoutDirection::RightToLeft, |e| Box::new(e));
    assert!(caret_x(&state, 0) > WIDTH / 2.0, "{}", caret_x(&state, 0));
    let (_tree, _doc, state) = mounted("", LayoutDirection::LeftToRight, |e| Box::new(e));
    assert!(caret_x(&state, 0) < 1.0, "{}", caret_x(&state, 0));
}

#[test]
fn a_bound_direction_lays_the_text_out_again() {
    let direction = teksilo_core::signal::Signal::new(TextBaseDirection::LeftToRight);
    let bound = direction.clone();
    let (mut tree, _doc, state) = mounted("abc", LayoutDirection::LeftToRight, move |e| {
        Box::new(e.base_direction(bound))
    });
    assert!(caret_x(&state, 0) < 1.0);
    direction.set(TextBaseDirection::RightToLeft);
    settle(&mut tree);
    assert!(caret_x(&state, 0) > WIDTH / 2.0, "{}", caret_x(&state, 0));
}

/// The language of each text run of the update, as the consumer resolves it.
fn run_languages(tree: &mut WidgetTree) -> Vec<Option<String>> {
    let update = tree.sync_accessibility();
    let languages = teksilo_core::accessibility::audit::effective_languages(&update);
    update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == teksilo_core::accesskit::Role::TextRun)
        .map(|(id, _)| languages.get(id).cloned())
        .collect()
}

#[test]
fn a_document_in_its_own_language_is_read_in_it() {
    let french = LanguageTag::parse("fr").unwrap();
    let doc = TextDocument::new();
    doc.set_plain_text("日本語の文章\n二つ目").unwrap();
    doc.set_default_language("ja").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let mut tree = WidgetTree::new();
    tree.set_locale("fr-FR".to_owned());
    tree.add(editor.content_language(french));
    settle(&mut tree);

    let languages = run_languages(&mut tree);
    assert!(languages.len() >= 2, "{languages:?}");
    assert!(
        languages.iter().all(|l| l.as_deref() == Some("ja")),
        "every paragraph inherits the document's language: {languages:?}"
    );
}

#[test]
fn a_document_with_no_language_is_read_in_the_editors() {
    let japanese = LanguageTag::parse("ja").unwrap();
    let (mut tree, _doc, _state) = mounted(
        "日本語\nテキスト",
        LayoutDirection::LeftToRight,
        |e| Box::new(e.content_language(japanese)),
    );
    let languages = run_languages(&mut tree);
    assert!(!languages.is_empty());
    assert!(
        languages.iter().all(|l| l.as_deref() == Some("ja")),
        "{languages:?}"
    );
}
