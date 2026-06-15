// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::Instant;

use bastyde_canvas::Point;

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, SwipeDirection};

/// Recognizes a swipe gesture (quick directional movement above velocity threshold).
#[derive(Debug)]
pub struct SwipeRecognizer {
    min_velocity: f32,
    min_distance: f32,
    max_cross_ratio: f32,
    down_position: Option<Point>,
    down_time: Option<Instant>,
}

impl SwipeRecognizer {
    pub fn new() -> Self {
        Self {
            min_velocity: 200.0,  // pixels per second
            min_distance: 30.0,   // minimum swipe distance
            max_cross_ratio: 0.5, // max perpendicular/parallel ratio
            down_position: None,
            down_time: None,
        }
    }

    pub fn min_velocity(mut self, v: f32) -> Self {
        self.min_velocity = v;
        self
    }

    pub fn min_distance(mut self, d: f32) -> Self {
        self.min_distance = d;
        self
    }

    /// Process with an explicit timestamp for testability.
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down { position, .. } => {
                self.down_position = Some(*position);
                self.down_time = Some(now);
                GestureResult::Pending
            }
            RawPointerEvent::Move { .. } => GestureResult::Pending,
            RawPointerEvent::Up { position, .. } => {
                let (Some(down), Some(time)) = (self.down_position, self.down_time) else {
                    return GestureResult::Failed;
                };

                let dx = position.x - down.x;
                let dy = position.y - down.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let elapsed = now.duration_since(time).as_secs_f32();

                if dist < self.min_distance || elapsed <= 0.0 {
                    self.reset();
                    return GestureResult::Failed;
                }

                let velocity = dist / elapsed;
                if velocity < self.min_velocity {
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
}

impl Default for SwipeRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for SwipeRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        self.process_at(event, Instant::now())
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
