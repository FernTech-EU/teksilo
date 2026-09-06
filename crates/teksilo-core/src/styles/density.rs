// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Density helpers — the three functions that turn a hard-coded dimension into
//! a density-aware one.
//!
//! These live in `teksilo-core` rather than `teksilo-widgets` so that
//! `teksilo-terminal`, `teksilo-webview`, `teksilo-scene` and the future
//! `target_audit` can all reach them without a widgets dependency;
//! `teksilo-widgets` re-exports them for ergonomics.
//!
//! Each helper is the **identity at [`TargetDensity::Compact`]** for any value
//! that already conforms, which is what makes the density layer a no-op until
//! an app opts in:
//!
//! ```
//! use teksilo_core::styles::density::{dp, spacing};
//! use teksilo_tokens::{InputTokens, TargetDensity, TargetRole};
//!
//! let compact = InputTokens::for_density(TargetDensity::Compact);
//! assert_eq!(dp(28.0, TargetRole::Target, &compact), 28.0);
//! assert_eq!(spacing(8.0, &compact), 8.0);
//!
//! let touch = InputTokens::for_density(TargetDensity::Touch);
//! assert_eq!(dp(28.0, TargetRole::Target, &touch), 44.0);
//! ```
//!
//! Reference: `docs/density-and-targets.md`.
//!
//! [`TargetDensity::Compact`]: teksilo_tokens::TargetDensity::Compact

use teksilo_canvas::Size;
use teksilo_tokens::{InputTokens, TargetAxes, TargetRole};

/// Raise one dimension to the floor its [`TargetRole`] demands.
///
/// - [`TargetRole::Target`] — at least `tokens.target_size`, which is itself
///   never below `tokens.min_target_conformance` (24 dp, WCAG 2.2 SC 2.5.8 AA).
/// - [`TargetRole::Grab`] — at least `tokens.grab_size`.
/// - [`TargetRole::Decoration`] — returned unchanged; a rule or an icon is not
///   a target and must not grow.
///
/// The function only ever raises: a control that is already generous keeps its
/// own size. A negative or non-finite `base` is returned as-is for a
/// `Decoration`, and clamped up to the floor for the two interactive roles.
pub fn dp(base: f32, role: TargetRole, tokens: &InputTokens) -> f32 {
    match role {
        TargetRole::Target => base.max(tokens.target_size),
        TargetRole::Grab => base.max(tokens.grab_size),
        TargetRole::Decoration => base,
    }
}

/// Scale a gap or padding by the density's `spacing_factor`
/// (1.00 / 1.15 / 1.30).
///
/// Unlike [`dp`] this both grows and shrinks in principle, but no shipped
/// ladder has a factor below 1.0, so in practice it only ever grows.
pub fn spacing(base: f32, tokens: &InputTokens) -> f32 {
    base * tokens.spacing_factor
}

/// Raise the named axes of a minimum size to `tokens.target_size`.
///
/// The axes not named are passed through untouched — a full-width list row
/// passes [`TargetAxes::HEIGHT`] so its height reaches the target while its
/// width stays whatever the layout gives it.
pub fn density_min_size(base: Size, axes: TargetAxes, tokens: &InputTokens) -> Size {
    Size {
        width: if axes.has_width() {
            base.width.max(tokens.target_size)
        } else {
            base.width
        },
        height: if axes.has_height() {
            base.height.max(tokens.target_size)
        } else {
            base.height
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_tokens::TargetDensity;

    fn compact() -> InputTokens {
        InputTokens::for_density(TargetDensity::Compact)
    }

    /// The whole density layer must be inert until an app opts in: at Compact,
    /// a dimension that already conforms comes back untouched.
    #[test]
    fn compact_is_the_identity_for_conforming_values() {
        let t = compact();
        for base in [24.0_f32, 28.0, 32.0, 40.0, 48.0, 100.0] {
            assert_eq!(dp(base, TargetRole::Target, &t), base);
            assert_eq!(dp(base, TargetRole::Grab, &t), base);
            assert_eq!(dp(base, TargetRole::Decoration, &t), base);
            assert_eq!(spacing(base, &t), base);
        }
        let s = Size {
            width: 30.0,
            height: 26.0,
        };
        assert_eq!(density_min_size(s, TargetAxes::BOTH, &t), s);
    }

    /// A decoration is never grown, at any density — that is the whole point of
    /// distinguishing it from a target.
    #[test]
    fn decorations_are_never_scaled() {
        for d in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let t = InputTokens::for_density(d);
            assert_eq!(dp(1.0, TargetRole::Decoration, &t), 1.0);
            assert_eq!(dp(0.0, TargetRole::Decoration, &t), 0.0);
        }
    }

    /// A `Target` may never come back below the 24 dp WCAG floor, whatever it
    /// started at and whatever density is active.
    #[test]
    fn a_target_never_lands_below_the_conformance_floor() {
        for d in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let t = InputTokens::for_density(d);
            for base in [0.0_f32, 1.0, 12.0, 23.9, 24.0] {
                assert!(dp(base, TargetRole::Target, &t) >= t.min_target_conformance);
            }
        }
    }

    #[test]
    fn the_touch_ladder_raises_targets_and_grabs() {
        let t = InputTokens::for_density(TargetDensity::Touch);
        assert_eq!(dp(24.0, TargetRole::Target, &t), 44.0);
        assert_eq!(dp(6.0, TargetRole::Grab, &t), 16.0);
        assert_eq!(dp(60.0, TargetRole::Target, &t), 60.0);
        assert_eq!(spacing(10.0, &t), 13.0);
    }

    #[test]
    fn min_size_touches_only_the_named_axes() {
        let t = InputTokens::for_density(TargetDensity::Touch);
        let base = Size {
            width: 10.0,
            height: 10.0,
        };
        assert_eq!(
            density_min_size(base, TargetAxes::HEIGHT, &t),
            Size {
                width: 10.0,
                height: 44.0
            }
        );
        assert_eq!(
            density_min_size(base, TargetAxes::WIDTH, &t),
            Size {
                width: 44.0,
                height: 10.0
            }
        );
        assert_eq!(
            density_min_size(base, TargetAxes::BOTH, &t),
            Size {
                width: 44.0,
                height: 44.0
            }
        );
    }
}
