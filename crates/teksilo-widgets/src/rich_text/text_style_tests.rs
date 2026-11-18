// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `RichTextEditor::text_style`: the editor's unformatted text in a theme
//! text style, measured as a label in that style is, through the typesetter
//! every application shapes with. Without it, the 14 px default stays.

use std::any::TypeId;
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::presets::intui;
use teksilo_core::styles::Theme;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_text::SharedTypesetter;
use teksilo_text::text_document::{MoveMode, TextDocument, TextFormat};
use teksilo_tokens::TextStyleRole;

use super::{EditorHandle, RichTextEditor};
use crate::primitives::{TextWidget, VStack};

/// A tree measuring with text-typeset, as an application's does.
fn app_tree(theme: Theme) -> WidgetTree {
    let shared = SharedTypesetter::new_with_default_font();
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(shared.as_text_backend());
    let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
    registry.insert(TypeId::of::<SharedTypesetter>(), Box::new(shared));
    let ctx = teksilo_core::event_source::TreeAppContext::empty().with_app_state(registry);
    tree.set_app_context(Rc::new(ctx));
    tree
}

/// A theme whose body text is 16 px of the monospaced face, unlike the
/// default's 13 px of Inter.
fn large_mono_body() -> Theme {
    let mut theme = intui::light();
    theme.typography.body.size = 16.0;
    theme.typography.body.family = theme.typography.mono.family.clone();
    theme
}

fn settle(tree: &mut WidgetTree) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
    }
}

/// One line of `text`: a body label beside `editor`. Returns the label's
/// height and the editor's laid-out text height.
fn label_and_editor(tree: &mut WidgetTree, text: &str, editor: RichTextEditor) -> (f32, f32) {
    let handle = editor.handle();
    let label = TextWidget::new(lit!(text.to_owned())).style(TextStyleRole::Body);
    let (label_id, _) = mount(tree, label, editor);
    settle(tree);
    (tree.bounds(label_id).height, text_height(&handle))
}

fn mount(tree: &mut WidgetTree, label: TextWidget, editor: RichTextEditor) -> (WidgetId, WidgetId) {
    let label_id = tree.add(label);
    let editor_id = tree.add(editor);
    tree.add(VStack::new().child(label_id).child(editor_id));
    (label_id, editor_id)
}

fn text_height(handle: &EditorHandle) -> f32 {
    handle
        .content_height()
        .expect("the editor has laid its text out")
}

fn editor_over(text: &str) -> RichTextEditor {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    RichTextEditor::read_only(doc)
}

#[test]
fn body_style_lines_measure_as_a_body_label() {
    for theme in [intui::light(), large_mono_body()] {
        let mut tree = app_tree(theme);
        let (label, editor) = label_and_editor(
            &mut tree,
            "Are you there?",
            editor_over("Are you there?").text_style(TextStyleRole::Body),
        );
        assert!(
            (label - editor).abs() < 0.01,
            "a body line is a body label's height: label {label}, editor {editor}"
        );
    }
}

#[test]
fn the_body_style_follows_the_themes_size() {
    let mut small = app_tree(intui::light());
    let (_, at_13) = label_and_editor(
        &mut small,
        "Hi",
        editor_over("Hi").text_style(TextStyleRole::Body),
    );
    let mut large = app_tree(large_mono_body());
    let (_, at_16) = label_and_editor(
        &mut large,
        "Hi",
        editor_over("Hi").text_style(TextStyleRole::Body),
    );
    assert!(at_16 > at_13 + 2.0, "13 px gives {at_13}, 16 px {at_16}");
}

/// Without `text_style`, nothing moves: a document editor's text sizes are
/// the document's, and the theme's body size is not one of them.
#[test]
fn without_a_text_style_the_theme_changes_nothing() {
    let height_in = |theme: Theme, editable: bool| {
        let doc = TextDocument::new();
        doc.set_plain_text("Hi").unwrap();
        let editor = if editable {
            RichTextEditor::editor(doc)
        } else {
            RichTextEditor::read_only(doc)
        };
        let handle = editor.handle();
        let mut tree = app_tree(theme);
        tree.add(editor);
        settle(&mut tree);
        text_height(&handle)
    };
    for editable in [true, false] {
        assert_eq!(
            height_in(intui::light(), editable),
            height_in(large_mono_body(), editable),
            "editable: {editable}"
        );
    }
}

/// A run whose format names a size keeps it.
#[test]
fn a_size_the_document_names_wins() {
    let sized = || {
        let doc = TextDocument::new();
        doc.set_plain_text("Big").unwrap();
        let cursor = doc.cursor();
        cursor.set_position(0, MoveMode::MoveAnchor);
        cursor.set_position(3, MoveMode::KeepAnchor);
        cursor
            .set_char_format(&TextFormat {
                font_point_size: Some(30),
                ..TextFormat::default()
            })
            .unwrap();
        RichTextEditor::read_only(doc)
    };
    let height_of = |editor: RichTextEditor| {
        let handle = editor.handle();
        let mut tree = app_tree(intui::light());
        tree.add(editor);
        settle(&mut tree);
        text_height(&handle)
    };
    let plain = height_of(sized());
    let styled = height_of(sized().text_style(TextStyleRole::Body));
    assert!(plain > 30.0, "the run is 30 px: {plain}");
    assert_eq!(plain, styled);
}

#[test]
fn a_role_follows_a_theme_change() {
    let mut tree = app_tree(intui::light());
    let editor = editor_over("Hi").text_style(TextStyleRole::Body);
    let handle = editor.handle();
    tree.add(editor);
    settle(&mut tree);
    let before = text_height(&handle);

    tree.set_theme(large_mono_body());
    settle(&mut tree);
    let after = text_height(&handle);

    let mut reference = app_tree(large_mono_body());
    let (label, _) = label_and_editor(&mut reference, "Hi", editor_over("Hi"));
    assert!(after > before, "before {before}, after {after}");
    assert!((after - label).abs() < 0.01, "after {after}, label {label}");
}

/// The global text scale multiplies the style's size once, as it multiplies
/// a label's.
#[test]
fn the_text_scale_applies_once() {
    let mut tree = app_tree(intui::light());
    tree.set_user_text_scale(2.0);
    let (label, editor) = label_and_editor(
        &mut tree,
        "Hi",
        editor_over("Hi").text_style(TextStyleRole::Body),
    );
    assert!(
        (label - editor).abs() < 0.01,
        "label {label}, editor {editor} at twice the size"
    );
}

/// `min_lines` counts lines of the style's height.
#[test]
fn min_lines_counts_lines_of_the_style() {
    let mut tree = app_tree(large_mono_body());
    let label = TextWidget::new(lit!("Hi")).style(TextStyleRole::Body);
    let editor = editor_over("").text_style(TextStyleRole::Body).min_lines(3);
    let (label_id, editor_id) = mount(&mut tree, label, editor);
    settle(&mut tree);
    let line = tree.bounds(label_id).height;
    let editor = tree.bounds(editor_id).height;
    assert!(
        (editor - 3.0 * line).abs() < 0.5,
        "three lines of {line} make {editor}"
    );
}
