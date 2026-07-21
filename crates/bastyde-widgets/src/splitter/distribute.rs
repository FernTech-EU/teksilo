// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pure N-pane sizing engine.
//!
//! [`distribute`] projects the model's stored pixel sizes onto the
//! current available main-axis extent, honoring per-pane min/max, the
//! collapse tween (`progress`), and container-resize slack (stretch
//! factors). It is a **pure function** — no `Signal`/model mutation — so
//! the widget can call it from `place_children` every layout pass and the
//! result is idempotent for a given `(available, panes, progress)`.
//!
//! Distribution mirrors the framework's own grow/shrink model
//! (`primitives/linear_layout.rs`): positive slack grows `stretch > 0`
//! panes proportional to weight (Qt `setStretchFactor`); a deficit
//! shrinks panes proportional to their room-above-min. Both are iterative
//! clamp-and-redistribute loops that respect max/min floors.

use super::model::PaneSnapshot;

/// Sub-pixel tolerance for "absorbed everything" / "no room left" checks.
const EPS: f32 = 0.01;

/// Compute the effective main-axis size of every pane.
///
/// - `available` = `bounds.main − (N−1) × gutter_thickness`.
/// - `progress[i]` ∈ `[0,1]` is pane `i`'s collapse tween (1 = expanded,
///   0 = fully collapsed); missing entries default to `1.0`.
///
/// The returned sizes sum to `≤ available` (they fall short only when the
/// container is larger than `Σ max`, i.e. nothing left to grow).
pub fn distribute(available: f32, panes: &[PaneSnapshot], progress: &[f32]) -> Vec<f32> {
    let n = panes.len();
    if n == 0 {
        return Vec::new();
    }
    let available = available.max(0.0);

    // Phase 0+1: effective request, clamped to its effective [min, max].
    let mut sizes = vec![0.0f32; n];
    let mut emin = vec![0.0f32; n];
    let mut emax = vec![f32::INFINITY; n];
    for i in 0..n {
        let p = panes[i];
        let prog = progress.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        let (req, lo, hi) = if p.collapsed {
            // The pane tweens between its `collapsed_size` (fully collapsed,
            // prog 0 — usually 0, but e.g. an accordion header height) and its
            // stored size (prog 1). `collapsed_size` is a *floor*: even if the
            // stored size is smaller (a stretch-grown pane whose stored size is
            // a tiny fallback until first dragged), a collapsed pane never folds
            // below its header. It may dip below its min while collapsing.
            let c = p.collapsed_size;
            let top = p.stored_size.max(c);
            (c + (top - c) * prog, 0.0, top)
        } else {
            (
                p.stored_size,
                p.min_size,
                p.max_size.unwrap_or(f32::INFINITY),
            )
        };
        emin[i] = lo;
        emax[i] = hi;
        // Defensive: an impossible [min,max] honors the min.
        sizes[i] = if lo > hi { lo } else { req.clamp(lo, hi) };
    }

    let total: f32 = sizes.iter().sum();
    let slack = available - total;
    if slack > EPS {
        grow(&mut sizes, &emax, panes, slack);
    } else if slack < -EPS {
        shrink(&mut sizes, &emin, panes, -slack);
    }
    sizes
}

/// Distribute positive `surplus` to non-collapsed `stretch > 0` panes,
/// proportional to weight, re-clamping to max and iterating. Any
/// remainder (no stretch pane, or all maxed) goes to the last
/// non-collapsed pane — clamped to its max, so nothing renders oversized.
fn grow(sizes: &mut [f32], emax: &[f32], panes: &[PaneSnapshot], mut surplus: f32) {
    let n = sizes.len();
    let mut frozen = vec![false; n];
    for (i, p) in panes.iter().enumerate() {
        if p.collapsed {
            frozen[i] = true;
        }
    }

    loop {
        let pool: Vec<usize> = (0..n)
            .filter(|&i| !frozen[i] && panes[i].stretch > 0.0 && sizes[i] < emax[i] - EPS)
            .collect();
        if pool.is_empty() {
            break;
        }
        let total_stretch: f32 = pool.iter().map(|&i| panes[i].stretch).sum();
        if total_stretch <= 0.0 {
            break;
        }
        let mut absorbed = 0.0;
        for &i in &pool {
            let give = surplus * (panes[i].stretch / total_stretch);
            let room = emax[i] - sizes[i];
            let take = give.min(room);
            sizes[i] += take;
            absorbed += take;
            if sizes[i] >= emax[i] - EPS {
                frozen[i] = true;
            }
        }
        surplus -= absorbed;
        if absorbed < EPS || surplus < EPS {
            break;
        }
    }

    // No stretch panes (or all maxed): the last non-collapsed pane absorbs
    // the remainder, clamped to its max.
    if surplus > EPS
        && let Some(i) = (0..n).rev().find(|&i| !panes[i].collapsed)
    {
        let room = (emax[i] - sizes[i]).max(0.0);
        sizes[i] += surplus.min(room);
    }
}

/// Shrink non-collapsed panes to absorb `deficit`, proportional to each
/// pane's room above its min, freezing at min and iterating. A leftover
/// deficit is unavoidable overflow (container smaller than `Σ min`) and
/// is accepted — the container clips it.
fn shrink(sizes: &mut [f32], emin: &[f32], panes: &[PaneSnapshot], mut deficit: f32) {
    let n = sizes.len();
    let mut frozen = vec![false; n];
    for (i, p) in panes.iter().enumerate() {
        if p.collapsed {
            frozen[i] = true; // collapsed panes are already minimal
        }
    }

    loop {
        let pool: Vec<usize> = (0..n)
            .filter(|&i| !frozen[i] && sizes[i] - emin[i] > EPS)
            .collect();
        if pool.is_empty() {
            break;
        }
        let total_room: f32 = pool.iter().map(|&i| sizes[i] - emin[i]).sum();
        if total_room <= 0.0 {
            break;
        }
        let mut absorbed = 0.0;
        for &i in &pool {
            let room = sizes[i] - emin[i];
            let take = (deficit * (room / total_room)).min(room);
            sizes[i] -= take;
            absorbed += take;
            if sizes[i] - emin[i] <= EPS {
                frozen[i] = true;
            }
        }
        deficit -= absorbed;
        if absorbed < EPS || deficit < EPS {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(
        stored: f32,
        min: f32,
        max: Option<f32>,
        stretch: f32,
        collapsed: bool,
    ) -> PaneSnapshot {
        PaneSnapshot {
            stored_size: stored,
            min_size: min,
            max_size: max,
            stretch,
            collapsed,
            collapsed_size: 0.0,
            visible: true,
        }
    }

    fn ones(n: usize) -> Vec<f32> {
        vec![1.0; n]
    }

    #[test]
    fn collapsed_pane_folds_to_collapsed_size_not_zero() {
        // A collapsed pane with `collapsed_size = 30` folds to 30 (e.g. an
        // accordion header), not 0; the freed space goes to its sibling. On
        // expand (progress 1) it restores to its stored size.
        let mut p0 = pane(200.0, 50.0, None, 1.0, true);
        p0.collapsed_size = 30.0;
        let p1 = pane(200.0, 50.0, None, 1.0, false);

        // Fully collapsed (progress 0).
        let collapsed = distribute(400.0, &[p0, p1], &[0.0, 1.0]);
        assert!(
            approx(collapsed[0], 30.0),
            "collapsed pane folds to 30, got {}",
            collapsed[0]
        );
        assert!(
            approx(collapsed[1], 370.0),
            "sibling absorbs the freed space, got {}",
            collapsed[1]
        );

        // Expanded (progress 1) → restores stored size.
        let expanded = distribute(400.0, &[p0, p1], &[1.0, 1.0]);
        assert!(
            approx(expanded[0], 200.0),
            "expands back to stored size, got {}",
            expanded[0]
        );
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.5
    }

    #[test]
    fn equal_share_from_zero_stored() {
        // new(3): stored=0, min=96, stretch=1 → equal shares filling 600.
        let panes = vec![pane(0.0, 96.0, None, 1.0, false); 3];
        let sizes = distribute(600.0, &panes, &ones(3));
        for s in &sizes {
            assert!(approx(*s, 200.0), "got {sizes:?}");
        }
    }

    #[test]
    fn stretch_absorbs_surplus_by_weight() {
        // Two panes, stored 100 each, stretch 1 and 3. Surplus 400 over a
        // 600 container splits 1:3 → +100 / +300.
        let panes = vec![
            pane(100.0, 0.0, None, 1.0, false),
            pane(100.0, 0.0, None, 3.0, false),
        ];
        let sizes = distribute(600.0, &panes, &ones(2));
        assert!(approx(sizes[0], 200.0), "{sizes:?}");
        assert!(approx(sizes[1], 400.0), "{sizes:?}");
    }

    #[test]
    fn zero_stretch_surplus_goes_to_last_pane() {
        let panes = vec![
            pane(100.0, 0.0, None, 0.0, false),
            pane(100.0, 0.0, None, 0.0, false),
        ];
        let sizes = distribute(500.0, &panes, &ones(2));
        assert!(approx(sizes[0], 100.0), "{sizes:?}");
        assert!(approx(sizes[1], 400.0), "{sizes:?}");
    }

    #[test]
    fn shrink_proportional_to_room_and_clamps_min() {
        // Stored 300 + 300 in a 400 container, mins 100 each. Deficit 200
        // splits evenly → 200 / 200.
        let panes = vec![
            pane(300.0, 100.0, None, 1.0, false),
            pane(300.0, 100.0, None, 1.0, false),
        ];
        let sizes = distribute(400.0, &panes, &ones(2));
        assert!(approx(sizes[0], 200.0), "{sizes:?}");
        assert!(approx(sizes[1], 200.0), "{sizes:?}");
        // Below Σmin → both pinned at min, overflow accepted.
        let tiny = distribute(150.0, &panes, &ones(2));
        assert!(approx(tiny[0], 100.0) && approx(tiny[1], 100.0), "{tiny:?}");
    }

    #[test]
    fn max_clamps_and_reroutes_surplus() {
        // Pane 0 capped at 150; the rest of the surplus goes to pane 1.
        let panes = vec![
            pane(100.0, 0.0, Some(150.0), 1.0, false),
            pane(100.0, 0.0, None, 1.0, false),
        ];
        let sizes = distribute(600.0, &panes, &ones(2));
        assert!(approx(sizes[0], 150.0), "{sizes:?}");
        assert!(approx(sizes[1], 450.0), "{sizes:?}");
    }

    #[test]
    fn collapse_progress_scales_effective_size() {
        // Pane 0 collapsing at progress 0.5: effective ≈ stored*0.5; the
        // freed space flows to pane 1 (stretch 1).
        let panes = vec![
            pane(200.0, 96.0, None, 1.0, true),
            pane(200.0, 96.0, None, 1.0, false),
        ];
        let half = distribute(600.0, &panes, &[0.5, 1.0]);
        assert!(approx(half[0], 100.0), "{half:?}");
        assert!(approx(half[1], 500.0), "{half:?}");
        // Fully collapsed → 0, all space to pane 1.
        let full = distribute(600.0, &panes, &[0.0, 1.0]);
        assert!(approx(full[0], 0.0), "{full:?}");
        assert!(approx(full[1], 600.0), "{full:?}");
    }

    #[test]
    fn all_collapsed_yields_zero() {
        let panes = vec![pane(200.0, 96.0, None, 1.0, true); 2];
        let sizes = distribute(600.0, &panes, &[0.0, 0.0]);
        assert!(approx(sizes[0], 0.0) && approx(sizes[1], 0.0), "{sizes:?}");
    }

    #[test]
    fn min_greater_than_max_honors_min() {
        let panes = vec![pane(50.0, 200.0, Some(100.0), 1.0, false)];
        let sizes = distribute(80.0, &panes, &ones(1));
        // emax < emin defensive path → honor min (200), even past available.
        assert!(approx(sizes[0], 200.0), "{sizes:?}");
    }

    #[test]
    fn idempotent_for_same_input() {
        let panes = vec![
            pane(120.0, 50.0, None, 1.0, false),
            pane(300.0, 80.0, Some(500.0), 2.0, false),
            pane(0.0, 96.0, None, 0.0, true),
        ];
        let a = distribute(700.0, &panes, &[1.0, 1.0, 0.3]);
        let b = distribute(700.0, &panes, &[1.0, 1.0, 0.3]);
        assert_eq!(a, b);
    }
}

// ---------------------------------------------------------------------------
// Property-based tests for `distribute` (see module doc above for the
// documented contract this file is testing against).
// ---------------------------------------------------------------------------

/// Property tests for [`distribute`], the pure N-pane sizing engine.
///
/// **Why inline, not `crates/bastyde-widgets/tests/`**: `distribute` is a
/// `pub fn`, but the module that declares it (`mod distribute;` in
/// `splitter.rs`, line ~33) is private, and `splitter.rs`'s `pub use
/// self::model::{...}` re-export list does not include `distribute` itself
/// (only `PaneSnapshot` and friends from `model`). So `bastyde_widgets::
/// splitter::distribute::distribute` is unreachable from an external
/// integration-test crate under `tests/`, and the task rules forbid loosening
/// visibility just to make a test reachable. This lives as a sibling of the
/// existing `#[cfg(test)] mod tests` in the same file instead, reached via
/// `cargo test -p bastyde-widgets --lib splitter::distribute::proptests`.
///
/// **Why proptest for this function specifically**: `distribute` is a pure,
/// total function (`no `Signal`/model mutation, no `Result`, no
/// documented preconditions on its inputs) that the widget calls from
/// `place_children` on *every* layout pass, fed by whatever a `SplitterModel`
/// happens to hold at that moment — including states a hand-written unit
/// test wouldn't think to construct (a pane whose `min_size` exceeds its
/// `max_size`, a fully collapsed set of panes, an `available` that can't
/// possibly satisfy every `min`). The existing `#[cfg(test)] mod tests`
/// above covers exact worked examples; this file generalizes those into
/// properties that must hold for the whole input space the type system
/// allows, and lets proptest's shrinker find the minimal reproducer when one
/// doesn't.
///
/// **Contracts asserted** (see each banner for the full reasoning):
/// 1. Growth fully absorbs positive slack when at least the growable panes'
///    capacity is unbounded and stretch-weighted.
/// 2. Shrink fully closes a deficit down to `available` when every pane's
///    floor is zero.
/// 3. An unsatisfiable shrink (`Σ min > available`) floors every pane at its
///    `min` and accepts the resulting overflow, rather than clamping to
///    `available` or panicking.
/// 4. Every pane stays within its effective `[min, max]` — or, for a
///    deliberately contradictory pane (`min > max`), is pinned exactly at
///    `min` (the documented "defensive" path).
/// 5. Growth splits proportionally to `stretch` weight.
/// 6. With `stretch == 0` everywhere, all surplus routes to the last
///    non-collapsed pane.
/// 7. A collapsed pane at `progress == 0` folds to exactly its
///    `collapsed_size`.
/// 8. `distribute` is deterministic for identical inputs.
/// 9. Increasing `available` never shrinks any individual pane.
/// 10. A negative `available` behaves identically to `available == 0.0`.
/// 11. Given only non-negative pane fields (but still zero/negative
///     `available`, zero/negative `stretch`, and `min > max`
///     contradictions), every returned size stays finite and non-negative.
/// 12. A pane whose `min_size`/`max_size` is itself non-finite (NaN or
///     ±Infinity) is a **known risk** for a non-finite result or an outright
///     panic in the current implementation — see that property's comment.
///
/// **Generator cost**: every pane-list generator is bounded to at most 8
/// panes (`prop::collection::vec(..., 1..=8)`), matching the ≤ 12-pane cap
/// requested for this suite — real `Splitter`s rarely exceed a handful of
/// panes, and `distribute` is `O(panes)` per call with no nested
/// multiplication of independent unbounded ranges (the failure mode that
/// previously exhausted memory elsewhere in this crate came from multiplying
/// two *independent* large quantities — e.g. a small `cell_size` against a
/// huge extent — into a huge collection; there is no such combination here,
/// since every generated quantity is a single scalar `f32` or a bounded
/// `Vec<f32>`/`Vec<PaneSnapshot>`, never a size *derived by multiplying* two
/// generated magnitudes). Where a property needs two related `f32` values
/// (e.g. "available" before/after growth, or a fraction of a computed total)
/// the second value is derived from the first via `.prop_map`/
/// `.prop_flat_map` rather than drawn independently, per the "couple
/// dependent inputs" rule.
///
/// Override the per-property case count with `PROPTEST_CASES=N cargo test
/// -p bastyde-widgets --lib splitter::distribute::proptests`.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // -----------------------------------------------------------------
    // Shared generators
    // -----------------------------------------------------------------

    /// `available` biased toward the documented edge cases: exactly zero,
    /// negative (must be clamped, never panic), and an ordinary positive
    /// range up to a few thousand logical pixels (a generous but plausible
    /// window/pane extent).
    fn arb_available() -> impl Strategy<Value = f32> {
        prop_oneof![Just(0.0_f32), Just(-1.0_f32), 0.5f32..4000.0_f32,]
    }

    /// A pane whose fields deliberately range over ordinary, boundary, and
    /// *contradictory* (`min_size > max_size`) configurations, while staying
    /// finite — the "adversarial but not NaN" domain shared by the
    /// determinism/monotonicity/bounds-compliance properties below. `stored`,
    /// `min`, and `max` all include negative values on purpose (nothing in
    /// `distribute` validates them; the model layer's `PaneEntry::
    /// from_descriptor` clamps these to sane ranges before they ever reach
    /// `distribute` in production, so this generator explores the wider
    /// space `distribute` itself never rejects). `visible` is always `true`
    /// since `distribute` never reads that field (confirmed by inspection —
    /// varying it would only add noise).
    fn arb_pane() -> impl Strategy<Value = PaneSnapshot> {
        (
            prop_oneof![Just(0.0f32), -500.0f32..=3000.0f32],
            prop_oneof![Just(0.0f32), -500.0f32..=1500.0f32],
            prop_oneof![Just(None), (-500.0f32..=1500.0f32).prop_map(Some)],
            prop_oneof![Just(0.0f32), -5.0f32..=10.0f32],
            any::<bool>(),
            prop_oneof![Just(0.0f32), -100.0f32..=500.0f32],
        )
            .prop_map(
                |(stored_size, min_size, max_size, stretch, collapsed, collapsed_size)| {
                    PaneSnapshot {
                        stored_size,
                        min_size,
                        max_size,
                        stretch,
                        collapsed,
                        collapsed_size,
                        visible: true,
                    }
                },
            )
    }

    /// Up to 8 panes drawn from [`arb_pane`] plus a matching-length
    /// `progress` vector (each `[0,1]` or slightly astray of it before the
    /// function's own `.clamp(0.0, 1.0)` normalizes it) — coupled via
    /// `prop_flat_map` so the two vectors always agree in length, exactly
    /// what `distribute` expects for a well-formed call.
    fn arb_panes_with_progress() -> impl Strategy<Value = (Vec<PaneSnapshot>, Vec<f32>)> {
        prop::collection::vec(arb_pane(), 1..=8).prop_flat_map(|panes| {
            let n = panes.len();
            let progress = prop::collection::vec(
                prop_oneof![Just(0.0f32), Just(1.0f32), 0.0f32..=1.0f32],
                n..=n,
            );
            (Just(panes), progress)
        })
    }

    /// A single non-collapsed pane tuned so growth can *always* fully
    /// absorb positive slack: `min = 0` (the phase-0 clamp never pins it
    /// above a ceiling) and `max = None` (unbounded headroom, so the
    /// stretch pool never saturates and freezes early).
    fn arb_growable_pane() -> impl Strategy<Value = PaneSnapshot> {
        (0.0f32..=2000.0f32, 0.1f32..=20.0f32).prop_map(|(stored_size, stretch)| PaneSnapshot {
            stored_size,
            min_size: 0.0,
            max_size: None,
            stretch,
            collapsed: false,
            collapsed_size: 0.0,
            visible: true,
        })
    }

    fn arb_growable_panes() -> impl Strategy<Value = Vec<PaneSnapshot>> {
        prop::collection::vec(arb_growable_pane(), 1..=8)
    }

    /// Like [`arb_growable_pane`] but with `stored_size` pinned to zero, so
    /// a pane's final size is *purely* its stretch-weighted share of
    /// `available` — no leftover initial size to fold into the comparison.
    fn arb_zero_stored_growable_pane() -> impl Strategy<Value = PaneSnapshot> {
        (0.1f32..=20.0f32).prop_map(|stretch| PaneSnapshot {
            stored_size: 0.0,
            min_size: 0.0,
            max_size: None,
            stretch,
            collapsed: false,
            collapsed_size: 0.0,
            visible: true,
        })
    }

    fn arb_zero_stored_growable_panes() -> impl Strategy<Value = Vec<PaneSnapshot>> {
        prop::collection::vec(arb_zero_stored_growable_pane(), 2..=6)
    }

    /// A non-collapsed pane with `stretch == 0` and unbounded max — the
    /// "everybody keeps their pixel size, nobody grows" fallback scenario.
    fn arb_zero_stretch_pane() -> impl Strategy<Value = PaneSnapshot> {
        (0.0f32..=500.0f32).prop_map(|stored_size| PaneSnapshot {
            stored_size,
            min_size: 0.0,
            max_size: None,
            stretch: 0.0,
            collapsed: false,
            collapsed_size: 0.0,
            visible: true,
        })
    }

    fn arb_zero_stretch_panes() -> impl Strategy<Value = Vec<PaneSnapshot>> {
        prop::collection::vec(arb_zero_stretch_pane(), 1..=8)
    }

    /// A pane pinned strictly above zero on `min_size`, with `stored_size`
    /// guaranteed `>= min_size` (`min + extra`) and an unbounded max, used to
    /// construct a deliberately unsatisfiable shrink (`Σ min > available`).
    fn arb_min_bound_pane() -> impl Strategy<Value = PaneSnapshot> {
        (1.0f32..=500.0f32, 0.0f32..=2000.0f32, 0.0f32..=5.0f32).prop_map(
            |(min_size, extra, stretch)| PaneSnapshot {
                stored_size: min_size + extra,
                min_size,
                max_size: None,
                stretch,
                collapsed: false,
                collapsed_size: 0.0,
                visible: true,
            },
        )
    }

    fn arb_min_bound_panes() -> impl Strategy<Value = Vec<PaneSnapshot>> {
        prop::collection::vec(arb_min_bound_pane(), 1..=8)
    }

    /// Every size-like field (`stored_size`, `min_size`, `max_size` when
    /// `Some`, `collapsed_size`) restricted to `>= 0` — the "sane input"
    /// domain real callers (`PaneEntry::from_descriptor`, which clamps all
    /// four of these to non-negative before ever building a `PaneSnapshot`)
    /// always produce — while still allowing `min_size > max_size`
    /// contradictions and zero/negative `stretch`, since neither is
    /// prevented by construction from a raw `PaneSnapshot` literal.
    fn arb_pane_nonneg() -> impl Strategy<Value = PaneSnapshot> {
        (
            0.0f32..=3000.0f32,
            0.0f32..=1500.0f32,
            prop_oneof![Just(None), (0.0f32..=1500.0f32).prop_map(Some)],
            prop_oneof![Just(0.0f32), -5.0f32..=10.0f32],
            any::<bool>(),
            0.0f32..=500.0f32,
        )
            .prop_map(
                |(stored_size, min_size, max_size, stretch, collapsed, collapsed_size)| {
                    PaneSnapshot {
                        stored_size,
                        min_size,
                        max_size,
                        stretch,
                        collapsed,
                        collapsed_size,
                        visible: true,
                    }
                },
            )
    }

    fn arb_nonneg_panes_with_progress() -> impl Strategy<Value = (Vec<PaneSnapshot>, Vec<f32>)> {
        prop::collection::vec(arb_pane_nonneg(), 1..=8).prop_flat_map(|panes| {
            let n = panes.len();
            let progress = prop::collection::vec(
                prop_oneof![Just(0.0f32), Just(1.0f32), 0.0f32..=1.0f32],
                n..=n,
            );
            (Just(panes), progress)
        })
    }

    /// A finite-but-wild value, or a genuinely non-finite one (NaN or
    /// ±Infinity) — the domain used only by the dedicated non-finite-bounds
    /// property (12), kept separate from [`arb_pane`] so the other eleven
    /// properties exercise a domain that is known not to trip the
    /// `f32::clamp` panic contract quoted in that property's comment.
    fn arb_non_finite_bound() -> impl Strategy<Value = f32> {
        prop_oneof![
            Just(f32::NAN),
            Just(f32::INFINITY),
            Just(f32::NEG_INFINITY),
            0.0f32..=1000.0f32,
        ]
    }

    fn arb_pane_with_wild_bounds() -> impl Strategy<Value = PaneSnapshot> {
        (
            0.0f32..=1000.0f32,
            arb_non_finite_bound(),
            prop_oneof![Just(None), arb_non_finite_bound().prop_map(Some)],
            0.1f32..=5.0f32,
        )
            .prop_map(|(stored_size, min_size, max_size, stretch)| PaneSnapshot {
                stored_size,
                min_size,
                max_size,
                stretch,
                collapsed: false,
                collapsed_size: 0.0,
                visible: true,
            })
    }

    fn arb_panes_with_wild_bounds() -> impl Strategy<Value = Vec<PaneSnapshot>> {
        prop::collection::vec(arb_pane_with_wild_bounds(), 1..=4)
    }

    // ── 1. growth fully absorbs positive slack when capacity is unbounded ──
    // With `min = 0` and `max = None` on every pane and at least one positive
    // `stretch`, the growth pool never saturates (its room is always
    // infinite), so the single-round proportional fill converges exactly:
    // `Σ sizes == available`. `available` is derived from the panes'
    // `stored_size` total plus a non-negative `extra_slack`, guaranteeing
    // growth (not shrink) is the branch taken.

    proptest! {
        #[test]
        fn grow_conservation_with_unbounded_max_and_positive_stretch(
            panes in arb_growable_panes(),
            extra_slack in 0.0f32..=5000.0f32,
        ) {
            let total_initial: f32 = panes.iter().map(|p| p.stored_size).sum();
            let available = total_initial + extra_slack;
            let progress = vec![1.0f32; panes.len()];
            let sizes = distribute(available, &panes, &progress);
            let sum: f32 = sizes.iter().sum();
            prop_assert!(
                (sum - available).abs() < 1.0,
                "growth with min=0/max=None/stretch>0 should fully absorb slack: sum={} available={} sizes={:?}",
                sum, available, sizes
            );
        }
    }

    // ── 2. shrink fully closes a deficit down to available when every min
    //      is zero ──
    // With `min = 0` everywhere, every pane's "room above min" equals its
    // whole initial size, so total room always covers any deficit up to the
    // total itself. `available` is drawn as a fraction of the (independently
    // recomputed) initial total, guaranteeing `available <= total_initial`
    // (shrink-or-equal) and that the deficit never exceeds total room.

    proptest! {
        #[test]
        fn shrink_conservation_when_every_min_is_zero(
            panes in arb_zero_stretch_panes(),
            available_fraction in 0.0f32..=1.0f32,
        ) {
            let initial: Vec<f32> = panes
                .iter()
                .map(|p| p.stored_size.min(p.max_size.unwrap_or(f32::INFINITY)))
                .collect();
            let total_initial: f32 = initial.iter().sum();
            let available = total_initial * available_fraction;
            let progress = vec![1.0f32; panes.len()];
            let sizes = distribute(available, &panes, &progress);
            let sum: f32 = sizes.iter().sum();
            prop_assert!(
                (sum - available).abs() < 1.0,
                "shrink with every min=0 should always fully reach available: sum={} available={} sizes={:?}",
                sum, available, sizes
            );
        }
    }

    // ── 3. an unsatisfiable shrink floors every pane at its min and accepts
    //      the resulting overflow ──
    // `shrink`'s own doc comment: "a leftover deficit is unavoidable overflow
    // ... and is accepted — the container clips it." With `available` set to
    // a fraction (<= 0.9, leaving comfortable margin past the algorithm's
    // internal EPS) of `Σ min`, the deficit exceeds total room above min in
    // a single round, so every non-collapsed pane converges to exactly its
    // own `min_size`, and the total sits at `Σ min` — strictly above
    // `available`, not clamped down to it.

    proptest! {
        #[test]
        fn shrink_floor_accepts_overflow_when_total_min_exceeds_available(
            panes in arb_min_bound_panes(),
            shortfall_fraction in 0.0f32..=0.9f32,
        ) {
            let total_min: f32 = panes.iter().map(|p| p.min_size).sum();
            let available = total_min * shortfall_fraction;
            let progress = vec![1.0f32; panes.len()];
            let sizes = distribute(available, &panes, &progress);
            let sum: f32 = sizes.iter().sum();
            prop_assert!(
                sum >= available - 0.5,
                "an unsatisfiable shrink must not undershoot available: sum={} available={}",
                sum, available
            );
            prop_assert!(
                (sum - total_min).abs() < 1.0,
                "an unsatisfiable shrink floors every pane at its min, so the total should equal Sum(min)={}, got {} (sizes={:?})",
                total_min, sum, sizes
            );
            for (i, (size, p)) in sizes.iter().zip(panes.iter()).enumerate() {
                prop_assert!(
                    (size - p.min_size).abs() < 0.5,
                    "pane {} should be pinned at its min {} under an unsatisfiable shrink, got {}",
                    i, p.min_size, size
                );
            }
        }
    }

    // ── 4. every pane stays within its effective bounds, or honors min when
    //      those bounds are contradictory ──
    // Mirrors the "Defensive: an impossible [min,max] honors the min"
    // comment at the top of `distribute` (and the `min_greater_than_max_
    // honors_min` unit test) generalized to arbitrary panes, including
    // collapsed ones (whose effective bounds are `[0, max(stored,
    // collapsed_size)]` per the documented phase-0 formula).

    proptest! {
        #[test]
        fn every_pane_respects_its_effective_bounds_or_honors_min_when_contradictory(
            panes_and_progress in arb_panes_with_progress(),
            available in arb_available(),
        ) {
            let (panes, progress) = panes_and_progress;
            let sizes = distribute(available, &panes, &progress);
            for (i, (size, p)) in sizes.iter().zip(panes.iter()).enumerate() {
                let (emin, emax) = if p.collapsed {
                    (0.0f32, p.stored_size.max(p.collapsed_size))
                } else {
                    (p.min_size, p.max_size.unwrap_or(f32::INFINITY))
                };
                if emin <= emax {
                    prop_assert!(
                        *size >= emin - 0.5 && *size <= emax + 0.5,
                        "pane {} size {} should stay within its effective bounds [{}, {}] (available={}, panes={:?})",
                        i, size, emin, emax, available, panes
                    );
                } else {
                    prop_assert!(
                        (*size - emin).abs() < 0.5,
                        "pane {} has contradictory bounds (min {} > max {}); distribute must honor min, got {}",
                        i, emin, emax, size
                    );
                }
            }
        }
    }

    // ── 5. growth splits proportionally to stretch weight ──
    // With every pane's `stored_size` pinned to zero and `max = None`, the
    // whole of `available` is fresh slack distributed in a single
    // (uncapped) round: `size[i] == available * stretch[i] / Σ stretch`.
    // Generalizes `stretch_absorbs_surplus_by_weight` from 2 hand-picked
    // weights to N arbitrary positive weights.

    proptest! {
        #[test]
        fn growth_splits_proportionally_to_stretch_weight(
            panes in arb_zero_stored_growable_panes(),
            available in 0.0f32..=5000.0f32,
        ) {
            let progress = vec![1.0f32; panes.len()];
            let sizes = distribute(available, &panes, &progress);
            let total_stretch: f32 = panes.iter().map(|p| p.stretch).sum();
            for (i, (size, p)) in sizes.iter().zip(panes.iter()).enumerate() {
                let expected = available * (p.stretch / total_stretch);
                prop_assert!(
                    (size - expected).abs() < 1.0,
                    "pane {} (stretch {}, total_stretch {}) should get {} of {}: expected {}, got {}",
                    i, p.stretch, total_stretch, p.stretch / total_stretch, available, expected, size
                );
            }
        }
    }

    // ── 6. zero stretch everywhere routes all surplus to the last pane ──
    // Generalizes `zero_stretch_surplus_goes_to_last_pane`: with every
    // pane's `stretch == 0`, the growth pool is empty from round one, so
    // every pane but the last keeps its stored size unchanged and the last
    // non-collapsed pane (here, simply the last pane — none are collapsed)
    // absorbs the entire `extra_slack`.

    proptest! {
        #[test]
        fn zero_stretch_everywhere_routes_all_surplus_to_the_last_pane(
            panes in arb_zero_stretch_panes(),
            extra_slack in 0.0f32..=5000.0f32,
        ) {
            let total_initial: f32 = panes.iter().map(|p| p.stored_size).sum();
            let available = total_initial + extra_slack;
            let progress = vec![1.0f32; panes.len()];
            let sizes = distribute(available, &panes, &progress);
            let n = panes.len();
            for i in 0..(n - 1) {
                prop_assert!(
                    (sizes[i] - panes[i].stored_size).abs() < 0.5,
                    "pane {} has stretch 0 and should keep its stored size {}, got {}",
                    i, panes[i].stored_size, sizes[i]
                );
            }
            let last = n - 1;
            let expected_last = panes[last].stored_size + extra_slack;
            prop_assert!(
                (sizes[last] - expected_last).abs() < 1.0,
                "the last pane should absorb the whole surplus {}: expected {}, got {}",
                extra_slack, expected_last, sizes[last]
            );
        }
    }

    // ── 7. a collapsed pane at progress 0 folds to exactly its
    //      collapsed_size ──
    // Generalizes `collapsed_pane_folds_to_collapsed_size_not_zero`'s
    // fully-collapsed half: `req = collapsed_size + (top - collapsed_size) *
    // 0 == collapsed_size` exactly, and since `collapsed_size >= 0` here,
    // it never gets clamped up by the `[0, top]` bounds. A single stretchy
    // sibling exists purely so a non-degenerate `available` has somewhere
    // to route the freed space (irrelevant to this pane's own assertion).

    proptest! {
        #[test]
        fn collapsed_pane_folds_to_exactly_its_collapsed_size_at_zero_progress(
            pane in (0.0f32..=500.0f32, 0.0f32..=2000.0f32, 0.0f32..=10.0f32).prop_map(
                |(collapsed_size, stored_size, stretch)| PaneSnapshot {
                    stored_size,
                    min_size: 0.0,
                    max_size: None,
                    stretch,
                    collapsed: true,
                    collapsed_size,
                    visible: true,
                },
            ),
            sibling_stretch in 0.1f32..=10.0f32,
            available in 0.5f32..=4000.0f32,
        ) {
            let sibling = PaneSnapshot {
                stored_size: 0.0,
                min_size: 0.0,
                max_size: None,
                stretch: sibling_stretch,
                collapsed: false,
                collapsed_size: 0.0,
                visible: true,
            };
            let sizes = distribute(available, &[pane, sibling], &[0.0, 1.0]);
            prop_assert!(
                (sizes[0] - pane.collapsed_size).abs() < 0.5,
                "a pane at progress 0 should fold to exactly its collapsed_size {}, got {}",
                pane.collapsed_size, sizes[0]
            );
        }
    }

    // ── 8. distribute is deterministic for identical inputs ──
    // `distribute` is documented as a pure function with no `Signal`/model
    // mutation, called fresh on every layout pass; calling it twice with
    // byte-identical arguments must produce byte-identical output (no
    // hidden iteration-order or timing dependency).

    proptest! {
        #[test]
        fn distribute_is_deterministic_for_identical_inputs(
            panes_and_progress in arb_panes_with_progress(),
            available in arb_available(),
        ) {
            let (panes, progress) = panes_and_progress;
            let a = distribute(available, &panes, &progress);
            let b = distribute(available, &panes, &progress);
            prop_assert_eq!(&a, &b, "distribute must be a pure function of its inputs: {:?} vs {:?}", a, b);
        }
    }

    // ── 9. increasing available never shrinks any individual pane ──
    // Phase-0/1 (the per-pane initial clamp) doesn't depend on `available`
    // at all; growth only ever adds, shrink only ever subtracts down to a
    // fixed floor, and collapsed panes are frozen out of both — so for a
    // fixed pane/progress configuration, every pane's size should be a
    // non-decreasing function of `available`. `available_hi` is derived
    // from `available_lo` (base + non-negative extra) rather than drawn
    // independently, per the "couple dependent inputs" rule.

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn increasing_available_never_shrinks_any_individual_pane(
            panes_and_progress in arb_panes_with_progress(),
            available_lo in arb_available(),
            extra in 0.0f32..=3000.0f32,
        ) {
            let (panes, progress) = panes_and_progress;
            let available_hi = available_lo.max(0.0) + extra;
            let sizes_lo = distribute(available_lo, &panes, &progress);
            let sizes_hi = distribute(available_hi, &panes, &progress);
            for (i, (lo, hi)) in sizes_lo.iter().zip(sizes_hi.iter()).enumerate() {
                prop_assert!(
                    *hi >= *lo - 0.5,
                    "pane {} shrank from {} to {} when available grew from {} to {}",
                    i, lo, hi, available_lo, available_hi
                );
            }
        }
    }

    // ── 10. a negative available behaves identically to available == 0.0 ──
    // Directly reflects the function's own first line: `let available =
    // available.max(0.0);`. Any negative `available` must therefore produce
    // exactly (not just approximately) the same result as `available =
    // 0.0`, since both take the identical code path afterward.

    proptest! {
        #[test]
        fn negative_available_is_treated_identically_to_zero(
            panes_and_progress in arb_panes_with_progress(),
            negative_available in -10000.0f32..0.0f32,
        ) {
            let (panes, progress) = panes_and_progress;
            let from_negative = distribute(negative_available, &panes, &progress);
            let from_zero = distribute(0.0, &panes, &progress);
            prop_assert_eq!(
                &from_negative, &from_zero,
                "distribute clamps available to 0 internally (`available.max(0.0)`), so negative available {} must match available=0: {:?} vs {:?}",
                negative_available, from_negative, from_zero
            );
        }
    }

    // ── 11. finite, non-negative pane fields always produce finite,
    //       non-negative sizes ──
    // Every arithmetic step in `distribute` either starts from a
    // non-negative `[emin, emax]` bound (both `0.0` for a collapsed pane,
    // both `>= 0` for a non-collapsed one when every input field is `>= 0`)
    // and only moves *within* it (grow adds, capped at `emax`; shrink
    // subtracts, floored at `emin`), or takes the contradictory-bounds
    // branch which pins the pane at `emin >= 0`. No division here is ever
    // by a total that can be zero without the surrounding `if total <= 0.0
    // { break }` guard firing first. So even with `available` zero/negative,
    // `stretch` zero/negative, and deliberate `min > max` contradictions,
    // sizes should stay finite and non-negative as long as the pane's own
    // *size-like* fields are. (This is a materially different, stronger
    // claim than "never negative for arbitrary input" — see property 12 and
    // the report for the scoped-out negative-min-size counterexample.)

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
        #[test]
        fn finite_nonnegative_inputs_never_produce_nan_or_negative_sizes(
            panes_and_progress in arb_nonneg_panes_with_progress(),
            available in arb_available(),
        ) {
            let (panes, progress) = panes_and_progress;
            let sizes = distribute(available, &panes, &progress);
            for (i, size) in sizes.iter().enumerate() {
                prop_assert!(
                    size.is_finite(),
                    "pane {} produced a non-finite size {} for available={}, panes={:?}",
                    i, size, available, panes
                );
                prop_assert!(
                    *size >= -0.05,
                    "pane {} produced a meaningfully negative size {} from all-non-negative pane fields (available={}, panes={:?})",
                    i, size, available, panes
                );
            }
        }
    }

    // ── 12. sizes stay finite even when a pane's min or max bound is
    //       itself non-finite ──
    // KNOWN RISK, not a softened assertion: `distribute`'s phase-0 clamp
    // does `if lo > hi { lo } else { req.clamp(lo, hi) }` to defend against
    // a contradictory-but-finite `[min, max]` (property 4). That guard
    // cannot catch NaN — `NaN > hi` and `lo > NaN` are both `false` — so a
    // NaN `min_size`/`max_size` falls through to `req.clamp(lo, hi)`, and
    // `f32::clamp`'s documented contract is "Panics if min > max, min is
    // NaN, or max is NaN" (core::num::f32, verified against the local
    // rustc 1.96 sysroot source). A `min_size`/`max_size` of `f32::INFINITY`
    // does *not* panic but is likely to produce a non-finite output size
    // via that same clamp (e.g. `min_size = f32::INFINITY` with a
    // contradictory guard hit sets the pane's size to `Infinity`). This
    // property states the desired contract (finite output, no panic); if
    // proptest reports a panic or a non-finite value here, that is real
    // information about the current implementation, not a flaw in the
    // property — see the report for why this one carries low confidence.

    proptest! {
        #[test]
        fn sizes_stay_finite_even_when_a_pane_min_or_max_is_non_finite(
            panes in arb_panes_with_wild_bounds(),
            available in arb_available(),
        ) {
            let progress = vec![1.0f32; panes.len()];
            let sizes = distribute(available, &panes, &progress);
            for (i, size) in sizes.iter().enumerate() {
                prop_assert!(
                    size.is_finite(),
                    "pane {} produced a non-finite size {} from a non-finite min/max bound (available={}, panes={:?})",
                    i, size, available, panes
                );
            }
        }
    }
}
