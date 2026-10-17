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
//! These are the values at the desktop's default animation speed.
//!
//! ## The desktop's animation speed
//!
//! Plasma's animation speed setting is `kdeglobals [KDE]
//! AnimationDurationFactor`, a real, 1 when absent, 2 for twice as slow.
//! [`desktop_motion`] applies it as Kirigami applies it to its units,
//! which drive the controls whose timing the preset reproduces (spec j.2,
//! `[derived]`): the long duration is `max(1, round(200 f))` ms with `f`
//! the factor (0 for a negative one), the short half of it rounded down and
//! the very long twice it. So `duration_normal` and `duration_collapse` are
//! that long duration, `duration_fast` its half and `duration_slow` its
//! double. The busy progress bar's period is `breezerc`'s
//! `ProgressBarBusyStepDuration` and the factor does not scale it, nor the
//! tooltip delays, which are dwell times rather than animations.
//!
//! A factor of 0 is the desktop asking for reduced motion: the long
//! duration is 1 ms and the short one 0, which the framework plays as no
//! animation, and the platform reports the preference from the same factor.
//! This is not [`MotionTokens::scaled`], which would scale every duration
//! by the factor itself: Kirigami derives the short duration from the long
//! one, and Breeze does not scale the busy bar.
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

/// `[derived]` Breeze motion tokens at the desktop animation speed `factor`
/// (`kdeglobals [KDE] AnimationDurationFactor`, 1 when the desktop sets
/// none), with the busy progress bar's stripes moving one period every
/// `busy_step` (`breezerc` `ProgressBarBusyStepDuration`, 800 ms by
/// default).
///
/// The long duration is `max(1, round(200 x max(0, factor)))` ms, rounded
/// half up: `duration_normal` and `duration_collapse` are it,
/// `duration_fast` half of it rounded down, `duration_slow` twice it. The
/// busy period and the tooltip delays are not scaled. A factor that is not
/// finite is read as 1. See the [module documentation](self).
pub fn desktop_motion(factor: f64, busy_step: Duration) -> MotionTokens {
    let factor = if factor.is_finite() {
        factor.max(0.0)
    } else {
        1.0
    };
    // Capped so that twice the long duration still fits a `u64` of
    // milliseconds.
    let long = (BREEZE_NORMAL_MS as f64 * factor + 0.5)
        .floor()
        .clamp(1.0, f64::from(u32::MAX)) as u64;
    MotionTokens {
        duration_fast: Duration::from_millis(long / 2),
        duration_normal: Duration::from_millis(long),
        duration_slow: Duration::from_millis(2 * long),
        duration_collapse: Duration::from_millis(long),
        duration_indeterminate_sweep: busy_step,
        ..breeze_motion()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(d: Duration) -> u128 {
        d.as_millis()
    }

    #[test]
    fn the_default_speed_is_the_default_motion() {
        assert_eq!(
            desktop_motion(1.0, Duration::from_millis(BREEZE_BUSY_STEP_MS)),
            breeze_motion()
        );
    }

    #[test]
    fn the_factor_scales_kirigamis_long_duration() {
        for (factor, fast, normal, slow) in [
            (2.0, 200, 400, 800),
            (0.5, 50, 100, 200),
            (0.25, 25, 50, 100),
            // Half of the long duration is rounded down: 67 / 2.
            (0.335, 33, 67, 134),
            // Rounded half up: 200 x 0.0125 is 2.5.
            (0.0125, 1, 3, 6),
        ] {
            let m = desktop_motion(factor, Duration::from_millis(800));
            assert_eq!(ms(m.duration_fast), fast, "{factor}");
            assert_eq!(ms(m.duration_normal), normal, "{factor}");
            assert_eq!(ms(m.duration_slow), slow, "{factor}");
            assert_eq!(m.duration_collapse, m.duration_normal, "{factor}");
            assert_eq!(m.duration_instant, Duration::ZERO);
            assert_eq!(m.easing_standard, Easing::Linear);
        }
    }

    #[test]
    fn the_busy_period_and_the_tooltip_delays_are_not_scaled() {
        let m = desktop_motion(3.0, Duration::from_millis(500));
        assert_eq!(ms(m.duration_indeterminate_sweep), 500);
        assert_eq!(ms(m.tooltip_delay), 700);
        assert_eq!(ms(m.tooltip_delay_heavy), 700);
        assert_eq!(
            m.tooltip_reshow_delay,
            MotionTokens::default().tooltip_reshow_delay
        );
    }

    #[test]
    fn a_factor_of_zero_leaves_a_one_millisecond_long_duration() {
        for factor in [0.0, -1.0] {
            let m = desktop_motion(factor, Duration::from_millis(800));
            assert_eq!(ms(m.duration_fast), 0, "{factor}");
            assert_eq!(ms(m.duration_normal), 1, "{factor}");
            assert_eq!(ms(m.duration_slow), 2, "{factor}");
            assert_eq!(ms(m.duration_collapse), 1, "{factor}");
            assert_eq!(ms(m.duration_indeterminate_sweep), 800, "{factor}");
        }
    }

    #[test]
    fn a_factor_that_is_not_finite_is_the_default_speed() {
        for factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                desktop_motion(factor, Duration::from_millis(800)),
                breeze_motion(),
                "{factor}"
            );
        }
        // A huge finite factor is capped rather than overflowing.
        let m = desktop_motion(1e300, Duration::from_millis(800));
        assert_eq!(ms(m.duration_normal), u128::from(u32::MAX));
    }

    #[test]
    fn kirigamis_rule_is_not_the_frameworks_uniform_scale() {
        // Pins why `desktop_motion` does not call `MotionTokens::scaled`:
        // the uniform scale rounds the short duration on its own and
        // scales the busy period.
        let scaled = breeze_motion().scaled(0.335);
        let kirigami = desktop_motion(0.335, Duration::from_millis(800));
        assert_eq!(ms(scaled.duration_fast), 34);
        assert_eq!(ms(kirigami.duration_fast), 33);
        assert_ne!(
            scaled.duration_indeterminate_sweep,
            kirigami.duration_indeterminate_sweep
        );
    }

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
