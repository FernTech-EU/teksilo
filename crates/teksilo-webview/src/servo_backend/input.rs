// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`to_servo`]: the widget's [`WebViewInputEvent`] as Servo's `InputEvent`.
//!
//! Positions arrive in page pixels, the frame's own device pixels, which is
//! what Servo hit-tests in (`WebViewPoint::Device`). Two things do not carry
//! over exactly:
//!
//! - **Physical keys.** Teksilo's key events carry the logical key and the
//!   text it produced, not the physical key, so every key reaches the page
//!   with `code == "Unidentified"`. A page reading `KeyboardEvent.code` (a
//!   game, an editor with layout-independent shortcuts) sees nothing it can
//!   use; `key` and the text are right.
//! - **Wheel units.** Servo scrolls by the delta's value in device pixels
//!   whatever its mode says, and passes the mode on to the page's `WheelEvent`.
//!   So a wheel's lines are sent as device pixels, [`PIXELS_PER_LINE`] each,
//!   marked `DeltaLine` as Servo's own embedder does.

use servo::{
    Code, CompositionEvent, CompositionState, DevicePoint, EditingActionEvent, ImeEvent,
    InputEvent, Key, KeyState, KeyboardEvent, Location, Modifiers as ServoModifiers, MouseButton,
    MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent, MouseMoveEvent, NamedKey,
    TouchEvent, TouchEventType, TouchId, TouchPointerType, WebViewPoint, WheelDelta, WheelEvent,
    WheelMode,
};
use teksilo_canvas::Point;
use teksilo_core::event::{Key as TKey, Modifiers, PointerButton, ScrollDelta};

use crate::backend::{WebViewEditCommand, WebViewInputEvent, WebViewTouchPhase};

/// Device pixels per wheel line, at a scale of 1. Servo's own winit embedder
/// scrolls 76 pixels per wheel notch, and Teksilo counts three lines to a
/// notch.
pub(super) const PIXELS_PER_LINE: f64 = 76.0 / 3.0;

/// `event` as Servo takes it; `scale` is the window's device pixels per
/// logical pixel, for the deltas that arrive in logical units. `None` for an
/// event Servo has no form for.
pub(super) fn to_servo(event: &WebViewInputEvent, scale: f32) -> Option<InputEvent> {
    let point = |p: &Point| WebViewPoint::Device(DevicePoint::new(p.x, p.y));
    Some(match event {
        WebViewInputEvent::PointerMove { position } => {
            InputEvent::MouseMove(MouseMoveEvent::new(point(position)))
        }
        WebViewInputEvent::PointerButton {
            position,
            button,
            pressed,
        } => InputEvent::MouseButton(MouseButtonEvent::new(
            if *pressed {
                MouseButtonAction::Down
            } else {
                MouseButtonAction::Up
            },
            mouse_button(*button),
            point(position),
        )),
        WebViewInputEvent::PointerLeave => InputEvent::MouseLeftViewport(MouseLeftViewportEvent {
            focus_moving_to_another_iframe: false,
        }),
        WebViewInputEvent::Touch {
            contact,
            phase,
            position,
            pen,
        } => InputEvent::Touch(TouchEvent::new(
            match phase {
                WebViewTouchPhase::Down => TouchEventType::Down,
                WebViewTouchPhase::Move => TouchEventType::Move,
                WebViewTouchPhase::Up => TouchEventType::Up,
                WebViewTouchPhase::Cancel => TouchEventType::Cancel,
            },
            // Ids are minted in order and live for one contact, so their low
            // bits tell apart every contact that can be down at once.
            TouchId((contact.get() & 0x7fff_ffff) as i32),
            point(position),
            if *pen {
                TouchPointerType::Pen
            } else {
                TouchPointerType::Touch
            },
        )),
        WebViewInputEvent::Scroll {
            position, delta, ..
        } => {
            // Teksilo's positive y scrolls toward the end of the content;
            // Servo's reveals content above.
            let (x, y, mode) = match *delta {
                ScrollDelta::Lines { x, y } => (
                    f64::from(x) * PIXELS_PER_LINE * f64::from(scale),
                    f64::from(y) * PIXELS_PER_LINE * f64::from(scale),
                    WheelMode::DeltaLine,
                ),
                ScrollDelta::Pixels { x, y } => (
                    f64::from(x * scale),
                    f64::from(y * scale),
                    WheelMode::DeltaPixel,
                ),
            };
            InputEvent::Wheel(WheelEvent::new(
                WheelDelta {
                    x: -x,
                    y: -y,
                    z: 0.0,
                    mode,
                },
                point(position),
            ))
        }
        WebViewInputEvent::Key {
            key,
            modifiers,
            pressed,
            text,
        } => InputEvent::Keyboard(KeyboardEvent::new_without_event(
            if *pressed {
                KeyState::Down
            } else {
                KeyState::Up
            },
            servo_key(*key, *modifiers, text.as_deref()),
            Code::Unidentified,
            Location::Standard,
            servo_modifiers(*modifiers),
            false,
            false,
        )),
        WebViewInputEvent::ImeComposition { text, .. } => {
            InputEvent::Ime(ImeEvent::Composition(CompositionEvent {
                state: CompositionState::Update,
                data: text.clone(),
            }))
        }
        WebViewInputEvent::ImeCommit { text } => {
            InputEvent::Ime(ImeEvent::Composition(CompositionEvent {
                state: CompositionState::End,
                data: text.clone(),
            }))
        }
        WebViewInputEvent::Edit(command) => InputEvent::EditingAction(match command {
            WebViewEditCommand::Copy => EditingActionEvent::Copy,
            WebViewEditCommand::Cut => EditingActionEvent::Cut,
            WebViewEditCommand::Paste => EditingActionEvent::Paste,
        }),
    })
}

fn mouse_button(button: PointerButton) -> MouseButton {
    match button {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Middle => MouseButton::Middle,
        PointerButton::Back => MouseButton::Back,
        PointerButton::Forward => MouseButton::Forward,
        _ => MouseButton::Other(0),
    }
}

fn servo_modifiers(modifiers: Modifiers) -> ServoModifiers {
    let mut out = ServoModifiers::empty();
    out.set(ServoModifiers::SHIFT, modifiers.shift());
    out.set(ServoModifiers::CONTROL, modifiers.ctrl());
    out.set(ServoModifiers::ALT, modifiers.alt());
    out.set(ServoModifiers::META, modifiers.super_key());
    out
}

/// The key as the page's `KeyboardEvent.key` names it: the produced text for
/// a printable key, the letter for a letter pressed with a shortcut modifier
/// (Ctrl+C produces no text), the named key otherwise.
fn servo_key(key: TKey, modifiers: Modifiers, text: Option<&str>) -> Key {
    if let Some(text) = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control)) {
        return Key::Character(text.to_string());
    }
    let named = match key {
        TKey::Space => return Key::Character(" ".into()),
        TKey::Character(c) => return Key::Character(c.to_string()),
        TKey::Enter => NamedKey::Enter,
        TKey::Escape => NamedKey::Escape,
        TKey::Tab => NamedKey::Tab,
        TKey::Backspace => NamedKey::Backspace,
        TKey::Delete => NamedKey::Delete,
        TKey::Insert => NamedKey::Insert,
        TKey::ArrowUp => NamedKey::ArrowUp,
        TKey::ArrowDown => NamedKey::ArrowDown,
        TKey::ArrowLeft => NamedKey::ArrowLeft,
        TKey::ArrowRight => NamedKey::ArrowRight,
        TKey::Home => NamedKey::Home,
        TKey::End => NamedKey::End,
        TKey::PageUp => NamedKey::PageUp,
        TKey::PageDown => NamedKey::PageDown,
        TKey::CapsLock => NamedKey::CapsLock,
        TKey::ContextMenu => NamedKey::ContextMenu,
        TKey::F1 => NamedKey::F1,
        TKey::F2 => NamedKey::F2,
        TKey::F3 => NamedKey::F3,
        TKey::F4 => NamedKey::F4,
        TKey::F5 => NamedKey::F5,
        TKey::F6 => NamedKey::F6,
        TKey::F7 => NamedKey::F7,
        TKey::F8 => NamedKey::F8,
        TKey::F9 => NamedKey::F9,
        TKey::F10 => NamedKey::F10,
        TKey::F11 => NamedKey::F11,
        TKey::F12 => NamedKey::F12,
        other => match letter(other) {
            Some(c) => {
                let c = if modifiers.shift() {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                return Key::Character(c.to_string());
            }
            None => NamedKey::Unidentified,
        },
    };
    Key::Named(named)
}

/// The lowercase letter a letter key names.
fn letter(key: TKey) -> Option<char> {
    const LETTERS: [(TKey, char); 26] = [
        (TKey::A, 'a'),
        (TKey::B, 'b'),
        (TKey::C, 'c'),
        (TKey::D, 'd'),
        (TKey::E, 'e'),
        (TKey::F, 'f'),
        (TKey::G, 'g'),
        (TKey::H, 'h'),
        (TKey::I, 'i'),
        (TKey::J, 'j'),
        (TKey::K, 'k'),
        (TKey::L, 'l'),
        (TKey::M, 'm'),
        (TKey::N, 'n'),
        (TKey::O, 'o'),
        (TKey::P, 'p'),
        (TKey::Q, 'q'),
        (TKey::R, 'r'),
        (TKey::S, 's'),
        (TKey::T, 't'),
        (TKey::U, 'u'),
        (TKey::V, 'v'),
        (TKey::W, 'w'),
        (TKey::X, 'x'),
        (TKey::Y, 'y'),
        (TKey::Z, 'z'),
    ];
    LETTERS.iter().find(|(k, _)| *k == key).map(|(_, c)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(event: InputEvent) -> WheelDelta {
        match event {
            InputEvent::Wheel(wheel) => wheel.delta,
            other => panic!("not a wheel event: {other:?}"),
        }
    }

    fn key(event: InputEvent) -> Key {
        match event {
            InputEvent::Keyboard(k) => k.event.key,
            other => panic!("not a key event: {other:?}"),
        }
    }

    #[test]
    fn a_wheel_down_reveals_content_below() {
        let down = WebViewInputEvent::Scroll {
            position: Point::new(10.0, 10.0),
            delta: ScrollDelta::Lines { x: 0.0, y: 3.0 },
            modifiers: Modifiers::NONE,
        };
        let delta = wheel(to_servo(&down, 1.0).unwrap());
        assert!(delta.y < 0.0, "Servo's negative y scrolls down: {delta:?}");
        assert!(
            (delta.y + 76.0).abs() < 1e-6,
            "one notch is 76 px: {delta:?}"
        );
        assert_eq!(delta.mode, WheelMode::DeltaLine);
    }

    #[test]
    fn a_trackpad_scroll_is_in_device_pixels() {
        let scroll = WebViewInputEvent::Scroll {
            position: Point::new(10.0, 10.0),
            delta: ScrollDelta::Pixels { x: 4.0, y: -10.0 },
            modifiers: Modifiers::NONE,
        };
        let delta = wheel(to_servo(&scroll, 2.0).unwrap());
        assert_eq!((delta.x, delta.y), (-8.0, 20.0));
        assert_eq!(delta.mode, WheelMode::DeltaPixel);
    }

    #[test]
    fn a_printable_key_is_its_text_and_a_shortcut_letter_is_its_letter() {
        let typed = WebViewInputEvent::Key {
            key: TKey::E,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: Some("é".into()),
        };
        assert_eq!(
            key(to_servo(&typed, 1.0).unwrap()),
            Key::Character("é".into())
        );

        let copy = WebViewInputEvent::Key {
            key: TKey::C,
            modifiers: Modifiers::CTRL,
            pressed: true,
            text: None,
        };
        assert_eq!(
            key(to_servo(&copy, 1.0).unwrap()),
            Key::Character("c".into())
        );

        let enter = WebViewInputEvent::Key {
            key: TKey::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: Some("\r".into()),
        };
        assert_eq!(
            key(to_servo(&enter, 1.0).unwrap()),
            Key::Named(NamedKey::Enter),
            "a control character is not text"
        );
    }

    #[test]
    fn a_press_is_a_mouse_button_at_the_page_pixel() {
        let press = WebViewInputEvent::PointerButton {
            position: Point::new(20.0, 40.0),
            button: PointerButton::Secondary,
            pressed: true,
        };
        match to_servo(&press, 2.0).unwrap() {
            InputEvent::MouseButton(e) => {
                assert_eq!(e.action, MouseButtonAction::Down);
                assert_eq!(e.button, MouseButton::Right);
                assert_eq!(
                    e.point,
                    WebViewPoint::Device(DevicePoint::new(20.0, 40.0)),
                    "positions are page pixels already; the scale applies to deltas only"
                );
            }
            other => panic!("{other:?}"),
        }
    }
}
