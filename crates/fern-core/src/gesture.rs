//! UIKit-style gesture recognizer model.
//!
//! Gesture recognizers are composable state machines attached to widgets.
//! Each recognizer monitors the raw pointer event stream and emits recognized
//! gestures when patterns complete. They are pure state machines with no
//! platform dependencies, making them trivially unit-testable.
//!
//! The [`GestureArena`] arbitrates when multiple recognizers compete on the
//! same event stream: all are fed in parallel, and when one recognizes, the
//! rest are reset.

use std::time::{Duration, Instant};

use fern_canvas::{Point, Vec2};

use crate::event::PointerButton;

/// Raw pointer events fed into gesture recognizers.
#[derive(Debug, Clone, Copy)]
pub enum RawPointerEvent {
    Down {
        position: Point,
        button: PointerButton,
    },
    Move {
        position: Point,
    },
    Up {
        position: Point,
        button: PointerButton,
    },
}

/// Result of processing a raw event through a gesture recognizer.
#[derive(Debug, Clone)]
pub enum GestureResult {
    /// Not enough data yet — keep feeding events.
    Pending,
    /// A gesture has been recognized.
    Recognized(GestureEvent),
    /// This event sequence cannot match the gesture — recognizer should be reset.
    Failed,
}

/// A recognized gesture event.
#[derive(Debug, Clone)]
pub enum GestureEvent {
    Tap { position: Point },
    DoubleTap { position: Point },
    LongPress { position: Point },
    DragStarted { position: Point, button: PointerButton },
    DragMoved { position: Point, delta: Vec2 },
    DragEnded { position: Point },
    PinchStarted { center: Point },
    PinchChanged { center: Point, scale: f32, rotation: f32 },
    PinchEnded,
    Swipe { direction: SwipeDirection, velocity: f32 },
}

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Trait for gesture recognizers. Each is a composable state machine.
pub trait GestureRecognizer {
    /// Feed a raw pointer event and return the recognition result.
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult;

    /// Reset the recognizer to its initial state.
    fn reset(&mut self);

    /// Priority for arbitration when multiple recognizers compete.
    /// Higher priority wins.
    fn priority(&self) -> u32;
}

// ---------------------------------------------------------------------------
// TapRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a single tap (pointer down + up without significant movement).
#[derive(Debug)]
pub struct TapRecognizer {
    max_distance: f32,
    down_position: Option<Point>,
}

impl TapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 5.0,
            down_position: None,
        }
    }

    pub fn max_distance(mut self, d: f32) -> Self {
        self.max_distance = d;
        self
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
            RawPointerEvent::Down { position, .. } => {
                self.down_position = Some(*position);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position {
                    let dx = position.x - down.x;
                    let dy = position.y - down.y;
                    if (dx * dx + dy * dy).sqrt() > self.max_distance {
                        return GestureResult::Failed;
                    }
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up { position, .. } => {
                if let Some(down) = self.down_position {
                    let dx = position.x - down.x;
                    let dy = position.y - down.y;
                    if (dx * dx + dy * dy).sqrt() <= self.max_distance {
                        return GestureResult::Recognized(GestureEvent::Tap {
                            position: *position,
                        });
                    }
                }
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
    }

    fn priority(&self) -> u32 {
        10
    }
}

// ---------------------------------------------------------------------------
// DoubleTapRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a double-tap (two taps within a time window and distance).
#[derive(Debug)]
pub struct DoubleTapRecognizer {
    max_distance: f32,
    max_interval: Duration,
    first_tap_position: Option<Point>,
    first_tap_time: Option<Instant>,
    down_position: Option<Point>,
}

impl DoubleTapRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 10.0,
            max_interval: Duration::from_millis(300),
            first_tap_position: None,
            first_tap_time: None,
            down_position: None,
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

    /// Feed an event with an explicit timestamp (for testability without real clocks).
    pub fn process_at(&mut self, event: &RawPointerEvent, now: Instant) -> GestureResult {
        match event {
            RawPointerEvent::Down { position, .. } => {
                self.down_position = Some(*position);
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position {
                    if distance(*position, down) > self.max_distance {
                        return GestureResult::Failed;
                    }
                }
                GestureResult::Pending
            }
            RawPointerEvent::Up { position, .. } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Failed;
                };
                if distance(*position, down) > self.max_distance {
                    return GestureResult::Failed;
                }

                if let (Some(first_pos), Some(first_time)) =
                    (self.first_tap_position, self.first_tap_time)
                {
                    // Second tap — check distance from first and time interval
                    if distance(*position, first_pos) <= self.max_distance
                        && now.duration_since(first_time) <= self.max_interval
                    {
                        self.reset();
                        return GestureResult::Recognized(GestureEvent::DoubleTap {
                            position: *position,
                        });
                    }
                    // Too far/too slow — treat this as a new first tap
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.down_position = None;
                    GestureResult::Pending
                } else {
                    // First tap — record and wait for second
                    self.first_tap_position = Some(*position);
                    self.first_tap_time = Some(now);
                    self.down_position = None;
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
        self.down_position = None;
    }

    fn priority(&self) -> u32 {
        15 // Higher than tap — double-tap should win over single tap
    }
}

// ---------------------------------------------------------------------------
// LongPressRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a long press (pointer held down beyond a duration without movement).
///
/// Because recognizers are pure state machines, the caller must drive time
/// by calling [`check_timeout`] when the timer fires (e.g. from an event-loop
/// timer). The recognizer itself does not spawn timers.
#[derive(Debug)]
pub struct LongPressRecognizer {
    max_distance: f32,
    min_duration: Duration,
    down_position: Option<Point>,
    down_time: Option<Instant>,
    recognized: bool,
}

impl LongPressRecognizer {
    pub fn new() -> Self {
        Self {
            max_distance: 5.0,
            min_duration: Duration::from_millis(500),
            down_position: None,
            down_time: None,
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

    /// Check if the long press timer has expired. Called externally
    /// (e.g. from the event loop timer callback).
    pub fn check_timeout(&mut self, now: Instant) -> GestureResult {
        if self.recognized {
            return GestureResult::Pending;
        }
        if let (Some(pos), Some(time)) = (self.down_position, self.down_time) {
            if now.duration_since(time) >= self.min_duration {
                self.recognized = true;
                return GestureResult::Recognized(GestureEvent::LongPress { position: pos });
            }
        }
        GestureResult::Pending
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
            RawPointerEvent::Down { position, .. } => {
                self.down_position = Some(*position);
                self.down_time = Some(Instant::now());
                self.recognized = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                if let Some(down) = self.down_position {
                    if distance(*position, down) > self.max_distance {
                        return GestureResult::Failed;
                    }
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
        self.recognized = false;
    }

    fn priority(&self) -> u32 {
        25 // Higher than drag — long press wins over drag
    }
}

// ---------------------------------------------------------------------------
// DragRecognizer
// ---------------------------------------------------------------------------

/// Recognizes a drag gesture (pointer down + move beyond threshold).
#[derive(Debug)]
pub struct DragRecognizer {
    threshold: f32,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
    dragging: bool,
    last_position: Option<Point>,
}

impl DragRecognizer {
    pub fn new() -> Self {
        Self {
            threshold: 5.0,
            down_position: None,
            down_button: None,
            dragging: false,
            last_position: None,
        }
    }

    pub fn threshold(mut self, t: f32) -> Self {
        self.threshold = t;
        self
    }
}

impl Default for DragRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for DragRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down { position, button } => {
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                self.last_position = Some(*position);
                self.dragging = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Pending;
                };

                if self.dragging {
                    let last = self.last_position.unwrap_or(down);
                    let delta = Vec2::new(position.x - last.x, position.y - last.y);
                    self.last_position = Some(*position);
                    return GestureResult::Recognized(GestureEvent::DragMoved {
                        position: *position,
                        delta,
                    });
                }

                let dx = position.x - down.x;
                let dy = position.y - down.y;
                if (dx * dx + dy * dy).sqrt() >= self.threshold {
                    self.dragging = true;
                    self.last_position = Some(*position);
                    return GestureResult::Recognized(GestureEvent::DragStarted {
                        position: *position,
                        button: self.down_button.unwrap_or(PointerButton::Primary),
                    });
                }

                GestureResult::Pending
            }
            RawPointerEvent::Up { position, .. } => {
                if self.dragging {
                    self.dragging = false;
                    self.down_position = None;
                    return GestureResult::Recognized(GestureEvent::DragEnded {
                        position: *position,
                    });
                }
                self.down_position = None;
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
        self.dragging = false;
        self.last_position = None;
    }

    fn priority(&self) -> u32 {
        20
    }
}

// ---------------------------------------------------------------------------
// SwipeRecognizer
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// GestureArena — multi-recognizer arbitration
// ---------------------------------------------------------------------------

/// Arbitrates among multiple gesture recognizers competing on the same event
/// stream. All recognizers are fed each event in parallel. When one recognizes,
/// the others are reset. Failed recognizers are excluded from future events
/// until the next sequence (pointer up resets all).
pub struct GestureArena {
    entries: Vec<ArenaEntry>,
}

struct ArenaEntry {
    recognizer: Box<dyn GestureRecognizer>,
    failed: bool,
}

impl GestureArena {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a recognizer to the arena.
    pub fn add(&mut self, recognizer: impl GestureRecognizer + 'static) {
        self.entries.push(ArenaEntry {
            recognizer: Box::new(recognizer),
            failed: false,
        });
    }

    /// Feed a raw pointer event to all active recognizers.
    ///
    /// Returns the recognized gesture from the highest-priority recognizer,
    /// or `None` if no gesture was recognized yet.
    pub fn process(&mut self, event: &RawPointerEvent) -> Option<GestureEvent> {
        // On pointer down, reset all failure states for a fresh sequence
        if matches!(event, RawPointerEvent::Down { .. }) {
            for entry in &mut self.entries {
                entry.failed = false;
                entry.recognizer.reset();
            }
        }

        let mut best: Option<(usize, u32, GestureEvent)> = None;

        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.failed {
                continue;
            }
            match entry.recognizer.process(event) {
                GestureResult::Recognized(gesture) => {
                    let prio = entry.recognizer.priority();
                    if best.as_ref().map_or(true, |(_, bp, _)| prio > *bp) {
                        best = Some((i, prio, gesture));
                    }
                }
                GestureResult::Failed => {
                    entry.failed = true;
                }
                GestureResult::Pending => {}
            }
        }

        if let Some((winner_idx, _, _)) = &best {
            // Winner recognized — reset all non-winning recognizers.
            // The winner keeps its state (important for multi-event gestures
            // like drag, which fire DragStart then DragUpdate then DragEnd).
            for (i, entry) in self.entries.iter_mut().enumerate() {
                if i != *winner_idx && !entry.failed {
                    entry.recognizer.reset();
                    entry.failed = false;
                }
            }
        }

        best.map(|(_, _, gesture)| gesture)
    }

    /// Reset all recognizers in the arena.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            entry.recognizer.reset();
            entry.failed = false;
        }
    }

    /// Returns true if the arena has any recognizers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of recognizers in the arena.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for GestureArena {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArena")
            .field("num_recognizers", &self.entries.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- TapRecognizer ---

    #[test]
    fn tap_recognized_on_quick_down_up() {
        let mut rec = TapRecognizer::new();
        assert!(matches!(
            rec.process(&RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            }),
            GestureResult::Pending
        ));
        assert!(matches!(
            rec.process(&RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            }),
            GestureResult::Recognized(GestureEvent::Tap { .. })
        ));
    }

    #[test]
    fn tap_fails_if_moved_too_far() {
        let mut rec = TapRecognizer::new().max_distance(5.0);
        rec.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(
            rec.process(&RawPointerEvent::Move {
                position: Point::new(20.0, 10.0),
            }),
            GestureResult::Failed
        ));
    }

    // --- DoubleTapRecognizer ---

    #[test]
    fn double_tap_recognized_within_interval() {
        let mut rec = DoubleTapRecognizer::new().max_interval(Duration::from_millis(500));
        let t0 = Instant::now();

        // First tap
        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(50),
        );

        // Second tap within interval
        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(11.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(200),
        );
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(11.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(250),
        );
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DoubleTap { .. })
        ));
    }

    #[test]
    fn double_tap_fails_if_too_slow() {
        let mut rec = DoubleTapRecognizer::new().max_interval(Duration::from_millis(300));
        let t0 = Instant::now();

        // First tap
        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(50),
        );

        // Second tap too late
        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(400),
        );
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(450),
        );
        // Should be Pending (treated as new first tap), not Recognized
        assert!(matches!(result, GestureResult::Pending));
    }

    #[test]
    fn double_tap_fails_if_too_far() {
        let mut rec = DoubleTapRecognizer::new().max_distance(5.0);
        let t0 = Instant::now();

        // First tap
        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(50),
        );

        // Second tap too far from first
        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(50.0, 50.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(100),
        );
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(50.0, 50.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(150),
        );
        // Treated as new first tap
        assert!(matches!(result, GestureResult::Pending));
    }

    // --- LongPressRecognizer ---

    #[test]
    fn long_press_recognized_after_timeout() {
        let mut rec = LongPressRecognizer::new().min_duration(Duration::from_millis(500));
        rec.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });

        let down_time = rec.down_time.unwrap();

        // Not yet
        assert!(matches!(
            rec.check_timeout(down_time + Duration::from_millis(200)),
            GestureResult::Pending
        ));

        // Now!
        assert!(matches!(
            rec.check_timeout(down_time + Duration::from_millis(600)),
            GestureResult::Recognized(GestureEvent::LongPress { .. })
        ));
    }

    #[test]
    fn long_press_fails_on_movement() {
        let mut rec = LongPressRecognizer::new().max_distance(5.0);
        rec.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(
            rec.process(&RawPointerEvent::Move {
                position: Point::new(30.0, 30.0),
            }),
            GestureResult::Failed
        ));
    }

    #[test]
    fn long_press_fails_on_early_up() {
        let mut rec = LongPressRecognizer::new();
        rec.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(
            rec.process(&RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            }),
            GestureResult::Failed
        ));
    }

    // --- DragRecognizer ---

    #[test]
    fn drag_recognizer_fires_after_threshold() {
        let mut rec = DragRecognizer::new().threshold(5.0);

        assert!(matches!(
            rec.process(&RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            }),
            GestureResult::Pending
        ));

        // Small move — still pending
        assert!(matches!(
            rec.process(&RawPointerEvent::Move {
                position: Point::new(12.0, 10.0),
            }),
            GestureResult::Pending
        ));

        // Large move — drag started
        assert!(matches!(
            rec.process(&RawPointerEvent::Move {
                position: Point::new(20.0, 10.0),
            }),
            GestureResult::Recognized(GestureEvent::DragStarted { .. })
        ));
    }

    #[test]
    fn drag_emits_moved_and_ended() {
        let mut rec = DragRecognizer::new().threshold(1.0);
        rec.process(&RawPointerEvent::Down {
            position: Point::new(0.0, 0.0),
            button: PointerButton::Primary,
        });
        // Cross threshold
        rec.process(&RawPointerEvent::Move {
            position: Point::new(10.0, 0.0),
        });

        // Subsequent move
        let result = rec.process(&RawPointerEvent::Move {
            position: Point::new(15.0, 0.0),
        });
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DragMoved { .. })
        ));

        // Release
        let result = rec.process(&RawPointerEvent::Up {
            position: Point::new(15.0, 0.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DragEnded { .. })
        ));
    }

    #[test]
    fn drag_fails_on_up_without_movement() {
        let mut rec = DragRecognizer::new().threshold(5.0);
        rec.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(
            rec.process(&RawPointerEvent::Up {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            }),
            GestureResult::Failed
        ));
    }

    #[test]
    fn reset_clears_state() {
        let mut rec = DragRecognizer::new().threshold(1.0);
        rec.process(&RawPointerEvent::Down {
            position: Point::new(0.0, 0.0),
            button: PointerButton::Primary,
        });
        rec.process(&RawPointerEvent::Move {
            position: Point::new(10.0, 0.0),
        });
        rec.reset();

        // After reset, move without down should be pending (not drag)
        assert!(matches!(
            rec.process(&RawPointerEvent::Move {
                position: Point::new(20.0, 0.0),
            }),
            GestureResult::Pending
        ));
    }

    // --- SwipeRecognizer ---

    #[test]
    fn swipe_right_recognized() {
        let mut rec = SwipeRecognizer::new()
            .min_velocity(100.0)
            .min_distance(20.0);
        let t0 = Instant::now();

        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 50.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(200.0, 55.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(100),
        );
        match result {
            GestureResult::Recognized(GestureEvent::Swipe { direction, velocity }) => {
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

        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(200.0, 50.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(10.0, 55.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(100),
        );
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

        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(50.0, 10.0),
                button: PointerButton::Primary,
            },
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

        rec.process_at(
            &RawPointerEvent::Down {
                position: Point::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            t0,
        );
        // Equal dx and dy — diagonal, cross_ratio = 1.0 > 0.5
        let result = rec.process_at(
            &RawPointerEvent::Up {
                position: Point::new(100.0, 100.0),
                button: PointerButton::Primary,
            },
            t0 + Duration::from_millis(100),
        );
        assert!(matches!(result, GestureResult::Failed));
    }

    // --- GestureArena ---

    #[test]
    fn arena_tap_wins_over_nothing() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new());

        arena.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        let result = arena.process(&RawPointerEvent::Up {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(result, Some(GestureEvent::Tap { .. })));
    }

    #[test]
    fn arena_drag_wins_over_tap_on_movement() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new().max_distance(5.0));
        arena.add(DragRecognizer::new().threshold(5.0));

        arena.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });

        // Move beyond both thresholds — drag recognized (higher priority)
        let result = arena.process(&RawPointerEvent::Move {
            position: Point::new(30.0, 10.0),
        });
        assert!(matches!(result, Some(GestureEvent::DragStarted { .. })));
    }

    #[test]
    fn arena_tap_recognized_when_drag_fails() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new());
        arena.add(DragRecognizer::new().threshold(5.0));

        arena.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });

        // Up without significant movement — drag fails, tap wins
        let result = arena.process(&RawPointerEvent::Up {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(result, Some(GestureEvent::Tap { .. })));
    }

    #[test]
    fn arena_resets_on_new_down() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new().max_distance(5.0));

        // First sequence: fail the tap by moving
        arena.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        arena.process(&RawPointerEvent::Move {
            position: Point::new(50.0, 50.0),
        });

        // New sequence: should work fresh
        arena.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        let result = arena.process(&RawPointerEvent::Up {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(matches!(result, Some(GestureEvent::Tap { .. })));
    }

    #[test]
    fn arena_empty_returns_none() {
        let mut arena = GestureArena::new();
        assert!(arena.is_empty());
        let result = arena.process(&RawPointerEvent::Down {
            position: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
        });
        assert!(result.is_none());
    }
}
