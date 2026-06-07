//! `AnimationSpec` — fluent ergonomic façade over `Signal::animate_to`
//! / `animate_looping` / `try_animate_with_options`.
//!
//! Captures duration, easing, looping mode, frame-interval throttle,
//! pixel-stable epsilon, and the platform reduced-motion preference at
//! build time. Cloned into event-handler closures so a tween fires in
//! one call:
//!
//! ```ignore
//! let knob_anim = ctx.animate().fast().standard();
//! handlers = handlers.on_tap(move |_, _| {
//!     knob_anim.to_or_snap(&knob_position, target);
//! });
//! ```
//!
//! `looping()` quietly enables sub-perceptual quantization
//! (epsilon = 1/255) and a 60 Hz frame interval by default — the two
//! settings every continuous loop should have but that the bare
//! `Signal::animate_looping` API makes opt-in.

use std::time::Duration;

use bastyde_tokens::{Easing, MotionTokens};

use crate::animation::AnimationRequest;
use crate::signal::Signal;

/// Frame interval used when `looping()` is enabled and no explicit
/// override is set. 60 Hz (16.667 ms), matches the scheduler default
/// and the most common display refresh rate so a continuous loop
/// advances once per vsync on a 60 Hz panel and every other frame on
/// 120 Hz. Slower loops where the eye can't resolve sub-30-Hz detail
/// (e.g. `ProgressBar::indeterminate` at 15 Hz) override via
/// `frame_interval(d)` to halve wgpu submits.
const DEFAULT_LOOP_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// Sub-perceptual epsilon for looping color/opacity/position
/// animations. 1/255 ≈ one 8-bit channel step — below this, the
/// scheduler skips the `Signal::set` call and the bound widgets
/// don't get a spurious repaint.
const LOOP_DEFAULT_EPSILON: f32 = 1.0 / 255.0;

/// A fluent specification for animating a `Signal<f32>`.
///
/// Cheap to clone (one `MotionTokens`, a few primitives). Built via
/// [`BuildContext::animate`](crate::build_context::BuildContext::animate)
/// at widget build time, then captured into event-handler closures
/// that drive animations.
#[derive(Debug, Clone)]
pub struct AnimationSpec {
    motion: MotionTokens,
    duration: Duration,
    easing: Easing,
    looping: bool,
    frame_interval: Option<Duration>,
    epsilon: f32,
    reduced_motion: bool,
}

impl AnimationSpec {
    /// Build a default spec (`duration_normal` + `easing_standard`).
    /// Callers normally use `BuildContext::animate` instead, which
    /// wires in the platform reduced-motion preference.
    pub fn from_motion(motion: MotionTokens, reduced_motion: bool) -> Self {
        let duration = motion.duration_normal;
        let easing = motion.easing_standard;
        Self {
            motion,
            duration,
            easing,
            looping: false,
            frame_interval: None,
            epsilon: 0.0,
            reduced_motion,
        }
    }

    // -- duration presets (read from MotionTokens) ----------------------------

    /// `MotionTokens::duration_instant` (default 0 ms).
    pub fn instant(mut self) -> Self {
        self.duration = self.motion.duration_instant;
        self
    }

    /// `MotionTokens::duration_fast` (default 120 ms — tooltip fade,
    /// interactive feedback).
    pub fn fast(mut self) -> Self {
        self.duration = self.motion.duration_fast;
        self
    }

    /// `MotionTokens::duration_normal` (default 200 ms — notification
    /// slides, generic transitions).
    pub fn normal(mut self) -> Self {
        self.duration = self.motion.duration_normal;
        self
    }

    /// `MotionTokens::duration_slow` (default 300 ms — dialog
    /// appearance).
    pub fn slow(mut self) -> Self {
        self.duration = self.motion.duration_slow;
        self
    }

    /// `MotionTokens::duration_collapse` (default 200 ms — accordion /
    /// disclosure expand-collapse).
    pub fn collapse(mut self) -> Self {
        self.duration = self.motion.duration_collapse;
        self
    }

    /// `MotionTokens::duration_indeterminate_sweep` (default 900 ms —
    /// indeterminate progress sweep, spinner period). Implies
    /// `looping()`.
    pub fn sweep(mut self) -> Self {
        self.duration = self.motion.duration_indeterminate_sweep;
        self.set_looping_defaults()
    }

    /// Set the duration explicitly. Prefer the named presets when one
    /// fits — they keep the design system honest.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    // -- easing ---------------------------------------------------------------

    /// `MotionTokens::easing_standard` (default `EaseOut`). The Int-UI
    /// "single mild ease-out everywhere" curve.
    pub fn standard(mut self) -> Self {
        self.easing = self.motion.easing_standard;
        self
    }

    pub fn linear(mut self) -> Self {
        self.easing = Easing::Linear;
        self
    }

    pub fn ease_in(mut self) -> Self {
        self.easing = Easing::EaseIn;
        self
    }

    pub fn ease_out(mut self) -> Self {
        self.easing = Easing::EaseOut;
        self
    }

    pub fn ease_in_out(mut self) -> Self {
        self.easing = Easing::EaseInOut;
        self
    }

    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    // -- loop / frame interval / quantization ---------------------------------

    /// Switch to looping mode. Sets a sub-perceptual epsilon (1/255)
    /// and a 60 Hz frame interval **only if not already overridden**,
    /// so a continuous bar / spinner advances once per vsync on a
    /// 60 Hz panel without forcing higher-refresh displays into
    /// extra `Signal::set` calls (`==`-equal values short-circuit).
    pub fn looping(self) -> Self {
        self.set_looping_defaults()
    }

    fn set_looping_defaults(mut self) -> Self {
        self.looping = true;
        if self.frame_interval.is_none() {
            self.frame_interval = Some(DEFAULT_LOOP_FRAME_INTERVAL);
        }
        if self.epsilon == 0.0 {
            self.epsilon = LOOP_DEFAULT_EPSILON;
        }
        self
    }

    /// Throttle scheduler ticks to at most one per `interval`. Use to
    /// drop a 60 Hz signal animation to 15-30 Hz when the eye can't
    /// resolve the difference and every doubled frame costs a wgpu
    /// submit (e.g. `ProgressBar::indeterminate`'s wide sweep, set to
    /// 15 Hz via `Duration::from_millis(66)`).
    pub fn frame_interval(mut self, interval: Duration) -> Self {
        self.frame_interval = Some(interval);
        self
    }

    /// Per-tick quantization. Skip `Signal::set(value)` when the new
    /// value differs from the last set value by less than `epsilon`.
    /// `0.0` (the one-shot default) means "set every tick".
    pub fn epsilon(mut self, epsilon: f32) -> Self {
        self.epsilon = epsilon;
        self
    }

    // -- application ----------------------------------------------------------

    /// Animate `signal` to `target` using this spec. Returns
    /// immediately; the scheduler drives the tween.
    ///
    /// Does NOT honor `prefers_reduced_motion`; use
    /// [`to_or_snap`](Self::to_or_snap) for that.
    pub fn to(&self, signal: &Signal<f32>, target: f32) {
        let _ = signal.try_animate_with_options(self.into_request(target));
    }

    /// Animate to `target`, but if the user prefers reduced motion
    /// (snapshot at build time), snap directly to `target` instead of
    /// tweening. This is the right default for one-shot UI tweens
    /// (toggle knob, accordion collapse, fade).
    ///
    /// For continuous looping animations, prefer gating the call site
    /// (don't start the loop at all under reduced motion) — snapping
    /// a loop to its end value just stops it on the wrong frame.
    pub fn to_or_snap(&self, signal: &Signal<f32>, target: f32) {
        if self.reduced_motion {
            signal.set(target);
        } else {
            self.to(signal, target);
        }
    }

    /// Whether the captured platform preference is for reduced motion.
    /// Use as a gate before kicking off a continuous looping animation
    /// (`if !spec.reduced_motion() { spec.to(&signal, 1.0); }`).
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    #[allow(clippy::wrong_self_convention)]
    fn into_request(&self, target: f32) -> AnimationRequest {
        AnimationRequest {
            target,
            duration: self.duration,
            easing: self.easing,
            frame_interval: self.frame_interval,
            looping: self.looping,
            epsilon: self.epsilon,
            max_duration: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion() -> MotionTokens {
        MotionTokens::default()
    }

    #[test]
    fn presets_pull_from_motion_tokens() {
        let m = motion();
        let s = AnimationSpec::from_motion(m.clone(), false);
        assert_eq!(s.clone().fast().into_request(0.0).duration, m.duration_fast);
        assert_eq!(
            s.clone().normal().into_request(0.0).duration,
            m.duration_normal
        );
        assert_eq!(s.clone().slow().into_request(0.0).duration, m.duration_slow);
        assert_eq!(
            s.clone().collapse().into_request(0.0).duration,
            m.duration_collapse
        );
        assert_eq!(
            s.clone().sweep().into_request(0.0).duration,
            m.duration_indeterminate_sweep
        );
    }

    #[test]
    fn looping_sets_subperceptual_epsilon_and_frame_interval() {
        let s = AnimationSpec::from_motion(motion(), false).looping();
        let r = s.into_request(1.0);
        assert!(r.looping);
        assert_eq!(r.epsilon, LOOP_DEFAULT_EPSILON);
        assert_eq!(r.frame_interval, Some(DEFAULT_LOOP_FRAME_INTERVAL));
    }

    #[test]
    fn looping_preserves_explicit_frame_interval() {
        let custom = Duration::from_millis(66);
        let s = AnimationSpec::from_motion(motion(), false)
            .frame_interval(custom)
            .looping();
        assert_eq!(s.into_request(1.0).frame_interval, Some(custom));
    }

    #[test]
    fn looping_preserves_explicit_epsilon() {
        let s = AnimationSpec::from_motion(motion(), false)
            .epsilon(0.5)
            .looping();
        assert_eq!(s.into_request(1.0).epsilon, 0.5);
    }

    #[test]
    fn sweep_implies_looping() {
        let r = AnimationSpec::from_motion(motion(), false)
            .sweep()
            .into_request(1.0);
        assert!(r.looping);
        assert_eq!(r.epsilon, LOOP_DEFAULT_EPSILON);
    }

    #[test]
    fn standard_resets_easing_to_token() {
        let m = motion();
        let r = AnimationSpec::from_motion(m.clone(), false)
            .ease_in_out()
            .standard()
            .into_request(0.0);
        assert_eq!(r.easing, m.easing_standard);
    }

    #[test]
    fn to_or_snap_under_reduced_motion_sets_directly() {
        use crate::signal::Signal;
        let signal = Signal::new_animated(0.0);
        let s = AnimationSpec::from_motion(motion(), true).fast();
        s.to_or_snap(&signal, 0.75);
        // Direct set, no pending animation request.
        assert!(!signal.has_pending_animation());
        assert_eq!(signal.get(), 0.75);
    }

    #[test]
    fn to_or_snap_without_reduced_motion_queues_request() {
        use crate::signal::Signal;
        let signal = Signal::new_animated(0.0);
        let s = AnimationSpec::from_motion(motion(), false).fast();
        s.to_or_snap(&signal, 0.75);
        assert!(signal.has_pending_animation());
        assert_eq!(signal.get(), 0.0); // hasn't moved yet
    }
}
