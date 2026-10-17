// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Easing curve for animations.
///
/// The four named curves are cheap closed forms. `CubicBezier` is the
/// general CSS `cubic-bezier(x1, y1, x2, y2)` curve (control points
/// `(0,0)`, `(x1,y1)`, `(x2,y2)`, `(1,1)`) — needed to express
/// design-language motion specs that don't reduce to the named curves
/// (Material 3 emphasized/standard, Fluent's curves). `x1`/`x2` should
/// lie in `0..=1`; `y1`/`y2` may overshoot for spring-like motion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    #[default]
    EaseOut,
    EaseInOut,
    CubicBezier {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
}

impl Easing {
    /// Apply the easing curve to a linear progress value `t` in [0, 1].
    /// Returns the eased value (which may overshoot [0, 1] for a
    /// `CubicBezier` with `y` control points outside the unit range).
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            Self::CubicBezier { x1, y1, x2, y2 } => cubic_bezier(x1, y1, x2, y2, t),
        }
    }
}

/// Evaluate a CSS `cubic-bezier(x1, y1, x2, y2)` curve at time `t`.
///
/// The curve is parametric in `s`; we solve `X(s) == t` (Newton-Raphson,
/// bisection fallback) then return `Y(s)`. Control points are `(0,0)`,
/// `(x1,y1)`, `(x2,y2)`, `(1,1)`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    // Polynomial coefficients for X(s) and Y(s) (P0 = origin, P3 = (1,1)).
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |s: f32| ((ax * s + bx) * s + cx) * s;
    let sample_y = |s: f32| ((ay * s + by) * s + cy) * s;
    let sample_dx = |s: f32| (3.0 * ax * s + 2.0 * bx) * s + cx;

    // Newton-Raphson from s = t.
    let mut s = t;
    for _ in 0..8 {
        let x = sample_x(s) - t;
        if x.abs() < 1e-6 {
            return sample_y(s);
        }
        let dx = sample_dx(s);
        if dx.abs() < 1e-6 {
            break;
        }
        s -= x / dx;
    }
    // Bisection fallback (guaranteed to converge on a monotone X).
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    let mut s = t.clamp(lo, hi);
    for _ in 0..32 {
        let x = sample_x(s);
        if (x - t).abs() < 1e-6 {
            break;
        }
        if t > x {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) * 0.5;
    }
    sample_y(s)
}

/// Linearly interpolate between two f32 values.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Motion tokens — Int UI durations and easing.
///
/// Int UI's philosophy: hover and press are **instant**; animation is reserved
/// for floating elements (tooltips, balloons, dialogs). JetBrains explicitly
/// avoids decorative animation. A single mild ease-out is used everywhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTokens {
    /// 0 ms — most state changes (hover, press).
    #[serde(with = "duration_millis")]
    pub duration_instant: Duration,
    /// ~120 ms — tooltip fade, popup fade.
    #[serde(with = "duration_millis")]
    pub duration_fast: Duration,
    /// ~200 ms — notification balloon slide.
    #[serde(with = "duration_millis")]
    pub duration_normal: Duration,
    /// ~300 ms — dialog appearance.
    #[serde(with = "duration_millis")]
    pub duration_slow: Duration,
    /// ~200 ms — accordion / disclosure expand-collapse height tween.
    /// Held distinct from `duration_normal` so apps that want a slower
    /// disclosure feel can adjust it without slowing every other motion.
    #[serde(with = "duration_millis")]
    pub duration_collapse: Duration,
    /// ~900 ms — period of one full sweep for indeterminate progress
    /// bars and the future spinner. Long enough to read as continuous
    /// motion without strobing.
    #[serde(with = "duration_millis")]
    pub duration_indeterminate_sweep: Duration,
    /// ~500 ms — hover dwell before a plain or rich tooltip shows.
    ///
    /// Matches the Windows / GTK / WinForms desktop default (Windows ties
    /// this to the system double-click time, 500 ms out of the box). Short
    /// enough to feel responsive once the pointer has paused; long enough
    /// that sweeping a dense toolbar does not flash tips.
    #[serde(with = "duration_millis", default = "default_tooltip_delay")]
    pub tooltip_delay: Duration,
    /// ~700 ms — hover dwell for heavyweight tooltips (composite surfaces,
    /// scene-item tips). Longer so heavy content doesn't pop on transient
    /// hover.
    #[serde(with = "duration_millis", default = "default_tooltip_delay_heavy")]
    pub tooltip_delay_heavy: Duration,
    /// ~100 ms — shortened delay when a tooltip was just shown or dismissed
    /// and the pointer moves to another anchor (Windows `TTDT_RESHOW`).
    /// Keeps scanning adjacent controls fluent after the first tip has
    /// established intent.
    #[serde(with = "duration_millis", default = "default_tooltip_reshow_delay")]
    pub tooltip_reshow_delay: Duration,
    /// Standard easing curve. Int UI uses one mild ease-out everywhere.
    pub easing_standard: Easing,
}

/// Serde default for [`MotionTokens::tooltip_delay`] — keeps themes
/// serialized before this field deserializable.
fn default_tooltip_delay() -> Duration {
    Duration::from_millis(500)
}

/// Serde default for [`MotionTokens::tooltip_delay_heavy`].
fn default_tooltip_delay_heavy() -> Duration {
    Duration::from_millis(700)
}

/// Serde default for [`MotionTokens::tooltip_reshow_delay`].
fn default_tooltip_reshow_delay() -> Duration {
    Duration::from_millis(100)
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            duration_instant: Duration::from_millis(0),
            duration_fast: Duration::from_millis(120),
            duration_normal: Duration::from_millis(200),
            duration_slow: Duration::from_millis(300),
            duration_collapse: Duration::from_millis(200),
            duration_indeterminate_sweep: Duration::from_millis(900),
            tooltip_delay: Duration::from_millis(500),
            tooltip_delay_heavy: Duration::from_millis(700),
            tooltip_reshow_delay: Duration::from_millis(100),
            easing_standard: Easing::EaseOut,
        }
    }
}

impl MotionTokens {
    /// These tokens with every animation duration multiplied by `factor`:
    /// the shape of a desktop's global animation-speed setting (KDE's
    /// `AnimationDurationFactor`, where 2 is twice as slow and 0.5 twice as
    /// fast).
    ///
    /// Scales `duration_instant`, `duration_fast`, `duration_normal`,
    /// `duration_slow`, `duration_collapse` and
    /// `duration_indeterminate_sweep`. The three tooltip delays are **not**
    /// scaled: they are how long the pointer must rest before a tip shows, a
    /// dwell rather than an animation, and slowing animations must not make
    /// tooltips harder to summon. The easing is kept.
    ///
    /// A scaled duration is rounded to the millisecond, and a duration that
    /// was not zero stays at least 1 ms, so a fast factor never turns a
    /// tween into a snap by rounding. A zero duration stays zero.
    ///
    /// `factor` 0 (or a negative one) makes every scaled duration zero,
    /// which is how the framework already reads "no animation": a
    /// zero-duration tween snaps to its target, and the looping consumers
    /// of `duration_indeterminate_sweep` clamp a zero period rather than
    /// divide by it. A factor that is not finite (NaN or infinity) leaves
    /// the tokens unchanged. Durations are capped at `u32::MAX` ms.
    pub fn scaled(&self, factor: f32) -> MotionTokens {
        if !factor.is_finite() {
            return self.clone();
        }
        let factor = f64::from(factor.max(0.0));
        let scale = |d: Duration| -> Duration {
            if factor == 0.0 || d.is_zero() {
                return Duration::ZERO;
            }
            let ms = (d.as_secs_f64() * 1000.0 * factor)
                .round()
                .clamp(1.0, f64::from(u32::MAX));
            Duration::from_millis(ms as u64)
        };
        MotionTokens {
            duration_instant: scale(self.duration_instant),
            duration_fast: scale(self.duration_fast),
            duration_normal: scale(self.duration_normal),
            duration_slow: scale(self.duration_slow),
            duration_collapse: scale(self.duration_collapse),
            duration_indeterminate_sweep: scale(self.duration_indeterminate_sweep),
            tooltip_delay: self.tooltip_delay,
            tooltip_delay_heavy: self.tooltip_delay_heavy,
            tooltip_reshow_delay: self.tooltip_reshow_delay,
            easing_standard: self.easing_standard,
        }
    }
}

mod duration_millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        duration.as_millis().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_default_durations_increasing() {
        let m = MotionTokens::default();
        assert!(m.duration_instant < m.duration_fast);
        assert!(m.duration_fast < m.duration_normal);
        assert!(m.duration_normal < m.duration_slow);
        assert!(m.tooltip_reshow_delay < m.tooltip_delay);
        assert!(m.tooltip_delay <= m.tooltip_delay_heavy);
    }

    #[test]
    fn tooltip_delays_match_desktop_os_norms() {
        // Windows TTDT_INITIAL ≈ double-click time (500 ms); GTK
        // gtk-tooltip-timeout default 500 ms; WinForms InitialDelay 500 ms.
        // Reshow is Windows TTDT_RESHOW (initial / 5 ≈ 100 ms).
        let m = MotionTokens::default();
        assert_eq!(m.tooltip_delay, Duration::from_millis(500));
        assert_eq!(m.tooltip_delay_heavy, Duration::from_millis(700));
        assert_eq!(m.tooltip_reshow_delay, Duration::from_millis(100));
    }

    #[test]
    fn scaled_by_one_is_identity() {
        let m = MotionTokens::default();
        assert_eq!(m.scaled(1.0), m);
    }

    #[test]
    fn scaled_multiplies_every_animation_duration() {
        let m = MotionTokens::default();
        let s = m.scaled(2.0);
        assert_eq!(s.duration_fast, Duration::from_millis(240));
        assert_eq!(s.duration_normal, Duration::from_millis(400));
        assert_eq!(s.duration_slow, Duration::from_millis(600));
        assert_eq!(s.duration_collapse, Duration::from_millis(400));
        assert_eq!(s.duration_indeterminate_sweep, Duration::from_millis(1800));
        let h = m.scaled(0.5);
        assert_eq!(h.duration_fast, Duration::from_millis(60));
        assert_eq!(h.duration_indeterminate_sweep, Duration::from_millis(450));
        assert_eq!(h.easing_standard, m.easing_standard);
    }

    #[test]
    fn scaled_leaves_the_tooltip_delays_alone() {
        let m = MotionTokens::default();
        for f in [0.0, 0.25, 2.0, 10.0] {
            let s = m.scaled(f);
            assert_eq!(s.tooltip_delay, m.tooltip_delay);
            assert_eq!(s.tooltip_delay_heavy, m.tooltip_delay_heavy);
            assert_eq!(s.tooltip_reshow_delay, m.tooltip_reshow_delay);
        }
    }

    #[test]
    fn scaled_rounds_and_keeps_a_non_zero_duration_non_zero() {
        let m = MotionTokens {
            duration_fast: Duration::from_millis(3),
            duration_normal: Duration::from_micros(2500),
            ..MotionTokens::default()
        };
        let s = m.scaled(0.01);
        assert_eq!(s.duration_fast, Duration::from_millis(1));
        assert_eq!(s.duration_slow, Duration::from_millis(3));
        // Rounded to the millisecond, half away from zero: 2.5 ms gives 3.
        assert_eq!(m.scaled(1.0).duration_normal, Duration::from_millis(3));
        // A zero duration stays zero at any factor.
        assert_eq!(m.scaled(4.0).duration_instant, Duration::ZERO);
    }

    #[test]
    fn scaled_by_zero_disables_animation() {
        let s = MotionTokens::default().scaled(0.0);
        for d in [
            s.duration_instant,
            s.duration_fast,
            s.duration_normal,
            s.duration_slow,
            s.duration_collapse,
            s.duration_indeterminate_sweep,
        ] {
            assert_eq!(d, Duration::ZERO);
        }
        assert_eq!(MotionTokens::default().scaled(-3.0), s);
    }

    #[test]
    fn scaled_by_a_non_finite_factor_is_identity() {
        let m = MotionTokens::default();
        assert_eq!(m.scaled(f32::NAN), m);
        assert_eq!(m.scaled(f32::INFINITY), m);
        assert_eq!(m.scaled(f32::NEG_INFINITY), m);
    }

    #[test]
    fn easing_boundaries() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert!((easing.apply(0.0) - 0.0).abs() < 0.001, "{:?} at 0", easing);
            assert!((easing.apply(1.0) - 1.0).abs() < 0.001, "{:?} at 1", easing);
        }
    }

    #[test]
    fn easing_monotonic() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            let mut prev = 0.0;
            for i in 0..=100 {
                let t = i as f32 / 100.0;
                let v = easing.apply(t);
                assert!(v >= prev - 0.001, "{:?} not monotonic at t={}", easing, t);
                prev = v;
            }
        }
    }

    #[test]
    fn lerp_basic() {
        assert_eq!(super::lerp(0.0, 100.0, 0.0), 0.0);
        assert_eq!(super::lerp(0.0, 100.0, 1.0), 100.0);
        assert_eq!(super::lerp(0.0, 100.0, 0.5), 50.0);
        assert_eq!(super::lerp(10.0, 20.0, 0.25), 12.5);
    }

    #[test]
    fn easing_clamps_input() {
        assert!((Easing::Linear.apply(-1.0) - 0.0).abs() < 0.001);
        assert!((Easing::Linear.apply(2.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn cubic_bezier_linear_identity() {
        // cubic-bezier(0,0,1,1) is the identity (== linear).
        let e = Easing::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!((e.apply(t) - t).abs() < 0.01, "linear bezier at {t}");
        }
    }

    #[test]
    fn cubic_bezier_boundaries_and_monotonic() {
        // Material 3 "standard" curve.
        let e = Easing::CubicBezier {
            x1: 0.2,
            y1: 0.0,
            x2: 0.0,
            y2: 1.0,
        };
        assert!((e.apply(0.0) - 0.0).abs() < 0.001);
        assert!((e.apply(1.0) - 1.0).abs() < 0.001);
        let mut prev = -1.0;
        for i in 0..=100 {
            let v = e.apply(i as f32 / 100.0);
            assert!(v >= prev - 0.01, "bezier not monotonic at {i}");
            prev = v;
        }
    }

    #[test]
    fn cubic_bezier_ease_out_is_front_loaded() {
        // CSS "ease-out" (0,0,0.58,1): at the midpoint progress is well
        // past halfway (decelerating).
        let e = Easing::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 0.58,
            y2: 1.0,
        };
        assert!(e.apply(0.5) > 0.6, "ease-out should be front-loaded");
    }
}
