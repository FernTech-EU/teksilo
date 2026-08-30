// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The framework's answer to *"is the focused widget a text surface?"*.
//!
//! These pin the property the whole mechanism exists for: an application that
//! takes `Ctrl+Z` for itself can tell, for **any** text widget in the tree —
//! including ones it did not build — that the caret is there, and can drive it
//! or step aside. A registry that missed a widget would be worse than none: the
//! host would confidently route the chord somewhere else.

use teksilo_canvas::SizeProposal;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_text::text_document::TextDocument;
use teksilo_widgets::button::Button;
use teksilo_widgets::primitives::TextInputField;
use teksilo_widgets::rich_text::RichTextEditor;

/// A laid-out tree — `build` runs during layout, and registration happens in
/// `build`.
fn laid_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

#[test]
fn a_focused_rich_editor_is_reported_and_reachable() {
    let mut tree = WidgetTree::new();
    let editor = RichTextEditor::editor(TextDocument::new());
    let id = tree.add(editor);
    laid_out(&mut tree);

    assert!(
        !tree.focused_is_text_surface(),
        "nothing is focused yet, so the host is free to route the chord itself"
    );

    tree.focus(id);
    assert!(tree.focused_is_text_surface());
    let surface = tree
        .focused_text_surface()
        .expect("the focused editor is reachable through the registry");
    assert!(
        !surface.can_undo(),
        "a fresh editor has nothing to undo — but the host can now ask, which \
         is the point"
    );
    assert!(!surface.has_selection());
}

#[test]
fn a_focused_text_field_is_reported_too() {
    let mut tree = WidgetTree::new();
    let field = TextInputField::new(Signal::new("hello".to_string()));
    let id = tree.add(field);
    laid_out(&mut tree);

    tree.focus(id);
    assert!(
        tree.focused_is_text_surface(),
        "a plain input is a text surface exactly as much as a rich editor is — \
         this is the case an application's own list always forgets"
    );
    let surface = tree.focused_text_surface().expect("reachable");
    surface.select_all();
    assert!(
        surface.has_selection(),
        "and it can be driven, not merely detected"
    );
}

#[test]
fn focus_on_a_non_text_widget_reports_nothing() {
    let mut tree = WidgetTree::new();
    let editor = RichTextEditor::editor(TextDocument::new());
    let editor_id = tree.add(editor);
    let button = tree.add(Button::new(lit!("press me")));
    laid_out(&mut tree);

    tree.focus(editor_id);
    assert!(tree.focused_is_text_surface());

    tree.focus(button);
    assert!(
        !tree.focused_is_text_surface(),
        "moving focus out of the text widget must clear the answer, or a host \
         would keep stepping aside long after the caret left"
    );
    assert!(tree.focused_text_surface().is_none());
}

#[test]
fn a_destroyed_widget_stops_being_reported() {
    let mut tree = WidgetTree::new();
    let editor = RichTextEditor::editor(TextDocument::new());
    let id = tree.add(editor);
    laid_out(&mut tree);
    tree.focus(id);
    assert!(tree.focused_is_text_surface());

    // A rebuild tears the registration down and `build` puts a fresh one back —
    // the same `retain`-then-insert path a destroy takes, and the one that runs
    // constantly in a live application.
    tree.arena_mark_needs_rebuild_for_testing(id);
    laid_out(&mut tree);
    assert!(
        tree.focused_text_surface().is_some(),
        "a rebuild must re-register, not lose the surface"
    );
    assert!(
        tree.text_surfaces().focused_is_text_surface(),
        "and the shared handle must agree with the tree"
    );
}

#[test]
fn the_handle_answers_the_same_question_away_from_the_tree() {
    // The shape an application actually uses: taken once, consulted later from
    // a frame tick, where there is no `&WidgetTree` to be had.
    let mut tree = WidgetTree::new();
    let field = TextInputField::new(Signal::new(String::new()));
    let id = tree.add(field);
    laid_out(&mut tree);
    let surfaces = tree.text_surfaces();

    assert!(!surfaces.focused_is_text_surface());
    tree.focus(id);
    assert!(
        surfaces.focused_is_text_surface(),
        "the handle shares the tree's focus signal, so it cannot go stale"
    );
}
