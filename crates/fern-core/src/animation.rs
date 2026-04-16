//! Animation scheduler — drives `Signal<f32>` values smoothly over time.
//!
//! The scheduler stores active animations and advances them each frame.
//! It uses simulated time (for deterministic tests via `advance_time`)
//! or real time (for windowed apps).
//!
//! ## High-level API
//!
//! ```ignore
//! // From an event handler or build():
//! sidebar_width.animate_to(0.0, Duration::from_millis(200), Easing::EaseInOut);
//! ```
//!
//! This replaces the current value with a smooth interpolation to the target
//! over the given duration. The framework drives the animation automatically.

use std::time::{Duration, Instant};

use fern_tokens::Easing;

use crate::signal::Signal;

/// A pending animation request on a `Signal<f32>`.
///
/// Filled in by `Signal::animate_to()` and consumed by the widget tree's
/// `process_pending_animations` pass, which hands it to the scheduler.
#[derive(Debug, Clone)]
pub struct AnimationRequest {
    pub target: f32,
    pub duration: Duration,
    pub easing: Easing,
    pub frame_interval: Option<Duration>,
    /// If true, the animation loops: resets to the signal's current
    /// value each time it reaches `target`.
    pub looping: bool,
}

/// A single active animation driving a `Signal<f32>` from `start` to `end`.
struct ActiveAnimation {
    signal: Signal<f32>,
    start_value: f32,
    end_value: f32,
    start_time: Instant,
    duration: Duration,
    easing: Easing,
    frame_interval: Duration,
    next_tick: Instant,
    /// If true, the animation restarts from `start_value` when it
    /// reaches `end_value`, looping indefinitely. Stopped by
    /// `cancel()` or by dropping the signal.
    looping: bool,
}

/// Default frame interval for animations: ~30 fps. Smooth enough for
/// UI transitions while keeping CPU/GPU usage low. Individual
/// animations can override via `animate_with_frame_interval`.
const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Manages active animations and advances them each frame.
pub struct AnimationScheduler {
    animations: Vec<ActiveAnimation>,
}

impl AnimationScheduler {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
        }
    }

    /// Start animating a `Signal<f32>` from its current value to `target`.
    /// If the signal is already being animated, the previous animation is
    /// replaced (the current in-flight value becomes the new start).
    pub fn animate(
        &mut self,
        signal: &Signal<f32>,
        target: f32,
        duration: Duration,
        easing: Easing,
        now: Instant,
    ) {
        self.animate_with_frame_interval(signal, target, duration, easing, None, now);
    }

    pub fn animate_with_frame_interval(
        &mut self,
        signal: &Signal<f32>,
        target: f32,
        duration: Duration,
        easing: Easing,
        frame_interval: Option<Duration>,
        now: Instant,
    ) {
        let current = signal.get();
        self.cancel(signal);

        if (current - target).abs() < f32::EPSILON || duration.is_zero() {
            signal.set(target);
            signal.clear_animation_target();
            return;
        }

        self.animations.push(ActiveAnimation {
            signal: signal.clone(),
            start_value: current,
            end_value: target,
            start_time: now,
            duration,
            easing,
            frame_interval: frame_interval.unwrap_or(DEFAULT_FRAME_INTERVAL),
            next_tick: now,
            looping: false,
        });
    }

    /// Start a looping animation that cycles from `start` to `end`
    /// repeatedly. The signal resets to `start` each time it reaches
    /// `end`. Runs until cancelled.
    pub fn animate_looping(
        &mut self,
        signal: &Signal<f32>,
        start: f32,
        end: f32,
        period: Duration,
        easing: Easing,
        frame_interval: Option<Duration>,
        now: Instant,
    ) {
        self.cancel(signal);
        signal.set(start);

        self.animations.push(ActiveAnimation {
            signal: signal.clone(),
            start_value: start,
            end_value: end,
            start_time: now,
            duration: period,
            easing,
            frame_interval: frame_interval.unwrap_or(DEFAULT_FRAME_INTERVAL),
            next_tick: now,
            looping: true,
        });
    }

    /// Cancel any active animation on the given signal.
    pub fn cancel(&mut self, signal: &Signal<f32>) {
        self.animations.retain(|a| !Signal::same(&a.signal, signal));
    }

    /// Advance all active animations to the given time.
    /// Completed animations are removed. Returns true if any animation
    /// is still running (caller should request another frame).
    pub fn tick(&mut self, now: Instant) -> bool {
        self.animations.retain_mut(|anim| {
            if now < anim.next_tick {
                return true;
            }

            let elapsed = now.saturating_duration_since(anim.start_time);
            let t = if anim.duration.is_zero() {
                1.0
            } else {
                (elapsed.as_secs_f32() / anim.duration.as_secs_f32()).min(1.0)
            };
            let eased = anim.easing.apply(t);
            let value = fern_tokens::lerp(anim.start_value, anim.end_value, eased);
            anim.signal.set(value);

            if t >= 1.0 && anim.looping {
                // Restart the loop, carrying over any overshoot
                anim.start_time = now;
                anim.signal.set(anim.start_value);
                anim.next_tick = now + anim.frame_interval;
                true
            } else if t >= 1.0 {
                anim.signal.clear_animation_target();
                false
            } else {
                anim.next_tick = now + anim.frame_interval;
                true
            }
        });

        !self.animations.is_empty()
    }

    /// Whether any animation is currently active.
    pub fn has_active(&self) -> bool {
        !self.animations.is_empty()
    }

    /// The earliest deadline when the next animation tick is needed.
    /// Returns None if no animations are active.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.animations.iter().map(|anim| anim.next_tick).min()
    }

    /// Number of active animations (for testing/debugging).
    pub fn active_count(&self) -> usize {
        self.animations.len()
    }
}

impl Default for AnimationScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AnimationScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationScheduler")
            .field("active_count", &self.animations.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animate_from_current_to_target() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.tick(start);
        assert!((signal.get() - 0.0).abs() < 1.0);

        let has_more = scheduler.tick(start + Duration::from_millis(100));
        assert!(has_more);
        assert!((signal.get() - 50.0).abs() < 1.0);

        let has_more = scheduler.tick(start + Duration::from_millis(200));
        assert!(!has_more);
        assert!((signal.get() - 100.0).abs() < 0.01);
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn eased_animation() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            100.0,
            Duration::from_millis(200),
            Easing::EaseIn,
            start,
        );

        // At 50%, EaseIn (t²) gives 0.25, so value ≈ 25
        scheduler.tick(start + Duration::from_millis(100));
        assert!((signal.get() - 25.0).abs() < 1.0);
    }

    #[test]
    fn zero_duration_sets_immediately() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(&signal, 100.0, Duration::ZERO, Easing::Linear, start);
        assert_eq!(scheduler.active_count(), 0);
        assert!((signal.get() - 100.0).abs() < 0.01);
    }

    #[test]
    fn replace_existing_animation() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );

        scheduler.tick(start + Duration::from_millis(100));
        let mid_value = signal.get();
        assert!((mid_value - 50.0).abs() < 1.0);

        let mid_time = start + Duration::from_millis(100);
        scheduler.animate(
            &signal,
            0.0,
            Duration::from_millis(100),
            Easing::Linear,
            mid_time,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.tick(mid_time + Duration::from_millis(50));
        assert!((signal.get() - 25.0).abs() < 2.0);
    }

    #[test]
    fn cancel_stops_animation() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.cancel(&signal);
        assert_eq!(scheduler.active_count(), 0);

        assert!((signal.get() - 0.0).abs() < 0.01);
    }

    #[test]
    fn already_at_target_no_animation() {
        let signal = Signal::<f32>::new_animated(50.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            50.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn multiple_signals_animated_independently() {
        let a = Signal::<f32>::new_animated(0.0);
        let b = Signal::<f32>::new_animated(100.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(&a, 100.0, Duration::from_millis(200), Easing::Linear, start);
        scheduler.animate(&b, 0.0, Duration::from_millis(200), Easing::Linear, start);
        assert_eq!(scheduler.active_count(), 2);

        scheduler.tick(start + Duration::from_millis(100));
        assert!((a.get() - 50.0).abs() < 1.0);
        assert!((b.get() - 50.0).abs() < 1.0);

        scheduler.tick(start + Duration::from_millis(200));
        assert_eq!(scheduler.active_count(), 0);
        assert!((a.get() - 100.0).abs() < 0.01);
        assert!((b.get() - 0.0).abs() < 0.01);
    }

    #[test]
    fn looping_animation_restarts() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate_looping(
            &signal,
            0.0,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            None,
            start,
        );

        // At 50%: value ≈ 50
        scheduler.tick(start + Duration::from_millis(100));
        assert!((signal.get() - 50.0).abs() < 1.0);

        // At 100%: value resets to 0 (loop restarts)
        let has_more = scheduler.tick(start + Duration::from_millis(200));
        assert!(has_more, "looping animation should keep running");
        assert!(
            signal.get() < 5.0,
            "should have reset near 0, got {}",
            signal.get()
        );

        // Second loop at 50%: value ≈ 50 again
        scheduler.tick(start + Duration::from_millis(300));
        assert!((signal.get() - 50.0).abs() < 5.0);
    }

    #[test]
    fn looping_animation_cancelled() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate_looping(
            &signal,
            0.0,
            10.0,
            Duration::from_millis(100),
            Easing::Linear,
            None,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.cancel(&signal);
        assert_eq!(scheduler.active_count(), 0);
    }
}
