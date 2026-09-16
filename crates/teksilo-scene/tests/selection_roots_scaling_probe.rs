// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What does pruning a selection to its roots cost, and does that cost follow
//! the selection or the *square* of it?
//!
//! One printed table and one hard gate, the shape `pan_scaling_probe.rs`
//! already uses in this crate.
//!
//! `Scene::selection_roots` is not a cold path. Every selection-transform
//! recompute runs it (once, plus one `transformable_roots` per operation), so
//! it is reached whenever the model or the selection moves — a marquee, a
//! commit, an animating item, a data re-source — and again from the item-drag
//! group and the keyboard nudge. Its complexity is what decides whether
//! select-all on a large board leaves the window usable.
//!
//! The defect this probe replaces scanned every other id per id and asked
//! `is_descendant_of` — O(n²·depth). It is measured here as the function it is,
//! not through a caller, deliberately: how *often* a path asks is the caller's
//! business and changes, and a gate that measured the two together would go
//! green on a caching change while the quadratic scan sat there waiting for the
//! next call site. One call:
//!
//! | selected | per-id scan | one pass |
//! | ---: | ---: | ---: |
//! | 100 | 151 µs | 4.0 µs |
//! | 1 000 | 7.03 ms | 36.3 µs |
//! | 5 000 | 177.87 ms | 180.2 µs |
//! | 10 000 | 709.96 ms | 388.5 µs |
//!
//! Four of those per recompute — which is what a transform chrome pass costs —
//! is most of a second at 5 000 selected, against 0.7 ms now.
//!
//! Run with:
//!   cargo test -p teksilo-scene --test selection_roots_scaling_probe --release -- --nocapture

use std::time::Instant;

use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ItemId, RectItem, Scene};

const PASSES: u32 = 40;

/// A flat scene of `n` items, all of them selected. The worst case for the
/// quadratic form and the common case in an app: a marquee selects siblings.
fn flat(n: usize) -> (Scene, Vec<ItemId>) {
    let mut scene = Scene::new();
    let ids: Vec<ItemId> = (0..n)
        .map(|i| {
            scene.add_item(
                RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
                Point::new((i % 100) as f32 * 30.0, (i / 100) as f32 * 30.0),
            )
        })
        .collect();
    (scene, ids)
}

/// `n` items in chains of `depth` — so pruning has real work to do and the
/// answer is one root per chain.
fn nested(n: usize, depth: usize) -> (Scene, Vec<ItemId>) {
    let mut scene = Scene::new();
    let mut ids = Vec::with_capacity(n);
    let mut parent: Option<ItemId> = None;
    for i in 0..n {
        let id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)), Point::ZERO);
        if i % depth != 0 {
            scene.set_item_parent(id, parent);
        }
        parent = Some(id);
        ids.push(id);
    }
    (scene, ids)
}

/// Median nanoseconds for one `selection_roots` call.
fn time_one_prune(scene: &Scene, ids: &[ItemId]) -> u128 {
    let mut samples = Vec::with_capacity(PASSES as usize);
    for _ in 0..PASSES {
        let t = Instant::now();
        let roots = std::hint::black_box(scene.selection_roots(ids));
        samples.push(t.elapsed().as_nanos());
        std::hint::black_box(roots.len());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn cost_of_one_selection_roots_call_by_selection_size() {
    println!("\n  Cost of ONE Scene::selection_roots call (median ns)\n");
    println!("  shape       | selected |      ns |  ns/item |  vs 1000");
    println!("  ------------+----------+---------+----------+---------");
    let mut base: Option<u128> = None;
    for n in [10usize, 100, 1_000, 5_000, 10_000] {
        let (scene, ids) = flat(n);
        let ns = time_one_prune(&scene, &ids);
        if n == 1_000 {
            base = Some(ns);
        }
        let per = ns as f64 / n as f64;
        let vs = base.map_or_else(
            || "-".to_string(),
            |b| format!("{:.2}x", ns as f64 / b.max(1) as f64),
        );
        println!("  flat        | {n:>8} | {ns:>7} | {per:>8.2} | {vs:>8}");
    }
    for (n, depth) in [(1_000usize, 8usize), (10_000, 8)] {
        let (scene, ids) = nested(n, depth);
        let ns = time_one_prune(&scene, &ids);
        let per = ns as f64 / n as f64;
        println!("  nested/{depth:<4} | {n:>8} | {ns:>7} | {per:>8.2} |        -");
    }
    println!(
        "\n  Reading it: `ns/item` is the number that must stay flat. One that\n  \
         climbs with the selection is a quadratic scan — and this runs on every\n  \
         pointer move, every paint and every accessibility walk.\n"
    );
}

/// The hard gate. A ratio rather than a duration, for the reason
/// `pan_scaling_probe.rs`'s sibling gives: an absolute nanosecond budget is a
/// statement about the machine, and this is a statement about the algorithm.
///
/// Ten times the items must cost about ten times as much, not a hundred times.
/// The quadratic form measured 7.03 ms at 1 000 and 709.96 ms at 10 000 — a
/// **100.9×** climb for a 10× selection, and it fails this gate at 100.6×. The
/// one-pass form measures 36 µs and 389 µs, a 10.7× climb. A 25× ceiling sits
/// far below the quadratic form and far above the linear one.
///
/// Delete the `HashSet` in `selection_roots` and put the per-id `ids.iter()`
/// scan back, and this reddens.
#[test]
fn selection_roots_is_linear_in_the_selection() {
    let (small_scene, small_ids) = flat(1_000);
    let (big_scene, big_ids) = flat(10_000);
    let small = time_one_prune(&small_scene, &small_ids).max(1);
    let big = time_one_prune(&big_scene, &big_ids);
    let ratio = big as f64 / small as f64;
    assert_eq!(
        small_ids.len(),
        small_scene.selection_roots(&small_ids).len()
    );
    assert!(
        ratio < 25.0,
        "selection_roots took {big} ns for 10 000 selected items against {small} ns \
         for 1 000 ({ratio:.1}× for 10× the items) — it is scanning the selection \
         per item again. One pass over the ids asking a HashSet about each \
         ancestor is O(n·depth); the per-id scan is O(n²·depth), and this runs on \
         the free hover path."
    );
}

/// Pruning must still mean what it meant: order preserved, descendants of a
/// selected ancestor dropped, a lone id kept.
#[test]
fn selection_roots_keeps_the_meaning_it_had() {
    let mut scene = Scene::new();
    let r = || RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0));
    let root = scene.add_item(r(), Point::ZERO);
    let child = scene.add_item(r(), Point::ZERO);
    let grandchild = scene.add_item(r(), Point::ZERO);
    let sibling = scene.add_item(r(), Point::ZERO);
    scene.set_item_parent(child, Some(root));
    scene.set_item_parent(grandchild, Some(child));

    // A whole chain collapses to its root, whatever order it is offered in.
    assert_eq!(
        scene.selection_roots(&[grandchild, child, root]),
        vec![root]
    );
    // Order is the caller's, not the tree's.
    assert_eq!(
        scene.selection_roots(&[sibling, root, grandchild]),
        vec![sibling, root]
    );
    // A grandchild whose *grandparent* is selected is still pruned — the walk
    // is the whole ancestor chain, not just the parent.
    assert_eq!(scene.selection_roots(&[root, grandchild]), vec![root]);
    // A child alone is its own root.
    assert_eq!(scene.selection_roots(&[grandchild]), vec![grandchild]);
    // Nothing prunes a duplicate of itself.
    assert_eq!(scene.selection_roots(&[root, root]), vec![root, root]);
    // An empty set stays empty.
    assert!(scene.selection_roots(&[]).is_empty());
}
