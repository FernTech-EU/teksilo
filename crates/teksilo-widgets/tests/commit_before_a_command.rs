// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A click on a control that takes no focus (a toolbar button, a menu bar
//! title) blurs nothing, so before its command runs the focused field commits
//! what was typed in it, through its own commit: a spin box's parse, a date
//! field's. The value reaches the bound signal and the change callback fires
//! once; the field keeps focus; the real blur that follows commits nothing
//! again, so the change callback does not fire a second time.
//!
//! Pinned for every widget whose commit goes through `TextInputField`'s
//! commit hook, since each wires its own.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::PointerKind;
use teksilo_widgets::button::Button;
use teksilo_widgets::common::datetime::{Date, Time};
use teksilo_widgets::primitives::HStack;
use teksilo_widgets::{
    DateEdit, DateRange, DateRangeEdit, DateTimeEdit, HexColorInput, SpinBox, TextInput, TimeEdit,
};

fn tick(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(16));
    tree.layout(SizeProposal::exact(800.0, 60.0));
}

/// Mount `editor` after a button that takes no focus from a click (`tool`)
/// and one that does (`sink`); type `text` over what its field holds, click
/// `tool`, then move focus to `sink`. `changes` counts the editor's change
/// callback; `committed` says whether its value is the one typed. Each step
/// is checked as it happens.
fn type_click_then_blur(
    name: &str,
    editor: impl Widget + 'static,
    text: &str,
    changes: &Cell<u32>,
    committed: impl Fn() -> bool,
) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tool = tree.add(
        Button::new(lit!("tool"))
            .on_activate_fn(|_| {})
            .focus_on_click(false),
    );
    let sink = tree.add(Button::new(lit!("sink")).on_activate_fn(|_| {}));
    let editor = tree.add(editor);
    tree.add(HStack::new().child(tool).child(sink).child(editor));
    tick(&mut tree);
    let field = tree.first_focusable_descendant(editor).unwrap();
    let sink = tree.first_focusable_descendant(sink).unwrap();
    let b = tree.bounds(tool);
    let at = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);

    tree.focus(field);
    tree.press_key(Key::A, Modifiers::COMMAND);
    tree.type_text(field, text);
    tick(&mut tree);
    assert!(!committed(), "{name}: nothing is committed while typing");

    tree.tap_with(PointerKind::Mouse, at);
    tick(&mut tree);
    assert_eq!(
        tree.focused(),
        Some(field),
        "{name}: the click takes no focus"
    );
    assert!(committed(), "{name}: the click commits what was typed");
    assert_eq!(changes.get(), 1, "{name}: the change is reported once");

    tree.focus(sink);
    tick(&mut tree);
    assert!(committed(), "{name}: the blur keeps the value");
    assert_eq!(changes.get(), 1, "{name}: and reports no second change");
}

fn counter() -> (Rc<Cell<u32>>, Rc<Cell<u32>>) {
    let changes = Rc::new(Cell::new(0));
    (changes.clone(), changes)
}

#[test]
fn a_spin_box_commits_before_the_command() {
    let value = Signal::new(10_i32);
    let (changes, counted) = counter();
    let spin = SpinBox::new(value.clone(), 0, 59)
        .on_value_changed(move |_, _| counted.set(counted.get() + 1));
    type_click_then_blur("SpinBox", spin, "35", &changes, || value.get() == 35);
}

#[test]
fn a_date_edit_commits_before_the_command() {
    let value = Signal::new(Some(Date::new(2026, 1, 1).unwrap()));
    let (changes, counted) = counter();
    let edit = DateEdit::new(value.clone())
        .format_pattern("%Y-%m-%d")
        .show_calendar_button(false)
        .on_value_changed(move |_, _| counted.set(counted.get() + 1));
    let typed = Date::new(2026, 3, 15).unwrap();
    type_click_then_blur("DateEdit", edit, "2026-03-15", &changes, || {
        value.get() == Some(typed)
    });
}

#[test]
fn a_time_edit_commits_before_the_command() {
    let value = Signal::new(Some(Time::new(9, 0, 0, 0).unwrap()));
    let (changes, counted) = counter();
    let edit = TimeEdit::new(value.clone())
        .format_pattern("%H:%M")
        .on_value_changed(move |_, _| counted.set(counted.get() + 1));
    let typed = Time::new(14, 30, 0, 0).unwrap();
    type_click_then_blur("TimeEdit", edit, "14:30", &changes, || {
        value.get() == Some(typed)
    });
}

#[test]
fn a_hex_color_input_commits_before_the_command() {
    let value = Signal::new(teksilo_tokens::Color::from_hex("#000000"));
    let (changes, counted) = counter();
    let input = HexColorInput::new(value.clone())
        .on_value_changed(move |_, _| counted.set(counted.get() + 1));
    type_click_then_blur("HexColorInput", input, "#336699", &changes, || {
        value.get() == teksilo_tokens::Color::from_hex("#336699")
    });
}

/// The date half of a `DateTimeEdit`, the field focus lands on first.
#[test]
fn a_date_time_edit_commits_before_the_command() {
    let start = Date::new(2026, 1, 1).unwrap().at(9, 0, 0, 0);
    let value = Signal::new(Some(start));
    let (changes, counted) = counter();
    let edit = DateTimeEdit::new(value.clone())
        .date_format_pattern("%Y-%m-%d")
        .show_calendar_button(false)
        .on_value_changed(move |_, _| counted.set(counted.get() + 1));
    let typed = Date::new(2026, 3, 15).unwrap().at(9, 0, 0, 0);
    type_click_then_blur("DateTimeEdit", edit, "2026-03-15", &changes, || {
        value.get() == Some(typed)
    });
}

/// The start half of a `DateRangeEdit`, the field focus lands on first.
#[test]
fn a_date_range_edit_commits_before_the_command() {
    let range = |start: Date| DateRange {
        start,
        end: Date::new(2026, 12, 31).unwrap(),
    };
    let value = Signal::new(Some(range(Date::new(2026, 1, 1).unwrap())));
    let (changes, counted) = counter();
    let edit = DateRangeEdit::new(value.clone())
        .format_pattern("%Y-%m-%d")
        .on_value_changed(move |_, _| counted.set(counted.get() + 1));
    let typed = range(Date::new(2026, 3, 15).unwrap());
    type_click_then_blur("DateRangeEdit", edit, "2026-03-15", &changes, || {
        value.get() == Some(typed)
    });
}

/// The guard: with no click, nothing is reported before the blur, so the
/// tests above are not passing on a commit the typing made by itself.
#[test]
fn without_the_click_the_blur_commits() {
    let value = Signal::new(10_i32);
    let (changes, counted) = counter();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let sink = tree.add(Button::new(lit!("sink")).on_activate_fn(|_| {}));
    let spin = tree.add(
        SpinBox::new(value.clone(), 0, 59)
            .on_value_changed(move |_, _| counted.set(counted.get() + 1)),
    );
    let _: WidgetId = tree.add(HStack::new().child(sink).child(spin));
    tick(&mut tree);
    let field = tree.first_focusable_descendant(spin).unwrap();
    tree.focus(field);
    tree.press_key(Key::A, Modifiers::COMMAND);
    tree.type_text(field, "35");
    tick(&mut tree);
    assert_eq!((value.get(), changes.get()), (10, 0));
    tree.focus(tree.first_focusable_descendant(sink).unwrap());
    tick(&mut tree);
    assert_eq!((value.get(), changes.get()), (35, 1));
}

/// An app's own `on_commit_fn` on a plain `TextInput`: it runs before the
/// command of a control that takes no focus, and once on the blur that
/// follows, not twice.
#[test]
fn an_apps_on_commit_fn_runs_before_the_command_and_once() {
    let log = Rc::new(std::cell::RefCell::new(Vec::<&'static str>::new()));
    let text = Signal::new(String::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tool = tree.add(
        Button::new(lit!("tool"))
            .on_activate_fn({
                let log = log.clone();
                move |_| log.borrow_mut().push("command")
            })
            .focus_on_click(false),
    );
    let sink = tree.add(Button::new(lit!("sink")).on_activate_fn(|_| {}));
    let input = tree.add(TextInput::new(text.clone()).on_commit_fn({
        let log = log.clone();
        move |_| log.borrow_mut().push("commit")
    }));
    tree.add(HStack::new().child(tool).child(sink).child(input));
    tick(&mut tree);
    let field = tree.first_focusable_descendant(input).unwrap();
    let sink = tree.first_focusable_descendant(sink).unwrap();
    let b = tree.bounds(tool);
    let at = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);

    tree.focus(field);
    tree.type_text(field, "hello");
    tick(&mut tree);
    assert!(log.borrow().is_empty());

    tree.tap_with(PointerKind::Mouse, at);
    tick(&mut tree);
    assert_eq!(*log.borrow(), ["commit", "command"]);

    tree.focus(sink);
    tick(&mut tree);
    assert_eq!(*log.borrow(), ["commit", "command"], "the blur adds none");
}
