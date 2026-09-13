// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a drop lands inside one tree row: before it, into it, or after it.
//!
//! Both hierarchical views compute this twice each — once in `on_drag_hover`,
//! which decides the affordance the user sees, and once in `on_drop`, which
//! decides what actually happens. Four copies of one rule is exactly the drift
//! that makes a drag land somewhere other than where the insertion line
//! promised, so the rule lives here and all four call it.
//!
//! # Why a pointer kind, and why `Into` is the band that yields
//!
//! The bands were plain thirds. On the default 28 dp tree row that is 9.33 dp
//! each — fine for a cursor that lands where it is put, too fine for a
//! fingertip whose reported centre wanders. So a coarse pointer widens the two
//! **edge** bands toward [`COARSE_MIN_BAND`], and `Into` takes what is left.
//!
//! That direction is a ruling, not an accident. `Before` and `After` are the
//! universal operation — reordering, which every list supports — while `Into`
//! is the tree-only reparent, and `Into` has two other routes a finger can
//! take: the **spring-load** (hovering a branch expands it, after which the
//! child rows offer a `Before` at the position that was wanted) and the
//! keyboard. Widening `Before`/`After` costs the operation with alternatives;
//! widening `Into` would cost the one without.
//!
//! # The floor is not always reachable, and this says so
//!
//! Three bands of `COARSE_MIN_BAND` need `3 × COARSE_MIN_BAND` of row. Short
//! rows do not have it, so [`MAX_EDGE_FRACTION`] caps each edge band and keeps
//! `Into` alive rather than letting the floor eat it: below
//! `COARSE_MIN_BAND / MAX_EDGE_FRACTION` of row height the edge bands are the
//! widest they can be while leaving `Into` reachable at all, and that is
//! narrower than the floor. Every band still has positive extent at every
//! *positive* row height, which is the invariant that matters — a band of zero
//! would be a drop position no pointer could ever express. A row height that is
//! not positive yields three zero bands and no reachable position at all, which
//! is what `a_degenerate_row_yields_empty_bands` asserts.
//!
//! # Not a density question
//!
//! [`TargetDensity`] describes how big the UI's own targets are;
//! this describes how precisely a *device* can be parked inside a row whose
//! height the application chose. The same argument
//! `common::drag_autoscroll` makes for the auto-scroll edge band, and the
//! answer here does not move with density either.
//!
//! [`TargetDensity`]: teksilo_tokens::TargetDensity

use teksilo_data::DropPosition;
use teksilo_tokens::PointerKind;

/// Smallest edge band a coarse pointer should be asked to hit, in dp.
///
/// Deliberately well under `InputTokens::min_target_conformance`: that number
/// governs a target you must *acquire*, and this is a positional
/// discrimination made during a drag that is already in flight, with live
/// feedback showing which band the pointer is in.
pub(crate) const COARSE_MIN_BAND: f32 = 12.0;

/// The most of a row one edge band may take, as a fraction. Two edges at this
/// fraction leave `Into` a fifth of the row, so it is never squeezed out.
pub(crate) const MAX_EDGE_FRACTION: f32 = 0.4;

/// The share of the row each band gets when the row is tall enough that plain
/// thirds already clear the floor — the shape every tree row had before this
/// module existed, and the one a precise pointer still gets at every height.
const EDGE_FRACTION: f32 = 1.0 / 3.0;

/// The floor for a pointer kind. A precise pointer has none: a cursor lands
/// where it is put, so it keeps the plain thirds exactly.
pub(crate) fn min_band_for(kind: PointerKind) -> f32 {
    if kind.is_coarse() {
        COARSE_MIN_BAND
    } else {
        0.0
    }
}

/// The three bands of a row of `row_h`, in order, summing to `row_h`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RowDropBands {
    /// Height of the leading band, which drops **before** the row.
    pub(crate) before: f32,
    /// Height of the middle band, which drops **into** the row.
    pub(crate) into: f32,
    /// Height of the trailing band, which drops **after** the row.
    pub(crate) after: f32,
}

/// Split a row of `row_h` into its before / into / after bands for `kind`.
///
/// The two edge bands are equal; `Into` absorbs the residue, so the three
/// always sum to `row_h` exactly. A non-finite or non-positive `row_h` yields
/// three zero bands (the caller's `row_at` cannot have produced a real row).
pub(crate) fn row_drop_bands(row_h: f32, kind: PointerKind) -> RowDropBands {
    if !row_h.is_finite() || row_h <= 0.0 {
        return RowDropBands {
            before: 0.0,
            into: 0.0,
            after: 0.0,
        };
    }
    let edge = (row_h * EDGE_FRACTION)
        .max(min_band_for(kind))
        .min(row_h * MAX_EDGE_FRACTION);
    RowDropBands {
        before: edge,
        into: row_h - 2.0 * edge,
        after: edge,
    }
}

/// Which band `y_in_row` — the offset from the row's own top — falls in.
///
/// Boundaries belong to the band above them, so a `y` exactly on the first
/// boundary is `Into` and one exactly on the second is `After`; that is the
/// pre-existing `<` / `>` convention, kept.
pub(crate) fn drop_position_in_row(y_in_row: f32, row_h: f32, kind: PointerKind) -> DropPosition {
    let bands = row_drop_bands(row_h, kind);
    if y_in_row < bands.before {
        DropPosition::Before
    } else if y_in_row > bands.before + bands.into {
        DropPosition::After
    } else {
        DropPosition::Into
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use teksilo_tokens::{PenKind, TargetDensity};

    const HEIGHTS: [f32; 7] = [16.0, 20.0, 28.0, 32.0, 40.0, 44.0, 96.0];

    fn precise() -> [PointerKind; 3] {
        [
            PointerKind::Mouse,
            PointerKind::Pen(PenKind::Pen),
            PointerKind::Unknown,
        ]
    }

    /// The no-regression statement for the whole module: a precise pointer gets
    /// the plain thirds this rule always was, at every positive row height.
    #[test]
    fn a_precise_pointer_keeps_the_plain_thirds() {
        for kind in precise() {
            for h in HEIGHTS {
                let b = row_drop_bands(h, kind);
                assert!(
                    (b.before - h / 3.0).abs() < 1e-4
                        && (b.into - h / 3.0).abs() < 1e-4
                        && (b.after - h / 3.0).abs() < 1e-4,
                    "{kind:?} at row height {h} got {b:?}, not thirds",
                );
            }
        }
    }

    /// The bands tile the row exactly — no gap a pointer could fall into with
    /// no answer, no overlap where two answers disagree.
    #[test]
    fn the_bands_tile_the_row_exactly() {
        for kind in [PointerKind::Mouse, PointerKind::Touch] {
            for h in HEIGHTS {
                let b = row_drop_bands(h, kind);
                assert!(
                    (b.before + b.into + b.after - h).abs() < 1e-4,
                    "{kind:?} at {h}: {b:?} does not sum to the row",
                );
            }
        }
    }

    /// Every one of the three positions is reachable at every positive row
    /// height: each
    /// band has positive extent, and a `y` inside it resolves to it.
    #[test]
    fn all_three_positions_are_reachable_at_every_row_height() {
        for kind in [PointerKind::Mouse, PointerKind::Touch] {
            for h in HEIGHTS {
                let b = row_drop_bands(h, kind);
                assert!(
                    b.before > 0.0 && b.into > 0.0 && b.after > 0.0,
                    "{kind:?} at {h}: {b:?} has a band no pointer can express",
                );
                assert_eq!(
                    drop_position_in_row(b.before / 2.0, h, kind),
                    DropPosition::Before,
                );
                assert_eq!(
                    drop_position_in_row(b.before + b.into / 2.0, h, kind),
                    DropPosition::Into,
                );
                assert_eq!(
                    drop_position_in_row(h - b.after / 2.0, h, kind),
                    DropPosition::After,
                );
            }
        }
    }

    /// A finger's edge band never gets *narrower* than a mouse's, and clears
    /// the floor at every positive row height that can afford it.
    #[test]
    fn a_finger_gets_the_wider_edge_band_wherever_the_row_affords_it() {
        let affordable = COARSE_MIN_BAND / MAX_EDGE_FRACTION;
        for h in HEIGHTS {
            let mouse = row_drop_bands(h, PointerKind::Mouse);
            let touch = row_drop_bands(h, PointerKind::Touch);
            assert!(
                touch.before >= mouse.before - 1e-4,
                "a finger's band shrank at row height {h}: {touch:?} vs {mouse:?}",
            );
            if h >= affordable {
                assert!(
                    touch.before >= COARSE_MIN_BAND - 1e-4,
                    "row height {h} can afford the floor but got {touch:?}",
                );
            }
        }
    }

    /// The documented limit, stated as a test so it cannot rot into a false
    /// claim: below `COARSE_MIN_BAND / MAX_EDGE_FRACTION` the floor is NOT met,
    /// and `Into` is what the cap protects.
    #[test]
    fn a_short_row_cannot_give_all_three_bands_the_floor() {
        let short = 28.0_f32; // `TreeView::DEFAULT_ITEM_HEIGHT`
        assert!(short < COARSE_MIN_BAND / MAX_EDGE_FRACTION);
        let b = row_drop_bands(short, PointerKind::Touch);
        assert!(
            b.before < COARSE_MIN_BAND,
            "the floor cannot be met at {short} dp, and pretending it is met \
             would mean an `Into` band of {}",
            short - 2.0 * COARSE_MIN_BAND,
        );
        assert!(b.into > 0.0, "…but `Into` must survive: {b:?}");
    }

    /// The bands do not move with [`TargetDensity`] — see the module header.
    /// Asserted so that wiring density in later is a deliberate act with a
    /// failing test in front of it, not a silent drift.
    #[test]
    fn the_bands_do_not_move_with_density() {
        let baseline = row_drop_bands(32.0, PointerKind::Touch);
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            // Nothing here reads the density; naming it is the point.
            let _ = density;
            assert_eq!(row_drop_bands(32.0, PointerKind::Touch), baseline);
        }
    }

    /// A degenerate row answers rather than dividing by zero.
    #[test]
    fn a_degenerate_row_yields_empty_bands() {
        for h in [0.0, -4.0, f32::NAN, f32::INFINITY] {
            let b = row_drop_bands(h, PointerKind::Touch);
            assert_eq!(b.before, 0.0);
            assert_eq!(b.into, 0.0);
            assert_eq!(b.after, 0.0);
        }
    }
}
