// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What does an off-screen **heavyweight card** cost?
//!
//! `pan_scaling_probe` asks the same question of the lightweight tier and
//! answers it in microseconds. This one asks it of the tier that has a
//! lifecycle — a real `Widget` in the arena, with focus, text state and
//! animations — and answers it in three currencies, because the defect it
//! pins (§1.7) is not really a speed defect:
//!
//! * **AccessKit nodes.** `A11yOffScreenMode::ViewportOnly` exists, in its own
//!   words, for "very large scenes where listing off-screen content would
//!   overwhelm AT clients". It never did that for the heavyweight tier: every
//!   materialised card reached the framework walker unfiltered.
//! * **Tab stops.** A card 90 000 px away was a keyboard trap sitting in the
//!   Tab ring between two visible ones.
//! * **Time**, for the AT walk and for one pan sample.
//!
//! The first two are *counts*, so they are exact and they are what the hard
//! gate asserts. The timings are the table you read when something feels slow.
//!
//! The on-screen population is held fixed across every row, so the off-screen
//! tail is the single independent variable.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test heavyweight_retention_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{Scene, SceneView};

const VIEWPORT: (f32, f32) = (800.0, 600.0);
/// Cards inside the viewport, held constant across every row.
const ONSCREEN: usize = 24;
const PASSES: u32 = 30;

/// A focusable leaf — the smallest thing that still costs an AT node and a Tab
/// stop, which is what this probe counts.
#[derive(Debug)]
struct Card;

impl Widget for Card {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(teksilo_core::widget_builder::HandlerSet::new().focusable(true));
        vec![]
    }
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(60.0, 40.0).into()
    }
}

/// `ONSCREEN` cards in the viewport plus `tail` cards parked a million units
/// away — past every region any `A11yOffScreenMode` produces.
fn scene_with(tail: usize) -> Scene {
    let mut scene = Scene::new();
    for i in 0..ONSCREEN {
        let x = (i % 8) as f32 * 90.0;
        let y = (i / 8) as f32 * 60.0;
        scene.add_widget(Card, Rect::new(x, y, 60.0, 40.0));
    }
    for i in 0..tail {
        let x = 1_000_000.0 + (i % 100) as f32 * 70.0;
        let y = 1_000_000.0 + (i / 100) as f32 * 50.0;
        scene.add_widget(Card, Rect::new(x, y, 60.0, 40.0));
    }
    scene
}

struct Sample {
    at_nodes: usize,
    tab_stops: usize,
    at_walk_ns: u128,
    pan_ns: u128,
}

fn measure(tail: usize) -> Sample {
    let mut tree = WidgetTree::new();
    let view = SceneView::new(scene_with(tail));
    let pan_x = view.pan_x_signal();
    let view_id = tree.add(view);
    let proposal = SizeProposal::exact(VIEWPORT.0, VIEWPORT.1);
    // Warm up: pay build + materialisation once. One pass is enough — the
    // parent's cull decision is read back inside the same layout, so nothing
    // here is waiting for a second one.
    tree.layout(proposal);

    let at_nodes = tree.accessibility_tree_snapshot().nodes.len();
    let tab_stops = tree.tab_stops_within(view_id).len();

    let mut walk = Vec::with_capacity(PASSES as usize);
    for _ in 0..PASSES {
        let t = Instant::now();
        let update = tree.accessibility_tree_snapshot();
        walk.push(t.elapsed().as_nanos());
        std::hint::black_box(update.nodes.len());
    }
    walk.sort_unstable();

    let mut pan = Vec::with_capacity(PASSES as usize);
    for i in 0..PASSES {
        pan_x.set(i as f32 * 0.5);
        let t = Instant::now();
        tree.layout(proposal);
        pan.push(t.elapsed().as_nanos());
    }
    pan.sort_unstable();

    Sample {
        at_nodes,
        tab_stops,
        at_walk_ns: walk[walk.len() / 2],
        pan_ns: pan[pan.len() / 2],
    }
}

#[test]
fn cost_of_an_off_screen_heavyweight_tail() {
    println!("\n  Cost of an off-screen HEAVYWEIGHT tail ({ONSCREEN} cards on screen)\n");
    println!("     tail |  total |  AT nodes | tab stops |   AT walk |  pan sample");
    println!("  --------+--------+-----------+-----------+-----------+------------");
    for tail in [0usize, 1_000, 20_000, 50_000] {
        let s = measure(tail);
        let total = ONSCREEN + tail;
        println!(
            "  {tail:>7} | {total:>6} | {:>9} | {:>9} | {:>7} µs | {:>8} µs",
            s.at_nodes,
            s.tab_stops,
            s.at_walk_ns / 1_000,
            s.pan_ns / 1_000,
        );
    }
    println!(
        "\n  Reading it: `AT nodes` and `tab stops` must not move at all — a card\n  \
         nobody can reach is not a card an assistive client should be offered,\n  \
         and a Tab stop 90 000 px away is a keyboard trap. The two timings\n  \
         follow from that: the walk has nothing extra to visit and the layout\n  \
         pass has nothing extra to recurse into.\n"
    );
}

/// The hard gate, and it is exact.
///
/// Counts rather than durations because the claim is about membership: the
/// defect was `ViewportOnly` publishing one AccessKit node and one Tab stop per
/// off-screen card, forever. A ratio would tolerate it; an equality does not.
#[test]
fn an_off_screen_card_costs_no_at_node_and_no_tab_stop() {
    let base = measure(0);
    for tail in [1usize, 1_000, 20_000] {
        let s = measure(tail);
        assert_eq!(
            s.at_nodes,
            base.at_nodes,
            "a tail of {tail} off-screen cards added {} AccessKit nodes — \
             `A11yOffScreenMode` is a no-op for the heavyweight tier again",
            s.at_nodes as i64 - base.at_nodes as i64,
        );
        assert_eq!(
            s.tab_stops,
            base.tab_stops,
            "a tail of {tail} off-screen cards added {} Tab stops — each one a \
             keyboard trap between two visible cards",
            s.tab_stops as i64 - base.tab_stops as i64,
        );
    }
}

/// The timing half, as a ratio, for the same reason its siblings are ratios:
/// the claim is structural ("the walk does not follow the model") and a CI
/// runner's absolute microseconds are not.
///
/// The defect measured **456×** at a 20 000-card tail (18 165 µs against
/// 40 µs). A residual remains and is deliberate: the framework walker tests
/// `arena.is_active` once per child of the `SceneView`, so it still touches —
/// but does not descend into, emit, or name — every parked card. That is
/// ~5 ns per card, and removing it would mean the arena keeping a second,
/// active-only child list per node. 10× leaves that residual room at every
/// scene size this probe covers while catching any return of the walk itself.
#[test]
fn the_accessibility_walk_does_not_cost_the_off_screen_tail() {
    let no_tail = measure(0).at_walk_ns;
    let long_tail = measure(20_000).at_walk_ns;
    let ratio = long_tail as f64 / no_tail.max(1) as f64;
    assert!(
        ratio < 10.0,
        "the accessibility walk took {long_tail} ns with a 20 000-card \
         off-screen tail against {no_tail} ns without it ({ratio:.1}×) — it is \
         walking cards nobody can reach again. The walk publishes the set the \
         layout pass recorded, not the model; see `SceneView::at_heavy`."
    );
}
