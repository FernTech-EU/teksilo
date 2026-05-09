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

use crate::arena::WidgetArena;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// A pending animation request on a `Signal<f32>`.
///
/// Filled in by `Signal::animate_to()` / `Signal::animate_looping()` and
/// consumed by the widget tree's `process_pending_animations` pass, which
/// hands it to the scheduler.
#[derive(Debug, Clone)]
pub struct AnimationRequest {
    pub target: f32,
    pub duration: Duration,
    pub easing: Easing,
    pub frame_interval: Option<Duration>,
    /// If true, the animation loops: resets to the signal's current
    /// value each time it reaches `target`.
    pub looping: bool,
    /// Per-tick quantization: skip `signal.set(value)` when the new value
    /// differs from the last set value by less than this. Terminal ticks
    /// (completion / loop restart) always bypass the check. `0.0` = always
    /// set (default).
    pub epsilon: f32,
    /// Opt-in wall-clock cap. When elapsed since the animation's start
    /// exceeds this, the animation snaps to `start_value` and drops.
    /// `None` = no cap.
    pub max_duration: Option<Duration>,
}

impl Default for AnimationRequest {
    fn default() -> Self {
        Self {
            target: 0.0,
            duration: Duration::ZERO,
            easing: Easing::Linear,
            frame_interval: None,
            looping: false,
            epsilon: 0.0,
            max_duration: None,
        }
    }
}

/// A single active animation driving a `Signal<f32>` from `start` to `end`.
struct ActiveAnimation {
    widget_id: WidgetId,
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
    /// `cancel()` / `cancel_by_widget()` or by widget rebuild/destroy.
    looping: bool,
    epsilon: f32,
    last_set_value: f32,
    /// Wall-clock when the animation entered the scheduler; used with
    /// `max_duration` to enforce the opt-in cap. Note this is distinct
    /// from `start_time`, which gets rebased on pause/resume.
    started_at: Instant,
    max_duration: Option<Duration>,
}

/// Default frame interval for animations: ~30 fps. Smooth enough for
/// UI transitions while keeping CPU/GPU usage low. Individual
/// animations can override via `animate_with_frame_interval`.
const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Manages active animations and advances them each frame.
pub struct AnimationScheduler {
    animations: Vec<ActiveAnimation>,
    /// `false` pauses every entry: `tick` is a no-op and `next_deadline`
    /// returns `None`, so the scheduler stops contributing to
    /// `ControlFlow::WaitUntil`. Used to suspend animations while the
    /// owning window is unfocused or occluded.
    window_active: bool,
    /// Wall-clock when the scheduler last went inactive. Used on resume
    /// to rebase each animation's `start_time` so `t` is phase-continuous
    /// across the pause (no snap, no skipped frames).
    paused_at: Option<Instant>,
}

impl AnimationScheduler {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
            window_active: true,
            paused_at: None,
        }
    }

    /// Start animating a `Signal<f32>` from its current value to `target`.
    /// If the signal is already being animated, the previous animation is
    /// replaced (the current in-flight value becomes the new start).
    pub fn animate(
        &mut self,
        signal: &Signal<f32>,
        widget_id: WidgetId,
        target: f32,
        duration: Duration,
        easing: Easing,
        now: Instant,
    ) {
        self.animate_with_options(
            signal, widget_id, target, duration, easing, None, 0.0, None, now,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn animate_with_options(
        &mut self,
        signal: &Signal<f32>,
        widget_id: WidgetId,
        target: f32,
        duration: Duration,
        easing: Easing,
        frame_interval: Option<Duration>,
        epsilon: f32,
        max_duration: Option<Duration>,
        now: Instant,
    ) {
        // If an animation is already in flight on this signal, fast-forward
        // its value to where it *should* be at `now` before cancelling.
        // Without this, a stream of `animate_to` calls (e.g. one per frame
        // from a fast mouse-wheel flick) would each restart from the same
        // pre-tick `signal.get()` value: `process_pending_animations` runs
        // before `tick` in the layout pass, so a freshly-started animation
        // always has `elapsed == 0` on its first tick. Net effect: the
        // signal would never advance until events stop, then the last
        // animation eases to target — the "lag-then-catch-up" pattern.
        if let Some(existing) = self
            .animations
            .iter()
            .find(|a| Signal::same(&a.signal, signal))
            && !existing.looping
        {
            let elapsed = now.saturating_duration_since(existing.start_time);
            let t = if existing.duration.is_zero() {
                1.0
            } else {
                (elapsed.as_secs_f32() / existing.duration.as_secs_f32()).min(1.0)
            };
            let eased = existing.easing.apply(t);
            let value = fern_tokens::lerp(existing.start_value, existing.end_value, eased);
            signal.set(value);
        }

        let current = signal.get();
        self.cancel(signal);

        if (current - target).abs() < f32::EPSILON || duration.is_zero() {
            signal.set(target);
            signal.clear_animation_target();
            return;
        }

        self.animations.push(ActiveAnimation {
            widget_id,
            signal: signal.clone(),
            start_value: current,
            end_value: target,
            start_time: now,
            duration,
            easing,
            frame_interval: frame_interval.unwrap_or(DEFAULT_FRAME_INTERVAL),
            next_tick: now,
            looping: false,
            epsilon,
            last_set_value: current,
            started_at: now,
            max_duration,
        });
    }

    /// Start a looping animation that cycles from `start` to `end`
    /// repeatedly. The signal resets to `start` each time it reaches
    /// `end`. Runs until cancelled.
    #[allow(clippy::too_many_arguments)]
    pub fn animate_looping(
        &mut self,
        signal: &Signal<f32>,
        widget_id: WidgetId,
        start: f32,
        end: f32,
        period: Duration,
        easing: Easing,
        frame_interval: Option<Duration>,
        epsilon: f32,
        max_duration: Option<Duration>,
        now: Instant,
    ) {
        self.cancel(signal);
        signal.set(start);

        self.animations.push(ActiveAnimation {
            widget_id,
            signal: signal.clone(),
            start_value: start,
            end_value: end,
            start_time: now,
            duration: period,
            easing,
            frame_interval: frame_interval.unwrap_or(DEFAULT_FRAME_INTERVAL),
            next_tick: now,
            looping: true,
            epsilon,
            last_set_value: start,
            started_at: now,
            max_duration,
        });
    }

    /// Cancel any active animation on the given signal.
    pub fn cancel(&mut self, signal: &Signal<f32>) {
        self.animations.retain(|a| !Signal::same(&a.signal, signal));
    }

    /// Cancel every animation whose driving widget matches `widget_id`.
    ///
    /// Called when a widget is destroyed or rebuilt: the widget's
    /// `Signal<f32>` clones in the scheduler would otherwise outlive the
    /// widget, continuing to tick against an orphaned signal whose
    /// observers no longer exist — silent CPU waste and, on rebuild, a
    /// second animation for the fresh signal stacking on top of the old.
    pub fn cancel_by_widget(&mut self, widget_id: WidgetId) {
        self.animations.retain(|a| {
            if a.widget_id == widget_id {
                a.signal.clear_animation_target();
                false
            } else {
                true
            }
        });
    }

    /// Mark the owning window as active (focused-and-visible) or not.
    ///
    /// Inactive: `tick` is a no-op; `next_deadline` returns `None`. On
    /// transition back to active, each animation's `start_time` is
    /// rebased by the paused duration so the eased phase `t` is
    /// continuous — a 50%-through sweep resumes at 50%, not snapped to
    /// some other spot on the curve.
    pub fn set_window_active(&mut self, active: bool, now: Instant) {
        if self.window_active == active {
            return;
        }
        if active {
            if let Some(paused_at) = self.paused_at.take() {
                let offset = now.saturating_duration_since(paused_at);
                for anim in &mut self.animations {
                    anim.start_time += offset;
                    anim.next_tick = now;
                }
            }
        } else {
            self.paused_at = Some(now);
        }
        self.window_active = active;
    }

    pub fn is_window_active(&self) -> bool {
        self.window_active
    }

    /// Advance all active animations to the given time.
    /// Returns true if any animation is still running *and eligible to
    /// run next tick* (caller should request another frame).
    ///
    /// `arena` + `paint_epoch` gate per-widget visibility for **looping**
    /// animations only: a continuous spinner whose owner widget is
    /// dormant or hasn't been painted in the most recent paint pass
    /// skips its tick. One-shot tweens are NOT visibility-gated — a
    /// widget that animates its own size from zero (e.g. `Collapse`
    /// growing from height=0 to natural) would otherwise be paused on
    /// the very tick that would make it visible, locking it in the
    /// invisible state forever. Pass `paint_epoch == 0` to disable the
    /// gate entirely (headless tests that never call `render()`).
    pub fn tick(&mut self, now: Instant, arena: &WidgetArena, paint_epoch: u64) -> bool {
        if !self.window_active {
            return !self.animations.is_empty();
        }

        self.animations.retain_mut(|anim| {
            if !anim_widget_alive(arena, anim.widget_id) {
                anim.signal.clear_animation_target();
                return false;
            }

            if let Some(max) = anim.max_duration
                && now.saturating_duration_since(anim.started_at) >= max
            {
                anim.signal.set(anim.start_value);
                anim.signal.clear_animation_target();
                return false;
            }

            if anim.looping && !anim_widget_visible(arena, anim.widget_id, paint_epoch) {
                // Looping animation on an offscreen owner — pause to
                // save CPU. Leave start_time alone so resume picks up
                // mid-phase. Push next_tick so we don't spin on a
                // paused entry when the scheduler is polled via some
                // other deadline.
                anim.next_tick = now + anim.frame_interval;
                return true;
            }

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

            let terminal = t >= 1.0;
            // Terminal ticks always set unconditionally so we land exactly
            // on end_value (or snap to start on loop restart); epsilon
            // quantization only applies to intermediate ticks.
            if terminal || (value - anim.last_set_value).abs() >= anim.epsilon {
                anim.signal.set(value);
                anim.last_set_value = value;
            }

            if t >= 1.0 && anim.looping {
                anim.start_time = now;
                anim.signal.set(anim.start_value);
                anim.last_set_value = anim.start_value;
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

    /// Whether any animation is currently stored in the scheduler
    /// (ignores pause state — prefer `has_running`).
    pub fn has_active(&self) -> bool {
        !self.animations.is_empty()
    }

    /// Whether any animation is *eligible to advance* on the next tick:
    /// stored AND the window is active. Used by the idle-work predicates
    /// so a window-paused scheduler doesn't keep the event loop in
    /// `ControlFlow::WaitUntil`.
    ///
    /// This does NOT check per-widget visibility (we'd need the arena
    /// and the current paint epoch). An animation whose widget is
    /// offscreen is still reported here; the per-widget gate lives in
    /// `next_deadline` and `tick` directly.
    pub fn has_running(&self) -> bool {
        self.window_active && !self.animations.is_empty()
    }

    /// Earliest deadline at which a not-paused animation wants to
    /// tick. Returns `None` when the scheduler is window-paused, all
    /// (looping) animations are hidden, or there are no animations at
    /// all. One-shot tweens are NOT visibility-gated — see the
    /// matching note on [`tick`](Self::tick).
    pub fn next_deadline(&self, arena: &WidgetArena, paint_epoch: u64) -> Option<Instant> {
        if !self.window_active {
            return None;
        }
        self.animations
            .iter()
            .filter(|anim| {
                anim_widget_alive(arena, anim.widget_id)
                    && (!anim.looping || anim_widget_visible(arena, anim.widget_id, paint_epoch))
            })
            .map(|anim| anim.next_tick)
            .min()
    }

    /// Number of active animations (for testing/debugging).
    pub fn active_count(&self) -> usize {
        self.animations.len()
    }
}

use crate::motion_visibility::{alive as anim_widget_alive, painted_recently as anim_widget_visible};

impl Default for AnimationScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AnimationScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationScheduler")
            .field("active_count", &self.animations.len())
            .field("window_active", &self.window_active)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::WidgetArena;
    use crate::test_widgets::FillWidget;

    fn test_arena_with_widget() -> (WidgetArena, WidgetId) {
        let mut arena = WidgetArena::new();
        let id = arena.insert(Box::new(FillWidget::new()));
        (arena, id)
    }

    #[test]
    fn animate_from_current_to_target() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.tick(start, &arena, 0);
        assert!((signal.get() - 0.0).abs() < 1.0);

        let has_more = scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!(has_more);
        assert!((signal.get() - 50.0).abs() < 1.0);

        let has_more = scheduler.tick(start + Duration::from_millis(200), &arena, 0);
        assert!(!has_more);
        assert!((signal.get() - 100.0).abs() < 0.01);
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn eased_animation() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::EaseIn,
            start,
        );

        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!((signal.get() - 25.0).abs() < 1.0);
    }

    #[test]
    fn zero_duration_sets_immediately() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (_arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(&signal, id, 100.0, Duration::ZERO, Easing::Linear, start);
        assert_eq!(scheduler.active_count(), 0);
        assert!((signal.get() - 100.0).abs() < 0.01);
    }

    #[test]
    fn replace_existing_animation() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );

        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        let mid_value = signal.get();
        assert!((mid_value - 50.0).abs() < 1.0);

        let mid_time = start + Duration::from_millis(100);
        scheduler.animate(
            &signal,
            id,
            0.0,
            Duration::from_millis(100),
            Easing::Linear,
            mid_time,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.tick(mid_time + Duration::from_millis(50), &arena, 0);
        assert!((signal.get() - 25.0).abs() < 2.0);
    }

    #[test]
    fn cancel_stops_animation() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (_arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.cancel(&signal);
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn already_at_target_no_animation() {
        let signal = Signal::<f32>::new_animated(50.0);
        let mut scheduler = AnimationScheduler::new();
        let (_arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
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
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &a,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        scheduler.animate(
            &b,
            id,
            0.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        assert_eq!(scheduler.active_count(), 2);

        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!((a.get() - 50.0).abs() < 1.0);
        assert!((b.get() - 50.0).abs() < 1.0);

        scheduler.tick(start + Duration::from_millis(200), &arena, 0);
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn looping_animation_restarts() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate_looping(
            &signal,
            id,
            0.0,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            None,
            0.0,
            None,
            start,
        );

        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!((signal.get() - 50.0).abs() < 1.0);

        let has_more = scheduler.tick(start + Duration::from_millis(200), &arena, 0);
        assert!(has_more, "looping animation should keep running");
        assert!(signal.get() < 5.0);

        scheduler.tick(start + Duration::from_millis(300), &arena, 0);
        assert!((signal.get() - 50.0).abs() < 5.0);
    }

    #[test]
    fn looping_animation_cancelled() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (_arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate_looping(
            &signal,
            id,
            0.0,
            10.0,
            Duration::from_millis(100),
            Easing::Linear,
            None,
            0.0,
            None,
            start,
        );
        assert_eq!(scheduler.active_count(), 1);

        scheduler.cancel(&signal);
        assert_eq!(scheduler.active_count(), 0);
    }

    #[test]
    fn cancel_by_widget_removes_all_animations_owned_by_widget() {
        let a = Signal::<f32>::new_animated(0.0);
        let b = Signal::<f32>::new_animated(0.0);
        let c = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let mut arena = WidgetArena::new();
        let id_x = arena.insert(Box::new(FillWidget::new()));
        let id_y = arena.insert(Box::new(FillWidget::new()));
        let now = Instant::now();

        scheduler.animate(&a, id_x, 1.0, Duration::from_secs(1), Easing::Linear, now);
        scheduler.animate(&b, id_x, 1.0, Duration::from_secs(1), Easing::Linear, now);
        scheduler.animate(&c, id_y, 1.0, Duration::from_secs(1), Easing::Linear, now);
        assert_eq!(scheduler.active_count(), 3);

        scheduler.cancel_by_widget(id_x);
        assert_eq!(scheduler.active_count(), 1);

        // c (owned by id_y) still running
        let _ = arena;
    }

    #[test]
    fn window_inactive_pauses_tick() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );
        scheduler.set_window_active(false, start);

        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!(
            signal.get() < 1.0,
            "paused scheduler must not advance the signal"
        );
        assert!(scheduler.next_deadline(&arena, 0).is_none());
    }

    #[test]
    fn resume_rebases_phase_continuously() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );

        // Advance halfway (t=0.5 → value≈50).
        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!((signal.get() - 50.0).abs() < 1.0);

        // Window goes inactive at t=100ms.
        scheduler.set_window_active(false, start + Duration::from_millis(100));

        // 10 seconds of real time pass with window hidden. No ticks happen.
        let resume_at = start + Duration::from_millis(100) + Duration::from_secs(10);
        scheduler.set_window_active(true, resume_at);

        // 50ms *after* resume, we should be at ≈75% of the eased curve,
        // NOT at 100% (which is what we'd get without rebasing start_time).
        scheduler.tick(resume_at + Duration::from_millis(50), &arena, 0);
        let after_resume = signal.get();
        assert!(
            (after_resume - 75.0).abs() < 2.0,
            "expected phase-continuous resume ≈ 75, got {after_resume}"
        );
    }

    #[test]
    fn epsilon_skips_intermediate_sets_but_not_terminal() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        // ε = 10 → tick at t=0.05 produces value=5, below ε, should skip.
        scheduler.animate_with_options(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            None,
            10.0,
            None,
            start,
        );

        scheduler.tick(start + Duration::from_millis(10), &arena, 0);
        assert!(
            signal.get() < 1.0,
            "sub-ε tick should NOT call signal.set, signal.get() = {}",
            signal.get()
        );

        // Terminal tick must set end_value regardless of ε.
        scheduler.tick(start + Duration::from_millis(200), &arena, 0);
        assert!(
            (signal.get() - 100.0).abs() < 0.01,
            "terminal tick must bypass ε and land exactly on end"
        );
    }

    #[test]
    fn max_duration_snaps_to_start_and_drops() {
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();

        scheduler.animate_looping(
            &signal,
            id,
            0.0,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            None,
            0.0,
            Some(Duration::from_secs(1)),
            start,
        );

        scheduler.tick(start + Duration::from_millis(100), &arena, 0);
        assert!(signal.get() > 0.0);

        // 1.5s past start-at: past the 1s cap.
        let has_more = scheduler.tick(start + Duration::from_millis(1500), &arena, 0);
        assert!(!has_more, "capped animation should drop");
        assert_eq!(scheduler.active_count(), 0);
        assert!(
            signal.get().abs() < 0.01,
            "capped animation should snap to start_value (0), got {}",
            signal.get()
        );
    }

    #[test]
    fn one_shot_tween_runs_even_when_owner_appears_offscreen() {
        // Regression: a `Collapse`-style widget animating its own
        // height from 0 → natural would set up a one-shot tween whose
        // owner is the Collapse widget itself. Because the widget's
        // current bounds are zero, the paint pass would skip it,
        // never stamping `last_painted_epoch`. The visibility gate
        // would then see `lpe + 1 < paint_epoch` and pause the
        // animation forever — locking the widget at height=0.
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (mut arena, id) = test_arena_with_widget();
        // Simulate a never-painted widget (lpe stays at the default 0)
        // while the global paint_epoch has advanced many frames.
        let _ = arena.get_mut(id); // ensure node exists; lpe defaults to 0
        let start = Instant::now();
        let paint_epoch: u64 = 42; // many frames have passed

        scheduler.animate(
            &signal,
            id,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            start,
        );

        scheduler.tick(start + Duration::from_millis(100), &arena, paint_epoch);
        assert!(
            signal.get() > 10.0,
            "one-shot tween must progress despite owner having stale paint_epoch (got {})",
            signal.get()
        );

        scheduler.tick(start + Duration::from_millis(200), &arena, paint_epoch);
        assert!(
            (signal.get() - 100.0).abs() < 0.01,
            "one-shot tween must reach target (got {})",
            signal.get()
        );
    }

    #[test]
    fn looping_animation_pauses_when_owner_offscreen() {
        // The looping case (e.g. spinner in a hidden tab) keeps the
        // visibility gate: we don't want a hidden spinner to burn CPU
        // ticking against a widget the user can't see.
        let signal = Signal::<f32>::new_animated(0.0);
        let mut scheduler = AnimationScheduler::new();
        let (arena, id) = test_arena_with_widget();
        let start = Instant::now();
        let paint_epoch: u64 = 42;

        scheduler.animate_looping(
            &signal,
            id,
            0.0,
            100.0,
            Duration::from_millis(200),
            Easing::Linear,
            None,
            0.0,
            None,
            start,
        );

        // With paint_epoch high and lpe at 0, the loop must NOT
        // progress — the value stays at the start.
        scheduler.tick(start + Duration::from_millis(100), &arena, paint_epoch);
        assert!(
            signal.get().abs() < 0.01,
            "looping animation should pause for offscreen owner (got {})",
            signal.get()
        );
    }
}
