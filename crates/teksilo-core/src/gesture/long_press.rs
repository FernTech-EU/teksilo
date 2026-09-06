// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::Duration;

use teksilo_canvas::Point;

use crate::event::{ButtonMask, Modifiers, PointerButton};
use crate::pointer::{EventTime, PointerInfo};

use super::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, RecognizerContext, TapEvent,
    distance,
};

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
///
/// The hold and the travel it tolerates come from the active profile's
/// `long_press` / `long_press_slop` — 500 ms and 5 dp for a mouse (exactly the
/// pre-P06 constants), 500 ms and 18 dp for a finger — unless
/// [`min_duration`](Self::min_duration) / [`max_distance`](Self::max_distance)
/// pin them.
#[derive(Debug)]
pub struct LongPressRecognizer {
    max_distance: Option<f32>,
    min_duration: Option<Duration>,
    accept: ButtonMask,
    down_position: Option<Point>,
    pub(super) down_time: Option<EventTime>,
    down_button: Option<PointerButton>,
    down_modifiers: Modifiers,
    down_pointer: Option<PointerInfo>,
    /// The hold resolved at the press, so the deadline
    /// [`next_deadline`](GestureRecognizer::next_deadline) reports does not
    /// need a context the event loop cannot supply.
    hold: Duration,
    recognized: bool,
}

impl LongPressRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: None,
            min_duration: None,
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_time: None,
            down_button: None,
            down_modifiers: Modifiers::NONE,
            down_pointer: None,
            hold: Duration::ZERO,
            recognized: false,
        }
    }

    /// Pin the travel the hold tolerates, overriding the profile's
    /// `long_press_slop`.
    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = Some(d);
        self
    }

    /// Pin how long the press must be held, overriding the profile's
    /// `long_press`.
    pub fn min_duration(mut self, dur: Duration) -> Self {
        self.min_duration = Some(dur);
        self
    }

    fn slop(&self, cx: &RecognizerContext) -> f32 {
        self.max_distance.unwrap_or(cx.profile.long_press_slop)
    }

    fn hold_for(&self, cx: &RecognizerContext) -> Duration {
        self.min_duration.unwrap_or(cx.profile.long_press)
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
    fn check_timeout(&mut self, now: EventTime) -> GestureResult {
        let cx = super::config::RecognizerContext::new(
            now,
            teksilo_tokens::GestureProfile::MOUSE,
            teksilo_canvas::Rect::ZERO,
            crate::pointer::PointerInfo::mouse(now),
        );
        GestureRecognizer::tick(self, &cx)
    }

    /// The pre-P06 call shape — see `TapRecognizer::process`.
    #[cfg(test)]
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        let cx = super::config::RecognizerContext::for_event(event);
        GestureRecognizer::process(self, event, &cx)
    }
}

impl Default for LongPressRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for LongPressRecognizer {
    fn process(&mut self, event: &RawPointerEvent, cx: &RecognizerContext) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position,
                button,
                modifiers,
                pointer,
                ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                self.down_position = Some(*position);
                self.down_time = Some(cx.now);
                self.down_button = Some(*button);
                self.down_modifiers = *modifiers;
                self.down_pointer = Some(*pointer);
                self.hold = self.hold_for(cx);
                self.recognized = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position, .. } => {
                if let Some(down) = self.down_position
                    && distance(*position, down) > self.slop(cx)
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
            RawPointerEvent::Cancel { .. } => {
                self.reset();
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_time = None;
        self.down_button = None;
        self.down_modifiers = Modifiers::NONE;
        self.down_pointer = None;
        self.hold = Duration::ZERO;
        self.recognized = false;
    }

    fn priority(&self) -> u32 {
        25 // Higher than drag — long press wins over drag
    }

    fn tap_family(&self) -> bool {
        true
    }

    fn tick(&mut self, cx: &RecognizerContext) -> GestureResult {
        if self.recognized {
            return GestureResult::Pending;
        }
        if let (Some(pos), Some(time), Some(button)) =
            (self.down_position, self.down_time, self.down_button)
            && cx.now.saturating_since(time) >= self.hold
        {
            self.recognized = true;
            return GestureResult::Recognized(GestureEvent::LongPress(TapEvent {
                position: pos,
                button,
                modifiers: self.down_modifiers,
                pointer: self.down_pointer.unwrap_or(cx.pointer),
            }));
        }
        GestureResult::Pending
    }

    fn next_deadline(&self) -> Option<EventTime> {
        if self.recognized {
            return None;
        }
        self.down_time.and_then(|t| t.checked_add(self.hold))
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
        let later = EventTime::from_millis(500);
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
