// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::Point;

/// Only the unit tests still speak in wall-clock instants; the recognizers
/// themselves read `RecognizerContext::now`.
#[cfg(test)]
use std::time::Instant;

use crate::pointer::EventTime;

use super::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, RecognizerContext,
    SwipeDirection,
};

/// Recognizes a swipe gesture (quick directional movement above velocity threshold).
///
/// The velocity and distance it demands come from the active profile's
/// `swipe_min_velocity` / `swipe_min_distance` — 200 dp/s and 30 dp for a
/// mouse (exactly the pre-P06 constants), 300 dp/s and 40 dp for a finger,
/// whose travel is less precise — unless [`min_velocity`](Self::min_velocity) /
/// [`min_distance`](Self::min_distance) pin them. The cross-axis ratio is the
/// recognizer's own: it describes the shape of a swipe, not the device.
#[derive(Debug)]
pub struct SwipeRecognizer {
    min_velocity: Option<f32>,
    min_distance: Option<f32>,
    max_cross_ratio: f32,
    down_position: Option<Point>,
    down_time: Option<EventTime>,
    /// Anchors the `Instant` timeline the pre-P06 unit tests drive this with.
    #[cfg(test)]
    test_epoch: Option<std::time::Instant>,
}

impl SwipeRecognizer {
    pub fn new() -> Self {
        Self {
            min_velocity: None,
            min_distance: None,
            max_cross_ratio: 0.5, // max perpendicular/parallel ratio
            down_position: None,
            down_time: None,
            #[cfg(test)]
            test_epoch: None,
        }
    }

    /// Pin the velocity a swipe must reach, overriding the profile's
    /// `swipe_min_velocity`.
    pub fn min_velocity(mut self, v: f32) -> Self {
        self.min_velocity = Some(v);
        self
    }

    /// Pin the distance a swipe must cover, overriding the profile's
    /// `swipe_min_distance`.
    pub fn min_distance(mut self, d: f32) -> Self {
        self.min_distance = Some(d);
        self
    }
}

impl Default for SwipeRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for SwipeRecognizer {
    fn process(&mut self, event: &RawPointerEvent, cx: &RecognizerContext) -> GestureResult {
        let min_velocity = self.min_velocity.unwrap_or(cx.profile.swipe_min_velocity);
        let min_distance = self.min_distance.unwrap_or(cx.profile.swipe_min_distance);
        let now = cx.now;
        match event {
            RawPointerEvent::Down { position, .. } => {
                self.down_position = Some(*position);
                self.down_time = Some(now);
                GestureResult::Pending
            }
            RawPointerEvent::Move { .. } => GestureResult::Pending,
            RawPointerEvent::Cancel { .. } => {
                self.reset();
                GestureResult::Failed
            }
            RawPointerEvent::Up { position, .. } => {
                let (Some(down), Some(time)) = (self.down_position, self.down_time) else {
                    return GestureResult::Failed;
                };

                let dx = position.x - down.x;
                let dy = position.y - down.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let elapsed = now.saturating_since(time).as_secs_f32();

                if dist < min_distance || elapsed <= 0.0 {
                    self.reset();
                    return GestureResult::Failed;
                }

                let velocity = dist / elapsed;
                if velocity < min_velocity {
                    self.reset();
                    return GestureResult::Failed;
                }

                let abs_dx = dx.abs();
                let abs_dy = dy.abs();

                // Determine primary axis and check cross-axis ratio
                let (direction, cross_ratio) = if abs_dx >= abs_dy {
                    let dir = if dx > 0.0 {
                        SwipeDirection::Right
                    } else {
                        SwipeDirection::Left
                    };
                    (dir, abs_dy / abs_dx.max(0.001))
                } else {
                    let dir = if dy > 0.0 {
                        SwipeDirection::Down
                    } else {
                        SwipeDirection::Up
                    };
                    (dir, abs_dx / abs_dy.max(0.001))
                };

                if cross_ratio > self.max_cross_ratio {
                    self.reset();
                    return GestureResult::Failed;
                }

                self.reset();
                GestureResult::Recognized(GestureEvent::Swipe {
                    direction,
                    velocity,
                })
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_time = None;
    }

    fn priority(&self) -> u32 {
        30 // High priority — swipe is decisive
    }
}

#[cfg(test)]
impl SwipeRecognizer {
    /// The pre-P06 call shape: an `Instant` timeline anchored at the first
    /// call, mapped onto the [`EventTime`] one the recognizer now reads.
    fn process_at(&mut self, event: &RawPointerEvent, now: std::time::Instant) -> GestureResult {
        let epoch = *self.test_epoch.get_or_insert(now);
        let cx = super::config::RecognizerContext::new(
            EventTime::from_duration(now.saturating_duration_since(epoch)),
            teksilo_tokens::GestureProfile::MOUSE,
            teksilo_canvas::Rect::ZERO,
            event.pointer(),
        );
        GestureRecognizer::process(self, event, &cx)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::gesture::test_helpers::*;

    #[test]
    fn swipe_right_recognized() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(10.0, 50.0)), t0);
        let result = rec.process_at(
            &up(Point::new(200.0, 55.0)),
            t0 + Duration::from_millis(100),
        );
        match result {
            GestureResult::Recognized(GestureEvent::Swipe {
                direction,
                velocity,
            }) => {
                assert_eq!(direction, SwipeDirection::Right);
                assert!(velocity > 100.0);
            }
            other => panic!("Expected Swipe, got {:?}", other),
        }
    }

    #[test]
    fn swipe_left_recognized() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(200.0, 50.0)), t0);
        let result = rec.process_at(&up(Point::new(10.0, 55.0)), t0 + Duration::from_millis(100));
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::Swipe {
                direction: SwipeDirection::Left,
                ..
            })
        ));
    }

    #[test]
    fn swipe_fails_if_too_slow() {
        let mut rec = SwipeRecognizer::new().min_velocity(500.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(10.0, 10.0)), t0);
        let result = rec.process_at(
            &up(Point::new(50.0, 10.0)),
            t0 + Duration::from_secs(5), // Very slow
        );
        assert!(matches!(result, GestureResult::Failed));
    }

    #[test]
    fn swipe_fails_if_diagonal() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(&down(Point::new(10.0, 10.0)), t0);
        // Equal dx and dy — diagonal, cross_ratio = 1.0 > 0.5
        let result = rec.process_at(
            &up(Point::new(100.0, 100.0)),
            t0 + Duration::from_millis(100),
        );
        assert!(matches!(result, GestureResult::Failed));
    }
}
