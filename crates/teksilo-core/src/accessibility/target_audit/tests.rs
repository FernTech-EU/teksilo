// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The harness measuring itself.
//!
//! A conformance harness has one characteristic failure mode: **reporting zero
//! violations because it measures nothing**. Every test here exists to redden
//! under one specific way that could happen, and the file is the mutation
//! contract for the walker:
//!
//! | mutate | reddens |
//! | --- | --- |
//! | credit the outset arithmetically instead of probing it | [`a_grip_its_parent_hugs_reaches_nothing`] |
//! | probe with the mouse door (no outset, no slop) | [`a_grip_in_a_wide_parent_reaches_the_floor_through_its_outset`], [`the_audit_attributes_a_grips_reach_to_its_outset_not_to_slop`] |
//! | probe with the slop door only, never the exact one | [`the_same_control_alone_is_reached_by_the_slop_pass`] |
//! | drop `Widget::target_regions` from the walk | [`a_reported_region_is_measured_as_its_own_target`] |
//! | judge the painted rectangle instead of the reach | [`a_grip_in_a_wide_parent_reaches_the_floor_through_its_outset`] |
//! | let the spacing exception swallow every shortfall | [`an_undersized_control_beside_an_eligible_row_fails`] |
//! | compare a *capped* reach against the paint | [`a_target_wider_than_the_probe_budget_is_still_judged`] |
//! | claim a violation is never `capped` | [`a_violation_can_have_a_capped_axis_and_the_verdict_is_the_other_axis`] |
//! | drop the shadow test altogether | [`a_target_another_target_covers_part_of_is_not_judged_on_its_size`] |
//! | drop a region's credit for the growth confirmed at a shared edge | [`a_region_sharing_its_nodes_edge_inherits_the_growth_confirmed_there`] |
//! | revert `arena.rs`'s `won_through_outset`, so the slop pass takes an outset's claim back | [`a_grip_that_won_through_its_outset_keeps_its_point_against_the_slop_pass`] |
//! | re-derive the conformance floor downstream — in [`PinnedGeometry::covers`], or by stamping the generic table onto a row instead of the ladder the walker judged with | [`an_entry_is_judged_against_the_floor_the_walker_measured_not_the_generic_one`] |
//!
//! Every row above has been **run**, and two of them were wrong when the table
//! was first written. "Probe with the slop door only" named the attribution test,
//! which survives it: the attribution asks whether the reach is confirmed
//! *without* slop, and if both doors carry slop the answer is still yes — the
//! mouse-door mutation is what that test catches. And "judge the painted
//! rectangle" named the undersized-control test, which cannot catch it: a 10 dp
//! paint is a failure whether the paint or the reach is judged. The row that
//! test does catch is the eighth, which is the one that had been missing — the
//! shape where the gate goes quiet by calling every shortfall an exemption.
//!
//! The fixtures are built out of `crate::test_widgets`, so nothing here depends
//! on teksilo-widgets: the walker must be testable in the crate that owns it.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{EdgeInsets, Point, Rect, Size, SizeProposal};
use teksilo_tokens::{InputTokens, PointerKind, TargetDensity};

use super::*;
use crate::accessibility::target_audit::PinnedDp::{ClearsFloor, Is};
use crate::partition::TargetRegion;
use crate::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use crate::widget_builder::WidgetBuilder;

// ---------------------------------------------------------------------------
// Fixture widgets
// ---------------------------------------------------------------------------

/// A leaf of a fixed size. `outset` is what it declares from
/// [`Widget::hit_outset`] for a direct pointer; `regions` is what it reports
/// from [`Widget::target_regions`], as fractions of its own rectangle.
#[derive(Debug)]
struct Leaf {
    size: Size,
    outset: f32,
    regions: Vec<(u16, f32, f32)>,
}

impl Leaf {
    fn new(w: f32, h: f32) -> Self {
        Self {
            size: Size::new(w, h),
            outset: 0.0,
            regions: Vec::new(),
        }
    }

    fn outset(mut self, dp: f32) -> Self {
        self.outset = dp;
        self
    }

    /// A reported region occupying `[start, start + width]` of the leaf's own
    /// width, full height.
    fn region(mut self, part: u16, start: f32, width: f32) -> Self {
        self.regions.push((part, start, width));
        self
    }
}

impl Widget for Leaf {
    fn layout_response(&self, _p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        self.size.into()
    }

    fn hit_outset(&self, kind: PointerKind, _tokens: &InputTokens) -> EdgeInsets {
        if self.outset > 0.0 && kind.is_direct() {
            EdgeInsets {
                top: self.outset,
                bottom: self.outset,
                leading: self.outset,
                trailing: self.outset,
            }
        } else {
            EdgeInsets::ZERO
        }
    }

    fn target_regions(&self, bounds: Rect) -> Vec<TargetRegion> {
        self.regions
            .iter()
            .map(|&(part, start, width)| {
                TargetRegion::target(
                    Rect::new(bounds.x + start, bounds.y, width, bounds.height),
                    part,
                )
            })
            .collect()
    }
}

/// A container that places every child at its own intrinsic size, stacked
/// horizontally with a gap, and reports exactly the extent it was told to.
#[derive(Debug)]
struct Row {
    size: Size,
    gap: f32,
    pad: f32,
    stack: bool,
    clip: bool,
    children: Vec<crate::widget_id::WidgetId>,
}

impl Row {
    fn new(w: f32, h: f32) -> Self {
        Self {
            size: Size::new(w, h),
            gap: 0.0,
            pad: 0.0,
            stack: false,
            clip: false,
            children: Vec::new(),
        }
    }

    fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Inset the first child from the row's leading edge. Load-bearing in any
    /// fixture that measures an outset: a control flush against the window edge
    /// can grow on one side only, because a press outside the client area
    /// reaches nothing.
    fn pad(mut self, pad: f32) -> Self {
        self.pad = pad;
        self
    }

    /// Place every child at the same origin, so the later sibling — the one the
    /// reverse-sibling walk reaches first — covers the earlier one.
    fn stack(mut self) -> Self {
        self.stack = true;
        self
    }

    fn clip(mut self) -> Self {
        self.clip = true;
        self
    }

    fn child(mut self, id: crate::widget_id::WidgetId) -> Self {
        self.children.push(id);
        self
    }
}

impl Widget for Row {
    /// Re-attach the pre-registered children, and say so, because
    /// `set_input_density` marks the tree at `BindingLevel::Rebuild` and a
    /// re-derive would destroy the very subtree the fixture is measuring.
    fn build(
        &mut self,
        _ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        self.children.clone()
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn layout_response(&self, _p: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        self.size.into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _p: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let mut x = bounds.x + self.pad;
        for child in children.iter_mut() {
            let size = ctx
                .child_size(child.id, SizeProposal::unspecified())
                .unwrap_or(Size::new(0.0, 0.0));
            child.origin = Point::new(x, bounds.y + (bounds.height - size.height) / 2.0);
            child.size = size;
            if !self.stack {
                x += size.width + self.gap;
            }
        }
    }

    fn children(&self) -> Vec<crate::widget_id::WidgetId> {
        self.children.clone()
    }

    fn clips_children(&self) -> bool {
        self.clip
    }
}

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

fn tree_at(density: TargetDensity) -> WidgetTree {
    let mut tree = WidgetTree::new().with_theme(crate::presets::intui::light());
    tree.set_input_density(density);
    tree
}

fn tap() -> Rc<Cell<u32>> {
    Rc::new(Cell::new(0))
}

fn find(measurements: &[TargetMeasurement], widget: &str) -> TargetMeasurement {
    measurements
        .iter()
        .find(|m| m.path.ends_with(widget) && m.part.is_none())
        .unwrap_or_else(|| panic!("no measurement for {widget} in {measurements:#?}"))
        .clone()
}

// ---------------------------------------------------------------------------
// The reach limit: an outset only claims what its ancestors already own
// ---------------------------------------------------------------------------

/// A 6 dp grip that declares a 9 dp outset, inside a parent that hugs it and
/// clips — the shape P35 measured on the inspector's resize strip.
///
/// The outset is never offered a point outside the parent, and the clip stops
/// the slop pass reaching in, so the grip's reach is its own 6 dp. **A harness
/// that adds `6 + 2 × 9` certifies a 24 dp target that does not exist**, which
/// is why this is the first test in the file.
#[test]
fn a_grip_its_parent_hugs_reaches_nothing() {
    let counter = tap();
    let c = counter.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let grip = tree.add(Leaf::new(6.0, 6.0).outset(9.0).on_tap(move |_, _| {
        c.set(c.get() + 1);
    }));
    let hug = tree.add(Row::new(6.0, 6.0).clip().child(grip));
    // A pane beside the gutter, as a real splitter has: it denies the spacing
    // exception, so the undersized reach is judged as the AA failure it is.
    let n = tap();
    let cn = n.clone();
    let pane = tree.add(Leaf::new(200.0, 40.0).on_tap(move |_, _| cn.set(cn.get() + 1)));
    // One dp of air between the two, so the grip's right edge is its own and
    // not a boundary pixel shared with the pane painted beside it.
    let _root = tree.add(
        Row::new(400.0, 300.0)
            .pad(40.0)
            .gap(1.0)
            .child(hug)
            .child(pane),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(m.size, Size::new(6.0, 6.0));
    assert_eq!(
        m.expanded,
        Size::new(6.0, 6.0),
        "an outset its ancestors cannot offer a point to must be credited nothing",
    );
    assert!(!m.sources.any(), "no mechanism reached it: {m:?}");
    assert_eq!(m.rule, Some(TargetRule::MinTargetConformance));
}

/// The same grip in a parent large enough to offer the outset a point: now the
/// 9 dp ring works, and the reach is exactly `6 + 9 + 9`.
///
/// The assertion is an **equality**, not a `>=`: a bound here would pass for a
/// grip whose parent happened to be tappable itself.
#[test]
fn a_grip_in_a_wide_parent_reaches_the_floor_through_its_outset() {
    let counter = tap();
    let c = counter.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let grip = tree.add(Leaf::new(6.0, 6.0).outset(9.0).on_tap(move |_, _| {
        c.set(c.get() + 1);
    }));
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(grip));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(m.size, Size::new(6.0, 6.0));
    assert!(
        (m.expanded.width - 24.0).abs() < 0.1 && (m.expanded.height - 24.0).abs() < 0.1,
        "6 dp grip + 9 dp ring per edge = 24 dp, got {:?}",
        m.expanded,
    );
    assert_eq!(m.rule, None, "24 dp clears the AA floor at Compact");
}

/// The attribution: that grip's reach comes from the **exact** pass's outset,
/// not from the miss-only slop pass.
///
/// The slop pass would have offered only 8 dp (`min(profile.hit_slop,
/// slop_budget)`), so the last 1 dp of the ring is reachable through one door
/// and not the other — which is what makes the two distinguishable at all.
#[test]
fn the_audit_attributes_a_grips_reach_to_its_outset_not_to_slop() {
    let counter = tap();
    let c = counter.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let grip = tree.add(Leaf::new(6.0, 6.0).outset(9.0).on_tap(move |_, _| {
        c.set(c.get() + 1);
    }));
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(grip));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert!(m.sources.outset, "the outset delivers the ring: {m:?}");
    assert!(
        !m.sources.slop,
        "the slop pass is never needed where the outset already reaches: {m:?}",
    );
}

// ---------------------------------------------------------------------------
// The eligibility trap: an eligible bubble owner on the path
// ---------------------------------------------------------------------------

/// A 10 dp control inside a **tappable** row. The row owns every press the
/// control misses at distance zero, so the slop pass cannot re-attribute
/// anything, and the control's reach is its painted 10 dp.
///
/// This is the deliberately-undersized fixture the gate owes, and it is also
/// the discriminating geometry from A10's redundancy trap: put the same control
/// in a bare stack and the slop pass tops it up to 24 dp, making a broken
/// control look conformant.
#[test]
fn an_undersized_control_beside_an_eligible_row_fails() {
    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree = tree_at(TargetDensity::Compact);
    let small = tree.add(Leaf::new(10.0, 10.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _row = tree.add(
        Row::new(400.0, 40.0)
            .pad(40.0)
            .child(small)
            .on_tap(move |_, _| cb.set(cb.get() + 1)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(m.expanded, Size::new(10.0, 10.0), "{m:?}");
    assert_eq!(m.rule, Some(TargetRule::MinTargetConformance));

    let violations = target_audit(&tree, TargetDensity::Compact);
    assert!(
        violations
            .iter()
            .any(|v| v.path.ends_with("Leaf") && v.rule.is_conformance_failure()),
        "the audit must fail an undersized control: {violations:#?}",
    );
}

/// The same control with the row inert: now the slop pass does reach it, and it
/// is topped up to the density's `target_size`.
///
/// The pair of tests is the point. Neither alone tells you whether the harness
/// measures reach or geometry; together they pin that it measures reach, and
/// that reach depends on what is *around* a control.
#[test]
fn the_same_control_alone_is_reached_by_the_slop_pass() {
    let a = tap();
    let ca = a.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let small = tree.add(Leaf::new(10.0, 10.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _row = tree.add(Row::new(400.0, 40.0).pad(40.0).child(small));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert!(
        (m.expanded.width - 24.0).abs() < 0.1,
        "10 dp + 2 × ((24 − 10) / 2) = 24 dp, got {:?}",
        m.expanded,
    );
    assert!(m.sources.slop, "{m:?}");
    assert!(!m.sources.outset, "{m:?}");
    assert_eq!(m.rule, None);
}

/// The second discriminator A10 records: a control whose own box already
/// reaches `target_size` earns a slop top-up of exactly zero, so the pass is
/// not a fallback for a control the density projection already grew.
#[test]
fn a_control_already_at_the_target_size_earns_no_slop() {
    let a = tap();
    let ca = a.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let control = tree.add(Leaf::new(24.0, 24.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _row = tree.add(Row::new(400.0, 40.0).pad(40.0).child(control));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(m.expanded, Size::new(24.0, 24.0), "{m:?}");
    assert!(!m.sources.any(), "{m:?}");
    assert_eq!(m.rule, None);
}

// ---------------------------------------------------------------------------
// Reported regions
// ---------------------------------------------------------------------------

/// A widget that paints two zones inside one node has both measured, and an
/// undersized zone fails even though the node it lives in is comfortably large.
///
/// Without the `target_regions` leg of the walk, the node passes on its own
/// 200 dp and the 8 dp zone is invisible to the audit — the exact blindness the
/// hook exists to remove.
#[test]
fn a_reported_region_is_measured_as_its_own_target() {
    let a = tap();
    let ca = a.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let node = tree.add(
        Leaf::new(200.0, 30.0)
            .region(0, 0.0, 192.0)
            .region(1, 192.0, 8.0)
            .on_tap(move |_, _| ca.set(ca.get() + 1)),
    );
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(node));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let all = measure_targets(&tree, TargetDensity::Compact);
    let label = all
        .iter()
        .find(|m| m.part == Some(0))
        .expect("the wide zone is reported");
    let filter = all
        .iter()
        .find(|m| m.part == Some(1))
        .expect("the narrow zone is reported");
    assert_eq!(label.rule, None, "{label:?}");
    assert_eq!(
        filter.rule,
        Some(TargetRule::MinTargetConformance),
        "an 8 dp zone inside a 200 dp node is still an 8 dp target: {filter:?}",
    );
    // The node's own row passes, which is why the region leg is load-bearing.
    let own = find(&all, "Leaf");
    assert_eq!(own.rule, None, "{own:?}");
}

/// A region abutting its node's edge inherits the growth the walker
/// **confirmed** for that node there; an interior region inherits nothing.
///
/// Both regions belong to the same node and are covered by the same declared
/// outset, so the only thing that differs is whether the region shares the edge
/// the growth was confirmed at. That is what makes this an assertion about the
/// sharing rule rather than about the outset: the node's own reach is asserted
/// first, so a failure says which of the two is missing.
///
/// The three widgets that ship a `target_regions` split — `ScrollBar`,
/// `HueStrip`/`AlphaStrip`, `HeaderCell` — all report a part spanning their
/// node's full thickness, so without this credit every one of them is judged at
/// its 12-to-14 dp paint. Measured: deleting it puts 27 targets in the widgets
/// gate and 12 in scene's below the floor.
#[test]
fn a_region_sharing_its_nodes_edge_inherits_the_growth_confirmed_there() {
    let counter = tap();
    let c = counter.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    // 12 dp across, declaring 6 dp per side: 24 dp of reach, one dp inside the
    // Compact axis budget, so nothing here is capped and every number below is
    // an exact measurement.
    let bar = tree.add(
        Leaf::new(12.0, 200.0)
            .outset(6.0)
            .region(0, 0.0, 12.0)
            .region(1, 3.0, 6.0)
            .on_tap(move |_, _| c.set(c.get() + 1)),
    );
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(bar));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let all = measure_targets(&tree, TargetDensity::Compact);
    let node = find(&all, "Leaf");
    assert!(
        (node.expanded.width - 24.0).abs() <= 0.05,
        "the node's own ring must reach 24 dp for there to be anything to \
         inherit; if this is the line that failed, the outset is broken and the \
         region credit is not: {node:?}",
    );

    let full = all
        .iter()
        .find(|m| m.part == Some(0))
        .expect("the full-width part is reported");
    let interior = all
        .iter()
        .find(|m| m.part == Some(1))
        .expect("the interior part is reported");
    assert!(
        (full.expanded.width - 24.0).abs() <= 0.05,
        "a part spanning its node's whole width shares both side edges, so it \
         inherits both confirmed growths: {full:?}",
    );
    assert!(
        (interior.expanded.width - 6.0).abs() <= 0.05,
        "a part touching neither side edge inherits nothing horizontally — a \
         region is measured, never probed: {interior:?}",
    );
    assert_eq!(
        interior.rule,
        Some(TargetRule::MinTargetConformance),
        "and 6 dp is still 6 dp: {interior:?}",
    );
}

/// A grip that won its point through its own `Widget::hit_outset` keeps it: the
/// miss-only slop pass does not take a deliberate claim back.
///
/// The two mechanisms otherwise fight and the outset loses every time, because a
/// grip only ever claims a point at a **positive** distance from its own shape,
/// which is exactly the condition under which a slop-eligible node lying under
/// the ring is strictly closer. The row below is 28 dp — flush against the grip
/// and eligible — so it is at distance zero for every point in the ring.
///
/// Guarded here because the fix lives in `arena.rs`, and in teksilo-widgets by
/// `an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls`, which
/// pins the same two subjects on real controls: with the fix reverted a
/// `TableView`'s 12 dp scroll bar reaches **18 dp instead of 32** at Touch and a
/// `SearchField`'s 16 dp clear button reaches **22 dp instead of 24** at
/// **Compact** — reach falling as density rises, and a regression in a shipped
/// control at the density CI runs at. Both figures are that test's assertions,
/// re-measured for this note; the earlier "instead of 30" was the `ScrollArea`
/// fixture's bar, not the table's.
#[test]
fn a_grip_that_won_through_its_outset_keeps_its_point_against_the_slop_pass() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let c = tap();
        let cc = c.clone();
        let mut tree = tree_at(density);
        // A tappable band on EACH side, one dp clear of the grip. Every part of
        // this geometry is load-bearing and two earlier versions of it measured
        // nothing:
        //
        // * Both sides, because a band covers the ring's far points and so
        //   denies the grip the slop pass's own top-up. With one side open the
        //   grip earns 8 dp there above Compact against the ring's 6, and the
        //   measurement stops being the ring's — that version read 26.
        // * Four dp of air, because `collect_candidates` takes a candidate only
        //   at `distance > 0`: the pass is miss-only, so a band *containing* the
        //   point is never a candidate and a FLUSH band cannot steal anything.
        //   That version survived its own mutation. A band beats the grip only
        //   where it is the nearer of the two, which is the leading HALF of the
        //   gap — so the window is `gap / 2` wide, and at `gap = 1` it is 0.5 dp,
        //   exactly `PROBE_STEP`. The walk then samples both of its ends and
        //   never its inside, and that version survived too. Four dp gives a
        //   2 dp window and three samples in it. The gap is what a real
        //   `TableView` has between its rows and its scroll bar.
        // * 28 dp bands, because `((up_to - 28) / 2)` is 0 at Compact and 8 at
        //   Touch, so a band is a candidate only above Compact. That is the
        //   defect's real signature and why it was invisible at the density CI
        //   runs at: reverting the fix leaves this test green at Compact and
        //   reddens it at the other two.
        let bands: Vec<_> = (0..2)
            .map(|_| {
                let cr = tap();
                tree.add(Leaf::new(150.0, 28.0).on_tap(move |_, _| cr.set(cr.get() + 1)))
            })
            .collect();
        let grip = tree.add(
            Leaf::new(12.0, 200.0)
                .outset(6.0)
                .on_tap(move |_, _| cc.set(cc.get() + 1)),
        );
        let _root = tree.add(
            Row::new(400.0, 300.0)
                .pad(40.0)
                .gap(4.0)
                .child(bands[0])
                .child(grip)
                .child(bands[1]),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let all = measure_targets(&tree, density);
        let m = all
            .iter()
            .find(|x| x.part.is_none() && (x.size.width - 12.0).abs() <= 0.05)
            .unwrap_or_else(|| panic!("the grip is measured at {density:?}: {all:#?}"))
            .clone();
        assert!(
            (m.expanded.width - 24.0).abs() <= 0.05,
            "the grip keeps both halves of its own ring at {density:?}: \
             measured {:.4}, expected 24.00 — a reach short of that means a band \
             beside it took part of the ring back through the miss-only pass",
            m.expanded.width,
        );
        assert!(
            m.sources.outset,
            "and the reach is attributed to the exact pass at {density:?}: {:?}",
            m.sources,
        );
        assert!(
            !m.rule.is_some_and(|r| r.is_conformance_failure()),
            "24 dp clears the AA floor at {density:?}: {:?}",
            m.rule,
        );
    }
}

// ---------------------------------------------------------------------------
// Occlusion, delegation, empties
// ---------------------------------------------------------------------------

/// A target a sibling paints on top of has no reachable point at its centre,
/// and that is a conformance failure rather than something to skip: nothing can
/// press it.
#[test]
fn a_target_a_sibling_covers_is_a_failure_not_a_skip() {
    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree = tree_at(TargetDensity::Compact);
    let under = tree.add(Leaf::new(40.0, 40.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let over = tree.add(Leaf::new(40.0, 40.0).on_tap(move |_, _| cb.set(cb.get() + 1)));
    // Both children are placed at the same origin, `over` last, so it is walked
    // first by the reverse-sibling order.
    let _root = tree.add(
        Row::new(400.0, 300.0)
            .pad(40.0)
            .stack()
            .child(under)
            .child(over),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let all = measure_targets(&tree, TargetDensity::Compact);
    let covered = all
        .iter()
        .filter(|m| m.part.is_none() && m.path.ends_with("Leaf"))
        .find(|m| m.expanded.width == 0.0)
        .expect("one of the two leaves is unreachable");
    assert_eq!(covered.rule, Some(TargetRule::MinTargetConformance));
    assert_eq!(covered.skipped, None, "occlusion is judged, not skipped");
}

/// A wrapper whose tappable descendant covers its centre is a delegation
/// wrapper: the descendant is the target a user aims at and carries the row of
/// its own, so the wrapper is skipped rather than reported as a 0 dp target.
#[test]
fn a_wrapper_its_own_child_covers_delegates_rather_than_failing() {
    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree = tree_at(TargetDensity::Compact);
    let inner = tree.add(Leaf::new(40.0, 40.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let outer = tree.add(
        Row::new(40.0, 40.0)
            .child(inner)
            .on_tap(move |_, _| cb.set(cb.get() + 1)),
    );
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(outer));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let all = measure_targets(&tree, TargetDensity::Compact);
    let wrapper = all
        .iter()
        .find(|m| m.part.is_none() && m.path.ends_with("Row") && m.node == outer)
        .expect("the wrapper is collected");
    assert_eq!(wrapper.skipped, Some(SkipReason::DelegatesToDescendant));
    assert_eq!(wrapper.rule, None);
    let child = find(all.as_slice(), "Leaf");
    assert_eq!(child.rule, None, "{child:?}");
}

// ---------------------------------------------------------------------------
// The rules themselves
// ---------------------------------------------------------------------------

/// The three floors, stated as the module documents them: 24 dp conformance at
/// every density, 24 / 32 / 44 dp recommendation, and **44 dp is never AA**.
#[test]
fn the_conformance_floor_never_scales_and_the_recommendation_always_does() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let tokens = InputTokens::for_density(density);
        assert_eq!(tokens.min_target_conformance, 24.0);
        assert!(tokens.target_size >= tokens.min_target_conformance);
    }
    assert_eq!(
        InputTokens::for_density(TargetDensity::Touch).target_size,
        44.0,
        "the Touch ladder is SC 2.5.5 AAA / Apple HIG, and the rule that names \
         it must be TouchTargetRecommendation, not MinTargetConformance",
    );
    assert!(TargetRule::MinTargetConformance.is_conformance_failure());
    assert!(!TargetRule::TouchTargetRecommendation.is_conformance_failure());
    assert!(!TargetRule::SpacingException.is_conformance_failure());
}

/// A 24 dp control clears AA at every density and still trips the
/// recommendation above Compact — the distinction the audit exists to keep
/// visible without letting it into the gate.
///
/// The control sits in a **tappable** row on purpose. Isolate it and the slop
/// pass tops it up to the density's `target_size`, so the recommendation is
/// satisfied for free and the test would assert nothing above Compact; the
/// interaction is worth knowing, and it is why the fixture has a row.
#[test]
fn a_conformant_control_still_reports_the_recommendation_above_compact() {
    for (density, expect) in [
        (TargetDensity::Compact, None),
        (
            TargetDensity::Comfortable,
            Some(TargetRule::TouchTargetRecommendation),
        ),
        (
            TargetDensity::Touch,
            Some(TargetRule::TouchTargetRecommendation),
        ),
    ] {
        let a = tap();
        let b = tap();
        let (ca, cb) = (a.clone(), b.clone());
        let mut tree = tree_at(density);
        let control = tree.add(Leaf::new(24.0, 24.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
        let _row = tree.add(
            Row::new(400.0, 60.0)
                .pad(40.0)
                .child(control)
                .on_tap(move |_, _| cb.set(cb.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let m = find(&measure_targets(&tree, density), "Leaf");
        assert_eq!(m.expanded, Size::new(24.0, 24.0), "at {density:?}: {m:?}");
        assert_eq!(m.rule, expect, "at {density:?}: {m:?}");
        assert!(
            !m.rule.is_some_and(|r| r.is_conformance_failure()),
            "24 dp conforms at every density: {m:?}",
        );
    }
}

/// An isolated undersized target records the spacing exception it leans on
/// rather than being silently forgiven, and the exception is **not** a
/// conformance failure.
///
/// The fixture has to defeat the slop pass to get here, which a `clips_children`
/// hug does; the target is then genuinely alone, because nothing else in the
/// tree takes a press.
#[test]
fn an_isolated_undersized_target_records_the_spacing_exception() {
    let a = tap();
    let ca = a.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let small = tree.add(Leaf::new(6.0, 6.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let hug = tree.add(Row::new(6.0, 6.0).clip().child(small));
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(hug));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(m.expanded, Size::new(6.0, 6.0), "{m:?}");
    assert_eq!(m.rule, Some(TargetRule::SpacingException), "{m:?}");
    assert!(!m.rule.unwrap().is_conformance_failure());
}

/// Two undersized targets 4 dp apart cannot both claim the exception, so the
/// same 6 dp control that was excused alone is an AA failure beside a
/// neighbour.
#[test]
fn two_undersized_targets_close_together_lose_the_exception() {
    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree = tree_at(TargetDensity::Compact);
    let one = tree.add(Leaf::new(6.0, 6.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let two = tree.add(Leaf::new(6.0, 6.0).on_tap(move |_, _| cb.set(cb.get() + 1)));
    let hug = tree.add(Row::new(16.0, 6.0).gap(4.0).clip().child(one).child(two));
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(hug));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let all = measure_targets(&tree, TargetDensity::Compact);
    let rules: Vec<_> = all
        .iter()
        .filter(|m| m.part.is_none() && m.path.ends_with("Leaf"))
        .map(|m| m.rule)
        .collect();
    assert_eq!(
        rules,
        vec![
            Some(TargetRule::MinTargetConformance),
            Some(TargetRule::MinTargetConformance)
        ],
        "{all:#?}",
    );
}

// ---------------------------------------------------------------------------
// The doors
// ---------------------------------------------------------------------------

/// `audit_at_density` makes the tree's density and the audited ladder the same
/// thing, which is the one thing `target_audit`'s two arguments can get wrong.
#[test]
fn audit_at_density_switches_the_tree_before_it_measures() {
    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree = tree_at(TargetDensity::Compact);
    let control = tree.add(Leaf::new(24.0, 24.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _row = tree.add(
        Row::new(400.0, 60.0)
            .pad(40.0)
            .child(control)
            .on_tap(move |_, _| cb.set(cb.get() + 1)),
    );

    let compact = audit_at_density(&mut tree, TargetDensity::Compact, Size::new(400.0, 300.0));
    assert!(compact.is_empty(), "{compact:#?}");

    let touch = audit_at_density(&mut tree, TargetDensity::Touch, Size::new(400.0, 300.0));
    let leaf = touch
        .iter()
        .find(|v| v.path.ends_with("Leaf"))
        .expect("a fixed 24 dp leaf cannot follow the Touch ladder");
    assert_eq!(leaf.density, TargetDensity::Touch);
    assert_eq!(leaf.rule, TargetRule::TouchTargetRecommendation);
    assert!(
        touch.iter().all(|v| !v.rule.is_conformance_failure()),
        "and it still conforms at AA: {touch:#?}",
    );
}

/// An app-installed Tier-3 slot is **reported**, because a hand-written style
/// owns its own metrics and the framework overruling them would be worse than
/// saying so.
///
/// The question is asked of the slots themselves — derive the theme at two
/// densities and name the ones that are still the same object — and *not* of
/// whether the theme happens to carry a
/// [`DensityProjection`](crate::styles::DensityProjection). The third and fourth
/// legs below are why, and the third is the one that used to be wrong: a
/// projection that re-derives only the tokens leaves an app's style exactly as
/// frozen as a theme with no projection at all, and every shipped preset carries
/// a projection, so a presence check reported nothing in the case this function
/// exists for.
#[test]
fn an_app_installed_style_slot_is_reported_as_unprojected() {
    let plain = crate::presets::intui::light();
    assert!(
        unprojected_style_slots(&plain).is_empty(),
        "a preset with no slots installed has nothing to report",
    );

    // 1. No projection, an app slot: frozen at every density, so reported.
    let mut custom = crate::presets::intui::light();
    custom.style_slots.button = Some(std::rc::Rc::new(StubButtonStyle));
    assert_eq!(unprojected_style_slots(&custom), vec!["button"]);

    // 2. A projection that moves the tokens and carries the slots across
    //    untouched — which is what `Theme::with_density`'s own default branch
    //    does, and what a preset's projection does to any slot the preset does
    //    not own. The style is just as frozen, and saying otherwise because a
    //    projection is *registered* is the defect this leg holds.
    let mut tokens_only =
        crate::presets::intui::light().with_density_projection(|theme, density| {
            crate::styles::Theme {
                input: InputTokens::for_density(density),
                ..theme.clone()
            }
        });
    tokens_only.style_slots.button = Some(std::rc::Rc::new(StubButtonStyle));
    assert_eq!(
        unprojected_style_slots(&tokens_only),
        vec!["button"],
        "a projection that does not rebuild this slot leaves it frozen, \
         whatever else it re-derives",
    );

    // 3. And a projection that really does rebuild the slot reports nothing for
    //    it: the re-derived theme carries a different `Rc`, at every density.
    let mut rebuilding =
        crate::presets::intui::light().with_density_projection(|theme, density| {
            let mut out = crate::styles::Theme {
                input: InputTokens::for_density(density),
                ..theme.clone()
            };
            out.style_slots.button = Some(std::rc::Rc::new(StubButtonStyle));
            out
        });
    rebuilding.style_slots.button = Some(std::rc::Rc::new(StubButtonStyle));
    assert!(
        unprojected_style_slots(&rebuilding).is_empty(),
        "a theme that re-derives this slot reports none",
    );
}

#[derive(Debug)]
struct StubButtonStyle;

impl crate::styles::ButtonStyle for StubButtonStyle {
    fn make_body(
        &self,
        _cfg: &crate::styles::ButtonStyleConfig,
        ctx: &mut crate::build_context::BuildContext,
    ) -> crate::widget_id::WidgetId {
        ctx.add(Leaf::new(12.0, 12.0))
    }
}

// ---------------------------------------------------------------------------
// The shadow test: reach against paint, on the axes the probe actually finished
// ---------------------------------------------------------------------------

/// A target **wider than the probe's own axis budget** is judged, not written off
/// as shadowed.
///
/// The budget belongs to the axis and is shared between that axis's two
/// directions ([`probe_limit`], one dp past the largest floor), so the widest
/// reach any axis can report is one budget. Comparing that against the paint
/// makes every target wider than one floor "limited by another target" — by
/// arithmetic, with nothing on top of it — and a target the audit skips is a
/// target the gate never judges. A capped axis needs no verdict of its own
/// anyway: spending the budget is proof of clearing every floor.
///
/// Reddens if the shadow window is widened past the axis budget, or if `capped`
/// stops being tracked per axis.
#[test]
fn a_target_wider_than_the_probe_budget_is_still_judged() {
    let a = tap();
    let ca = a.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let node = tree.add(Leaf::new(200.0, 30.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _root = tree.add(Row::new(400.0, 300.0).pad(40.0).child(node));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(
        m.skipped, None,
        "nothing covers this node, so it is not shadowed by anything: {m:#?}",
    );
    assert!(m.capped, "and its horizontal axis spent the budget: {m:#?}");
    assert_eq!(
        m.rule, None,
        "a 200 x 30 dp target clears every floor: {m:#?}"
    );
}

/// A **violation** can be `capped`, and the field's own docs used to say it could
/// not.
///
/// The true statement is narrower: the axis that *decides* a verdict is never
/// capped, because [`probe_limit`] is one dp past the largest floor, so an axis
/// that spends the budget is proof of clearing every floor. Another axis of the
/// same violation can spend it — and then that figure in `expanded` is the budget
/// spent rather than a boundary, which anyone quoting a violation's reach needs
/// to know. Measured here rather than reasoned: a 200 x 10 dp control in a
/// tappable row fails on its height and spends the whole budget on its width. The
/// widgets gate's `link` and `spin_box` fixtures are the shipped instances.
#[test]
fn a_violation_can_have_a_capped_axis_and_the_verdict_is_the_other_axis() {
    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree = tree_at(TargetDensity::Compact);
    let wide = tree.add(Leaf::new(200.0, 10.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _row = tree.add(
        Row::new(400.0, 40.0)
            .pad(40.0)
            .child(wide)
            .on_tap(move |_, _| cb.set(cb.get() + 1)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let tokens = InputTokens::for_density(TargetDensity::Compact);
    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    assert_eq!(
        m.rule,
        Some(TargetRule::MinTargetConformance),
        "the height is 10 dp and the row beside it denies the slop pass: {m:#?}",
    );
    assert!(
        m.capped,
        "and the width axis spent the whole probe budget: {m:#?}",
    );
    assert!(
        (m.expanded.width - probe_limit(&tokens)).abs() <= 0.05,
        "so the width in `expanded` is the budget ({}), not a boundary —          measured {:.4}",
        probe_limit(&tokens),
        m.expanded.width,
    );
    assert!(
        (m.expanded.height - 10.0).abs() <= 0.05,
        "while the axis that decides the verdict is measured exactly: {m:#?}",
    );
    assert!(
        target_audit(&tree, TargetDensity::Compact)
            .iter()
            .any(|v| v.path.ends_with("Leaf") && v.rule.is_conformance_failure()),
        "and it reaches the gate as a failure",
    );
}

/// A target whose reach really does fall short of its own paint — because a
/// sibling painted on top owns part of it — is reported
/// [`SkipReason::ShadowedByAnotherTarget`] and judged against no floor.
///
/// The fixture is deliberately **narrow**: a 20 dp leaf with an 8 dp tappable
/// sibling stacked over its leading edge. The axis budget is shared between the
/// axis's two directions, so on a target wider than the budget the second
/// direction is starved and the axis reads `capped` — which is correct (a capped
/// reach is a lower bound and says nothing about who owns the rest of the paint)
/// and means the shadow rule only ever fires where the whole axis reach fits
/// inside one budget. That is also exactly where it matters: a capped axis is
/// provably past every floor, so a skip there would change no verdict, whereas
/// here the 14 dp reach would otherwise be reported as a 20 dp control failing on
/// its size when what limits it is the sibling.
#[test]
fn a_target_another_target_covers_part_of_is_not_judged_on_its_size() {
    let a = tap();
    let ca = a.clone();
    let b = tap();
    let cb = b.clone();
    let mut tree = tree_at(TargetDensity::Compact);
    let under = tree.add(Leaf::new(20.0, 30.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let over = tree.add(Leaf::new(8.0, 30.0).on_tap(move |_, _| cb.set(cb.get() + 1)));
    // `stack` places both children at the same origin, so `over` — reached first
    // by the reverse-sibling walk — covers `under`'s leading 8 dp.
    let _root = tree.add(
        Row::new(400.0, 300.0)
            .pad(40.0)
            .stack()
            .child(under)
            .child(over),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let all = measure_targets(&tree, TargetDensity::Compact);
    let m = all
        .iter()
        .find(|x| x.node == under && x.part.is_none())
        .expect("the covered leaf is measured");
    assert!(
        m.expanded.width + 0.05 < m.size.width,
        "the sibling really does take part of its width: reach {:?}, paint {:?}",
        m.expanded,
        m.size,
    );
    assert_eq!(
        m.skipped,
        Some(SkipReason::ShadowedByAnotherTarget),
        "what limits it is the sibling on top, not its size: {m:#?}",
    );
    assert_eq!(m.rule, None, "so it carries no verdict: {m:#?}");
}

// ---------------------------------------------------------------------------
// The ladder is the tree's own, not the generic table
// ---------------------------------------------------------------------------

/// A theme whose `target_size` at Touch is above the generic ladder's rung —
/// the shape Material 3 ships (`teksilo_theme_material3::input_tokens` raises it
/// to 48 dp), reproduced here so the walker's own crate can hold the property
/// without reaching for a preset crate.
fn theme_with_target_size(density: TargetDensity, target_size: f32) -> crate::styles::Theme {
    let mut theme = crate::presets::intui::light();
    theme.input = InputTokens {
        target_size,
        ..InputTokens::for_density(density)
    };
    theme
}

/// The probe budget and the recommendation floor both come from the **tree's**
/// token ladder, so a preset that raises `target_size` is measured against its
/// own number rather than against the generic table's.
///
/// The leaf is 46 dp inside a tappable row: the row denies the miss-only slop
/// pass any claim outside the leaf, so the reach is the leaf's own rectangle and
/// nothing else, and the figure below is a boundary the probe found rather than
/// a budget it spent.
///
/// Reading the generic table instead caps the measurement at `44 + 1` — one dp
/// past a floor this tree does not use — so the leaf comes back 45 dp wide,
/// `capped`, and clearing a recommendation it does not meet.
#[test]
fn the_probe_reads_the_trees_own_token_ladder() {
    let mut tree = WidgetTree::new().with_theme(theme_with_target_size(TargetDensity::Touch, 48.0));
    let leaf_tap = tap();
    let lt = leaf_tap.clone();
    let leaf = tree.add(Leaf::new(46.0, 46.0).on_tap(move |_, _| lt.set(lt.get() + 1)));
    let row_tap = tap();
    let rt = row_tap.clone();
    let row = tree.add(
        Row::new(300.0, 120.0)
            .pad(40.0)
            .child(leaf)
            .on_tap(move |_, _| rt.set(rt.get() + 1)),
    );
    let _root = tree.add(Row::new(400.0, 300.0).pad(20.0).child(row));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Touch), "Leaf");
    assert!(
        (m.expanded.width - 46.0).abs() < 0.1 && (m.expanded.height - 46.0).abs() < 0.1,
        "the leaf's own 46 dp, measured to a boundary: {:?}",
        m.expanded,
    );
    assert!(
        !m.capped,
        "a budget one dp past this tree's own 48 dp floor reaches the boundary: {m:?}",
    );
    assert_eq!(
        m.rule,
        Some(TargetRule::TouchTargetRecommendation),
        "46 dp clears the 24 dp AA floor and falls short of this theme's 48 dp \
         recommendation: {m:?}",
    );
}

/// What makes the change above a no-op for every existing measurement: the
/// ladder a tree carrying the Int UI preset ends up with **is** the generic
/// table's, at every density.
///
/// Int UI registers no `DensityProjection`, so `Theme::with_density` takes its
/// default branch. This asserts that through the real plumbing — `with_theme`,
/// `set_input_density`, `Theme::with_density` — rather than by restating the
/// expression, so it reddens if a projection is ever registered for Int UI or if
/// the density switch stops writing the theme. A preset that *does* carry one is
/// measured against its own ladder, which is
/// [`the_probe_reads_the_trees_own_token_ladder`].
#[test]
fn an_int_ui_trees_ladder_is_the_generic_table() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let tree = tree_at(density);
        assert_eq!(
            tree.theme().input,
            InputTokens::for_density(density),
            "an Int UI tree at {density:?} must carry the generic ladder, or \
             every measurement taken through the generic table before this \
             change silently moved",
        );
    }
}

/// A theme whose SC 2.5.8 conformance floor is above the generic ladder's
/// 24 dp, with `target_size` left where the density puts it — so the floor is
/// the only thing that moved, and `target_size >= min_target_conformance` still
/// holds the way every shipped `InputTokens` does.
fn theme_with_conformance_floor(density: TargetDensity, floor: f32) -> crate::styles::Theme {
    let mut theme = crate::presets::intui::light();
    theme.input = InputTokens {
        min_target_conformance: floor,
        ..InputTokens::for_density(density)
    };
    theme
}

/// **The floor a violation is excused against is the one it was judged
/// against.** An allow-list entry may not re-derive it from the generic table.
///
/// The two readings only differ where a theme raises the floor, which no shipped
/// preset does — so the divergence is invisible to every other test in the
/// programme, and this one builds the theme that exposes it. A 30 x 16 leaf
/// inside a tappable row under a 32 dp floor reaches its own rectangle and
/// nothing else: 16 dp fails the floor, and 30 dp — comfortably past the generic
/// 24 and comfortably short of this theme's 32 — is the axis the two readings
/// disagree about. An entry pinning that axis [`PinnedDp::ClearsFloor`] excuses
/// the failure if `covers` asks `InputTokens::for_density`, and refuses it if it
/// asks the violation.
///
/// Three legs, because the first two alone would pass for the wrong reason. The
/// walker must stamp the theme's floor; the entry must refuse; and the *same*
/// entry with that one axis pinned exactly must still cover, so what refuses is
/// the floor comparison and not a path, a density, a theme or a second axis.
///
/// Mutation contract: restore
/// `let floor = InputTokens::for_density(v.density).min_target_conformance;` in
/// [`PinnedGeometry::covers`] and this test goes red on its second leg.
#[test]
fn an_entry_is_judged_against_the_floor_the_walker_measured_not_the_generic_one() {
    const RAISED: f32 = 32.0;
    let generic = InputTokens::for_density(TargetDensity::Touch).min_target_conformance;
    assert_eq!(
        generic, 24.0,
        "the generic table's floor is what this test contrasts against",
    );

    let a = tap();
    let b = tap();
    let (ca, cb) = (a.clone(), b.clone());
    let mut tree =
        WidgetTree::new().with_theme(theme_with_conformance_floor(TargetDensity::Touch, RAISED));
    let small = tree.add(Leaf::new(30.0, 16.0).on_tap(move |_, _| ca.set(ca.get() + 1)));
    let _row = tree.add(
        Row::new(400.0, 60.0)
            .pad(40.0)
            .child(small)
            .on_tap(move |_, _| cb.set(cb.get() + 1)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let violation = target_audit(&tree, TargetDensity::Touch)
        .into_iter()
        .find(|v| v.path.ends_with("Leaf"))
        .expect("the leaf fails the raised floor");
    assert_eq!(
        violation.rule,
        TargetRule::MinTargetConformance,
        "16 dp is under 32: {violation:?}",
    );
    assert!(
        (violation.expanded.width - 30.0).abs() < 0.1
            && (violation.expanded.height - 16.0).abs() < 0.1,
        "the tappable row denies the slop pass, so the reach is the leaf's own \
         rectangle: {violation:?}",
    );

    // 1. The walker judged against the tree's floor and says so on the row.
    assert_eq!(
        violation.conformance_floor, RAISED,
        "the row must carry the floor its verdict was taken against, not the \
         generic {generic}: {violation:?}",
    );

    // 2. And the entry reads that, not the generic table. 30 dp clears 24 and
    //    does not clear 32, so the two readings give opposite answers here.
    assert!(
        ClearsFloor.matches(violation.expanded.width, generic),
        "read against the generic table, 30 dp clears the floor — which is what \
         would excuse this failure",
    );
    let entry = AllowedViolation {
        path: "Leaf",
        measured: &[PinnedGeometry {
            densities: &[TargetDensity::Touch],
            themes: &["intui.light"],
            paints: (ClearsFloor, Is(16.0)),
            reaches: (ClearsFloor, Is(16.0)),
        }],
        owner: "this test",
        exception: None,
        why: "a fixture entry",
    };
    assert!(
        !entry.matches(&violation),
        "30 dp is under this tree's {RAISED} dp floor, so a `ClearsFloor` axis \
         does not clear it and the entry must not excuse the failure the walker \
         just reported: {violation}",
    );

    // 3. Nothing else about the entry refuses it: pin that one axis exactly and
    //    the same entry covers, so leg 2 failed on the floor comparison alone.
    let exact = AllowedViolation {
        measured: &[PinnedGeometry {
            densities: &[TargetDensity::Touch],
            themes: &["intui.light"],
            paints: (Is(30.0), Is(16.0)),
            reaches: (Is(30.0), Is(16.0)),
        }],
        ..entry
    };
    assert!(
        exact.matches(&violation),
        "the path, density, theme and both exact axes all match — only the \
         `ClearsFloor` reading separates the two entries: {violation}",
    );
}

// ---------------------------------------------------------------------------
// The measurement slack covers an extent, not a direction
// ---------------------------------------------------------------------------

/// Every figure this module compares is an **extent** — two directions'
/// boundaries added — so the slack it allows for its own refinement error must
/// cover two quanta, not one; and it must stay a bound on that error rather than
/// growing into a tolerance on the finding.
///
/// Two-sided deliberately, because the two sides fail in opposite directions and
/// both are live. Below two quanta the slack judges a target that is short only
/// by the probe — a verdict about the audit rather than about the tree — and,
/// because the same arithmetic decides whether a row is filed as shadowed, it
/// silences a real shortfall rather than forgiving it. Far above it the slack
/// starts forgiving genuine shortfalls at the floor: the same constant sits in
/// [`classify`](super::Walker::classify), where every extra dp is a dp under
/// 24 that stops being reported.
///
/// The upper bound is twice the noise band, which is where the current value's
/// third quantum lives — margin for two rows of one control landing either side
/// of a quantum, the reason [`PIN_TOLERANCE`] carries a third as well, and
/// measured to move no row of any fixture list.
#[test]
fn the_shadow_slack_covers_an_extent_not_a_direction() {
    let quantum = PROBE_STEP / (1 << PROBE_REFINE) as f32;
    assert!(
        PROBE_EPSILON >= 2.0 * quantum,
        "an extent carries two directions' refinement error ({} dp), and the \
         slack is {} dp",
        2.0 * quantum,
        PROBE_EPSILON,
    );
    assert!(
        PROBE_EPSILON <= 4.0 * quantum,
        "a slack more than twice the noise band ({} dp) is no longer a bound on \
         the probe's error but a tolerance on the finding, and the same constant \
         decides what `classify` reports against the 24 dp floor; it is {} dp",
        4.0 * quantum,
        PROBE_EPSILON,
    );
}

/// A target short of its own paint **only by the probe's own rounding** is
/// judged, not filed as shadowed by something on top of it.
///
/// The shadow test exists to say "what limits this target is another target,
/// not its size". A slack narrower than two refinement quanta makes it say that
/// about a target nothing overlaps, and the row then carries no verdict at all
/// — a gate that goes quiet rather than red. This leaf has a tappable row
/// around it and no sibling anywhere near it, so the only thing between its
/// paint and its reach is the bisection.
///
/// The extent is deliberately off the refinement grid and chosen near the TOP
/// of the noise band: a shortfall anywhere in the band is noise, so a slack
/// picked from inside the band would still pass against a fixture near the
/// bottom of it. The assertions below hold the fixture there.
#[test]
fn a_target_short_of_its_paint_only_by_measurement_noise_is_still_judged() {
    let quantum = PROBE_STEP / (1 << PROBE_REFINE) as f32;
    let mut tree = tree_at(TargetDensity::Compact);
    let leaf_tap = tap();
    let lt = leaf_tap.clone();
    // 21.748 dp: half of it falls just under a refinement grid point, so each
    // direction's boundary is reported almost a whole quantum short.
    let leaf = tree.add(Leaf::new(21.748, 21.748).on_tap(move |_, _| lt.set(lt.get() + 1)));
    let row_tap = tap();
    let rt = row_tap.clone();
    let row = tree.add(
        Row::new(300.0, 120.0)
            .pad(40.0)
            .child(leaf)
            .on_tap(move |_, _| rt.set(rt.get() + 1)),
    );
    let _root = tree.add(Row::new(400.0, 300.0).pad(20.0).child(row));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let m = find(&measure_targets(&tree, TargetDensity::Compact), "Leaf");
    let shortfall = m.size.height - m.expanded.height;
    assert!(
        shortfall > 1.9 * quantum && shortfall < 2.0 * quantum,
        "the fixture must sit near the top of the two-quantum noise band, or a \
         slack picked from inside the band would still cover it: shortfall {} \
         dp, band ({}, {})",
        shortfall,
        quantum,
        2.0 * quantum,
    );
    assert_eq!(
        m.skipped, None,
        "nothing overlaps this leaf, so the only gap between its paint and its \
         reach is the probe's own: {m:#?}",
    );
    assert_eq!(
        m.rule,
        Some(TargetRule::MinTargetConformance),
        "and a 21.7 dp target under the 24 dp floor is the failure it looks \
         like: {m:#?}",
    );
}
