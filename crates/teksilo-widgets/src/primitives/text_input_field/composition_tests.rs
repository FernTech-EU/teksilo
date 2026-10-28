// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An input method's composition that takes the place of text already in the
//! field (Korean Hanja conversion), and a composition ended by a press or a
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
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::MoveMode;

use super::state::SharedState;
use super::{TextFieldHandle, TextInputField};
use crate::EditSource;
use crate::button::press_test_support::{finger, touch};
use crate::common::ime_ops_test::ImeOps;

fn viewport() -> SizeProposal {
    SizeProposal::exact(400.0, 200.0)
}

struct Field {
    tree: WidgetTree,
    state: SharedState,
    handle: TextFieldHandle,
    text: Signal<String>,
    /// A second field, where focus goes next.
    title: WidgetId,
    title_text: Signal<String>,
    arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
    ops: ImeOps,
}

impl Field {
    /// A focused, laid-out field holding `text`, its caret at `caret`, above a
    /// second field.
    fn new(text: &str, caret: usize) -> Self {
        let signal = Signal::new(text.to_string());
        let arrivals = Rc::new(RefCell::new(Vec::new()));
        let seen = arrivals.clone();
        let field = TextInputField::new(signal.clone())
            .on_text_inserted(move |source, chars| seen.borrow_mut().push((source, chars)));
        let handle = field.handle();
        let state_slot = field.state_slot.clone();
        let title_text = Signal::new(String::new());
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let padded = tree.add(crate::primitives::Padding::symmetric(20.0, 20.0).child(field));
        let other = tree.add(
            crate::primitives::FixedSize::new()
                .width(300.0)
                .child(TextInputField::new(title_text.clone())),
        );
        tree.add(crate::primitives::VStack::new().child(padded).child(other));
        tree.layout(viewport());
        let _ = tree.render();
        let state = state_slot.borrow().clone().expect("the field was built");
        let field = tree
            .first_focusable_descendant(padded)
            .expect("the field is focusable");
        let title = tree
            .first_focusable_descendant(other)
            .expect("the title field is focusable");
        tree.focus(field);
        let mut f = Self {
            tree,
            state,
            handle,
            text: signal,
            title,
            title_text,
            arrivals,
            ops: ImeOps::default(),
        };
        f.settle();
        {
            let st = f.state.borrow();
            st.cursor.set_position(caret, MoveMode::MoveAnchor);
        }
        super::sync_cursor_signals(&f.state);
        f
    }

    fn settle(&mut self) {
        for _ in 0..2 {
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
        let st = self.state.borrow();
        let caret = st.engine.caret_rect(offset, CursorAffinity::Downstream);
        Point::new(
            st.viewport_origin.x + caret[0] - st.scroll_x + 0.5,
            st.viewport_origin.y + st.text_offset_y + caret[1] + caret[3] / 2.0,
        )
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

    fn doc_text(&self) -> String {
        self.state
            .borrow()
            .document
            .to_plain_text()
            .unwrap_or_default()
    }

    fn caret(&self) -> usize {
        self.state.borrow().cursor.position()
    }

    fn composition(&self) -> Option<std::ops::Range<usize>> {
        self.handle.composition_range()
    }

    fn arrivals(&self) -> Vec<(EditSource, usize)> {
        self.arrivals.borrow().clone()
    }

    /// Type `text` as keys, and let the frame insert it.
    fn type_keys(&mut self, text: &str) {
        let field = self.tree.focused().expect("the field is focused");
        self.tree.type_text(field, text);
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
        let st = self.state.borrow();
        (st.document.can_undo(), st.document.can_redo())
    }
}

const PINYIN: [&str; 5] = ["n", "ni", "nih", "niha", "nihao"];

// ── A composition that takes the place of text ──────────────────────────────

/// The Korean Hanja conversion: the input method committed 한, then marked
/// 韓 over it. The field shows 韓 in its place, not after it.
#[test]
fn a_composition_takes_the_place_of_the_text_it_marks() {
    let mut f = Field::new("X\u{D55C}Y", 2);
    f.compose("\u{97D3}", Some(1..2));
    assert_eq!(f.doc_text(), "X\u{97D3}Y", "the marked text is replaced");
    assert_eq!(f.composition(), Some(1..2));

    f.commit("\u{97D3}");
    f.settle();
    assert_eq!(f.text.get(), "X\u{97D3}Y", "the commit takes the place");
    assert_eq!(f.caret(), 2);
    assert_eq!(f.composition(), None);
    assert_eq!(f.arrivals(), vec![(EditSource::Ime, 1)]);

    f.handle.undo();
    f.settle();
    assert_eq!(
        f.text.get(),
        "X\u{D55C}Y",
        "one undo puts back the syllable that was converted"
    );
}

#[test]
fn a_cancelled_replacement_puts_the_text_back() {
    let mut f = Field::new("X\u{D55C}Y", 2);
    f.compose("\u{97D3}", Some(1..2));
    assert_eq!(f.doc_text(), "X\u{97D3}Y");
    // The empty update, and no commit after it.
    f.compose("", None);
    assert_eq!(
        f.doc_text(),
        "X\u{97D3}Y",
        "the empty update alone decides nothing: a commit may follow"
    );
    f.settle();
    assert_eq!(
        f.text.get(),
        "X\u{D55C}Y",
        "the frame after it settles a cancel"
    );
    assert_eq!(f.caret(), 2, "the caret after the text put back");
    assert!(f.arrivals().is_empty(), "{:?}", f.arrivals());
}

/// A cancel is also settled by the next key, before the key acts: the caret
/// the key moves is the one after the text put back.
#[test]
fn a_key_after_the_empty_update_settles_a_cancel_first() {
    let mut f = Field::new("X\u{D55C}Y", 2);
    f.compose("\u{97D3}", Some(1..2));
    f.compose("", None);
    f.tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    f.settle();
    assert_eq!(f.text.get(), "X\u{D55C}Y");
    assert_eq!(f.caret(), 1, "the key moved the caret from after the text");
}

/// The title field's case: the input method marked four characters over
/// characters 10..14, then the user moved through candidates. Nothing is
/// duplicated.
#[test]
fn a_composition_replaces_several_characters() {
    let mut f = Field::new("Chapter 1 \u{B098}\u{D55C}\u{AE00}\u{D55C} end", 14);
    f.compose("\u{B098}\u{D55C}\u{AE00}\u{D55C}", Some(10..14));
    assert_eq!(
        f.doc_text(),
        "Chapter 1 \u{B098}\u{D55C}\u{AE00}\u{D55C} end"
    );
    assert_eq!(f.composition(), Some(10..14));
    // A later update names the same text: it replaces the composition only.
    f.compose("\u{7F85}\u{97D3}\u{AE00}\u{97D3}", Some(10..14));
    assert_eq!(
        f.doc_text(),
        "Chapter 1 \u{7F85}\u{97D3}\u{AE00}\u{97D3} end"
    );
    f.commit("\u{7F85}\u{97D3}\u{AE00}\u{97D3}");
    f.settle();
    assert_eq!(
        f.text.get(),
        "Chapter 1 \u{7F85}\u{97D3}\u{AE00}\u{97D3} end"
    );
    assert_eq!(f.arrivals(), vec![(EditSource::Ime, 4)]);
}

#[test]
fn a_replacement_under_max_length_counts_the_marked_text_as_gone() {
    let mut f = Field::new("ab\u{D55C}", 3);
    // A field already full: the composition still has the marked text's room.
    f.state.borrow_mut().max_length = Some(3);
    f.compose("\u{97D3}", Some(2..3));
    assert_eq!(f.doc_text(), "ab\u{97D3}");
    f.commit("\u{97D3}");
    f.settle();
    assert_eq!(f.text.get(), "ab\u{97D3}");
}

// ── A press or a focus change ends a composition ────────────────────────────

#[test]
fn a_click_elsewhere_keeps_the_composition_and_ends_it() {
    let mut f = Field::new("hello world", 5);
    f.compose("ni", None);
    assert_eq!(f.doc_text(), "helloni world");

    f.click(1);
    assert_eq!(
        f.doc_text(),
        "helloni world",
        "the composition's text stays"
    );
    assert_eq!(f.caret(), 1, "and the caret goes to the click");
    assert_eq!(f.composition(), None, "it is no longer a composition");
    assert_eq!(f.arrivals(), vec![(EditSource::Ime, 2)]);
    assert_eq!(f.ops.finished, 1, "the platform is told once");

    // What the platform may still deliver: an empty update and an empty commit.
    f.commit("");
    f.settle();
    assert_eq!(
        f.text.get(),
        "helloni world",
        "a late empty update is inert"
    );
    assert_eq!(f.caret(), 1);
    assert_eq!(f.ops.finished, 1);

    // As a commit would be, the kept text is one undo step.
    f.handle.undo();
    f.settle();
    assert_eq!(f.text.get(), "hello world");
}

#[test]
fn a_tap_elsewhere_keeps_the_composition_and_ends_it() {
    let mut f = Field::new("hello world", 5);
    f.compose("ni", None);
    f.finger_tap(1);
    assert_eq!(f.doc_text(), "helloni world");
    assert_eq!(f.caret(), 1);
    assert_eq!(f.ops.finished, 1);
}

#[test]
fn a_click_during_a_replacement_keeps_the_replacement() {
    let mut f = Field::new("X\u{D55C}Y", 2);
    f.compose("\u{97D3}", Some(1..2));
    f.click(0);
    assert_eq!(f.doc_text(), "X\u{97D3}Y");
    assert_eq!(f.caret(), 0);
    assert_eq!(f.ops.finished, 1);
    f.commit("");
    f.settle();
    assert_eq!(
        f.text.get(),
        "X\u{97D3}Y",
        "nothing put back after the finish"
    );
    f.handle.undo();
    f.settle();
    assert_eq!(
        f.text.get(),
        "X\u{D55C}Y",
        "one undo puts the marked text back"
    );
}

#[test]
fn a_focus_change_keeps_the_composition_and_ends_it() {
    let mut f = Field::new("hello", 5);
    f.compose("ni", None);
    let title = f.title;
    f.tree.focus_ops(title, &mut f.ops);
    assert_eq!(f.doc_text(), "helloni");
    assert_eq!(f.ops.finished, 1);
    assert_eq!(f.arrivals(), vec![(EditSource::Ime, 2)]);

    // The platform's late empty update and empty commit reach the field
    // focused now, and change nothing there either.
    f.commit("");
    f.settle();
    assert_eq!(f.text.get(), "helloni");
    assert_eq!(f.title_text.get(), "");
}

#[test]
fn a_press_with_no_composition_ends_nothing() {
    let mut f = Field::new("hello world", 5);
    f.click(1);
    assert_eq!(f.caret(), 1);
    let title = f.title;
    f.tree.focus_ops(title, &mut f.ops);
    assert_eq!(f.ops.finished, 0);
    assert!(f.arrivals().is_empty());
}

// ── Typing just before a press ──────────────────────────────────────────────

/// The Microsoft Korean IME commits its syllable on the mouse-down itself: an
/// empty update and the commit reach the field before the press. The syllable
/// goes in where it was composed, and the press then moves the caret.
#[test]
fn a_commit_just_before_a_click_lands_where_it_was_composed() {
    let mut f = Field::new("Showcase exercises", 8);
    f.compose("\u{D55C}", None);
    f.settle();
    let target = f.at(2);
    f.commit("\u{D55C}");
    f.dispatch(WidgetEvent::pointer_down(
        target,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert_eq!(f.doc_text(), "Showcase\u{D55C} exercises");
    assert_eq!(f.caret(), 2);
    f.settle();
    assert_eq!(f.text.get(), "Showcase\u{D55C} exercises");
    assert_eq!(f.arrivals(), vec![(EditSource::Ime, 1)]);
}

#[test]
fn typing_just_before_a_click_lands_where_it_was_typed() {
    let mut f = Field::new("Showcase exercises", 8);
    let target = f.at(2);
    let field = f.tree.focused().unwrap();
    f.tree.type_text(field, "ab");
    f.dispatch(WidgetEvent::pointer_down(
        target,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    f.settle();
    assert_eq!(f.text.get(), "Showcaseab exercises");
    assert_eq!(f.caret(), 2);
}

#[test]
fn a_commit_just_before_a_tap_lands_where_it_was_composed() {
    let mut f = Field::new("Showcase exercises", 8);
    f.compose("\u{D55C}", None);
    f.settle();
    let target = f.at(2);
    f.commit("\u{D55C}");
    let id = finger();
    f.tree
        .dispatch_pointer_with_ops(touch(id, PointerPhase::Down, target, 0), &mut f.ops);
    f.tree
        .dispatch_pointer_with_ops(touch(id, PointerPhase::Up, target, 30), &mut f.ops);
    f.settle();
    assert_eq!(f.text.get(), "Showcase\u{D55C} exercises");
    assert_eq!(f.caret(), 2);
}

/// A select-all the application makes in the same frame selects the text
/// with the typing in it, rather than having the typing replace it.
#[test]
fn typing_just_before_a_programmatic_select_all_is_kept() {
    let mut f = Field::new("Showcase", 8);
    let field = f.tree.focused().unwrap();
    f.tree.type_text(field, "ab");
    f.handle.select_all();
    f.settle();
    assert_eq!(f.text.get(), "Showcaseab");
    assert!(f.handle.has_selection());
}

/// A screen reader's selection in the same frame: the typing goes in where it
/// was typed, and the reader's positions, read in the text it was shown, move
/// with it.
#[test]
fn typing_just_before_an_assistive_selection_lands_where_it_was_typed() {
    use teksilo_core::accesskit::{Action, ActionData, TextPosition, TextSelection};
    let mut f = Field::new("Showcase exercises", 8);
    let field = f.tree.focused().unwrap();
    f.tree.type_text(field, "ab");
    let node = teksilo_core::accessibility::widget_id_to_node_id(field);
    let at = |character_index| TextPosition {
        node,
        character_index,
    };
    let handled = f.tree.dispatch_access_action(
        node,
        Action::SetTextSelection,
        Some(ActionData::SetTextSelection(TextSelection {
            anchor: at(9),
            focus: at(18),
        })),
        &mut f.ops,
    );
    assert!(handled);
    f.settle();
    assert_eq!(f.text.get(), "Showcaseab exercises");
    let st = f.state.borrow();
    assert_eq!(
        st.cursor.selected_text().unwrap_or_default(),
        "exercises",
        "the range moved with the text typed before it"
    );
}

/// What KWin sends when a click ends a Hangul syllable in a `TextInput`: it
/// commits the syllable itself before the button (an empty update, then the
/// commit), and sends another empty update between the press and the
/// release. The syllable stays where it was composed and the next key goes
/// to the click.
#[test]
fn a_click_that_kwin_ends_a_syllable_with_puts_the_next_key_at_the_click() {
    let text = Signal::new("aaaaaaaaaa".to_string());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let root = tree.add(crate::text_input::TextInput::new(text.clone()));
    let size = SizeProposal::exact(300.0, 40.0);
    let frame = |tree: &mut WidgetTree| {
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        tree.layout(size);
        let _ = tree.render();
    };
    frame(&mut tree);
    let field = tree.first_focusable_descendant(root).unwrap();
    tree.focus(field);
    frame(&mut tree);
    let state = tree
        .widget_as_any(field)
        .and_then(|w| w.downcast_ref::<TextInputField>())
        .map(|f| f.state().clone())
        .expect("a built TextInputField");
    {
        let st = state.borrow();
        st.cursor.set_position(10, MoveMode::MoveAnchor);
    }
    let mut ops = ImeOps::default();
    for preedit in ["\u{314E}", "\u{D558}", "\u{D55C}"] {
        tree.dispatch_event_with_ops(WidgetEvent::ime_composition(preedit, Some(3..3)), &mut ops);
        frame(&mut tree);
    }
    let at = {
        let st = state.borrow();
        let caret = st.engine.caret_rect(3, CursorAffinity::Downstream);
        Point::new(
            st.viewport_origin.x + caret[0] - st.scroll_x + 0.5,
            st.viewport_origin.y + st.text_offset_y + caret[1] + caret[3] / 2.0,
        )
    };
    tree.dispatch_event_with_ops(WidgetEvent::ime_composition("", None), &mut ops);
    tree.dispatch_event_with_ops(WidgetEvent::ime_commit("\u{D55C}"), &mut ops);
    tree.dispatch_event_with_ops(
        WidgetEvent::pointer_down(at, PointerButton::Primary, Modifiers::NONE),
        &mut ops,
    );
    tree.dispatch_event_with_ops(WidgetEvent::ime_composition("", Some(0..0)), &mut ops);
    tree.dispatch_event_with_ops(
        WidgetEvent::pointer_up(at, PointerButton::Primary, Modifiers::NONE),
        &mut ops,
    );
    frame(&mut tree);
    tree.type_text(field, "Y");
    frame(&mut tree);
    frame(&mut tree);
    assert_eq!(text.get(), "aaaYaaaaaaa\u{D55C}");
    assert_eq!(ops.finished, 0, "the composition was already over");
}

// ── Undo after a composition ────────────────────────────────────────────────

/// The states a composition went through are never recorded: one undo takes
/// back what it committed, the next the typing before it.
#[test]
fn one_undo_takes_back_a_commit_and_none_its_states() {
    let mut f = Field::new("", 0);
    f.type_keys("abc");
    f.compose_states(&PINYIN);
    assert_eq!(f.doc_text(), "abcnihao");
    f.dispatch(WidgetEvent::ime_commit("\u{4F60}\u{597D}"));
    f.settle();
    assert_eq!(f.text.get(), "abc\u{4F60}\u{597D}");
    assert_eq!(
        f.arrivals(),
        vec![(EditSource::Keyboard, 3), (EditSource::Ime, 2)]
    );

    f.undo();
    assert_eq!(f.text.get(), "abc", "one undo takes back the commit");
    f.undo();
    assert_eq!(f.text.get(), "", "the next one the typing");
    assert_eq!(
        f.history(),
        (false, true),
        "and no state of the composition is left"
    );
    f.redo();
    assert_eq!(f.text.get(), "abc");
    f.redo();
    assert_eq!(f.text.get(), "abc\u{4F60}\u{597D}");
    assert_eq!(f.history(), (true, false));
}

/// The same with the empty update winit sends right before the commit.
#[test]
fn one_undo_takes_back_a_commit_after_its_empty_update() {
    let mut f = Field::new("", 0);
    f.type_keys("abc");
    f.compose_states(&PINYIN);
    f.commit("\u{4F60}\u{597D}");
    f.settle();
    assert_eq!(f.text.get(), "abc\u{4F60}\u{597D}");
    assert_eq!(f.caret(), 5);

    f.undo();
    assert_eq!(f.text.get(), "abc");
    f.undo();
    assert_eq!(f.text.get(), "");
    f.redo();
    f.redo();
    assert_eq!(f.text.get(), "abc\u{4F60}\u{597D}");
}

/// A cancelled composition leaves the history as it found it: the redo stack
/// is still there, and nothing was pushed on the undo stack.
#[test]
fn a_cancelled_composition_leaves_the_history_alone() {
    let mut f = Field::new("", 0);
    f.type_keys("abc");
    f.undo();
    assert_eq!(f.history(), (false, true));

    f.compose_states(&PINYIN);
    f.settle();
    assert_eq!(
        f.text.get(),
        "nihao",
        "the bound text shows the composition"
    );
    assert_eq!(f.history(), (false, true), "showing it records nothing");
    f.compose("", None);
    f.settle();
    assert_eq!(f.text.get(), "");
    assert_eq!(f.history(), (false, true), "nor does cancelling it");
    f.redo();
    assert_eq!(f.text.get(), "abc", "the redo stack survived it");
}

/// Typing on either side of a cancelled composition is one undo step, as if
/// the composition had never been.
#[test]
fn typing_around_a_cancelled_composition_is_one_step() {
    let mut f = Field::new("", 0);
    f.type_keys("ab");
    f.compose_states(&["n", "ni"]);
    f.compose("", None);
    f.settle();
    f.type_keys("cd");
    assert_eq!(f.text.get(), "abcd");
    f.undo();
    assert_eq!(f.text.get(), "", "one undo takes back both runs of typing");
}

/// The Korean flow: a syllable composed and committed, then converted to
/// Hanja by a composition that takes its place. One undo gives the syllable
/// back, the next takes it away.
#[test]
fn undoing_a_hanja_conversion_gives_the_syllable_back() {
    let mut f = Field::new("", 0);
    f.compose_states(&["\u{314E}", "\u{D558}", "\u{D55C}"]);
    f.commit("\u{D55C}");
    f.settle();
    assert_eq!(f.text.get(), "\u{D55C}");
    f.compose("\u{97D3}", Some(0..1));
    assert_eq!(f.doc_text(), "\u{97D3}");
    f.commit("\u{97D3}");
    f.settle();
    assert_eq!(f.text.get(), "\u{97D3}");

    f.undo();
    assert_eq!(f.text.get(), "\u{D55C}", "one undo gives back the syllable");
    f.undo();
    assert_eq!(f.text.get(), "");
}

/// A composition a click ends is one undo step: exactly the composed text.
#[test]
fn a_composition_a_click_ends_undoes_as_its_text() {
    let mut f = Field::new("", 0);
    f.type_keys("abc");
    f.compose_states(&["n", "ni"]);
    f.click(1);
    assert_eq!(f.doc_text(), "abcni");
    f.settle();
    f.undo();
    assert_eq!(
        f.text.get(),
        "abc",
        "one undo takes back exactly the composed text"
    );
    f.undo();
    assert_eq!(f.text.get(), "");
}

/// The same for a composition a focus change ends.
#[test]
fn a_composition_a_focus_change_ends_undoes_as_its_text() {
    let mut f = Field::new("", 0);
    f.type_keys("abc");
    f.compose_states(&["n", "ni"]);
    let title = f.title;
    f.tree.focus_ops(title, &mut f.ops);
    assert_eq!(f.doc_text(), "abcni");
    f.settle();
    f.undo();
    assert_eq!(f.text.get(), "abc");
}

/// Undo pressed while a composition is shown: the composition ends first, its
/// text kept, and the undo takes that text back, leaving the typing before
/// it. The field no longer claims a composition.
#[test]
fn an_undo_during_a_composition_takes_it_away() {
    let mut f = Field::new("", 0);
    f.type_keys("abc");
    f.compose_states(&["n", "ni"]);
    assert_eq!(f.composition(), Some(3..5));
    f.tree.press_key(Key::Z, Modifiers::COMMAND);
    assert_eq!(
        f.doc_text(),
        "abc",
        "the kept composition is undone, not the typing"
    );
    assert_eq!(f.composition(), None, "the field agrees at once");
    f.settle();
    assert_eq!(f.text.get(), "abc");
    {
        let st = f.state.borrow();
        assert!(!st.composition.is_held(&st.document));
    }

    f.compose("nih", None);
    assert_eq!(f.doc_text(), "abcnih");
    assert_eq!(f.composition(), Some(3..6));
    f.commit("\u{4F60}");
    f.settle();
    assert_eq!(f.text.get(), "abc\u{4F60}");
    f.undo();
    assert_eq!(f.text.get(), "abc");
}

/// The application writing the bound text during a composition reverts it,
/// and the field no longer claims it: the input method's commit then goes in
/// as text with no composition before it.
#[test]
fn the_application_writing_the_text_ends_a_composition() {
    let mut f = Field::new("abc", 3);
    f.compose_states(&["n", "ni"]);
    f.text.set("xyz".to_string());
    assert_eq!(f.doc_text(), "xyz");
    assert_eq!(f.composition(), None);
    f.commit("\u{4F60}");
    f.settle();
    assert!(
        f.text.get().contains('\u{4F60}') && f.text.get().contains("xyz"),
        "{:?}",
        f.text.get()
    );
}

/// The commit of a composition obeys the field's rules: the filter and the
/// length cap, counted without the composition.
#[test]
fn a_composition_commit_obeys_the_filter_and_the_length_cap() {
    let mut f = Field::new("ab", 2);
    f.state.borrow_mut().max_length = Some(4);
    f.compose_states(&["n", "ni"]);
    assert_eq!(f.doc_text(), "abni");
    f.commit("\u{4F60}\u{597D}\u{5417}");
    f.settle();
    assert_eq!(
        f.text.get(),
        "ab\u{4F60}\u{597D}",
        "capped at the field's length"
    );
    assert_eq!(f.arrivals(), vec![(EditSource::Ime, 2)]);

    let mut f = Field::new("", 0);
    f.state.borrow_mut().char_filter = Some(Rc::new(|c: char| c.is_ascii_digit()));
    f.compose("1a2", None);
    assert_eq!(
        f.doc_text(),
        "12",
        "the composition is filtered as it is shown"
    );
    f.commit("4\n2x");
    f.settle();
    assert_eq!(f.text.get(), "42", "and the commit as it lands");
    f.undo();
    assert_eq!(f.text.get(), "");
}

/// A click on a control that takes no focus (a toolbar button) ends the
/// composition first, its text kept, as a blur would, so the command it runs
/// sees the text the field shows and the input method lets the composition go.
#[test]
fn a_command_from_a_control_that_takes_no_focus_ends_the_composition_first() {
    use teksilo_core::widget_builder::WidgetBuilder;
    let text = Signal::new(String::new());
    let field = TextInputField::new(text.clone());
    let handle = field.handle();
    let seen = Rc::new(RefCell::new(None::<String>));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tool = tree.add(
        crate::button::Button::new(teksilo_i18n::lit!("Search"))
            .on_activate_fn({
                let (handle, seen) = (handle.clone(), seen.clone());
                move |_| *seen.borrow_mut() = Some(handle.text())
            })
            .focus_on_click(false),
    );
    let field = tree.add(
        crate::primitives::FixedSize::new()
            .width(300.0)
            .child(field),
    );
    tree.add(crate::primitives::VStack::new().child(tool).child(field));
    tree.layout(viewport());
    let _ = tree.render();
    let field = tree.first_focusable_descendant(field).unwrap();
    tree.focus(field);
    let mut ops = ImeOps::default();
    tree.dispatch_event_with_ops(WidgetEvent::ime_composition("ni", None), &mut ops);

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
    assert_eq!(
        seen.borrow().as_deref(),
        Some("ni"),
        "the command sees the kept text"
    );
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(viewport());
    assert_eq!(
        text.get(),
        "ni",
        "the bound text holds it at the next frame"
    );
}

/// Undo pressed while a composition shows ends the composition first, its
/// text kept, then undoes that text, and the platform is told.
#[test]
fn undo_during_a_composition_ends_it_first() {
    let mut f = Field::new("", 0);
    f.compose("ni", None);
    f.dispatch(WidgetEvent::KeyDown {
        key: Key::Z,
        modifiers: Modifiers::COMMAND,
        text: None,
    });
    f.settle();
    assert_eq!(f.ops.finished, 1, "the platform is told");
    assert_eq!(f.text.get(), "", "the undo takes the kept composition back");
}
