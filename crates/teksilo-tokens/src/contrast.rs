// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! WCAG contrast helpers for colours a theme does not choose itself.
//!
//! A palette read from the desktop, or derived from a user's accent, can put
//! a foreground on a backdrop it was never checked against. These helpers
//! measure such a pair the way it is painted ([`composite_contrast`]) and,
//! when it falls short, move the foreground just far enough to read
//! ([`lift_until_readable`]).
//!
//! The usual floors are WCAG 2.x's: 4.5:1 for normal text (SC 1.4.3) and
//! 3:1 for the boundary of a control or a graphical object (SC 1.4.11).

use crate::color::Color;

/// The number of equal steps [`lift_until_readable`] takes between the
/// foreground and black or white. A step of 1 % moves an 8-bit channel by at
/// most 2.55 levels, so the lift overshoots its floor by very little.
const LIFT_STEPS: u32 = 100;

/// The WCAG 2.x contrast ratio of `fg` painted over `bg`, in `1.0..=21.0`.
///
/// Unlike [`Color::contrast_ratio`], which assumes both colours are already
/// opaque, this composites a translucent `fg` over `bg` first (straight
/// alpha, in sRGB, as the renderer blends), so a 40 % ink is measured as the
/// lighter colour it actually paints. `bg` is the backdrop and is treated as
/// opaque: its alpha is ignored.
pub fn composite_contrast(fg: Color, bg: Color) -> f32 {
    composite_over(fg, bg).contrast_ratio(bg.with_alpha(1.0))
}

/// `fg`, moved toward black or toward white until it reaches `floor` against
/// every colour in `on`.
///
/// Contrast is measured with [`composite_contrast`], so `fg`'s alpha is
/// honoured and kept: only its red, green and blue move. The direction is
/// whichever of black or white gives the higher minimum contrast over `on`,
/// so a colour on a light backdrop darkens and one on a dark backdrop
/// lightens. The colour moves in small steps and stops at the first that
/// passes, so it changes no more than it has to.
///
/// Returns `fg` unchanged when it already passes, or when `on` is empty. When
/// no step passes (a floor above what black or white can reach on these
/// backdrops, a very translucent `fg`, or backdrops on both sides of the
/// midpoint), returns the step with the highest minimum contrast, which for a
/// single backdrop is the saturated end: black or white.
pub fn lift_until_readable(fg: Color, on: &[Color], floor: f32) -> Color {
    let worst = |c: Color| -> f32 {
        on.iter()
            .map(|&bg| composite_contrast(c, bg))
            .fold(f32::INFINITY, f32::min)
    };
    if on.is_empty() || worst(fg) >= floor {
        return fg;
    }
    let toward = |target: Color, t: f32| -> Color {
        let inv = 1.0 - t;
        Color::new(
            fg.r() * inv + target.r() * t,
            fg.g() * inv + target.g() * t,
            fg.b() * inv + target.b() * t,
            fg.a(),
        )
    };
    let target = if worst(toward(Color::BLACK, 1.0)) >= worst(toward(Color::WHITE, 1.0)) {
        Color::BLACK
    } else {
        Color::WHITE
    };
    let mut best = (worst(fg), fg);
    for step in 1..=LIFT_STEPS {
        let candidate = toward(target, step as f32 / LIFT_STEPS as f32);
        let score = worst(candidate);
        if score >= floor {
            return candidate;
        }
        if score > best.0 {
            best = (score, candidate);
        }
    }
    best.1
}

/// `fg` composited over an opaque `bg`.
fn composite_over(fg: Color, bg: Color) -> Color {
    // A NaN alpha would poison every channel; read it as opaque, the safest
    // meaning of an undefined alpha for a colour meant to be read.
    let a = if fg.a().is_nan() {
        1.0
    } else {
        fg.a().clamp(0.0, 1.0)
    };
    let inv = 1.0 - a;
    Color::new(
        fg.r() * a + bg.r() * inv,
        fg.g() * a + bg.g() * inv,
        fg.b() * a + bg.b() * inv,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Color {
        Color::from_hex(s)
    }

    #[test]
    fn an_opaque_foreground_measures_as_contrast_ratio() {
        for (fg, bg) in [
            ("#000000", "#FFFFFF"),
            ("#3DAEE9", "#EFF0F1"),
            ("#DA4453", "#232629"),
        ] {
            let (fg, bg) = (hex(fg), hex(bg));
            assert!((composite_contrast(fg, bg) - fg.contrast_ratio(bg)).abs() < 1e-5);
        }
        assert!((composite_contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
    }

    #[test]
    fn a_translucent_foreground_is_composited_first() {
        let fg = Color::BLACK.with_alpha(0.5);
        let bg = Color::WHITE;
        let painted = Color::new(0.5, 0.5, 0.5, 1.0);
        assert!((composite_contrast(fg, bg) - painted.contrast_ratio(bg)).abs() < 1e-5);
        // `contrast_ratio` alone ignores the alpha and overstates it.
        assert!(composite_contrast(fg, bg) < fg.contrast_ratio(bg));
        // A fully transparent ink is invisible.
        assert!((composite_contrast(Color::BLACK.with_alpha(0.0), bg) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_backdrop_alpha_is_ignored() {
        let fg = hex("#336699");
        let bg = hex("#EEEEEE");
        assert_eq!(
            composite_contrast(fg, bg),
            composite_contrast(fg, bg.with_alpha(0.2))
        );
    }

    #[test]
    fn a_readable_colour_is_returned_unchanged() {
        let fg = hex("#232629");
        assert_eq!(lift_until_readable(fg, &[hex("#FFFFFF")], 4.5), fg);
        assert_eq!(lift_until_readable(fg, &[], 21.0), fg);
    }

    #[test]
    fn a_colour_on_a_light_backdrop_darkens_just_enough() {
        // Breeze's light negative text, #DA4453, is about 4.0:1 on white.
        let fg = hex("#DA4453");
        let bg = Color::WHITE;
        assert!(composite_contrast(fg, bg) < 4.5);
        let lifted = lift_until_readable(fg, &[bg], 4.5);
        let ratio = composite_contrast(lifted, bg);
        assert!(ratio >= 4.5, "{ratio}");
        assert!(ratio < 4.8, "lifted further than it had to: {ratio}");
        assert!(lifted.relative_luminance() < fg.relative_luminance());
        // The hue survives: red stays the dominant channel.
        assert!(lifted.r() > lifted.g() && lifted.r() > lifted.b());
    }

    #[test]
    fn a_colour_on_a_dark_backdrop_lightens() {
        let fg = hex("#2980B9");
        let bg = hex("#1B1E20");
        assert!(composite_contrast(fg, bg) < 4.5);
        let lifted = lift_until_readable(fg, &[bg], 4.5);
        assert!(composite_contrast(lifted, bg) >= 4.5);
        assert!(lifted.relative_luminance() > fg.relative_luminance());
    }

    #[test]
    fn every_backdrop_must_pass() {
        let fg = hex("#7F8C8D");
        let on = [hex("#FFFFFF"), hex("#EFF0F1"), hex("#DEE0E2")];
        let lifted = lift_until_readable(fg, &on, 4.5);
        for bg in on {
            assert!(composite_contrast(lifted, bg) >= 4.5);
        }
    }

    #[test]
    fn the_alpha_is_kept_and_counted() {
        let fg = hex("#000000").with_alpha(0.35);
        let bg = Color::WHITE;
        let lifted = lift_until_readable(fg, &[bg], 3.0);
        assert_eq!(lifted.a(), fg.a());
        // Already black: no step can darken it further, so it saturates.
        assert_eq!(lifted, fg);
        let grey = hex("#888888").with_alpha(0.8);
        let lifted = lift_until_readable(grey, &[bg], 4.5);
        assert_eq!(lifted.a(), 0.8);
        assert!(composite_contrast(lifted, bg) >= 4.5);
    }

    #[test]
    fn an_unreachable_floor_saturates() {
        let fg = hex("#808080");
        let bg = Color::WHITE;
        // Nothing reaches 25:1; the lift ends at black.
        assert_eq!(lift_until_readable(fg, &[bg], 25.0), Color::BLACK);
        let dark = hex("#000000");
        assert_eq!(lift_until_readable(fg, &[dark], 25.0), Color::WHITE);
    }

    #[test]
    fn backdrops_on_both_sides_keep_the_best_step() {
        // Mid grey between white and black can never reach 4.5:1 on both;
        // the result is the best compromise and never worse than the input.
        let fg = hex("#606060");
        let on = [Color::WHITE, Color::BLACK];
        let worst = |c: Color| {
            on.iter()
                .map(|&bg| composite_contrast(c, bg))
                .fold(f32::INFINITY, f32::min)
        };
        let lifted = lift_until_readable(fg, &on, 4.5);
        assert!(worst(lifted) >= worst(fg));
    }

    #[test]
    fn a_nan_alpha_reads_as_opaque() {
        let fg = Color::new(0.0, 0.0, 0.0, f32::NAN);
        assert!((composite_contrast(fg, Color::WHITE) - 21.0).abs() < 1e-3);
    }
}
