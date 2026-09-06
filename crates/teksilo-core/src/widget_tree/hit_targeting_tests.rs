// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The adversarial hit-targeting suite.
//!
//! Every rule A10 states about the three mechanisms is pinned here against a
//! real [`WidgetTree`], one rule per test, with the geometry spelled out in the
//! comment so a failure reads as a sentence rather than a coordinate.
//!
//! It lives under `widget_tree` rather than beside the [`HitSlop`] algebra
//! because these are end-to-end tests of the arena and the router's doors; the
//! pure arithmetic is unit-tested in `pointer/hit_slop/tests.rs`.

use std::cell::Cell as StdCell;
use std::rc::Rc;

use teksilo_canvas::{EdgeInsets, Point, Rect, SizeProposal};
use teksilo_tokens::{InputTokens, PointerKind, PointerKindMask, TargetDensity};

use crate::build_context::BuildContext;
use crate::pointer::hit_slop::{HitContext, HitSlop, circle_distance, rect_distance};
use crate::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use crate::widget_builder::HandlerSet;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// A leaf that can declare any combination of the hit-targeting hooks, so one
/// fixture covers every rule rather than a widget per case.
#[derive(Default)]
struct Cell {
    /// Non-zero to declare a `Widget::hit_outset`.
    outset: f32,
    /// Which pointer kinds the outset is offered to.
    outset_kinds: Option<PointerKindMask>,
    /// A `Widget::hit_slop` override — the third link of the chain.
    slop: Option<HitSlop>,
    /// Report a disc rather than the rectangle from `hit_distance`.
    round: bool,
    /// Withdraw from the slop pass entirely at the shape level.
    no_distance: bool,
    /// Carry an `on_tap`, which is what makes a node "eligible".
    tappable: bool,
    /// Set the node-level flags.
    pass_through: bool,
    transparent: bool,
    node_no_slop: bool,
    node_slop: Option<HitSlop>,
    enabled: Option<bool>,
    clips: bool,
    /// An explicit measured size, for content the parent does not place (an
    /// overlay's root, which is sized by what it reports).
    size: Option<teksilo_canvas::Size>,
    /// Bumped whenever this cell's `on_tap` fires.
    taps: Option<Rc<StdCell<u32>>>,
    children: Vec<WidgetId>,
}

impl std::fmt::Debug for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cell").finish()
    }
}

impl Cell {
    fn new() -> Self {
        Self::default()
    }
    fn tappable(mut self) -> Self {
        self.tappable = true;
        self
    }
    fn outset(mut self, dp: f32) -> Self {
        self.outset = dp;
        self
    }
    fn outset_kinds(mut self, mask: PointerKindMask) -> Self {
        self.outset_kinds = Some(mask);
        self
    }
    fn round(mut self) -> Self {
        self.round = true;
        self
    }
    fn widget_slop(mut self, slop: HitSlop) -> Self {
        self.slop = Some(slop);
        self
    }
    fn node_slop(mut self, slop: HitSlop) -> Self {
        self.node_slop = Some(slop);
        self
    }
    fn node_no_slop(mut self) -> Self {
        self.node_no_slop = true;
        self
    }
    fn no_distance(mut self) -> Self {
        self.no_distance = true;
        self
    }
    fn pass_through(mut self) -> Self {
        self.pass_through = true;
        self
    }
    fn transparent(mut self) -> Self {
        self.transparent = true;
        self
    }
    fn disabled(mut self) -> Self {
        self.enabled = Some(false);
        self
    }
    fn clips(mut self) -> Self {
        self.clips = true;
        self
    }
    fn counting(mut self, counter: &Rc<StdCell<u32>>) -> Self {
        self.tappable = true;
        self.taps = Some(counter.clone());
        self
    }
    fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Some(teksilo_canvas::Size::new(width, height));
        self
    }
    fn child(mut self, id: WidgetId) -> Self {
        self.children.push(id);
        self
    }
}

impl Widget for Cell {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let mut set = HandlerSet::new();
        if self.tappable {
            let counter = self.taps.clone();
            set = set.on_tap(move |_e, _c| {
                if let Some(ref counter) = counter {
                    counter.set(counter.get() + 1);
                }
            });
        }
        if self.pass_through {
            set = set.event_pass_through(true);
        }
        if self.transparent {
            set = set.hit_transparent(true);
        }
        if self.node_no_slop {
            set = set.no_hit_slop();
        }
        if let Some(slop) = self.node_slop {
            set = set.hit_slop(slop);
        }
        if self.clips {
            set = set.clips_children(true);
        }
        ctx.apply_self_handlers(set);
        if let Some(enabled) = self.enabled {
            let id = ctx.self_id();
            ctx.enabled_when(id, enabled);
        }
        self.children.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.size {
            Some(size) => size.into(),
            None => proposal.resolve(0.0, 0.0).into(),
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }

    fn hit_outset(&self, kind: teksilo_tokens::PointerKind, _tokens: &InputTokens) -> EdgeInsets {
        // The default policy every grip follows: direct pointers only, unless
        // the widget deliberately opts every kind in.
        let mask = self.outset_kinds.unwrap_or(PointerKindMask::DIRECT);
        if self.outset > 0.0 && mask.contains(kind) {
            EdgeInsets::uniform(self.outset)
        } else {
            EdgeInsets::ZERO
        }
    }

    fn hit_slop(
        &self,
        _kind: teksilo_tokens::PointerKind,
        _tokens: &InputTokens,
    ) -> Option<HitSlop> {
        self.slop
    }

    fn hit_distance(&self, local: Point, bounds: Rect) -> Option<f32> {
        if self.no_distance {
            return None;
        }
        if self.round {
            let radius = bounds.width.min(bounds.height) / 2.0;
            return Some(circle_distance(bounds.center(), radius, local));
        }
        Some(rect_distance(bounds, local))
    }
}

/// A container that places each child at an absolute rectangle, so a fixture
/// states its geometry outright instead of deriving it from a stack.
#[derive(Debug)]
struct Board {
    placed: Vec<(WidgetId, Rect)>,
}

impl Board {
    fn new() -> Self {
        Self { placed: Vec::new() }
    }
    fn at(mut self, id: WidgetId, rect: Rect) -> Self {
        self.placed.push((id, rect));
        self
    }
}

impl Widget for Board {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for (child, (_, rect)) in children.iter_mut().zip(&self.placed) {
            child.origin = Point::new(bounds.x + rect.x, bounds.y + rect.y);
            child.size = rect.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.placed.iter().map(|(id, _)| *id).collect()
    }
}

fn touch() -> InputTokens {
    InputTokens::for_density(TargetDensity::Touch)
}

fn at(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn touch_pointer() -> crate::pointer::PointerInfo {
    crate::pointer::PointerInfo::touch(
        crate::pointer::PointerId::MOUSE,
        crate::pointer::EventTime::ZERO,
    )
}

fn mouse_pointer() -> crate::pointer::PointerInfo {
    crate::pointer::PointerInfo::mouse(crate::pointer::EventTime::ZERO)
}

// ---------------------------------------------------------------------------
// Mechanism 2 — `Widget::hit_outset`, inside the exact pass
// ---------------------------------------------------------------------------

/// The fixture A10 names: a 6 dp grip lying across a tappable row, with the row
/// painted **after** it so the ordinary reverse-sibling walk would hand every
/// press to the row. A press in the grip's outset ring must still reach the
/// grip.
fn grip_over_row(outset: f32) -> (WidgetTree, WidgetId, WidgetId) {
    let mut tree = WidgetTree::new();
    // Grip: 6 dp wide, full height, at x = 100.
    let grip = tree.add(Cell::new().tappable().outset(outset));
    // Row: the whole board, added AFTER so it is the topmost sibling.
    let row = tree.add(Cell::new().tappable());
    let board = tree.add(
        Board::new()
            .at(grip, Rect::new(100.0, 0.0, 6.0, 200.0))
            .at(row, Rect::new(0.0, 0.0, 300.0, 200.0)),
    );
    let _ = board;
    tree.layout(SizeProposal::exact(300.0, 200.0));
    (tree, grip, row)
}

#[test]
fn a_grip_outset_wins_over_the_row_it_overlaps() {
    let (tree, grip, row) = grip_over_row(9.0);
    let finger = touch_pointer();
    // 8 dp past the grip's trailing edge, deep inside the row: the ring wins.
    assert_eq!(tree.hit_test_for(at(114.0, 100.0), &finger), Some(grip));
    // 8 dp before its leading edge, likewise.
    assert_eq!(tree.hit_test_for(at(92.0, 100.0), &finger), Some(grip));
    // Beyond the ring the row takes the press, as it always did.
    assert_eq!(tree.hit_test_for(at(140.0, 100.0), &finger), Some(row));
    // Inside the grip itself, unchanged.
    assert_eq!(tree.hit_test_for(at(103.0, 100.0), &finger), Some(grip));
}

/// Same rule against a *pane* rather than a row: the grip is declared first and
/// the panes fill the board around it, so without the pre-pass the pane on top
/// would swallow the ring.
#[test]
fn a_grip_outset_wins_over_the_pane_it_overlaps() {
    let mut tree = WidgetTree::new();
    let grip = tree.add(Cell::new().tappable().outset(9.0));
    let left = tree.add(Cell::new().tappable());
    let right = tree.add(Cell::new().tappable());
    tree.add(
        Board::new()
            .at(grip, Rect::new(147.0, 0.0, 6.0, 200.0))
            .at(left, Rect::new(0.0, 0.0, 147.0, 200.0))
            .at(right, Rect::new(153.0, 0.0, 147.0, 200.0)),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));
    let finger = touch_pointer();
    assert_eq!(tree.hit_test_for(at(140.0, 100.0), &finger), Some(grip));
    assert_eq!(tree.hit_test_for(at(160.0, 100.0), &finger), Some(grip));
    assert_eq!(tree.hit_test_for(at(100.0, 100.0), &finger), Some(left));
    assert_eq!(tree.hit_test_for(at(200.0, 100.0), &finger), Some(right));
}

/// Two grips 20 dp apart, each with a 9 dp ring: the rings overlap in the
/// middle, and the nearer grip wins rather than whichever happens to be the
/// later sibling.
#[test]
fn a_grip_outset_wins_over_an_adjacent_grip_by_being_nearer() {
    let mut tree = WidgetTree::new();
    let a = tree.add(Cell::new().tappable().outset(9.0));
    let b = tree.add(Cell::new().tappable().outset(9.0));
    let row = tree.add(Cell::new().tappable());
    tree.add(
        Board::new()
            .at(a, Rect::new(100.0, 0.0, 6.0, 200.0))
            .at(b, Rect::new(126.0, 0.0, 6.0, 200.0))
            .at(row, Rect::new(0.0, 0.0, 300.0, 200.0)),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));
    let finger = touch_pointer();
    // Gap runs 106..126. Nearer A:
    assert_eq!(tree.hit_test_for(at(110.0, 100.0), &finger), Some(a));
    // Nearer B:
    assert_eq!(tree.hit_test_for(at(122.0, 100.0), &finger), Some(b));
    // The midpoint (116) is 10 dp from each — outside both 9 dp rings, so the
    // row underneath keeps it.
    assert_eq!(tree.hit_test_for(at(116.0, 100.0), &finger), Some(row));
}

/// The outset is offered to direct pointers only unless the widget says
/// otherwise — a mouse hot-spot is exact and widening it steals clicks.
#[test]
fn an_outset_is_zero_for_a_precise_pointer_unless_the_widget_opts_in() {
    let (tree, _grip, row) = grip_over_row(9.0);
    let mouse = mouse_pointer();
    assert_eq!(tree.hit_test_for(at(114.0, 100.0), &mouse), Some(row));
    assert_eq!(tree.hit_test(at(114.0, 100.0)), Some(row));

    // The rich-text image grip's case: a control with a genuinely undersized
    // MOUSE target opts every kind in explicitly.
    let mut tree = WidgetTree::new();
    let grip = tree.add(
        Cell::new()
            .tappable()
            .outset(5.0)
            .outset_kinds(PointerKindMask::ALL),
    );
    let row = tree.add(Cell::new().tappable());
    tree.add(
        Board::new()
            .at(grip, Rect::new(100.0, 0.0, 6.0, 200.0))
            .at(row, Rect::new(0.0, 0.0, 300.0, 200.0)),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert_eq!(tree.hit_test(at(109.0, 100.0)), Some(grip));
    assert_eq!(tree.hit_test(at(112.0, 100.0)), Some(row));
}

/// The ring may only claim space the parent already owns. A grip flush against
/// its parent's edge cannot reach the sibling beyond it.
#[test]
fn an_outset_never_escapes_the_parent() {
    let mut tree = WidgetTree::new();
    let grip = tree.add(Cell::new().tappable().outset(20.0));
    let inner = tree.add(Board::new().at(grip, Rect::new(0.0, 0.0, 6.0, 200.0)));
    let outside = tree.add(Cell::new().tappable());
    tree.add(
        Board::new()
            .at(outside, Rect::new(100.0, 0.0, 200.0, 200.0))
            .at(inner, Rect::new(0.0, 0.0, 100.0, 200.0)),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));
    let finger = touch_pointer();
    // 10 dp into the grip's ring but still inside the parent: the grip.
    assert_eq!(tree.hit_test_for(at(16.0, 100.0), &finger), Some(grip));
    // Past the parent's own right edge the ring has no reach at all, even
    // though 20 dp would otherwise carry it there.
    assert_eq!(tree.hit_test_for(at(105.0, 100.0), &finger), Some(outside));
}

/// Hit-only: declaring an outset moves nothing. This is the invariant that
/// makes the whole mechanism safe to adopt widget by widget.
#[test]
fn an_outset_moves_no_bounds() {
    let (plain, plain_grip, plain_row) = grip_over_row(0.0);
    let (outset, outset_grip, outset_row) = grip_over_row(9.0);
    assert_eq!(plain.bounds(plain_grip), outset.bounds(outset_grip));
    assert_eq!(plain.bounds(plain_row), outset.bounds(outset_row));
}

/// `no_hit_slop` is the head of ONE chain covering both widening mechanisms, so
/// it silences a widget's `hit_outset` too — the node behaves at every point
/// exactly as if it had never declared one.
#[test]
fn no_hit_slop_silences_the_outset_as_well() {
    fn build(declared: bool, silenced: bool) -> (WidgetTree, WidgetId, WidgetId) {
        let mut tree = WidgetTree::new();
        let mut cell = Cell::new().tappable();
        if declared {
            cell = cell.outset(9.0);
        }
        if silenced {
            cell = cell.node_no_slop();
        }
        let grip = tree.add(cell);
        let row = tree.add(Cell::new().tappable());
        tree.add(
            Board::new()
                .at(grip, Rect::new(100.0, 0.0, 6.0, 200.0))
                .at(row, Rect::new(0.0, 0.0, 300.0, 200.0)),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        (tree, grip, row)
    }
    fn label(hit: Option<WidgetId>, grip: WidgetId, row: WidgetId) -> &'static str {
        match hit {
            Some(id) if id == grip => "grip",
            Some(id) if id == row => "row",
            Some(_) => "other",
            None => "none",
        }
    }
    let (bare, bare_grip, bare_row) = build(false, false);
    let (loud, loud_grip, loud_row) = build(true, false);
    let (quiet, quiet_grip, quiet_row) = build(true, true);
    let finger = touch_pointer();
    let mut differed = false;
    for x in 90..120 {
        let p = at(x as f32 + 0.5, 100.0);
        let undeclared = label(bare.hit_test_for(p, &finger), bare_grip, bare_row);
        let declared = label(loud.hit_test_for(p, &finger), loud_grip, loud_row);
        let silenced = label(quiet.hit_test_for(p, &finger), quiet_grip, quiet_row);
        assert_eq!(
            undeclared, silenced,
            "no_hit_slop must reproduce the undeclared behaviour at {p:?}"
        );
        differed |= undeclared != declared;
    }
    assert!(
        differed,
        "the fixture must actually exercise the outset somewhere, or it proves nothing"
    );
}

/// A pass-through node absorbs nothing, so widening it would punch a hole in
/// whatever is behind it rather than widening a target.
#[test]
fn a_pass_through_node_gets_no_outset() {
    let mut tree = WidgetTree::new();
    let ghost = tree.add(Cell::new().outset(9.0).pass_through());
    let row = tree.add(Cell::new().tappable());
    tree.add(
        Board::new()
            .at(row, Rect::new(0.0, 0.0, 300.0, 200.0))
            .at(ghost, Rect::new(100.0, 0.0, 6.0, 200.0)),
    );
    tree.layout(SizeProposal::exact(300.0, 200.0));
    assert_eq!(
        tree.hit_test_for(at(112.0, 100.0), &touch_pointer()),
        Some(row)
    );
}

// ---------------------------------------------------------------------------
// Mechanism 3 — the miss-only slop pass
// ---------------------------------------------------------------------------

/// An isolated 16 dp control on an inert background, at Touch density: a press
/// 5 dp past its edge is re-attributed to it.
fn isolated_dot(cell: Cell) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let dot = tree.add(cell);
    // The backdrop takes no press, so the bubble path carries no owner.
    let back = tree.add(Cell::new());
    tree.add(
        Board::new()
            .at(back, Rect::new(0.0, 0.0, 200.0, 200.0))
            .at(dot, Rect::new(92.0, 92.0, 16.0, 16.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    (tree, dot)
}

#[test]
fn an_isolated_small_target_catches_a_near_miss() {
    let (tree, dot) = isolated_dot(Cell::new().tappable());
    let finger = touch_pointer();
    // Touch radius 8 dp, `up_to` 44 dp, node 16 dp → outset
    // ((44 − 16) / 2).clamp(0, 8) = 8.
    assert_eq!(tree.hit_test_for(at(113.0, 100.0), &finger), Some(dot));
    // 9 dp out is past the outset.
    assert_ne!(tree.hit_test_for(at(117.0, 100.0), &finger), Some(dot));
    // The exact hit is untouched.
    assert_eq!(tree.hit_test_for(at(100.0, 100.0), &finger), Some(dot));
}

/// The same press with a mouse is never re-attributed. The mouse profile's
/// radius is `0.0`, which is what makes every historical hit test byte-identical.
#[test]
fn a_mouse_press_is_never_re_attributed() {
    let (tree, dot) = isolated_dot(Cell::new().tappable());
    let mouse = mouse_pointer();
    assert_ne!(tree.hit_test_for(at(113.0, 100.0), &mouse), Some(dot));
    assert_eq!(
        tree.hit_test(at(113.0, 100.0)),
        tree.hit_test_for(at(113.0, 100.0), &mouse)
    );
}

/// **The named case.** A row that takes presses, with an inline checkbox in it:
/// a press on the row's label 5 dp from the checkbox stays on the row. The row
/// owns the press at distance zero, and nothing beats zero.
#[test]
fn a_row_label_five_dp_from_an_inline_checkbox_still_bubbles_to_the_row() {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let row_taps = Rc::new(StdCell::new(0));
    let box_taps = Rc::new(StdCell::new(0));
    // The checkbox is a child of the row, 16 dp square at x = 8.
    let checkbox = tree.add(Cell::new().counting(&box_taps));
    let label = tree.add(Cell::new());
    let row = tree.add(Cell::new().counting(&row_taps).child(checkbox).child(label));
    tree.add(Board::new().at(row, Rect::new(0.0, 0.0, 300.0, 48.0)));
    // Place the row's own children by hand through a Board nested inside it is
    // unnecessary: the row is a Cell, whose children fill it. Use a Board as
    // the row instead so the checkbox lands where the fixture says.
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let checkbox = tree.add(Cell::new().counting(&box_taps));
    let label = tree.add(Cell::new());
    let inner = tree.add(
        Board::new()
            .at(checkbox, Rect::new(8.0, 16.0, 16.0, 16.0))
            .at(label, Rect::new(29.0, 0.0, 271.0, 48.0)),
    );
    let row = tree.add(Cell::new().counting(&row_taps).child(inner));
    tree.add(Board::new().at(row, Rect::new(0.0, 0.0, 300.0, 48.0)));
    tree.layout(SizeProposal::exact(300.0, 48.0));

    let finger = touch_pointer();
    // x = 29 is the label's leading edge — exactly 5 dp from the checkbox's
    // trailing edge at x = 24. The label is inert, so the press bubbles to the
    // row, and the checkbox (5 dp away, inside its 8 dp outset) must NOT
    // steal it.
    let hit = tree.hit_test_for(at(29.0, 24.0), &finger);
    assert_eq!(hit, Some(label), "the exact pass still lands on the label");
    // And the press it produces belongs to the row.
    let owner = std::iter::successors(hit, |id| tree.arena.get(*id).and_then(|n| n.parent))
        .find(|id| tree.arena.takes_a_press(*id));
    assert_eq!(
        owner,
        Some(row),
        "the bubble owner is the row, not the checkbox"
    );
    assert_ne!(hit, Some(checkbox));
}

/// The complement: with **no** eligible handler anywhere on the bubble path,
/// the same near miss does reach the small control.
#[test]
fn with_no_owner_on_the_path_the_near_miss_lands_on_the_control() {
    let box_taps = Rc::new(StdCell::new(0));
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let checkbox = tree.add(Cell::new().counting(&box_taps));
    let label = tree.add(Cell::new());
    // The row is inert this time.
    let row = tree.add(
        Board::new()
            .at(checkbox, Rect::new(8.0, 16.0, 16.0, 16.0))
            .at(label, Rect::new(29.0, 0.0, 271.0, 48.0)),
    );
    tree.add(Board::new().at(row, Rect::new(0.0, 0.0, 300.0, 48.0)));
    tree.layout(SizeProposal::exact(300.0, 48.0));
    assert_eq!(
        tree.hit_test_for(at(29.0, 24.0), &touch_pointer()),
        Some(checkbox)
    );
}

/// A disabled control is never a candidate — nor is one under a disabled
/// ancestor.
#[test]
fn a_disabled_control_is_never_a_candidate() {
    let (tree, dot) = isolated_dot(Cell::new().tappable().disabled());
    assert_ne!(
        tree.hit_test_for(at(113.0, 100.0), &touch_pointer()),
        Some(dot)
    );
}

/// A read-only surface is never a candidate. "Read-only" is a question only the
/// `TextSurface` registry can answer, so the tree hands the arena a probe.
#[test]
fn a_read_only_surface_is_never_a_candidate() {
    let (tree, dot) = isolated_dot(Cell::new().tappable());
    let t = touch();
    let point = at(113.0, 100.0);
    let open = HitContext::new(PointerKind::Touch, &t);
    assert_eq!(
        tree.arena.hit_test_at_with_slop(point, None, &open),
        Some(dot)
    );
    let refuses = |id: WidgetId| id == dot;
    let closed = HitContext::new(PointerKind::Touch, &t).read_only_probe(&refuses);
    assert_ne!(
        tree.arena.hit_test_at_with_slop(point, None, &closed),
        Some(dot)
    );
}

/// `hit_transparent` prunes the whole subtree, so neither the decorative
/// wrapper nor the control inside it can be re-attributed to.
#[test]
fn a_hit_transparent_subtree_is_pruned_from_the_pass() {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let inner = tree.add(Cell::new().tappable());
    let badge = tree.add(Cell::new().transparent().child(inner));
    let back = tree.add(Cell::new());
    tree.add(
        Board::new()
            .at(back, Rect::new(0.0, 0.0, 200.0, 200.0))
            .at(badge, Rect::new(92.0, 92.0, 16.0, 16.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let hit = tree.hit_test_for(at(113.0, 100.0), &touch_pointer());
    assert_ne!(hit, Some(inner));
    assert_ne!(hit, Some(badge));
}

/// An `event_pass_through` node absorbs nothing and so is not a candidate — but
/// its children remain eligible, exactly as they remain hit-testable.
#[test]
fn event_pass_through_is_not_a_candidate_but_its_children_are() {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let inner = tree.add(Cell::new().tappable());
    let ghost = tree.add(Cell::new().tappable().pass_through().child(inner));
    let back = tree.add(Cell::new());
    tree.add(
        Board::new()
            .at(back, Rect::new(0.0, 0.0, 200.0, 200.0))
            .at(ghost, Rect::new(92.0, 92.0, 16.0, 16.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let t = touch();
    let hit = HitContext::new(PointerKind::Touch, &t);
    let ids: Vec<WidgetId> = tree
        .arena
        .hit_candidates(tree.arena.roots()[0], at(113.0, 100.0), None, &hit)
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert!(
        ids.contains(&inner),
        "the child of a pass-through node stays eligible"
    );
    assert!(
        !ids.contains(&ghost),
        "the pass-through node itself does not"
    );
}

/// Slop never escapes a clipping ancestor's **uninflated** rectangle: a control
/// scrolled just out of view must not catch a press on the scroller's border.
#[test]
fn slop_never_escapes_a_clips_children_ancestor() {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let dot = tree.add(Cell::new().tappable());
    // The dot sits at the very trailing edge inside the clipper.
    let inner = tree.add(Board::new().at(dot, Rect::new(84.0, 92.0, 16.0, 16.0)));
    let clipper = tree.add(Cell::new().clips().child(inner));
    let back = tree.add(Cell::new());
    tree.add(
        Board::new()
            .at(back, Rect::new(0.0, 0.0, 200.0, 200.0))
            .at(clipper, Rect::new(0.0, 0.0, 100.0, 200.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    let finger = touch_pointer();
    // Inside the clipper, 4 dp from the dot: re-attributed.
    assert_eq!(tree.hit_test_for(at(96.0, 100.0), &finger), Some(dot));
    // 4 dp PAST the clipper's edge and 4 dp from the dot — the clip stops it.
    assert_ne!(tree.hit_test_for(at(104.0, 100.0), &finger), Some(dot));
}

/// Candidates are restricted to the topmost overlay layer the exact pass
/// entered: a press inside an open menu can never reach the page behind it,
/// however tempting the geometry underneath.
#[test]
fn candidates_are_restricted_to_the_topmost_overlay_layer() {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    // A small tappable control on the page, exactly the kind the slop pass
    // would otherwise reach for.
    let page_dot = tree.add(Cell::new().tappable());
    let anchor = tree.add(Cell::new());
    tree.add(
        Board::new()
            .at(page_dot, Rect::new(40.0, 100.0, 16.0, 16.0))
            .at(anchor, Rect::new(60.0, 60.0, 16.0, 16.0)),
    );
    // The overlay's content is inert and takes no press.
    let panel = tree.add(Cell::new().size(120.0, 120.0));
    // Shown BEFORE the first layout: `layout` early-returns on a clean tree
    // with an unchanged proposal, so an overlay pushed between two identical
    // layout calls would never be positioned.
    tree.show_overlay(crate::overlay::OverlayRequest {
        content_id: panel,
        anchor,
        placement: crate::overlay::OverlayPlacement::Below,
        dismiss: crate::overlay::DismissBehavior::Manual,
        layer: crate::overlay::OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
        fade_duration: None,
    });
    tree.layout(SizeProposal::exact(200.0, 200.0));

    let panel_bounds = tree.bounds(panel);
    assert!(
        panel_bounds.width > 0.0 && panel_bounds.height > 0.0,
        "the fixture needs a real overlay rectangle, got {panel_bounds:?}"
    );
    let dot_bounds = tree.bounds(page_dot);
    // Pick a point inside the overlay and within slop reach of the page dot —
    // the ONLY configuration in which the restriction is observable.
    let probe = at(dot_bounds.right() + 4.5, dot_bounds.center().y);
    assert!(
        panel_bounds.contains(probe),
        "the probe {probe:?} must lie inside the overlay {panel_bounds:?}"
    );
    let finger = touch_pointer();
    let hit = tree.hit_test_for(probe, &finger);
    assert_ne!(
        hit,
        Some(page_dot),
        "the page behind the overlay is out of reach"
    );
    assert_eq!(hit, Some(panel));

    // Prove the restriction is what did it: with the overlay gone, the same
    // press is re-attributed to the dot.
    let overlay_id = tree.active_overlays()[0];
    tree.dismiss_overlay(overlay_id);
    tree.layout(SizeProposal::exact(200.0, 200.0));
    assert_eq!(tree.hit_test_for(probe, &finger), Some(page_dot));
}

/// A transform inverse-maps the point and converts the local distance through
/// the minimum singular value, so the reach the token promises is the reach the
/// user gets **on screen**.
#[test]
fn a_transform_inverse_maps_the_point_and_scales_the_radius() {
    fn tree_with_scale(scale: f32) -> (WidgetTree, WidgetId) {
        let mut tree = WidgetTree::new();
        tree.set_input_density(TargetDensity::Touch);
        let dot = tree.add(Cell::new().tappable());
        let scaled = tree.add(Board::new().at(dot, Rect::new(0.0, 0.0, 16.0, 16.0)));
        tree.set_transform(scaled, teksilo_canvas::Transform2D::scale(scale, scale));
        let back = tree.add(Cell::new());
        tree.add(
            Board::new()
                .at(back, Rect::new(0.0, 0.0, 200.0, 200.0))
                .at(scaled, Rect::new(0.0, 0.0, 16.0, 16.0)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        (tree, dot)
    }
    let finger = touch_pointer();
    // Unscaled: the 16 dp dot spans 0..16 and earns an 8 dp outset.
    let (tree, dot) = tree_with_scale(1.0);
    assert_eq!(tree.hit_test_for(at(23.0, 8.0), &finger), Some(dot));
    assert_ne!(tree.hit_test_for(at(25.0, 8.0), &finger), Some(dot));

    // Scaled 2×: the dot covers 0..32 on screen and the point is inverse-mapped
    // into its local space. The reach stays 8 dp ON SCREEN — a local distance
    // of 4 is a screen distance of 8 — so x = 39 is in and x = 41 is out.
    let (tree, dot) = tree_with_scale(2.0);
    assert_eq!(tree.hit_test_for(at(35.0, 16.0), &finger), Some(dot));
    assert_eq!(tree.hit_test_for(at(39.0, 16.0), &finger), Some(dot));
    assert_ne!(tree.hit_test_for(at(41.0, 16.0), &finger), Some(dot));
}

/// A round control reports its own silhouette, so a press past the corner of
/// its box loses where a press past its edge wins — the point of implementing
/// `hit_distance` beside `hit_shape`.
#[test]
fn a_round_control_measures_to_its_disc() {
    let (round, dot_round) = isolated_dot(Cell::new().tappable().round());
    let (square, dot_square) = isolated_dot(Cell::new().tappable());
    let finger = touch_pointer();
    // Straight out from the edge: 5 dp for both.
    assert_eq!(
        round.hit_test_for(at(113.0, 100.0), &finger),
        Some(dot_round)
    );
    assert_eq!(
        square.hit_test_for(at(113.0, 100.0), &finger),
        Some(dot_square)
    );
    // Diagonally past the corner: 7.07 dp from the box, ~12 dp from the disc.
    let corner = at(113.0, 113.0);
    assert_eq!(square.hit_test_for(corner, &finger), Some(dot_square));
    assert_ne!(round.hit_test_for(corner, &finger), Some(dot_round));
}

/// A widget may withdraw from the pass at the shape level by answering `None`.
#[test]
fn a_widget_can_withdraw_by_reporting_no_distance() {
    let (tree, dot) = isolated_dot(Cell::new().tappable().no_distance());
    assert_ne!(
        tree.hit_test_for(at(113.0, 100.0), &touch_pointer()),
        Some(dot)
    );
}

/// A candidate must still be something a press would reach. Re-attributing to a
/// node that ignores presses would swallow one silently.
#[test]
fn an_inert_node_is_never_a_candidate() {
    let (tree, dot) = isolated_dot(Cell::new());
    assert_ne!(
        tree.hit_test_for(at(113.0, 100.0), &touch_pointer()),
        Some(dot)
    );
}

/// Nearest wins, not first found.
#[test]
fn the_nearest_candidate_wins() {
    let mut tree = WidgetTree::new();
    tree.set_input_density(TargetDensity::Touch);
    let near = tree.add(Cell::new().tappable());
    let far = tree.add(Cell::new().tappable());
    let back = tree.add(Cell::new());
    tree.add(
        Board::new()
            .at(back, Rect::new(0.0, 0.0, 200.0, 200.0))
            // `far` is the later sibling — topmost — and still loses.
            .at(near, Rect::new(84.0, 92.0, 16.0, 16.0))
            .at(far, Rect::new(106.0, 92.0, 16.0, 16.0)),
    );
    tree.layout(SizeProposal::exact(200.0, 200.0));
    // x = 103: 3 dp from `near` (ends at 100), 3 dp from `far` (starts at 106).
    // Move one dp toward each and the nearer takes it.
    assert_eq!(
        tree.hit_test_for(at(102.0, 100.0), &touch_pointer()),
        Some(near)
    );
    assert_eq!(
        tree.hit_test_for(at(104.0, 100.0), &touch_pointer()),
        Some(far)
    );
}

// ---------------------------------------------------------------------------
// The precedence chain
// ---------------------------------------------------------------------------

/// `no_hit_slop` > node `.hit_slop(..)` > `Widget::hit_slop` > density default,
/// all four levels in one test.
#[test]
fn the_precedence_chain_resolves_in_order() {
    let point = at(113.0, 100.0);
    let finger = touch_pointer();

    // 4. Density default: Touch gives 8 dp of reach to a 16 dp control.
    let (tree, dot) = isolated_dot(Cell::new().tappable());
    assert_eq!(tree.hit_test_for(point, &finger), Some(dot));

    // 3. The widget's own say beats the density: a 2 dp radius cannot reach.
    let (tree, dot) = isolated_dot(Cell::new().tappable().widget_slop(HitSlop {
        radius: 2.0,
        up_to: 44.0,
    }));
    assert_ne!(tree.hit_test_for(point, &finger), Some(dot));

    // 2. The node's `.hit_slop(..)` beats the widget's: back to 8 dp.
    let (tree, dot) = isolated_dot(
        Cell::new()
            .tappable()
            .widget_slop(HitSlop {
                radius: 2.0,
                up_to: 44.0,
            })
            .node_slop(HitSlop {
                radius: 8.0,
                up_to: 44.0,
            }),
    );
    assert_eq!(tree.hit_test_for(point, &finger), Some(dot));

    // 1. `no_hit_slop` beats everything below it.
    let (tree, dot) = isolated_dot(
        Cell::new()
            .tappable()
            .widget_slop(HitSlop {
                radius: 8.0,
                up_to: 44.0,
            })
            .node_slop(HitSlop {
                radius: 8.0,
                up_to: 44.0,
            })
            .node_no_slop(),
    );
    assert_ne!(tree.hit_test_for(point, &finger), Some(dot));
}

// ---------------------------------------------------------------------------
// The two invariants the whole package rests on
// ---------------------------------------------------------------------------

/// Declaring either hook changes **nothing** for a mouse — the exact pass sees
/// zero insets and the slop pass short-circuits on a zero radius.
#[test]
fn mouse_hit_tests_are_unchanged_by_either_declaration() {
    // Two identical fixtures except that one declares an outset and a slop.
    // Labelling the result rather than comparing raw ids keeps the assertion
    // honest across two independent arenas.
    fn build(declare: bool) -> (WidgetTree, WidgetId, WidgetId) {
        let mut tree = WidgetTree::new();
        let grip = if declare {
            Cell::new().tappable().outset(9.0).node_slop(HitSlop {
                radius: 16.0,
                up_to: 64.0,
            })
        } else {
            Cell::new().tappable()
        };
        let a = tree.add(grip);
        let b = tree.add(Cell::new().tappable());
        tree.add(
            Board::new()
                .at(a, Rect::new(100.0, 0.0, 6.0, 200.0))
                .at(b, Rect::new(0.0, 0.0, 300.0, 200.0)),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        (tree, a, b)
    }
    fn label(hit: Option<WidgetId>, a: WidgetId, b: WidgetId) -> &'static str {
        match hit {
            Some(id) if id == a => "grip",
            Some(id) if id == b => "row",
            Some(_) => "other",
            None => "none",
        }
    }
    let (bare, bare_a, bare_b) = build(false);
    let (rich, rich_a, rich_b) = build(true);
    let mouse = mouse_pointer();
    for x in 0..300 {
        for y in [0, 47, 100, 199] {
            let p = at(x as f32 + 0.5, y as f32 + 0.5);
            let plain = label(bare.hit_test(p), bare_a, bare_b);
            let declared = label(rich.hit_test(p), rich_a, rich_b);
            assert_eq!(plain, declared, "mouse hit differs at {p:?}");
            let via_pointer = label(rich.hit_test_for(p, &mouse), rich_a, rich_b);
            assert_eq!(
                declared, via_pointer,
                "hit_test_for(mouse) differs from hit_test at {p:?}"
            );
        }
    }
}

/// Compact layout is unchanged by either declaration: hit targeting never
/// touches geometry.
#[test]
fn compact_layout_is_unchanged_by_either_declaration() {
    let build = |declare: bool| {
        let mut tree = WidgetTree::new();
        let cell = if declare {
            Cell::new().tappable().outset(9.0).node_slop(HitSlop {
                radius: 16.0,
                up_to: 64.0,
            })
        } else {
            Cell::new().tappable()
        };
        let a = tree.add(cell);
        let b = tree.add(Cell::new().tappable());
        let board = tree.add(
            Board::new()
                .at(a, Rect::new(100.0, 0.0, 6.0, 200.0))
                .at(b, Rect::new(0.0, 0.0, 300.0, 200.0)),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        (tree.bounds(a), tree.bounds(b), tree.bounds(board))
    };
    assert_eq!(build(false), build(true));
    // …and the fixture really is at the density CI runs at.
    assert_eq!(
        InputTokens::default().density,
        TargetDensity::Compact,
        "the default ladder must stay Compact for this to mean anything"
    );
}
