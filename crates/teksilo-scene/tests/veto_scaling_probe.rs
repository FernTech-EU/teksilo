// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What does one **hit test** cost when a press claimant is painted over the
//! cards, and does that cost follow the number of cards?
//!
//! The arena asks `Widget::accepts_child_hit` once per hit-tested child. A
//! `SceneView` answers by asking what its own lightweight picker has at that
//! point, which is a walk of the handler snapshot — so the answer is memoised
//! and one pointer sample is meant to cost **one** walk however many cards the
//! pointer is over.
//!
//! That is a claim about cost alone: the verdicts are identical whether the
//! memo hits or misses, so no behavioural test in this crate can see the
//! difference. It is invisible in a profile too until the scene is large. So
//! the scan count is published by the view (`SceneView::veto_snapshot_scans`)
//! and gated here.
//!
//! The regression this exists to stop has already happened once: the memo's key
//! grew a field — the asking child's own `PaintKey` — that differs on every
//! call, so it missed on every child and a 100-card hit test went from one scan
//! to a hundred (2.2x wall clock). It still looked exactly like a working memo.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test veto_scaling_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_scene::{RectItem, Scene, SceneItemHandlerSet, SceneLayer, SceneView};
use teksilo_tokens::Color;

/// The square every card and the claimant occupy, so every card is under the
/// pointer and every one of them has to be asked about.
const AREA: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 200.0,
    height: 200.0,
};

const SAMPLES: u32 = 2000;

/// A heavyweight card: a widget that paints one solid colour.
#[derive(Debug)]
struct Card;

impl Widget for Card {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(AREA.width, AREA.height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        canvas.fill_rect(bounds, Color::new(0.5, 0.5, 0.5, 1.0));
    }
}

/// `cards` fully overlapping heavyweight cards, optionally with one lightweight
/// press claimant in the [`Over`](SceneLayer::Over) band covering all of them.
///
/// `Over` is the band that is above *every* card, so with a claimant the veto
/// has to reject every one of them — the worst case, and the shape of an ink
/// page whose strokes take the press. Without one, the view's
/// `over_claimants` flag is false and the veto is meant to cost a `bool` read
/// per child and nothing else.
fn tree_with(cards: usize, claimant: bool) -> (WidgetTree, teksilo_core::WidgetId) {
    let mut scene = Scene::new();
    for i in 0..cards {
        let id = scene.add_widget(Card, AREA);
        scene.set_z(id, i as f32);
    }
    if claimant {
        let item = scene.add_item(RectItem::new(AREA), Point::ZERO);
        scene.set_layer(item, SceneLayer::Over);
        let mut handlers = SceneItemHandlerSet::new();
        handlers.on_tap(|_, _| {});
        scene.set_item_handlers(item, Some(handlers));
    }

    let mut tree = WidgetTree::new();
    let view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, view)
}

fn scans(tree: &WidgetTree, view: teksilo_core::WidgetId) -> u64 {
    tree.widget_as_any(view)
        .and_then(|w| w.downcast_ref::<SceneView>())
        .expect("the root is a SceneView")
        .veto_snapshot_scans()
}

/// A moving pointer: every sample is a different point, so the memo is asked a
/// fresh question each time — which is what a pointer actually does, and the
/// only way a per-hit-test cost is observable at all.
fn sample_point(i: u32) -> Point {
    Point::new(90.0 + (i % 17) as f32 * 0.5, 90.0 + (i % 13) as f32 * 0.5)
}

/// One hit test scans the snapshot **once**, whatever the card count.
///
/// What reddens it: routing the asking child's own `PaintKey` into the
/// *memoised* query — `accepts_child_hit` calling the scan with
/// `self.child_paint_key(child)` as its floor, and `veto_memo` keyed by
/// `(generation, transform, point, floor)`. The floor differs per child, so the
/// memo misses on every one and the 100-card row reports 100 scans instead of 1.
///
/// Two things have to stay true for one scan, and this checks the conjunction:
/// the memoised question is floor-free, **and** `accepts_child_hit` asks it
/// without a per-child argument. Breaking either alone is not enough to redden
/// this — the other still forces one question per hit walk — which is the
/// belt-and-braces the original defect earned.
#[test]
fn one_hit_test_costs_one_snapshot_scan_at_any_card_count() {
    for cards in [1usize, 10, 100, 400] {
        let (tree, view) = tree_with(cards, true);
        let before = scans(&tree, view);
        let hit = tree.hit_test(sample_point(0));
        let spent = scans(&tree, view) - before;

        assert_eq!(
            hit,
            Some(view),
            "{cards} cards: the Over claimant vetoes every one of them, so the \
             press belongs to the view's own lightweight dispatch"
        );
        assert_eq!(
            spent, 1,
            "{cards} cards: one hit test must walk the handler snapshot once, \
             not once per child (walked {spent} times)"
        );
    }
}

/// The same claim across a stream of moving samples: the count follows the
/// number of samples, not the product of samples and cards.
#[test]
fn a_stream_of_samples_costs_one_scan_each() {
    let (tree, view) = tree_with(200, true);
    let before = scans(&tree, view);
    for i in 0..SAMPLES {
        tree.hit_test(sample_point(i));
    }
    let spent = scans(&tree, view) - before;
    // Distinct points, so each is a fresh question; a repeat hits the memo only
    // when it is consecutive, which the cycle above never makes it.
    assert!(
        spent <= u64::from(SAMPLES),
        "{SAMPLES} samples over 200 cards must cost at most one scan each, \
         spent {spent}"
    );
}

/// The table. Not a gate — read it when a hit test feels slow.
///
/// Two columns per scene, because a memo is worth what the repetition of the
/// question is worth. **moving** is a real pointer: every sample is a new
/// point, so the memo answers each sample once and the row is what dragging
/// over the scene costs. **still** is the same point over and over — a hover
/// that has settled — where the memo carries the answer across whole hit tests
/// and the scan count falls to zero.
///
/// The second table is the scene with **no** press claimant at all, which is
/// most scenes. Nothing there can ever veto a card, so the whole feature is
/// meant to cost one `bool` read per hit-tested child — and that is only true
/// while `accepts_child_hit` declines *before* looking the child's `PaintKey`
/// up.
#[test]
fn hit_test_cost_table() {
    for (title, claimant) in [
        (
            "one Over press claimant over N fully overlapping cards",
            true,
        ),
        ("no press claimant, N fully overlapping cards", false),
    ] {
        println!();
        println!("hit test, {title}");
        println!(
            "{:>8}  {:>12} {:>11}  {:>12} {:>11}",
            "cards", "moving", "scans/hit", "still", "scans/hit"
        );
        for cards in [0usize, 1, 10, 50, 100, 200, 400] {
            let (tree, view) = tree_with(cards, claimant);
            // Warm the layout / snapshot paths before timing.
            for i in 0..32 {
                tree.hit_test(sample_point(i));
            }

            let before = scans(&tree, view);
            let t = Instant::now();
            for i in 0..SAMPLES {
                std::hint::black_box(tree.hit_test(sample_point(i)));
            }
            let moving = t.elapsed().as_secs_f64() / f64::from(SAMPLES) * 1e6;
            let moving_scans = (scans(&tree, view) - before) as f64 / f64::from(SAMPLES);

            let at = sample_point(0);
            tree.hit_test(at);
            let before = scans(&tree, view);
            let t = Instant::now();
            for _ in 0..SAMPLES {
                std::hint::black_box(tree.hit_test(at));
            }
            let still = t.elapsed().as_secs_f64() / f64::from(SAMPLES) * 1e6;
            let still_scans = (scans(&tree, view) - before) as f64 / f64::from(SAMPLES);

            println!(
                "{cards:>8}  {moving:>9.2} us {moving_scans:>11.2}  {still:>9.2} us {still_scans:>11.2}"
            );
        }
    }
}

/// A scene with no press claimant never walks the snapshot at all, whatever the
/// card count — the `over_claimants` short-circuit, pinned.
#[test]
fn a_scene_with_no_claimant_never_scans() {
    let (tree, view) = tree_with(200, false);
    let before = scans(&tree, view);
    for i in 0..SAMPLES {
        tree.hit_test(sample_point(i));
    }
    assert_eq!(
        scans(&tree, view) - before,
        0,
        "nothing in this scene can veto a card, so the veto must not look at \
         the snapshot once"
    );
}

fn key_probes(tree: &WidgetTree, view: teksilo_core::WidgetId) -> u64 {
    tree.widget_as_any(view)
        .and_then(|w| w.downcast_ref::<SceneView>())
        .expect("the root is a SceneView")
        .veto_key_probes()
}

/// The **other** half of the veto's cost: with no press claimant under the
/// pointer, the veto must not look a single child's `PaintKey` up.
///
/// The two halves fail independently, which is why this is a separate test and
/// a separate counter. Breaking the memo's key makes the *scan* count follow
/// the card count; evaluating the child's key before the claimant query can
/// decline it makes *this* count follow it. Each is invisible to the other's
/// assertion, and neither changes a verdict — so a 200-card scene looked
/// exactly like a working veto while paying 200 `RefCell` borrows and `HashMap`
/// probes per hit test.
///
/// What reddens it: binding the key to a local in `accepts_child_hit` —
/// `let floor = self.child_paint_key(child);` ahead of
/// `self.topmost_press_claimant(point)` — instead of asking for it inside the
/// `Some` arm. That form shipped in the tree once, left behind by a mutation
/// check the machine crashed in the middle of, and no test saw it.
#[test]
fn a_scene_with_no_claimant_never_probes_a_childs_key() {
    let (tree, view) = tree_with(200, false);
    let before = key_probes(&tree, view);
    for i in 0..SAMPLES {
        tree.hit_test(sample_point(i));
    }
    assert_eq!(
        key_probes(&tree, view) - before,
        0,
        "nothing here can veto a card, so the claimant query declines first and \
         no child's key is ever looked up"
    );
}

/// And with a claimant present the key *is* looked up — so the test above pins
/// an order rather than an unreachable branch.
#[test]
fn a_claimant_makes_the_veto_ask_for_the_childs_key() {
    let (tree, view) = tree_with(4, true);
    let before = key_probes(&tree, view);
    tree.hit_test(sample_point(0));
    assert!(
        key_probes(&tree, view) > before,
        "a press claimant is over these cards, so the veto has to compare each \
         one's key against it"
    );
}
