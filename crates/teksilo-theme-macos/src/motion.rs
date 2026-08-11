// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS motion tokens.
//!
//! Apple publishes exactly **one** animation duration for AppKit —
//! `NSAnimationContext`'s default, and the implicit duration a
//! `CATransaction` runs at when nothing overrides it, both **0.25 s**.
//! That is the value a Core Animation property change picks up when the
//! app says nothing about timing, which makes it the closest thing macOS
//! has to a "normal" step, and it is transcribed as
//! [`MACOS_NORMAL_MS`]. The fast and slow steps around it are
//! `[derived]`: sheet, popover, window-zoom and menu-fade durations are
//! all unpublished, and inventing precise-looking numbers for them would
//! be worse than deriving two obvious neighbours from the one real value.
//!
//! The easing is `kCAMediaTimingFunctionEaseInEaseOut`, the default Core
//! Animation curve — `cubic-bezier(0.42, 0, 0.58, 1)`. Apple documents
//! the four named curves qualitatively and never states their control
//! points; the quadruple above is the standard, cross-corroborated
//! definition and matches CSS's `ease-in-out` exactly.
//!
//! That symmetry is the point, and it is what makes macOS motion feel
//! different from the other two presets Teksilo ships. Fluent's
//! `ControlFastOutSlowInKeySpline` is `(0, 0, 0, 1)` — it leaves at full
//! speed and settles asymptotically, so Fluent motion reads as
//! *arriving*. Material 3's emphasised curve is likewise front-loaded.
//! macOS eases *in and out equally*: things start gently, travel, and
//! stop gently, which reads as *unhurried* rather than eager.
//!
//! **Not overridden:** the three tooltip delays and the indeterminate
//! sweep. Help-tag delay is a desktop convention rather than a design-
//! language choice, macOS publishes no figure for it, and the framework
//! baseline already sits where every desktop toolkit puts it.

use std::time::Duration;

use teksilo_tokens::{Easing, MotionTokens};

/// `NSAnimationContext.defaultDuration` / `CATransaction`'s implicit
/// duration — the one duration Apple actually publishes.
pub const MACOS_NORMAL_MS: u64 = 250;
/// `[derived]` — a shorter step for hover and press feedback, which
/// macOS runs faster than a layout change but does not name.
pub const MACOS_FAST_MS: u64 = 150;
/// `[derived]` — a longer step for sheets and window-scale transitions.
pub const MACOS_SLOW_MS: u64 = 350;

/// `kCAMediaTimingFunctionEaseInEaseOut` — `cubic-bezier(0.42, 0, 0.58, 1)`.
pub const MACOS_STANDARD_EASING: Easing = Easing::CubicBezier {
    x1: 0.42,
    y1: 0.0,
    x2: 0.58,
    y2: 1.0,
};

/// macOS motion tokens.
pub fn macos_motion() -> MotionTokens {
    MotionTokens {
        duration_instant: Duration::from_millis(0),
        duration_fast: Duration::from_millis(MACOS_FAST_MS),
        duration_normal: Duration::from_millis(MACOS_NORMAL_MS),
        duration_slow: Duration::from_millis(MACOS_SLOW_MS),
        // A disclosure triangle turning and its content unfolding is the
        // canonical 0.25 s Core Animation change.
        duration_collapse: Duration::from_millis(MACOS_NORMAL_MS),
        easing_standard: MACOS_STANDARD_EASING,
        // Tooltip delays and the indeterminate sweep keep the baseline —
        // see the module doc.
        ..MotionTokens::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_is_the_published_core_animation_default() {
        assert_eq!(
            macos_motion().duration_normal,
            Duration::from_millis(250),
            "0.25 s is the one duration Apple publishes"
        );
    }

    #[test]
    fn durations_are_ordered() {
        let m = macos_motion();
        assert!(m.duration_instant < m.duration_fast);
        assert!(m.duration_fast < m.duration_normal);
        assert!(m.duration_normal < m.duration_slow);
    }

    /// The signature difference from Fluent's `(0, 0, 0, 1)` and from
    /// Material 3's front-loaded emphasised curve.
    #[test]
    fn the_easing_is_symmetric_rather_than_front_loaded() {
        let e = macos_motion().easing_standard;
        assert!(matches!(
            e,
            Easing::CubicBezier {
                x1: 0.42,
                y1: 0.0,
                x2: 0.58,
                y2: 1.0
            }
        ));
        // Halfway through the timeline, halfway to the target — that is
        // what "eases in and out equally" means, and it is exactly what a
        // decelerate-only curve does not do.
        assert!((e.apply(0.5) - 0.5).abs() < 0.02);
        // …and it is genuinely eased at both ends, not linear.
        assert!(e.apply(0.25) < 0.25, "must ease in");
        assert!(e.apply(0.75) > 0.75, "must ease out");
        assert!(e.apply(0.0) <= 1e-6);
        assert!(e.apply(1.0) >= 1.0 - 1e-6);
    }

    #[test]
    fn it_is_not_fluents_decelerate_only_curve() {
        // Pins the contrast the module doc draws: Fluent's curve is
        // already past three-quarters at the midpoint.
        let fluent = Easing::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 0.0,
            y2: 1.0,
        };
        assert!(fluent.apply(0.5) > 0.75);
        assert!(macos_motion().easing_standard.apply(0.5) < 0.75);
    }

    #[test]
    fn tooltip_delays_and_the_sweep_keep_the_desktop_baseline() {
        let m = macos_motion();
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
    fn macos_animates_more_deliberately_than_intui() {
        // IntUI's philosophy is that hover and press are instant and
        // animation is reserved for floating elements; macOS animates
        // nearly everything, and takes longer over it.
        let m = macos_motion();
        let base = MotionTokens::default();
        assert!(m.duration_fast > base.duration_fast);
        assert!(m.duration_normal > base.duration_normal);
        assert!(m.duration_slow > base.duration_slow);
    }
}
