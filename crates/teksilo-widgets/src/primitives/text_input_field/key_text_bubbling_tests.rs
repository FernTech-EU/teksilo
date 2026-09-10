// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
use super::*;
use std::cell::Cell;
use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::WidgetBuilder;
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;

/// Dispatch one `KeyDown` to a focused field that sits *inside* a widget
/// carrying an `on_key`, and report whether that outer handler saw it.
///
/// The nesting is the point. Hanging the handler on the field itself puts
/// it on the same node the click focuses, above the field's own handler
/// rather than behind it, and the bubble under test never happens — which
/// is exactly how an earlier version of this test passed on the bug.
fn outer_handler_sees(key: Key, text: Option<&str>, field: TextInputField) -> bool {
    let seen = Rc::new(Cell::new(false));
    let seen_for_handler = seen.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let outer = tree.add(
        crate::primitives::VStack::new()
            .child(field)
            .on_key(move |_ev, _ctx| {
                seen_for_handler.set(true);
                EventResponse::Handled
            }),
    );
    tree.layout(SizeProposal::exact(200.0, 40.0));

    let b = tree.bounds(outer);
    let centre = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        centre,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        centre,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    let focused = tree.focused().expect("the click focused something");
    assert_ne!(
        focused, outer,
        "focus must land on the field, or nothing below the outer handler is being tested"
    );

    tree.dispatch_event(WidgetEvent::KeyDown {
        key,
        modifiers: Modifiers::NONE,
        text: text.map(str::to_string),
    });
    seen.get()
}

/// The bug: winit gives Escape `text: Some("\u{1b}")`, the field had no
/// `Escape` arm so it fell into the printable-character branch, the control
/// character was filtered out, and the empty result was read as "input
/// rejected" — which swallows the key. Escape therefore never left a
/// focused field, and anything above it that closes on Escape stayed open.
#[test]
fn escape_bubbles_out_of_a_field_even_carrying_its_control_text() {
    assert!(
        outer_handler_sees(
            Key::Escape,
            Some("\u{1b}"),
            TextInputField::new(Signal::new("hello".to_string()))
        ),
        "Escape must reach the widget above the field"
    );
}

/// ...and it made no difference with `text: None`, which is why the whole
/// suite went green on the bug. Kept so the two cases stay visibly paired.
#[test]
fn escape_bubbles_out_of_a_field_without_text() {
    assert!(outer_handler_sees(
        Key::Escape,
        None,
        TextInputField::new(Signal::new("hello".to_string()))
    ));
}

/// The other half of the guard, and the reason it is written against the
/// *text* rather than the `Key` variant: a character the field's filter
/// rejects is still swallowed, so a digits-only field does not let a
/// rejected letter fall through and match a shortcut.
///
/// A typed letter arrives as `Key::A`, not `Key::Character('a')`, so a
/// variant test here would have silently stopped swallowing letters.
#[test]
fn a_filter_rejected_character_is_still_swallowed() {
    let digits_only =
        TextInputField::new(Signal::new(String::new())).char_filter(|c: char| c.is_ascii_digit());
    assert!(
        !outer_handler_sees(Key::A, Some("a"), digits_only),
        "a rejected letter must not bubble into a shortcut match"
    );
}
