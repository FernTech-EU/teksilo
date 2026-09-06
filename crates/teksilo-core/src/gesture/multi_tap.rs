// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::Duration;

/// Only the unit tests still speak in wall-clock instants; the recognizers
/// themselves read `RecognizerContext::now`.
#[cfg(test)]
use std::time::Instant;

use teksilo_canvas::Point;

use crate::event::{ButtonMask, PointerButton};

use super::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, RecognizerContext, TapEvent,
    distance,
};

/// Recognizes a double-tap (two taps within a time window and distance,
/// using the same button).
///
/// Default `accept` is [`ButtonMask::PRIMARY`]; presses on other buttons
/// are ignored. Within the recognized sequence, both taps must match
/// the press button — a `Primary` then `Secondary` sequence resets to
/// the new tap as a fresh "first" rather than firing `DoubleTap`.
///
/// # Where the count lives
///
/// The recognizer does **not** count taps. The count is the node's
/// [`TapStreak`](super::TapStreak), read through
/// [`RecognizerContext::streak`], because on a touchscreen the second tap is a
/// different [`PointerId`](crate::pointer::PointerId) — a different contact and
/// therefore a different arena — and a count kept here would be destroyed
/// between the two taps. This recognizer contributes the *within-tap* rules
/// (accept mask, button match, press-to-release travel) and fires when the
/// streak reaches two.
#[derive(Debug)]
pub struct DoubleTapRecognizer {
    max_distance: Option<f32>,
    max_interval: Option<Duration>,
    accept: ButtonMask,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
    /// Anchors the `Instant` timeline the pre-P06 unit tests drive this with.
    #[cfg(test)]
    test_state: TestDriver,
}

impl DoubleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: None,
            max_interval: None,
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_button: None,
            #[cfg(test)]
            test_state: TestDriver::default(),
        }
    }

    /// Pin the travel a tap of the pair tolerates, overriding the profile's
    /// `multi_tap_slop`.
    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = Some(d);
        self
    }

    /// Pin the gap the pair tolerates, overriding the profile's
    /// `multi_tap_interval`.
    ///
    /// An override can only ever **narrow** the window: the node streak that
    /// counts the taps is shared by every recognizer on the node and uses the
    /// profile.
    pub fn max_interval(mut self, interval: Duration) -> Self {
        self.max_interval = Some(interval);
        self
    }

    fn slop(&self, cx: &RecognizerContext) -> f32 {
        self.max_distance.unwrap_or(cx.profile.multi_tap_slop)
    }

    fn interval(&self, cx: &RecognizerContext) -> Duration {
        self.max_interval.unwrap_or(cx.profile.multi_tap_interval)
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

impl Default for DoubleTapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for DoubleTapRecognizer {
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
                    && distance(*position, down) > self.slop(cx)
                {
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
                if distance(*position, down) > self.slop(cx) {
                    return GestureResult::Failed;
                }

                // The node streak has already counted this release (the arena
                // set advances it before the recognizers run). Two means the
                // pair just completed — subject to this instance's own, always
                // tighter, overrides.
                if cx.streak.count() == 2
                    && cx.streak.since_previous() <= self.interval(cx)
                    && cx.streak.travel_from_previous() <= self.slop(cx)
                {
                    return GestureResult::Recognized(GestureEvent::DoubleTap(TapEvent {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                        pointer: *pointer,
                    }));
                }
                GestureResult::Pending
            }
            RawPointerEvent::Cancel { .. } => {
                self.reset();
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
    }

    fn tap_family(&self) -> bool {
        true
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
/// Defaults match `DoubleTapRecognizer` (the profile's `multi_tap_interval` /
/// `multi_tap_slop`, and `Primary` only) so the two fire as a natural
/// escalating pair. Mixed-button sequences restart the streak.
///
/// Like [`DoubleTapRecognizer`], it does not count the taps itself — see that
/// type's "Where the count lives".
#[derive(Debug)]
pub struct TripleTapRecognizer {
    max_distance: Option<f32>,
    max_interval: Option<Duration>,
    accept: ButtonMask,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
    /// Anchors the `Instant` timeline the pre-P06 unit tests drive this with.
    #[cfg(test)]
    test_state: TestDriver,
}

impl TripleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: None,
            max_interval: None,
            accept: ButtonMask::PRIMARY,
            down_position: None,
            down_button: None,
            #[cfg(test)]
            test_state: TestDriver::default(),
        }
    }

    /// Pin the travel a tap of the run tolerates, overriding the profile's
    /// `multi_tap_slop`.
    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = Some(d);
        self
    }

    /// Pin the gap the run tolerates, overriding the profile's
    /// `multi_tap_interval`. Narrowing only — see
    /// [`DoubleTapRecognizer::max_interval`].
    pub fn max_interval(mut self, interval: Duration) -> Self {
        self.max_interval = Some(interval);
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

    fn slop(&self, cx: &RecognizerContext) -> f32 {
        self.max_distance.unwrap_or(cx.profile.multi_tap_slop)
    }

    fn interval(&self, cx: &RecognizerContext) -> Duration {
        self.max_interval.unwrap_or(cx.profile.multi_tap_interval)
    }
}

impl Default for TripleTapRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for TripleTapRecognizer {
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
                    && distance(*position, down) > self.slop(cx)
                {
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
                if distance(*position, down) > self.slop(cx) {
                    return GestureResult::Failed;
                }

                if cx.streak.count() == 3
                    && cx.streak.since_previous() <= self.interval(cx)
                    && cx.streak.travel_from_previous() <= self.slop(cx)
                {
                    return GestureResult::Recognized(GestureEvent::TripleTap(TapEvent {
                        position: *position,
                        button: *button,
                        modifiers: *modifiers,
                        pointer: *pointer,
                    }));
                }
                GestureResult::Pending
            }
            RawPointerEvent::Cancel { .. } => {
                self.reset();
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
    }

    fn priority(&self) -> u32 {
        // Higher than DoubleTap so that when both would fire on the same
        // up event (shouldn't happen in practice — TripleTap only fires
        // after three taps and DoubleTap only at tap 2) TripleTap wins.
        20
    }

    fn tap_family(&self) -> bool {
        true
    }

    fn resets_on_peer_recognition(&self) -> bool {
        // Cooperative with `DoubleTapRecognizer` — see the matching
        // override on DoubleTapRecognizer. The arena must not wipe our
        // in-flight press state when DoubleTap fires at click 2.
        false
    }
}

/// The per-instance state the pre-P06 `process_at` call shape needs: a
/// standalone [`TapStreak`](super::TapStreak) standing in for the node's, and
/// an anchor mapping the tests' `Instant` timeline onto the `EventTime` one.
#[cfg(test)]
#[derive(Debug, Default)]
struct TestDriver {
    epoch: Option<std::time::Instant>,
    streak: super::TapStreak,
    contact: super::config::TapContact,
}

/// Drive one recognizer the way a [`GestureArenaSet`](super::GestureArenaSet)
/// would — advance the streak on a qualifying release, then feed the event —
/// but against the driver's own streak instead of a node's.
///
/// Deliberately routed through the same [`TapStreak::advance`] and
/// [`TapContact`](super::config::TapContact) the real path uses, so these tests check
/// the shipped rule rather than a re-implementation of it.
#[cfg(test)]
fn drive_standalone<R: GestureRecognizer + ?Sized>(
    rec: &mut R,
    driver: &mut TestDriver,
    event: &RawPointerEvent,
    now: std::time::Instant,
) -> GestureResult {
    let epoch = *driver.epoch.get_or_insert(now);
    let now = crate::pointer::EventTime::from_duration(now.saturating_duration_since(epoch));
    let profile = teksilo_tokens::GestureProfile::MOUSE;
    if let Some((press, button)) = driver.contact.observe(event, &profile) {
        driver.streak.advance(now, &profile, press, button);
    }
    let base = RecognizerContext::new(now, profile, teksilo_canvas::Rect::ZERO, event.pointer());
    let cx = base.with_streak(&driver.streak);
    rec.process(event, &cx)
}

#[cfg(test)]
impl DoubleTapRecognizer {
    /// Feed an event with an explicit timestamp — the pre-P06 call shape.
    fn process_at(&mut self, event: &RawPointerEvent, now: std::time::Instant) -> GestureResult {
        let mut driver = std::mem::take(&mut self.test_state);
        let result = drive_standalone(self, &mut driver, event, now);
        self.test_state = driver;
        result
    }
}

#[cfg(test)]
impl TripleTapRecognizer {
    /// Feed an event with an explicit timestamp — the pre-P06 call shape.
    fn process_at(&mut self, event: &RawPointerEvent, now: std::time::Instant) -> GestureResult {
        let mut driver = std::mem::take(&mut self.test_state);
        let result = drive_standalone(self, &mut driver, event, now);
        self.test_state = driver;
        result
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
