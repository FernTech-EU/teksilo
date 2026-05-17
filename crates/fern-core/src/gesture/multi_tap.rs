use std::time::{Duration, Instant};

use fern_canvas::Point;

use crate::event::{ButtonMask, PointerButton};

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapEvent, distance};

/// Recognizes a double-tap (two taps within a time window and distance,
/// using the same button).
///
/// Default `accept` is [`ButtonMask::PRIMARY`]; presses on other buttons
/// are ignored. Within the recognized sequence, both taps must match
/// the press button — a `Primary` then `Secondary` sequence resets to
/// the new tap as a fresh "first" rather than firing `DoubleTap`.
#[derive(Debug)]
pub struct DoubleTapRecognizer {
    max_distance: f32,
    max_interval: Duration,
    accept: ButtonMask,
    first_tap_position: Option<Point>,
    first_tap_time: Option<Instant>,
    first_tap_button: Option<PointerButton>,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl DoubleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 10.0,
            max_interval: Duration::from_millis(300),
            accept: ButtonMask::PRIMARY,
            first_tap_position: None,
            first_tap_time: None,
            first_tap_button: None,
            down_position: None,
            down_button: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    pub fn max_interval(mut self, interval: Duration) -> Self {
        self.max_interval = interval;
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

    /// Feed an event with an explicit timestamp (for testability without real clocks).
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                // Cross-tap button-match: if we have a first tap from a
                // different button, the new press starts fresh — reset
                // the accumulated state to avoid spuriously firing a
                // mixed-button DoubleTap.
                if let Some(first_button) = self.first_tap_button
                    && first_button != *button
                {
                    self.first_tap_position = None;
                    self.first_tap_time = None;
                    self.first_tap_button = None;
                }
                self.down_position = Some(*position);
                self.down_button = Some(*button);
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
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
            } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Failed;
                };
                let Some(down_button) = self.down_button else {
                    return GestureResult::Failed;
                };
                self.down_position = None;
                self.down_button = None;
                if *button != down_button {
                    return GestureResult::Failed;
                }
                if distance(*position, down) > self.max_distance {
                    return GestureResult::Failed;
                }

                if let (Some(first_pos), Some(first_time), Some(first_button)) = (
                    self.first_tap_position,
                    self.first_tap_time,
                    self.first_tap_button,
                ) {
                    // Second tap — check distance, time interval, AND
                    // button match against the first tap.
                    if first_button == *button
                        && distance(*position, first_pos) <= self.max_distance
                        && now.duration_since(first_time) <= self.max_interval
                    {
                        self.reset();
                        return GestureResult::Recognized(GestureEvent::DoubleTap(TapEvent {
                            position: *position,
                            button: *button,
                            modifiers: *modifiers,
                        }));
                    }
                    // Out of window or button mismatch — treat as new
                    // first tap.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    GestureResult::Pending
                } else {
                    // First tap — record and wait for second.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    GestureResult::Pending
                }
            }
        }
    }
}

impl Default for DoubleTapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for DoubleTapRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        self.process_at(event, Instant::now())
    }

    fn reset(&mut self) {
        self.first_tap_position = None;
        self.first_tap_time = None;
        self.first_tap_button = None;
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        15 // Higher than tap — double-tap should win over single tap
    }

    fn resets_on_peer_recognition(&self) -> bool {
        // Cooperative with `TripleTapRecognizer`: when we fire a DoubleTap
        // at click 2, the triple-tap recognizer may still be mid-sequence
        // waiting for click 3. The arena must not wipe triple-tap state
        // because of our win, and symmetrically we don't want our state
        // wiped by a triple-tap's win either (though we've already reset
        // ourselves internally by then).
        false
    }
}

/// Recognizes a triple tap (three taps within a time window and
/// distance, all using the same button).
///
/// State machine mirrors `DoubleTapRecognizer` with one extra step:
/// Idle → FirstTapLanded → SecondTapLanded → Recognized(TripleTap).
/// Defaults match `DoubleTapRecognizer` (300 ms / 10 px / Primary only)
/// so the two fire as a natural escalating pair. Mixed-button sequences
/// reset to a fresh first tap.
#[derive(Debug)]
pub struct TripleTapRecognizer {
    max_distance: f32,
    max_interval: Duration,
    accept: ButtonMask,
    first_tap_position: Option<Point>,
    first_tap_time: Option<Instant>,
    first_tap_button: Option<PointerButton>,
    second_tap_position: Option<Point>,
    second_tap_time: Option<Instant>,
    second_tap_button: Option<PointerButton>,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
}

impl TripleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 10.0,
            max_interval: Duration::from_millis(300),
            accept: ButtonMask::PRIMARY,
            first_tap_position: None,
            first_tap_time: None,
            first_tap_button: None,
            second_tap_position: None,
            second_tap_time: None,
            second_tap_button: None,
            down_position: None,
            down_button: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
    }

    pub fn max_interval(mut self, interval: Duration) -> Self {
        self.max_interval = interval;
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

    /// Feed an event with an explicit timestamp (for testability without real clocks).
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                if !self.accept.contains(*button) {
                    return GestureResult::Pending;
                }
                // Cross-tap button-match: if any accumulated tap used a
                // different button, drop everything and start fresh.
                let mismatch = self.first_tap_button.map(|b| b != *button).unwrap_or(false)
                    || self
                        .second_tap_button
                        .map(|b| b != *button)
                        .unwrap_or(false);
                if mismatch {
                    self.first_tap_position = None;
                    self.first_tap_time = None;
                    self.first_tap_button = None;
                    self.second_tap_position = None;
                    self.second_tap_time = None;
                    self.second_tap_button = None;
                }
                self.down_position = Some(*position);
                self.down_button = Some(*button);
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
            RawPointerEvent::Up {
                position,
                button,
                modifiers,
            } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Failed;
                };
                let Some(down_button) = self.down_button else {
                    return GestureResult::Failed;
                };
                self.down_position = None;
                self.down_button = None;
                if *button != down_button {
                    return GestureResult::Failed;
                }
                if distance(*position, down) > self.max_distance {
                    return GestureResult::Failed;
                }

                // Third tap landed — this is the third if both prior
                // timings AND buttons are in window/match.
                if let (
                    Some(first_pos),
                    Some(first_time),
                    Some(first_button),
                    Some(second_pos),
                    Some(second_time),
                    Some(second_button),
                ) = (
                    self.first_tap_position,
                    self.first_tap_time,
                    self.first_tap_button,
                    self.second_tap_position,
                    self.second_tap_time,
                    self.second_tap_button,
                ) {
                    if first_button == *button
                        && second_button == *button
                        && distance(*position, second_pos) <= self.max_distance
                        && now.duration_since(second_time) <= self.max_interval
                        && distance(second_pos, first_pos) <= self.max_distance
                        && second_time.duration_since(first_time) <= self.max_interval
                    {
                        self.reset();
                        return GestureResult::Recognized(GestureEvent::TripleTap(TapEvent {
                            position: *position,
                            button: *button,
                            modifiers: *modifiers,
                        }));
                    }
                    // Out of window or button mismatch: fold this tap
                    // forward as a fresh first.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    self.second_tap_position = None;
                    self.second_tap_time = None;
                    self.second_tap_button = None;
                    return GestureResult::Pending;
                }

                // First or second tap.
                if let (Some(first_pos), Some(first_time), Some(first_button)) = (
                    self.first_tap_position,
                    self.first_tap_time,
                    self.first_tap_button,
                ) {
                    // Second tap — if in window AND button matches, promote.
                    if first_button == *button
                        && distance(*position, first_pos) <= self.max_distance
                        && now.duration_since(first_time) <= self.max_interval
                    {
                        self.second_tap_position = Some(*position);
                        self.second_tap_time = Some(now);
                        self.second_tap_button = Some(*button);
                        return GestureResult::Pending;
                    }
                    // Out of window or mismatch — treat as fresh first.
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.first_tap_button = Some(*button);
                    self.second_tap_position = None;
                    self.second_tap_time = None;
                    self.second_tap_button = None;
                    return GestureResult::Pending;
                }

                // No prior tap — record as first.
                self.first_tap_position = Some(*position);
                self.first_tap_time = Some(now);
                self.first_tap_button = Some(*button);
                self.second_tap_position = None;
                self.second_tap_time = None;
                self.second_tap_button = None;
                GestureResult::Pending
            }
        }
    }
}

impl Default for TripleTapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for TripleTapRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        self.process_at(event, Instant::now())
    }

    fn reset(&mut self) {
        self.first_tap_position = None;
        self.first_tap_time = None;
        self.first_tap_button = None;
        self.second_tap_position = None;
        self.second_tap_time = None;
        self.second_tap_button = None;
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        // Higher than DoubleTap so that when both would fire on the same
        // up event (shouldn't happen in practice — TripleTap only fires
        // after three taps and DoubleTap only at tap 2) TripleTap wins.
        20
    }

    fn resets_on_peer_recognition(&self) -> bool {
        // Cooperative with `DoubleTapRecognizer` — see the matching
        // override on DoubleTapRecognizer. The arena must not wipe our
        // accumulated first/second tap state when DoubleTap fires at
        // click 2, or click 3 would never promote us to TripleTap.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modifiers;
    use crate::gesture::test_helpers::*;

    // --- DoubleTapRecognizer ---

    #[test]
    fn double_tap_recognized_within_interval() {
        let mut rec = DoubleTapRecognizer::new().max_interval(Duration::from_millis(500));
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        // First tap
        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        // Second tap within interval
        rec.process_at(
            &down(Point::new(11.0, 10.0)),
            t0 + Duration::from_millis(200),
        );
        let result = rec.process_at(&up(Point::new(11.0, 10.0)), t0 + Duration::from_millis(250));
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DoubleTap(_))
        ));
    }

    #[test]
    fn double_tap_fails_if_too_slow() {
        let mut rec = DoubleTapRecognizer::new().max_interval(Duration::from_millis(300));
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        rec.process_at(&down(p), t0 + Duration::from_millis(400));
        let result = rec.process_at(&up(p), t0 + Duration::from_millis(450));
        // Should be Pending (treated as new first tap), not Recognized
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_fails_if_too_far() {
        let mut rec = DoubleTapRecognizer::new().max_distance(5.0);
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        // Second tap too far from first
        rec.process_at(
            &down(Point::new(50.0, 50.0)),
            t0 + Duration::from_millis(100),
        );
        let result = rec.process_at(&up(Point::new(50.0, 50.0)), t0 + Duration::from_millis(150));
        // Treated as new first tap
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_button_mismatch_fails_at_second_down() {
        // First tap Primary, second tap Secondary → no DoubleTap. The
        // second tap is recorded as a fresh first instead.
        let mut rec = DoubleTapRecognizer::new().accept_any_button();
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down_btn(p, PointerButton::Primary), t0);
        rec.process_at(
            &up_btn(p, PointerButton::Primary),
            t0 + Duration::from_millis(50),
        );

        rec.process_at(
            &down_btn(p, PointerButton::Secondary),
            t0 + Duration::from_millis(150),
        );
        let result = rec.process_at(
            &up_btn(p, PointerButton::Secondary),
            t0 + Duration::from_millis(200),
        );
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_carries_modifiers_from_second_up() {
        let mut rec = DoubleTapRecognizer::new();
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), t0);
        rec.process_at(&up(p), t0 + Duration::from_millis(50));

        rec.process_at(&down(p), t0 + Duration::from_millis(150));
        let result = rec.process_at(
            &up_full(p, PointerButton::Primary, Modifiers::SHIFT),
            t0 + Duration::from_millis(200),
        );
        match result {
            GestureResult::Recognized(GestureEvent::DoubleTap(event)) => {
                assert!(event.modifiers.shift());
            }
            other => panic!("expected DoubleTap with shift, got {:?}", other),
        }
    }

    // --- TripleTapRecognizer ---

    #[test]
    fn triple_tap_recognized_within_intervals() {
        let mut rec = TripleTapRecognizer::new();
        let t0 = Instant::now();
        let p = Point::new(10.0, 10.0);

        // Three taps all within window, all at (10, 10).
        for i in 0..3 {
            let offset = Duration::from_millis(200 * i as u64);
            rec.process_at(&down(p), t0 + offset);
            let result = rec.process_at(&up(p), t0 + offset + Duration::from_millis(50));
            if i < 2 {
                assert!(matches!(result, GestureResult::Pending));
            } else {
                assert!(matches!(
                    result,
                    GestureResult::Recognized(GestureEvent::TripleTap(_))
                ));
            }
        }
    }

    #[test]
    fn triple_tap_fails_if_third_is_too_slow() {
        let mut rec = TripleTapRecognizer::new().max_interval(Duration::from_millis(300));
        let t0 = Instant::now();
        let stamp = |ms| t0 + Duration::from_millis(ms);
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), stamp(0));
        rec.process_at(&up(p), stamp(50));
        rec.process_at(&down(p), stamp(200));
        rec.process_at(&up(p), stamp(250));

        // Third tap > 300 ms after the second — does not recognize.
        rec.process_at(&down(p), stamp(700));
        let result = rec.process_at(&up(p), stamp(750));
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn triple_tap_fails_if_third_is_too_far() {
        let mut rec = TripleTapRecognizer::new().max_distance(5.0);
        let t0 = Instant::now();
        let stamp = |ms| t0 + Duration::from_millis(ms);
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down(p), stamp(0));
        rec.process_at(&up(p), stamp(50));
        rec.process_at(&down(p), stamp(100));
        rec.process_at(&up(p), stamp(150));

        // Third tap > 5 px from the second.
        rec.process_at(&down(Point::new(30.0, 10.0)), stamp(200));
        let result = rec.process_at(&up(Point::new(30.0, 10.0)), stamp(250));
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn triple_tap_button_mismatch_fails_at_third_down() {
        // Third tap with a different button → no TripleTap. The
        // second-tap state collapses and the new tap becomes a fresh
        // first.
        let mut rec = TripleTapRecognizer::new().accept_any_button();
        let t0 = Instant::now();
        let stamp = |ms| t0 + Duration::from_millis(ms);
        let p = Point::new(10.0, 10.0);

        rec.process_at(&down_btn(p, PointerButton::Primary), stamp(0));
        rec.process_at(&up_btn(p, PointerButton::Primary), stamp(50));
        rec.process_at(&down_btn(p, PointerButton::Primary), stamp(150));
        rec.process_at(&up_btn(p, PointerButton::Primary), stamp(200));

        rec.process_at(&down_btn(p, PointerButton::Secondary), stamp(300));
        let result = rec.process_at(&up_btn(p, PointerButton::Secondary), stamp(350));
        assert!(matches!(result, GestureResult::Pending));
    }
}
