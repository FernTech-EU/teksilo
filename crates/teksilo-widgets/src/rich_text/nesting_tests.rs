// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The nesting ceiling: how deep Tab, `indent`, `increase_blockquote_depth`
//! and the blockquote toggle may take a block.
//!
//! Tab in a blockquote wraps the block in one more quote, and Tab in a list
//! moves the item one level deeper. Both used to go on for as long as the key
//! was held: about a hundred presses built a quote no Djot loader that bounds
//! its parser's recursion would open again. These tests hold the gestures at
//! the ceiling, and check that everything that takes a block back out still
//! works there, and that a list item in a quote or a table cell moves between
//! levels as one in the main text does.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{DocumentFragment, FlowElement, MoveMode, TextDocument};

use super::nesting::{self, MAX_BLOCKQUOTE_DEPTH, MAX_LIST_DEPTH};
use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};

/// The quote ceiling, in levels.
const QUOTE_CEILING: usize = MAX_BLOCKQUOTE_DEPTH;
/// The list ceiling, in levels (a top-level item is level 1).
const LIST_CEILING: usize = MAX_LIST_DEPTH;

/// Far more presses than any ceiling, so a test that stops short proves the
/// ceiling and not the count.
const HELD: usize = 200;

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

/// Mount `editor` in a fresh tree and give it the keyboard focus.
///
/// Focusing clicks near the top-left corner of the text, which moves the
/// caret; a test that needs a particular caret or selection sets it after
/// this returns.
fn mount_focused(editor: RichTextEditor) -> WidgetTree {
    let mut tree = WidgetTree::new();
    let id = tree.add(editor);
    tree.layout(viewport());
    let _ = tree.render();
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(20.0, 20.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(20.0, 20.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    let focused = tree.focused();
    assert!(
        focused.is_some_and(|f| f == id || tree.is_descendant_of(f, id)),
        "the click must focus the editor"
    );
    tree
}

fn press(tree: &mut WidgetTree, key: Key, modifiers: Modifiers, times: usize) {
    for _ in 0..times {
        tree.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
    }
    settle(tree);
}

fn settle(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(200));
    tree.layout(viewport());
}

fn quote_depth(state: &SharedState) -> usize {
    state.borrow().cursor.blockquote_depth_at_cursor()
}

/// The caret's list level: 1 for a top-level item, 0 outside any list.
fn list_level(state: &SharedState) -> usize {
    state
        .borrow()
        .cursor
        .current_list()
        .map_or(0, |list| usize::from(list.indent()) + 1)
}

/// Quote depth of the block holding `position`, read without moving the
/// editor's own caret.
fn quote_depth_at(doc: &TextDocument, position: usize) -> usize {
    let probe = doc.cursor();
    probe.set_position(position, MoveMode::MoveAnchor);
    probe.blockquote_depth_at_cursor()
}

/// Where `needle` starts in the document's plain text, in characters.
fn char_offset_of(doc: &TextDocument, needle: &str) -> usize {
    let text = doc.to_plain_text().unwrap();
    let byte = text.find(needle).expect("the needle is in the document");
    text[..byte].chars().count()
}

fn quoted(markdown: &str) -> TextDocument {
    let doc = TextDocument::new();
    doc.set_markdown(markdown).unwrap().wait().unwrap();
    doc
}

/// `Before`, a quote holding `Deep`, and `After`, with `Deep` already wrapped
/// to the quote ceiling through the programmatic command.
fn deep_quote_between_paragraphs() -> (TextDocument, RichTextEditor) {
    let doc = quoted("Before\n\n> Deep\n\nAfter\n");
    let editor = RichTextEditor::editor(doc.clone());
    editor.set_caret_position(char_offset_of(&doc, "Deep"));
    for _ in 1..QUOTE_CEILING {
        editor.increase_blockquote_depth();
    }
    assert_eq!(
        quote_depth_at(&doc, char_offset_of(&doc, "Deep")),
        QUOTE_CEILING,
        "setup: `Deep` must sit at the quote ceiling"
    );
    (doc, editor)
}

#[test]
fn holding_tab_in_a_quote_stops_at_the_ceiling() {
    let doc = quoted("> Quoted line.\n");
    let text = doc.to_plain_text().unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();
    let mut tree = mount_focused(editor);
    assert_eq!(quote_depth(&state), 1);

    press(&mut tree, Key::Tab, Modifiers::NONE, HELD);

    assert_eq!(
        quote_depth(&state),
        QUOTE_CEILING,
        "Tab must nest the quote up to the ceiling and no further"
    );
    assert_eq!(
        doc.to_plain_text().unwrap(),
        text,
        "Tab at the ceiling must leave the text alone, not type a tab into it"
    );
}

#[test]
fn shift_tab_still_steps_out_of_a_quote_at_the_ceiling() {
    let doc = quoted("> Quoted line.\n");
    let editor = RichTextEditor::editor(doc);
    let state = editor.state_handle();
    let mut tree = mount_focused(editor);
    press(&mut tree, Key::Tab, Modifiers::NONE, HELD);

    press(&mut tree, Key::Tab, Modifiers::SHIFT, 1);
    assert_eq!(
        quote_depth(&state),
        QUOTE_CEILING - 1,
        "Shift+Tab must pop one level at the ceiling"
    );

    press(&mut tree, Key::Tab, Modifiers::NONE, 1);
    assert_eq!(
        quote_depth(&state),
        QUOTE_CEILING,
        "one level below the ceiling, Tab nests again"
    );
}

#[test]
fn holding_tab_in_a_list_stops_at_the_ceiling() {
    let doc = TextDocument::new();
    doc.set_plain_text("item").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.insert_list(false);
    let state = editor.state_handle();
    let mut tree = mount_focused(editor);
    assert_eq!(list_level(&state), 1);

    press(&mut tree, Key::Tab, Modifiers::NONE, HELD);

    assert_eq!(
        list_level(&state),
        LIST_CEILING,
        "Tab must indent the item up to the ceiling and no further"
    );
    assert_eq!(
        doc.to_plain_text().unwrap(),
        "item",
        "Tab at the ceiling must leave the text alone"
    );
}

#[test]
fn shift_tab_still_steps_out_of_a_list_at_the_ceiling() {
    let doc = TextDocument::new();
    doc.set_plain_text("item").unwrap();
    let editor = RichTextEditor::editor(doc);
    editor.insert_list(false);
    let state = editor.state_handle();
    let mut tree = mount_focused(editor);
    press(&mut tree, Key::Tab, Modifiers::NONE, HELD);

    press(&mut tree, Key::Tab, Modifiers::SHIFT, 1);
    assert_eq!(
        list_level(&state),
        LIST_CEILING - 1,
        "Shift+Tab must outdent one level at the ceiling"
    );
}

// ---------------------------------------------------------------------------
// A list item inside a quote or a table cell
//
// Moving an item to another level takes it out of its list and makes a new
// list of it at the new level. Up to text-document 1.12.2 the second step only
// found blocks in the main text, so inside a quote or a table cell the item
// left its list and joined none, and every later Tab nested the quote instead.
// From 1.12.3 on the rebuild works in every frame, and these tests hold the
// editor to moving the item exactly as it does in the main text.
// ---------------------------------------------------------------------------

/// One block as these tests read it: its text, how many quotes hold it, its
/// list level (1 for a top-level item, 0 outside a list) and its table cell.
type Listed = (String, usize, usize, Option<(usize, usize)>);

/// Every block of `doc`, in order, as [`Listed`] reads it.
///
/// Each block is read at its own start. The caret's own queries answer for
/// the next paragraph when the caret sits at the end of an item's text, so
/// they cannot tell where an item moved.
fn listing(doc: &TextDocument) -> Vec<Listed> {
    doc.blocks()
        .iter()
        .map(|block| {
            let level = block
                .list()
                .map_or(0, |list| usize::from(list.indent()) + 1);
            let cell = block.table_cell().map(|cell| (cell.row, cell.column));
            (
                block.text(),
                quote_depth_at(doc, block.position()),
                level,
                cell,
            )
        })
        .collect()
}

/// `before`, with the item reading `item` at `level` and every other block as
/// it was.
fn with_level(before: &[Listed], item: &str, level: usize) -> Vec<Listed> {
    before
        .iter()
        .map(|(text, quotes, old_level, cell)| {
            let level = if text == item { level } else { *old_level };
            (text.clone(), *quotes, level, *cell)
        })
        .collect()
}

/// The level of the item reading `item` in `listing`.
fn level_of(listing: &[Listed], item: &str) -> usize {
    listing
        .iter()
        .find(|(text, ..)| text == item)
        .map(|&(_, _, level, _)| level)
        .expect("the item is in the document")
}

/// The ways a writer or a toolbar moves the caret's list item a level: Tab and
/// Shift+Tab, or `indent` and `outdent` on the editor or on its handle.
#[derive(Clone, Copy, Debug)]
enum Through {
    Keys,
    Editor,
    Handle,
}

/// An editor over a document, moving list items [`Through`] one of those ways.
struct Mover {
    handle: EditorHandle,
    surface: Surface,
}

/// What each way of moving an item needs besides the handle.
enum Surface {
    /// A focused editor in a tree, for the keys. Boxed: a tree is far larger
    /// than the other variants.
    Keys(Box<WidgetTree>),
    /// The editor itself, whose own commands are asked.
    Editor(RichTextEditor),
    /// Nothing more: the handle's commands are asked.
    Handle,
}

impl Mover {
    fn new(doc: &TextDocument, through: Through) -> Self {
        let editor = RichTextEditor::editor(doc.clone());
        let handle = editor.handle();
        let surface = match through {
            Through::Keys => Surface::Keys(Box::new(mount_focused(editor))),
            Through::Editor => Surface::Editor(editor),
            Through::Handle => Surface::Handle,
        };
        Self { handle, surface }
    }

    /// Put the caret at `at` and move its item one level: deeper with Tab or
    /// `indent`, up with Shift+Tab or `outdent`.
    fn move_item(&mut self, at: usize, deeper: bool) {
        self.handle.select_range(at, at);
        match &mut self.surface {
            Surface::Keys(tree) => {
                let modifiers = if deeper {
                    Modifiers::NONE
                } else {
                    Modifiers::SHIFT
                };
                press(tree, Key::Tab, modifiers, 1);
            }
            Surface::Editor(editor) if deeper => editor.indent(),
            Surface::Editor(editor) => editor.outdent(),
            Surface::Handle if deeper => self.handle.indent(),
            Surface::Handle => self.handle.outdent(),
        }
    }
}

#[test]
fn a_list_item_in_a_quote_moves_one_level_each_way_and_undo_puts_it_back() {
    // In one and two quotes, from the start, the middle and the end of the
    // item's text, for a middle item and for the last one (whose end is where
    // the check "is the caret in a list" used to read the paragraph after the
    // list, and Tab typed a tab character there), through the keys and both
    // command surfaces. Each move takes the item one level, leaves every other
    // block, every quote and every character where they were, and is one undo
    // step, which redo takes again.
    for markdown in [
        "> - one\n> - two\n> - three\n\nAfter\n",
        "> > - one\n> > - two\n> > - three\n>\n> After\n",
    ] {
        for item in ["two", "three"] {
            for offset in [0, 1, item.chars().count()] {
                for through in [Through::Keys, Through::Editor, Through::Handle] {
                    let context = format!("{item:?}+{offset} in {markdown:?} through {through:?}");
                    let doc = quoted(markdown);
                    let text = doc.to_plain_text().unwrap();
                    let before = listing(&doc);
                    assert_eq!(level_of(&before, item), 1, "setup: {context}");
                    let deeper = with_level(&before, item, 2);
                    let at = paragraph_position(&doc, item) + offset;
                    let mut mover = Mover::new(&doc, through);

                    mover.move_item(at, true);
                    assert_eq!(listing(&doc), deeper, "one level in: {context}");

                    mover.handle.undo();
                    assert_eq!(listing(&doc), before, "undoing it: {context}");
                    mover.handle.redo();
                    assert_eq!(listing(&doc), deeper, "redoing it: {context}");

                    mover.move_item(at, false);
                    assert_eq!(listing(&doc), before, "one level out: {context}");

                    mover.handle.undo();
                    assert_eq!(listing(&doc), deeper, "undoing it: {context}");
                    assert_eq!(
                        doc.to_plain_text().unwrap(),
                        text,
                        "no move may cost the text: {context}"
                    );
                }
            }
        }
    }
}

#[test]
fn holding_tab_in_a_list_inside_a_quote_stops_at_the_list_ceiling() {
    // The list takes Tab before the quote does, so a held Tab moves the item
    // down to the list ceiling, keeps the key there, and never nests the quote
    // around the item or around the quoted paragraph after it. From the end of
    // the item's text, the list check used to read that paragraph and nest its
    // quote instead.
    for (markdown, quotes) in [
        ("> - item\n", 1),
        ("> - item\n>\n> After\n", 1),
        ("> > - item\n> >\n> > After\n", 2),
    ] {
        for offset in [0, "item".len()] {
            let context = format!("from +{offset} in {markdown:?}");
            let doc = quoted(markdown);
            let text = doc.to_plain_text().unwrap();
            let before = listing(&doc);
            let editor = RichTextEditor::editor(doc.clone());
            let handle = editor.handle();
            let state = editor.state_handle();
            let mut tree = mount_focused(editor);
            let at = paragraph_position(&doc, "item") + offset;
            handle.select_range(at, at);
            assert_eq!(
                nesting::deeper_list_indent(&state.borrow()),
                Some(1),
                "setup: a quoted item goes one level down as any other does: {context}"
            );

            press(&mut tree, Key::Tab, Modifiers::NONE, HELD);

            assert_eq!(
                listing(&doc),
                with_level(&before, "item", LIST_CEILING),
                "Tab must move the item to the list ceiling and change nothing else: {context}"
            );
            assert!(
                before.iter().all(|&(_, depth, ..)| depth == quotes),
                "setup: every block sits in {quotes} quotes: {context}"
            );
            assert_eq!(nesting::deeper_list_indent(&state.borrow()), None);
            assert_eq!(
                doc.to_plain_text().unwrap(),
                text,
                "Tab at the ceiling must leave the text alone: {context}"
            );

            press(&mut tree, Key::Tab, Modifiers::SHIFT, 1);
            assert_eq!(
                listing(&doc),
                with_level(&before, "item", LIST_CEILING - 1),
                "Shift+Tab must move it one level back up: {context}"
            );
        }
    }
}

#[test]
fn a_list_item_in_a_table_cell_moves_through_indent_and_outdent() {
    // In a table cell Tab and Shift+Tab move between cells, so `indent` and
    // `outdent` are how an item there changes level. They move it as they do
    // in the main text, keep it in its cell, stop at the list ceiling, and are
    // each one undo step; the keys still go to the next cell and leave the
    // item's level alone.
    for (markdown, quotes) in [
        ("Before\n\n| One. | x |\n|---|---|\n| y | z |\n\nAfter\n", 0),
        (
            "> Before\n>\n> | One. | x |\n> |---|---|\n> | y | z |\n>\n> After\n",
            1,
        ),
    ] {
        let doc = quoted(markdown);
        let editor = RichTextEditor::editor(doc.clone());
        let cell = paragraph_position(&doc, "One.");
        editor.set_caret_position(cell);
        editor.insert_list(false);
        let text = doc.to_plain_text().unwrap();
        let before = listing(&doc);
        assert!(
            before.contains(&("One.".into(), quotes, 1, Some((0, 0)))),
            "setup: a list item in the first cell of {markdown:?}: {before:?}"
        );

        for through in [Through::Editor, Through::Handle] {
            let context = format!("{markdown:?} through {through:?}");
            let mut mover = Mover::new(&doc, through);
            for end in [0, "One.".len()] {
                mover.move_item(cell + end, true);
                assert_eq!(
                    listing(&doc),
                    with_level(&before, "One.", 2),
                    "`indent` from +{end}: {context}"
                );
                mover.handle.undo();
                assert_eq!(listing(&doc), before, "undoing it: {context}");
            }

            for _ in 0..HELD {
                mover.move_item(cell, true);
            }
            assert_eq!(
                listing(&doc),
                with_level(&before, "One.", LIST_CEILING),
                "`indent` must stop at the list ceiling: {context}"
            );
            mover.move_item(cell, false);
            assert_eq!(
                listing(&doc),
                with_level(&before, "One.", LIST_CEILING - 1),
                "`outdent`: {context}"
            );
            mover.handle.undo();
            assert_eq!(
                listing(&doc),
                with_level(&before, "One.", LIST_CEILING),
                "undoing it: {context}"
            );
            for _ in 0..HELD {
                mover.move_item(cell, false);
            }
            assert_eq!(
                listing(&doc),
                before,
                "`outdent` must stop at the top level, in the list: {context}"
            );
            assert_eq!(
                doc.to_plain_text().unwrap(),
                text,
                "no move may cost the text: {context}"
            );
        }

        // The cell takes Tab before the list does, in a quote as in the main
        // text.
        let editor = RichTextEditor::editor(doc.clone());
        let handle = editor.handle();
        let state = editor.state_handle();
        let mut tree = mount_focused(editor);
        handle.select_range(cell, cell);
        press(&mut tree, Key::Tab, Modifiers::NONE, 1);
        assert_eq!(
            state
                .borrow()
                .cursor
                .current_table_cell()
                .map(|at| (at.row, at.column)),
            Some((0, 1)),
            "Tab in a cell must still go to the next cell: {markdown:?}"
        );
        assert_eq!(
            listing(&doc),
            before,
            "Tab in a cell must leave the item's level alone: {markdown:?}"
        );
    }
}

#[test]
fn a_list_item_in_a_quote_still_leaves_its_list_by_backspace_or_remove_from_list() {
    // Shift+Tab stops at the top level of a list, as it does in the main text,
    // so it cannot take an item out of its list. Backspace at the start of the
    // item does, from any level, after which Shift+Tab steps out of the quote.
    let doc = quoted("> - outer\n>\n>   - inner\n");
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let state = editor.state_handle();
    let inner = char_offset_of(&doc, "inner");
    let mut tree = mount_focused(editor);
    handle.select_range(inner, inner);
    assert_eq!((quote_depth(&state), list_level(&state)), (1, 2), "setup");

    press(&mut tree, Key::Backspace, Modifiers::NONE, 1);
    assert_eq!(
        (quote_depth(&state), list_level(&state)),
        (1, 0),
        "Backspace at the start of the item must take it out of its list"
    );
    press(&mut tree, Key::Tab, Modifiers::SHIFT, 1);
    assert_eq!(
        (quote_depth(&state), list_level(&state)),
        (0, 0),
        "Shift+Tab must then step out of the quote"
    );
    assert_eq!(
        doc.to_plain_text().unwrap(),
        "outer\ninner",
        "no way out may cost the item its text"
    );

    // `remove_from_list`, the toolbar's way out, takes the item out as well.
    let doc = quoted("> - outer\n>\n>   - inner\n");
    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();
    editor.set_caret_position(char_offset_of(&doc, "inner"));
    editor.remove_from_list();
    assert_eq!(
        (quote_depth(&state), list_level(&state)),
        (1, 0),
        "`remove_from_list` must take the item out of its list"
    );
}

#[test]
fn decrease_blockquote_depth_takes_a_list_item_out_of_its_quote_where_shift_tab_moves_it_up() {
    // On a paragraph in a quote, Shift+Tab and `decrease_blockquote_depth` do
    // the same. On a list item Shift+Tab is a list outdent: it moves the item
    // one level up, or does nothing at the top level, and leaves the quote
    // alone. The command takes the item out of one quote and keeps its list
    // level.
    for (markdown, needle, before, shift_tab, decreased) in [
        ("> Quoted.\n", "Quoted.", (1, 0), (0, 0), (0, 0)),
        ("> - item\n", "item", (1, 1), (1, 1), (0, 1)),
        (
            "> - outer\n>\n>   - inner\n",
            "inner",
            (1, 2),
            (1, 1),
            (0, 2),
        ),
        ("> > - deep\n", "deep", (2, 1), (2, 1), (1, 1)),
    ] {
        let doc = quoted(markdown);
        let text = doc.to_plain_text().unwrap();
        let editor = RichTextEditor::editor(doc.clone());
        let handle = editor.handle();
        let state = editor.state_handle();
        let at = char_offset_of(&doc, needle);
        let mut tree = mount_focused(editor);
        handle.select_range(at, at);
        assert_eq!(
            (quote_depth(&state), list_level(&state)),
            before,
            "setup: {markdown:?}"
        );
        press(&mut tree, Key::Tab, Modifiers::SHIFT, 1);
        assert_eq!(
            (quote_depth(&state), list_level(&state)),
            shift_tab,
            "Shift+Tab on {markdown:?}"
        );
        assert_eq!(
            doc.to_plain_text().unwrap(),
            text,
            "Shift+Tab on {markdown:?}"
        );

        for through_handle in [false, true] {
            let doc = quoted(markdown);
            let editor = RichTextEditor::editor(doc.clone());
            let handle = editor.handle();
            let state = editor.state_handle();
            editor.set_caret_position(char_offset_of(&doc, needle));
            if through_handle {
                handle.decrease_blockquote_depth();
            } else {
                editor.decrease_blockquote_depth();
            }
            assert_eq!(
                (quote_depth(&state), list_level(&state)),
                decreased,
                "`decrease_blockquote_depth` on {markdown:?} (through the handle: {through_handle})"
            );
            assert_eq!(
                doc.to_plain_text().unwrap(),
                text,
                "`decrease_blockquote_depth` on {markdown:?} may not cost the text"
            );
        }
    }
}

#[test]
fn tab_over_a_selection_holding_a_quote_at_the_ceiling_changes_nothing() {
    // Tab in a quote wraps the whole selection, and with it every quote the
    // selection holds: the one at the ceiling would end up one level past it.
    let doc = quoted("> Outer A\n>\n> > Deep\n>\n> Outer B\n");
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    editor.set_caret_position(char_offset_of(&doc, "Deep"));
    for _ in 2..QUOTE_CEILING {
        editor.increase_blockquote_depth();
    }
    let deep = char_offset_of(&doc, "Deep");
    assert_eq!(quote_depth_at(&doc, deep), QUOTE_CEILING, "setup");
    let text = doc.to_plain_text().unwrap();

    let mut tree = mount_focused(editor);
    let start = char_offset_of(&doc, "Outer A");
    let end = char_offset_of(&doc, "Outer B") + "Outer B".len();
    handle.select_range(start, end);

    press(&mut tree, Key::Tab, Modifiers::NONE, 1);

    assert_eq!(
        quote_depth_at(&doc, deep),
        QUOTE_CEILING,
        "the quote at the ceiling must not be wrapped past it"
    );
    assert_eq!(
        quote_depth_at(&doc, start),
        1,
        "a refused wrap must not wrap part of the selection either"
    );
    assert_eq!(
        doc.to_plain_text().unwrap(),
        text,
        "a refused wrap must not type a tab over the selection"
    );
}

#[test]
fn a_selection_from_an_empty_paragraph_is_judged_by_its_ends() {
    // A range query over the text leaves out an empty paragraph where the
    // range starts and a paragraph that begins exactly where it ends. A
    // selection from an empty paragraph to the start of the next one covers
    // nothing else, so the wrap has to read both ends itself.
    let doc = quoted("> Deep\n");
    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();
    editor.set_caret_position(0);
    for _ in 1..QUOTE_CEILING {
        editor.increase_blockquote_depth();
    }
    editor.insert_block();
    let deep = char_offset_of(&doc, "Deep");
    assert_eq!(
        (quote_depth_at(&doc, 0), quote_depth_at(&doc, deep)),
        (QUOTE_CEILING, QUOTE_CEILING),
        "setup: an empty paragraph and `Deep`, both at the ceiling"
    );
    editor.select_range(0, deep);

    assert!(!nesting::deeper_quote_is_available(&state.borrow()));
    editor.increase_blockquote_depth();

    assert_eq!(
        (quote_depth_at(&doc, 0), quote_depth_at(&doc, deep)),
        (QUOTE_CEILING, QUOTE_CEILING),
        "neither paragraph may be wrapped past the ceiling"
    );
}

#[test]
fn toggling_a_quote_around_one_at_the_ceiling_changes_nothing() {
    let (doc, editor) = deep_quote_between_paragraphs();
    editor.select_all();

    editor.toggle_blockquote();

    assert_eq!(
        quote_depth_at(&doc, char_offset_of(&doc, "Deep")),
        QUOTE_CEILING,
        "the toggle must not wrap a quote at the ceiling past it"
    );
    assert_eq!(
        quote_depth_at(&doc, 0),
        0,
        "a refused toggle must leave the paragraphs around it unquoted"
    );
}

#[test]
fn the_indent_and_depth_commands_stop_at_the_ceiling() {
    let doc = quoted("> Quoted line.\n");
    let editor = RichTextEditor::editor(doc.clone());
    editor.set_caret_position(0);
    for _ in 0..HELD {
        editor.increase_blockquote_depth();
    }
    assert_eq!(quote_depth_at(&doc, 0), QUOTE_CEILING);

    let doc = TextDocument::new();
    doc.set_plain_text("item").unwrap();
    let editor = RichTextEditor::editor(doc);
    editor.insert_list(false);
    editor.set_caret_position(0);
    let handle = editor.handle();
    for _ in 0..HELD {
        handle.indent();
    }
    assert_eq!(list_level(&editor.state_handle()), LIST_CEILING);
}

#[test]
fn increase_blockquote_depth_outside_a_quote_does_nothing() {
    // Documented as the command behind Tab in a quote, and as a no-op outside
    // one; it used to wrap a plain paragraph in a new quote.
    let doc = TextDocument::new();
    doc.set_plain_text("Plain.").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.set_caret_position(0);

    editor.increase_blockquote_depth();
    editor.handle().increase_blockquote_depth();

    assert_eq!(quote_depth_at(&doc, 0), 0);
}

#[test]
fn the_context_menu_greys_out_a_toggle_the_ceiling_refuses() {
    let (_doc, editor) = deep_quote_between_paragraphs();
    let handle = editor.handle();
    let mut tree = mount_focused(editor);
    handle.select_all();
    let _ = tree.render();

    // Inside the selection, so the right-click keeps it.
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(30.0, 10.0),
        PointerButton::Secondary,
        Modifiers::NONE,
    ));
    tree.layout(viewport());

    let update = tree.sync_accessibility();
    let (_, row) = update
        .nodes
        .iter()
        .find(|(_, n)| {
            n.role() == teksilo_core::accesskit::Role::MenuItem
                && n.label() == Some("Toggle blockquote")
        })
        .expect("the menu must offer the blockquote toggle");
    assert!(
        row.is_disabled(),
        "the toggle must be greyed out where the ceiling refuses it"
    );
}

#[test]
fn the_deepest_structure_the_gestures_build_stays_inside_a_96_container_loader() {
    // A list at the list ceiling inside quotes at the quote ceiling is the
    // deepest thing the gestures can build. A Djot parser recurses once per
    // blockquote and once per list level, so its deepest line costs a loader
    // 64 + 16 = 80, under a loader that refuses past 96.
    //
    // The list is a staircase, each item one level below the one above it. A
    // Djot parser reads an item nested further than that one level below its
    // neighbour, so from text-document 1.12.3 on the writer sets a lone deep
    // item down at the level a reload gives it, and only a staircase reaches
    // the parser 16 levels deep. The quotes come first and Tab builds the
    // staircase inside them, as it would in the main text.
    let items: Vec<String> = (1..=LIST_CEILING)
        .map(|level| format!("Level {level}"))
        .collect();
    let doc = TextDocument::new();
    doc.set_plain_text(&items.join("\n")).unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.select_all();
    editor.insert_list(false);
    let handle = editor.handle();
    handle.toggle_blockquote();
    for _ in 0..HELD {
        handle.increase_blockquote_depth();
    }
    let mut tree = mount_focused(editor);
    for (presses, item) in items.iter().enumerate() {
        let at = paragraph_position(&doc, item);
        handle.select_range(at, at);
        press(&mut tree, Key::Tab, Modifiers::NONE, presses);
    }
    let staircase: Vec<Listed> = items
        .iter()
        .enumerate()
        .map(|(presses, item)| (item.clone(), QUOTE_CEILING, presses + 1, None))
        .collect();
    assert_eq!(
        listing(&doc),
        staircase,
        "setup: a list stepping down to the list ceiling inside quotes at the quote ceiling"
    );

    let djot = doc.to_djot().unwrap();
    let deepest = djot
        .lines()
        .map(djot_line_nesting)
        .max()
        .expect("the export has a line");
    assert_eq!(
        deepest,
        QUOTE_CEILING + LIST_CEILING,
        "the deepest line must cost one container per quote and per list level:\n{djot}"
    );
    assert!(deepest < 96, "the deepest line nests {deepest} containers");
}

#[test]
fn a_quote_at_the_ceiling_reports_no_deeper_level() {
    // The availability the gestures and the context menu read: it must turn
    // off exactly at the ceiling and back on one level below it.
    let doc = quoted("> Quoted line.\n");
    let editor = RichTextEditor::editor(doc);
    let handle = editor.handle();
    let state = editor.state_handle();
    editor.set_caret_position(0);
    assert!(nesting::deeper_quote_is_available(&state.borrow()));

    for _ in 1..QUOTE_CEILING {
        handle.increase_blockquote_depth();
    }

    assert_eq!(quote_depth(&state), QUOTE_CEILING, "setup");
    assert!(!nesting::deeper_quote_is_available(&state.borrow()));
    assert!(
        nesting::toggle_is_available(&state.borrow()),
        "the toggle unwraps here, and unwrapping is never limited"
    );

    handle.decrease_blockquote_depth();
    assert!(
        nesting::deeper_quote_is_available(&state.borrow()),
        "one level below the ceiling a deeper quote is available again"
    );
}

#[test]
fn a_list_item_at_the_ceiling_reports_no_deeper_level() {
    let doc = TextDocument::new();
    doc.set_plain_text("item").unwrap();
    let editor = RichTextEditor::editor(doc);
    editor.insert_list(false);
    editor.set_caret_position(0);
    let handle = editor.handle();
    let state = editor.state_handle();
    assert_eq!(nesting::deeper_list_indent(&state.borrow()), Some(1));

    for _ in 1..LIST_CEILING {
        handle.indent();
    }

    assert_eq!(list_level(&state), LIST_CEILING, "setup");
    assert_eq!(nesting::deeper_list_indent(&state.borrow()), None);

    handle.outdent();
    assert!(
        nesting::deeper_list_indent(&state.borrow()).is_some(),
        "one level above the ceiling a deeper level is available again"
    );
}

#[test]
fn a_toggle_around_a_quote_at_the_ceiling_is_unavailable() {
    let (_doc, editor) = deep_quote_between_paragraphs();
    let state = editor.state_handle();
    editor.set_caret_position(0);
    assert!(
        nesting::toggle_is_available(&state.borrow()),
        "wrapping the plain paragraph on its own stays available"
    );

    editor.select_all();

    assert!(!nesting::toggle_is_available(&state.borrow()));
}

#[test]
fn nothing_deeper_is_available_where_a_gesture_has_nothing_to_act_on() {
    let doc = TextDocument::new();
    doc.set_plain_text("Plain.").unwrap();
    let editor = RichTextEditor::editor(doc);
    let state = editor.state_handle();
    editor.set_caret_position(0);
    assert_eq!(
        nesting::deeper_list_indent(&state.borrow()),
        None,
        "not in a list"
    );
    assert!(
        !nesting::deeper_quote_is_available(&state.borrow()),
        "not in a quote"
    );
    assert!(
        nesting::toggle_is_available(&state.borrow()),
        "a plain paragraph can be quoted"
    );
}

#[test]
fn the_deepest_quote_under_a_selection_matches_asking_every_paragraph() {
    // The probe walks only the frames between the selection's two ends, once
    // each. Asking every selected paragraph for its own depth gives the same
    // answer at a document-wide lookup per paragraph; this holds the two
    // together over every pair of paragraph edges in a document that nests
    // quotes, lists and tables inside one another, empty paragraphs included.
    let doc = quoted(
        "Intro\n\n\
         > Quote one\n>\n\
         > - item in a quote\n> - second item\n>\n\
         > > Quote two\n> >\n\
         > > > Quote three\n> >\n\
         > > Back in two\n>\n\
         > | a | b |\n> |---|---|\n> | 1 | 2 |\n>\n\
         > Back in one\n\n\
         | c | d |\n|---|---|\n| 3 | 4 |\n\n\
         Middle\n\n\
         > Last quote\n\n\
         Outro\n",
    );
    let paragraphs = || (0..doc.block_count()).filter_map(|n| doc.block_by_number(n));
    for after in ["Back in two", "Middle", "Outro"] {
        let block = paragraphs()
            .find(|block| block.text() == after)
            .expect("setup: the paragraph is in the document");
        let cursor = doc.cursor();
        cursor.set_position(block.position() + block.length(), MoveMode::MoveAnchor);
        cursor.insert_block().unwrap();
    }
    assert!(
        paragraphs().any(|block| block.length() == 0),
        "setup: the document must hold an empty paragraph"
    );
    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();

    let mut edges: Vec<usize> = paragraphs()
        .flat_map(|block| {
            let start = block.position();
            [start, start + block.length() / 2, start + block.length()]
        })
        .collect();
    edges.sort_unstable();
    edges.dedup();

    let (mut across_frames, mut deeper_than_either_end) = (0, 0);
    for &from in &edges {
        for &to in &edges {
            editor.select_range(from, to);
            // The editor may move an end off a table's boundary, and the wrap
            // acts on the selection as the editor holds it.
            let (anchor, position, crosses_frames) = {
                let st = state.borrow();
                let crosses_frames = st.cursor.selection_spans_multiple_frames();
                (st.cursor.anchor(), st.cursor.position(), crosses_frames)
            };
            let expected = (!crosses_frames)
                .then(|| deepest_quote_paragraph_by_paragraph(&doc, anchor, position));
            assert_eq!(
                nesting::deepest_quote_under_wrap(&state.borrow()),
                expected,
                "the selection from {anchor} to {position}"
            );
            let deeper_end = quote_depth_at(&doc, anchor).max(quote_depth_at(&doc, position));
            across_frames += usize::from(crosses_frames);
            deeper_than_either_end +=
                usize::from(expected.is_some_and(|deepest| deepest > deeper_end));
        }
    }
    assert!(
        across_frames > 1000 && deeper_than_either_end > 100,
        "setup: {across_frames} selections cross a frame boundary and \
         {deeper_than_either_end} hold a quote deeper than both their ends"
    );
}

#[test]
fn nothing_puts_a_quote_inside_a_table_cell() {
    // The probe does not look inside a table between a selection's two ends:
    // a cell is as deep as the frame its table sits in, because nothing the
    // editor does can put a quote in a cell. This tries each way a quote can
    // reach a paragraph, and an HTML import, with the table at the top level
    // and inside a quote. The text-document release that first lets a quote
    // into a cell fails here, and the probe must then walk into the cells as
    // well.
    const NESTED_QUOTE: &str =
        "<blockquote><p>q1</p><blockquote><p>q2</p></blockquote></blockquote>";
    let copied_quote = copy_of(&quoted("> q1\n>\n> > q2\n"));
    for (place, prefix) in [("at the top level", ""), ("in a quote", "> ")] {
        let markdown =
            format!("Before\n\n{prefix}| a | b |\n{prefix}|---|---|\n{prefix}| c | d |\n\nAfter\n");
        for gesture in [
            "the toggle",
            "Tab's command",
            "an HTML paste",
            "a pasted or dropped copy",
        ] {
            let context = format!("{gesture}, with the table {place}");
            let doc = quoted(&markdown);
            let editor = RichTextEditor::editor(doc.clone());
            let state = editor.state_handle();
            let cell = paragraph_position(&doc, "b");
            editor.set_caret_position(cell);
            match gesture {
                "the toggle" => {
                    assert!(
                        nesting::toggle_is_available(&state.borrow()),
                        "{context}: the toggle must reach the document"
                    );
                    editor.toggle_blockquote();
                }
                "Tab's command" => {
                    assert_eq!(
                        nesting::deeper_quote_is_available(&state.borrow()),
                        !prefix.is_empty(),
                        "{context}: the command reaches the document inside a quote"
                    );
                    editor.increase_blockquote_depth();
                }
                // The call a paste of HTML from another application makes.
                "an HTML paste" => editor.insert_html(NESTED_QUOTE),
                // The call a paste of the editor's own copy, and a drop, make.
                _ => {
                    let cursor = doc.cursor();
                    cursor.set_position(cell, MoveMode::MoveAnchor);
                    cursor.insert_fragment(&copied_quote).unwrap();
                }
            }
            if gesture.ends_with("paste") || gesture.ends_with("copy") {
                assert!(
                    cell_paragraphs(&doc)
                        .iter()
                        .any(|(text, _)| text.contains("q2")),
                    "{context}: setup, the quote's text must reach the cell"
                );
            }
            assert_cells_as_deep_as_their_table(&doc, &context);
        }
    }

    let doc = TextDocument::new();
    doc.set_html(&format!(
        "<p>Before</p><table><tr><td>{NESTED_QUOTE}</td><td><p>b</p></td></tr></table><p>After</p>"
    ))
    .unwrap()
    .wait()
    .unwrap();
    assert!(
        cell_paragraphs(&doc)
            .iter()
            .any(|(text, _)| text.contains("q2")),
        "setup: an HTML import must keep the text of a quote inside a cell: {:?}",
        cell_paragraphs(&doc)
    );
    assert_cells_as_deep_as_their_table(&doc, "an HTML import");
}

#[test]
fn a_cell_marked_as_a_quote_reaches_no_djot_parser_as_one() {
    // A host can mark a cell's own frame as a quote through `set_frame_format`.
    // The depth query counts the mark and the probe does not, which the
    // ceiling can afford: a Djot export writes a cell's text inline, so the
    // mark never reaches a parser as a quote.
    let doc = quoted("Before\n\n| a | b |\n|---|---|\n| c | d |\n\nAfter\n");
    let cell = paragraph_position(&doc, "b");
    let frame = doc
        .block_at_position(cell)
        .expect("setup: the cell holds a paragraph")
        .frame();
    let top_level = doc
        .block_at_position(0)
        .expect("setup: the document starts with a paragraph")
        .frame();
    assert_ne!(
        frame.id(),
        top_level.id(),
        "setup: a cell has a frame of its own"
    );
    let mut format = frame.format();
    format.is_blockquote = Some(true);
    doc.cursor().set_frame_format(frame.id(), &format).unwrap();
    assert_eq!(
        (quote_depth_at(&doc, cell), quote_depth_at(&doc, 0)),
        (1, 0),
        "setup: the depth query counts the mark, on that cell alone"
    );

    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();
    editor.select_all();
    assert_eq!(
        nesting::deepest_quote_under_wrap(&state.borrow()),
        Some(0),
        "the probe does not count a mark on a cell's frame"
    );

    // A real quote reaches the export as one, so a quote-free export is not
    // the export dropping every quote.
    editor.set_caret_position(paragraph_position(&doc, "After"));
    editor.toggle_blockquote();
    let djot = doc.to_djot().unwrap();
    let quote_lines: Vec<&str> = djot.lines().filter(|line| line.starts_with('>')).collect();
    assert_eq!(
        quote_lines,
        ["> After"],
        "only the real quote may reach the export as one:\n{djot}"
    );
}

/// The deepest quote under the selection from `anchor` to `position`, read
/// paragraph by paragraph: the depth of every block the selection overlaps,
/// and of the blocks at its two ends, which a range query leaves out when they
/// are empty or start exactly at the end.
fn deepest_quote_paragraph_by_paragraph(
    doc: &TextDocument,
    anchor: usize,
    position: usize,
) -> usize {
    let (start, end) = (anchor.min(position), anchor.max(position));
    // Without a selection the wrap takes the caret's own paragraph.
    if start == end {
        return quote_depth_at(doc, end);
    }
    // With one, it reads the start as a character index, which at the end of
    // a paragraph is the separator and belongs to the next paragraph, and the
    // end as a caret, which there is still in the paragraph it ends.
    let first = doc
        .block_at_position(start)
        .map_or(0, |block| quote_depth_at(doc, block.position()));
    let ends = first.max(quote_depth_at(doc, end));
    doc.blocks_in_range(start, end - start)
        .iter()
        .map(|block| quote_depth_at(doc, block.position()))
        .fold(ends, usize::max)
}

/// Where the paragraph reading `text` starts. A table takes positions of its
/// own, so past one this differs from the offset in the plain text.
fn paragraph_position(doc: &TextDocument, text: &str) -> usize {
    (0..doc.block_count())
        .filter_map(|n| doc.block_by_number(n))
        .find(|block| block.text() == text)
        .map(|block| block.position())
        .expect("the paragraph is in the document")
}

/// The text and the quote depth of every paragraph in a table cell.
fn cell_paragraphs(doc: &TextDocument) -> Vec<(String, usize)> {
    (0..doc.block_count())
        .filter_map(|n| doc.block_by_number(n))
        .filter(|block| block.table_cell().is_some())
        .map(|block| (block.text(), quote_depth_at(doc, block.position())))
        .collect()
}

/// How many quotes deep the frame holding the document's table sits, or
/// `None` without a table.
fn table_depth(doc: &TextDocument) -> Option<usize> {
    let mut pending: Vec<(FlowElement, usize)> =
        doc.flow().into_iter().map(|element| (element, 0)).collect();
    while let Some((element, depth)) = pending.pop() {
        match element {
            FlowElement::Table(_) => return Some(depth),
            FlowElement::Frame(frame) => {
                let depth = depth + usize::from(frame.format().is_blockquote == Some(true));
                pending.extend(frame.flow().into_iter().map(|element| (element, depth)));
            }
            FlowElement::Block(_) => {}
        }
    }
    None
}

/// Every paragraph in the document's table sits exactly as deep as the frame
/// holding the table, and the probe over the whole document agrees with
/// asking every paragraph.
fn assert_cells_as_deep_as_their_table(doc: &TextDocument, context: &str) {
    let table = table_depth(doc)
        .unwrap_or_else(|| panic!("{context}: the document must still hold its table"));
    let cells = cell_paragraphs(doc);
    assert!(
        cells.iter().all(|&(_, depth)| depth == table),
        "{context}: every cell must sit {table} quotes deep, as its table does: {cells:?}"
    );
    let editor = RichTextEditor::editor(doc.clone());
    let state = editor.state_handle();
    editor.select_all();
    let (anchor, position) = {
        let st = state.borrow();
        (st.cursor.anchor(), st.cursor.position())
    };
    assert_eq!(
        nesting::deepest_quote_under_wrap(&state.borrow()),
        Some(deepest_quote_paragraph_by_paragraph(doc, anchor, position)),
        "{context}: the probe must agree with asking every paragraph"
    );
}

/// What a copy or a drag of the whole of `doc` carries.
fn copy_of(doc: &TextDocument) -> DocumentFragment {
    let editor = RichTextEditor::editor(doc.clone());
    editor.select_all();
    editor.state_handle().borrow().cursor.selection()
}

/// Containers a Djot parser opens for `line`: one per blockquote marker, and
/// one per list level (the bullet itself, plus one per two columns of
/// indentation in front of it).
fn djot_line_nesting(line: &str) -> usize {
    let mut rest = line;
    let mut quotes = 0;
    while let Some(after) = rest.strip_prefix('>') {
        quotes += 1;
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    let indent = rest.len() - rest.trim_start_matches(' ').len();
    let body = &rest[indent..];
    let is_item = body.starts_with("- ") || body.starts_with("* ") || body.starts_with("+ ");
    quotes + if is_item { indent / 2 + 1 } else { 0 }
}
