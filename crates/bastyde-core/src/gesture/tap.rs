use bastyde_canvas::Point;

use crate::event::{ButtonMask, PointerButton};

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapEvent};

/// Recognizes a single tap (pointer down + up without significant movement).
///
/// The recognizer is button-aware: it only fires when the press button
/// matches the release button, and only for buttons present in
/// [`accept`](TapRecognizer::accept_buttons). Default `accept` is
/// [`ButtonMask::PRIMARY`] — left-click only — which is what users
/// expect from a "tap" and keeps right-click free for context menus.
#[derive(Debug)]
pub struct TapRecognizer {
    max_distance: f32,
    accept: ButtonMask,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl TapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 5.0,
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_button: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    /// Restrict (or extend) the set of buttons that can fire this
    /// recognizer. Default is [`ButtonMask::PRIMARY`].
    pub fn accept_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.accept = mask.into();
        self
    }

    /// Convenience: accept any pointer button.
    pub fn accept_any_button(self) -> Self {
        self.accept_buttons(ButtonMask::ALL)
    }
}

impl Default for TapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for TapRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position {
                    let dx = position.x - down.x;
                    let dy = position.y - down.y;
                    if (dx * dx + dy * dy).sqrt() > self.max_distance {
                        self.down_position = None;
                        self.down_button = None;
                        return GestureResult::Failed;
                    }
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
            } => {
                let Some(down) = self.down_position.take() else {
                    return GestureResult::Failed;
                };
                let Some(down_button) = self.down_button.take() else {
                    return GestureResult::Failed;
                };
                if *button != down_button {
                    return GestureResult::Failed;
                }
                let dx = position.x - down.x;
                let dy = position.y - down.y;
                if (dx * dx + dy * dy).sqrt() <= self.max_distance {
                    return GestureResult::Recognized(GestureEvent::Tap(TapEvent {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                    }));
                }
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modifiers;
    use crate::gesture::test_helpers::*;

    #[test]
    fn tap_recognized_on_quick_down_up() {
        let mut rec = TapRecognizer::new();
        assert!(matches!(
            rec.process(&down(Point::new(10.0, 10.0))),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&up(Point::new(10.0, 10.0))),
            GestureResult::Recognized(GestureEvent::Tap(_))
        ));
    }

    #[test]
    fn tap_fails_if_moved_too_far() {
        let mut rec = TapRecognizer::new().max_distance(5.0);
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&move_to(Point::new(20.0, 10.0))),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_default_filters_secondary_button() {
        let mut rec = TapRecognizer::new();
        // Right-click: Down is silently ignored (still Pending), Up
        // sees no down_position recorded and fails — no Tap fires.
        assert!(matches!(
            rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Secondary)),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Secondary)),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_default_filters_middle_button() {
        let mut rec = TapRecognizer::new();
        assert!(matches!(
            rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Middle)),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Middle)),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_accept_secondary_recognises_right_click() {
        let mut rec = TapRecognizer::new().accept_buttons(ButtonMask::SECONDARY);
        rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Secondary));
        match rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Secondary)) {
            GestureResult::Recognized(GestureEvent::Tap(event)) => {
                assert_eq!(event.button, PointerButton::Secondary);
            }
            other => panic!("expected Tap on right-click, got {:?}", other),
        }
    }

    #[test]
    fn tap_button_mismatch_fails() {
        let mut rec = TapRecognizer::new().accept_any_button();
        rec.process(&down_btn(Point::new(10.0, 10.0), PointerButton::Primary));
        // Up with a different button → Failed, no Tap.
        assert!(matches!(
            rec.process(&up_btn(Point::new(10.0, 10.0), PointerButton::Secondary)),
            GestureResult::Failed
        ));
    }

    #[test]
    fn tap_carries_modifiers_from_up() {
        let mut rec = TapRecognizer::new();
        rec.process(&down_full(
            Point::new(10.0, 10.0),
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        match rec.process(&up_full(
            Point::new(10.0, 10.0),
            PointerButton::Primary,
            Modifiers::CTRL | Modifiers::SHIFT,
        )) {
            GestureResult::Recognized(GestureEvent::Tap(event)) => {
                assert!(event.modifiers.ctrl());
                assert!(event.modifiers.shift());
                assert!(!event.modifiers.alt());
            }
            other => panic!("expected Tap with modifiers, got {:?}", other),
        }
    }

    #[test]
    fn tap_recognised_for_forward_or_back_when_accepted() {
        let mut rec = TapRecognizer::new().accept_buttons(ButtonMask::FORWARD | ButtonMask::BACK);
        rec.process(&down_btn(Point::new(0.0, 0.0), PointerButton::Forward));
        assert!(matches!(
            rec.process(&up_btn(Point::new(0.0, 0.0), PointerButton::Forward)),
            GestureResult::Recognized(GestureEvent::Tap(_))
        ));
        rec.process(&down_btn(Point::new(0.0, 0.0), PointerButton::Back));
        assert!(matches!(
            rec.process(&up_btn(Point::new(0.0, 0.0), PointerButton::Back)),
            GestureResult::Recognized(GestureEvent::Tap(_))
        ));
    }
}
