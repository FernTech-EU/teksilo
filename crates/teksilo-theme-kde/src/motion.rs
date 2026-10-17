// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze motion tokens.
//!
//! Two upstream clocks drive a Plasma application's motion, and the tokens
//! take each step from the one that owns it:
//!
//! - **The Breeze widget style** animates hover, press, focus and toggle
//!   transitions over its `AnimationsDuration`, 100 ms, linearly. Its
//!   progress is quantised to ten steps, which at 60 Hz is about six frames
//!   per transition, so the steps cannot be seen and the curve is
//!   [`Easing::Linear`].
//! - **Kirigami** (and the desktop QtQuick style built on it) derives its
//!   units from one long duration, 200 ms: short is half of it, very long
//!   twice it. Those give the normal, slow and collapse steps.
//!
//! | Token | ms | Source |
//! | --- | --- | --- |
//! | `duration_fast` | 100 | Breeze `AnimationsDuration`; Kirigami short duration |
//! | `duration_normal` | 200 | Kirigami long duration |
//! | `duration_slow` | 400 | Kirigami very long duration |
//! | `duration_collapse` | 200 | Kirigami long duration |
//! | `duration_indeterminate_sweep` | 800 | Breeze busy progress step |
//! | `tooltip_delay`, `tooltip_delay_heavy` | 700 | Kirigami tooltip delay |
//!
//! The desktop's animation speed (`AnimationDurationFactor`) is applied by
//! the desktop reader, not here: these are the factor-1 values.
//!
//! ## The switch curves
//!
//! The one Breeze control whose motion is not linear is the switch of the
//! desktop QtQuick style. Its handle moves on Qt's smoothed animation, which
//! Qt documents as a quadratic ease-in-out, and its colour changes on an
//! ease-in cubic. Both are given as cubic Bézier curves for the switch style
//! to use, [`BREEZE_SWITCH_HANDLE_EASING`] and [`BREEZE_SWITCH_COLOR_EASING`].
//! The quadratic ease-in-out is two parabolas joined at the middle, which no
//! single cubic Bézier curve is, so the handle's curve is its usual
//! approximation; the ease-in cubic is itself a cubic Bézier curve, so the
//! colour's is exact. The tests bound how far each strays from the curve it
//! stands for.

use std::time::Duration;

use teksilo_tokens::{Easing, MotionTokens};

/// Hover, press and toggle transitions. `[S]` Breeze's `AnimationsDuration`
/// default, which is also `[K]` Kirigami's short duration.
pub const BREEZE_FAST_MS: u64 = 100;
/// `[K]` Kirigami's long duration.
pub const BREEZE_NORMAL_MS: u64 = 200;
/// `[K]` Kirigami's very long duration, twice the long one.
pub const BREEZE_SLOW_MS: u64 = 400;
/// `[K]` A disclosure opening or closing: Kirigami's long duration.
pub const BREEZE_COLLAPSE_MS: u64 = 200;
/// `[S]` One period of the busy progress bar's stripes. The desktop's
/// animation speed does not scale it.
pub const BREEZE_BUSY_STEP_MS: u64 = 800;
/// `[K]` Kirigami's tooltip delay. The desktop's animation speed does not
/// scale it.
pub const BREEZE_TOOLTIP_DELAY_MS: u64 = 700;

/// `[S]` Breeze's widget-style transitions are linear.
pub const BREEZE_STANDARD_EASING: Easing = Easing::Linear;

/// `[derived]` The switch handle's motion. Qt documents its smoothed
/// animation as a quadratic ease-in-out (`[qt]`); this cubic Bézier is that
/// curve's usual approximation, within 0.02 of it everywhere (see the
/// tests).
pub const BREEZE_SWITCH_HANDLE_EASING: Easing = Easing::CubicBezier {
    x1: 0.455,
    y1: 0.03,
    x2: 0.515,
    y2: 0.955,
};

/// `[derived]` The switch handle's colour change. The desktop QtQuick style
/// runs it on an ease-in cubic (`[Qd]`), `t` cubed. With its control points
/// at one and two thirds of the way along and none of the way up, a cubic
/// Bézier's horizontal coordinate is its parameter and its vertical one the
/// parameter cubed, so this curve is that ease-in exactly. (The common
/// approximation `(0.55, 0.055, 0.675, 0.19)` strays 0.03 from it.)
pub const BREEZE_SWITCH_COLOR_EASING: Easing = Easing::CubicBezier {
    x1: 1.0 / 3.0,
    y1: 0.0,
    x2: 2.0 / 3.0,
    y2: 0.0,
};

/// Breeze motion tokens at the desktop's default animation speed.
pub fn breeze_motion() -> MotionTokens {
    MotionTokens {
        duration_instant: Duration::ZERO,
        duration_fast: Duration::from_millis(BREEZE_FAST_MS),
        duration_normal: Duration::from_millis(BREEZE_NORMAL_MS),
        duration_slow: Duration::from_millis(BREEZE_SLOW_MS),
        duration_collapse: Duration::from_millis(BREEZE_COLLAPSE_MS),
        duration_indeterminate_sweep: Duration::from_millis(BREEZE_BUSY_STEP_MS),
        tooltip_delay: Duration::from_millis(BREEZE_TOOLTIP_DELAY_MS),
        tooltip_delay_heavy: Duration::from_millis(BREEZE_TOOLTIP_DELAY_MS),
        // `[unpublished]` KDE states no reshow delay: the framework's
        // desktop baseline.
        tooltip_reshow_delay: MotionTokens::default().tooltip_reshow_delay,
        easing_standard: BREEZE_STANDARD_EASING,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The largest distance between two curves, sampled every 1/1000.
    fn max_deviation(a: impl Fn(f32) -> f32, b: impl Fn(f32) -> f32) -> f32 {
        (0..=1000)
            .map(|i| {
                let t = i as f32 / 1000.0;
                (a(t) - b(t)).abs()
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn durations_are_the_breeze_and_kirigami_values() {
        let m = breeze_motion();
        assert_eq!(m.duration_instant, Duration::ZERO);
        assert_eq!(m.duration_fast, Duration::from_millis(100));
        assert_eq!(m.duration_normal, Duration::from_millis(200));
        assert_eq!(m.duration_slow, Duration::from_millis(400));
        assert_eq!(m.duration_collapse, Duration::from_millis(200));
        assert_eq!(m.duration_indeterminate_sweep, Duration::from_millis(800));
        assert_eq!(m.tooltip_delay, Duration::from_millis(700));
        assert_eq!(m.tooltip_delay_heavy, Duration::from_millis(700));
    }

    #[test]
    fn the_kirigami_steps_derive_from_the_long_duration() {
        // Short is half the long duration and very long is twice it.
        assert_eq!(BREEZE_FAST_MS * 2, BREEZE_NORMAL_MS);
        assert_eq!(BREEZE_NORMAL_MS * 2, BREEZE_SLOW_MS);
        assert_eq!(BREEZE_COLLAPSE_MS, BREEZE_NORMAL_MS);
    }

    #[test]
    fn the_standard_curve_is_linear() {
        let e = breeze_motion().easing_standard;
        assert_eq!(e, Easing::Linear);
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!((e.apply(t) - t).abs() < 1e-6);
        }
    }

    #[test]
    fn the_handle_curve_stays_within_two_hundredths_of_the_quadratic_ease_in_out() {
        let deviation = max_deviation(
            |t| BREEZE_SWITCH_HANDLE_EASING.apply(t),
            |t| Easing::EaseInOut.apply(t),
        );
        assert!(
            deviation <= 0.02,
            "the handle curve strays {deviation} from the quadratic ease-in-out"
        );
    }

    #[test]
    fn the_colour_curve_stays_within_two_hundredths_of_the_cubic_ease_in() {
        let deviation = max_deviation(|t| BREEZE_SWITCH_COLOR_EASING.apply(t), |t| t * t * t);
        assert!(
            deviation <= 0.02,
            "the colour curve strays {deviation} from the cubic ease-in"
        );
    }

    #[test]
    fn the_common_ease_in_cubic_approximation_would_stray_too_far() {
        // Pins why the colour curve is the exact one.
        let common = Easing::CubicBezier {
            x1: 0.55,
            y1: 0.055,
            x2: 0.675,
            y2: 0.19,
        };
        assert!(max_deviation(|t| common.apply(t), |t| t * t * t) > 0.02);
    }

    #[test]
    fn the_reshow_delay_keeps_the_desktop_baseline() {
        assert_eq!(
            breeze_motion().tooltip_reshow_delay,
            MotionTokens::default().tooltip_reshow_delay
        );
    }
}
