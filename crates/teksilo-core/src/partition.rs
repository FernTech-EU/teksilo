// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! In-node target geometry: [`TargetRegion`], the shape a widget *reports*, and
//! [`partition_targets`], the engine that carves one node's rectangle into
//! several of them.
//!
//! # Why a widget reports regions it did not lay out
//!
//! Some controls are painted as several targets inside **one** leaf node: a
//! scroll bar draws its thumb on the same canvas as its track; a slider draws
//! track, fill and knob together; a table header column draws a label beside a
//! filter affordance. Nothing in the widget tree knows those sub-targets exist,
//! so nothing can route a coarse press to the nearest one, and no audit can
//! check that any of them clears the 24 dp conformance floor.
//!
//! [`Widget::target_regions`] is how such a widget says what it painted. It is
//! **reporting only** — implementing it changes no layout and no hit test on
//! its own. Three things read it: the router (routing a coarse press to the
//! nearest region within its role's floor), the target-conformance audit, and
//! the widget's own event handling, which can now ask one function where its
//! parts are instead of re-deriving the split at press time and drifting from
//! what it painted.
//!
//! [`partition_targets`] is that one function for the common case — a
//! horizontal split of a node into named zones with a minimum size each.
//!
//! ```
//! use teksilo_canvas::Rect;
//! use teksilo_core::environment::LayoutDirection;
//! use teksilo_core::partition::partition_targets;
//!
//! // A 200 dp header cell: label takes the room, a 24 dp filter button trails.
//! let zones = partition_targets(
//!     Rect::new(0.0, 0.0, 200.0, 28.0),
//!     &[0.8, 0.2],
//!     24.0,
//!     LayoutDirection::LeftToRight,
//! );
//! assert_eq!(zones[0], Rect::new(0.0, 0.0, 160.0, 28.0));
//! assert_eq!(zones[1], Rect::new(160.0, 0.0, 40.0, 28.0));
//! ```
//!
//! Reference: `docs/density-and-targets.md`.
//!
//! [`Widget::target_regions`]: crate::widget::Widget::target_regions

use teksilo_canvas::Rect;
use teksilo_tokens::TargetRole;

use crate::environment::LayoutDirection;

/// One interactive sub-region of a single widget node.
///
/// `part` is the widget's own discriminator — an index, or a `#[repr(u16)]`
/// enum cast — so a consumer that knows the widget can tell the thumb from the
/// track, and one that does not can still measure both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetRegion {
    /// The region's rectangle, in the same space as the `bounds` the widget was
    /// asked about (absolute arena coordinates for a node's own bounds).
    pub rect: Rect,
    /// What the region is for, which is what decides the floor it is audited
    /// against: `Target` → `target_size`, `Grab` → `grab_size`, `Decoration` →
    /// not audited.
    pub role: TargetRole,
    /// Which part of the widget this is. Meaningful only to the widget that
    /// reported it.
    pub part: u16,
}

impl TargetRegion {
    /// A tappable region.
    pub fn target(rect: Rect, part: u16) -> Self {
        Self {
            rect,
            role: TargetRole::Target,
            part,
        }
    }

    /// A draggable grip or divider region.
    pub fn grab(rect: Rect, part: u16) -> Self {
        Self {
            rect,
            role: TargetRole::Grab,
            part,
        }
    }

    /// A region that is painted but not interactive, reported so an audit can
    /// see the whole picture without flagging it.
    pub fn decoration(rect: Rect, part: u16) -> Self {
        Self {
            rect,
            role: TargetRole::Decoration,
            part,
        }
    }
}

/// Split `bounds` **horizontally** into one rectangle per entry of `fractions`,
/// giving every zone at least `min` dp of width, laid out in reading order.
///
/// The engine behind every in-node coordinate split: a table header's
/// label / filter divide, a `SplitButton`'s chevron, a tab's label versus its
/// close button. Using it rather than open-coding `bounds.width * 0.8` is what
/// keeps the split the widget *paints* identical to the split it *hit-tests*,
/// and what lets one floor be raised for every such control at once.
///
/// # The rules
///
/// * **Weights, not percentages.** `fractions` are normalised by their own sum,
///   so `&[0.8, 0.2]`, `&[4.0, 1.0]` and `&[80.0, 20.0]` all mean the same
///   thing. A negative or non-finite entry counts as `0.0`; an all-zero set
///   splits evenly.
/// * **The floor is enforced by clamp-and-redistribute.** Any zone whose
///   proportional share falls below `min` is pinned to `min`, and the remaining
///   width is re-proportioned among the zones still above it — repeated until
///   it settles (at most one pass per zone).
/// * **Reading order, not screen order.** The result is indexed by
///   `fractions`: `zones[0]` is the *leading* zone, which is the leftmost under
///   [`LeftToRight`](LayoutDirection::LeftToRight) and the **rightmost** under
///   [`RightToLeft`](LayoutDirection::RightToLeft). A caller never re-orders
///   for RTL.
/// * **The partition is exact.** Zones tile `bounds` with no gap and no
///   overlap; the last zone absorbs the floating-point residue, so the widths
///   sum to `bounds.width` to the bit.
/// * **`y` and `height` are `bounds`'.** This splits one axis; a vertical split
///   (a `TreeView` row's before / into / after thirds, a drop target's edge
///   bands) is [`DropRegion`](crate::styles::DropRegion)'s job, which has to
///   answer in two dimensions anyway.
///
/// # When the floor cannot be met
///
/// If `min × zones > bounds.width` there is no partition that honours the
/// floor, and the function **splits the width evenly** rather than honouring
/// the floor for some zones and starving others, or returning fewer rectangles
/// than it was asked for.
///
/// That is a deliberate choice among three bad options. Returning fewer
/// rectangles would break every caller that indexes the result and would make a
/// target silently vanish at a narrow width — the failure mode hardest to
/// notice and worst to hit. Honouring the floor for a prefix would make which
/// zone gets starved depend on declaration order, which is invisible at the
/// call site. An even split keeps every zone reachable, keeps the geometry
/// predictable, and leaves exactly one observable symptom: sub-floor zones,
/// which is precisely what the target-conformance audit is for. A caller that
/// would rather drop a zone than shrink it should check the width itself and
/// pass a shorter `fractions`.
pub fn partition_targets(
    bounds: Rect,
    fractions: &[f32],
    min: f32,
    direction: LayoutDirection,
) -> Vec<Rect> {
    let n = fractions.len();
    if n == 0 {
        return Vec::new();
    }
    let total = if bounds.width.is_finite() && bounds.width > 0.0 {
        bounds.width
    } else {
        0.0
    };
    let min = if min.is_finite() && min > 0.0 {
        min
    } else {
        0.0
    };

    let widths = solve_widths(total, fractions, min);

    // Lay the solved widths out along the axis, leading first. RTL walks the
    // rectangle from the right, so `zones[0]` is still the zone the reader
    // meets first.
    let mut zones = Vec::with_capacity(n);
    let mut cursor = 0.0_f32;
    for (i, w) in widths.iter().enumerate() {
        // The last zone absorbs the residue so the tiling is exact.
        let w = if i + 1 == n { total - cursor } else { *w };
        let x = match direction {
            LayoutDirection::LeftToRight => bounds.x + cursor,
            LayoutDirection::RightToLeft => bounds.x + total - cursor - w,
        };
        zones.push(Rect::new(x, bounds.y, w.max(0.0), bounds.height));
        cursor += w;
    }
    zones
}

/// Solve the one-dimensional distribution: proportional shares, with every zone
/// below `min` pinned to it and the rest re-proportioned.
fn solve_widths(total: f32, fractions: &[f32], min: f32) -> Vec<f32> {
    let n = fractions.len();
    // No partition can honour the floor — see the doc comment for why an even
    // split is the deliberate answer.
    if min * (n as f32) > total {
        return vec![total / n as f32; n];
    }

    let weights: Vec<f32> = fractions
        .iter()
        .map(|f| if f.is_finite() && *f > 0.0 { *f } else { 0.0 })
        .collect();
    let sum: f32 = weights.iter().sum();
    // An all-zero (or entirely invalid) weight set means "split evenly".
    let weights: Vec<f32> = if sum > 0.0 { weights } else { vec![1.0; n] };

    let mut pinned = vec![false; n];
    let mut widths = vec![0.0_f32; n];
    // Clamp-and-redistribute, exactly as the stack's shrink pass does: pin
    // everything that undershoots the floor, share what is left among the rest,
    // repeat. Each pass pins at least one zone, so it terminates in ≤ n passes.
    for _ in 0..=n {
        let free: f32 = total - min * pinned.iter().filter(|p| **p).count() as f32;
        let live_weight: f32 = weights
            .iter()
            .zip(&pinned)
            .filter(|(_, p)| !**p)
            .map(|(w, _)| *w)
            .sum();
        let mut newly_pinned = false;
        for i in 0..n {
            if pinned[i] {
                widths[i] = min;
                continue;
            }
            let share = if live_weight > 0.0 {
                free * weights[i] / live_weight
            } else {
                0.0
            };
            if share < min {
                pinned[i] = true;
                widths[i] = min;
                newly_pinned = true;
            } else {
                widths[i] = share;
            }
        }
        if !newly_pinned {
            break;
        }
    }
    widths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(zones: &[Rect]) -> Vec<f32> {
        zones.iter().map(|z| z.width).collect()
    }

    fn approx(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-3, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn an_empty_split_returns_nothing() {
        assert!(
            partition_targets(
                Rect::new(0.0, 0.0, 100.0, 10.0),
                &[],
                24.0,
                LayoutDirection::LeftToRight
            )
            .is_empty()
        );
    }

    /// Weights normalise by their own sum, so a caller may write percentages,
    /// fractions or ratios.
    #[test]
    fn weights_are_normalised_by_their_sum() {
        let r = Rect::new(0.0, 0.0, 100.0, 10.0);
        for f in [
            [0.8_f32, 0.2].as_slice(),
            [4.0, 1.0].as_slice(),
            [80.0, 20.0].as_slice(),
        ] {
            approx(
                &widths(&partition_targets(r, f, 0.0, LayoutDirection::LeftToRight)),
                &[80.0, 20.0],
            );
        }
    }

    /// The zones tile the bounds exactly — no gap, no overlap, no residue.
    #[test]
    fn the_partition_is_exact() {
        let r = Rect::new(7.5, 3.0, 101.0, 10.0);
        for dir in [LayoutDirection::LeftToRight, LayoutDirection::RightToLeft] {
            let z = partition_targets(r, &[1.0, 1.0, 1.0], 0.0, dir);
            let sum: f32 = z.iter().map(|q| q.width).sum();
            assert!(
                (sum - r.width).abs() < 1e-4,
                "{dir:?}: {sum} != {}",
                r.width
            );
            let mut xs: Vec<f32> = z.iter().map(|q| q.x).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert!((xs[0] - r.x).abs() < 1e-4);
            let last = z.iter().map(|q| q.right()).fold(f32::MIN, f32::max);
            assert!((last - r.right()).abs() < 1e-4);
        }
    }

    /// RTL keeps the *index* order and flips the *screen* order: `zones[0]` is
    /// the leading zone, which is on the right.
    #[test]
    fn rtl_reverses_the_screen_order_and_not_the_index_order() {
        let r = Rect::new(0.0, 0.0, 100.0, 10.0);
        let ltr = partition_targets(r, &[0.7, 0.3], 0.0, LayoutDirection::LeftToRight);
        let rtl = partition_targets(r, &[0.7, 0.3], 0.0, LayoutDirection::RightToLeft);
        // Same widths, in the same index order.
        approx(&widths(&ltr), &widths(&rtl));
        assert_eq!(ltr[0].x, 0.0, "LTR: the leading zone starts at the left");
        assert_eq!(
            rtl[0].right(),
            100.0,
            "RTL: the leading zone ends at the right"
        );
        assert_eq!(rtl[1].x, 0.0, "RTL: the trailing zone is on the left");
    }

    /// A zone whose proportional share undershoots the floor is pinned to it,
    /// and the rest re-proportion around the pin.
    #[test]
    fn the_floor_is_enforced_by_clamp_and_redistribute() {
        // 200 dp, 95/5 — the trailing zone's 10 dp share is below the 24 dp
        // floor, so it takes 24 and the leading zone keeps the other 176.
        let z = partition_targets(
            Rect::new(0.0, 0.0, 200.0, 28.0),
            &[0.95, 0.05],
            24.0,
            LayoutDirection::LeftToRight,
        );
        approx(&widths(&z), &[176.0, 24.0]);

        // Two zones under the floor, one comfortably above: both pin, the third
        // absorbs the rest.
        let z = partition_targets(
            Rect::new(0.0, 0.0, 200.0, 28.0),
            &[0.9, 0.05, 0.05],
            24.0,
            LayoutDirection::LeftToRight,
        );
        approx(&widths(&z), &[152.0, 24.0, 24.0]);
    }

    /// The pinning cascades: pinning one zone can push a second below the floor,
    /// and the loop must catch that rather than settling after one pass.
    #[test]
    fn pinning_cascades_until_it_settles() {
        // 100 dp, 24 dp floor, weights 70/20/10. First pass: 70/20/10 → the
        // 10 dp zone pins. Second pass: 76 free over 90 weight → 59.1 / 16.9 →
        // the 16.9 zone pins too. Third: 52 / 24 / 24.
        let z = partition_targets(
            Rect::new(0.0, 0.0, 100.0, 10.0),
            &[0.7, 0.2, 0.1],
            24.0,
            LayoutDirection::LeftToRight,
        );
        approx(&widths(&z), &[52.0, 24.0, 24.0]);
    }

    /// When `min × n` exceeds the width there is no conforming partition. The
    /// deliberate answer is an even split: every zone stays reachable, every
    /// zone is visibly sub-floor, and no caller's index goes missing.
    #[test]
    fn an_unmeetable_floor_splits_evenly_and_keeps_every_zone() {
        let z = partition_targets(
            Rect::new(0.0, 0.0, 50.0, 10.0),
            &[0.9, 0.05, 0.05],
            24.0,
            LayoutDirection::LeftToRight,
        );
        assert_eq!(z.len(), 3, "no zone may be dropped");
        approx(&widths(&z), &[50.0 / 3.0, 50.0 / 3.0, 50.0 / 3.0]);
        assert!(
            z.iter().all(|q| q.width < 24.0),
            "the shortfall stays visible to the audit rather than being hidden"
        );
        // Still an exact tiling.
        let sum: f32 = z.iter().map(|q| q.width).sum();
        assert!((sum - 50.0).abs() < 1e-4);
    }

    /// A degenerate input must not produce a `NaN` rectangle or panic.
    #[test]
    fn degenerate_inputs_are_inert() {
        let z = partition_targets(
            Rect::new(0.0, 0.0, 100.0, 10.0),
            &[f32::NAN, -1.0, 0.0],
            f32::NAN,
            LayoutDirection::LeftToRight,
        );
        approx(&widths(&z), &[100.0 / 3.0; 3]);
        let z = partition_targets(
            Rect::new(0.0, 0.0, 0.0, 10.0),
            &[1.0, 1.0],
            24.0,
            LayoutDirection::LeftToRight,
        );
        approx(&widths(&z), &[0.0, 0.0]);
    }

    #[test]
    fn a_region_carries_its_role_and_part() {
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(TargetRegion::target(r, 0).role, TargetRole::Target);
        assert_eq!(TargetRegion::grab(r, 1).role, TargetRole::Grab);
        assert_eq!(TargetRegion::decoration(r, 2).role, TargetRole::Decoration);
        assert_eq!(TargetRegion::grab(r, 9).part, 9);
    }
}
