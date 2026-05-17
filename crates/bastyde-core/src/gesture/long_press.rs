use std::time::{Duration, Instant};

use bastyde_canvas::Point;

use crate::event::{ButtonMask, Modifiers, PointerButton};

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapEvent, distance};

/// Recognizes a long press (pointer held down beyond a duration without movement).
///
/// Because recognizers are pure state machines, the caller must drive time
/// by calling [`GestureRecognizer::tick`] when the timer fires (e.g. from
/// an event-loop deadline). The recognizer itself does not spawn timers.
/// The [`GestureRecognizer::next_deadline`] method exposes when the next
/// tick is needed so the event loop can wake up in time.
///
/// Default `accept` is [`ButtonMask::PRIMARY`]; presses on other buttons
/// are ignored. Modifiers are captured at the `Down` (since the
/// recognition timer fires before any `Up`) and surfaced through the
/// emitted [`TapEvent`].
#[derive(Debug)]
pub struct LongPressRecognizer {
    max_distance: f32,
    min_duration: Duration,
    accept: ButtonMask,
    down_position: Option<Point>,
    pub(super) down_time: Option<Instant>,
    down_button: Option<PointerButton>,
    down_modifiers: Modifiers,
    recognized: bool,
}

impl LongPressRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 5.0,
            min_duration: Duration::from_millis(500),
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_time: None,
            down_button: None,
            down_modifiers: Modifiers::NONE,
            recognized: false,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    pub fn min_duration(mut self, dur: Duration) -> Self {
        self.min_duration = dur;
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

    #[cfg(test)]
    fn check_timeout(&mut self, now: Instant) -> GestureResult {
        self.tick(now)
    }
}

impl Default for LongPressRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for LongPressRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position,
                button,
                modifiers,
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                self.down_position = Some(*position);
                self.down_time = Some(Instant::now());
                self.down_button = Some(*button);
                self.down_modifiers = *modifiers;
                self.recognized = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position
                    && distance(*position, down) > self.max_distance
                {
                    return GestureResult::Failed;
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up { .. } => {
                if self.recognized {
                    // Already fired — the up is just cleanup
                    self.reset();
                    GestureResult::Pending
                } else {
                    GestureResult::Failed
                }
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_time = None;
        self.down_button = None;
        self.down_modifiers = Modifiers::NONE;
        self.recognized = false;
    }

    fn priority(&self) -> u32 {
        25 // Higher than drag — long press wins over drag
    }

    fn tick(&mut self, now: Instant) -> GestureResult {
        if self.recognized {
            return GestureResult::Pending;
        }
        if let (Some(pos), Some(time), Some(button)) =
            (self.down_position, self.down_time, self.down_button)
            && now.duration_since(time) >= self.min_duration
        {
            self.recognized = true;
            return GestureResult::Recognized(GestureEvent::LongPress(TapEvent {
                position: pos,
                button,
                modifiers: self.down_modifiers,
            }));
        }
        GestureResult::Pending
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.recognized {
            return None;
        }
        self.down_time.map(|t| t + self.min_duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::test_helpers::*;

    #[test]
    fn long_press_recognized_after_timeout() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(500));
        rec.process(&down(Point::new(10.0, 10.0)));

        let down_time = rec.down_time.unwrap();

        // Not yet
        assert!(matches!(
            rec.check_timeout(down_time + Duration::from_millis(200)),
            GestureResult::Pending
        ));

        // Now!
        assert!(matches!(
            rec.check_timeout(down_time + Duration::from_millis(600)),
            GestureResult::Recognized(GestureEvent::LongPress(_))
        ));
    }

    #[test]
    fn long_press_fails_on_movement() {
        let mut rec = LongPressRecognizer::new().max_distance(5.0);
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&move_to(Point::new(30.0, 30.0))),
            GestureResult::Failed
        ));
    }

    #[test]
    fn long_press_fails_on_early_up() {
        let mut rec = LongPressRecognizer::new();
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&up(Point::new(10.0, 10.0))),
            GestureResult::Failed
        ));
    }

    #[test]
    fn long_press_default_filters_secondary() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(50));
        // Right-click Down is silently ignored.
        rec.process(&down_btn(Point::new(0.0, 0.0), PointerButton::Secondary));
        // Even after the timeout, no LongPress fires because no down
        // state was captured.
        let later = Instant::now() + Duration::from_millis(500);
        assert!(matches!(rec.check_timeout(later), GestureResult::Pending));
    }

    #[test]
    fn long_press_carries_modifiers_from_down() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(50));
        rec.process(&down_full(
            Point::new(0.0, 0.0),
            PointerButton::Primary,
            Modifiers::SHIFT,
        ));
        let down_time = rec.down_time.unwrap();
        match rec.check_timeout(down_time + Duration::from_millis(100)) {
            GestureResult::Recognized(GestureEvent::LongPress(event)) => {
                assert!(event.modifiers.shift());
                assert_eq!(event.button, PointerButton::Primary);
            }
            other => panic!("expected LongPress, got {:?}", other),
        }
    }
}
