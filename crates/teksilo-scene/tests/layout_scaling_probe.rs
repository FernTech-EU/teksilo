// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! PROBE (measurement, not a regression test): what does it cost to move **one**
//! item in a scene of N — the drag path — and does that cost scale with the
//! viewport or with the whole scene?
//!
//! `SceneView::layout_response_impl` calls `refresh_hit_snapshots` on every layout
//! pass, which brings two snapshots up to date: the draggable-item snapshot and
//! the handler-dispatch snapshot. It does **not** rebuild them from `scene.ids()`
//! every time — `hit_snapshot` plans `Reuse` (nothing was emitted, the pan case:
//! one `u64` comparison), `Patch` (a geometry change: the named ids and their
//! subtrees rewritten in place by binary search over the `PaintKey` sort), or
//! `Rebuild` (a structural change: the walk over **every** item rather than the
//! visible ones, an allocation per item that carries handlers — a handler set is
//! cloned and boxed — and a sort by z, so O(N log N)). Moving one item, which is
//! what this probe does, emits `LocalPosChanged` and therefore takes the `Patch`
//! path; what the table below measures is the rest of the pass plus that patch,
//! not a rebuild. (Publishing the item's geometry used to cost two more: the old
//! `clone_shape_test()` boxed a closure and `.into()` re-allocated it into an
//! `Rc`. `SceneItem::shape` returns a value whose default variant allocates
//! nothing and whose path variant is a refcount bump, so those are gone too.)
//!
//! Dragging one card across a scene relayouts on every pointer sample, so this
//! is the per-sample cost. The `offscreen` column repeats the measurement with
//! every item parked far outside the viewport: if the two columns match, nothing
//! about the pass is viewport-conditioned and the spatial index is not helping
//! here.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test layout_scaling_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{ItemId, RectItem, SceneModel, SceneView};

const VIEWPORT: (f32, f32) = (800.0, 600.0);
const PASSES: u32 = 30;

/// A scene of `n` small rect items. `offscreen` parks them all far outside the
/// viewport so any viewport-conditioned work would drop out of the measurement.
fn scene_with(n: usize, offscreen: bool) -> (SceneModel, ItemId) {
    let model = SceneModel::new();
    let base = if offscreen { 1_000_000.0 } else { 0.0 };
    let mut first = None;
    for i in 0..n {
        let x = base + (i % 100) as f32 * 30.0;
        let y = base + (i / 100) as f32 * 30.0;
        let id = model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(x, y),
        );
        first.get_or_insert(id);
    }
    (model, first.expect("n > 0"))
}

/// Median nanoseconds to move one item and relayout — one pointer sample of a drag.
fn time_one_item_move(n: usize, offscreen: bool) -> u128 {
    let (model, dragged) = scene_with(n, offscreen);
    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::with_model(model.clone()));
    let proposal = SizeProposal::exact(VIEWPORT.0, VIEWPORT.1);
    tree.layout(proposal); // warm up: pay build + materialisation once

    let start = model.local_pos(dragged).unwrap_or(Point::ZERO);
    let mut samples = Vec::with_capacity(PASSES as usize);
    for i in 0..PASSES {
        // One pointer sample: nudge the dragged item, then relayout.
        model.set_local_pos(dragged, Point::new(start.x + i as f32, start.y));
        let t = Instant::now();
        tree.layout(proposal);
        samples.push(t.elapsed().as_nanos());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn cost_of_moving_one_item_by_scene_size() {
    println!("\n  Cost of ONE pointer sample of a drag, by scene size");
    println!("  (median ns to move one item + relayout)\n");
    println!("   items |   onscreen |  offscreen |  growth |  ns/item");
    println!("  -------+------------+------------+---------+---------");
    let mut prev: Option<(usize, u128)> = None;
    for n in [100usize, 1_000, 5_000, 20_000] {
        let on = time_one_item_move(n, false);
        let off = time_one_item_move(n, true);
        let growth = match prev {
            Some((pn, pv)) => format!("{:.1}x/{:.0}x", on as f64 / pv as f64, n as f64 / pn as f64),
            None => "-".to_string(),
        };
        println!(
            "  {n:>6} | {on:>10} | {off:>10} | {growth:>7} | {:>7.1}",
            on as f64 / n as f64
        );
        prev = Some((n, on));
    }
    println!(
        "\n  Reading it: a flat ns/item column means the pass is LINEAR in total\n  \
         scene size. Matching onscreen/offscreen columns mean the pass is NOT\n  \
         viewport-conditioned — an entirely off-screen scene costs full price.\n"
    );
}
