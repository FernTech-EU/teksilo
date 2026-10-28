// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where the field's text came from, and what the input system may do to it.
//!
//! `on_text_inserted` reports each arrival by route: typed, committed by the
//! input system (and by which part of it), pasted, or written by an assistive
//! technology. The count is of what landed, after the field's own rules. An
//! input-system commit may name a range to replace, and `writing_tools` says
//! whether the system's Writing Tools may act on the field at all.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::{Action, ActionData};
use teksilo_core::event::{ImeOrigin, Key, Modifiers, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_platform::clipboard::{ClipboardHandle, MemoryClipboard};

use super::{EchoMode, TextFieldHandle, TextInputField};
use crate::EditSource;

/// Everything a field reports, for assertions.
#[derive(Clone, Default)]
struct Arrivals(Rc<RefCell<Vec<(EditSource, usize)>>>);

impl Arrivals {
    fn seen(&self) -> Vec<(EditSource, usize)> {
        self.0.borrow().clone()
    }

    fn recorder(&self) -> impl Fn(EditSource, usize) + 'static {
        let seen = self.0.clone();
        move |source, chars| seen.borrow_mut().push((source, chars))
    }
}

struct Field {
    tree: WidgetTree,
    /// The focusable `TextInputField`, the node events reach.
    field: WidgetId,
    text: Signal<String>,
    clipboard: ClipboardHandle,
    arrivals: Arrivals,
}

impl Field {
    /// A focused `TextInputField` holding `text`, made by `make`, reporting
    /// its arrivals.
    fn new(text: &str, make: impl FnOnce(TextInputField) -> TextInputField) -> Self {
        let signal = Signal::new(text.to_string());
        let arrivals = Arrivals::default();
        let field = make(TextInputField::new(signal.clone())).on_text_inserted(arrivals.recorder());
        Self::mount(field, signal, arrivals)
    }

    /// A focused composite built on the field, already reporting to
    /// `arrivals`.
    fn mount(widget: impl Widget + 'static, text: Signal<String>, arrivals: Arrivals) -> Self {
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
        tree.layout(SizeProposal::exact(300.0, 40.0));
        // A bare field is its own focusable node; a composite holds one.
        let field = tree.first_focusable_descendant(root).unwrap_or(root);
        tree.focus(field);
        let mut f = Self {
            tree,
            field,
            text,
            clipboard,
            arrivals,
        };
        f.frame();
        f
    }

    fn frame(&mut self) {
        self.tree.request_frame();
        self.tree
            .tick_animations(std::time::Duration::from_millis(16));
        self.tree.layout(SizeProposal::exact(300.0, 40.0));
    }

    fn settle(&mut self) {
        self.frame();
        self.frame();
    }

    fn type_text(&mut self, text: &str) {
        self.tree.type_text(self.field, text);
        self.settle();
    }

    fn commit(
        &mut self,
        text: &str,
        origin: ImeOrigin,
        replacement: Option<std::ops::Range<usize>>,
    ) {
        self.tree.dispatch_event(WidgetEvent::ImeCommit {
            text: text.to_string(),
            origin,
            replacement,
        });
        self.settle();
    }

    fn access(&mut self, action: Action, value: &str) {
        self.tree.dispatch_event(WidgetEvent::AccessAction {
            action,
            target: Some(self.field),
            target_node: teksilo_core::accessibility::widget_id_to_node_id(self.field),
            data: Some(ActionData::Value(value.into())),
        });
        self.settle();
    }

    /// Put the caret at `at`, with nothing selected.
    fn caret_at(&mut self, at: usize) {
        let state = self.state();
        {
            let st = state.borrow();
            st.cursor
                .set_position(at, teksilo_text::text_document::MoveMode::MoveAnchor);
        }
        super::sync_cursor_signals(&state);
    }

    fn state(&self) -> super::SharedState {
        self.tree
            .widget_as_any(self.field)
            .and_then(|w| w.downcast_ref::<TextInputField>())
            .map(|field| field.state().clone())
            .expect("a built TextInputField")
    }

    fn writing_tools_allowed(&self) -> bool {
        self.state().borrow().writing_tools_allowed()
    }
}

// ── Each route reports as itself ────────────────────────────────────────────

#[test]
fn typing_reports_keyboard() {
    let mut f = Field::new("", |field| field);
    f.type_text("abc");
    assert_eq!(f.text.get(), "abc");
    assert_eq!(
        f.arrivals.seen(),
        vec![(EditSource::Keyboard, 3)],
        "one report for the batch the frame inserted"
    );
}

#[test]
fn a_composition_commit_reports_ime() {
    let mut f = Field::new("", |field| field);
    f.tree
        .dispatch_event(WidgetEvent::ime_composition("ni".to_string(), None));
    f.settle();
    assert!(f.arrivals.seen().is_empty(), "a preedit is not an arrival");
    f.tree.dispatch_event(WidgetEvent::ime_commit("\u{4F60}"));
    f.settle();
    assert_eq!(f.text.get(), "\u{4F60}");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Ime, 1)]);
}

#[test]
fn an_insertion_reports_ime() {
    let mut f = Field::new("ab", |field| field);
    f.caret_at(1);
    f.commit("\u{1F600}", ImeOrigin::Insertion, None);
    assert_eq!(f.text.get(), "a\u{1F600}b", "lands at the caret");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Ime, 1)]);
}

#[test]
fn dictation_reports_dictation() {
    let mut f = Field::new("", |field| field);
    f.commit("hello", ImeOrigin::Dictation, None);
    assert_eq!(f.text.get(), "hello");
    assert_eq!(
        f.arrivals.seen(),
        vec![(EditSource::Dictation, 5)],
        "dictated text is neither typed nor composed"
    );
}

#[test]
fn writing_tools_report_writing_tools() {
    let mut f = Field::new("a cat", |field| field);
    f.commit("the", ImeOrigin::WritingTools, Some(0..1));
    assert_eq!(f.text.get(), "the cat");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::WritingTools, 3)]);
}

#[test]
fn a_paste_reports_clipboard() {
    let mut f = Field::new("", |field| field);
    f.clipboard.set_text("pasted").unwrap();
    f.tree.press_key(Key::V, Modifiers::COMMAND);
    f.settle();
    assert_eq!(f.text.get(), "pasted");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Clipboard, 6)]);
}

#[test]
fn an_assistive_technology_reports_accessibility() {
    let mut f = Field::new("hi", |field| field);
    f.caret_at(2);
    f.access(Action::ReplaceSelectedText, "!");
    assert_eq!(f.text.get(), "hi!");
    f.access(Action::SetValue, "four");
    assert_eq!(f.text.get(), "four");
    assert_eq!(
        f.arrivals.seen(),
        vec![
            (EditSource::Accessibility, 1),
            (EditSource::Accessibility, 4)
        ]
    );
}

/// The application writing the bound signal is not text arriving.
#[test]
fn the_application_writing_the_signal_reports_nothing() {
    let mut f = Field::new("", |field| field);
    f.text.set("loaded from a file".to_string());
    f.settle();
    assert!(f.arrivals.seen().is_empty(), "{:?}", f.arrivals.seen());
}

// ── What lands is what is counted ───────────────────────────────────────────

#[test]
fn max_length_reports_what_landed() {
    let mut f = Field::new("abc", |field| field.max_length(5));
    f.caret_at(3);
    f.commit("defgh", ImeOrigin::Dictation, None);
    assert_eq!(f.text.get(), "abcde");
    assert_eq!(
        f.arrivals.seen(),
        vec![(EditSource::Dictation, 2)],
        "three of the five dictated characters never reached the field"
    );
}

#[test]
fn max_length_counts_a_typed_run_after_truncation() {
    let mut f = Field::new("abc", |field| field.max_length(4));
    f.caret_at(3);
    f.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Character('x'),
        modifiers: Modifiers::NONE,
        text: Some("xyz".to_string()),
    });
    f.settle();
    assert_eq!(f.text.get(), "abcx");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Keyboard, 1)]);
}

/// A single-line field strips what typing would strip: a dictated line break
/// does not split the field, nor end up in it.
#[test]
fn a_single_line_field_strips_a_dictated_line_break() {
    let mut f = Field::new("", |field| field);
    f.commit("a\nb", ImeOrigin::Dictation, None);
    assert_eq!(f.text.get(), "ab");
    let mut g = Field::new("xy", |field| field);
    g.commit("a\nb", ImeOrigin::Dictation, Some(0..2));
    assert_eq!(g.text.get(), "ab", "a replacement follows the same rule");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Dictation, 2)]);
    assert_eq!(g.arrivals.seen(), vec![(EditSource::Dictation, 2)]);
}

// ── A replacement ───────────────────────────────────────────────────────────

#[test]
fn a_replacement_is_one_undo_step() {
    let signal = Signal::new("the quick fox".to_string());
    let arrivals = Arrivals::default();
    let field = TextInputField::new(signal.clone()).on_text_inserted(arrivals.recorder());
    let handle: TextFieldHandle = field.handle();
    let mut f = Field::mount(field, signal, arrivals);
    f.caret_at(13);

    f.commit("slow", ImeOrigin::WritingTools, Some(4..9));
    assert_eq!(
        f.text.get(),
        "the slow fox",
        "exactly the range is replaced"
    );
    assert_eq!(f.arrivals.seen(), vec![(EditSource::WritingTools, 4)]);

    handle.undo();
    f.settle();
    assert_eq!(
        f.text.get(),
        "the quick fox",
        "one undo restores the user's own words whole"
    );
}

#[test]
fn a_replacement_past_the_end_is_clamped() {
    let mut f = Field::new("abc", |field| field);
    f.commit("Z", ImeOrigin::Dictation, Some(2..99));
    assert_eq!(f.text.get(), "abZ");
    f.commit("!", ImeOrigin::Dictation, Some(40..50));
    assert_eq!(f.text.get(), "abZ!");
    assert_eq!(
        f.arrivals.seen(),
        vec![(EditSource::Dictation, 1), (EditSource::Dictation, 1)]
    );
}

#[test]
fn a_replacement_under_max_length_counts_the_range_as_gone() {
    let mut f = Field::new("abcde", |field| field.max_length(5));
    f.commit("XYZW", ImeOrigin::WritingTools, Some(1..4));
    assert_eq!(
        f.text.get(),
        "aXYZe",
        "room for the three replaced, not four"
    );
    assert_eq!(f.arrivals.seen(), vec![(EditSource::WritingTools, 3)]);
}

#[test]
fn a_replacement_the_filter_refuses_leaves_the_field_alone() {
    let mut f = Field::new("12", |field| field.char_filter(|c| c.is_ascii_digit()));
    f.commit("twelve", ImeOrigin::WritingTools, Some(0..2));
    assert_eq!(f.text.get(), "12");
    assert!(f.arrivals.seen().is_empty(), "{:?}", f.arrivals.seen());
}

#[test]
fn a_read_only_field_refuses_the_input_system() {
    let mut f = Field::new("keep", |field| field.read_only(true));
    f.commit("x", ImeOrigin::Dictation, None);
    f.commit("rewritten", ImeOrigin::WritingTools, Some(0..4));
    assert_eq!(f.text.get(), "keep");
    assert!(f.arrivals.seen().is_empty(), "{:?}", f.arrivals.seen());
}

// ── Writing Tools ───────────────────────────────────────────────────────────

#[test]
fn writing_tools_are_allowed_by_default() {
    let f = Field::new("", |field| field);
    assert!(f.writing_tools_allowed());
}

#[test]
fn writing_tools_can_be_turned_off() {
    let f = Field::new("", |field| field.writing_tools(false));
    assert!(!f.writing_tools_allowed());

    let switch = Signal::new(true);
    let g = Field::new("", |field| field.writing_tools(switch.clone()));
    assert!(g.writing_tools_allowed());
    switch.set(false);
    assert!(
        !g.writing_tools_allowed(),
        "a bound signal is read as the question is asked"
    );
}

#[test]
fn a_secure_field_never_lets_writing_tools_in() {
    let f = Field::new("", |field| {
        field.secure(EchoMode::Masked).writing_tools(true)
    });
    assert!(!f.writing_tools_allowed());
}

// ── The composites forward them ─────────────────────────────────────────────

#[test]
fn text_input_forwards_its_arrivals() {
    let text = Signal::new(String::new());
    let arrivals = Arrivals::default();
    let input = crate::TextInput::new(text.clone()).on_text_inserted(arrivals.recorder());
    let mut f = Field::mount(input, text, arrivals);
    f.type_text("ab");
    assert_eq!(f.text.get(), "ab");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Keyboard, 2)]);
}

#[test]
fn search_field_forwards_its_arrivals() {
    let text = Signal::new(String::new());
    let arrivals = Arrivals::default();
    let search = crate::SearchField::new(text.clone()).on_text_inserted(arrivals.recorder());
    let mut f = Field::mount(search, text, arrivals);
    f.type_text("q");
    assert_eq!(f.arrivals.seen(), vec![(EditSource::Keyboard, 1)]);
}

#[test]
fn password_field_forwards_its_arrivals() {
    let text = Signal::new(String::new());
    let arrivals = Arrivals::default();
    let password = crate::PasswordField::new(text.clone()).on_text_inserted(arrivals.recorder());
    let mut f = Field::mount(password, text, arrivals);
    f.type_text("pw");
    assert_eq!(
        f.arrivals.seen(),
        vec![(EditSource::Keyboard, 2)],
        "counted like any field, and only counted"
    );
}

#[test]
fn text_input_and_search_field_forward_writing_tools() {
    let text = Signal::new(String::new());
    let input = crate::TextInput::new(text.clone()).writing_tools(false);
    let f = Field::mount(input, text, Arrivals::default());
    assert!(!f.writing_tools_allowed());

    let text = Signal::new(String::new());
    let search = crate::SearchField::new(text.clone()).writing_tools(false);
    let g = Field::mount(search, text, Arrivals::default());
    assert!(!g.writing_tools_allowed());
}

#[test]
fn a_password_field_keeps_writing_tools_out() {
    let text = Signal::new(String::new());
    let password = crate::PasswordField::new(text.clone());
    let f = Field::mount(password, text, Arrivals::default());
    assert!(
        !f.writing_tools_allowed(),
        "a secure field, whatever is set"
    );
}
