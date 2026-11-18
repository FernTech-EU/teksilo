// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `RichTextEditor::fit_width`: an editor as wide as its widest line at the
//! width it is offered, measured through the typesetter every application
//! shapes with, and laid out at that width without wrapping anywhere else.

use std::any::TypeId;
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::presets::intui;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_text::SharedTypesetter;
use teksilo_text::text_document::{Alignment, BlockFormat, MoveMode, TextDocument};
use teksilo_tokens::TextStyleRole;

use super::{EditorHandle, RichTextEditor};
use crate::primitives::{HStack, Spacer, TextWidget, VStack};

/// The slack `TextWidget` adds to the width it lays a text out at.
const SLACK: f32 = 0.5;

fn app_tree() -> WidgetTree {
    let shared = SharedTypesetter::new_with_default_font();
    let mut tree = WidgetTree::new()
        .with_theme(intui::light())
        .with_text_backend(shared.as_text_backend());
    let mut registry: HashMap<TypeId, Box<dyn std::any::Any>> = HashMap::new();
    registry.insert(TypeId::of::<SharedTypesetter>(), Box::new(shared));
    let ctx = teksilo_core::event_source::TreeAppContext::empty().with_app_state(registry);
    tree.set_app_context(Rc::new(ctx));
    tree
}

fn settle(tree: &mut WidgetTree, width: f32) {
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(SizeProposal::exact(width, 300.0));
        let _ = tree.render();
    }
}

fn doc_with(text: &str) -> TextDocument {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    doc
}

/// A body-styled viewer over `text`, fitting its width.
fn viewer(text: &str) -> RichTextEditor {
    RichTextEditor::read_only(doc_with(text))
        .text_style(TextStyleRole::Body)
        .fit_width(true)
}

/// `editor` beside a spacer in a row `width` wide, as a chat bubble sits.
fn in_a_row(tree: &mut WidgetTree, editor: RichTextEditor, width: f32) -> WidgetId {
    let id = tree.add(editor);
    tree.add(HStack::new().child(id).child(Spacer::new()));
    settle(tree, width);
    id
}

/// `editor` in a column `width` wide, which offers it that width: a text
/// longer than that wraps there.
fn in_a_column(tree: &mut WidgetTree, editor: RichTextEditor, width: f32) -> WidgetId {
    let id = tree.add(editor);
    tree.add(VStack::new().child(id));
    settle(tree, width);
    id
}

/// The width a body label gives `text` on one line.
fn label_width(text: &str) -> f32 {
    let mut tree = app_tree();
    let id = tree.add(TextWidget::new(lit!(text.to_owned())).style(TextStyleRole::Body));
    tree.add(HStack::new().child(id).child(Spacer::new()));
    settle(&mut tree, 1000.0);
    tree.bounds(id).width
}

fn text_height(handle: &EditorHandle) -> f32 {
    handle
        .content_height()
        .expect("the editor has laid its text out")
}

/// The issue's reproduction: a short message makes a narrow viewer.
#[test]
fn a_short_text_makes_a_narrow_viewer() {
    let mut tree = app_tree();
    let id = in_a_row(&mut tree, viewer("Hi").min_lines(1), 400.0);
    let width = tree.bounds(id).width;
    let label = label_width("Hi");
    assert!(
        (width - (label + SLACK)).abs() < 0.01,
        "\"Hi\" is {label} wide, the viewer {width}"
    );
}

/// Without `fit_width` the viewer still takes what it is offered.
#[test]
fn without_fit_width_the_viewer_takes_the_row() {
    let mut tree = app_tree();
    let id = in_a_row(
        &mut tree,
        RichTextEditor::read_only(doc_with("Hi")).min_lines(1),
        400.0,
    );
    assert!(tree.bounds(id).width > 100.0);
}

const LONG: &str = "A message long enough to wrap at the width of the row, over \
                    several lines, which is what a chat bubble does with one.";

/// A text that wraps is as wide as its widest line at the offered width, and
/// wraps there exactly as it would at the full width.
#[test]
fn a_wrapping_text_is_as_wide_as_its_widest_line() {
    let mut fitted = app_tree();
    let editor = viewer(LONG).min_lines(1);
    let fitted_handle = editor.handle();
    let fitted_id = in_a_column(&mut fitted, editor, 300.0);

    let mut full = app_tree();
    let editor = RichTextEditor::read_only(doc_with(LONG))
        .text_style(TextStyleRole::Body)
        .min_lines(1);
    let full_handle = editor.handle();
    let full_id = in_a_column(&mut full, editor, 300.0);
    let widest = full_handle.state.borrow().engine.max_content_width();

    let width = fitted.bounds(fitted_id).width;
    assert!(width < 300.0, "narrower than the row: {width}");
    assert!(
        (width - (widest + SLACK)).abs() < 0.01,
        "the widest line is {widest}, the viewer {width}"
    );
    assert_eq!(
        text_height(&fitted_handle),
        text_height(&full_handle),
        "the same lines, so the same height"
    );
    assert_eq!(fitted.bounds(fitted_id).height, full.bounds(full_id).height);
}

/// A stack measures a child with no width, then gives it the width it asked
/// for: the viewer asks for the same width again, pass after pass.
#[test]
fn measured_then_given_its_width_it_keeps_it() {
    let mut tree = app_tree();
    let id = tree.add(viewer("Are you there?").min_lines(1));
    let row = tree.add(HStack::new().child(id).child(Spacer::new()));
    tree.add(VStack::new().child(row));
    settle(&mut tree, 400.0);
    let first = tree.bounds(id).width;
    for _ in 0..5 {
        settle(&mut tree, 400.0);
        assert_eq!(tree.bounds(id).width, first);
    }
    let label = label_width("Are you there?");
    assert!(
        (first - (label + SLACK)).abs() < 0.01,
        "one line: label {label}, viewer {first}"
    );
}

#[test]
fn an_unbounded_width_is_the_longest_paragraph_on_one_line() {
    let mut tree = app_tree();
    let editor = viewer("short\na longer paragraph").min_lines(1);
    let handle = editor.handle();
    let id = tree.add(editor);
    let row = tree.add(HStack::new().child(id).child(Spacer::new()));
    tree.add(VStack::new().child(row));
    settle(&mut tree, 1000.0);
    let label = label_width("a longer paragraph");
    let width = tree.bounds(id).width;
    assert!((width - (label + SLACK)).abs() < 0.01, "{label} / {width}");
    let line = label_height();
    let height = text_height(&handle);
    assert!(
        (height - 2.0 * line).abs() < 0.5,
        "two lines of {line}, no wrap: {height}"
    );
}

fn label_height() -> f32 {
    let mut tree = app_tree();
    let id = tree.add(TextWidget::new(lit!("Hi")).style(TextStyleRole::Body));
    tree.add(VStack::new().child(id));
    settle(&mut tree, 400.0);
    tree.bounds(id).height
}

#[test]
fn max_lines_caps_the_height_of_a_fitted_viewer() {
    let mut tree = app_tree();
    let id = in_a_column(&mut tree, viewer(LONG).max_lines(2), 300.0);
    let bounds = tree.bounds(id);
    assert!(bounds.width < 300.0);
    assert!(
        (bounds.height - 2.0 * label_height()).abs() < 0.5,
        "two lines: {}",
        bounds.height
    );
}

/// An empty editor keeps room for its caret, and grows as it is typed into.
#[test]
fn an_empty_editor_keeps_its_caret_and_grows_with_its_text() {
    let mut tree = app_tree();
    let editor = RichTextEditor::editor(TextDocument::new())
        .text_style(TextStyleRole::Body)
        .fit_width(true)
        .min_lines(1);
    let handle = editor.handle();
    let id = in_a_row(&mut tree, editor, 400.0);
    let empty = tree.bounds(id).width;
    assert!(empty > 0.0, "an empty editor has width for its caret");

    tree.focus(tree.first_focusable_descendant(id).unwrap_or(id));
    handle.insert_text("Hello there");
    settle(&mut tree, 400.0);
    let typed = tree.bounds(id).width;
    assert!(
        typed > empty + label_width("Hello there") - 1.0,
        "empty {empty}, typed {typed}"
    );
}

/// A centred paragraph sits in the middle of the fitted width.
#[test]
fn a_centred_paragraph_is_centred_in_the_fitted_width() {
    let doc = doc_with("Hi\nHello there friend");
    let cursor = doc.cursor();
    cursor.set_position(0, MoveMode::MoveAnchor);
    cursor
        .set_block_format(&BlockFormat {
            alignment: Some(Alignment::Center),
            ..BlockFormat::default()
        })
        .unwrap();
    let editor = RichTextEditor::read_only(doc)
        .text_style(TextStyleRole::Body)
        .fit_width(true)
        .min_lines(1);
    let handle = editor.handle();
    let mut tree = app_tree();
    let id = in_a_row(&mut tree, editor, 400.0);
    let width = tree.bounds(id).width;
    assert!((width - (label_width("Hello there friend") + SLACK)).abs() < 0.01);
    let start = handle
        .offset_content_rect(0)
        .expect("the caret rect of the first line");
    let expected = (width - label_width("Hi")) / 2.0;
    assert!(
        (start.x - expected).abs() < 1.0,
        "\"Hi\" starts at {}, its centred place is {expected}",
        start.x
    );
}

/// A right-to-left paragraph starts at the right edge of the fitted width.
#[test]
fn a_right_to_left_paragraph_starts_at_the_fitted_right_edge() {
    let editor = viewer("Hi\nHello there friend")
        .base_direction(crate::text_base_direction::TextBaseDirection::RightToLeft)
        .min_lines(1);
    let handle = editor.handle();
    let mut tree = app_tree();
    let id = in_a_row(&mut tree, editor, 400.0);
    let width = tree.bounds(id).width;
    assert!((width - (label_width("Hello there friend") + SLACK)).abs() < 0.01);
    let hi = handle.range_content_rect(0, 2).expect("the first line");
    assert!(
        (hi.x + hi.width - width).abs() < 1.0,
        "\"Hi\" ends at {} in a box {width} wide",
        hi.x + hi.width
    );
}

/// The messenger layout: in a row, under a cap of three quarters of it, a
/// short message is narrow and a long one wraps at the cap.
#[test]
fn a_bubble_under_a_cap_wraps_at_the_cap() {
    let in_a_bubble = |text: &str| {
        let mut tree = app_tree();
        let id = tree.add(viewer(text).min_lines(1));
        let capped = tree.add(crate::primitives::MaxSize::width(300.0).child(id));
        tree.add(HStack::new().child(capped).child(Spacer::new()));
        settle(&mut tree, 400.0);
        tree.bounds(id).width
    };
    let short = in_a_bubble("Hi");
    assert!(
        (short - (label_width("Hi") + SLACK)).abs() < 0.01,
        "{short}"
    );
    let long = in_a_bubble(LONG);
    assert!(long > 250.0 && long <= 300.0, "{long}");
}

/// Without `min_lines` / `max_lines` the height is the proposal's, and the
/// width still follows the text.
#[test]
fn a_greedy_viewer_fits_its_width_too() {
    let mut tree = app_tree();
    let id = in_a_row(&mut tree, viewer("Hi"), 400.0);
    let width = tree.bounds(id).width;
    let label = label_width("Hi");
    assert!(
        (width - (label + SLACK)).abs() < 0.01,
        "\"Hi\" is {label} wide, the viewer {width}"
    );
}

fn markdown(text: &str) -> TextDocument {
    let doc = TextDocument::new();
    doc.set_markdown(text).unwrap().wait().unwrap();
    doc
}

/// A paragraph whose first line is indented by `indent` pixels.
fn indented(text: &str, indent: i32) -> TextDocument {
    let doc = doc_with(text);
    let cursor = doc.cursor();
    cursor.set_position(0, MoveMode::MoveAnchor);
    cursor
        .set_block_format(&BlockFormat {
            text_indent: Some(indent),
            ..BlockFormat::default()
        })
        .unwrap();
    doc
}

/// The height of the text of `doc` laid out in a column far wider than it,
/// where no line wraps.
fn unwrapped_height(doc: TextDocument) -> f32 {
    let mut tree = app_tree();
    let editor = RichTextEditor::read_only(doc)
        .text_style(TextStyleRole::Body)
        .min_lines(1);
    let handle = editor.handle();
    in_a_column(&mut tree, editor, 2000.0);
    text_height(&handle)
}

/// A fitted body viewer over `doc` beside a spacer, which measures it with no
/// width: its width and the height of its text as laid out at that width.
fn fitted_in_a_row(doc: TextDocument) -> (f32, f32) {
    let mut tree = app_tree();
    let editor = RichTextEditor::read_only(doc)
        .text_style(TextStyleRole::Body)
        .fit_width(true)
        .min_lines(1);
    let handle = editor.handle();
    let id = in_a_row(&mut tree, editor, 1000.0);
    (tree.bounds(id).width, text_height(&handle))
}

/// What the bridge gives a blockquote that names none of its own: a 16 px
/// left margin, a 3 px border and 8 px of padding, the last two taken off
/// both sides.
const QUOTE_CHROME: f32 = 16.0 + 2.0 * (3.0 + 8.0);

/// A quote is as wide as its text and its own margin, border and padding,
/// and laid out at that width keeps its text on one line.
#[test]
fn a_quote_is_as_wide_as_its_text_and_its_frame() {
    let (width, height) = fitted_in_a_row(markdown("> Hi there\n"));
    assert!(width.is_finite(), "a quote has a natural width: {width}");
    let expected = label_width("Hi there") + QUOTE_CHROME + SLACK;
    assert!(
        (width - expected).abs() < 0.01,
        "the quote is {width} wide, its text and frame {expected}"
    );
    assert_eq!(
        height,
        unwrapped_height(markdown("> Hi there\n")),
        "one line"
    );
}

#[test]
fn a_nested_quote_counts_both_frames() {
    let (width, height) = fitted_in_a_row(markdown("> > Hi there\n"));
    let expected = label_width("Hi there") + 2.0 * QUOTE_CHROME + SLACK;
    assert!(
        (width - expected).abs() < 0.01,
        "the nested quote is {width} wide, its text and frames {expected}"
    );
    assert_eq!(
        height,
        unwrapped_height(markdown("> > Hi there\n")),
        "one line"
    );
}

/// A table's columns share the width it is given, so it has none of its own:
/// offered a width, the editor takes it; offered none, the table is laid out
/// as text-typeset lays it out then, and the width is finite.
#[test]
fn a_table_takes_the_offered_width_and_stays_finite_unbounded() {
    let with_table = || {
        let doc = doc_with("Hi");
        let cursor = doc.cursor();
        cursor.set_position(2, MoveMode::MoveAnchor);
        cursor.insert_table(2, 2).unwrap();
        doc
    };
    let mut tree = app_tree();
    let id = in_a_column(
        &mut tree,
        RichTextEditor::read_only(with_table())
            .text_style(TextStyleRole::Body)
            .fit_width(true)
            .min_lines(1),
        300.0,
    );
    assert_eq!(tree.bounds(id).width, 300.0);

    let (width, _) = fitted_in_a_row(with_table());
    assert!(
        width.is_finite() && width < 1000.0,
        "offered no width, a table still has a finite one: {width}"
    );
}

/// The first line of a paragraph is broken against the width less its
/// indent, so the indent is part of the width it needs.
#[test]
fn a_first_line_indent_is_counted() {
    let (width, height) = fitted_in_a_row(indented("Hi there", 20));
    let expected = label_width("Hi there") + 20.0 + SLACK;
    assert!(
        (width - expected).abs() < 0.01,
        "the paragraph is {width} wide, its text and indent {expected}"
    );
    assert_eq!(
        height,
        unwrapped_height(indented("Hi there", 20)),
        "one line"
    );
}

/// A hanging (negative) indent gives the first line more room than the rest:
/// it needs less, and laid out at what it asks for nothing wraps.
#[test]
fn a_hanging_indent_does_not_wrap_the_first_line() {
    let (width, height) = fitted_in_a_row(indented("Hi there", -10));
    assert!(width.is_finite() && width > 0.0, "{width}");
    assert_eq!(
        height,
        unwrapped_height(indented("Hi there", -10)),
        "one line"
    );
}

/// A list item's text starts after its marker's indent, which it needs too.
#[test]
fn a_list_item_counts_its_indent() {
    let (width, height) = fitted_in_a_row(markdown("- Hi there\n"));
    let text = label_width("Hi there");
    assert!(
        width > text + SLACK + 1.0,
        "the item is {width} wide, its text alone {text}"
    );
    assert_eq!(
        height,
        unwrapped_height(markdown("- Hi there\n")),
        "one line"
    );
}

/// Measured unbounded, then given that width back by a stack, pass after
/// pass: the width and the lines stay.
fn assert_stable(doc: TextDocument) {
    let mut tree = app_tree();
    let editor = RichTextEditor::read_only(doc)
        .text_style(TextStyleRole::Body)
        .fit_width(true)
        .min_lines(1);
    let handle = editor.handle();
    let id = tree.add(editor);
    let row = tree.add(HStack::new().child(id).child(Spacer::new()));
    tree.add(VStack::new().child(row));
    settle(&mut tree, 1000.0);
    let width = tree.bounds(id).width;
    let height = text_height(&handle);
    assert!(width.is_finite());
    for _ in 0..5 {
        settle(&mut tree, 1000.0);
        assert_eq!(tree.bounds(id).width, width);
        assert_eq!(text_height(&handle), height);
    }
}

#[test]
fn a_quote_keeps_its_width_pass_after_pass() {
    assert_stable(markdown("> Are you there?\n"));
}

#[test]
fn an_indented_paragraph_keeps_its_width_pass_after_pass() {
    assert_stable(indented("Are you there?", 20));
}

/// A larger text size edits nothing, and still moves the width.
#[test]
fn a_new_font_size_scale_is_measured_again() {
    let text = "Are you there?";
    let mut tree = app_tree();
    let editor = viewer(text);
    let handle = editor.handle();
    let id = in_a_row(&mut tree, editor, 1000.0);
    let before = tree.bounds(id).width;

    handle.set_font_size_scale(1.5);
    settle(&mut tree, 1000.0);
    let after = tree.bounds(id).width;

    let mut fresh = app_tree();
    let editor = viewer(text).font_size_scale(1.5);
    let fresh_handle = editor.handle();
    let fresh_id = in_a_row(&mut fresh, editor, 1000.0);
    assert!(after > before * 1.4, "{before} then {after}");
    assert_eq!(after, fresh.bounds(fresh_id).width);
    assert_eq!(text_height(&handle), text_height(&fresh_handle), "one line");
}

/// The first measurement comes before the first paint, which is where the
/// language and the directions used to reach the engine.
#[test]
fn the_first_measurement_knows_the_base_direction() {
    let mut tree = app_tree();
    let editor =
        viewer("Hi").base_direction(crate::text_base_direction::TextBaseDirection::RightToLeft);
    let handle = editor.handle();
    let id = tree.add(editor);
    tree.add(HStack::new().child(id).child(Spacer::new()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        handle.state.borrow().engine.default_direction(),
        teksilo_text::TextDirection::RightToLeft
    );
}
