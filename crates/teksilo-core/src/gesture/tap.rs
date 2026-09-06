// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::Point;

use crate::event::{ButtonMask, PointerButton};

use super::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, RecognizerContext, TapEvent,
};

/// Recognizes a single tap (pointer down + up without significant movement).
///
/// The recognizer is button-aware: it only fires when the press button
/// matches the release button, and only for buttons present in
/// [`accept`](TapRecognizer::accept_buttons). Default `accept` is
/// [`ButtonMask::PRIMARY`] — left-click only — which is what users
/// expect from a "tap" and keeps right-click free for context menus.
///
/// The travel a tap tolerates is the press's
/// [`TapBoundary`](super::TapBoundary): a radius of the active profile's
/// `tap_slop` for a precise pointer — 5 dp for a mouse, exactly the pre-P06
/// constant — and the pressed node's **own bounds** for a coarse one, because a
/// finger's reported centre wanders several device pixels while resting inside
/// the control it is pressing, and a tap that stayed on its target is a tap.
/// [`max_distance`](Self::max_distance) pins a radius for either.
#[derive(Debug)]
pub struct TapRecognizer {
    max_distance: Option<f32>,
    accept: ButtonMask,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl TapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: None,
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_button: None,
        }
    }

    /// Pin the travel a tap tolerates, overriding the profile's `tap_slop`.
    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = Some(d);
        self
    }

    /// Where this press stops being a tap. One predicate, shared with the
    /// arbitration's `cancel_taps` trigger, so the recognizer and the router
    /// cannot disagree about whether a press has slid off.
    fn boundary(&self, cx: &RecognizerContext) -> super::TapBoundary {
        match self.max_distance {
            Some(d) => super::TapBoundary::Radius(d),
            None => super::TapBoundary::for_pointer(&cx.pointer, &cx.profile),
        }
    }

    fn left_boundary(&self, cx: &RecognizerContext, down: Point, at: Point) -> bool {
        self.boundary(cx)
            .left(down, at, Some(cx.local_bounds), &cx.profile)
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
    fn process(&mut self, event: &RawPointerEvent, cx: &RecognizerContext) -> GestureResult {
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
            RawPointerEvent::Move { position, .. } => {
                if let Some(down) = self.down_position
                    && self.left_boundary(cx, down, *position)
                {
                    self.down_position = None;
                    self.down_button = None;
                    return GestureResult::Failed;
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
                pointer,
                ..
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
                if !self.left_boundary(cx, down, *position) {
                    return GestureResult::Recognized(GestureEvent::Tap(TapEvent {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                        pointer: *pointer,
                    }));
                }
                GestureResult::Failed
            }
            RawPointerEvent::Cancel { .. } => {
                self.down_position = None;
                self.down_button = None;
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

    fn tap_family(&self) -> bool {
        true
    }
}

#[cfg(test)]
impl TapRecognizer {
    /// Drive the recognizer with the shipped profile for the event's own
    /// pointer kind. The pre-P06 call shape, kept so the migration is checked
    /// against unchanged tests.
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        let cx = super::config::RecognizerContext::for_event(event);
        GestureRecognizer::process(self, event, &cx)
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
