// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for the colour arithmetic
//! (crates/teksilo-kde-scheme/src/math.rs).
//!
//! Section d.5 of Teksilo's KDE specification requires the HCY conversion
//! to give back every 8-bit colour exactly; the golden vectors pin 62
//! colours, and this suite covers the rest of the space. Each case is a
//! handful of floating-point operations, so 1024 cases per property cost
//! well under a second. Manual override knob: `PROPTEST_CASES=N cargo test
//! -p teksilo-kde-scheme --test prop_math`.
//!
//! Safe run (engineering/docs/property-testing.md):
//!
//! ```text
//! cargo test -p teksilo-kde-scheme --test prop_math --no-run
//! BIN=$(cargo test -p teksilo-kde-scheme --test prop_math --no-run --message-format=json 2>/dev/null \
//!   | jq -r 'select(.executable != null) | .executable' | tail -1)
//! ( ulimit -v 6000000 -t 300; "$BIN" --test-threads=1 )
//! ```

use proptest::prelude::*;
use teksilo_kde_scheme::KdeColor;
use teksilo_kde_scheme::math::{Hcy, lift_luma_until};
use teksilo_tokens::contrast::composite_contrast;

/// Any 8-bit colour, alpha included. Cost: four bytes, no allocation.
fn arb_color8() -> impl Strategy<Value = KdeColor> {
    any::<[u8; 4]>().prop_map(|[r, g, b, a]| KdeColor::rgba8(r, g, b, a))
}

/// Up to four opaque 8-bit backdrops. Cost: at most four colours, each
/// measured once per lift step (256 steps), so about a thousand contrast
/// computations per case.
fn arb_backdrops() -> impl Strategy<Value = Vec<KdeColor>> {
    prop::collection::vec(
        any::<[u8; 3]>().prop_map(|[r, g, b]| KdeColor::rgb8(r, g, b)),
        0..4,
    )
}

fn worst(fg: KdeColor, backdrops: &[KdeColor]) -> f32 {
    backdrops
        .iter()
        .map(|bg| composite_contrast(fg.to_color(), bg.to_color()))
        .fold(f32::INFINITY, f32::min)
}

// ── 1. converting an 8-bit colour to HCY and back gives the same colour,
//      in all four 16-bit channels ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]

    #[test]
    fn every_eight_bit_colour_survives_the_hcy_round_trip(color in arb_color8()) {
        let hcy = Hcy::from_color(color);
        let back = hcy.to_color();
        prop_assert_eq!(back, color, "{:?} went to {:?} and came back as {:?}", color, hcy, back);
    }
}

// ── 2. the WCAG lift never makes the worst contrast over the backdrops
//      lower than it was, and keeps the alpha ──
proptest! {
    #[test]
    fn the_luma_lift_never_lowers_the_worst_contrast(
        color in arb_color8(),
        backdrops in arb_backdrops(),
        floor in 1.0_f32..21.0,
    ) {
        let lifted = lift_luma_until(color, &backdrops, floor);
        let (before, after) = (worst(color, &backdrops), worst(lifted, &backdrops));
        prop_assert!(
            after >= before,
            "{:?} on {:?} at floor {}: worst contrast {} fell to {} ({:?})",
            color, backdrops, floor, before, after, lifted,
        );
        prop_assert_eq!(lifted.a, color.a, "alpha changed: {:?} became {:?}", color, lifted);
    }
}
