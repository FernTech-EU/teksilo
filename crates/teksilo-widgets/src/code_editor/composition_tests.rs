// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An input method's composition ended by the editor rather than by the input
//! method: a press or a tap in the editor, a focus change, an edit command,
//! and the command of a control that takes no focus. Each keeps the
//! composition's text where it stands, as one undo step, and tells the
//! platform the composition is over, as `RichTextEditor` and
//! `TextInputField` do.
//!
//! winit sends an empty update right before every commit, on every platform,
//! so the tests send one too where the platform would.

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::pointer::PointerPhase;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, TextDocument};

use super::state::SharedState;
use super::{CodeEditor, CodeEditorHandle};
use crate::button::press_test_support::{finger, touch};
use crate::common::ime_ops_test::ImeOps;

fn viewport() -> SizeProposal {
    SizeProposal::exact(520.0, 400.0)
}

/// What a tool does when clicked: the editor's undo, through its handle.
#[derive(Clone, Copy, Debug)]
enum Tool {
    /// A `Button` built with `focus_on_click(false)`.
    KeepsFocusAway,
    /// A plain `on_tap` node with nothing focusable up to the root.
    NotFocusable,
}

/// A focused code editor below a tool and beside somewhere else for the focus
/// to go, its input method's requests counted in `ops`.
struct Editor {
    tree: WidgetTree,
    state: SharedState,
    handle: CodeEditorHandle,
    doc: TextDocument,
    editor: WidgetId,
    tool: WidgetId,
    elsewhere: WidgetId,
    ops: ImeOps,
}

impl Editor {
    /// An editor holding `text`, its caret at `caret`, whose tool is `tool`.
    fn with_tool(text: &str, caret: usize, tool: Tool) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let editor = CodeEditor::new(doc.clone());
        let state = editor.state.clone();
        let handle = editor.handle();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let undo = {
            let handle = handle.clone();
            move || handle.undo()
        };
        let tool = match tool {
            Tool::KeepsFocusAway => tree.add(
                crate::button::Button::new(teksilo_i18n::lit!("Undo"))
                    .on_activate_fn(move |_| undo())
                    .focus_on_click(false),
            ),
            Tool::NotFocusable => tree.add(
                crate::primitives::FixedSize::new()
                    .width(60.0)
                    .height(24.0)
                    .child(crate::primitives::RectWidget::new())
                    .on_tap(move |_, _| undo()),
            ),
        };
        let other = tree.add(
            crate::primitives::FixedSize::new()
                .width(40.0)
                .height(20.0)
                .child(crate::primitives::RectWidget::new().focusable(true)),
        );
        let padded = tree.add(
            crate::primitives::FixedSize::new()
                .height(240.0)
                .child(crate::primitives::Padding::symmetric(20.0, 20.0).child(editor)),
        );
        tree.add(
            crate::primitives::VStack::new()
                .child(tool)
                .child(padded)
                .child(other),
        );
        tree.layout(viewport());
        // The engine lays out in `paint`: a test that never renders hit-tests
        // an empty layout.
        let _ = tree.render();
        let editor = tree
            .first_focusable_descendant(padded)
            .expect("the editor is focusable");
        let elsewhere = tree
            .first_focusable_descendant(other)
            .expect("somewhere else for the focus to go");
        tree.focus(editor);
        let mut e = Self {
            tree,
            state,
            handle,
            doc,
            editor,
            tool,
            elsewhere,
            ops: ImeOps::default(),
        };
        e.settle();
        e.state
            .borrow()
            .cursor
            .set_position(caret, MoveMode::MoveAnchor);
        e
    }

    fn new(text: &str, caret: usize) -> Self {
        Self::with_tool(text, caret, Tool::KeepsFocusAway)
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

    fn dispatch(&mut self, event: WidgetEvent) {
        self.tree.dispatch_event_with_ops(event, &mut self.ops);
    }

    fn compose(&mut self, text: &str) {
        self.dispatch(WidgetEvent::ime_composition(text, None));
    }

    /// What winit sends for a commit: an empty update, then the text.
    fn commit(&mut self, text: &str) {
        self.compose("");
        self.dispatch(WidgetEvent::ime_commit(text));
    }

    fn type_keys(&mut self, text: &str) {
        let focused = self.tree.focused().expect("the editor is focused");
        self.tree.type_text(focused, text);
        self.settle();
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.dispatch(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
    }

    fn text(&self) -> String {
        self.doc.to_plain_text().unwrap_or_default()
    }

    fn caret(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    fn composition(&self) -> Option<std::ops::Range<usize>> {
        let st = self.state.borrow();
        st.composition.range(&st.document)
    }

    fn is_composing(&self) -> bool {
        let st = self.state.borrow();
        st.composition.is_held(&st.document)
    }

    fn undo(&mut self) {
        self.handle.undo();
        self.settle();
    }

    /// The window point of the caret boundary before character `offset`.
    fn at(&self, offset: usize) -> Point {
        let st = self.state.borrow();
        let rect = super::keyboard::window_rect_at(&st, offset).expect("the editor has laid out");
        Point::new(rect.x + 0.5, rect.y + rect.height / 2.0)
    }

    fn click_at(&mut self, at: Point) {
        self.dispatch(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        self.dispatch(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }

    fn click(&mut self, offset: usize) {
        let at = self.at(offset);
        self.click_at(at);
    }

    fn finger_tap(&mut self, offset: usize) {
        let at = self.at(offset);
        let id = finger();
        self.tree
            .dispatch_pointer_with_ops(touch(id, PointerPhase::Down, at, 0), &mut self.ops);
        self.tree
            .dispatch_pointer_with_ops(touch(id, PointerPhase::Up, at, 30), &mut self.ops);
    }

    fn click_tool(&mut self) {
        let b = self.tree.bounds(self.tool);
        self.click_at(Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0));
    }

    fn focus_elsewhere(&mut self) {
        let elsewhere = self.elsewhere;
        self.tree.focus_ops(elsewhere, &mut self.ops);
    }
}

#[test]
fn a_click_elsewhere_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello world", 5);
    e.compose("ni");
    assert_eq!(e.text(), "helloni world");

    e.click(1);
    assert_eq!(e.text(), "helloni world", "the composition's text stays");
    assert_eq!(e.caret(), 1, "and the caret goes to the click");
    assert_eq!(e.composition(), None, "it is no longer a composition");
    assert!(!e.is_composing());
    assert_eq!(e.ops.finished, 1, "the platform is told once");

    // What the platform may still deliver: an empty update and an empty commit.
    e.commit("");
    e.settle();
    assert_eq!(e.text(), "helloni world", "a late empty update is inert");
    assert_eq!(e.caret(), 1);
    assert_eq!(e.ops.finished, 1);

    // As a commit would be, the kept text is one undo step.
    e.undo();
    assert_eq!(e.text(), "hello world");
}

/// A press inside the composition's own text is treated as any other press.
#[test]
fn a_click_inside_the_composition_keeps_it_too() {
    let mut e = Editor::new("hello world", 5);
    e.compose("nihao");
    // Laid out, so the click lands inside the composition as shown.
    e.settle();
    assert_eq!(e.composition(), Some(5..10));
    e.click(6);
    assert_eq!(e.text(), "hellonihao world");
    assert_eq!(e.caret(), 6);
    assert_eq!(e.composition(), None);
    assert_eq!(e.ops.finished, 1);
}

#[test]
fn a_tap_elsewhere_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello world", 5);
    e.compose("ni");
    e.finger_tap(1);
    assert_eq!(e.text(), "helloni world");
    assert_eq!(e.caret(), 1);
    assert!(!e.is_composing());
    assert_eq!(e.ops.finished, 1);
}

/// Focus leaving ends the composition with its text kept: it was thrown away,
/// so a writer who clicked into another field lost what the input method
/// showed.
#[test]
fn a_focus_change_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello", 5);
    e.compose("ni");
    e.focus_elsewhere();
    assert_eq!(e.text(), "helloni");
    assert_eq!(e.ops.finished, 1);
    assert!(!e.is_composing());

    // The platform's late empty update and empty commit change nothing.
    e.commit("");
    e.settle();
    assert_eq!(e.text(), "helloni");
    e.undo();
    assert_eq!(e.text(), "hello", "the kept text is one undo step");
}

#[test]
fn a_press_with_no_composition_ends_nothing() {
    let mut e = Editor::new("hello world", 5);
    e.click(1);
    assert_eq!(e.caret(), 1);
    e.focus_elsewhere();
    assert_eq!(e.ops.finished, 0);
    assert_eq!(e.text(), "hello world");
}

/// On Windows the input method completes the composition as the click ends
/// it, and its commit can differ from what it showed: Microsoft Pinyin shows
/// `nihao` as "ni'hao" and commits "nihao". The editor keeps the commit; with
/// none, or one that leaves nothing once cleaned, what it showed.
#[test]
fn a_click_keeps_the_input_methods_commit_in_place_of_what_it_showed() {
    for (shown, committed, kept) in [
        ("ni'hao", Some("nihao"), "nihao"),
        ("ni'hao", None, "ni'hao"),
        ("ni'hao", Some(""), "ni'hao"),
        ("ni'hao", Some("\u{7}"), "ni'hao"),
    ] {
        let mut e = Editor::new("abc def ", 8);
        e.ops.committed = committed.map(str::to_owned);
        e.compose("n");
        e.compose(shown);
        e.click(1);
        let expected = format!("abc def {kept}");
        assert_eq!(e.text(), expected, "{committed:?}");
        assert_eq!(e.caret(), 1, "{committed:?}: the caret at the click");
        assert_eq!(e.ops.finished, 1, "{committed:?}");
        // What reaches the editor after the handler: the empty update (the
        // application drops winit's copy of the commit).
        e.compose("");
        e.settle();
        assert_eq!(e.text(), expected, "{committed:?}: nothing changes after");
        e.undo();
        assert_eq!(
            e.text(),
            "abc def ",
            "{committed:?}: one undo takes the kept text back"
        );
    }
}

/// A click past the end of a line whose composition the input method commits
/// shorter than it showed puts the caret at the end of the line: the press
/// hit-tests the text as the commit left it, not as the composition showed
/// it, which reached past the end of the committed text.
#[test]
fn a_click_after_a_shorter_commit_hits_the_committed_text() {
    let mut e = Editor::new("abc  def\nxyz12", 4);
    e.ops.committed = Some("\u{4F60}".to_owned());
    e.compose("ni'hao");
    e.settle();
    assert_eq!(e.text(), "abc ni'hao def\nxyz12");
    let end = e.at(14);
    e.click_at(Point::new(end.x + 40.0, end.y));
    assert_eq!(e.text(), "abc \u{4F60} def\nxyz12");
    assert_eq!(e.caret(), 9, "the end of the committed line, not the next");
}

/// A finger's hold selects the word under it, so it ends a composition first,
/// as a tap does (the finger's press did not: it may have been a pan).
#[test]
fn a_hold_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello world", 11);
    e.compose("ni");
    e.settle();
    let at = e.at(2);
    e.tree.long_press_at(teksilo_tokens::PointerKind::Touch, at);
    assert_eq!(e.text(), "hello worldni");
    assert!(!e.is_composing());
}

/// An undo, or one of the editor's own edit commands, pressed while a
/// composition shows ends the composition first, its text kept, then acts on
/// that text: the platform is told, so the input method's next update cannot
/// bring the composition back at the caret. Select-all leaves it alive.
#[test]
fn an_edit_command_during_a_composition_ends_it_first() {
    for (key, expected, finished) in [
        (Key::Z, "abc ", 1),
        (Key::D, "abc ni\nabc ni", 1),
        (Key::A, "abc ni", 0),
    ] {
        let mut e = Editor::new("", 0);
        e.type_keys("abc ");
        e.compose("ni");
        e.key(key, Modifiers::COMMAND);
        e.settle();
        assert_eq!(e.ops.finished, finished, "{key:?}: the platform is told");
        assert_eq!(e.is_composing(), finished == 0, "{key:?}");
        assert_eq!(e.text(), expected, "{key:?}");
    }
}

/// A click on a control that takes no focus (a toolbar's Undo), built with
/// `focus_on_click(false)` or not focusable at all, ends the composition
/// first, its text kept, as a blur would: the command then acts on that
/// text. Left alive, the undo took the composition out of the text while the
/// input method still held it, and the next key brought it back.
#[test]
fn a_command_from_a_control_that_takes_no_focus_ends_the_composition_first() {
    for tool in [Tool::KeepsFocusAway, Tool::NotFocusable] {
        let mut e = Editor::with_tool("", 0, tool);
        e.type_keys("abc ");
        e.compose("ni");
        assert_eq!(e.text(), "abc ni");
        e.click_tool();
        assert_eq!(
            e.ops.finished, 1,
            "{tool:?}: the platform is told the composition is over"
        );
        assert!(!e.is_composing(), "{tool:?}");
        assert_eq!(
            e.text(),
            "abc ",
            "{tool:?}: the undo took back the kept composition, not the typing before it"
        );
        assert_eq!(
            e.tree.focused(),
            Some(e.editor),
            "{tool:?}: the editor keeps the focus"
        );
    }
}

/// A composition is at the primary caret only: an input method has one
/// composition window. The other carets go as it starts, and a click keeps it
/// once.
#[test]
fn a_composition_with_several_carets_is_at_the_primary_one() {
    let mut e = Editor::new("ab\ncd", 2);
    {
        let mut st = e.state.borrow_mut();
        super::mouse::add_caret_at(&mut st, 5);
    }
    e.compose("ni");
    assert_eq!(e.text(), "abni\ncd");
    assert!(e.state.borrow().extra_carets.is_empty());
    e.click(0);
    assert_eq!(e.text(), "abni\ncd");
    assert_eq!(e.ops.finished, 1);
}
