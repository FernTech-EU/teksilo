// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What does one **pan** sample cost, and does that cost follow the viewport or
//! the whole scene?
//!
//! One printed table and one hard gate. The table is the measurement to read
//! when something feels slow; the gate
//! ([`a_pan_does_not_cost_the_off_screen_tail`]) is what stops the cost
//! returning.
//!
//! A pan touches no item. It writes `pan_x` / `pan_y`, which `SceneView` binds
//! at `BindingLevel::Relayout`, so every sample runs a full layout pass. The
//! on-screen population is held **fixed** at [`ONSCREEN`] items across the tail
//! rows, so the off-screen tail is the single independent variable: a cost that
//! climbs with it is cost spent on items nobody can see.
//!
//! The first rows vary the *visible* population instead, down to an empty
//! scene. That empty row is the floor — the framework's own cost for a layout
//! pass over a one-node tree — and the claim this probe exists to check is that
//! a pan over 50 000 items does not measurably exceed it.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test pan_scaling_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{RectItem, SceneModel, SceneView};

const VIEWPORT: (f32, f32) = (800.0, 600.0);
/// On-screen population held constant while the tail varies. Chosen to fill the
/// viewport at the grid spacing below.
const ONSCREEN: usize = 540;
const PASSES: u32 = 60;

/// `onscreen` items inside the viewport plus `tail` items parked a million
/// units away, so the two populations can be varied independently.
fn scene_with(onscreen: usize, tail: usize) -> SceneModel {
    let model = SceneModel::new();
    for i in 0..onscreen {
        let x = (i % 30) as f32 * 26.0;
        let y = (i / 30) as f32 * 32.0;
        model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(x, y),
        );
    }
    for i in 0..tail {
        let x = 1_000_000.0 + (i % 100) as f32 * 30.0;
        let y = 1_000_000.0 + (i / 100) as f32 * 30.0;
        model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(x, y),
        );
    }
    model
}

/// Median nanoseconds for one pan sample: write `pan_x`, run the layout pass.
fn time_one_pan_sample(onscreen: usize, tail: usize) -> u128 {
    let model = scene_with(onscreen, tail);
    let mut tree = WidgetTree::new();
    let view = SceneView::with_model(model.clone());
    let pan_x = view.pan_x_signal();
    let _view_id = tree.add(view);
    let proposal = SizeProposal::exact(VIEWPORT.0, VIEWPORT.1);
    tree.layout(proposal); // warm up: pay build + materialisation once

    let mut samples = Vec::with_capacity(PASSES as usize);
    for i in 0..PASSES {
        pan_x.set(i as f32 * 0.5);
        let t = Instant::now();
        tree.layout(proposal);
        samples.push(t.elapsed().as_nanos());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn cost_of_one_pan_sample_by_population() {
    println!("\n  Cost of ONE pan sample (median ns to set pan_x + relayout)\n");
    println!("  onscreen |   tail |  total |  pan sample |  vs empty");
    println!("  ---------+--------+--------+-------------+----------");
    let mut floor: Option<u128> = None;
    for (onscreen, tail) in [
        (0usize, 0usize),
        (1, 0),
        (100, 0),
        (ONSCREEN, 0),
        (ONSCREEN, 460),
        (ONSCREEN, 19_460),
        (ONSCREEN, 49_460),
    ] {
        let ns = time_one_pan_sample(onscreen, tail);
        let total = onscreen + tail;
        let ratio = match floor {
            Some(f) => format!("{:.2}x", ns as f64 / f as f64),
            None => "1.00x".to_string(),
        };
        floor.get_or_insert(ns);
        println!("  {onscreen:>8} | {tail:>6} | {total:>6} | {ns:>11} | {ratio:>9}");
    }
    println!(
        "\n  Reading it: the first row is an EMPTY scene — the framework's own cost\n  \
         for one layout pass. Every later row should sit on it. A number that\n  \
         climbs with `total` is cost spent on items outside the viewport.\n"
    );
}

/// The hard gate. See the sibling in `mutation_scaling_probe.rs` for why this
/// is a ratio rather than a duration.
///
/// The visible population is identical in both measurements, so the only
/// difference between them is 49 000 items nobody can see. The defect this
/// replaces measured **52×** (306 µs with no tail against 16 095 µs with a
/// 49 460-item one); a 3× ceiling is far enough below that to be unambiguous
/// and far enough above the floor (~1.1 µs, the framework's own cost for a
/// layout pass) to be quiet.
///
/// The exact, timing-free half of this claim lives in
/// `view::tests::hit_snapshot_cache`, which asserts in *branches taken* that a
/// pan reuses the snapshots. This test covers what that one cannot: the rest of
/// the pass — `place_children`'s cull in particular, which used to enumerate
/// every visible item through the spatial index on a view with no children at
/// all.
#[test]
fn a_pan_does_not_cost_the_off_screen_tail() {
    let no_tail = time_one_pan_sample(ONSCREEN, 0);
    let long_tail = time_one_pan_sample(ONSCREEN, 49_460);
    let ratio = long_tail as f64 / no_tail.max(1) as f64;
    assert!(
        ratio < 3.0,
        "one pan sample took {long_tail} ns with a 49 460-item off-screen tail \
         against {no_tail} ns without it ({ratio:.1}×) — the pass is paying for \
         items outside the viewport again. A pan emits no `ItemChange`, so the \
         hit snapshots must be reused whole; see `view::hit_snapshot`."
    );
}
