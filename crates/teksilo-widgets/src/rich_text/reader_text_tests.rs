// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The text a screen reader gets from a rich-text editor, read the way the
//! platform adapters read it: through `accesskit_consumer`.
//!
//! The consumer concatenates the runs, ends a paragraph where a run ends in a
//! `\n`, and counts an offset in Unicode scalars. So each paragraph's last run
//! ends with the separator the document has after it, and for plain text a
//! reader's offset is the document's position: the end of one paragraph and
//! the start of the next are two offsets, not one (reader finding text-03).

use crate::common::text_nav::{WORD_MODIFIER, by_convention};
use accesskit_consumer::{NodeRef, Tree};
use teksilo_canvas::SizeProposal;
use teksilo_core::accessibility::widget_id_to_node_id;
use teksilo_core::accesskit::{Action, ActionData, Role, TextSelection, TreeUpdate};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{LINE_SEPARATOR, TextDocument};

use super::{EditorHandle, RichTextEditor};

struct Reader {
    doc: TextDocument,
    handle: EditorHandle,
    tree: WidgetTree,
    id: WidgetId,
}

impl Reader {
    fn plain(text: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        Self::over(doc, RichTextEditor::editor)
    }

    fn markdown(markdown: &str) -> Self {
        let doc = TextDocument::new();
        doc.set_markdown(markdown).unwrap().wait().unwrap();
        Self::over(doc, RichTextEditor::editor)
    }

    fn over(doc: TextDocument, make: fn(TextDocument) -> RichTextEditor) -> Self {
        let editor = make(doc.clone());
        Self::mounted(doc, editor)
    }

    fn mounted(doc: TextDocument, editor: RichTextEditor) -> Self {
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let id = tree.add(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        tree.dispatch_event(WidgetEvent::pointer_down(
            teksilo_canvas::Point::new(20.0, 20.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        Self {
            doc,
            handle,
            tree,
            id,
        }
    }

    fn update(&mut self) -> TreeUpdate {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(200));
        self.tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = self.tree.render();
        self.tree.sync_accessibility()
    }

    /// The editor's text as a reader reviews it.
    fn text(&mut self) -> String {
        with_editor(&self.update(), |editor| editor.document_range().text())
    }

    /// The reader's offset for the caret, after putting it at `position`.
    fn offset_of(&mut self, position: usize) -> usize {
        self.handle.select_range(position, position);
        with_editor(&self.update(), |editor| {
            editor
                .text_selection_focus()
                .expect("a focused editor reports its caret")
                .to_global_usv_index()
        })
    }

    /// Put the caret where a reader's `SetTextSelection` at `offset` puts it,
    /// with the position the adapter derives from that offset.
    fn select_from_reader(&mut self, offset: usize) -> usize {
        let at = with_editor(&self.update(), |editor| {
            editor
                .text_position_from_global_usv_index(offset)
                .expect("an offset inside the text")
                .to_raw()
        });
        let mut ops = teksilo_core::window::NoopWindowOps;
        assert!(self.tree.dispatch_access_action(
            widget_id_to_node_id(self.id),
            Action::SetTextSelection,
            Some(ActionData::SetTextSelection(TextSelection {
                anchor: at,
                focus: at,
            })),
            &mut ops,
        ));
        self.handle.cursor_position()
    }

    fn document_len(&self) -> usize {
        self.doc.to_plain_text().unwrap_or_default().chars().count()
    }
}

fn with_editor<R>(update: &TreeUpdate, read: impl FnOnce(&NodeRef<'_>) -> R) -> R {
    let tree = Tree::new(update.clone(), true);
    let state = tree.state();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if matches!(node.role(), Role::MultilineTextInput | Role::Document) {
            return read(&node);
        }
        stack.extend(node.children());
    }
    panic!("no editor node in the tree");
}

#[test]
fn paragraphs_reach_a_reader_with_a_break_between_them() {
    let mut r = Reader::plain("One\nTwo\n\nFour");
    assert_eq!(r.text(), "One\nTwo\n\nFour\n");
}

/// Every caret position, a line break and an empty paragraph included, is the
/// same offset for a reader, so Right across a paragraph break moves on and the
/// caret is never reported in the other paragraph.
#[test]
fn a_readers_offset_is_the_document_position() {
    let mut r = Reader::plain("café one\n\nde");
    r.handle.select_range(4, 4);
    assert!(r.handle.insert_line_break());
    let len = r.document_len();
    assert_eq!(r.text(), "café\n one\n\nde\n");
    for position in 0..=len {
        assert_eq!(r.offset_of(position), position, "caret at {position}");
    }
}

#[test]
fn the_character_at_a_paragraphs_end_is_its_break() {
    let mut r = Reader::plain("One\nTwo");
    let text = r.text();
    let end_of_one = r.offset_of(3);
    assert_eq!(text.chars().nth(end_of_one), Some('\n'));
    assert_eq!(r.offset_of(4), 4, "the next paragraph starts one further");
}

/// Paragraph navigation stops at the paragraph, where it ran from the top of
/// the document to the first `\n` a code block happened to hold.
#[test]
fn a_paragraph_is_one_paragraph_for_a_reader() {
    let mut r = Reader::markdown("# Title\n\nFirst paragraph.\n\nSecond.\n");
    let text = r.text();
    assert_eq!(text, "Title\nFirst paragraph.\nSecond.\n");
    r.handle.select_range(10, 10);
    let (start, end) = with_editor(&r.update(), |editor| {
        let caret = editor.text_selection_focus().unwrap();
        (
            caret.backward_to_paragraph_start().to_global_usv_index(),
            caret.forward_to_paragraph_end().to_global_usv_index(),
        )
    });
    assert_eq!((start, end), (6, 23), "{:?}", &text[start..end]);
}

#[test]
fn enter_adds_a_character_a_reader_hears() {
    let mut r = Reader::plain("One");
    let before = r.text().chars().count();
    r.handle.select_range(3, 3);
    r.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(r.text().chars().count(), before + 1);
}

/// `SetTextSelection` lands where the reader asked: at a paragraph start, and
/// past a character longer than a byte, where the byte offset used to be added
/// to a character position.
#[test]
fn a_readers_selection_lands_on_the_document_position() {
    let mut r = Reader::plain("café\nnext");
    for position in [0, 3, 4, 5, 7, 9] {
        assert_eq!(
            r.select_from_reader(position),
            position,
            "offset {position}"
        );
    }
}

/// An image is one character of the document, heard as its description in its
/// place. It used to be a run of its own after the paragraph, which also
/// counted it twice.
#[test]
fn an_image_is_heard_in_its_place_and_counted_once() {
    let doc = TextDocument::new();
    doc.set_plain_text("Para one\nLast").unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    editor.set_caret_position(4);
    editor.insert_image("pic", "a cat", 10, 10);
    let mut r = Reader::mounted(doc, editor);
    assert_eq!(r.text(), "Paraa cat one\nLast\n");
    // The caret right after the image maps back to the position after it.
    let after_image = r.offset_of(5);
    assert_eq!(r.select_from_reader(after_image), 5);
    let last = r.offset_of(10);
    assert_eq!(r.select_from_reader(last), 10);
}

/// A table reads cell by cell, each cell's paragraph ending in its break.
///
/// A table has two positions of its own before its first cell, which the caret
/// stops on (Right from the paragraph before it takes three presses to reach
/// the first cell) and which no text holds: a reader is told the caret is at
/// the first cell there rather than nowhere. Every other position round-trips
/// through a reader's offset.
#[test]
fn a_table_reads_cell_by_cell_and_every_caret_is_reported() {
    let mut r = Reader::markdown("Before\n\n| a | b |\n|---|---|\n| c | d |\n\nAfter\n");
    assert_eq!(r.text(), "Before\na\nb\nc\nd\nAfter\n");
    let first_cell = 9;
    let mut last_position = 0;
    while {
        r.handle.select_range(last_position + 1, last_position + 1);
        r.handle.cursor_position() == last_position + 1
    } {
        last_position += 1;
    }
    let mut previous = 0;
    for position in 0..=last_position {
        let offset = r.offset_of(position);
        assert!(offset >= previous, "offsets follow positions at {position}");
        previous = offset;
        if (7..first_cell).contains(&position) {
            assert_eq!(offset, r.offset_of(first_cell), "table position {position}");
        } else {
            assert_eq!(
                r.select_from_reader(offset),
                position,
                "caret at {position}"
            );
        }
    }
}

#[test]
fn a_read_only_viewer_separates_paragraphs_too() {
    let doc = TextDocument::new();
    doc.set_plain_text("One\nTwo").unwrap();
    let mut r = Reader::over(doc, RichTextEditor::read_only);
    assert_eq!(r.text(), "One\nTwo\n");
}

/// A list item's first line starts with its marker as it is painted, a bullet
/// or a number and a space, nested items included (reader finding text-04).
/// A markdown numbered list keeps the delimiter its source wrote, here a full
/// stop.
#[test]
fn a_list_item_is_heard_with_its_marker() {
    let mut r = Reader::markdown("Intro\n\n- one\n- two\n  1. nested\n\n1. first\n2. second\n");
    assert_eq!(
        r.text(),
        "Intro\n• one\n• two\n1. nested\n1. first\n2. second\n"
    );
    // The line a reader hears on Down onto the second item.
    let chars: Vec<char> = r.text().chars().collect();
    r.handle.select_range(10, 10);
    let (start, end) = with_editor(&r.update(), |editor| {
        let caret = editor.text_selection_focus().unwrap();
        (
            caret.backward_to_line_start().to_global_usv_index(),
            caret.forward_to_line_end().to_global_usv_index(),
        )
    });
    let line: String = chars[start..end].iter().collect();
    assert_eq!(line, "• two\n");
}

/// Undoing a change to a list reaches a reader at once, in both directions,
/// with no key pressed after it. The document was already right, but nothing
/// said so until the next typed letter, so the screen, NVDA and braille went
/// on describing the old structure. Needs text-document 1.16.1.
#[test]
fn undoing_a_list_change_is_heard_without_another_key() {
    let mut r = Reader::markdown("- one\n");
    assert_eq!(r.text(), "• one\n");

    // Backspace at the item's start takes it out of its list.
    r.handle.select_range(0, 0);
    r.update();
    r.press(Key::Backspace);
    assert_eq!(r.text(), "one\n");

    r.handle.undo();
    assert_eq!(r.text(), "• one\n", "undo gives the marker back");
    r.handle.redo();
    assert_eq!(r.text(), "one\n", "redo takes it away again");
}

/// Taking an item out of its list, and its undo and redo, tell a reader the
/// caret moved, as UIA's `TextSelectionChanged`. The caret stays at the
/// item's start in the document, but the reader's offset moves by the
/// marker's two characters, and NVDA rebuilds its braille line only on a
/// caret event: without one the display kept the old line until the next
/// key. The event is raised only when the selection the adapter holds
/// changes (`accesskit_windows` `node.rs`, `enqueue_property_implied_events`).
#[test]
fn a_list_change_at_the_caret_raises_a_caret_event() {
    use crate::common::heard_test::{Uia, UiaRecorder};

    let mut r = Reader::markdown("Intro\n\n- one two\n");
    let item = "Intro\n".chars().count();
    r.handle.select_range(item, item);
    r.update();
    let editor = with_editor(&r.update(), |editor| editor.role());
    let mut adapter = UiaRecorder::attach(&mut r.tree);
    let selection_changed = Uia::TextSelectionChanged(editor);
    let mut step = |r: &mut Reader, name: &str, act: &dyn Fn(&mut Reader)| {
        act(r);
        r.update();
        let events = adapter.frame(&mut r.tree);
        (
            events.contains(&selection_changed),
            format!("{name}: {events:?}"),
        )
    };

    // Each leg leaves the caret at the item's start in the document: what
    // moves is only the reader's offset, by the marker.
    let (raised, events) = step(&mut r, "Backspace", &|r| r.press(Key::Backspace));
    assert_eq!(r.text(), "Intro\none two\n");
    assert_eq!(r.handle.cursor_position(), item, "Backspace");
    assert!(raised, "{events}");
    let (raised, events) = step(&mut r, "undo", &|r| r.handle.undo());
    assert_eq!(r.text(), "Intro\n• one two\n");
    assert_eq!(r.handle.cursor_position(), item, "undo");
    assert!(raised, "{events}");
    let (raised, events) = step(&mut r, "redo", &|r| r.handle.redo());
    assert_eq!(r.text(), "Intro\none two\n");
    assert_eq!(r.handle.cursor_position(), item, "redo");
    assert!(raised, "{events}");

    // The same with the caret inside the item's text: the undo gives the
    // item its marker back and leaves the caret where it was. (A redo from
    // there puts the caret back at the item's start, which moves it anyway.)
    let inside = item + "one ".chars().count();
    let _ = step(&mut r, "caret inside", &|r| {
        r.handle.select_range(inside, inside)
    });
    let (raised, events) = step(&mut r, "undo, caret inside", &|r| r.handle.undo());
    assert_eq!(r.text(), "Intro\n• one two\n");
    assert_eq!(r.handle.cursor_position(), inside);
    assert!(raised, "{events}");

    // Nothing in the frame after a change raises it again.
    let (raised, events) = step(&mut r, "next frame", &|_| {});
    assert!(!raised, "{events}");
}

/// An item put in above the caret's renumbers the caret's item, its "2."
/// becoming "3.", and leaves the caret at the same place in the item's text.
/// A reader is told the caret moved, as for a marker that comes or goes, so
/// NVDA rebuilds a braille line that still showed "2.".
#[test]
fn a_renumbered_item_at_the_caret_raises_a_caret_event() {
    use crate::common::heard_test::{Uia, UiaRecorder};

    let mut r = Reader::markdown("1. one\n2. two\n");
    let two = "one\n".chars().count();
    let inside = two + "t".chars().count();
    r.handle.select_range(inside, inside);
    r.update();
    let before = r.text();
    assert!(before.contains("2. two"), "{before:?}");
    let editor = with_editor(&r.update(), |editor| editor.role());
    let mut adapter = UiaRecorder::attach(&mut r.tree);
    let selection_changed = Uia::TextSelectionChanged(editor);

    // A new item after the first, written by the application.
    let above = r.doc.cursor_at(two - 1);
    above.insert_block().unwrap();
    above.insert_text("one and a half").unwrap();
    let after = r.text();
    assert!(after.contains("3. two"), "{after:?}");
    let moved_by = "one and a half\n".chars().count();
    assert_eq!(
        r.handle.cursor_position(),
        inside + moved_by,
        "the caret stays in its item's text"
    );
    let events = adapter.frame(&mut r.tree);
    assert!(events.contains(&selection_changed), "{events:?}");
}

/// Ticking the box of the caret's task item changes its marker and nothing
/// else: a reader is told the caret moved, so the braille line shows the
/// new box. The editor has no gesture of its own for the box, so the
/// application ticks it through the document, as a host's own control would.
#[test]
fn a_task_ticked_at_the_caret_raises_a_caret_event() {
    use crate::common::heard_test::{Uia, UiaRecorder};
    use teksilo_text::text_document::{BlockFormat, MarkerType};

    let mut r = Reader::markdown("- [ ] buy milk\n");
    let inside = "buy ".chars().count();
    r.handle.select_range(inside, inside);
    r.update();
    let before = r.text();
    assert!(before.starts_with('\u{2610}'), "{before:?}");
    let editor = with_editor(&r.update(), |editor| editor.role());
    let mut adapter = UiaRecorder::attach(&mut r.tree);
    let selection_changed = Uia::TextSelectionChanged(editor);

    r.doc
        .cursor_at(inside)
        .set_block_format(&BlockFormat {
            marker: Some(MarkerType::Checked),
            ..Default::default()
        })
        .unwrap();
    let after = r.text();
    assert!(after.starts_with('\u{2611}'), "{after:?}");
    assert_eq!(r.handle.cursor_position(), inside, "the caret stays put");
    let events = adapter.frame(&mut r.tree);
    assert!(events.contains(&selection_changed), "{events:?}");
}

/// A change after the caret leaves the selection the adapter holds alone, so
/// a reader is not told the caret moved when it did not, and a letter typed
/// and undone each move it, as before.
#[test]
fn an_edit_that_does_not_move_the_caret_raises_no_caret_event() {
    use crate::common::heard_test::{Uia, UiaRecorder};

    let mut r = Reader::markdown("- one\n- two\n");
    r.handle.select_range(1, 1);
    r.update();
    let editor = with_editor(&r.update(), |editor| editor.role());
    let mut adapter = UiaRecorder::attach(&mut r.tree);
    let selection_changed = Uia::TextSelectionChanged(editor);

    // Text added to the item after the caret's.
    r.doc.cursor_at(r.document_len()).insert_text("!").unwrap();
    r.update();
    assert_eq!(r.text(), "• one\n• two!\n");
    assert_eq!(r.handle.cursor_position(), 1);
    let events = adapter.frame(&mut r.tree);
    assert!(
        events.iter().any(|e| matches!(e, Uia::TextChanged(_))),
        "{events:?}"
    );
    assert!(!events.contains(&selection_changed), "{events:?}");

    r.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::X,
        modifiers: Modifiers::NONE,
        text: Some("x".into()),
    });
    r.update();
    let events = adapter.frame(&mut r.tree);
    assert!(events.contains(&selection_changed), "typing: {events:?}");
    r.handle.undo();
    r.update();
    let events = adapter.frame(&mut r.tree);
    assert!(events.contains(&selection_changed), "undo: {events:?}");
}

/// The marker is one character no document position is. The caret at an
/// item's start is reported after it, on the item's text; a reader's caret
/// put on it lands at the item's start; every position still round-trips.
#[test]
fn a_caret_is_never_inside_a_marker() {
    let mut r = Reader::markdown("Intro\n\n- one\n- two\n");
    let text = r.text();
    assert_eq!(text, "Intro\n• one\n• two\n");
    let chars: Vec<char> = text.chars().collect();
    for position in 0..=r.document_len() {
        let offset = r.offset_of(position);
        assert_ne!(chars.get(offset), Some(&'•'), "caret at {position}");
        assert_ne!(chars.get(offset), Some(&' '), "caret at {position}");
        assert_eq!(
            r.select_from_reader(offset),
            position,
            "caret at {position}"
        );
    }
    let one = "Intro\n".chars().count();
    assert_eq!(chars[r.offset_of(one)], 'o');
    let marker = "Intro\n".chars().count();
    assert_eq!(chars[marker], '•');
    assert_eq!(
        r.select_from_reader(marker),
        one,
        "a caret put on the marker"
    );
}

/// A selection running up to an item's start ends before the item's marker,
/// so a reader hears what was selected and not the next item's bullet; one
/// starting there starts after it.
#[test]
fn a_selection_up_to_an_item_holds_no_marker_of_it() {
    let mut r = Reader::markdown("Intro\n\n- one\n- two\n");
    let mut selected = |anchor: usize, focus: usize| {
        r.handle.select_range(anchor, focus);
        with_editor(&r.update(), |editor| {
            editor
                .text_selection()
                .map(|s| s.text())
                .unwrap_or_default()
        })
    };
    assert_eq!(selected(0, 6), "Intro\n");
    assert_eq!(selected(6, 0), "Intro\n");
    assert_eq!(selected(6, 10), "one\n", "Shift+Down from the item's start");
    assert_eq!(selected(10, 6), "one\n");
    assert_eq!(selected(0, 13), "Intro\n• one\n• two");
}

/// Down through a list item that wraps reads each of its lines, as Down
/// through a wrapped paragraph does. The caret Down puts at the start of
/// the item's second line was reported at the end of its first, so a reader
/// heard the first line again and the next Down left the item (NVDA round 4,
/// T30). The line asked for is the one a UIA `ExpandToEnclosingUnit(Line)`
/// gives on the caret.
/// The line a UIA client gets for the caret, by the steps NVDA takes: the start
/// of the range `GetSelection` returns (which is the selection's anchor when the
/// two ends are the same place), expanded to a line as `accesskit_windows` does.
fn uia_line_at_caret(editor: &NodeRef<'_>) -> (usize, usize) {
    let caret = editor.text_selection().unwrap().start();
    let start = if caret.is_line_start() {
        caret
    } else {
        caret.backward_to_line_start()
    };
    (
        start.to_global_usv_index(),
        start.forward_to_line_end().to_global_usv_index(),
    )
}

#[test]
fn home_on_a_wrapped_line_of_an_editable_text_reads_that_line() {
    let doc = TextDocument::new();
    doc.set_plain_text(
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho",
    )
    .unwrap();
    let mut r = Reader::over(doc, RichTextEditor::editor);
    let chars: Vec<char> = r.text().chars().collect();
    r.handle.select_range(chars.len(), chars.len());
    r.tree.press_key(Key::Home, Modifiers::NONE);
    let (start, end) = with_editor(&r.update(), uia_line_at_caret);
    let line: String = chars[start..end].iter().collect();
    assert!(
        line.trim_end().ends_with("pi rho") && !line.starts_with("alpha"),
        "the last line, alone: {line:?}"
    );
}

#[test]
fn down_through_a_wrapped_list_item_reads_each_of_its_lines() {
    let doc = TextDocument::new();
    doc.set_markdown(
        "Intro\n\n- alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu \
         xi omicron pi rho\n- two\n",
    )
    .unwrap()
    .wait()
    .unwrap();
    let mut r = Reader::over(doc, RichTextEditor::read_only);
    let chars: Vec<char> = r.text().chars().collect();
    r.handle.select_range(0, 0);
    let mut heard = Vec::new();
    for _ in 0..4 {
        let (start, end) = with_editor(&r.update(), uia_line_at_caret);
        heard.push(chars[start..end].iter().collect::<String>());
        r.tree.press_key(Key::ArrowDown, Modifiers::NONE);
    }
    assert_eq!(heard[0], "Intro\n");
    assert!(heard[1].starts_with("\u{2022} alpha"), "{heard:?}");
    assert!(
        heard[2].ends_with("pi rho\n") && !heard[2].contains("alpha"),
        "the item's second line, alone: {heard:?}"
    );
    assert_eq!(heard[3], "\u{2022} two\n");
}

/// A paragraph that wraps in the reader's width, and the caret at the start of
/// its second line, at the offset that line starts at.
fn at_the_second_line() -> (Reader, usize) {
    let mut r = Reader::plain(
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho \
         sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta eta theta iota",
    );
    r.handle.select_range(0, 0);
    r.update();
    r.tree.press_key(Key::ArrowDown, Modifiers::NONE);
    r.update();
    r.tree.press_key(Key::Home, Modifiers::NONE);
    r.update();
    let wrap = r.handle.cursor_position();
    assert!(wrap > 0, "the paragraph wraps");
    assert_eq!(r.line_start_at_caret(), wrap);
    (r, wrap)
}

impl Reader {
    fn press(&mut self, key: Key) {
        self.tree.press_key(key, Modifiers::NONE);
        self.update();
    }

    /// Where the line a UIA client gets for the caret starts.
    fn line_start_at_caret(&mut self) -> usize {
        with_editor(&self.update(), uia_line_at_caret).0
    }

    /// Put the caret where a reader's `SetTextSelection` names, by a position
    /// read from the tree, as `Select` on a UIA range does.
    fn select_position(
        &mut self,
        pick: impl FnOnce(&NodeRef<'_>) -> teksilo_core::accesskit::TextPosition,
    ) {
        let at = with_editor(&self.update(), pick);
        let mut ops = teksilo_core::window::NoopWindowOps;
        assert!(self.tree.dispatch_access_action(
            widget_id_to_node_id(self.id),
            Action::SetTextSelection,
            Some(ActionData::SetTextSelection(TextSelection {
                anchor: at,
                focus: at,
            })),
            &mut ops,
        ));
    }
}

/// Left, walking back through a wrapped line, stops at its first character and
/// then goes on to the line above. It used to go from the second character
/// straight to the end of the line above, so the first was never reached.
#[test]
fn left_stops_at_the_start_of_a_wrapped_line() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap + 1, wrap + 1);
    r.press(Key::ArrowLeft);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap, "on the line it was walking");
    r.press(Key::ArrowLeft);
    assert_eq!(r.handle.cursor_position(), wrap - 1);
    assert_eq!(r.line_start_at_caret(), 0, "on the line above");
}

#[test]
fn right_stops_at_the_start_of_a_wrapped_line() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap - 1, wrap - 1);
    r.press(Key::ArrowRight);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
    r.press(Key::ArrowRight);
    assert_eq!(r.handle.cursor_position(), wrap + 1);
}

/// End leaves the caret at the end of the line, at the offset the next line
/// starts at. Right from there reaches that start, not its second character.
#[test]
fn right_from_the_end_of_a_wrapped_line_reaches_the_start_of_the_next() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(3, 3);
    r.press(Key::End);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), 0, "End stays on its line");
    r.press(Key::ArrowRight);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
    r.press(Key::ArrowRight);
    assert_eq!(r.handle.cursor_position(), wrap + 1);
}

#[test]
fn a_word_step_onto_a_wrap_is_on_the_line_of_that_word() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap + 2, wrap + 2);
    r.tree.press_key(Key::ArrowLeft, WORD_MODIFIER);
    r.update();
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
}

/// Ctrl+Right stops at the start of the next word, as Windows and Linux editors
/// do, and the next word may start a wrapped line: the caret is then at the
/// start of that line, where a reader reads it. It used to stop at the end of
/// the word, before the space, and never reached a wrap. On macOS `⌥→` stops
/// at the word's end, before the space that ends the first line.
#[test]
fn a_word_step_forward_lands_on_the_next_words_start_on_its_line() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap - 3, wrap - 3);
    r.tree.press_key(Key::ArrowRight, WORD_MODIFIER);
    r.update();
    assert_eq!(r.handle.cursor_position(), by_convention(wrap, wrap - 1));
    assert_eq!(
        r.line_start_at_caret(),
        by_convention(wrap, 0),
        "the line of the caret"
    );

    r.handle.select_range(0, 0);
    r.tree.press_key(Key::ArrowRight, WORD_MODIFIER);
    r.update();
    assert_eq!(
        r.handle.cursor_position(),
        by_convention(6, 5),
        "alpha beta: the start of beta, or the end of alpha"
    );
}

/// Ctrl+Shift+Right selects up to the next word's start, and Ctrl+Delete
/// removes the rest of the word and the space after it. On macOS `⌥⇧→` and
/// `⌥⌦` stop at the word's end.
#[test]
fn a_word_selection_and_a_word_delete_reach_the_next_words_start() {
    let mut r = Reader::plain("alpha beta gamma");
    r.handle.select_range(2, 2);
    r.tree
        .press_key(Key::ArrowRight, WORD_MODIFIER | Modifiers::SHIFT);
    r.update();
    assert_eq!(r.handle.selection(), (2, by_convention(6, 5)));

    r.handle.select_range(2, 2);
    r.tree.press_key(Key::Delete, WORD_MODIFIER);
    r.update();
    assert_eq!(
        r.doc.to_plain_text().unwrap(),
        by_convention("albeta gamma", "al beta gamma")
    );
    assert_eq!(r.handle.cursor_position(), 2);
}

/// A word step counts characters, not bytes, through accented words: the
/// landings are character offsets in the document.
#[test]
fn a_word_step_forward_through_accented_words_counts_characters() {
    let mut r = Reader::plain("déjà été là");
    r.handle.select_range(1, 1);
    r.tree.press_key(Key::ArrowRight, WORD_MODIFIER);
    r.update();
    assert_eq!(r.handle.cursor_position(), by_convention(5, 4));
    r.tree.press_key(Key::ArrowRight, WORD_MODIFIER);
    r.update();
    assert_eq!(r.handle.cursor_position(), by_convention(9, 8));
}

/// Backspace on a line's second character leaves the caret on that line. It
/// used to take the writer to the end of the line above.
#[test]
fn backspace_stays_on_a_wrapped_line() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap + 1, wrap + 1);
    r.press(Key::Backspace);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
}

/// Undoing a Backspace at the start of a wrapped line (which takes the space
/// before the wrap, so the text wraps elsewhere until the undo) puts the caret
/// back at the start of that line. It was shown at the end of the line above,
/// and a reader asked for the caret's line read that one.
#[test]
fn undoing_a_change_at_a_wrap_puts_the_caret_on_the_line_below() {
    let (mut r, wrap) = at_the_second_line();
    r.press(Key::Backspace);
    assert_eq!(r.handle.cursor_position(), wrap - 1);
    r.tree.press_key(Key::Z, Modifiers::COMMAND);
    r.update();
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap, "the line after the wrap");

    // Redone and undone again from the handle, as a host's Edit menu does.
    r.handle.redo();
    r.update();
    r.handle.undo();
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap, "from the handle too");
}

/// The same after a letter typed at the start of a wrapped line.
#[test]
fn undoing_a_letter_typed_at_a_wrap_puts_the_caret_on_the_line_below() {
    let (mut r, wrap) = at_the_second_line();
    r.tree.type_text(r.id, "x");
    r.update();
    let typed = r.doc.to_plain_text().unwrap();
    assert_eq!(typed.chars().nth(wrap), Some('x'));
    r.tree.press_key(Key::Z, Modifiers::COMMAND);
    r.update();
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
}

/// An undo or a redo with nothing to take back leaves the caret where it is
/// shown: End leaves it at the end of the first line, at the offset the next
/// line starts at, and an undo that did nothing put it at the start of the
/// next line. From the key and from the handle.
#[test]
fn an_undo_with_nothing_to_undo_leaves_the_caret_on_its_line() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(3, 3);
    r.press(Key::End);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), 0, "End stays on its line");
    assert!(!r.doc.can_undo() && !r.doc.can_redo(), "no history");

    r.tree.press_key(Key::Z, Modifiers::COMMAND);
    r.update();
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), 0, "Ctrl+Z");
    r.tree
        .press_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    r.update();
    assert_eq!(r.line_start_at_caret(), 0, "Ctrl+Shift+Z");

    r.handle.undo();
    assert_eq!(r.line_start_at_caret(), 0, "the handle's undo");
    r.handle.redo();
    assert_eq!(r.line_start_at_caret(), 0, "the handle's redo");
    assert_eq!(r.handle.cursor_position(), wrap);
}

/// A composition's text is not on the undo stack, but every undo takes it out
/// first. From the handle, with nothing else to undo, that still moves the
/// caret, so its side of a wrap is set again: back at the start of the second
/// line, it is shown there.
#[test]
fn an_undo_that_takes_out_a_composition_puts_the_caret_on_its_line() {
    let (mut r, wrap) = at_the_second_line();
    r.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "xyz".to_string(),
        cursor: None,
        replacement: None,
    });
    r.update();
    assert!(r.doc.has_transient_edits() && !r.doc.can_undo());
    assert_eq!(r.handle.cursor_position(), wrap + 3);
    r.handle.undo();
    assert!(!r.doc.has_transient_edits(), "the composition is taken out");
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
}

#[test]
fn deleting_a_selection_from_the_start_of_a_wrapped_line_stays_on_it() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap, wrap + 2);
    r.press(Key::Delete);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
}

/// Delete does not move the caret, on either side of a wrap.
#[test]
fn delete_leaves_the_caret_on_the_line_it_is_on() {
    let (mut r, wrap) = at_the_second_line();
    r.press(Key::Delete);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap, "from the start of the line");

    r.handle.select_range(3, 3);
    r.press(Key::End);
    let end = r.handle.cursor_position();
    r.press(Key::Delete);
    assert_eq!(r.handle.cursor_position(), end);
    assert_eq!(r.line_start_at_caret(), 0, "from the end of the line above");
}

#[test]
fn left_and_right_with_a_selection_go_to_its_ends() {
    let (mut r, wrap) = at_the_second_line();
    for (anchor, caret) in [(6, 16), (16, 6), (wrap - 3, wrap + 3), (wrap + 3, wrap - 3)] {
        let (start, end) = (anchor.min(caret), anchor.max(caret));
        r.handle.select_range(anchor, caret);
        r.press(Key::ArrowLeft);
        assert_eq!(
            r.handle.selection(),
            (start, start),
            "Left from {anchor}..{caret}"
        );
        r.handle.select_range(anchor, caret);
        r.press(Key::ArrowRight);
        assert_eq!(
            r.handle.selection(),
            (end, end),
            "Right from {anchor}..{caret}"
        );
    }
}

/// Only the character step collapses to an end: Shift extends, and a word step
/// goes on from the caret, as they did.
#[test]
fn shift_and_word_steps_with_a_selection_are_not_collapsed_to_its_ends() {
    let mut r = Reader::plain("alpha beta gamma delta");
    r.handle.select_range(6, 16);
    r.tree.press_key(Key::ArrowLeft, Modifiers::SHIFT);
    r.update();
    assert_eq!(r.handle.selection(), (6, 15));
    r.handle.select_range(6, 16);
    r.tree.press_key(Key::ArrowLeft, WORD_MODIFIER);
    r.update();
    assert_eq!(r.handle.selection(), (11, 11));
}

/// A selection starting at a wrap holds the next line's first character, so its
/// start is on that line: UIA reads the selection's line from its start, and
/// NVDA+L read the line above. A selection ending there ends on the line above.
#[test]
fn a_selection_is_on_the_lines_of_the_text_it_holds() {
    let (mut r, wrap) = at_the_second_line();
    r.handle.select_range(wrap, wrap + 4);
    assert_eq!(r.line_start_at_caret(), wrap, "forward from the wrap");
    r.handle.select_range(wrap + 4, wrap);
    assert_eq!(r.line_start_at_caret(), wrap, "backward to the wrap");

    r.handle.select_range(wrap - 4, wrap);
    assert_eq!(r.line_start_at_caret(), 0, "up to the wrap");
    let upper = with_editor(&r.update(), |editor| {
        editor.text_selection().unwrap().end().to_global_usv_index()
    });
    assert_eq!(upper, wrap, "the selection still ends at the wrap");
}

/// Braille routing to the cell after a line's last character is a reader's
/// selection at the end of that line's last run. The caret stays on that line,
/// whichever side the caret was on before.
#[test]
fn a_reader_placing_the_caret_at_a_lines_end_stays_on_that_line() {
    let (mut r, wrap) = at_the_second_line();
    let line_end = with_editor(&r.update(), |editor| {
        editor
            .document_range()
            .start()
            .forward_to_line_end()
            .to_raw()
    });
    r.select_position(|_| line_end);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), 0, "the line it was routed on");
}

/// And one put at the start of a line's first run is on that line, after End
/// left the caret on the line above.
#[test]
fn a_reader_placing_the_caret_at_a_lines_start_stays_on_that_line() {
    let (mut r, wrap) = at_the_second_line();
    let line_start = with_editor(&r.update(), |editor| {
        editor.text_selection().unwrap().start().to_raw()
    });
    r.handle.select_range(3, 3);
    r.press(Key::End);
    assert_eq!(r.line_start_at_caret(), 0);
    r.select_position(|_| line_start);
    assert_eq!(r.handle.cursor_position(), wrap);
    assert_eq!(r.line_start_at_caret(), wrap);
}

/// The position after a line break is on the next line. There is no wrap to
/// choose a side of, and it used to read as the line above.
#[test]
fn a_caret_after_a_line_break_is_on_the_next_line() {
    let text: String = format!("one{LINE_SEPARATOR}two");
    let mut r = Reader::plain(&text);
    r.handle.select_range(4, 4);
    assert_eq!(r.line_start_at_caret(), 4, "after the break");
    r.handle.select_range(3, 3);
    assert_eq!(r.line_start_at_caret(), 0, "before the break");
    r.handle.select_range(4, 6);
    assert_eq!(r.line_start_at_caret(), 4, "a selection starting after it");
}

/// A line break inside a paragraph (Shift+Enter, a U+2028 in the document)
/// reaches a reader as the line feed a paragraph's end is, one character: NVDA
/// has no name for U+2028, so the caret just before one said nothing, and a
/// line holding only one was silent where an empty paragraph's is "blank".
#[test]
fn a_line_break_reaches_a_reader_as_a_line_feed() {
    let mut breaks = Reader::plain(&format!("a{LINE_SEPARATOR}{LINE_SEPARATOR}b"));
    assert_eq!(breaks.text(), "a\n\nb\n");
    let mut paragraphs = Reader::plain("a\n\nb");
    assert_eq!(paragraphs.text(), breaks.text(), "as two Enter read");

    // The caret just before the first break is on the break.
    let text: Vec<char> = breaks.text().chars().collect();
    assert_eq!(text[breaks.offset_of(1)], '\n');
}

/// The line a UIA client gets for the caret at `position`: where it starts,
/// its text and its boxes.
fn line_with_boxes_at(
    r: &mut Reader,
    position: usize,
) -> (usize, String, Vec<teksilo_core::accesskit::Rect>) {
    r.handle.select_range(position, position);
    with_editor(&r.update(), |editor| {
        let (start, _) = uia_line_at_caret(editor);
        let start = editor.text_position_from_global_usv_index(start).unwrap();
        let mut line = start.to_degenerate_range();
        line.set_end(start.forward_to_line_end());
        (
            start.to_global_usv_index(),
            line.text(),
            line.bounding_boxes(),
        )
    })
}

/// The empty line between two line breaks reads as an empty paragraph's line
/// does: one break character, with a box on that line, below the line above
/// and above the line below.
#[test]
fn an_empty_line_between_two_line_breaks_reads_as_an_empty_paragraph() {
    let mut breaks = Reader::plain(&format!("a{LINE_SEPARATOR}{LINE_SEPARATOR}b"));
    let (start, text, boxes) = line_with_boxes_at(&mut breaks, 2);
    assert_eq!((start, text.as_str()), (2, "\n"), "the caret's line");
    assert_eq!(boxes.len(), 1, "the line has a box");
    let (_, _, above) = line_with_boxes_at(&mut breaks, 0);
    let (_, _, below) = line_with_boxes_at(&mut breaks, 3);
    assert!(boxes[0].y1 > boxes[0].y0, "a box as tall as a line");
    assert!(above[0].y1 <= boxes[0].y0 && boxes[0].y1 <= below[0].y0);

    let mut paragraphs = Reader::plain("a\n\nb");
    assert_eq!(
        line_with_boxes_at(&mut paragraphs, 2),
        (start, text, boxes),
        "as an empty paragraph's line"
    );
}

/// Every position of a text with line breaks, the empty line between two of
/// them included, is the same offset for a reader, both ways, and the caret on
/// the empty line is on that line.
#[test]
fn every_position_around_line_breaks_round_trips() {
    let mut r = Reader::plain(&format!("one{LINE_SEPARATOR}{LINE_SEPARATOR}two"));
    for position in 0..=r.document_len() {
        let offset = r.offset_of(position);
        assert_eq!(offset, position, "caret at {position}");
        assert_eq!(r.select_from_reader(offset), position, "offset {offset}");
    }
    r.handle.select_range(4, 4);
    assert_eq!(r.line_start_at_caret(), 4, "the caret on the empty line");
    r.handle.select_range(5, 5);
    assert_eq!(r.line_start_at_caret(), 5, "after the second break");
    r.handle.select_range(3, 3);
    assert_eq!(r.line_start_at_caret(), 0, "before the first break");
}
