// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What does moving **one** item cost at the `Scene` level, with no view and no
//! layout pass in the way?
//!
//! One printed table and one hard gate
//! ([`moving_one_leaf_does_not_cost_the_scene`]).
//!
//! `Scene::set_local_pos` re-buckets the moved item's subtree in the spatial
//! index, because every descendant's scene-AABB shifts with it. `set_z` changes
//! no geometry and re-buckets nothing, so it is the control: the two differ by
//! exactly the subtree walk. A `set_local_pos` column that climbs with scene size
//! while the `set_z` column stays flat means the walk is costing the whole model
//! to move one leaf.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test mutation_scaling_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ItemId, RectItem, Scene};

const CALLS: u32 = 2_000;

/// A flat scene of `n` root-level rect items; the id returned is the one the
/// probe moves (a leaf with no descendants — the cheapest possible subtree).
fn flat_scene(n: usize) -> (Scene, ItemId) {
    let mut scene = Scene::new();
    let mut first = None;
    for i in 0..n {
        let x = (i % 100) as f32 * 30.0;
        let y = (i / 100) as f32 * 30.0;
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(x, y),
        );
        first.get_or_insert(id);
    }
    (scene, first.expect("n > 0"))
}

/// Median nanoseconds for one `set_local_pos` on a leaf.
fn time_set_local_pos(n: usize) -> u128 {
    let (mut scene, id) = flat_scene(n);
    let mut samples = Vec::with_capacity(CALLS as usize);
    for i in 0..CALLS {
        let p = Point::new(i as f32, 0.0);
        let t = Instant::now();
        scene.set_local_pos(id, p);
        samples.push(t.elapsed().as_nanos());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Median nanoseconds for one `set_z` on the same leaf — the control, since
/// `set_z` touches no geometry and therefore re-buckets nothing.
fn time_set_z(n: usize) -> u128 {
    let (mut scene, id) = flat_scene(n);
    let mut samples = Vec::with_capacity(CALLS as usize);
    for i in 0..CALLS {
        let z = i as f32;
        let t = Instant::now();
        scene.set_z(id, z);
        samples.push(t.elapsed().as_nanos());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn cost_of_moving_one_leaf_by_scene_size() {
    println!("\n  Cost of ONE Scene mutation on a leaf, by scene size");
    println!("  (median ns per call; set_z is the no-rebucket control)\n");
    println!("   items | set_local_pos |     set_z |  ratio");
    println!("  -------+---------------+-----------+-------");
    for n in [1_000usize, 10_000, 20_000, 50_000] {
        let pos = time_set_local_pos(n);
        let z = time_set_z(n);
        let ratio = if z == 0 {
            "-".to_string()
        } else {
            format!("{:.1}x", pos as f64 / z as f64)
        };
        println!("  {n:>6} | {pos:>13} | {z:>9} | {ratio:>6}");
    }
    println!(
        "\n  Reading it: `set_local_pos` should cost the moved subtree, not the model.\n  \
         A column that climbs with `items` while `set_z` stays flat is the\n  \
         whole-scene adjacency rebuild showing up.\n"
    );
}

/// The hard gate, and the reason this file is not only a printout.
///
/// A **ratio**, not a duration: the claim is structural — "moving one leaf
/// costs the subtree, not the model" — and a ratio survives a CI runner whose
/// absolute microseconds swing, because both measurements are medians taken
/// back to back in the same process. The threshold is deliberately loose: the
/// defect this replaces measured **47×** (1 432 ns at 1 000 items against
/// 67 578 ns at 50 000), so anything under 4× cannot be it coming back, and
/// anything over 4× is not noise.
#[test]
fn moving_one_leaf_does_not_cost_the_scene() {
    let small = time_set_local_pos(1_000);
    let large = time_set_local_pos(50_000);
    let ratio = large as f64 / small.max(1) as f64;
    assert!(
        ratio < 4.0,
        "moving one leaf in a 50 000-item scene took {large} ns against \
         {small} ns in a 1 000-item scene ({ratio:.1}×) — `set_local_pos` is \
         walking the whole model again. The kept `SceneEntry::children` \
         adjacency is what stops it; check that every parent-pointer write \
         still maintains it."
    );
}
