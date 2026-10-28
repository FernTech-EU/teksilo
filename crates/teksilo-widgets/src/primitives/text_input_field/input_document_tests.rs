// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's document as the platform's text input system reads it, for
//! the field and the widgets built on it: the selection, the text (none from
//! a secure field), the composition, where a range is drawn, the offset under
//! a point, and the UTF-16 offsets the platform counts in.
//!
//! Geometry is checked against the caret area the field reports to the
//! platform's IME as the caret moves (`last_ime_area`), never against fixed
//! pixel values: the private engine shapes with real font metrics.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{ImeOrigin, Key, Modifiers, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::text_input_document::TextInputDocument;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::MoveMode;

use super::{EchoMode, SharedState, TextFieldHandle, TextInputField};
use crate::EditSource;

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 40.0;

struct Field {
    tree: WidgetTree,
    /// The focusable `TextInputField`.
    field: WidgetId,
    text: Signal<String>,
    arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
}

impl Field {
    /// A focused `TextInputField` holding `text`, made by `make`.
    fn new(text: &str, make: impl FnOnce(TextInputField) -> TextInputField) -> Self {
        let signal = Signal::new(text.to_string());
        let arrivals = Rc::new(RefCell::new(Vec::new()));
        let seen = arrivals.clone();
        let field = make(TextInputField::new(signal.clone()))
            .on_text_inserted(move |source, chars| seen.borrow_mut().push((source, chars)));
        Self::mount(field, signal, arrivals)
    }

    /// A focused widget built on the field.
    fn mount(
        widget: impl Widget + 'static,
        text: Signal<String>,
        arrivals: Rc<RefCell<Vec<(EditSource, usize)>>>,
    ) -> Self {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let root = tree.add(widget);
        tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
        let field = tree.first_focusable_descendant(root).unwrap_or(root);
        tree.focus(field);
        let mut f = Self {
            tree,
            field,
            text,
            arrivals,
        };
        f.settle();
        f
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.tree.request_frame();
            self.tree
                .tick_animations(std::time::Duration::from_millis(16));
            self.tree.layout(SizeProposal::exact(WIDTH, HEIGHT));
        }
        let _ = self.tree.render();
    }

    fn state(&self) -> SharedState {
        self.tree
            .widget_as_any(self.field)
            .and_then(|w| w.downcast_ref::<TextInputField>())
            .map(|field| field.state().clone())
            .expect("a built TextInputField")
    }

    fn handle(&self) -> TextFieldHandle {
        self.tree
            .widget_as_any(self.field)
            .and_then(|w| w.downcast_ref::<TextInputField>())
            .map(TextInputField::handle)
            .expect("a built TextInputField")
    }

    /// Ask the focused widget's document, through the window's registry, the
    /// way the platform bridge does.
    fn ask<R>(&self, f: impl FnOnce(&dyn TextInputDocument) -> R) -> R {
        self.tree
            .text_surfaces()
            .with_focused_input_document(f)
            .expect("the focused field describes its document")
    }

    fn select(&mut self, anchor: usize, caret: usize) {
        let state = self.state();
        {
            let st = state.borrow();
            st.cursor.set_position(anchor, MoveMode::MoveAnchor);
            st.cursor.set_position(caret, MoveMode::KeepAnchor);
        }
        super::sync_cursor_signals(&state);
    }

    /// The caret area the field reports to the platform's IME after `key`
    /// moves the caret: an oracle for the caret rectangle that does not go
    /// through the code under test.
    fn reported_caret_after(&mut self, key: Key) -> Rect {
        self.tree.press_key(key, Modifiers::NONE);
        self.settle();
        self.state()
            .borrow()
            .last_ime_area
            .expect("a focused field reports its caret area")
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

// ── Reached through the window's registry ───────────────────────────────────

#[test]
fn the_focused_field_answers_through_the_window_registry() {
    let mut f = Field::new("hello world", |field| field);
    f.select(7, 2);
    let answers = f.ask(|d| {
        (
            d.char_count(),
            d.selection(),
            d.caret(),
            d.text_in(2..7),
            d.is_editable(),
            d.is_secure(),
            d.writing_tools_allowed(),
        )
    });
    assert_eq!(
        answers,
        (
            Some(11),
            Some(2..7),
            Some(2),
            Some("llo w".to_string()),
            true,
            false,
            true
        )
    );
}

#[test]
fn text_input_search_field_and_password_field_answer_through_the_field() {
    let text = Signal::new("hello".to_string());
    let input = Field::mount(crate::TextInput::new(text.clone()), text, Rc::default());
    assert_eq!(input.ask(|d| d.text_in(0..5)).as_deref(), Some("hello"));

    let text = Signal::new("query".to_string());
    let search = Field::mount(crate::SearchField::new(text.clone()), text, Rc::default());
    assert_eq!(search.ask(|d| d.text_in(0..5)).as_deref(), Some("query"));

    let text = Signal::new("secret".to_string());
    let password = Field::mount(crate::PasswordField::new(text.clone()), text, Rc::default());
    assert_eq!(
        password.ask(|d| (d.is_secure(), d.text_in(0..6), d.char_count())),
        (true, None, Some(6))
    );
}

// ── The text and its UTF-16 offsets ─────────────────────────────────────────

#[test]
fn text_is_clamped_and_an_emoji_is_one_character_and_two_units() {
    let f = Field::new("a\u{1F600}b", |field| field);
    f.ask(|d| {
        assert_eq!(d.char_count(), Some(3));
        assert_eq!(d.text_in(1..2).as_deref(), Some("\u{1F600}"));
        assert_eq!(d.text_in(2..99).as_deref(), Some("b"));
        assert_eq!(d.char_to_utf16(2), Some(3));
        assert_eq!(d.char_to_utf16(99), Some(4), "clamped at the end");
        assert_eq!(d.utf16_to_char(3), Some(2));
        assert_eq!(d.utf16_to_char(2), Some(1), "inside the pair: the emoji");
        assert_eq!(d.utf16_range_to_chars(1..3), Some(1..2));
        assert_eq!(d.char_range_to_utf16(0..3), Some(0..4));
    });
}

/// The application writing the bound signal loads the text whole, which the
/// document reports as a reset rather than an edit. The index follows it.
#[test]
fn the_index_follows_the_application_writing_the_signal() {
    let mut f = Field::new("ab", |field| field);
    let handle = f.handle();
    let before = handle.utf16_index().expect("an index");
    assert!(
        Rc::ptr_eq(&before, &handle.utf16_index().expect("an index")),
        "kept while nothing changes"
    );

    f.text.set("\u{1F600}\u{1F600}".to_string());
    f.settle();
    let after = handle.utf16_index().expect("an index");
    assert!(!Rc::ptr_eq(&before, &after));
    assert_eq!((after.char_count(), after.utf16_len()), (2, 4));
}

// ── Typing not yet inserted ─────────────────────────────────────────────────

#[test]
fn typing_not_yet_inserted_is_answered_as_inserted() {
    let mut f = Field::new("ab", |field| field);
    f.select(2, 2);
    f.tree.type_text(f.field, "cd");
    assert_eq!(
        f.state().borrow().pending_chars,
        "cd",
        "premise: the keys wait for the frame tick"
    );

    assert_eq!(
        f.ask(|d| (d.caret(), d.text_in(0..4))),
        (Some(4), Some("abcd".to_string()))
    );
    assert!(f.state().borrow().pending_chars.is_empty());

    f.settle();
    assert_eq!(f.text.get(), "abcd", "inserted once");
    assert_eq!(*f.arrivals.borrow(), vec![(EditSource::Keyboard, 2)]);
}

#[test]
fn a_composition_in_progress_is_its_range() {
    let mut f = Field::new("ab", |field| field);
    f.select(1, 1);
    assert_eq!(f.ask(|d| d.composition_range()), None);
    f.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: None,
    });
    assert_eq!(f.ask(|d| d.composition_range()), Some(1..3));
    f.tree.dispatch_event(WidgetEvent::ImeCommit {
        text: "\u{4F60}".to_string(),
        origin: ImeOrigin::Composition,
        replacement: None,
    });
    f.settle();
    assert_eq!(f.ask(|d| d.composition_range()), None);
    assert_eq!(f.text.get(), "a\u{4F60}b");
}

/// Each candidate tells the input method where the composition is: at the
/// caret after the preedit, with the preedit laid out. Read as the event is
/// handled, before any frame runs, because that is when the platform places
/// its candidate window.
///
/// The text was laid out again only at the next frame, so the area was read
/// off the text without the preedit, as many characters further into it: in
/// the middle of a field, off by the difference between the preedit's width
/// and the width of the characters after it.
#[test]
fn each_candidate_reports_the_caret_after_the_preedit() {
    let mut f = Field::new("hello world", |field| field);
    f.select(5, 5);
    for preedit in ["n", "ni", "nih", "niha", "nihao"] {
        // Each key of the syllable is a new candidate: measure its own report.
        f.state().borrow_mut().last_ime_area = None;
        f.tree.dispatch_event(WidgetEvent::ImeComposition {
            text: preedit.to_string(),
            cursor: None,
        });
        let reported = f
            .state()
            .borrow()
            .last_ime_area
            .expect("a composition reports where it is");
        f.settle();
        let end = 5 + preedit.chars().count();
        let caret = f
            .ask(|d| d.first_line_rect(end..end))
            .expect("laid out")
            .rect;
        assert!(
            close(reported.x, caret.x) && close(reported.y, caret.y),
            "{preedit:?}: reported {reported:?}, the caret after it is at {caret:?}"
        );
    }
    assert_eq!(f.ask(|d| d.composition_range()), Some(5..10));
}

/// A composition that starts in the frame some keys were typed in goes after
/// them: the typing waits for the frame, and used to be inserted then, at the
/// caret, which the preedit had already moved past.
#[test]
fn a_composition_goes_after_the_typing_of_its_frame() {
    let mut f = Field::new("ab", |field| field);
    f.select(2, 2);
    f.tree.type_text(f.field, "cd");
    assert_eq!(
        f.state().borrow().pending_chars,
        "cd",
        "premise: the keys wait for the frame tick"
    );
    f.tree.dispatch_event(WidgetEvent::ImeComposition {
        text: "ni".to_string(),
        cursor: None,
    });
    f.settle();
    assert_eq!(f.text.get(), "abcdni");
    assert_eq!(f.ask(|d| d.composition_range()), Some(4..6));
}

/// At the end of a field that overflows, a candidate scrolls the text so the
/// caret stays in view, and the area reported is where the caret is drawn
/// once scrolled, not where it would be without the scroll: past the field's
/// trailing edge.
#[test]
fn a_candidate_that_scrolls_the_field_reports_the_scrolled_caret() {
    let text = "the quick brown fox jumps over the lazy dog, again and again";
    let mut f = Field::new(text, |field| field);
    let end = text.chars().count();
    f.select(end, end);
    f.settle();
    assert!(f.state().borrow().scroll_x > 0.0, "the text overflows");
    for preedit in ["n", "ni", "nih", "niha", "nihao"] {
        f.state().borrow_mut().last_ime_area = None;
        f.tree.dispatch_event(WidgetEvent::ImeComposition {
            text: preedit.to_string(),
            cursor: None,
        });
        let reported = f
            .state()
            .borrow()
            .last_ime_area
            .expect("a composition reports where it is");
        f.settle();
        let after = end + preedit.chars().count();
        let caret = f
            .ask(|d| d.first_line_rect(after..after))
            .expect("laid out")
            .rect;
        assert!(
            close(reported.x, caret.x) && close(reported.y, caret.y),
            "{preedit:?}: reported {reported:?}, the caret after it is drawn at {caret:?}"
        );
        assert!(reported.x < WIDTH, "{preedit:?}: reported past the field");
    }
}

// ── What the input system may do ────────────────────────────────────────────

#[test]
fn a_read_only_field_is_not_editable() {
    let f = Field::new("fixed", |field| field.read_only(true));
    assert!(!f.ask(|d| d.is_editable()));
}

#[test]
fn a_secure_field_gives_no_text_and_no_writing_tools() {
    let mut f = Field::new("hunter2", |field| {
        field.secure(EchoMode::Masked).writing_tools(true)
    });
    f.select(0, 3);
    f.ask(|d| {
        assert!(d.is_secure());
        assert_eq!(d.text_in(0..7), None);
        assert!(!d.writing_tools_allowed());
        assert_eq!(d.selection(), Some(0..3), "where to insert is still told");
        let line = d.first_line_rect(0..3).expect("masked text has geometry");
        assert!(line.rect.width > 1.0, "three bullets have width");
    });
}

/// A field that echoes nothing must not tell its length through geometry.
#[test]
fn a_no_echo_field_answers_every_range_with_the_same_rect() {
    let f = Field::new("hunter2", |field| field.secure(EchoMode::NoEcho));
    f.ask(|d| {
        let short = d.first_line_rect(0..1).expect("laid out");
        let long = d.first_line_rect(0..7).expect("laid out");
        assert_eq!(short.rect, long.rect);
        assert_eq!(long.covered, 0..7, "the whole range, in one step");
        assert_eq!(d.first_line_rect(5..5).map(|l| l.rect), Some(long.rect));
        let inside = d.visible_text_rect().expect("laid out");
        assert_eq!(
            d.offset_at_point(Point::new(inside.x + inside.width / 2.0, inside.y + 5.0)),
            None
        );
    });
}

// ── Geometry ────────────────────────────────────────────────────────────────

#[test]
fn ranges_are_drawn_between_the_carets_the_field_reports() {
    let mut f = Field::new("hello world", |field| field);
    let start = f.reported_caret_after(Key::Home);
    let end = f.reported_caret_after(Key::End);

    f.ask(|d| {
        let at_start = d.first_line_rect(0..0).expect("laid out");
        assert_eq!(at_start.covered, 0..0);
        assert!(close(at_start.rect.x, start.x) && close(at_start.rect.y, start.y));
        assert!(close(at_start.rect.height, start.height));

        let whole = d.first_line_rect(0..11).expect("laid out");
        assert_eq!(whole.covered, 0..11, "one line holds every range");
        assert!(close(whole.rect.x, start.x), "{whole:?} from {start:?}");
        assert!(close(whole.rect.right(), end.x), "{whole:?} to {end:?}");
        assert!(close(whole.rect.y, start.y));
        assert!(close(whole.rect.height, start.height));

        let part = d.first_line_rect(2..5).expect("laid out");
        assert_eq!(part.covered, 2..5);
        assert!(part.rect.x > whole.rect.x && part.rect.right() < whole.rect.right());
        assert_eq!(
            d.first_line_rect(4..99).map(|l| l.covered),
            Some(4..11),
            "clamped at the end"
        );
    });
}

#[test]
fn the_offset_at_a_point_inside_beside_and_outside_the_text() {
    let f = Field::new("hello", |field| field);
    f.ask(|d| {
        let visible = d.visible_text_rect().expect("laid out");
        let caret = d.first_line_rect(3..3).expect("laid out").rect;
        let mid = caret.y + caret.height / 2.0;
        assert_eq!(d.offset_at_point(Point::new(caret.x + 1.0, mid)), Some(3));
        assert_eq!(
            d.offset_at_point(Point::new(visible.x + 0.5, mid)),
            Some(0),
            "at the left edge, before the text"
        );
        assert_eq!(
            d.offset_at_point(Point::new(visible.right() - 2.0, mid)),
            Some(5),
            "past the end of the text"
        );
        assert_eq!(
            d.offset_at_point(Point::new(caret.x, visible.bottom() + 5.0)),
            None,
            "below the field"
        );
        assert!(visible.contains(Point::new(caret.x, mid)));
    });
}

// ── Busy, and not built ─────────────────────────────────────────────────────

#[test]
fn a_busy_field_answers_nothing_instead_of_panicking() {
    let f = Field::new("hello", |field| field);
    let handle = f.handle();
    let d: &dyn TextInputDocument = &handle;
    let state = f.state();
    let _held = state.borrow_mut();
    assert_eq!(d.char_count(), None);
    assert_eq!(d.text_in(0..2), None);
    assert_eq!(d.selection(), None);
    assert_eq!(d.caret(), None);
    assert_eq!(d.composition_range(), None);
    assert_eq!(d.first_line_rect(0..2), None);
    assert_eq!(d.offset_at_point(Point::new(5.0, 5.0)), None);
    assert_eq!(d.visible_text_rect(), None);
    assert_eq!(d.utf16_index(), None);
    assert!(!d.is_editable());
    assert!(d.is_secure(), "the cautious answer");
    assert!(!d.writing_tools_allowed());
}

#[test]
fn a_field_not_yet_built_answers_nothing() {
    let handle = TextInputField::new(Signal::new("hello".to_string())).handle();
    let d: &dyn TextInputDocument = &handle;
    assert_eq!(d.char_count(), None);
    assert_eq!(d.selection(), None);
    assert!(!d.is_editable());
}
