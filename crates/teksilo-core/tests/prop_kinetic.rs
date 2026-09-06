// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property tests for the kinetic-scrolling core.
//!
//! Placement: `tests/`, because every item under test (`rubber_band`,
//! `ClampingSimulation`, `BouncingSimulation`, `KineticScroller`) is `pub` in
//! the `pub mod kinetic` of `teksilo-core` and therefore reachable from an
//! external test crate. See `docs/property-testing.md` for the decision
//! procedure.
//!
//! Case counts: proptest's default 256 everywhere. Every property here
//! evaluates a handful of closed-form expressions per case — no allocation
//! beyond the sample-time vector, no I/O, no tree — so the suite is cheap; the
//! reason to raise a count would be a *narrow* failure region, and none of
//! these properties has one. Deeper run:
//!
//! ```text
//! PROPTEST_CASES=4096 cargo test -p teksilo-core --test prop_kinetic
//! ```
//!
//! **Run these under the safe protocol** (`docs/property-testing.md`, "The safe
//! run protocol"): build with `--no-run`, then run the binary under
//! `ulimit -v`. That document opens with an incident in which an uncoupled
//! generator pairing a `1e6` extent with a `1.0` cell size asked for ~1e12
//! allocations and took a workstation down three times. The generators below
//! are coupled by `prop_flat_map` for exactly that reason: an offset is drawn
//! *from* its extent, and a release position is drawn *from* its range, so no
//! generated pair can put a 1e6 quantity next to a 1e-6 one.
//!
//! ```text
//! cargo test -p teksilo-core --test prop_kinetic --no-run
//! BIN=$(cargo test -p teksilo-core --test prop_kinetic --no-run --message-format=json 2>/dev/null \
//!   | jq -r 'select(.executable != null) | .executable' | tail -1)
//! ( ulimit -v 6000000 -t 300; "$BIN" --test-threads=1 )
//! ```

use std::time::Duration;

use proptest::prelude::*;
use teksilo_canvas::{Point, Vec2};
use teksilo_core::kinetic::{
    BouncingSimulation, ClampingSimulation, KineticScroller, ScrollSimulation, rubber_band,
    rubber_band_inverse,
};
use teksilo_core::pointer::EventTime;
use teksilo_tokens::{GestureProfile, OverscrollStyle, ScrollPhysicsTokens};

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

/// A viewport extent, in logical pixels.
///
/// Cost: a scalar. Bounded at 4000 because that is a generous 4K viewport edge
/// and because every dependent quantity below is derived *from* this value —
/// letting it reach 1e9 would let a coupled offset reach 1e10 and put the
/// closed-form arithmetic into the region where `f32` has no fractional bits
/// left, which tests the float format rather than the curve.
fn arb_extent() -> impl Strategy<Value = f32> {
    1.0f32..4000.0f32
}

/// An extent paired with an overscroll offset drawn *for that extent*.
///
/// Cost: two scalars per case. The coupling is the point: the rubber band's
/// whole behaviour is a function of `offset / extent`, so an independently
/// drawn pair would spend most of its cases at a ratio of either ~0 or ~10⁶
/// and almost none in the region a finger actually visits. Eight extents of
/// travel is already far past the asymptote.
fn arb_extent_and_offset() -> impl Strategy<Value = (f32, f32)> {
    arb_extent().prop_flat_map(|e| (Just(e), -8.0f32 * e..8.0f32 * e))
}

/// An extent with two offsets ordered `a <= b`, for the monotonicity property.
fn arb_extent_and_ordered_offsets() -> impl Strategy<Value = (f32, f32, f32)> {
    arb_extent_and_offset().prop_flat_map(|(e, a)| (Just(e), Just(a), a..8.0f32 * e))
}

/// A scroll range `(min, max)` with `min <= max`.
///
/// Cost: two scalars. The span is bounded at 20 000 dp — a long document, and
/// small enough that `min + span` keeps full `f32` fractional precision.
fn arb_range() -> impl Strategy<Value = (f32, f32)> {
    (-5000.0f32..5000.0f32, 0.0f32..20_000.0f32).prop_map(|(min, span)| (min, min + span))
}

/// A range with a release position drawn *inside* it.
///
/// Cost: three scalars. Coupled, so a release can never be a million pixels
/// outside a ten-pixel range — a state no scrollable reaches and whose only
/// effect would be to test `f32` saturation.
fn arb_range_and_inside_position() -> impl Strategy<Value = (f32, f32, f32)> {
    arb_range().prop_flat_map(|(min, max)| (Just(min), Just(max), min..=max))
}

/// A release velocity, in logical pixels per second.
///
/// Bounded by `GestureProfile::MOUSE.max_fling_velocity` (8000, Android's
/// `MAXIMUM_FLING_VELOCITY`), which is the largest value the fling path will
/// ever be handed: everything above it is clamped down before a simulation is
/// built.
fn arb_velocity() -> impl Strategy<Value = f32> {
    -8000.0f32..8000.0f32
}

/// Up to 24 evaluation instants inside `span` seconds, sorted.
///
/// Cost: a `Vec` of at most 24 `f32`s, plus one closed-form simulation
/// evaluation each — the dominant per-case cost of this suite, and a bounded
/// one. 24 is chosen over a larger count for the reason
/// `docs/property-testing.md` gives: properties find bugs through many small
/// cases, and a shrunk counterexample with 24 instants is still readable.
fn arb_instants(span: f32) -> impl Strategy<Value = Vec<Duration>> {
    prop::collection::vec(0.0f32..span, 1..24).prop_map(|mut v| {
        v.sort_by(|a, b| a.partial_cmp(b).expect("the generator draws no NaN"));
        v.into_iter().map(Duration::from_secs_f32).collect()
    })
}

/// Any `f32`, malformed values included, for the panic-freedom sweep.
fn arb_wild_f32() -> impl Strategy<Value = f32> {
    prop_oneof![
        8 => -1.0e7f32..1.0e7f32,
        1 => Just(0.0f32),
        1 => prop_oneof![
            Just(f32::NAN),
            Just(f32::INFINITY),
            Just(f32::NEG_INFINITY),
            Just(f32::MIN),
            Just(f32::MAX),
            Just(f32::MIN_POSITIVE),
            Just(-0.0f32),
        ],
    ]
}

/// How far outside `[min, max]` a position sits; zero when it is inside.
fn excursion(p: f32, min: f32, max: f32) -> f32 {
    if p < min {
        min - p
    } else if p > max {
        p - max
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// 1. rubber_band is the identity at zero, for every extent
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn the_rubber_band_is_the_identity_at_a_zero_offset(extent in arb_extent()) {
        let y = rubber_band(0.0, extent);
        prop_assert_eq!(
            y, 0.0,
            "extent={}: a drag that has not left the range must not be damped, got {}",
            extent, y
        );
    }
}

// ---------------------------------------------------------------------------
// 2. rubber_band is monotone non-decreasing in the offset — dragging further
//    can only ever move the content further, never back
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn the_rubber_band_is_monotone_in_the_offset(
        (extent, a, b) in arb_extent_and_ordered_offsets()
    ) {
        let ya = rubber_band(a, extent);
        let yb = rubber_band(b, extent);
        prop_assert!(
            yb >= ya,
            "extent={}: dragging from {} to {} moved the content backwards, {} → {}",
            extent, a, b, ya, yb
        );
    }
}

// ---------------------------------------------------------------------------
// 3. rubber_band is odd and strictly bounded by the extent — the content can
//    never be dragged a full viewport past the edge, in either direction
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn the_rubber_band_is_odd_and_bounded_by_the_extent(
        (extent, offset) in arb_extent_and_offset()
    ) {
        let y = rubber_band(offset, extent);
        prop_assert!(
            y.abs() < extent,
            "extent={} offset={}: damped to {}, at or past the asymptote",
            extent, offset, y
        );
        prop_assert_eq!(
            rubber_band(-offset, extent), -y,
            "extent={} offset={}: the curve must be odd", extent, offset
        );
        prop_assert_eq!(
            y.signum(), offset.signum(),
            "extent={} offset={}: damping must not flip the direction", extent, offset
        );
    }
}

// ---------------------------------------------------------------------------
// 4. rubber_band_inverse undoes rubber_band — the round trip a drag resuming
//    from an overscrolled position depends on
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn the_rubber_band_inverse_recovers_the_finger_travel(
        (extent, offset) in arb_extent_and_offset()
    ) {
        let back = rubber_band_inverse(rubber_band(offset, extent), extent);
        // The forward curve saturates, so the inverse is only recoverable to
        // the precision the forward direction kept. A relative tolerance is
        // the honest bound: near the asymptote a single `f32` ulp of damped
        // offset stands for many pixels of finger travel.
        let tolerance = 0.02 * offset.abs().max(1.0);
        prop_assert!(
            (back - offset).abs() <= tolerance,
            "extent={} offset={}: round-tripped to {} (tolerance {})",
            extent, offset, back, tolerance
        );
    }
}

// ---------------------------------------------------------------------------
// 5. rubber_band never panics and never produces a NaN, on arbitrary input
//    — the workspace has two recorded bugs where a hand-written `>` guard let
//    a NaN through because every NaN comparison is false
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 1024, ..ProptestConfig::default() })]
    #[test]
    fn the_rubber_band_never_panics_or_returns_a_nan(
        offset in arb_wild_f32(),
        extent in arb_wild_f32(),
    ) {
        let y = rubber_band(offset, extent);
        prop_assert!(
            !y.is_nan(),
            "rubber_band({}, {}) = NaN — a NaN here lands straight in a layout offset",
            offset, extent
        );
        let back = rubber_band_inverse(offset, extent);
        prop_assert!(
            !back.is_nan(),
            "rubber_band_inverse({}, {}) = NaN", offset, extent
        );
    }
}

// ---------------------------------------------------------------------------
// 6. A clamping simulation never leaves its range, at any instant
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn a_clamping_simulation_never_leaves_its_range(
        (min, max, position) in arb_range_and_inside_position(),
        velocity in arb_velocity(),
        instants in arb_instants(4.0),
    ) {
        let sim = ClampingSimulation::new(
            position, velocity, min, max, &ScrollPhysicsTokens::DEFAULT,
        );
        for t in instants {
            let p = sim.position(t);
            prop_assert!(
                p.is_finite(),
                "range=[{}, {}] v={} t={:?}: non-finite position {}",
                min, max, velocity, t, p
            );
            prop_assert!(
                p >= min && p <= max,
                "range=[{}, {}] v={} t={:?}: escaped to {} — clamping physics has no overshoot",
                min, max, velocity, t, p
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 7. A bouncing simulation stays inside its range plus the overshoot the
//    spring can produce, and comes back
// ---------------------------------------------------------------------------

/// The largest excursion Teksilo's shipped settle spring can produce, per unit
/// of hand-off velocity.
///
/// Analytic, for the overdamped solution the shipped tokens select (mass 0.5,
/// stiffness 100, damping ratio 1.1 ⇒ `r₁ = −22.0371`, `r₂ = −9.07561`): a
/// spring released from zero displacement at velocity `v` peaks at
/// `v·(e^{r₂t*} − e^{r₁t*})/(r₂ − r₁)` where `t* = ln(r₂/r₁)/(r₁ − r₂)
/// = 0.068453 s`, which is `0.024382·v`. Rounded up, with the hand-off velocity
/// itself capped at Flutter's `maxSpringTransferVelocity` of 5000.
const OVERSHOOT_PER_VELOCITY: f32 = 0.03;

proptest! {
    #[test]
    fn a_bouncing_simulation_stays_within_its_range_plus_the_spring_allowance(
        (min, max, position) in arb_range_and_inside_position(),
        velocity in arb_velocity(),
        instants in arb_instants(4.0),
    ) {
        let sim = BouncingSimulation::within(position, velocity, min, max);
        let allowance = OVERSHOOT_PER_VELOCITY * velocity.abs().min(5000.0) + 1.0;
        for t in instants {
            let p = sim.position(t);
            prop_assert!(
                p.is_finite(),
                "range=[{}, {}] v={} t={:?}: non-finite position {}",
                min, max, velocity, t, p
            );
            let out = excursion(p, min, max);
            prop_assert!(
                out <= allowance,
                "range=[{}, {}] v={} t={:?}: overshot by {}, allowance {}",
                min, max, velocity, t, out, allowance
            );
        }

        // And the spring must actually return it: five seconds is twenty times
        // the settle time of a spring whose slowest root is −9.08 per second.
        let resting = sim.position(Duration::from_secs(5));
        prop_assert!(
            excursion(resting, min, max) < 1.0,
            "range=[{}, {}] v={}: still {} outside after 5 s",
            min, max, velocity, excursion(resting, min, max)
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Neither simulation ever speeds up past its release velocity
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn a_simulation_released_in_range_never_exceeds_its_release_speed(
        (min, max, position) in arb_range_and_inside_position(),
        velocity in arb_velocity(),
        instants in arb_instants(4.0),
    ) {
        // Scoped to a release *inside* the range on purpose. A simulation
        // released already overscrolled is a spring under load, and a spring
        // under load accelerates — that is what a spring is. The claim being
        // tested is about a fling: energy is only ever removed.
        let speed = velocity.abs();
        let clamping = ClampingSimulation::new(
            position, velocity, min, max, &ScrollPhysicsTokens::DEFAULT,
        );
        let bouncing = BouncingSimulation::within(position, velocity, min, max);

        for t in instants {
            for (name, v) in [
                ("clamping", clamping.velocity(t)),
                ("bouncing", bouncing.velocity(t)),
            ] {
                prop_assert!(
                    v.is_finite(),
                    "{} range=[{}, {}] v={} t={:?}: non-finite velocity {}",
                    name, min, max, velocity, t, v
                );
                prop_assert!(
                    v.abs() <= speed + 1e-2,
                    "{} range=[{}, {}] released at {} t={:?}: sped up to {}",
                    name, min, max, velocity, t, v
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 9. A clamping fling's travel is monotone in its release speed — a harder
//    flick can never go less far
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn a_harder_clamping_flick_never_travels_less_far(
        slow in 60.0f32..4000.0f32,
        extra in 0.0f32..4000.0f32,
    ) {
        let tokens = ScrollPhysicsTokens::DEFAULT;
        let fast = slow + extra;
        let a = ClampingSimulation::new(0.0, slow, -1.0e6, 1.0e6, &tokens);
        let b = ClampingSimulation::new(0.0, fast, -1.0e6, 1.0e6, &tokens);
        let da = a.position(a.duration());
        let db = b.position(b.duration());
        prop_assert!(
            db >= da - 1e-2,
            "{} dp/s travelled {} but {} dp/s only travelled {}",
            slow, da, fast, db
        );
    }
}

// ---------------------------------------------------------------------------
// 10. A KineticScroller's reported offset is always inside its range, whatever
//     it is dragged through — the invariant every consumer indexes on
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn a_scroller_offset_stays_inside_its_range_through_any_drag(
        (min, max) in arb_range(),
        deltas in prop::collection::vec(-3000.0f32..3000.0f32, 1..24),
        rubber in any::<bool>(),
    ) {
        let style = if rubber {
            OverscrollStyle::RubberBand
        } else {
            OverscrollStyle::Clamp
        };
        let mut s = KineticScroller::new(style);
        s.set_range_y(min, max);
        s.set_viewport(Vec2::new(400.0, 600.0));

        for (i, d) in deltas.iter().enumerate() {
            let step = s.pan(
                EventTime::from_millis(i as u64 * 16),
                Point::new(0.0, *d),
                Vec2::new(0.0, *d),
            );
            prop_assert!(
                step.offset.y >= min && step.offset.y <= max,
                "range=[{}, {}] after {} deltas: offset {} is outside",
                min, max, i + 1, step.offset.y
            );
            prop_assert!(
                step.offset.y.is_finite() && step.overscroll.y.is_finite(),
                "range=[{}, {}] after {} deltas: non-finite offset {} / overscroll {}",
                min, max, i + 1, step.offset.y, step.overscroll.y
            );
            if !rubber {
                prop_assert_eq!(
                    step.overscroll.y, 0.0,
                    "range=[{}, {}]: a clamping scroller must never report overscroll",
                    min, max
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 11. Under reduced motion a fling always settles on the very first tick, for
//     any release and any range
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn reduced_motion_always_settles_in_a_single_tick(
        (min, max, position) in arb_range_and_inside_position(),
        vx in arb_velocity(),
        vy in arb_velocity(),
    ) {
        let mut s = KineticScroller::new(OverscrollStyle::RubberBand);
        s.set_range_x(min, max);
        s.set_range_y(min, max);
        s.set_reduced_motion(true);
        s.set_offset(Point::new(position, position));

        let started = s.fling(Vec2::new(vx, vy), &GestureProfile::TOUCH);
        if started {
            prop_assert!(s.is_animating(), "an accepted release leaves one step pending");
            let step = s.tick(EventTime::from_millis(16));
            prop_assert!(step.is_some(), "the pending step must be delivered");
            prop_assert!(
                !s.is_animating(),
                "range=[{}, {}] v=({}, {}): reduced motion must settle in one tick",
                min, max, vx, vy
            );
            prop_assert_eq!(
                s.tick(EventTime::from_millis(32)), None,
                "and there must be nothing after it"
            );
        } else {
            prop_assert!(
                !s.is_animating(),
                "a refused release must start nothing at all"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 12. A release below the profile's floor never starts anything, whatever the
//     scroller's state
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn a_release_below_the_floor_starts_nothing(
        (min, max, position) in arb_range_and_inside_position(),
        // A vector whose magnitude is under the floor by construction: each
        // component is under half of it, so the hypotenuse cannot reach it.
        vx in -24.0f32..24.0f32,
        vy in -24.0f32..24.0f32,
        rubber in any::<bool>(),
    ) {
        let profile = GestureProfile::TOUCH;
        prop_assert!(
            (vx * vx + vy * vy).sqrt() < profile.min_fling_velocity,
            "the generator must stay under the {} dp/s floor",
            profile.min_fling_velocity
        );

        let style = if rubber {
            OverscrollStyle::RubberBand
        } else {
            OverscrollStyle::Clamp
        };
        let mut s = KineticScroller::new(style);
        s.set_range_y(min, max);
        s.set_offset(Point::new(0.0, position));

        prop_assert!(
            !s.fling(Vec2::new(vx, vy), &profile),
            "range=[{}, {}] v=({}, {}): a sub-floor release must be refused",
            min, max, vx, vy
        );
        prop_assert!(!s.is_animating(), "and must start no animation");
        prop_assert_eq!(s.next_deadline(), None, "and must ask for no wake-up");
        prop_assert_eq!(
            s.offset().y, position,
            "and must not move the content"
        );
    }
}
