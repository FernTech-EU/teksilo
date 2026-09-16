// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What does one **transform** sample cost, and does that cost follow the
//! selection or the whole scene?
//!
//! The §3.4 spike's central objection to a live preview was that every sample
//! would also re-run two O(N) hit-snapshot rebuilds — 2.9 ms at 20 000 items —
//! so a per-sample relayout was unaffordable and the heavyweight tier would
//! have to preview as a ghost. The snapshots are now built once and invalidated
//! from the change stream, so that argument has to be re-measured rather than
//! inherited. This is the measurement.
//!
//! A transform sample writes no item. It bumps the controller's tick, which
//! `SceneView` binds at `BindingLevel::Relayout`, so each sample runs a layout
//! pass with the preview affine applied to the selection — the same shape a pan
//! sample has, plus the preview.
//!
//! One printed table and one hard gate. The table is the measurement to read
//! when something feels slow; the gate
//! ([`a_transform_sample_does_not_cost_the_unselected_scene`]) is what stops the
//! cost arriving.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test transform_scaling_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{
    ItemFlags, RectItem, SceneModel, SceneSelection, SceneSelectionMode, SceneView, TransformConfig,
};

const VIEWPORT: (f32, f32) = (800.0, 600.0);
const PASSES: u32 = 300;

/// `selected` draggable items in the top-leading corner, plus `rest` items
/// filling the scene around them, so the two populations vary independently.
fn scene_with(selected: usize, rest: usize) -> (SceneModel, SceneSelection) {
    let model = SceneModel::new();
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    let mut ids = Vec::with_capacity(selected);
    for i in 0..selected {
        let x = (i % 20) as f32 * 12.0;
        let y = (i / 20) as f32 * 12.0;
        let id = model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(x, y),
        );
        model.set_flag(id, ItemFlags::IS_DRAGGABLE, true);
        ids.push(id);
    }
    for i in 0..rest {
        let x = 300.0 + (i % 100) as f32 * 7.0;
        let y = 300.0 + (i / 100) as f32 * 7.0;
        model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 6.0, 6.0)),
            Point::new(x, y),
        );
    }
    selection.replace(ids);
    (model, selection)
}

/// Median nanoseconds for the two halves of one pointer sample of a live group
/// move: `(dispatch, relayout)`.
///
/// Split because they are two different costs with two different owners. The
/// **relayout** is what the controller added — the tick is bound at `Relayout`,
/// so each sample re-runs the pass that re-derives the preview. The
/// **dispatch** is the view's pre-existing per-move item hit test, which walks
/// the handler snapshot; it is the same cost a plain hover pays on a scene of
/// the same size, with or without a controller.
fn time_one_transform_sample(selected: usize, rest: usize) -> (u128, u128) {
    let (model, selection) = scene_with(selected, rest);
    let mut tree = WidgetTree::new();
    let view = SceneView::with_model(model)
        .selection_model(selection)
        .transform_controller(TransformConfig::new());
    let _view_id = tree.add(view);
    let proposal = SizeProposal::exact(VIEWPORT.0, VIEWPORT.1);
    tree.layout(proposal); // warm up: pay build + materialisation once

    // Grab the first item's body and latch the drag.
    tree.pointer_move(Point::new(5.0, 5.0));
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(5.0, 5.0),
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(30.0, 5.0)));
    tree.layout(proposal);

    let mut dispatch = Vec::with_capacity(PASSES as usize);
    let mut relayout = Vec::with_capacity(PASSES as usize);
    for i in 0..PASSES {
        let x = 40.0 + i as f32 * 0.5;
        let t = Instant::now();
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(x, 5.0)));
        dispatch.push(t.elapsed().as_nanos());
        let t = Instant::now();
        tree.layout(proposal);
        relayout.push(t.elapsed().as_nanos());
    }
    dispatch.sort_unstable();
    relayout.sort_unstable();
    (dispatch[dispatch.len() / 2], relayout[relayout.len() / 2])
}

#[test]
fn cost_of_one_transform_sample_by_population() {
    println!("\n  Cost of ONE group-move sample (median ns)\n");
    println!("  selected |   rest |  total |  relayout |  vs floor |   dispatch");
    println!("  ---------+--------+--------+-----------+-----------+-----------");
    let mut floor: Option<u128> = None;
    for (selected, rest) in [
        (1usize, 0usize),
        (10, 0),
        (10, 990),
        (10, 19_990),
        (100, 19_900),
        (1_000, 19_000),
    ] {
        let (dispatch, relayout) = time_one_transform_sample(selected, rest);
        let total = selected + rest;
        let ratio = match floor {
            Some(f) => format!("{:.2}x", relayout as f64 / f as f64),
            None => "1.00x".to_string(),
        };
        floor.get_or_insert(relayout);
        println!(
            "  {selected:>8} | {rest:>6} | {total:>6} | {relayout:>9} | {ratio:>9} | {dispatch:>9}"
        );
    }
    println!(
        "\n  Reading it. **relayout** is the cost the controller added: the tick is\n  \
         bound at `Relayout`, so a sample re-runs the pass that re-derives the\n  \
         preview. Rows 3-4 hold the selection fixed and grow the scene around it,\n  \
         which is what a sample pays for items nobody is dragging; rows 5-6 grow\n  \
         the selection instead, which is the gesture's own cost.\n\n  \
         **dispatch** is the view's pre-existing per-move item hit test. It walks\n  \
         the handler snapshot and is paid by a plain hover over a scene of the\n  \
         same size, controller or not — it is in the table so it is not mistaken\n  \
         for the controller's.\n"
    );
}

/// The same scene and the same drag with **no controller installed** — the
/// control the `dispatch` column has to be read against.
///
/// Deliberately a *drag*, not a hover: a captured pointer takes a different
/// route through the router than a free one, so comparing a controller's drag
/// against a plain hover would measure the capture rather than the controller.
/// The first selected item carries `IS_DRAGGABLE`, so this is the view's
/// pre-existing single-item drag over the identical scene.
fn time_one_plain_drag_sample(selected: usize, rest: usize) -> u128 {
    let (model, selection) = scene_with(selected, rest);
    let mut tree = WidgetTree::new();
    let view = SceneView::with_model(model).selection_model(selection);
    let _view_id = tree.add(view);
    let proposal = SizeProposal::exact(VIEWPORT.0, VIEWPORT.1);
    tree.layout(proposal);

    tree.pointer_move(Point::new(5.0, 5.0));
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(5.0, 5.0),
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(30.0, 5.0)));
    tree.layout(proposal);

    let mut samples = Vec::with_capacity(PASSES as usize);
    for i in 0..PASSES {
        let x = 40.0 + i as f32 * 0.5;
        let t = Instant::now();
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(x, 5.0)));
        samples.push(t.elapsed().as_nanos());
        tree.layout(proposal);
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// The `dispatch` column's scaling belongs to the view's pre-existing per-move
/// item hit test, not to the controller — and this is what says so rather than
/// asserting it in a comment. Same scene, same drag, controller against none.
#[test]
fn the_controller_does_not_add_the_per_move_dispatch_cost() {
    let (with_controller, _) = time_one_transform_sample(10, 19_990);
    let without = time_one_plain_drag_sample(10, 19_990);
    let ratio = with_controller as f64 / without.max(1) as f64;
    // The same 3× shape the sibling gates use, and for the same reason: both
    // arms allocate a 20 000-item scene, so the medians drift by tens of
    // percent between runs and a tight bound would be a flake rather than a
    // measurement. 3× still catches anything that walks the scene per sample.
    assert!(
        ratio < 3.0,
        "a pointer sample dispatched in {with_controller} ns with a transform \
         controller against {without} ns without one ({ratio:.1}×) on the same \
         20 000-item scene — the controller is adding per-move work it should \
         not. Its own per-sample cost is the relayout, which the sibling gate \
         measures."
    );
}

/// The hard gate, and the one the spike's cost objection turns on: a sample of a
/// small group move must not pay for the scene it is not moving.
///
/// The selection is identical in both measurements, so the only difference is
/// 19 000 items nobody is dragging. A 3× ceiling is the same shape
/// `pan_scaling_probe` uses and for the same reason: far enough above the floor
/// to be quiet, far enough below a per-sample O(N) rebuild to be unambiguous.
#[test]
fn a_transform_sample_does_not_cost_the_unselected_scene() {
    let (_, small) = time_one_transform_sample(10, 990);
    let (_, large) = time_one_transform_sample(10, 19_990);
    let ratio = large as f64 / small.max(1) as f64;
    assert!(
        ratio < 3.0,
        "the relayout half of one group-move sample took {large} ns in a \
         20 000-item scene against {small} ns in a 1 000-item one ({ratio:.1}×) \
         — the pass is paying for items the gesture is not moving. A transform \
         sample writes no item, so the hit snapshots must be reused whole; see \
         `view::hit_snapshot`."
    );
}
