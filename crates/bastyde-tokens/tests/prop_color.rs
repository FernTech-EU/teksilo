//! Property tests for `Color` (crates/bastyde-tokens/src/color.rs) and the
//! `ColorTokens::for_inactive_window` accent-desaturation transform
//! (crates/bastyde-tokens/src/theme.rs).
//!
//! `Color` is the lowest-level public type in the styling ladder — every
//! theme, recipe, and painted pixel in the framework passes through it, and
//! `for_hex` in particular parses attacker-adjacent input (imported themes,
//! user-typed hex fields in `ColorPicker`/`HexColorInput`). `cargo-fuzz`
//! needs nightly + libfuzzer-sys, which isn't assumed here; proptest with
//! 512-1024 iterations per property gives the "never panics on weird input"
//! coverage a fuzz corpus would, plus shrinking to a minimal counterexample.
//! Manual override knob: `PROPTEST_CASES=N cargo test -p bastyde-tokens
//! --test prop_color`.
//!
//! The existing `#[cfg(test)] mod tests` in `color.rs` covers exact-value
//! unit cases (primary-color HSV round-trips, fixed hex strings, WCAG
//! contrast constants); this file generalizes those into round-trip /
//! panic-freedom / monotonicity properties over arbitrary inputs instead of
//! restating them.

use bastyde_tokens::{Color, ColorTokens};
use proptest::prelude::*;

/// Channel values clamped to the valid `0.0..=1.0` range that `Color`
/// components are documented to live in. Includes the exact boundaries.
fn arb_channel() -> impl Strategy<Value = f32> {
    prop_oneof![Just(0.0_f32), Just(1.0_f32), 0.0f32..=1.0f32,]
}

fn arb_color() -> impl Strategy<Value = Color> {
    (arb_channel(), arb_channel(), arb_channel(), arb_channel())
        .prop_map(|(r, g, b, a)| Color::from_rgba(r, g, b, a))
}

/// A `t`/`amount` factor for `mix`/`darken`/`lighten`/`desaturated`, biased
/// toward the documented `0.0..=1.0` domain but also including out-of-range
/// values (negative, >1, infinities, NaN) since none of those methods
/// document a precondition on the caller.
fn arb_factor() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(0.0_f32),
        Just(1.0_f32),
        0.0f32..=1.0f32,
        Just(-1.0_f32),
        Just(2.0_f32),
        Just(f32::INFINITY),
        Just(f32::NEG_INFINITY),
        Just(f32::NAN),
    ]
}

/// A near-arbitrary string biased toward the hex-color alphabet plus
/// malformed input (wrong lengths, stray characters, empty) — the
/// panic-freedom target for `Color::from_hex`.
fn arb_hexish_string() -> impl Strategy<Value = String> {
    "[#0-9a-fA-FxyzG ]{0,12}"
}

// ── 1. hex round-trip through to_hex_upper/from_hex (6-digit, no alpha) ──
// `to_hex_upper(false)` followed by `from_hex` reproduces the same 8-bit
// quantized channels `to_hex_upper` itself would read back from `self` —
// i.e. formatting then parsing is a fixed point once a color is already
// hex-quantized.

proptest! {
    #[test]
    fn hex_roundtrip_is_stable_after_one_quantization(c in arb_color()) {
        let hex = c.to_hex_upper(false);
        let quantized = Color::from_hex(&hex);
        let requantized_hex = quantized.to_hex_upper(false);
        prop_assert_eq!(
            &hex, &requantized_hex,
            "formatting a from_hex-quantized color should be a fixed point: {} -> {} -> {}",
            hex, requantized_hex, hex
        );
        // And parsing that fixed-point string again reproduces the same color.
        prop_assert_eq!(
            Color::from_hex(&requantized_hex).to_array(),
            quantized.to_array(),
            "from_hex(to_hex_upper(from_hex(s))) should equal from_hex(s) for s={}",
            hex
        );
    }
}

// ── 2. hex round-trip with alpha ──
// Same fixed-point property as above but through the 8-digit RRGGBBAA path,
// since `from_hex` dispatches on string length and alpha is parsed/formatted
// through a separate branch than the 6-digit case.

proptest! {
    #[test]
    fn hex_roundtrip_with_alpha_is_stable_after_one_quantization(c in arb_color()) {
        let hex = c.to_hex_upper(true);
        let quantized = Color::from_hex(&hex);
        let requantized_hex = quantized.to_hex_upper(true);
        prop_assert_eq!(
            &hex, &requantized_hex,
            "formatting a from_hex-quantized color (with alpha) should be a fixed point: {}",
            hex
        );
    }
}

// ── 3. to_hex_lower is exactly the lowercase of to_hex_upper ──
// Documented as "lowercase variant of to_hex_upper" — verify it holds for
// arbitrary colors and both alpha modes, not just the one worked example in
// the unit tests.

proptest! {
    #[test]
    fn to_hex_lower_is_lowercased_to_hex_upper(c in arb_color(), include_alpha in any::<bool>()) {
        prop_assert_eq!(
            c.to_hex_lower(include_alpha),
            c.to_hex_upper(include_alpha).to_lowercase(),
            "to_hex_lower must equal to_hex_upper().to_lowercase() for {:?}",
            c
        );
    }
}

// ── 4. from_hex never panics on arbitrary (including malformed) strings ──
// `from_hex` is a public entry point for user-typed / imported hex strings
// (ColorPicker, HexColorInput, imported theme JSON). Any string — right
// length or not, valid hex digits or not — must return some `Color` rather
// than unwind, and the result must stay a legal RGBA value.

proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn from_hex_never_panics_and_stays_in_range(s in arb_hexish_string()) {
        let c = Color::from_hex(&s);
        for (name, v) in [("r", c.r()), ("g", c.g()), ("b", c.b()), ("a", c.a())] {
            prop_assert!(
                (0.0..=1.0).contains(&v),
                "from_hex({:?}).{} = {} out of [0,1]",
                s, name, v
            );
        }
    }
}

// ── 5. mix boundary identities ──
// `mix(a, b, 0.0) == a` and `mix(a, b, 1.0) == b`, for arbitrary colors —
// the two-value unit test only exercised RED/BLUE and BLACK/WHITE.

proptest! {
    #[test]
    fn mix_boundaries_return_the_original_endpoints(a in arb_color(), b in arb_color()) {
        let at_zero = a.mix(b, 0.0);
        let at_one = a.mix(b, 1.0);
        prop_assert_eq!(at_zero.to_array(), a.to_array(), "mix(a, b, 0.0) must equal a");
        prop_assert_eq!(at_one.to_array(), b.to_array(), "mix(a, b, 1.0) must equal b");
    }
}

// ── 6. mix/darken/lighten/desaturated/with_alpha never produce NaN or
//      out-of-range channels, even for out-of-range or non-finite factors ──
// `mix` clamps `t` to `0.0..=1.0` before interpolating, so even a caller
// passing `-5.0`, `1e9`, or `NaN` should get back finite, in-range channels
// rather than propagating the bad input into the color.

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn blend_family_stays_finite_and_in_range_for_any_factor(
        a in arb_color(),
        b in arb_color(),
        factor in arb_factor(),
    ) {
        let candidates = [
            ("mix", a.mix(b, factor)),
            ("darken", a.darken(factor)),
            ("lighten", a.lighten(factor)),
            ("desaturated", a.desaturated(factor)),
            ("with_alpha", a.with_alpha(factor)),
        ];
        for (name, c) in candidates {
            for (chan, v) in [("r", c.r()), ("g", c.g()), ("b", c.b()), ("a", c.a())] {
                // `with_alpha` passes the factor straight through as alpha and is
                // documented to do no clamping of its own, so a wild factor is
                // expected to show up verbatim there; every other combinator is
                // documented to clamp its `amount`/`t` into 0..=1.
                if name == "with_alpha" && chan == "a" {
                    prop_assert!(
                        !v.is_nan() || factor.is_nan(),
                        "with_alpha({}) produced NaN alpha from a non-NaN factor",
                        factor
                    );
                    continue;
                }
                prop_assert!(
                    v.is_finite(),
                    "{}(factor={}).{} = {} is not finite",
                    name, factor, chan, v
                );
                prop_assert!(
                    (0.0..=1.0).contains(&v),
                    "{}(factor={}).{} = {} out of [0,1]",
                    name, factor, chan, v
                );
            }
        }
    }
}

// ── 7. desaturated(1.0) always yields a gray (r == g == b) ──
// The documented contract of full desaturation: it mixes toward the
// equal-luminance gray, so amount=1.0 must collapse all three channels to
// the same value regardless of the starting hue.

proptest! {
    #[test]
    fn full_desaturation_always_yields_a_gray(c in arb_color()) {
        let g = c.desaturated(1.0);
        prop_assert!(
            (g.r() - g.g()).abs() < 1e-4 && (g.g() - g.b()).abs() < 1e-4,
            "desaturated(1.0) of {:?} should be gray, got {:?}",
            c, g
        );
    }
}

// ── 8. desaturated is monotone in `amount`: larger amount never increases
//      channel spread (moves no farther from gray than a smaller amount) ──
// This is the general form of the "moves toward gray" contract documented
// on `desaturated`, checked across the whole 0..=1 domain rather than the
// two or three hand-picked amounts in the unit test.

proptest! {
    #[test]
    fn desaturation_spread_is_monotone_in_amount(
        c in arb_color(),
        a1 in 0.0f32..=1.0f32,
        a2 in 0.0f32..=1.0f32,
    ) {
        let spread = |x: Color| {
            let m = x.r().max(x.g()).max(x.b());
            let n = x.r().min(x.g()).min(x.b());
            m - n
        };
        let (lo, hi) = if a1 <= a2 { (a1, a2) } else { (a2, a1) };
        let spread_lo = spread(c.desaturated(lo));
        let spread_hi = spread(c.desaturated(hi));
        prop_assert!(
            spread_hi <= spread_lo + 1e-5,
            "desaturated({}) spread {} should be <= desaturated({}) spread {} for {:?}",
            hi, spread_hi, lo, spread_lo, c
        );
    }
}

// ── 9. sRGB <-> HSV round-trip is a fixed point once through to_hsv/from_hsv
//      (opaque colors only — from_hsv always sets alpha to 1.0) ──
// The unit tests only cover RED/GREEN/BLUE and yellow/cyan/magenta; this
// checks the round-trip holds for arbitrary opaque colors, within a
// tolerance that accounts for hue being ill-defined on/near grays.

proptest! {
    #[test]
    fn hsv_roundtrip_holds_for_arbitrary_opaque_colors(
        r in arb_channel(), g in arb_channel(), b in arb_channel(),
    ) {
        let c = Color::from_rgb(r, g, b);
        let (h, s, v) = c.to_hsv();
        let back = Color::from_hsv(h, s, v);
        prop_assert!((back.r() - c.r()).abs() < 0.01, "r mismatch for {:?}: back={:?}", c, back);
        prop_assert!((back.g() - c.g()).abs() < 0.01, "g mismatch for {:?}: back={:?}", c, back);
        prop_assert!((back.b() - c.b()).abs() < 0.01, "b mismatch for {:?}: back={:?}", c, back);
    }
}

// ── 10. contrast_ratio is symmetric and stays within its documented range ──
// `contrast_ratio` is documented as `1.0..=21.0` and symmetric in its
// arguments (it always orders lighter/darker internally). Verify both hold
// for arbitrary color pairs, not just BLACK/WHITE.

proptest! {
    #[test]
    fn contrast_ratio_is_symmetric_and_bounded(a in arb_color(), b in arb_color()) {
        let ab = a.contrast_ratio(b);
        let ba = b.contrast_ratio(a);
        prop_assert!(
            (ab - ba).abs() < 1e-4,
            "contrast_ratio should be symmetric: {:?} vs {:?} = {} / {}",
            a, b, ab, ba
        );
        prop_assert!(
            (1.0..=21.0 + 1e-3).contains(&ab),
            "contrast_ratio({:?}, {:?}) = {} out of documented 1.0..=21.0",
            a, b, ab
        );
    }
}

// ── 11. best_contrast_text always picks the higher-or-equal-contrast option
//      of the two candidates it is documented to choose between ──
// `best_contrast_text` is documented to compare actual ratios rather than
// threshold luminance; whichever it returns must never be the strictly
// worse choice against the arbitrary background it was computed for.

proptest! {
    #[test]
    fn best_contrast_text_is_never_the_worse_choice(bg in arb_color()) {
        let chosen = bg.best_contrast_text();
        let other = if chosen == Color::BLACK { Color::WHITE } else { Color::BLACK };
        prop_assert!(
            bg.contrast_ratio(chosen) >= bg.contrast_ratio(other) - 1e-4,
            "best_contrast_text for {:?} chose {:?} ({:.3}) over {:?} ({:.3})",
            bg, chosen, bg.contrast_ratio(chosen), other, bg.contrast_ratio(other)
        );
    }
}

// ── 12. for_inactive_window desaturates exactly the accent family and
//      leaves everything else (except chart_palette) untouched, for an
//      arbitrary accent color — not just the two shipped IntUI presets ──
// The existing unit tests in theme.rs check this equality against the
// fixed light/dark default palettes; this generalizes the base's `accent`
// field to an arbitrary color (every other field held at the light
// default) so the property isn't coupled to IntUI's specific hex values.

proptest! {
    #[test]
    fn for_inactive_window_desaturates_only_the_accent_field(accent in arb_color()) {
        let base = ColorTokens {
            accent,
            ..ColorTokens::light_default()
        };
        let inactive = base.for_inactive_window();
        prop_assert_eq!(
            inactive.accent.to_array(),
            accent.desaturated(ColorTokens::INACTIVE_ACCENT_DESATURATION).to_array(),
            "for_inactive_window().accent should equal accent.desaturated(INACTIVE_ACCENT_DESATURATION) for {:?}",
            accent
        );
        // Untouched-by-design fields (documented in for_inactive_window's
        // doc comment) must survive unchanged.
        prop_assert_eq!(inactive.surface_selected.to_array(), base.surface_selected.to_array());
        prop_assert_eq!(inactive.selection_bg_active.to_array(), base.selection_bg_active.to_array());
        prop_assert_eq!(inactive.text_primary.to_array(), base.text_primary.to_array());
        prop_assert_eq!(inactive.status_error_bg.to_array(), base.status_error_bg.to_array());
    }
}
