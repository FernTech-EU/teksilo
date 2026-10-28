// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An input method's composition that takes the place of text already in the
//! document (Korean Hanja conversion), and a composition ended by a press or a
//! focus change, which keeps its text where it is and tells the platform it is
//! over.
//!
//! winit sends an empty update right before every commit, on every platform,
//! so the tests send one too: an empty update on its own must not decide that
//! a composition was cancelled.
//!
//! The composition is shown as transient edits, which no undo history
//! records: an undo after a commit takes back the committed text, never the
//! states the composition went through.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::pointer::PointerPhase;
use teksilo_core::signal::Signal;
use teksilo_core::text_input_document::TextInputDocument;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, TextDocument};

use super::state::SharedState;
use super::{EditorHandle, RichTextEditor};
use crate::EditSource;
use crate::button::press_test_support::{finger, touch};
use crate::common::ime_ops_test::ImeOps;
use crate::primitives::TextInputField;

fn viewport() -> SizeProposal {
    SizeProposal::exact(480.0, 400.0)
}

struct Editor {
    tree: WidgetTree,
    state: SharedState,
    handle: EditorHandle,
    doc: TextDocument,
    /// A title field, where focus goes next.
    title: WidgetId,
    title_text: Signal<String>,
    arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
    ops: ImeOps,
}

impl Editor {
    /// A focused, laid-out editor holding `text`, its caret at `caret`, below
    /// a title field.
    fn new(text: &str, caret: usize) -> Self {
        let doc = TextDocument::new();
        doc.set_plain_text(text).unwrap();
        let arrivals = Rc::new(RefCell::new(Vec::new()));
        let seen = arrivals.clone();
        let editor = RichTextEditor::editor(doc.clone())
            .on_text_inserted(move |source, chars| seen.borrow_mut().push((source, chars)));
        let handle = editor.handle();
        let state = editor.state.clone();
        let title_text = Signal::new(String::new());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let other = tree.add(
            crate::primitives::FixedSize::new()
                .width(300.0)
                .child(TextInputField::new(title_text.clone())),
        );
        let padded = tree.add(crate::primitives::Padding::symmetric(20.0, 20.0).child(editor));
        tree.add(crate::primitives::VStack::new().child(other).child(padded));
        tree.layout(viewport());
        // The engine lays out in `paint`: a test that never renders hit-tests
        // an empty layout.
        let _ = tree.render();
        let editor = tree
            .first_focusable_descendant(padded)
            .expect("the editor is focusable");
        let title = tree
            .first_focusable_descendant(other)
            .expect("the title field is focusable");
        tree.focus(editor);
        let mut e = Self {
            tree,
            state,
            handle,
            doc,
            title,
            title_text,
            arrivals,
            ops: ImeOps::default(),
        };
        e.settle();
        e.handle.select_range(caret, caret);
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

    fn dispatch(&mut self, event: WidgetEvent) {
        self.tree.dispatch_event_with_ops(event, &mut self.ops);
    }

    fn compose(&mut self, text: &str, replacement: Option<std::ops::Range<usize>>) {
        self.dispatch(WidgetEvent::ImeComposition {
            text: text.to_string(),
            cursor: None,
            replacement,
        });
    }

    /// What winit sends for a commit: an empty update, then the text.
    fn commit(&mut self, text: &str) {
        self.compose("", None);
        self.dispatch(WidgetEvent::ime_commit(text));
    }

    /// The window point of the caret boundary before character `offset`.
    fn at(&self, offset: usize) -> Point {
        let rect = self
            .handle
            .offset_rect(offset)
            .expect("the editor has laid out");
        Point::new(rect.x + 0.5, rect.y + rect.height / 2.0)
    }

    fn click(&mut self, offset: usize) {
        let at = self.at(offset);
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

    fn finger_tap(&mut self, offset: usize) {
        let at = self.at(offset);
        let id = finger();
        self.tree
            .dispatch_pointer_with_ops(touch(id, PointerPhase::Down, at, 0), &mut self.ops);
        self.tree
            .dispatch_pointer_with_ops(touch(id, PointerPhase::Up, at, 30), &mut self.ops);
    }

    fn text(&self) -> String {
        self.doc.to_plain_text().unwrap_or_default()
    }

    fn caret(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    fn composition(&self) -> Option<std::ops::Range<usize>> {
        TextInputDocument::composition_range(&self.handle)
    }

    fn arrivals(&self) -> Vec<(EditSource, usize)> {
        self.arrivals.borrow().clone()
    }

    /// Type `text` as keys, and let the frame insert it.
    fn type_keys(&mut self, text: &str) {
        let editor = self.tree.focused().expect("the editor is focused");
        self.tree.type_text(editor, text);
        self.settle();
    }

    /// Show each of `states` as the composition's next update.
    fn compose_states(&mut self, states: &[&str]) {
        for state in states {
            self.compose(state, None);
        }
    }

    fn undo(&mut self) {
        self.handle.undo();
        self.settle();
    }

    fn redo(&mut self) {
        self.handle.redo();
        self.settle();
    }

    fn history(&self) -> (bool, bool) {
        (self.doc.can_undo(), self.doc.can_redo())
    }
}

const PINYIN: [&str; 5] = ["n", "ni", "nih", "niha", "nihao"];

// ── A composition that takes the place of text ──────────────────────────────

/// The Korean Hanja conversion: the input method committed 한, then marked
/// 韓 over it. The editor shows 韓 in its place, not after it.
#[test]
fn a_composition_takes_the_place_of_the_text_it_marks() {
    let mut e = Editor::new("X\u{D55C}Y", 2);
    e.compose("\u{97D3}", Some(1..2));
    assert_eq!(e.text(), "X\u{97D3}Y", "the marked text is replaced");
    assert_eq!(e.composition(), Some(1..2));

    e.commit("\u{97D3}");
    e.settle();
    assert_eq!(e.text(), "X\u{97D3}Y", "the commit takes the place");
    assert_eq!(e.caret(), 2);
    assert_eq!(e.composition(), None);
    assert_eq!(e.arrivals(), vec![(EditSource::Ime, 1)]);

    e.handle.undo();
    e.settle();
    assert_eq!(
        e.text(),
        "X\u{D55C}Y",
        "one undo puts back the syllable that was converted"
    );
}

#[test]
fn a_cancelled_replacement_puts_the_text_back() {
    let mut e = Editor::new("X\u{D55C}Y", 2);
    e.compose("\u{97D3}", Some(1..2));
    // The empty update, and no commit after it.
    e.compose("", None);
    assert_eq!(
        e.text(),
        "X\u{97D3}Y",
        "the empty update alone decides nothing: a commit may follow"
    );
    e.settle();
    assert_eq!(
        e.text(),
        "X\u{D55C}Y",
        "the frame after it settles a cancel"
    );
    assert_eq!(e.caret(), 2, "the caret after the text put back");
    assert!(e.arrivals().is_empty(), "{:?}", e.arrivals());
}

/// A cancel is also settled by the next key, before the key acts: the caret
/// the key moves is the one after the text put back.
#[test]
fn a_key_after_the_empty_update_settles_a_cancel_first() {
    let mut e = Editor::new("X\u{D55C}Y", 2);
    e.compose("\u{97D3}", Some(1..2));
    e.compose("", None);
    e.tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    e.settle();
    assert_eq!(e.text(), "X\u{D55C}Y");
    assert_eq!(e.caret(), 1, "the key moved the caret from after the text");
}

/// A cancel followed straight by a new composition: the text goes back first,
/// and the new composition goes after it.
#[test]
fn a_new_composition_after_the_empty_update_settles_a_cancel_first() {
    let mut e = Editor::new("X\u{D55C}Y", 2);
    e.compose("\u{97D3}", Some(1..2));
    e.compose("", None);
    e.compose("ni", None);
    assert_eq!(e.text(), "X\u{D55C}niY");
    assert_eq!(e.composition(), Some(2..4));
}

/// The replaced text keeps its formatting when it goes back.
#[test]
fn a_cancel_puts_back_the_text_with_its_format() {
    let mut e = Editor::new("X\u{D55C}Y", 2);
    {
        let st = e.state.borrow();
        st.cursor.set_position(1, MoveMode::MoveAnchor);
        st.cursor.set_position(2, MoveMode::KeepAnchor);
        let _ = st
            .cursor
            .merge_char_format(&teksilo_text::text_document::TextFormat {
                font_bold: Some(true),
                ..Default::default()
            });
        st.cursor.set_position(2, MoveMode::MoveAnchor);
    }
    e.settle();
    e.compose("\u{97D3}", Some(1..2));
    e.compose("", None);
    e.settle();
    assert_eq!(e.text(), "X\u{D55C}Y");
    let st = e.state.borrow();
    st.cursor.set_position(1, MoveMode::MoveAnchor);
    st.cursor.set_position(2, MoveMode::KeepAnchor);
    let html = st.cursor.selection().to_html();
    assert!(
        html.contains("<strong>") || html.contains("font-weight") || html.contains("<b>"),
        "the syllable put back is still bold: {html}"
    );
}

/// The title field's case on the editor: the input method marked four
/// characters, then the user moved through candidates. Nothing is duplicated,
/// in a later paragraph too.
#[test]
fn a_composition_replaces_several_characters() {
    let mut e = Editor::new("one\nChapter \u{B098}\u{D55C}\u{AE00}\u{D55C} end", 16);
    e.compose("\u{B098}\u{D55C}\u{AE00}\u{D55C}", Some(12..16));
    assert_eq!(
        e.text(),
        "one\nChapter \u{B098}\u{D55C}\u{AE00}\u{D55C} end"
    );
    assert_eq!(e.composition(), Some(12..16));
    e.compose("\u{7F85}\u{97D3}\u{AE00}\u{97D3}", Some(12..16));
    assert_eq!(
        e.text(),
        "one\nChapter \u{7F85}\u{97D3}\u{AE00}\u{97D3} end"
    );
    e.commit("\u{7F85}\u{97D3}\u{AE00}\u{97D3}");
    e.settle();
    assert_eq!(
        e.text(),
        "one\nChapter \u{7F85}\u{97D3}\u{AE00}\u{97D3} end"
    );
    assert_eq!(e.arrivals(), vec![(EditSource::Ime, 4)]);
}

/// A replacement reaching the end of a later paragraph is taken whole: the
/// range counts the paragraph breaks.
#[test]
fn a_replacement_at_the_end_of_a_later_paragraph_is_taken_whole() {
    let mut e = Editor::new("one\ntwo\u{D55C}", 8);
    e.compose("\u{97D3}", Some(7..8));
    assert_eq!(e.text(), "one\ntwo\u{97D3}");
    e.commit("\u{97D3}");
    e.settle();
    assert_eq!(e.text(), "one\ntwo\u{97D3}");
}

// ── A press or a focus change ends a composition ────────────────────────────

#[test]
fn a_click_elsewhere_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello world", 5);
    e.compose("ni", None);
    assert_eq!(e.text(), "helloni world");

    e.click(1);
    assert_eq!(e.text(), "helloni world", "the composition's text stays");
    assert_eq!(e.caret(), 1, "and the caret goes to the click");
    assert_eq!(e.composition(), None, "it is no longer a composition");
    assert_eq!(e.arrivals(), vec![(EditSource::Ime, 2)]);
    assert_eq!(e.ops.finished, 1, "the platform is told once");

    // What the platform may still deliver: an empty update and an empty commit.
    e.commit("");
    e.settle();
    assert_eq!(e.text(), "helloni world", "a late empty update is inert");
    assert_eq!(e.caret(), 1);
    assert_eq!(e.ops.finished, 1);

    // As a commit would be, the kept text is one undo step.
    e.handle.undo();
    e.settle();
    assert_eq!(e.text(), "hello world");
}

/// A press inside the composition's own text is treated as any other press.
#[test]
fn a_click_inside_the_composition_keeps_it_too() {
    let mut e = Editor::new("hello world", 5);
    e.compose("nihao", None);
    e.click(6);
    assert_eq!(e.text(), "hellonihao world");
    assert_eq!(e.caret(), 6);
    assert_eq!(e.composition(), None);
    assert_eq!(e.ops.finished, 1);
}

#[test]
fn a_tap_elsewhere_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello world", 5);
    e.compose("ni", None);
    e.finger_tap(1);
    assert_eq!(e.text(), "helloni world");
    assert_eq!(e.caret(), 1);
    assert_eq!(e.ops.finished, 1);
}

#[test]
fn a_click_during_a_replacement_keeps_the_replacement() {
    let mut e = Editor::new("X\u{D55C}Y", 2);
    e.compose("\u{97D3}", Some(1..2));
    e.click(0);
    assert_eq!(e.text(), "X\u{97D3}Y");
    assert_eq!(e.caret(), 0);
    assert_eq!(e.ops.finished, 1);
    e.commit("");
    e.settle();
    assert_eq!(e.text(), "X\u{97D3}Y", "nothing put back after the finish");
    e.handle.undo();
    e.settle();
    assert_eq!(e.text(), "X\u{D55C}Y", "one undo puts the marked text back");
}

/// On macOS a composition started in the editor carried on in the title
/// field after a click there. Focus leaving ends it, its text kept.
#[test]
fn a_focus_change_keeps_the_composition_and_ends_it() {
    let mut e = Editor::new("hello", 5);
    e.compose("ni", None);
    let title = e.title;
    e.tree.focus_ops(title, &mut e.ops);
    assert_eq!(e.text(), "helloni");
    assert_eq!(e.ops.finished, 1);
    assert_eq!(e.arrivals(), vec![(EditSource::Ime, 2)]);
    assert!(!e.handle.is_composing());

    // The platform's late empty update and empty commit reach the field
    // focused now, and change nothing there either.
    e.commit("");
    e.settle();
    assert_eq!(e.text(), "helloni");
    assert_eq!(e.title_text.get(), "");
}

#[test]
fn a_press_with_no_composition_ends_nothing() {
    let mut e = Editor::new("hello world", 5);
    e.click(1);
    assert_eq!(e.caret(), 1);
    let title = e.title;
    e.tree.focus_ops(title, &mut e.ops);
    assert_eq!(e.ops.finished, 0);
    assert!(e.arrivals().is_empty());
}

// ── Typing just before a press ──────────────────────────────────────────────

/// The Microsoft Korean IME commits its syllable on the mouse-down itself: an
/// empty update and the commit reach the editor before the press. The
/// syllable goes in where it was composed, and the press then moves the caret
/// to the point the screen showed.
#[test]
fn a_commit_just_before_a_click_lands_where_it_was_composed() {
    let mut e = Editor::new("Showcase exercises", 8);
    e.compose("\u{D55C}", None);
    e.settle();
    // On screen: "Showcase한 exercises"; the click aims inside "exercises".
    let target = e.at(12);
    e.commit("\u{D55C}");
    e.dispatch(WidgetEvent::pointer_down(
        target,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    e.dispatch(WidgetEvent::pointer_up(
        target,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert_eq!(e.text(), "Showcase\u{D55C} exercises");
    assert_eq!(e.caret(), 12, "the caret where the click landed on screen");
    e.settle();
    assert_eq!(e.text(), "Showcase\u{D55C} exercises");
    assert_eq!(e.arrivals(), vec![(EditSource::Ime, 1)]);
    assert_eq!(e.ops.finished, 0, "the composition was already over");
}

#[test]
fn typing_just_before_a_click_lands_where_it_was_typed() {
    let mut e = Editor::new("Showcase exercises", 8);
    let target = e.at(2);
    let editor = e.tree.focused().unwrap();
    e.tree.type_text(editor, "ab");
    e.dispatch(WidgetEvent::pointer_down(
        target,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    e.settle();
    assert_eq!(e.text(), "Showcaseab exercises");
    assert_eq!(e.caret(), 2);
}

#[test]
fn a_commit_just_before_a_tap_lands_where_it_was_composed() {
    let mut e = Editor::new("Showcase exercises", 8);
    e.compose("\u{D55C}", None);
    e.settle();
    let target = e.at(2);
    e.commit("\u{D55C}");
    let id = finger();
    e.tree
        .dispatch_pointer_with_ops(touch(id, PointerPhase::Down, target, 0), &mut e.ops);
    e.tree
        .dispatch_pointer_with_ops(touch(id, PointerPhase::Up, target, 30), &mut e.ops);
    e.settle();
    assert_eq!(e.text(), "Showcase\u{D55C} exercises");
    assert_eq!(e.caret(), 2);
}

/// A selection the application makes in the same frame: the typing goes in
/// where it was typed, not over the selection, and the range is read in the
/// text without it.
#[test]
fn typing_just_before_a_programmatic_selection_lands_where_it_was_typed() {
    let mut e = Editor::new("Showcase exercises", 8);
    let editor = e.tree.focused().unwrap();
    e.tree.type_text(editor, "ab");
    e.handle.select_range(9, 18);
    e.settle();
    assert_eq!(e.text(), "Showcaseab exercises");
    assert_eq!(
        e.handle.selected_text(),
        "exercises",
        "the range moved with the text typed before it"
    );
}

// ── The preedit underline ───────────────────────────────────────────────────

/// The underline under a composition follows the editor's own vertical scroll:
/// the engine's caret geometry is already in screen space, so taking the
/// scroll off it again drew the underline that far above the composition,
/// under an earlier line.
#[test]
fn the_preedit_underline_follows_the_editor_scroll() {
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!("line {i} of a document that scrolls\n"));
    }
    let doc = TextDocument::new();
    doc.set_plain_text(&text).unwrap();
    let editor = RichTextEditor::editor(doc);
    let handle = editor.handle();
    let state = editor.state.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(editor);
    let size = SizeProposal::exact(400.0, 160.0);
    let frame = |tree: &mut WidgetTree| {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(size);
        let _ = tree.render();
    };
    frame(&mut tree);
    tree.focus(id);
    frame(&mut tree);
    // A caret on a line the scroll brings into view.
    let line = "line 0 of a document that scrolls\n".chars().count();
    let caret = line * 5;
    handle.select_range(caret, caret);
    const SCROLL: f32 = 48.0;
    state.borrow().scroll_y.set(SCROLL);
    frame(&mut tree);
    tree.dispatch_event(WidgetEvent::ime_composition("ni".to_string(), None));
    frame(&mut tree);
    let rendered = tree.render();

    let start = handle.offset_rect(caret).expect("laid out");
    let end = handle.offset_rect(caret + 2).expect("laid out");
    let expected_y = start.y + start.height - 1.0;
    let underline = rendered
        .decorations
        .iter()
        .map(|d| d.rect)
        .find(|r| {
            (r[3] - 1.0).abs() < 0.01
                && (r[0] - start.x).abs() < 0.5
                && (r[0] + r[2] - end.x).abs() < 0.5
        })
        .unwrap_or_else(|| panic!("no underline spans the composition ({start:?} to {end:?})"));
    let drawn_y = underline[1] + underline[3] / 2.0;
    assert!(
        (drawn_y - expected_y).abs() < 0.5,
        "the underline is drawn at {drawn_y}, under the composition at {expected_y} \
         (scrolled {SCROLL})"
    );
}

// ── Undo after a composition ────────────────────────────────────────────────

/// The states a composition went through are never recorded: one undo takes
/// back what it committed, the next the typing before it.
#[test]
fn one_undo_takes_back_a_commit_and_none_its_states() {
    let mut e = Editor::new("", 0);
    e.type_keys("abc");
    e.compose_states(&PINYIN);
    assert_eq!(e.text(), "abcnihao");
    e.dispatch(WidgetEvent::ime_commit("\u{4F60}\u{597D}"));
    e.settle();
    assert_eq!(e.text(), "abc\u{4F60}\u{597D}");
    assert_eq!(
        e.arrivals(),
        vec![(EditSource::Keyboard, 3), (EditSource::Ime, 2)]
    );

    e.undo();
    assert_eq!(e.text(), "abc", "one undo takes back the commit");
    e.undo();
    assert_eq!(e.text(), "", "the next one the typing");
    assert!(!e.doc.can_undo(), "and no state of the composition is left");
    e.redo();
    assert_eq!(e.text(), "abc");
    e.redo();
    assert_eq!(e.text(), "abc\u{4F60}\u{597D}");
    assert!(!e.doc.can_redo());
}

/// The same with the empty update winit sends right before the commit.
#[test]
fn one_undo_takes_back_a_commit_after_its_empty_update() {
    let mut e = Editor::new("", 0);
    e.type_keys("abc");
    e.compose_states(&PINYIN);
    e.commit("\u{4F60}\u{597D}");
    e.settle();
    assert_eq!(e.text(), "abc\u{4F60}\u{597D}");
    assert_eq!(e.caret(), 5);

    e.undo();
    assert_eq!(e.text(), "abc");
    e.undo();
    assert_eq!(e.text(), "");
    e.redo();
    e.redo();
    assert_eq!(e.text(), "abc\u{4F60}\u{597D}");
}

/// A cancelled composition leaves the history as it found it: the redo stack
/// is still there, and nothing was pushed on the undo stack.
#[test]
fn a_cancelled_composition_leaves_the_history_alone() {
    let mut e = Editor::new("", 0);
    e.type_keys("abc");
    e.undo();
    assert_eq!(e.history(), (false, true));

    e.compose_states(&PINYIN);
    assert_eq!(e.history(), (false, true), "showing it records nothing");
    e.compose("", None);
    e.settle();
    assert_eq!(e.text(), "");
    assert_eq!(e.history(), (false, true), "nor does cancelling it");
    e.redo();
    assert_eq!(e.text(), "abc", "the redo stack survived it");
}

/// Typing on either side of a cancelled composition is one undo step, as if
/// the composition had never been.
#[test]
fn typing_around_a_cancelled_composition_is_one_step() {
    let mut e = Editor::new("", 0);
    e.type_keys("ab");
    e.compose_states(&["n", "ni"]);
    e.compose("", None);
    e.settle();
    e.type_keys("cd");
    assert_eq!(e.text(), "abcd");
    e.undo();
    assert_eq!(e.text(), "", "one undo takes back both runs of typing");
}

/// The Korean flow: a syllable composed and committed, then converted to
/// Hanja by a composition that takes its place. One undo gives the syllable
/// back, the next takes it away.
#[test]
fn undoing_a_hanja_conversion_gives_the_syllable_back() {
    let mut e = Editor::new("", 0);
    e.compose_states(&["\u{314E}", "\u{D558}", "\u{D55C}"]);
    e.commit("\u{D55C}");
    e.settle();
    assert_eq!(e.text(), "\u{D55C}");
    e.compose("\u{97D3}", Some(0..1));
    assert_eq!(e.text(), "\u{97D3}");
    e.commit("\u{97D3}");
    e.settle();
    assert_eq!(e.text(), "\u{97D3}");

    e.undo();
    assert_eq!(e.text(), "\u{D55C}", "one undo gives back the syllable");
    e.undo();
    assert_eq!(e.text(), "");
    e.redo();
    e.redo();
    assert_eq!(e.text(), "\u{97D3}");
}

/// A composition a click ends is one undo step: exactly the composed text.
#[test]
fn a_composition_a_click_ends_undoes_as_its_text() {
    let mut e = Editor::new("", 0);
    e.type_keys("abc");
    e.compose_states(&["n", "ni"]);
    e.click(1);
    assert_eq!(e.text(), "abcni");
    e.settle();
    e.undo();
    assert_eq!(
        e.text(),
        "abc",
        "one undo takes back exactly the composed text"
    );
    e.undo();
    assert_eq!(e.text(), "");
}

/// The same for a composition a focus change ends.
#[test]
fn a_composition_a_focus_change_ends_undoes_as_its_text() {
    let mut e = Editor::new("", 0);
    e.type_keys("abc");
    e.compose_states(&["n", "ni"]);
    let title = e.title;
    e.tree.focus_ops(title, &mut e.ops);
    assert_eq!(e.text(), "abcni");
    e.settle();
    e.undo();
    assert_eq!(e.text(), "abc");
}

/// Undo pressed while a composition is shown: the composition ends first, its
/// text kept, and the undo takes that text back, leaving the typing before
/// it. The editor no longer claims a composition.
#[test]
fn an_undo_during_a_composition_takes_it_away() {
    let mut e = Editor::new("", 0);
    e.type_keys("abc");
    e.compose_states(&["n", "ni"]);
    assert!(e.handle.is_composing());
    e.tree.press_key(Key::Z, Modifiers::COMMAND);
    assert_eq!(
        e.text(),
        "abc",
        "the kept composition is undone, not the typing"
    );
    assert!(!e.handle.is_composing(), "the editor agrees at once");
    assert_eq!(e.composition(), None);
    e.settle();
    assert!(!e.state.borrow().composition.is_held(&e.doc));

    // The input method's next update starts a composition of its own, and its
    // commit is one step.
    e.compose("nih", None);
    assert_eq!(e.text(), "abcnih");
    assert_eq!(e.composition(), Some(3..6));
    e.commit("\u{4F60}");
    e.settle();
    assert_eq!(e.text(), "abc\u{4F60}");
    e.undo();
    assert_eq!(e.text(), "abc");
    assert_eq!(e.history(), (true, true));
}

/// A commit takes the format typed text takes: the format of the text before
/// it. Bold toggled at a collapsed caret formats nothing, for typed text as
/// for a commit (the document has no typing format), so the bold comes from
/// the text before the caret.
#[test]
fn a_commit_takes_the_format_typed_text_takes() {
    use teksilo_text::text_document::TextFormat;
    let bold_at = |e: &Editor, at: usize| {
        let probe = e.doc.cursor();
        probe.set_position(at, MoveMode::MoveAnchor);
        probe.set_position(at + 1, MoveMode::KeepAnchor);
        let html = probe.selection().to_html();
        html.contains("<strong>") || html.contains("font-weight") || html.contains("<b>")
    };
    let mut e = Editor::new("ab", 2);
    {
        let st = e.state.borrow();
        st.cursor.set_position(0, MoveMode::MoveAnchor);
        st.cursor.set_position(2, MoveMode::KeepAnchor);
        st.cursor
            .merge_char_format(&TextFormat {
                font_bold: Some(true),
                ..Default::default()
            })
            .unwrap();
        st.cursor.set_position(2, MoveMode::MoveAnchor);
    }
    e.settle();
    e.compose_states(&["n", "ni"]);
    e.commit("\u{4F60}");
    e.settle();
    e.type_keys("x");
    assert_eq!(e.text(), "ab\u{4F60}x");
    assert!(bold_at(&e, 2), "the committed text is bold");
    assert!(bold_at(&e, 3), "as the typed text after it is");

    // Bold toggled at a collapsed caret, before plain text.
    let mut e = Editor::new("ab", 2);
    e.tree.press_key(Key::B, Modifiers::COMMAND);
    e.compose("ni", None);
    e.commit("\u{4F60}");
    e.settle();
    e.type_keys("x");
    assert_eq!(e.text(), "ab\u{4F60}x");
    assert_eq!(
        bold_at(&e, 2),
        bold_at(&e, 3),
        "the commit is formatted as the typed text is"
    );
}

/// A click on a control that takes no focus (a toolbar's Undo) ends the
/// composition first, its text kept, as a blur would: the command then acts
/// on that text. Left alive, the undo took the composition out of the text
/// while the input method still held it, and the next key brought it back.
#[test]
fn a_command_from_a_control_that_takes_no_focus_ends_the_composition_first() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let doc = TextDocument::new();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tool = tree.add(
        crate::button::Button::new(teksilo_i18n::lit!("Undo"))
            .on_activate_fn({
                let handle = handle.clone();
                move |_| handle.undo()
            })
            .focus_on_click(false),
    );
    let editor = tree.add(
        crate::primitives::FixedSize::new()
            .height(200.0)
            .child(editor),
    );
    tree.add(crate::primitives::VStack::new().child(tool).child(editor));
    tree.layout(viewport());
    let _ = tree.render();
    let field = tree.first_focusable_descendant(editor).unwrap();
    tree.focus(field);
    tree.type_text(field, "abc ");
    let mut ops = ImeOps::default();
    for _ in 0..3 {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(viewport());
    }
    tree.dispatch_event_with_ops(WidgetEvent::ime_composition("ni", None), &mut ops);
    assert_eq!(doc.to_plain_text().unwrap(), "abc ni");

    let b = tree.bounds(tool);
    let at = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
    tree.dispatch_event_with_ops(
        WidgetEvent::pointer_down(at, PointerButton::Primary, Modifiers::NONE),
        &mut ops,
    );
    tree.dispatch_event_with_ops(
        WidgetEvent::pointer_up(at, PointerButton::Primary, Modifiers::NONE),
        &mut ops,
    );
    assert_eq!(
        ops.finished, 1,
        "the platform is told the composition is over"
    );
    assert!(!handle.is_composing());
    assert_eq!(
        doc.to_plain_text().unwrap(),
        "abc ",
        "the undo took back the kept composition, not the typing before it"
    );
}

/// Undo pressed while a composition shows (the input method passes ⌘Z on)
/// ends the composition first, its text kept, then undoes that text: the
/// platform is told, so the input method's next update cannot bring the
/// composition back at the caret. A format shortcut ends it the same way.
#[test]
fn an_edit_or_format_command_during_a_composition_ends_it_first() {
    for (key, expected) in [(Key::Z, "abc "), (Key::B, "abc ni")] {
        let mut e = Editor::new("", 0);
        e.type_keys("abc ");
        e.compose("ni", None);
        e.dispatch(WidgetEvent::KeyDown {
            key,
            modifiers: Modifiers::COMMAND,
            text: None,
        });
        e.settle();
        assert_eq!(e.ops.finished, 1, "{key:?}: the platform is told");
        assert!(!e.handle.is_composing(), "{key:?}");
        assert_eq!(e.text(), expected, "{key:?}");
    }
}
