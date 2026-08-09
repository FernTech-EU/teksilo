// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Overflow planning for [`SegmentedControl`](super::SegmentedControl).
//!
//! [`plan`] is a **pure** function of `(available width, per-segment
//! natural widths, the forced segment, chevron width, sizing, mode)`. It
//! owns the whole width story and is unit-tested in isolation — the
//! widget only feeds it measurements and applies the result. Same shape
//! as `Toolbar`'s private `compute_overflow`.
//!
//! ## The rule
//!
//! Declaration order is stable. Segments that do not fit move into a
//! trailing chevron menu, **from the end**. The one exception is the
//! *forced* segment (`must`): if it is not in the naturally-fitting
//! prefix it takes the **last visible slot**, evicting as many trailing
//! segments as it needs. That is what makes "the selected segment is
//! always visible" an invariant rather than a hope.
//!
//! ## Why the chevron reservation is a single up-front branch
//!
//! The "does everything fit with no chevron at all?" test runs once,
//! before any chevron width is reserved. The moment it fails, the
//! chevron's width is baked into `budget` for the rest of the call and
//! is never re-litigated. A "compute → toggle chevron → recompute" loop
//! would flap between "chevron shown" and "chevron hidden" at the exact
//! boundary width, one state per layout pass, forever.

use std::rc::Rc;

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;

use super::{Segment, SegmentSizing};

/// Float-fuzz tolerance for the fit comparisons, matching `Toolbar`.
const EPS: f32 = 0.5;

/// The outcome of one overflow calculation.
///
/// All indices address the **live** segment list — the subset whose
/// `Segment::visible` prop is currently `true`, in declaration order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Plan {
    /// Segments on the strip, in visual (reading) order. When a segment
    /// was forced into view it is the last entry; everything before it
    /// is a declaration-order prefix.
    pub visible: Vec<usize>,
    /// Width for each entry of [`visible`](Self::visible); same length.
    pub widths: Vec<f32>,
    /// Segments that did not fit, in declaration order. These become the
    /// chevron menu's rows.
    pub overflowed: Vec<usize>,
    /// Whether the trailing chevron trigger is shown at all.
    pub show_chevron: bool,
}

impl Plan {
    /// `true` when `index` is on the strip.
    pub fn is_visible(&self, index: usize) -> bool {
        self.visible.contains(&index)
    }

    /// The slot `index` occupies on the strip, if any. Slot indices
    /// address [`Plan::widths`] and the published slot geometry — they
    /// are *not* segment indices once a segment has been promoted.
    pub fn slot_of(&self, index: usize) -> Option<usize> {
        self.visible.iter().position(|&i| i == index)
    }
}

/// Plan the strip for one layout pass.
///
/// * `available` — inner width for the whole slot row, chevron included.
/// * `natural` — each live segment's measured intrinsic width.
/// * `must` — the segment that must stay visible (the promoted one), if any.
/// * `chevron` — measured intrinsic width of the overflow trigger.
pub(crate) fn plan(
    available: f32,
    natural: &[f32],
    must: Option<usize>,
    chevron: f32,
    sizing: SegmentSizing,
    compress: bool,
) -> Plan {
    let n = natural.len();
    if n == 0 {
        return Plan::default();
    }

    // Under `Uniform`, every segment is measured against the widest one,
    // so the strip never looks ragged; under `Fit`, against itself.
    let widest = natural.iter().copied().fold(0.0_f32, f32::max);
    let unit = |i: usize| match sizing {
        SegmentSizing::Uniform => widest,
        SegmentSizing::Fit => natural[i],
    };

    // The single up-front branch — see the module docs.
    let total: f32 = (0..n).map(unit).sum();
    if compress || total <= available + EPS {
        let visible: Vec<usize> = (0..n).collect();
        let widths = distribute(available, &visible, natural, sizing);
        return Plan {
            visible,
            widths,
            overflowed: Vec::new(),
            show_chevron: false,
        };
    }

    let budget = (available - chevron).max(0.0);

    // Greedy declaration-order prefix.
    let mut prefix: Vec<usize> = Vec::with_capacity(n);
    let mut used = 0.0_f32;
    for i in 0..n {
        let w = unit(i);
        if used + w <= budget + EPS {
            prefix.push(i);
            used += w;
        } else {
            break;
        }
    }

    // Force the promoted segment into the last slot, but only when it is
    // not already in the prefix — a segment that fits naturally is never
    // reordered.
    let mut visible = match must {
        Some(m) if m < n && !prefix.contains(&m) => {
            let mw = unit(m);
            while !prefix.is_empty() && used + mw > budget + EPS {
                let dropped = prefix.pop().expect("non-empty");
                used -= unit(dropped);
            }
            prefix.push(m);
            prefix
        }
        _ => prefix,
    };

    // A control that shows nothing is useless: keep one segment even when
    // the chevron alone would fill the width. The cell's single-line
    // label ellipsizes to whatever is left.
    if visible.is_empty() {
        visible.push(must.filter(|m| *m < n).unwrap_or(0));
    }

    let overflowed: Vec<usize> = (0..n).filter(|i| !visible.contains(i)).collect();
    let show_chevron = !overflowed.is_empty();
    let seg_space = if show_chevron {
        (available - chevron).max(0.0)
    } else {
        available
    };
    let widths = distribute(seg_space, &visible, natural, sizing);

    Plan {
        visible,
        widths,
        overflowed,
        show_chevron,
    }
}

/// Share `space` among the visible slots.
///
/// `Uniform` splits it evenly (today's look). `Fit` gives each slot its
/// natural width, sharing any leftover equally, and scales proportionally
/// when the slots must compress below natural.
fn distribute(space: f32, visible: &[usize], natural: &[f32], sizing: SegmentSizing) -> Vec<f32> {
    let k = visible.len();
    if k == 0 {
        return Vec::new();
    }
    let even = (space / k as f32).max(0.0);
    match sizing {
        SegmentSizing::Uniform => vec![even; k],
        SegmentSizing::Fit => {
            let natsum: f32 = visible.iter().map(|&i| natural[i]).sum();
            if natsum <= 0.0 {
                vec![even; k]
            } else if space >= natsum {
                let extra = (space - natsum) / k as f32;
                visible.iter().map(|&i| natural[i] + extra).collect()
            } else {
                let scale = space / natsum;
                visible
                    .iter()
                    .map(|&i| (natural[i] * scale).max(0.0))
                    .collect()
            }
        }
    }
}

/// Build the trailing overflow trigger: a chevron `IconButton` opening a
/// `MenuList` with one row per segment.
///
/// Every segment gets a row, built **once**; each row's visibility is
/// gated on that segment being overflowed, so resizing the control never
/// rebuilds the (dormant) popover subtree — it only flips visibility
/// props, which `MenuList` already honours in both layout and keyboard
/// navigation. This is `Toolbar`'s overflow-menu pattern.
///
/// Rows are `MenuItem::radio` bound to the control's private index
/// mirror, which is what gives them a real `Role::MenuItemRadio` and
/// automatic `push_to_radio_group` "N of M" from `MenuList` — no
/// `MenuItem` changes needed.
pub(crate) fn build_overflow_trigger(
    ctx: &mut BuildContext,
    segments: &[Segment],
    live: &[usize],
    index_mirror: &Signal<usize>,
    overflowed: &Signal<Vec<bool>>,
    icon_size: f32,
    select: Rc<dyn Fn(usize, &mut EventContext)>,
) -> WidgetId {
    use crate::icon_button::IconButton;
    use crate::menu_item::MenuItem;
    use crate::menu_list::MenuList;
    use crate::popover_widget::PopoverIconButton;
    use crate::primitives::IconWidget;
    use teksilo_core::accesskit::HasPopup;
    use teksilo_core::overlay::OverlayPlacement;

    let mut menu = MenuList::new();
    for (live_index, &seg_index) in live.iter().enumerate() {
        let segment = &segments[seg_index];
        // `radio` gives the row a real `Role::MenuItemRadio` and lets
        // `MenuList` wire `push_to_radio_group` automatically; the
        // activation handler routes the same change through the control's
        // funnel so `on_change` fires with an `EventContext` — something
        // the radio binding alone cannot provide.
        let mut item = MenuItem::new(segment.label.clone())
            .radio(live_index, index_mirror.clone())
            .enabled(not(&segment.disabled))
            .on_activate_fn({
                let select = select.clone();
                move |ctx| select(live_index, ctx)
            });
        // No `.icon()` here: `MenuItem` puts the radio dot in the leading
        // slot and rejects an icon outright, which is the right call — the
        // dot is what says "this is the current segment".
        if let Some(tip) = &segment.tooltip {
            item = item.tooltip(tip.clone());
        }
        // Fail open: the flag vector is seeded all-false at build time and
        // is re-published from `place_children`, so a short vector here
        // must read as "not overflowed" rather than panic.
        let flags = overflowed.clone();
        let visible = flags.map(move |f| f.get(live_index).copied() == Some(true));
        menu = menu.item_when(item, visible);
    }

    let trigger = IconButton::new(IconWidget::chevron_down(icon_size))
        .embedded()
        .tooltip(teksilo_i18n::tr_widget!(segmented_control_more()));

    ctx.add(
        PopoverIconButton::new(trigger)
            // `MenuList` already routes through the Menu `PopoverStyle`
            // for its own surface — don't double-chrome it.
            .bare()
            .content(menu)
            .placement(OverlayPlacement::BelowPreferred)
            .has_popup_kind(HasPopup::Menu),
    )
}

/// Logical negation of a `Prop<bool>`, preserving staticness.
///
/// `Signal::not` exists but `Prop` has no combinators, and routing a
/// `Prop::Static` through `as_signal()` would manufacture a bound signal
/// that can never change — a pointless binding on every menu row.
pub(crate) fn not(prop: &Prop<bool>) -> Prop<bool> {
    match prop {
        Prop::Static(v) => Prop::Static(!*v),
        Prop::Bound(signal) => Prop::Bound(signal.not()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 dp per segment, 30 dp chevron — easy arithmetic.
    fn nat(n: usize) -> Vec<f32> {
        vec![100.0; n]
    }

    #[test]
    fn everything_fits_means_no_chevron() {
        let p = plan(500.0, &nat(4), None, 30.0, SegmentSizing::Uniform, false);
        assert_eq!(p.visible, vec![0, 1, 2, 3]);
        assert!(p.overflowed.is_empty());
        assert!(!p.show_chevron);
        // Uniform fills the whole width.
        assert!(p.widths.iter().all(|w| (*w - 125.0).abs() < 0.01));
    }

    #[test]
    fn exact_fit_does_not_overflow_and_one_pixel_less_does() {
        // 4 x 100 = 400 natural.
        let fits = plan(400.0, &nat(4), None, 30.0, SegmentSizing::Uniform, false);
        assert!(!fits.show_chevron, "exactly 400 dp must fit 4 x 100 dp");

        let over = plan(399.0, &nat(4), None, 30.0, SegmentSizing::Uniform, false);
        assert!(over.show_chevron, "399 dp must not fit 4 x 100 dp");
        // budget = 399 - 30 = 369 -> 3 segments fit.
        assert_eq!(over.visible, vec![0, 1, 2]);
        assert_eq!(over.overflowed, vec![3]);
    }

    #[test]
    fn overflow_takes_from_the_end_in_declaration_order() {
        let p = plan(360.0, &nat(7), None, 30.0, SegmentSizing::Uniform, false);
        // budget = 330 -> 3 segments.
        assert_eq!(p.visible, vec![0, 1, 2]);
        assert_eq!(p.overflowed, vec![3, 4, 5, 6]);
        assert!(p.show_chevron);
    }

    #[test]
    fn must_inside_the_prefix_causes_no_reordering() {
        let p = plan(360.0, &nat(7), Some(1), 30.0, SegmentSizing::Uniform, false);
        assert_eq!(
            p.visible,
            vec![0, 1, 2],
            "a segment that already fits must keep its natural slot"
        );
    }

    #[test]
    fn must_outside_the_prefix_takes_the_last_slot() {
        let p = plan(360.0, &nat(7), Some(5), 30.0, SegmentSizing::Uniform, false);
        assert_eq!(
            p.visible,
            vec![0, 1, 5],
            "the forced segment evicts the trailing prefix entry and lands last"
        );
        assert_eq!(p.overflowed, vec![2, 3, 4, 6]);
    }

    #[test]
    fn at_least_one_segment_survives_an_impossibly_narrow_control() {
        let p = plan(20.0, &nat(5), Some(3), 30.0, SegmentSizing::Uniform, false);
        assert_eq!(p.visible, vec![3], "the forced segment is the last to go");
        assert_eq!(p.visible.len(), 1);
        assert!(p.widths.iter().all(|w| *w >= 0.0));
    }

    #[test]
    fn at_least_one_segment_survives_with_no_forced_segment() {
        let p = plan(20.0, &nat(5), None, 30.0, SegmentSizing::Uniform, false);
        assert_eq!(p.visible, vec![0]);
    }

    #[test]
    fn compress_mode_keeps_every_segment_and_never_shows_a_chevron() {
        let p = plan(120.0, &nat(7), Some(6), 30.0, SegmentSizing::Uniform, true);
        assert_eq!(p.visible.len(), 7);
        assert!(!p.show_chevron);
        assert!(p.overflowed.is_empty());
    }

    #[test]
    fn uniform_measures_against_the_widest_segment() {
        // One long segment forces every slot to its width, so fewer fit.
        let natural = vec![50.0, 50.0, 200.0, 50.0];
        let p = plan(300.0, &natural, None, 30.0, SegmentSizing::Uniform, false);
        // widest = 200 -> total 800 > 300; budget 270 -> only 1 slot.
        assert_eq!(p.visible, vec![0]);
    }

    #[test]
    fn fit_measures_each_segment_against_itself() {
        let natural = vec![50.0, 50.0, 200.0, 50.0];
        let p = plan(300.0, &natural, None, 30.0, SegmentSizing::Fit, false);
        // total 350 > 300; budget 270 -> 50 + 50 + 200 = 300 > 270, so
        // segments 0 and 1 fit and 2 does not.
        assert_eq!(p.visible, vec![0, 1]);
        assert_eq!(p.overflowed, vec![2, 3]);
        // Leftover 270 - 100 = 170 shared equally.
        assert!((p.widths[0] - 135.0).abs() < 0.01);
        assert!((p.widths[1] - 135.0).abs() < 0.01);
    }

    #[test]
    fn fit_compresses_proportionally_when_below_natural() {
        let natural = vec![100.0, 300.0];
        // Compress mode so both stay; 200 dp for 400 dp of content.
        let p = plan(200.0, &natural, None, 30.0, SegmentSizing::Fit, true);
        assert!((p.widths[0] - 50.0).abs() < 0.01);
        assert!((p.widths[1] - 150.0).abs() < 0.01);
    }

    #[test]
    fn empty_input_yields_an_empty_plan() {
        let p = plan(300.0, &[], None, 30.0, SegmentSizing::Uniform, false);
        assert!(p.visible.is_empty());
        assert!(p.widths.is_empty());
        assert!(!p.show_chevron);
    }

    #[test]
    fn widening_past_full_fit_restores_declaration_order_even_with_a_forced_segment() {
        // The promoted segment must not pin itself to the last slot once
        // there is room for everything.
        let p = plan(900.0, &nat(7), Some(5), 30.0, SegmentSizing::Uniform, false);
        assert_eq!(p.visible, vec![0, 1, 2, 3, 4, 5, 6]);
        assert!(!p.show_chevron);
    }

    #[test]
    fn slot_lookup_tracks_promotion() {
        let p = plan(360.0, &nat(7), Some(5), 30.0, SegmentSizing::Uniform, false);
        assert_eq!(p.slot_of(5), Some(2), "the promoted segment is last");
        assert_eq!(p.slot_of(0), Some(0));
        assert_eq!(p.slot_of(4), None);
        assert!(p.is_visible(5));
        assert!(!p.is_visible(4));
    }

    #[test]
    fn the_chevron_decision_is_monotonic_in_the_width() {
        // The anti-oscillation property: the chevron must never reappear
        // at a width where a narrower one already fit. A "compute →
        // toggle chevron → recompute" planner fails this at the boundary,
        // and would then flap one state per layout pass forever.
        let natural = nat(4);
        let mut first_fit: Option<f32> = None;
        for step in 0..=200 {
            let w = 300.0 + step as f32 * 0.5;
            let p = plan(w, &natural, None, 30.0, SegmentSizing::Uniform, false);
            match (p.show_chevron, first_fit) {
                (false, None) => first_fit = Some(w),
                (true, Some(fit)) => {
                    panic!("chevron reappeared at {w} dp after everything already fit at {fit} dp")
                }
                _ => {}
            }
        }
        assert!(
            first_fit.is_some(),
            "the sweep must cross the fit boundary or it proves nothing"
        );
    }

    #[test]
    fn a_narrower_width_never_shows_more_segments() {
        // Monotonicity of the visible count: shrinking the control can
        // only ever remove segments, never add them.
        let natural = nat(6);
        let mut previous = usize::MAX;
        for step in 0..=120 {
            let w = 700.0 - step as f32 * 5.0;
            let p = plan(w, &natural, None, 30.0, SegmentSizing::Uniform, false);
            assert!(
                p.visible.len() <= previous,
                "narrowing to {w} dp revealed a segment ({} > {previous})",
                p.visible.len()
            );
            previous = p.visible.len();
        }
        assert_eq!(previous, 1, "the floor is one visible segment");
    }
}
