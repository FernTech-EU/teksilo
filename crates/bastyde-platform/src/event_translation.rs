use bastyde_canvas::Point;
use bastyde_core::event::{Key, Modifiers, PointerButton, ScrollDelta, WidgetEvent};
use bastyde_core::gesture::{GestureEvent, TapEvent};

/// State tracked during event translation.
pub struct TranslationState {
    scale_factor: f64,
    cursor_position: Option<Point>,
    current_modifiers: Modifiers,
}

impl TranslationState {
    pub fn new() -> Self {
        Self {
            scale_factor: 1.0,
            cursor_position: None,
            current_modifiers: Modifiers::NONE,
        }
    }

    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn cursor_position(&self) -> Option<Point> {
        self.cursor_position
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.current_modifiers = modifiers;
    }
}

impl Default for TranslationState {
    fn default() -> Self {
        Self::new()
    }
}

/// Translate a winit CursorMoved event to a WidgetEvent::PointerMove.
pub fn translate_cursor_moved(
    physical_x: f64,
    physical_y: f64,
    state: &mut TranslationState,
) -> Option<WidgetEvent> {
    let logical_x = (physical_x / state.scale_factor) as f32;
    let logical_y = (physical_y / state.scale_factor) as f32;
    let position = Point::new(logical_x, logical_y);
    state.cursor_position = Some(position);
    Some(WidgetEvent::PointerMove { position })
}

/// Translate a winit `Ime` event into a bastyde-core `WidgetEvent`.
///
/// - `Preedit(text, cursor)` → `ImeComposition`. The `cursor` byte indices
///   `(begin, end)` index into the preedit `text` and are preserved as a
///   `Range`. `None` (hide-cursor) and empty `text` (winit's synthetic
///   clear, emitted right before `Commit`) flow through faithfully.
/// - `Commit(text)` → `ImeCommit`.
/// - `Enabled` / `Disabled` are OS acknowledgements (enablement is driven
///   by the focused node's descriptor) and produce no tree event.
pub fn translate_ime(ime: winit::event::Ime) -> Option<WidgetEvent> {
    match ime {
        winit::event::Ime::Preedit(text, cursor) => Some(WidgetEvent::ImeComposition {
            text,
            cursor: cursor.map(|(begin, end)| begin..end),
        }),
        winit::event::Ime::Commit(text) => Some(WidgetEvent::ImeCommit { text }),
        winit::event::Ime::Enabled | winit::event::Ime::Disabled => None,
    }
}

/// Translate a winit mouse button to a bastyde-core PointerButton.
pub fn translate_mouse_button(button: winit::event::MouseButton) -> Option<PointerButton> {
    match button {
        winit::event::MouseButton::Left => Some(PointerButton::Primary),
        winit::event::MouseButton::Right => Some(PointerButton::Secondary),
        winit::event::MouseButton::Middle => Some(PointerButton::Middle),
        winit::event::MouseButton::Back => Some(PointerButton::Back),
        winit::event::MouseButton::Forward => Some(PointerButton::Forward),
        // MouseButton::Other(_) — vendor-specific extra buttons we don't
        // currently surface. Returning None drops the event.
        _ => None,
    }
}

/// Translate a winit ElementState + MouseButton to PointerDown/Up.
pub fn translate_mouse_input(
    button_state: winit::event::ElementState,
    button: winit::event::MouseButton,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    let pointer_button = translate_mouse_button(button)?;
    let position = state.cursor_position.unwrap_or(Point::ZERO);
    match button_state {
        winit::event::ElementState::Pressed => Some(WidgetEvent::PointerDown {
            position,
            button: pointer_button,
            modifiers: state.current_modifiers,
        }),
        winit::event::ElementState::Released => Some(WidgetEvent::PointerUp {
            position,
            button: pointer_button,
            modifiers: state.current_modifiers,
        }),
    }
}

/// Translate winit keyboard modifiers to bastyde-core Modifiers.
pub fn translate_modifiers(mods: winit::keyboard::ModifiersState) -> Modifiers {
    let mut result = Modifiers::NONE;
    if mods.control_key() {
        result = result | Modifiers::CTRL;
    }
    if mods.shift_key() {
        result = result | Modifiers::SHIFT;
    }
    if mods.alt_key() {
        result = result | Modifiers::ALT;
    }
    if mods.super_key() {
        result = result | Modifiers::SUPER;
    }
    result
}

/// Translate a winit logical key to a bastyde-core Key.
pub fn translate_key(key: &winit::keyboard::Key) -> Option<Key> {
    match key {
        winit::keyboard::Key::Named(named) => translate_named_key(*named),
        winit::keyboard::Key::Character(c) => {
            let ch = c.chars().next()?;
            match ch.to_ascii_uppercase() {
                'A' => Some(Key::A),
                'B' => Some(Key::B),
                'C' => Some(Key::C),
                'D' => Some(Key::D),
                'E' => Some(Key::E),
                'F' => Some(Key::F),
                'G' => Some(Key::G),
                'H' => Some(Key::H),
                'I' => Some(Key::I),
                'J' => Some(Key::J),
                'K' => Some(Key::K),
                'L' => Some(Key::L),
                'M' => Some(Key::M),
                'N' => Some(Key::N),
                'O' => Some(Key::O),
                'P' => Some(Key::P),
                'Q' => Some(Key::Q),
                'R' => Some(Key::R),
                'S' => Some(Key::S),
                'T' => Some(Key::T),
                'U' => Some(Key::U),
                'V' => Some(Key::V),
                'W' => Some(Key::W),
                'X' => Some(Key::X),
                'Y' => Some(Key::Y),
                'Z' => Some(Key::Z),
                _ => Some(Key::Character(ch)),
            }
        }
        _ => None,
    }
}

fn translate_named_key(key: winit::keyboard::NamedKey) -> Option<Key> {
    use winit::keyboard::NamedKey;
    match key {
        NamedKey::Space => Some(Key::Space),
        NamedKey::Enter => Some(Key::Enter),
        NamedKey::Escape => Some(Key::Escape),
        NamedKey::Tab => Some(Key::Tab),
        NamedKey::Backspace => Some(Key::Backspace),
        NamedKey::Delete => Some(Key::Delete),
        NamedKey::ArrowUp => Some(Key::ArrowUp),
        NamedKey::ArrowDown => Some(Key::ArrowDown),
        NamedKey::ArrowLeft => Some(Key::ArrowLeft),
        NamedKey::ArrowRight => Some(Key::ArrowRight),
        NamedKey::Home => Some(Key::Home),
        NamedKey::End => Some(Key::End),
        NamedKey::PageUp => Some(Key::PageUp),
        NamedKey::PageDown => Some(Key::PageDown),
        NamedKey::F1 => Some(Key::F1),
        NamedKey::F2 => Some(Key::F2),
        NamedKey::F3 => Some(Key::F3),
        NamedKey::F4 => Some(Key::F4),
        NamedKey::F5 => Some(Key::F5),
        NamedKey::F6 => Some(Key::F6),
        NamedKey::F7 => Some(Key::F7),
        NamedKey::F8 => Some(Key::F8),
        NamedKey::F9 => Some(Key::F9),
        NamedKey::F10 => Some(Key::F10),
        NamedKey::F11 => Some(Key::F11),
        NamedKey::F12 => Some(Key::F12),
        // Caps Lock arrives as a discrete press/release. winit's
        // `ModifiersState` carries no lock state, so the window manager
        // tracks the active state itself on the key-down edge (drives
        // `WindowState::caps_lock` for the password-field warning).
        NamedKey::CapsLock => Some(Key::CapsLock),
        _ => None,
    }
}

/// Translate a winit MouseWheel event to a WidgetEvent::Scroll.
pub fn translate_mouse_wheel(
    delta: winit::event::MouseScrollDelta,
    _phase: winit::event::TouchPhase,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    // Winit uses "natural" sign: positive y = scroll up (content moves down).
    // Bastyde's ScrollDelta uses positive y = increase scroll offset (content moves up).
    // Negate both axes to match.
    //
    // Winit reports raw notch counts (typically 1.0 per wheel notch). Multiply
    // by a platform factor so each notch scrolls a comfortable number of lines.
    // 3 lines per notch matches the Windows/GTK default.
    const LINES_PER_NOTCH: f32 = 3.0;
    let scroll_delta = match delta {
        winit::event::MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines {
            x: -x * LINES_PER_NOTCH,
            y: -y * LINES_PER_NOTCH,
        },
        winit::event::MouseScrollDelta::PixelDelta(pos) => ScrollDelta::Pixels {
            x: -(pos.x / state.scale_factor) as f32,
            y: -(pos.y / state.scale_factor) as f32,
        },
    };
    Some(WidgetEvent::Scroll {
        delta: scroll_delta,
        modifiers: state.current_modifiers,
    })
}

// --- Desktop trackpad gesture passthrough ---
// On desktop, most gestures arrive as already-recognized events from the OS
// trackpad driver. These functions translate winit's high-level gesture events
// into Bastyde GestureEvents, which can be dispatched as WidgetEvent::Gesture.

/// Translate a winit PinchGesture into a Bastyde gesture event.
/// Returns PinchStarted on Started phase, PinchChanged on Changed, PinchEnded on Ended.
pub fn translate_pinch_gesture(
    delta: f64,
    phase: winit::event::TouchPhase,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    let center = state.cursor_position.unwrap_or(Point::ZERO);
    let gesture = match phase {
        winit::event::TouchPhase::Started => GestureEvent::PinchStarted { center },
        winit::event::TouchPhase::Moved => GestureEvent::PinchChanged {
            center,
            scale: 1.0 + delta as f32,
            rotation: 0.0,
        },
        winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
            GestureEvent::PinchEnded
        }
    };
    Some(WidgetEvent::Gesture { gesture })
}

/// Translate a winit RotationGesture into a PinchChanged with rotation.
/// Rotation gestures are folded into the pinch gesture model since they
/// typically co-occur with pinch on trackpads.
pub fn translate_rotation_gesture(
    delta_degrees: f32,
    phase: winit::event::TouchPhase,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    let center = state.cursor_position.unwrap_or(Point::ZERO);
    let gesture = match phase {
        winit::event::TouchPhase::Started => GestureEvent::PinchStarted { center },
        winit::event::TouchPhase::Moved => GestureEvent::PinchChanged {
            center,
            scale: 1.0,
            rotation: delta_degrees,
        },
        winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
            GestureEvent::PinchEnded
        }
    };
    Some(WidgetEvent::Gesture { gesture })
}

/// Translate a winit DoubleTapGesture (trackpad smart magnification).
///
/// Synthetic OS-driven double-tap: there's no underlying mouse button
/// or modifier set the OS hands us, so we attribute it to
/// `PointerButton::Primary` with no modifiers. Apps that need richer
/// trackpad-gesture metadata should match on `WidgetEvent::Gesture`
/// directly rather than hooking `on_double_tap`.
pub fn translate_double_tap_gesture(state: &TranslationState) -> Option<WidgetEvent> {
    let position = state.cursor_position.unwrap_or(Point::ZERO);
    Some(WidgetEvent::Gesture {
        gesture: GestureEvent::DoubleTap(TapEvent::new(
            position,
            PointerButton::Primary,
            state.current_modifiers,
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_moved_to_pointer_move() {
        let mut state = TranslationState::new();
        let event = translate_cursor_moved(100.0, 50.0, &mut state).unwrap();
        if let WidgetEvent::PointerMove { position } = event {
            assert_eq!(position.x, 100.0);
            assert_eq!(position.y, 50.0);
        } else {
            panic!("Expected PointerMove");
        }
    }

    #[test]
    fn scale_factor_divides_physical_coords() {
        let mut state = TranslationState::new();
        state.set_scale_factor(2.0);
        let event = translate_cursor_moved(200.0, 100.0, &mut state).unwrap();
        if let WidgetEvent::PointerMove { position } = event {
            assert_eq!(position.x, 100.0);
            assert_eq!(position.y, 50.0);
        } else {
            panic!("Expected PointerMove");
        }
    }

    #[test]
    fn mouse_button_translation() {
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Left),
            Some(PointerButton::Primary)
        );
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Right),
            Some(PointerButton::Secondary)
        );
    }

    #[test]
    fn mouse_press_to_pointer_down() {
        let mut state = TranslationState::new();
        translate_cursor_moved(50.0, 25.0, &mut state);
        let event = translate_mouse_input(
            winit::event::ElementState::Pressed,
            winit::event::MouseButton::Left,
            &state,
        )
        .unwrap();
        assert!(matches!(
            event,
            WidgetEvent::PointerDown {
                button: PointerButton::Primary,
                ..
            }
        ));
    }

    #[test]
    fn key_translation() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space);
        assert_eq!(translate_key(&key), Some(Key::Space));
    }

    #[test]
    fn pinch_gesture_started() {
        let mut state = TranslationState::new();
        translate_cursor_moved(100.0, 50.0, &mut state);
        let event =
            translate_pinch_gesture(0.0, winit::event::TouchPhase::Started, &state).unwrap();
        assert!(matches!(
            event,
            WidgetEvent::Gesture {
                gesture: GestureEvent::PinchStarted { .. }
            }
        ));
    }

    #[test]
    fn pinch_gesture_changed() {
        let mut state = TranslationState::new();
        translate_cursor_moved(100.0, 50.0, &mut state);
        let event = translate_pinch_gesture(0.5, winit::event::TouchPhase::Moved, &state).unwrap();
        if let WidgetEvent::Gesture {
            gesture: GestureEvent::PinchChanged { scale, .. },
        } = event
        {
            assert!((scale - 1.5).abs() < 0.001);
        } else {
            panic!("Expected PinchChanged");
        }
    }

    #[test]
    fn pinch_gesture_ended() {
        let state = TranslationState::new();
        let event = translate_pinch_gesture(0.0, winit::event::TouchPhase::Ended, &state).unwrap();
        assert!(matches!(
            event,
            WidgetEvent::Gesture {
                gesture: GestureEvent::PinchEnded
            }
        ));
    }

    #[test]
    fn rotation_gesture_translates() {
        let mut state = TranslationState::new();
        translate_cursor_moved(50.0, 50.0, &mut state);
        let event =
            translate_rotation_gesture(15.0, winit::event::TouchPhase::Moved, &state).unwrap();
        if let WidgetEvent::Gesture {
            gesture: GestureEvent::PinchChanged {
                rotation, scale, ..
            },
        } = event
        {
            assert!((rotation - 15.0).abs() < 0.001);
            assert!((scale - 1.0).abs() < 0.001);
        } else {
            panic!("Expected PinchChanged with rotation");
        }
    }

    #[test]
    fn double_tap_gesture_translates() {
        let mut state = TranslationState::new();
        translate_cursor_moved(75.0, 25.0, &mut state);
        let event = translate_double_tap_gesture(&state).unwrap();
        if let WidgetEvent::Gesture {
            gesture: GestureEvent::DoubleTap(tap_event),
        } = event
        {
            assert_eq!(tap_event.position.x, 75.0);
            assert_eq!(tap_event.position.y, 25.0);
            assert_eq!(tap_event.button, PointerButton::Primary);
        } else {
            panic!("Expected DoubleTap");
        }
    }

    #[test]
    fn ime_preedit_to_composition_preserves_cursor_bytes() {
        let evt = translate_ime(winit::event::Ime::Preedit("a".to_string(), Some((1, 1)))).unwrap();
        assert!(matches!(
            evt,
            WidgetEvent::ImeComposition { ref text, cursor: Some(ref r) }
                if text == "a" && *r == (1..1)
        ));
    }

    #[test]
    fn ime_preedit_hide_cursor_maps_to_none() {
        let evt = translate_ime(winit::event::Ime::Preedit(String::new(), None)).unwrap();
        assert!(matches!(
            evt,
            WidgetEvent::ImeComposition { ref text, cursor: None } if text.is_empty()
        ));
    }

    #[test]
    fn ime_commit_translates() {
        let evt = translate_ime(winit::event::Ime::Commit("你".to_string())).unwrap();
        assert!(matches!(evt, WidgetEvent::ImeCommit { ref text } if text == "你"));
    }

    #[test]
    fn ime_enabled_disabled_produce_no_event() {
        assert!(translate_ime(winit::event::Ime::Enabled).is_none());
        assert!(translate_ime(winit::event::Ime::Disabled).is_none());
    }
}
