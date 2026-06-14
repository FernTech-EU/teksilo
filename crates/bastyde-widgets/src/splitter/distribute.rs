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

    fn pane(stored: f32, min: f32, max: Option<f32>, stretch: f32, collapsed: bool) -> PaneSnapshot {
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
