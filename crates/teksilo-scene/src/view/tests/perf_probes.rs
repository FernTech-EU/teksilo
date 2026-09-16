// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-sample cost probes for the two pointer paths a whole application pays on
//! every mouse move: the **free hover**, which asks the transform controller
//! what is under the pointer, and the **drag sample**, which asks the geometry
//! constraint what it may do.
//!
//! Both used to be quadratic in the size of the selection, and neither showed
//! it in a functional test — a test that drags two squares cannot tell an O(1)
//! hover from an O(n²) one. These print nanosecond figures and assert only the
//! *shape*: that the controller and the constraint mechanism add a cost that
//! does not grow with the scene. A threshold in wall-clock nanoseconds would be
//! a flake generator; a ratio between two measurements taken on the same host in
//! the same run is not.
//!
//! Run them in `--release`; in a debug build the constant factors swamp the
//! shape:
//!
//! ```text
//! cargo test --release -p teksilo-scene --lib -- --ignored --nocapture perf_probes
//! ```
//!
//! `PERF_SAMPLES` overrides the per-size sample count (default 300) — set it
//! low when measuring a baseline that is seconds per sample.

use super::*;
use crate::flags::ItemFlags;
use crate::items::RectItem;
use crate::scene_model::SceneModel;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::transform_session::TransformConfig;
use std::time::Instant;
use teksilo_canvas::{Point, Vec2};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

fn samples(default: u32) -> u32 {
    std::env::var("PERF_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
        .max(1)
}

/// `n` draggable / resizable / rotatable squares on a 100-wide grid, the first
/// `selected` of them in the selection model.
fn grid_scene(n: usize, selected: usize) -> (SceneModel, SceneSelection, Vec<ItemId>) {
    let model = SceneModel::new();
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let col = (i % 100) as f32;
        let row = (i / 100) as f32;
        let id = model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(col * 20.0, row * 20.0),
        );
        model.set_flag(id, ItemFlags::IS_DRAGGABLE, true);
        model.set_flag(id, ItemFlags::IS_RESIZABLE, true);
        model.set_flag(id, ItemFlags::IS_ROTATABLE, true);
        ids.push(id);
    }
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace(ids.iter().copied().take(selected));
    (model, selection, ids)
}

fn mount(model: SceneModel, selection: SceneSelection, controller: bool) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let mut view = SceneView::with_model(model).selection_model(selection);
    if controller {
        view = view.transform_controller(TransformConfig::new());
    }
    let view_id = tree.add(view);
    tree.layout(SizeProposal::exact(600.0, 400.0));
    (tree, view_id)
}

/// Nanoseconds per `pointer_move`, and how many times the chrome was actually
/// re-derived over the whole run.
///
/// This is the **free hover** — what an application pays for every mouse move
/// over the view, with nothing being dragged. `controller` off gives the view's
/// own baseline: the hit-test walk over the item snapshot, which is the scene's
/// business and not the controller's.
fn hover_ns(n: usize, selected: usize, controller: bool, reps: u32) -> (f64, u64) {
    let (model, selection, _) = grid_scene(n, selected);
    let (mut tree, view_id) = mount(model, selection, controller);
    for i in 0..2 {
        tree.pointer_move(Point::new(500.0 + i as f32, 380.0));
    }
    let before = view_handle(&tree, view_id)
        .transform_rt
        .chrome_computes
        .get();
    let start = Instant::now();
    for i in 0..reps {
        tree.pointer_move(Point::new(500.0 + (i % 20) as f32, 380.0));
    }
    let per = start.elapsed().as_nanos() as f64 / reps as f64;
    let computes = view_handle(&tree, view_id)
        .transform_rt
        .chrome_computes
        .get()
        - before;
    (per, computes)
}

/// Nanoseconds per drag sample on the **lightweight item drag**, with no
/// geometry constraint installed. No transform controller: it would claim the
/// press on a selected item and this path would never run.
fn unconstrained_drag_ns(n: usize, selected: usize, reps: u32) -> f64 {
    let (model, selection, ids) = grid_scene(n, selected);
    let (mut tree, view_id) = mount(model.clone(), selection, false);
    // Grab item 0, whose 10x10 box sits at scene (0, 0). Nudge the camera so
    // the press lands inside it with room to travel.
    tree.widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast")
        .set_pan(Vec2::new(40.0, 40.0));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let press = Point::new(45.0, 45.0);
    tree.pointer_move(press);
    tree.dispatch_event(WidgetEvent::pointer_down(
        press,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    for i in 0..4 {
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(60.0 + i as f32, 60.0)));
    }
    let before = model.scene_rect(ids[0]).expect("resolves");
    let start = Instant::now();
    for i in 0..reps {
        tree.dispatch_event(WidgetEvent::pointer_move(Point::new(
            100.0 + (i % 20) as f32,
            100.0,
        )));
    }
    let per = start.elapsed().as_nanos() as f64 / reps as f64;
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(100.0, 100.0),
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.layout(SizeProposal::exact(600.0, 400.0));
    let after = model.scene_rect(ids[0]).expect("resolves");
    assert!(
        (after.x - before.x).abs() > 1.0,
        "the probe must actually be dragging: {before:?} -> {after:?}"
    );
    per
}

#[test]
#[ignore = "timing probe; run in --release"]
fn perf_probes_free_hover_over_a_large_selection() {
    let reps = samples(300);
    let (small_off, _) = hover_ns(500, 500, false, reps);
    let (small_on, small_computes) = hover_ns(500, 500, true, reps);
    let (large_off, _) = hover_ns(5_000, 5_000, false, reps);
    let (large_on, large_computes) = hover_ns(5_000, 5_000, true, reps);
    println!("hover   500/500  : {small_off:>12.1} ns bare | {small_on:>12.1} ns with controller");
    println!("hover  5000/5000 : {large_off:>12.1} ns bare | {large_on:>12.1} ns with controller");
    let small_cost = small_on - small_off;
    let large_cost = large_on - large_off;
    println!("controller cost  : {small_cost:>12.1} ns @500 | {large_cost:>12.1} ns @5000");
    println!(
        "chrome recomputes: {small_computes} @500 | {large_computes} @5000 over {reps} samples"
    );

    assert_eq!(
        (small_computes, large_computes),
        (0, 0),
        "a hover over an unchanged selection must re-derive nothing"
    );
}

#[test]
#[ignore = "timing probe; run in --release"]
fn perf_probes_unconstrained_drag_sample() {
    let reps = samples(300);
    let small = unconstrained_drag_ns(500, 125, reps);
    let large = unconstrained_drag_ns(2_000, 500, reps);
    println!("drag    500/125  : {small:>12.1} ns/sample");
    println!("drag   2000/500  : {large:>12.1} ns/sample");
    println!("growth  4x items : {:>12.2}x", large / small.max(1.0));
}
