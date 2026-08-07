// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared main-then-cross negotiation for the linear stack containers.
//!
//! `HStack` and `VStack` differ only in which axis is the *main* (distribution)
//! axis and which is the *cross* axis, plus how they align and order children.
//! The sizing math — grow on positive slack, shrink on a deficit, and the
//! height-for-width cross pass — is identical, so it lives here once.
//!
//! The negotiation, for a given main/cross extent:
//!
//! 1. **Main query.** Ask each child for its `LayoutResponse` with the main
//!    axis open and the cross axis at `cross_extent`. This yields the child's
//!    wanted main size, grow weight (`flex`), shrink weight (`shrink`), and
//!    compression floor (`min`).
//! 2. **Distribute the main axis.** With surplus, grow children proportional to
//!    `flex`. With a deficit (over-constraint), shrink children proportional to
//!    `shrink`, never below their `min`, redistributing as children hit their
//!    floor. Rigid children (`flex == 0` / `shrink == 0`) are untouched.
//! 3. **Cross pass (height-for-width).** For each child whose final main size
//!    differs from its wanted main size, re-query its cross size *at that final
//!    main size* — so wrapped text re-wraps and aspect-ratio content re-derives
//!    its cross extent. Unchanged children reuse the pass-1 cross size.
//! 4. **Aggregate** the container's own size, grow/shrink weights, and `min`
//!    floor, so a nested stack participates in its parent's distribution.

use teksilo_canvas::{Size, SizeProposal};
use teksilo_core::widget::{LayoutContext, LayoutResponse};
use teksilo_core::widget_id::WidgetId;

/// Sub-pixel tolerance: changes smaller than this neither trigger a cross
/// re-query nor keep the shrink loop iterating.
const EPS: f32 = 1.0e-3;

/// Which axis a linear container distributes along.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    #[inline]
    fn main_of(self, s: Size) -> f32 {
        match self {
            Axis::Horizontal => s.width,
            Axis::Vertical => s.height,
        }
    }

    #[inline]
    fn cross_of(self, s: Size) -> f32 {
        match self {
            Axis::Horizontal => s.height,
            Axis::Vertical => s.width,
        }
    }

    /// Build a `Size` from main/cross components for this axis.
    #[inline]
    fn size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::Horizontal => Size::new(main, cross),
            Axis::Vertical => Size::new(cross, main),
        }
    }

    /// Core-level main-axis tag, so orientation-agnostic children (`Spacer`)
    /// can size per axis via [`LayoutContext::with_stack_main_axis`].
    #[inline]
    fn stack_axis(self) -> teksilo_core::widget::StackAxis {
        match self {
            Axis::Horizontal => teksilo_core::widget::StackAxis::Horizontal,
            Axis::Vertical => teksilo_core::widget::StackAxis::Vertical,
        }
    }

    /// Build a `SizeProposal` from optional main/cross components for this axis.
    #[inline]
    fn proposal(self, main: Option<f32>, cross: Option<f32>) -> SizeProposal {
        match self {
            Axis::Horizontal => SizeProposal {
                width: main,
                height: cross,
            },
            Axis::Vertical => SizeProposal {
                width: cross,
                height: main,
            },
        }
    }
}

/// Per-child placement sizes produced by [`negotiate`], aligned 1:1 with the
/// input id slice (inactive ids report `(0.0, 0.0)`).
pub(crate) struct ChildSizes {
    pub main: Vec<f32>,
    pub cross: Vec<f32>,
}

/// Result of negotiating a linear container's children.
pub(crate) struct Negotiated {
    /// The container's own wanted size for the given proposal.
    pub size: Size,
    /// Aggregate grow weight: `Σ flex` if any child grows, else `0.0`.
    pub flex: f32,
    /// Aggregate shrink weight: `1.0` if any child can shrink, else `0.0`.
    pub shrink: f32,
    /// Aggregate compression floor (main = `Σ child.min_main + spacing`,
    /// cross = `max child.min_cross`).
    pub min: Size,
    /// Per-child final placement sizes (aligned to the input id slice).
    pub children: ChildSizes,
}

/// Distribute `ids` along `axis` within the given extents and produce both the
/// container's own [`LayoutResponse`] inputs and each child's final size.
///
/// `main_extent` / `cross_extent` are `Some(_)` when the parent has fixed that
/// axis (e.g. `place_children` with concrete bounds, or `layout_response` given
/// an exact proposal), `None` when asking for the intrinsic size on that axis.
pub(crate) fn negotiate(
    ids: &[WidgetId],
    ctx: &LayoutContext,
    main_extent: Option<f32>,
    cross_extent: Option<f32>,
    spacing: f32,
    axis: Axis,
) -> Negotiated {
    let n = ids.len();

    // Children are queried under a context that advertises this stack's main
    // axis, so an orientation-agnostic flexible child (`Spacer`) sizes its
    // minimum length on the main axis and reports `0` on the cross axis.
    let cctx = ctx.with_stack_main_axis(axis.stack_axis());
    let ctx = &cctx;

    // ── Pass 1: query each child along the main axis (cross offered) ─────────
    let main_proposal = axis.proposal(None, cross_extent);
    let mut active = vec![false; n];
    let mut wanted_main = vec![0.0_f32; n];
    let mut wanted_cross = vec![0.0_f32; n];
    let mut flex = vec![0.0_f32; n];
    let mut shrink = vec![0.0_f32; n];
    let mut min_main = vec![0.0_f32; n];
    let mut min_cross = vec![0.0_f32; n];

    let mut active_count = 0usize;
    let mut sum_wanted = 0.0_f32;
    let mut total_flex = 0.0_f32;

    for (i, &id) in ids.iter().enumerate() {
        let Some(r) = ctx.child_layout_response(id, main_proposal) else {
            continue; // inactive / missing — contributes nothing
        };
        active[i] = true;
        active_count += 1;
        wanted_main[i] = axis.main_of(r.size);
        wanted_cross[i] = axis.cross_of(r.size);
        flex[i] = r.flex;
        shrink[i] = r.shrink;
        min_main[i] = axis.main_of(r.min);
        min_cross[i] = axis.cross_of(r.min);
        sum_wanted += wanted_main[i];
        total_flex += r.flex;
    }

    let total_spacing = spacing * (active_count.saturating_sub(1)) as f32;

    // ── Pass 2: distribute the main axis ────────────────────────────────────
    let mut cur_main = wanted_main.clone();
    if let Some(extent) = main_extent {
        let raw = extent - sum_wanted - total_spacing;
        if raw > EPS && total_flex > 0.0 {
            // Surplus → grow proportional to flex.
            for i in 0..n {
                if active[i] && flex[i] > 0.0 {
                    cur_main[i] = wanted_main[i] + (flex[i] / total_flex) * raw;
                }
            }
        } else if raw < -EPS {
            // Deficit → shrink proportional to shrink weight, floored at min,
            // redistributing as children clamp to their floor.
            distribute_deficit(-raw, &active, &mut cur_main, &min_main, &shrink);
        }
    }

    // ── Pass 3: cross size at the FINAL main size (height-for-width) ─────────
    let mut cross = wanted_cross.clone();
    for i in 0..n {
        if active[i] && (cur_main[i] - wanted_main[i]).abs() > EPS {
            let p = axis.proposal(Some(cur_main[i]), cross_extent);
            if let Some(r) = ctx.child_layout_response(ids[i], p) {
                cross[i] = axis.cross_of(r.size);
            }
        }
    }

    // ── Pass 4: aggregate the container's own size + weights + floor ─────────
    let mut content_main = total_spacing;
    let mut self_cross = 0.0_f32;
    let mut agg_min_main = total_spacing;
    let mut agg_min_cross = 0.0_f32;
    let mut any_shrink = false;
    for i in 0..n {
        if !active[i] {
            continue;
        }
        content_main += cur_main[i];
        self_cross = self_cross.max(cross[i]);
        agg_min_main += min_main[i];
        agg_min_cross = agg_min_cross.max(min_cross[i]);
        if shrink[i] > 0.0 {
            any_shrink = true;
        }
    }

    // Both axes report the *content* size, symmetrically.
    //
    // Main: grow makes it equal the offered extent, shrink reduces it toward
    // the floor, and a rigid stack reports its natural content (never
    // over-claiming an offered extent it did not fill).
    //
    // Cross: fills the offered extent when given one, but never *hides* an
    // over-constrained child behind it. Reporting the bare offered extent
    // (`cross_extent.unwrap_or(self_cross)`) discarded the larger natural max
    // computed in pass 4, which made cross-axis over-constraint invisible to
    // every ancestor: a `VStack` in a 560 dp slot containing an 800 dp `HStack`
    // reported 560, so an enclosing `ScrollArea` concluded "no overflow",
    // showed no horizontal scroll bar, and `clips_children` silently swallowed
    // the excess — leaving content unreachable at *any* scroll position.
    //
    // `max` is a no-op whenever content fits (natural <= offered returns the
    // offered extent, exactly as before), so this changes behaviour only where
    // the layout is already over-constrained.
    let self_main = content_main.max(0.0);
    let self_cross = cross_extent
        .map(|e| e.max(self_cross))
        .unwrap_or(self_cross);

    let size = axis.size(self_main, self_cross);
    let min = axis.size(agg_min_main, agg_min_cross);

    // Only advertise grow/shrink to the parent when the parent is distributing
    // along THIS stack's main axis — i.e. it left the main axis open
    // (`main_extent.is_none()`). `flex`/`shrink` are axis-agnostic scalars the
    // parent applies to ITS main axis, so an HStack with a horizontal `Spacer`
    // placed in a VStack must NOT report flex (it does not grow vertically).
    // When the main axis is fixed (the parent is distributing our cross axis, or
    // gave exact bounds), a stack neither grows nor shrinks for that query. The
    // `min` floor is a per-axis `Size`, always a valid floor, so it is reported
    // unconditionally.
    let distribute_main = main_extent.is_none();
    let agg_flex = if distribute_main && total_flex > 0.0 {
        total_flex
    } else {
        0.0
    };
    let agg_shrink = if distribute_main && any_shrink {
        1.0
    } else {
        0.0
    };

    Negotiated {
        size,
        flex: agg_flex,
        shrink: agg_shrink,
        min,
        children: ChildSizes {
            main: cur_main,
            cross,
        },
    }
}

/// Convenience: turn a [`Negotiated`] into the container's own
/// [`LayoutResponse`] (size + grow/shrink weights + compression floor).
pub(crate) fn response(n: &Negotiated) -> LayoutResponse {
    LayoutResponse::flexible(n.size, n.flex)
        .with_shrink(n.shrink)
        .with_min(n.min)
}

/// Distribute a positive `deficit` across shrinkable children proportional to
/// their shrink weight, never below `min_main`. When a child clamps to its
/// floor it is frozen and the remaining deficit is redistributed among the
/// still-shrinkable children. Terminates in at most `O(n)` rounds; any residual
/// deficit (every shrinkable child at its floor, or none shrinkable) is left as
/// unavoidable overflow.
fn distribute_deficit(
    mut deficit: f32,
    active: &[bool],
    cur_main: &mut [f32],
    min_main: &[f32],
    shrink: &[f32],
) {
    let n = cur_main.len();
    let mut frozen = vec![false; n];
    loop {
        // Total shrink weight of the still-shrinkable pool.
        let mut total_shrink = 0.0_f32;
        for i in 0..n {
            if active[i] && !frozen[i] && shrink[i] > 0.0 && cur_main[i] - min_main[i] > EPS {
                total_shrink += shrink[i];
            }
        }
        if total_shrink < EPS || deficit < 0.01 {
            break; // nothing left to give, or deficit absorbed
        }

        let mut clamped_any = false;
        let mut absorbed = 0.0_f32;
        for i in 0..n {
            if !(active[i] && !frozen[i] && shrink[i] > 0.0) {
                continue;
            }
            let room = cur_main[i] - min_main[i];
            if room <= EPS {
                frozen[i] = true;
                continue;
            }
            let want = deficit * (shrink[i] / total_shrink);
            let take = want.min(room);
            cur_main[i] -= take;
            absorbed += take;
            if room - take <= EPS {
                frozen[i] = true;
                clamped_any = true;
            }
        }

        deficit -= absorbed;
        // If a full round absorbed effectively nothing and clamped nothing,
        // stop to avoid spinning on float dust.
        if absorbed < EPS && !clamped_any {
            break;
        }
    }
}
