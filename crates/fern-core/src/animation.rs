//! Animation scheduler — drives `State<f32>` values smoothly over time.
//!
//! The scheduler stores active animations and advances them each frame.
//! It uses simulated time (for deterministic tests via `advance_time`)
//! or real time (for windowed apps).
//!
//! ## High-level API
//!
//! ```ignore
//! // From an event handler or build():
//! sidebar_width.set_animated(0.0, Duration::from_millis(200), Easing::EaseInOut);
//! ```
//!
//! This replaces the current value with a smooth interpolation to the target
//! over the given duration. The framework drives the animation automatically.

use std::time::{Duration, Instant};

use fern_tokens::Easing;

use crate::signal::Signal;
use crate::state::State;

/// A single active animation driving a `State<f32>` from `start` to `end`.
struct ActiveAnimation {
    /// The state being animated.
    state: State<f32>,
    /// Value at animation start.
    start_value: f32,
    /// Target value.
    end_value: f32,
    /// When the animation started (simulated time).
    start_time: Instant,
    /// Total animation duration.
    duration: Duration,
    /// Easing curve.
    easing: Easing,
    /// Minimum interval between animation updates.
    frame_interval: Duration,
    /// Deadline for the next animation update.
    next_tick: Instant,
}

/// A single active animation driving a `Signal<f32>` from `start` to `end`.
struct ActiveSignalAnimation {
    signal: Signal<f32>,
    start_value: f32,
    end_value: f32,
    start_time: Instant,
    duration: Duration,
    easing: Easing,
    frame_interval: Duration,
    next_tick: Instant,
}

const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Manages active animations and advances them each frame.
pub struct AnimationScheduler {
    animations: Vec<ActiveAnimation>,
    signal_animations: Vec<ActiveSignalAnimation>,
}

impl AnimationScheduler {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
            signal_animations: Vec::new(),
        }
    }

    /// Start animating a `State<f32>` from its current value to `target`.
    /// If the state is already being animated, the previous animation is
    /// replaced (the current in-flight value becomes the new start).
    pub fn animate(
        &mut self,
        state: &State<f32>,
        target: f32,
        duration: Duration,
        easing: Easing,
        now: Instant,
    ) {
        self.animate_with_frame_interval(state, target, duration, easing, None, now);
    }

    pub fn animate_with_frame_interval(
        &mut self,
        state: &State<f32>,
        target: f32,
        duration: Duration,
        easing: Easing,
        frame_interval: Option<Duration>,
        now: Instant,
    ) {
        let current = *state.get();

        // Remove any existing animation for this state
        self.cancel(state);

        // Don't animate if already at target or zero duration
        if (current - target).abs() < f32::EPSILON || duration.is_zero() {
            state.set(target);
            state.clear_animation_target();
            return;
        }

        self.animations.push(ActiveAnimation {
            state: state.clone(),
            start_value: current,
            end_value: target,
            start_time: now,
            duration,
            easing,
            frame_interval: frame_interval.unwrap_or(DEFAULT_FRAME_INTERVAL),
            next_tick: now,
        });
    }

    /// Cancel any active animation on the given state.
    pub fn cancel(&mut self, state: &State<f32>) {
        self.animations.retain(|a| !State::same(&a.state, state));
    }

    /// Start animating a `Signal<f32>` from its current value to `target`.
    pub fn animate_signal(
        &mut self,
        signal: &Signal<f32>,
        target: f32,
        duration: Duration,
        easing: Easing,
        now: Instant,
    ) {
        self.animate_signal_with_frame_interval(signal, target, duration, easing, None, now);
    }

    pub fn animate_signal_with_frame_interval(
        &mut self,
        signal: &Signal<f32>,
        target: f32,
        duration: Duration,
        easing: Easing,
        frame_interval: Option<Duration>,
        now: Instant,
    ) {
        let current = signal.get();
        self.cancel_signal(signal);

        if (current - target).abs() < f32::EPSILON || duration.is_zero() {
            signal.set(target);
            signal.clear_animation_target();
            return;
        }

        self.signal_animations.push(ActiveSignalAnimation {
            signal: signal.clone(),
            start_value: current,
            end_value: target,
            start_time: now,
            duration,
            easing,
            frame_interval: frame_interval.unwrap_or(DEFAULT_FRAME_INTERVAL),
            next_tick: now,
        });
    }

    /// Cancel any active animation on the given signal.
    pub fn cancel_signal(&mut self, signal: &Signal<f32>) {
        self.signal_animations
            .retain(|a| !Signal::same(&a.signal, signal));
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
            anim.state.set(value);

            let keep = t < 1.0;
            if !keep {
                anim.state.clear_animation_target();
            } else {
                anim.next_tick = now + anim.frame_interval;
            }
            keep
        });

        self.signal_animations.retain_mut(|anim| {
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

            let keep = t < 1.0;
            if !keep {
                anim.signal.clear_animation_target();
            } else {
                anim.next_tick = now + anim.frame_interval;
            }
            keep
        });

        !self.animations.is_empty() || !self.signal_animations.is_empty()
    }

    /// Whether any animation is currently active.
    pub fn has_active(&self) -> bool {
        !self.animations.is_empty() || !self.signal_animations.is_empty()
    }

    /// The earliest deadline when the next animation tick is needed.
    /// Returns None if no animations are active.
    /// Targets ~60fps by scheduling the next tick 16ms from now.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.animations
            .iter()
            .map(|anim| anim.next_tick)
            .chain(self.signal_animations.iter().map(|anim| anim.next_tick))
            .min()
    }

    /// Number of active animations (for testing/debugging).
    pub fn active_count(&self) -> usize {
        self.animations.len() + self.signal_animations.len()
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
        let state = State::new(0.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &state,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        // At t=0: value should still be 0 (or very close)
        scheduler.tick(start);
        assert!((*state.get() - 0.0).abs() < 1.0);

        // At t=100ms (50%): value should be ~50
        let has_more = scheduler.tick(start + Duration::from_millis(100));
        assert!(has_more);
        assert!((*state.get() - 50.0).abs() < 1.0);

        // At t=200ms (100%): value should be 100, animation complete
        let has_more = scheduler.tick(start + Duration::from_millis(200));
        assert!(!has_more);
        assert!((*state.get() - 100.0).abs() < 0.01);
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn eased_animation() {
        let state = State::new(0.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &state,
            100.0,
            Duration::from_millis(200),
            Easing::EaseIn,
            start,
        );

        // At 50%, EaseIn (t²) gives 0.25, so value ≈ 25
        scheduler.tick(start + Duration::from_millis(100));
        assert!((*state.get() - 25.0).abs() < 1.0);
    }

    #[test]
    fn zero_duration_sets_immediately() {
        let state = State::new(0.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(&state, 100.0, Duration::ZERO, Easing::Linear, start);
        assert_eq!(scheduler.active_count(), 0); // no animation created
        assert!((*state.get() - 100.0).abs() < 0.01); // value set immediately
    }

    #[test]
    fn replace_existing_animation() {
        let state = State::new(0.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &state,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );

        // Advance to 50%
        scheduler.tick(start + Duration::from_millis(100));
        let mid_value = *state.get();
        assert!((mid_value - 50.0).abs() < 1.0);

        // Start a new animation from the current mid-value to 0
        let mid_time = start + Duration::from_millis(100);
        scheduler.animate(
            &state,
            0.0,
            Duration::from_millis(100),
            Easing::Linear,
            mid_time,
        );
        assert_eq!(scheduler.active_count(), 1); // old one replaced

        // At 50% of the new animation
        scheduler.tick(mid_time + Duration::from_millis(50));
        assert!((*state.get() - 25.0).abs() < 2.0); // ~25 (midpoint of 50→0)
    }

    #[test]
    fn cancel_stops_animation() {
        let state = State::new(0.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &state,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.cancel(&state);
        assert_eq!(scheduler.active_count(), 0);

        // Value stays where it was
        assert!((*state.get() - 0.0).abs() < 0.01);
    }

    #[test]
    fn already_at_target_no_animation() {
        let state = State::new(50.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(
            &state,
            50.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn multiple_states_animated_independently() {
        let a = State::new(0.0_f32);
        let b = State::new(100.0_f32);
        let mut scheduler = AnimationScheduler::new();
        let start = Instant::now();

        scheduler.animate(&a, 100.0, Duration::from_millis(200), Easing::Linear, start);
        scheduler.animate(&b, 0.0, Duration::from_millis(200), Easing::Linear, start);
        assert_eq!(scheduler.active_count(), 2);

        scheduler.tick(start + Duration::from_millis(100));
        assert!((*a.get() - 50.0).abs() < 1.0);
        assert!((*b.get() - 50.0).abs() < 1.0);

        scheduler.tick(start + Duration::from_millis(200));
        assert_eq!(scheduler.active_count(), 0);
        assert!((*a.get() - 100.0).abs() < 0.01);
        assert!((*b.get() - 0.0).abs() < 0.01);
    }
}
