// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The list toolbar commands: `current_list_style`, `set_list_style` and
//! `toggle_list`, through the same `EditorHandle` a toolbar holds.

use teksilo_canvas::SizeProposal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{ListStyle, TextDocument};

use super::{EditorHandle, RichTextEditor, list_style_is_ordered};

/// A laid-out editor over `markdown`, and the handle a toolbar would hold.
/// The tree owns the editor and must outlive the handle's use.
fn editor(markdown: &str) -> (TextDocument, EditorHandle, WidgetTree) {
    let doc = TextDocument::new();
    doc.set_markdown(markdown).unwrap().wait().unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let mut tree = WidgetTree::new();
    tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (doc, handle, tree)
}

/// Where `needle` starts in the document's plain text, in characters.
fn offset_of(doc: &TextDocument, needle: &str) -> usize {
    let text = doc.to_plain_text().unwrap();
    let byte = text.find(needle).expect("the needle is in the document");
    text[..byte].chars().count()
}

/// The caret goes inside `needle`, one character in.
fn caret_in(doc: &TextDocument, handle: &EditorHandle, needle: &str) {
    let at = offset_of(doc, needle) + 1;
    handle.select_range(at, at);
}

/// The list style of the paragraph holding `needle`, `None` outside a list.
fn style_of(doc: &TextDocument, needle: &str) -> Option<ListStyle> {
    doc.block_at_position(offset_of(doc, needle))
        .and_then(|block| block.list())
        .map(|list| list.style())
}

/// The id of the list the paragraph holding `needle` belongs to.
fn list_id_of(doc: &TextDocument, needle: &str) -> Option<usize> {
    doc.block_at_position(offset_of(doc, needle))
        .and_then(|block| block.list())
        .map(|list| list.id())
}

#[test]
fn the_kind_of_every_style() {
    for style in [ListStyle::Disc, ListStyle::Circle, ListStyle::Square] {
        assert!(!list_style_is_ordered(&style), "{style:?} is a bullet");
    }
    for style in [
        ListStyle::Decimal,
        ListStyle::LowerAlpha,
        ListStyle::UpperAlpha,
        ListStyle::LowerRoman,
        ListStyle::UpperRoman,
    ] {
        assert!(list_style_is_ordered(&style), "{style:?} numbers");
    }
}

#[test]
fn the_caret_outside_a_list_has_no_style() {
    let (doc, handle, _tree) = editor("Plain paragraph\n");
    caret_in(&doc, &handle, "Plain");
    assert_eq!(handle.current_list_style(), None);
}

#[test]
fn the_caret_reports_the_style_of_its_list() {
    let (doc, handle, _tree) = editor("1. First\n2. Second\n");
    caret_in(&doc, &handle, "Second");
    assert_eq!(handle.current_list_style(), Some(ListStyle::Decimal));
}

#[test]
fn bullets_outside_a_list_make_one() {
    let (doc, handle, _tree) = editor("Plain paragraph\n");
    caret_in(&doc, &handle, "Plain");
    handle.toggle_list(false);
    assert_eq!(handle.current_list_style(), Some(ListStyle::Disc));
    assert_eq!(doc.to_plain_text().unwrap(), "Plain paragraph");
}

#[test]
fn numbering_outside_a_list_makes_one() {
    let (doc, handle, _tree) = editor("Plain paragraph\n");
    caret_in(&doc, &handle, "Plain");
    handle.toggle_list(true);
    assert_eq!(handle.current_list_style(), Some(ListStyle::Decimal));
}

#[test]
fn bullets_in_a_bullet_list_take_the_paragraph_out() {
    let (doc, handle, _tree) = editor("- First\n- Second\n- Third\n");
    caret_in(&doc, &handle, "Second");
    handle.toggle_list(false);
    assert_eq!(handle.current_list_style(), None);
    assert_eq!(style_of(&doc, "First"), Some(ListStyle::Disc));
    assert_eq!(style_of(&doc, "Third"), Some(ListStyle::Disc));
}

/// A circle is a bullet as much as a disc is: the button stands for the kind.
#[test]
fn bullets_in_a_circle_list_take_the_paragraph_out() {
    let (doc, handle, _tree) = editor("Plain paragraph\n");
    caret_in(&doc, &handle, "Plain");
    handle.create_list(ListStyle::Circle);
    handle.toggle_list(false);
    assert_eq!(handle.current_list_style(), None);
}

#[test]
fn numbering_in_a_roman_list_takes_the_paragraph_out() {
    let (doc, handle, _tree) = editor("Plain paragraph\n");
    caret_in(&doc, &handle, "Plain");
    handle.create_list(ListStyle::UpperRoman);
    handle.toggle_list(true);
    assert_eq!(handle.current_list_style(), None);
}

/// Without a selection the whole list changes kind, and stays one list.
#[test]
fn numbering_in_a_bullet_list_renumbers_the_whole_list() {
    let (doc, handle, _tree) = editor("- First\n- Second\n- Third\n");
    let list = list_id_of(&doc, "First");
    caret_in(&doc, &handle, "Second");
    handle.toggle_list(true);
    for item in ["First", "Second", "Third"] {
        assert_eq!(style_of(&doc, item), Some(ListStyle::Decimal), "{item}");
        assert_eq!(list_id_of(&doc, item), list, "{item} stays in the list");
    }
}

#[test]
fn bullets_in_a_numbered_list_turn_it_into_bullets() {
    let (doc, handle, _tree) = editor("1. First\n2. Second\n");
    caret_in(&doc, &handle, "First");
    handle.toggle_list(false);
    assert_eq!(style_of(&doc, "First"), Some(ListStyle::Disc));
    assert_eq!(style_of(&doc, "Second"), Some(ListStyle::Disc));
}

/// With a selection, every selected paragraph leaves, and only those.
#[test]
fn a_selection_takes_every_selected_paragraph_out() {
    let (doc, handle, _tree) = editor("- First\n- Second\n- Third\n");
    let start = offset_of(&doc, "First") + 1;
    let end = offset_of(&doc, "Second") + 2;
    handle.select_range(start, end);
    handle.toggle_list(false);
    assert_eq!(style_of(&doc, "First"), None);
    assert_eq!(style_of(&doc, "Second"), None);
    assert_eq!(style_of(&doc, "Third"), Some(ListStyle::Disc));
}

/// A selection ending at the end of a paragraph does not reach the next one.
#[test]
fn a_selection_ending_at_a_paragraph_end_leaves_the_next_one_alone() {
    let (doc, handle, _tree) = editor("- First\n- Second\n");
    let start = offset_of(&doc, "First");
    handle.select_range(start, start + "First".chars().count());
    handle.toggle_list(false);
    assert_eq!(style_of(&doc, "First"), None);
    assert_eq!(style_of(&doc, "Second"), Some(ListStyle::Disc));
}

#[test]
fn a_toggle_is_one_undo_step() {
    let (doc, handle, _tree) = editor("- First\n- Second\n");
    caret_in(&doc, &handle, "Second");
    handle.toggle_list(true);
    assert_eq!(style_of(&doc, "First"), Some(ListStyle::Decimal));
    handle.undo();
    assert_eq!(style_of(&doc, "First"), Some(ListStyle::Disc));
    assert_eq!(style_of(&doc, "Second"), Some(ListStyle::Disc));
}

#[test]
fn set_list_style_keeps_the_depth() {
    let (doc, handle, _tree) = editor("- First\n");
    caret_in(&doc, &handle, "First");
    handle.indent();
    let depth = doc
        .block_at_position(offset_of(&doc, "First"))
        .and_then(|block| block.list())
        .map(|list| list.indent());
    handle.set_list_style(ListStyle::LowerRoman);
    let after = doc
        .block_at_position(offset_of(&doc, "First"))
        .and_then(|block| block.list());
    assert_eq!(
        after.as_ref().map(|l| l.style()),
        Some(ListStyle::LowerRoman)
    );
    assert_eq!(after.map(|l| l.indent()), depth);
}

#[test]
fn set_list_style_outside_a_list_does_nothing() {
    let (doc, handle, _tree) = editor("Plain paragraph\n");
    caret_in(&doc, &handle, "Plain");
    handle.set_list_style(ListStyle::Decimal);
    assert_eq!(handle.current_list_style(), None);
}

/// With a selection in a list of the other kind, the selected paragraphs
/// become a list of the new kind and the rest keep theirs.
#[test]
fn a_selection_in_the_other_kind_becomes_a_new_list() {
    let (doc, handle, _tree) = editor("1. First\n2. Second\n3. Third\n");
    let start = offset_of(&doc, "First") + 1;
    let end = offset_of(&doc, "Second") + 2;
    handle.select_range(start, end);
    handle.toggle_list(false);
    assert_eq!(style_of(&doc, "First"), Some(ListStyle::Disc));
    assert_eq!(style_of(&doc, "Second"), Some(ListStyle::Disc));
    assert_eq!(style_of(&doc, "Third"), Some(ListStyle::Decimal));
}
