// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent motion tokens.
//!
//! WinUI publishes exactly four control durations, and they are shorter
//! than the framework baseline:
//!
//! | ThemeResource | Value |
//! | --- | --- |
//! | `ControlFasterAnimationDuration` | 83 ms |
//! | `ControlFastAnimationDuration` | 167 ms |
//! | `ControlNormalAnimationDuration` | 250 ms |
//!
//! There is no named "slow" token; 333 ms recurs as a literal across the
//! flyout open/close storyboards, which is what [`FLUENT_SLOW_MS`] carries.
//!
//! The easing is a single curve, `ControlFastOutSlowInKeySpline`, whose
//! XAML value is the key spline `"0,0,0,1"` — `cubic-bezier(0, 0, 0, 1)`.
//! It leaves at full speed and settles asymptotically, which is why Fluent
//! motion reads as *arriving* rather than *travelling*. Every Fluent widget
//! style in this crate animates on it.
//!
//! **Not overridden:** the three tooltip delays. Those are desktop
//! conventions rather than design-language choices — Windows itself derives
//! the initial 500 ms from the system double-click time — and the framework
//! baseline already matches. Nor is
//! `duration_indeterminate_sweep`: WinUI's indeterminate `ProgressBar` is a
//! multi-segment storyboard with no single published period, so inventing
//! one would be worse than inheriting a sane default.
//!
//! **Do not confuse these with Fluent 2's *web* motion tokens** (50 / 100 /
//! 150 / 200 ms). That is a separate system for Fluent UI React; the values
//! here are the Windows desktop control resources, which is what a native
//! Teksilo app should feel like.

use std::time::Duration;

use teksilo_tokens::{Easing, MotionTokens};

/// `ControlFasterAnimationDuration`.
pub const FLUENT_FASTER_MS: u64 = 83;
/// `ControlFastAnimationDuration`.
pub const FLUENT_FAST_MS: u64 = 167;
/// `ControlNormalAnimationDuration`.
pub const FLUENT_NORMAL_MS: u64 = 250;
/// The unnamed literal WinUI's flyout storyboards use for their longest
/// step. There is no `ControlSlowAnimationDuration` resource.
pub const FLUENT_SLOW_MS: u64 = 333;

/// `ControlFastOutSlowInKeySpline` — the key spline `"0,0,0,1"`.
pub const FLUENT_STANDARD_EASING: Easing = Easing::CubicBezier {
    x1: 0.0,
    y1: 0.0,
    x2: 0.0,
    y2: 1.0,
};

/// Fluent motion tokens.
pub fn fluent_motion() -> MotionTokens {
    MotionTokens {
        duration_instant: Duration::from_millis(0),
        duration_fast: Duration::from_millis(FLUENT_FAST_MS),
        duration_normal: Duration::from_millis(FLUENT_NORMAL_MS),
        duration_slow: Duration::from_millis(FLUENT_SLOW_MS),
        duration_collapse: Duration::from_millis(FLUENT_NORMAL_MS),
        easing_standard: FLUENT_STANDARD_EASING,
        // Tooltip delays and the indeterminate sweep keep the baseline —
        // see the module doc.
        ..MotionTokens::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_the_winui_control_resources() {
        let m = fluent_motion();
        assert_eq!(m.duration_fast, Duration::from_millis(167));
        assert_eq!(m.duration_normal, Duration::from_millis(250));
        assert_eq!(m.duration_slow, Duration::from_millis(333));
        assert_eq!(m.duration_instant, Duration::ZERO);
    }

    #[test]
    fn durations_are_ordered() {
        let m = fluent_motion();
        assert!(m.duration_instant < m.duration_fast);
        assert!(m.duration_fast < m.duration_normal);
        assert!(m.duration_normal < m.duration_slow);
    }

    #[test]
    fn easing_is_the_fast_out_slow_in_key_spline() {
        let m = fluent_motion();
        assert!(matches!(
            m.easing_standard,
            Easing::CubicBezier {
                x1: 0.0,
                y1: 0.0,
                x2: 0.0,
                y2: 1.0
            }
        ));
        // It must decelerate: half-way through the timeline the value is
        // already well past half-way to the target.
        assert!(m.easing_standard.apply(0.5) > 0.75);
        assert!(m.easing_standard.apply(0.0) <= 1e-6);
        assert!(m.easing_standard.apply(1.0) >= 1.0 - 1e-6);
    }

    #[test]
    fn tooltip_delays_keep_the_desktop_baseline() {
        let m = fluent_motion();
        let base = MotionTokens::default();
        assert_eq!(m.tooltip_delay, base.tooltip_delay);
        assert_eq!(m.tooltip_delay_heavy, base.tooltip_delay_heavy);
        assert_eq!(m.tooltip_reshow_delay, base.tooltip_reshow_delay);
        assert_eq!(
            m.duration_indeterminate_sweep,
            base.duration_indeterminate_sweep
        );
    }

    #[test]
    fn motion_is_quicker_than_the_intui_baseline_where_it_differs() {
        // Fluent's fast/normal steps are longer than IntUI's 120/200 —
        // IntUI deliberately animates almost nothing, Fluent animates
        // deliberately.
        let m = fluent_motion();
        let base = MotionTokens::default();
        assert!(m.duration_fast > base.duration_fast);
        assert!(m.duration_normal > base.duration_normal);
    }
}
