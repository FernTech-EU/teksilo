// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A key, a command, an assistive technology's action or a press that comes
//! in the same frame as typing acts after that typing.
//!
//! Typed characters wait for the frame tick, which inserts them as one batch.
//! Every test here delivers the characters and what follows them with no
//! layout pass in between (the frame tick only runs inside one), so the
//! characters are still waiting when the next event arrives.

use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::accesskit::{Action, ActionData, TextPosition, TextSelection};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_platform::clipboard::{ClipboardHandle, MemoryClipboard};
use teksilo_text::text_document::{MoveMode, TextDocument};

use super::state::SharedState;
use super::{CodeEditor, CodeEditorHandle};

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

struct Editor {
    tree: WidgetTree,
    state: SharedState,
    handle: CodeEditorHandle,
    doc: TextDocument,
    editor: WidgetId,
    clipboard: ClipboardHandle,
}

impl Editor {
    /// A focused editor holding `text`, its caret at `caret`.
    fn new(text: &str, caret: usize) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let widget = CodeEditor::new(doc.clone());
        let state = widget.state.clone();
        let handle = widget.handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let clipboard = ClipboardHandle::new(MemoryClipboard::new());
        let mut registry: std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>> =
            std::collections::HashMap::new();
        registry.insert(
            std::any::TypeId::of::<ClipboardHandle>(),
            Box::new(clipboard.clone()),
        );
        tree.set_app_context(Rc::new(
            teksilo_core::event_source::TreeAppContext::empty().with_app_state(registry),
        ));
        let root = tree.add(widget);
        tree.layout(viewport());
        // The engine lays out in `paint`: a test that never renders hit-tests
        // an empty layout.
        let _ = tree.render();
        let editor = tree
            .first_focusable_descendant(root)
            .expect("the editor is focusable");
        tree.focus(editor);
        let mut e = Self {
            tree,
            state,
            handle,
            doc,
            editor,
            clipboard,
        };
        e.settle();
        e.select(caret, caret);
        e
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(viewport());
        }
        let _ = self.tree.render();
    }

    /// Typing, with no frame after it: the characters are still waiting.
    fn type_now(&mut self, text: &str) {
        self.tree.type_text(self.editor, text);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
    }

    fn select(&mut self, anchor: usize, position: usize) {
        let st = self.state.borrow();
        st.cursor.set_position(anchor, MoveMode::MoveAnchor);
        st.cursor.set_position(position, MoveMode::KeepAnchor);
    }

    /// The text, after the frames that insert anything still waiting.
    fn text_after_frame(&mut self) -> String {
        self.settle();
        self.doc.to_plain_text().unwrap_or_default()
    }

    /// `(anchor, position)` of the primary caret.
    fn caret(&self) -> (usize, usize) {
        let st = self.state.borrow();
        (st.cursor.anchor(), st.cursor.position())
    }

    /// The window point of the caret boundary before character `offset`.
    fn at(&self, offset: usize) -> Point {
        let st = self.state.borrow();
        let rect = super::keyboard::window_rect_at(&st, offset).expect("the editor has laid out");
        Point::new(rect.x + 0.5, rect.y + rect.height / 2.0)
    }

    fn click_at(&mut self, at: Point) {
        self.tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }

    fn access(&mut self, action: Action, data: ActionData) {
        self.tree.dispatch_event(WidgetEvent::AccessAction {
            action,
            target: Some(self.editor),
            target_node: teksilo_core::accessibility::widget_id_to_node_id(self.editor),
            data: Some(data),
        });
    }
}

// ── Keys ────────────────────────────────────────────────────────────────────

#[test]
fn backspace_removes_the_last_character_typed_in_its_frame() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::Backspace, Modifiers::NONE);
    assert_eq!(e.text_after_frame(), "xya");
    assert_eq!(e.caret(), (3, 3));
}

#[test]
fn enter_breaks_the_line_after_the_typing() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::Enter, Modifiers::NONE);
    assert_eq!(e.text_after_frame(), "xyab\n");
}

#[test]
fn left_moves_back_from_the_end_of_the_typing() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(e.text_after_frame(), "xyab");
    assert_eq!(e.caret(), (3, 3));
}

#[test]
fn home_goes_to_the_line_start_after_the_typing_went_in() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::Home, Modifiers::NONE);
    assert_eq!(e.text_after_frame(), "xyab");
    assert_eq!(e.caret(), (0, 0));
}

#[test]
fn end_goes_to_the_line_end_after_the_typing_went_in() {
    let mut e = Editor::new("xy", 0);
    e.type_now("ab");
    e.press(Key::End, Modifiers::NONE);
    assert_eq!(e.text_after_frame(), "abxy");
    assert_eq!(e.caret(), (4, 4));
}

#[test]
fn shift_left_selects_the_last_character_typed() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(e.text_after_frame(), "xyab");
    assert_eq!(e.caret(), (4, 3));
}

// ── The standard edit commands ──────────────────────────────────────────────

#[test]
fn select_all_selects_the_typing_too() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::A, Modifiers::COMMAND);
    assert_eq!(e.text_after_frame(), "xyab");
    assert_eq!(e.caret(), (0, 4));
}

#[test]
fn paste_goes_in_after_the_typing() {
    let mut e = Editor::new("xy", 2);
    let _ = e.clipboard.set_text("P");
    e.type_now("ab");
    e.press(Key::V, Modifiers::COMMAND);
    assert_eq!(e.text_after_frame(), "xyabP");
}

#[test]
fn undo_takes_back_the_typing_of_its_frame() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(e.text_after_frame(), "xy");
}

// ── The handle ──────────────────────────────────────────────────────────────

#[test]
fn the_handle_selects_all_after_the_typing() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.handle.select_all();
    assert_eq!(e.text_after_frame(), "xyab");
    assert_eq!(e.caret(), (0, 4));
}

#[test]
fn the_handle_undoes_the_typing_of_its_frame() {
    let mut e = Editor::new("xy", 2);
    e.type_now("ab");
    e.handle.undo();
    assert_eq!(e.text_after_frame(), "xy");
}

// ── An assistive technology's actions ───────────────────────────────────────

#[test]
fn an_assistive_insertion_goes_in_after_the_typing() {
    let mut e = Editor::new("", 0);
    e.type_now("ab");
    e.access(Action::ReplaceSelectedText, ActionData::Value("Z".into()));
    assert_eq!(e.text_after_frame(), "abZ");
}

#[test]
fn an_assistive_whole_value_write_replaces_the_typing_too() {
    let mut e = Editor::new("", 0);
    e.type_now("ab");
    e.access(Action::SetValue, ActionData::Value("Q".into()));
    assert_eq!(e.text_after_frame(), "Q");
}

#[test]
fn an_assistive_selection_lands_where_the_reader_put_it() {
    // The reader's offsets are in the text it was shown, without the typing:
    // the typing goes in where it was typed, and the selection is carried
    // across it.
    let mut e = Editor::new("xyz", 1);
    // The walk publishes the run a reader addresses.
    let _ = e.tree.accessibility_tree_snapshot();
    let node = {
        let st = e.state.borrow();
        let map = st.synthetic_to_element.borrow();
        let (node, _) = map
            .iter()
            .find(|(_, run)| run.text == "xyz")
            .expect("the line's run is mapped");
        *node
    };
    e.type_now("ab");
    let position = |character_index| TextPosition {
        node,
        character_index,
    };
    e.access(
        Action::SetTextSelection,
        ActionData::SetTextSelection(TextSelection {
            anchor: position(2),
            focus: position(3),
        }),
    );
    assert_eq!(e.text_after_frame(), "xabyz");
    assert_eq!(e.caret(), (4, 5), "\"z\" is selected, as the reader asked");
}

// ── A press ─────────────────────────────────────────────────────────────────

#[test]
fn a_press_after_typing_leaves_the_typing_where_it_was_typed() {
    let mut e = Editor::new("xy", 2);
    let start = e.at(0);
    e.type_now("ab");
    e.click_at(start);
    assert_eq!(e.text_after_frame(), "xyab");
    assert_eq!(e.caret(), (0, 0));
}

// ── Typing alone still waits for the frame ──────────────────────────────────

#[test]
fn a_burst_of_typing_waits_for_the_frame_and_is_one_undo_step() {
    let mut e = Editor::new("", 0);
    e.type_now("abc");
    assert_eq!(
        e.state.borrow().pending_chars,
        "abc",
        "typing waits for the frame, which inserts it as one batch"
    );
    assert_eq!(e.text_after_frame(), "abc");
    e.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(e.text_after_frame(), "", "one undo takes the whole burst");
}
