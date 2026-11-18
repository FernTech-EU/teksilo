// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A key, a command or a callback that comes in the same frame as typing acts
//! after that typing.
//!
//! Typed characters wait for the frame tick, which inserts them as one batch.
//! Every test here delivers the characters and what follows them with no
//! layout pass in between (the frame tick only runs inside one), so the
//! characters are still waiting when the key arrives: what a password
//! manager's auto-type, a bar-code reader or the automation bridge produce.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{Action, ActionData};
use teksilo_core::event::{ImeOrigin, Key, Modifiers, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_platform::clipboard::{ClipboardHandle, MemoryClipboard};
use teksilo_text::text_document::MoveMode;

use super::TextInputField;
use super::validator::ValidationOutcome;
use crate::EditSource;
use crate::primitives::{FixedSize, VStack};

/// What the callbacks under test saw, in the order they ran.
#[derive(Clone, Default)]
struct Log(Rc<RefCell<Vec<String>>>);

impl Log {
    fn push(&self, entry: String) {
        self.0.borrow_mut().push(entry);
    }

    fn seen(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 80.0;

struct Harness {
    tree: WidgetTree,
    root: WidgetId,
    /// The node that holds focus: the field, or the one inside a composite.
    field: WidgetId,
    clipboard: ClipboardHandle,
}

impl Harness {
    /// `widget` mounted with a clipboard, its first focusable node focused.
    fn mount(widget: impl Widget + 'static) -> Self {
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
        tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
        let field = tree.first_focusable_descendant(root).unwrap_or(root);
        tree.focus(field);
        let mut h = Self {
            tree,
            root,
            field,
            clipboard,
        };
        h.frame();
        h.frame();
        h
    }

    fn frame(&mut self) {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(16));
        self.tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
    }

    /// Typing, with no frame after it: the characters are still waiting.
    fn type_now(&mut self, text: &str) {
        self.tree.type_text(self.field, text);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.tree.press_key(key, modifiers);
    }

    fn state(&self) -> super::SharedState {
        self.tree
            .widget_as_any(self.field)
            .and_then(|w| w.downcast_ref::<TextInputField>())
            .map(|field| field.state().clone())
            .expect("a built TextInputField")
    }

    /// The document's text, after the frame that inserts anything still
    /// waiting.
    fn text_after_frame(&mut self) -> String {
        self.frame();
        self.frame();
        self.state()
            .borrow()
            .document
            .to_plain_text()
            .unwrap_or_default()
    }

    /// `(anchor, position)` of the caret.
    fn caret(&self) -> (usize, usize) {
        let state = self.state();
        let st = state.borrow();
        (st.cursor.anchor(), st.cursor.position())
    }

    /// Select `anchor..position` (a caret when they are equal).
    fn select(&mut self, anchor: usize, position: usize) {
        let state = self.state();
        {
            let st = state.borrow();
            st.cursor.set_position(anchor, MoveMode::MoveAnchor);
            st.cursor.set_position(position, MoveMode::KeepAnchor);
        }
        super::sync_cursor_signals(&state);
    }

    /// The last Tab stop of the tree: the node beside the field.
    fn last_stop(&self) -> WidgetId {
        *self
            .tree
            .tab_stops_within(self.root)
            .last()
            .expect("a Tab stop")
    }

    fn access(&mut self, action: Action, value: &str) {
        self.tree.dispatch_event(WidgetEvent::AccessAction {
            action,
            target: Some(self.field),
            target_node: teksilo_core::accessibility::widget_id_to_node_id(self.field),
            data: Some(ActionData::Value(value.into())),
        });
    }
}

/// A focused `TextInputField` holding `text`, made by `make`, with the caret
/// at its end.
fn field(
    text: &str,
    make: impl FnOnce(TextInputField, Signal<String>) -> TextInputField,
) -> Harness {
    let signal = Signal::new(text.to_string());
    let mut h = Harness::mount(make(TextInputField::new(signal.clone()), signal));
    let end = text.chars().count();
    h.select(end, end);
    h
}

/// A field whose `on_submit` writes what the bound signal holds into `log`.
fn submitting(text: &str, log: &Log) -> Harness {
    let log = log.clone();
    field(text, move |field, signal| {
        field.on_submit_fn(move |_| log.push(format!("submit {}", signal.get())))
    })
}

// ── Enter, and what the callbacks read ──────────────────────────────────────

#[test]
fn enter_submits_the_characters_typed_in_its_frame() {
    let log = Log::default();
    let mut h = submitting("", &log);
    h.type_now("ab");
    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(
        log.seen(),
        vec!["submit ab".to_string()],
        "on_submit reads the bound signal with both characters in it"
    );
    assert_eq!(h.text_after_frame(), "ab");
}

#[test]
fn enter_submits_after_a_backspace_in_its_frame() {
    // Not typing at all: the bound signal follows any edit a frame late.
    let log = Log::default();
    let mut h = submitting("abc", &log);
    h.press(Key::Backspace, Modifiers::NONE);
    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(log.seen(), vec!["submit ab".to_string()]);
}

#[test]
fn the_validator_on_enter_reads_the_characters_typed_in_its_frame() {
    let log = Log::default();
    let seen = log.clone();
    let mut h = field("", move |field, _| {
        field.validator(move |text| {
            seen.push(format!("validate {text}"));
            ValidationOutcome::Valid
        })
    });
    h.type_now("ab");
    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(log.seen(), vec!["validate ab".to_string()]);
}

#[test]
fn an_input_method_commit_then_enter_submits_it() {
    let log = Log::default();
    let mut h = submitting("", &log);
    h.tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "ab".to_string(),
        origin: ImeOrigin::Insertion,
        replacement: None,
    });
    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(log.seen(), vec!["submit ab".to_string()]);
}

#[test]
fn the_arrival_is_reported_once_and_before_the_submit() {
    let log = Log::default();
    let arrivals = log.clone();
    let submits = log.clone();
    let mut h = field("", move |field, signal| {
        field
            .on_text_inserted(move |source, chars| {
                arrivals.push(format!("inserted {source:?} {chars}"));
            })
            .on_submit_fn(move |_| submits.push(format!("submit {}", signal.get())))
    });
    h.type_now("ab");
    h.press(Key::Enter, Modifiers::NONE);
    h.text_after_frame();
    assert_eq!(
        log.seen(),
        vec![
            format!("inserted {:?} 2", EditSource::Keyboard),
            "submit ab".to_string()
        ],
        "the text arrived before Enter submitted it, and only once"
    );
}

#[test]
fn a_blur_commits_the_characters_typed_in_its_frame() {
    let log = Log::default();
    let commits = log.clone();
    let blurs = log.clone();
    let text = Signal::new(String::new());
    let commit_text = text.clone();
    let blur_text = text.clone();
    let mut h = Harness::mount(
        VStack::new()
            .child(
                TextInputField::new(text.clone())
                    .on_commit_fn(move |_| commits.push(format!("commit {}", commit_text.get())))
                    .on_blur_fn(move |_| blurs.push(format!("blur {}", blur_text.get()))),
            )
            .child(FixedSize::new().width(100.0).height(20.0).focusable(true)),
    );
    let other = h.last_stop();
    assert_ne!(other, h.field);
    h.type_now("ab");
    h.tree.focus(other);
    assert_eq!(
        log.seen(),
        vec!["commit ab".to_string(), "blur ab".to_string()]
    );
}

#[test]
fn a_command_that_takes_no_focus_sees_the_characters_typed_in_its_frame() {
    // A click on a toolbar button: the field commits for it while it keeps
    // focus, and the command runs after.
    let log = Log::default();
    let commits = log.clone();
    let taps = log.clone();
    let text = Signal::new(String::new());
    let commit_text = text.clone();
    let tap_text = text.clone();
    let mut h = Harness::mount(
        VStack::new()
            .child(
                TextInputField::new(text.clone())
                    .on_commit_fn(move |_| commits.push(format!("commit {}", commit_text.get()))),
            )
            .child(
                FixedSize::new()
                    .width(100.0)
                    .height(20.0)
                    .focusable(true)
                    .focus_on_click(false)
                    .on_tap(move |_, _| taps.push(format!("command {}", tap_text.get()))),
            ),
    );
    let button = h.last_stop();
    assert_ne!(button, h.field);
    h.type_now("ab");
    h.tree.click(button);
    assert_eq!(
        log.seen(),
        vec!["commit ab".to_string(), "command ab".to_string()]
    );
    assert_eq!(h.tree.focused(), Some(h.field), "the field kept focus");
}

// ── Keys that change the text or the caret ──────────────────────────────────

#[test]
fn backspace_removes_the_last_character_typed_in_its_frame() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::Backspace, Modifiers::NONE);
    assert_eq!(h.text_after_frame(), "xya");
    assert_eq!(h.caret(), (3, 3));
}

#[test]
fn delete_over_a_selection_acts_after_the_typing_that_replaced_it() {
    // "y" selected, "ab" typed over it, then Delete: the character after the
    // typing goes.
    let mut h = field("xyz", |field, _| field);
    h.select(1, 2);
    h.type_now("ab");
    h.press(Key::Delete, Modifiers::NONE);
    assert_eq!(h.text_after_frame(), "xab");
    assert_eq!(h.caret(), (3, 3));
}

#[test]
fn left_moves_back_from_the_end_of_the_typing() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.text_after_frame(), "xyab");
    assert_eq!(h.caret(), (3, 3));
}

#[test]
fn right_moves_on_from_the_end_of_the_typing() {
    let mut h = field("xy", |field, _| field);
    h.select(0, 0);
    h.type_now("ab");
    h.press(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.text_after_frame(), "abxy");
    assert_eq!(h.caret(), (3, 3));
}

#[test]
fn home_goes_to_the_start_after_the_typing_went_in() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::Home, Modifiers::NONE);
    assert_eq!(h.text_after_frame(), "xyab");
    assert_eq!(h.caret(), (0, 0));
}

#[test]
fn end_goes_to_the_end_after_the_typing_went_in() {
    let mut h = field("xy", |field, _| field);
    h.select(0, 0);
    h.type_now("ab");
    h.press(Key::End, Modifiers::NONE);
    assert_eq!(h.text_after_frame(), "abxy");
    assert_eq!(h.caret(), (4, 4));
}

#[test]
fn shift_left_selects_the_last_character_typed() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(h.text_after_frame(), "xyab");
    assert_eq!(h.caret(), (4, 3));
}

// ── The standard edit commands ──────────────────────────────────────────────

#[test]
fn select_all_selects_the_typing_too() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::A, Modifiers::COMMAND);
    assert_eq!(h.text_after_frame(), "xyab");
    assert_eq!(h.caret(), (0, 4));
}

#[test]
fn copy_after_select_all_copies_the_typing_too() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::A, Modifiers::COMMAND);
    h.press(Key::C, Modifiers::COMMAND);
    assert_eq!(h.clipboard.get_text().unwrap_or_default(), "xyab");
    assert_eq!(h.text_after_frame(), "xyab");
}

#[test]
fn cut_after_select_all_takes_the_typing_too() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::A, Modifiers::COMMAND);
    h.press(Key::X, Modifiers::COMMAND);
    assert_eq!(h.clipboard.get_text().unwrap_or_default(), "xyab");
    assert_eq!(h.text_after_frame(), "");
}

#[test]
fn paste_goes_in_after_the_typing() {
    let mut h = field("xy", |field, _| field);
    let _ = h.clipboard.set_text("P");
    h.type_now("ab");
    h.press(Key::V, Modifiers::COMMAND);
    assert_eq!(h.text_after_frame(), "xyabP");
}

#[test]
fn undo_takes_back_the_typing_of_its_frame() {
    let mut h = field("xy", |field, _| field);
    h.type_now("ab");
    h.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.text_after_frame(), "xy");
}

// ── An assistive technology's writes ────────────────────────────────────────

#[test]
fn an_assistive_insertion_goes_in_after_the_typing() {
    let mut h = field("", |field, _| field);
    h.type_now("ab");
    h.access(Action::ReplaceSelectedText, "Z");
    assert_eq!(h.text_after_frame(), "abZ");
}

#[test]
fn an_assistive_whole_value_write_replaces_the_typing_too() {
    let mut h = field("", |field, _| field);
    h.type_now("ab");
    h.access(Action::SetValue, "Q");
    assert_eq!(h.text_after_frame(), "Q");
}

// ── The handle ──────────────────────────────────────────────────────────────

#[test]
fn the_handle_reads_the_characters_typed_in_this_frame() {
    let handle = Rc::new(RefCell::new(None));
    let slot = handle.clone();
    let mut h = field("x", move |field, _| {
        *slot.borrow_mut() = Some(field.handle());
        field
    });
    let handle = handle.borrow().clone().expect("the handle");
    h.type_now("ab");
    assert_eq!(handle.text(), "xab");
}

// ── max_length ──────────────────────────────────────────────────────────────

#[test]
fn max_length_counts_the_characters_typed_in_its_frame() {
    let log = Log::default();
    let arrivals = log.clone();
    let mut h = field("x", move |field, _| {
        field.max_length(3).on_text_inserted(move |source, chars| {
            arrivals.push(format!("inserted {source:?} {chars}"));
        })
    });
    h.type_now("abcd");
    assert_eq!(h.text_after_frame(), "xab", "three characters, no more");
    assert_eq!(
        h.text_after_frame(),
        "xab",
        "and the refused ones do not come later"
    );
    assert_eq!(
        log.seen(),
        vec![format!("inserted {:?} 2", EditSource::Keyboard)]
    );
}

#[test]
fn max_length_counts_an_input_method_commit_beside_typing() {
    let mut h = field("", |field, _| field.max_length(3));
    h.type_now("ab");
    h.tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "cd".to_string(),
        origin: ImeOrigin::Insertion,
        replacement: None,
    });
    assert_eq!(h.text_after_frame(), "abc");
}

#[test]
fn max_length_counts_typing_over_a_selection() {
    // "yz" selected and replaced: the field can take two more than it holds.
    let mut h = field("xyz", |field, _| field.max_length(3));
    h.select(1, 3);
    h.type_now("abcd");
    assert_eq!(h.text_after_frame(), "xab");
}

// ── Typing alone still waits for the frame ──────────────────────────────────

#[test]
fn a_burst_of_typing_is_one_insertion_and_one_undo_step() {
    let log = Log::default();
    let arrivals = log.clone();
    let mut h = field("", move |field, _| {
        field.on_text_inserted(move |source, chars| {
            arrivals.push(format!("inserted {source:?} {chars}"));
        })
    });
    h.type_now("abc");
    assert_eq!(
        h.state().borrow().pending_chars,
        "abc",
        "typing waits for the frame, which inserts it as one batch"
    );
    assert_eq!(h.text_after_frame(), "abc");
    assert_eq!(
        log.seen(),
        vec![format!("inserted {:?} 3", EditSource::Keyboard)],
        "one report for the batch"
    );
    h.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(h.text_after_frame(), "", "one undo takes the whole burst");
}

// ── The fields built on it ──────────────────────────────────────────────────

#[test]
fn a_password_field_submits_the_characters_typed_in_its_frame() {
    let log = Log::default();
    let seen = log.clone();
    let password = Signal::new(String::new());
    let read = password.clone();
    let mut h = Harness::mount(
        crate::PasswordField::new(password)
            .on_submit_fn(move |_| seen.push(format!("submit {}", read.get()))),
    );
    h.type_now("hunter2");
    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(log.seen(), vec!["submit hunter2".to_string()]);
}

#[test]
fn a_search_field_submits_the_characters_typed_in_its_frame() {
    let log = Log::default();
    let seen = log.clone();
    let query = Signal::new(String::new());
    let read = query.clone();
    let mut h = Harness::mount(
        crate::SearchField::new(query)
            .on_submit_fn(move |_| seen.push(format!("submit {}", read.get()))),
    );
    h.type_now("rust");
    h.press(Key::Enter, Modifiers::NONE);
    assert_eq!(log.seen(), vec!["submit rust".to_string()]);
}
